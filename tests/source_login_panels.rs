use reader_rust::{
    api::protocol::build_source_panel_page,
    crawler::http_client::HttpClient,
    model::book_source::{book_source_from_value, BookSource},
    parser::{js::eval_js, rule_engine::RuleEngine},
    service::{
        book_service::{get_source_panel, remove_source_panel, BookService, ShowBrowserRequest},
        source_browser::{cancel_browser, complete_browser, BrowserHandler, BrowserPage},
    },
    storage::cache::file_cache::FileCache,
};
use serde_json::{json, Value};
use std::{path::PathBuf, sync::Arc, time::Duration};

fn service() -> (Arc<BookService>, PathBuf) {
    let root = std::env::temp_dir().join(format!("reader-browser-{}", uuid::Uuid::new_v4()));
    let service = BookService::new(HttpClient::new(5, None).unwrap(), RuleEngine::new().unwrap(), FileCache::new(root.join("cache")), root.to_str().unwrap());
    (Arc::new(service), root)
}

fn source() -> BookSource {
    book_source_from_value(json!({
        "bookSourceUrl":format!("聚合测试{}", uuid::Uuid::new_v4()), "bookSourceName":"面板测试",
        "loginUi":[{"name":"输入","type":"text"},{"name":"设置","type":"button","action":"configure()"}],
        "loginUrl":r#"
            function configure() {
                source.put('executions', String(Number(source.get('executions') || 0) + 1));
                var before = result['输入'];
                var body = java.startBrowserAwait('data:text/html,%3Cspan%20id="choice"%3Einitial%3C/span%3E', '设置线路', false).body();
                source.setVariable(before + '|' + body.match(/id="choice">([^<]*)</)[1]);
                java.upLoginData({'输入': '已保存'});
            }
        "#
    })).unwrap()
}

fn read(service: &BookService, source: &BookSource, code: &str) -> String {
    service.source_scope("default", source, || eval_js(code, "", "")).unwrap().unwrap()
}

fn browser_channel() -> (BrowserHandler, tokio::sync::mpsc::UnboundedReceiver<ShowBrowserRequest>) {
    let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
    (Arc::new(move |request| sender.send(request).map_err(|e| e.to_string())), receiver)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn await_keeps_the_original_stack_until_the_document_is_confirmed() {
    let (service, root) = service();
    let source = source();
    read(&service, &source, "source.setVariable('original')");
    let (handler, mut events) = browser_channel();
    let task = {
        let service = service.clone(); let source = source.clone();
        tokio::spawn(async move { service.run_source_login_with_browser(&source, Some("button:1".into()), Some(serde_json::Map::from_iter([("输入".into(), json!("user-input"))])), Some(handler)).await })
    };
    let request = tokio::time::timeout(Duration::from_secs(10), events.recv()).await.unwrap().unwrap();
    let id = request.panel_id.unwrap();
    assert!(request.await_result);
    assert_eq!(request.title.as_deref(), Some("设置线路"));
    assert!(get_source_panel(&id).unwrap().html.contains("initial"));
    assert!(!task.is_finished());
    assert_eq!(read(&service, &source, "source.getVariable()"), "original");
    assert!(complete_browser(&id, BrowserPage { url: String::new(), body: String::new() }).is_err());
    assert!(!task.is_finished());
    complete_browser(&id, BrowserPage { url: "data:text/html,fixture".into(), body: "<span id=\"choice\">selected</span>".into() }).unwrap();
    let view = tokio::time::timeout(Duration::from_secs(10), task).await.unwrap().unwrap().unwrap();
    assert_eq!(view["values"]["输入"], "已保存");
    assert_eq!(read(&service, &source, "source.getVariable()"), "user-input|selected");
    assert_eq!(read(&service, &source, "source.get('executions')"), "1");
    assert!(get_source_panel(&id).is_none());
    drop(service);
    let restored = BookService::new(HttpClient::new(5, None).unwrap(), RuleEngine::new().unwrap(), FileCache::new(root.join("cache")), root.to_str().unwrap());
    assert_eq!(read(&restored, &source, "source.getVariable()"), "user-input|selected");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cancelling_a_page_does_not_apply_empty_defaults_and_can_be_reopened() {
    let (service, _) = service();
    let mut source = source();
    source.login_url = Some(format!("{}\nfunction configureSafely() {{ try {{ configure(); }} catch(e) {{ java.toast('cancelled'); }} }}", source.login_url.unwrap()));
    source.login_ui = Some(r#"[{"name":"输入"},{"name":"设置","type":"button","action":"configureSafely()"}]"#.into());
    read(&service, &source, "source.setVariable('keep-me')");
    for action in ["configure()", "configureSafely()"] {
        source.login_ui = Some(json!([{"name":"输入"},{"name":"设置","type":"button","action":action}]).to_string());
        let (handler, mut events) = browser_channel();
        let task = {
            let service = service.clone(); let source = source.clone();
            tokio::spawn(async move { service.run_source_login_with_browser(&source, Some("button:1".into()), None, Some(handler)).await })
        };
        let request = tokio::time::timeout(Duration::from_secs(10), events.recv()).await.unwrap().unwrap();
        let id = request.panel_id.unwrap();
        cancel_browser(&id);
        let view = tokio::time::timeout(Duration::from_secs(10), task).await.unwrap().unwrap().unwrap();
        assert_eq!(view["cancelled"], true);
        assert_eq!(view["actionCompleted"], false);
        assert_eq!(read(&service, &source, "source.getVariable()"), "keep-me");
        assert!(get_source_panel(&id).is_none());
    }
}

#[tokio::test]
async fn show_browser_keeps_options_and_preloads_before_the_page_script() {
    let (service, _) = service();
    let mut source = source();
    let html = r#"<!DOCTYPE html><html><head><title>线路设置</title><script>window.pageSawPreload=window.fixturePreload;</script></head><body><label for="line">线路</label><input id="line" value="初始线路"><label for="note">备注</label><textarea id="note">初始备注</textarea><label for="theme">样式</label><select id="theme"><option value="light">浅色</option><option value="dark">深色</option></select><span id="choice">initial</span><button onclick="java.upConfig(JSON.stringify({heightPercentage:0.6}))">调整高度</button><button onclick="java.showBrowser('https://fixture.test/nested','<title>子面板</title><button onclick=&quot;window.close()&quot;>完成</button>',null,{heightPercentage:0.5})">打开子面板</button></body></html>"#;
    source.login_url = Some(format!("function configure() {{ java.showBrowser('https://fixture.test/settings', {}, {}, JSON.stringify({{heightPercentage:0.8, isDraggable:false, expandedCornersRadius:20, dismissOnTouchOutside:false}})); }}", json!(html), json!("window.fixturePreload='ready'; window.scriptLiteral='</script>';")));
    let view = service.run_source_login(&source, Some("button:1".into()), None).await.unwrap();
    let request: ShowBrowserRequest = serde_json::from_value(view["showBrowser"].clone()).unwrap();
    assert_eq!(request.options.as_ref().unwrap()["heightPercentage"], 0.8);
    let id = request.panel_id.unwrap();
    let page = build_source_panel_page(&id).unwrap();
    assert!(page.find("window.java =").unwrap() < page.find("window.fixturePreload='ready'").unwrap());
    assert!(page.find("window.fixturePreload='ready'").unwrap() < page.find("window.pageSawPreload").unwrap());
    assert!(page.contains("\\u003c/script>"));
    // The browser suite serves this actual backend-generated bridge and page.
    let fixture_dir = PathBuf::from("target/source-panel-fixtures");
    std::fs::create_dir_all(&fixture_dir).unwrap();
    std::fs::write(fixture_dir.join("settings.html"), page.replace(&id, "fixture-settings")).unwrap();
    remove_source_panel(&id);
}

#[tokio::test]
async fn login_controls_keep_defaults_and_apply_resets_and_button_labels() {
    let (service, _) = service();
    let source = book_source_from_value(json!({
        "bookSourceUrl":format!("控件测试{}", uuid::Uuid::new_v4()), "bookSourceName":"控件",
        "loginUi":[{"name":"选项","type":"select","chars":[null,"甲","乙"]},{"name":"密码","type":"password","default":"默认"},{"name":"操作","type":"button","action":"update()"},{"name":"说明","type":"button"}],
        "loginUrl":"function update(){java.upLoginData(null);java.upLoginData({'操作':'已更新','选项':'乙'});java.reLoginView();}"
    })).unwrap();
    let view = service.login_book_source(&source).await.unwrap();
    assert_eq!(view["values"]["选项"], "甲");
    let view = service.run_source_login(&source, Some("button:2".into()), Some(serde_json::from_value::<serde_json::Map<String, Value>>(json!({"选项":"甲","密码":"输入"})).unwrap())).await.unwrap();
    assert_eq!(view["values"]["密码"], "默认");
    assert_eq!(view["values"]["选项"], "乙");
    assert_eq!(view["fields"][2]["viewName"], "已更新");
    service.run_source_login(&source, Some("button:3".into()), None).await.unwrap();
}

/// 带 `type: "label"` 的登录面板必须能打开。
///
/// legado 的 `rowUiBuilder` 对 `RowUi.Type` 是**没有 else 的 `when`**：不认识的
/// 类型既不渲染也不报错，其余控件照常。而 yckceo id 8033「西瓜书屋 v3」把最后
/// 一项写成 `{"name":"⚠️ 站点限频3秒/次…","type":"label"}`，早先后端对未知类型
/// 直接 `Err`，整个登录面板只显示「不支持的登录控件: label」。
#[tokio::test]
async fn label_controls_do_not_break_the_login_form() {
    let (service, _) = service();
    let source = book_source_from_value(json!({
        "bookSourceUrl":format!("label测试{}", uuid::Uuid::new_v4()), "bookSourceName":"label",
        "loginUi":r#"[
            {"name":"状态栏","type":"button","action":"notice()","viewName":"'当前：'+(source.getVariable()||source.getKey())"},
            {"name":"说明","type":"label"},
            {"name":"⚠️ 站点限频3秒/次；主站搜索过一次CF盾长期有效。","type":"label"},
            {"name":"账号","type":"text"}
        ]"#,
        "loginUrl":"function notice(){java.toast('ok')}"
    })).unwrap();
    let view = service.login_book_source(&source).await.unwrap();
    let fields = view["fields"].as_array().unwrap();
    assert_eq!(fields.len(), 4, "label 与其它控件都要保留: {fields:?}");
    assert_eq!(fields[1]["type"], "label");
    assert_eq!(fields[2]["type"], "label");
    // label 不参与取值 map：否则展示文字会被写进 source.putLoginInfo。
    assert!(view["values"].get("说明").is_none(), "label 不应进入 values: {}", view["values"]);
    assert!(view["values"].get("⚠️ 站点限频3秒/次；主站搜索过一次CF盾长期有效。").is_none());
    assert_eq!(view["values"]["账号"], "");
    // 动态 viewName 要按 JS 求值（legado 的 evalUiJs 语义），不能把源码当文字显示。
    assert_eq!(fields[0]["viewName"], format!("当前：{}", source.book_source_url));
    // 点按钮仍然可用（label 的存在不该影响 action 的下标解析）。
    let view = service.run_source_login(&source, Some("button:0".into()), None).await.unwrap();
    assert_eq!(view["messages"][0], "ok");
    assert_eq!(view["actionCompleted"], true);
}

/// 不认识的控件类型既不能整页失败，也不该混进取值 map。
#[tokio::test]
async fn unknown_control_types_are_skipped_instead_of_failing() {
    let (service, _) = service();
    let source = book_source_from_value(json!({
        "bookSourceUrl":format!("未知控件{}", uuid::Uuid::new_v4()), "bookSourceName":"未知",
        "loginUi":[{"name":"账号","type":"text"},{"name":"奇怪控件","type":"input"},{"name":"说明","type":"LABEL"}]
    })).unwrap();
    let view = service.login_book_source(&source).await.unwrap();
    let fields = view["fields"].as_array().unwrap();
    assert_eq!(fields.len(), 2, "未知类型应被跳过: {fields:?}");
    assert_eq!(fields[1]["type"], "label", "类型名大小写不敏感");
    assert!(view["values"].get("奇怪控件").is_none());
}
