//! `ajaxAll` / `StrResponse.callTime()` / `js` 请求选项 的 legado 语义。
//!
//! 这三处都是「书源自己写的脚本靠它工作、缺了不会报错只会静默失效」的接口：
//!
//! - `java.ajaxAll(urls)` 是书源做**线路测速**的唯一入口，靠 `callTime()` 比快慢；
//! - `StrResponse.callTime()` 在 legado 里由 `AnalyzeUrl.getStrResponse` 记入
//!   `System.currentTimeMillis() - startTime`；
//! - `,{js:"…"}` 的脚本**返回值**才会覆盖 url（legado `evalJS(js, url)?.toString()?.let`）。
//!
//! 演过的现场（yckceo id 8035「手机小说」）：
//! - 一键测速全部线路被判定失效（`callTime` 不是函数 → `t` 恒为 -1）；
//! - 搜索恒失败，报 `builder error: relative URL without a base`
//!   （`js` 脚本末尾的 `try{cookie.removeCookie(u)}catch(e){}` 完成值是 `true`，
//!   被当成新 url）。
use axum::{routing::get, Router};
use reader_rust::{
    crawler::{http_client::HttpClient, url_analyzer::analyze_url},
    model::book_source::{book_source_from_value, BookSource},
    parser::{js::{eval_js, with_source}, rule_engine::RuleEngine},
    service::book_service::BookService,
    storage::cache::file_cache::FileCache,
};
use serde_json::json;
use std::path::Path;

fn service(root: &Path) -> BookService {
    BookService::new(HttpClient::new(5, None).unwrap(), RuleEngine::new().unwrap(), FileCache::new(root.join("cache")), root.to_str().unwrap())
}

/// `StrResponse.callTime()` 必须存在且是毫秒数，`ajaxAll` 要按输入顺序返回。
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ajax_all_exposes_call_time_in_input_order() {
    let router = Router::new()
        .route("/a", get(|| async { "a".repeat(200) }))
        .route("/b", get(|| async { "b".repeat(200) }));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    let root = std::env::temp_dir().join(format!("reader-ajaxall-{}", uuid::Uuid::new_v4()));
    let svc = service(&root);
    let source = book_source_from_value(json!({
        "bookSourceUrl": base, "bookSourceName": "测速",
    }))
    .unwrap();

    let out = svc
        .source_scope("default", &source, || {
            eval_js(
                r#"
var host = String(source.getKey());
var hs = [host + '/a', host + '/b'];
var rs = java.ajaxAll(hs);
var out = [];
for (var i = 0; i < rs.length; i++) {
  var body = String(rs[i].body());
  var t = rs[i].callTime();
  out.push(body.charAt(0) + ':' + (typeof t === 'number' && t >= 0 ? 'ok' : 'bad'));
}
out.join('|')
"#,
                "",
                &base,
            )
        })
        .unwrap()
        .unwrap();

    // 顺序必须与输入一致：`rs[i]` 要和 `hs[i]` 对上，书源靠下标选线路。
    assert_eq!(out, "a:ok|b:ok", "ajaxAll 应保持输入顺序且 callTime() 可用，实际 {out}");
    server.abort();
    drop(svc);
    let _ = std::fs::remove_dir_all(&root);
}

/// `,{js:"…"}` 的脚本返回值不是地址时必须**保留原 url**。
///
/// 书源把 `js` 选项当"请求前副作用"用，末尾写 `try{cookie.removeCookie(u)}catch(e){}`；
/// 按 ECMAScript 规范这条语句的完成值是 `true`，直接当 url 会炸成
/// `relative URL without a base`。
#[test]
fn request_js_option_does_not_replace_url_with_closure_value() {
    let source = book_source_from_value(json!({
        "bookSourceUrl": "https://js-option.test", "bookSourceName": "js 选项",
    }))
    .unwrap();

    // 完成值为 true（副作用脚本的典型形态）→ 必须保留原 url。
    let rule = r#"@js:['https://js-option.test/search/', '{"js":"try{cookie.removeCookie(baseUrl)}catch(e){}","method":"POST","body":"searchkey={{key}}&t=1"}'].join(',')"#;
    let spec = analyze_url(rule, "测试", 1, &source.book_source_url, &source).unwrap();
    assert_eq!(
        spec.url, "https://js-option.test/search/",
        "js 选项返回非地址文本时不该覆盖 url"
    );

    // 真正返回地址时仍然要生效（这是该选项的本意）。
    let rule = r#"@js:['https://js-option.test/search/', '{"js":"\'https://js-option.test/other/\'"}'].join(',')"#;
    let spec = analyze_url(rule, "测试", 1, &source.book_source_url, &source).unwrap();
    assert_eq!(spec.url, "https://js-option.test/other/", "js 返回合法地址时应覆盖 url");
}

/// `parentNode()` 必须存在，且返回的父元素要能读标签名与**自己内部**的内容。
///
/// 父快照刻意**不带父元素的整段内文**：目录规则常是整本 2000+ 章，
/// 每个 `<a>` 都复制一份父 `<dl>` 的内文会让总体积变成 O(n²)
/// （实测 2791 章 → 3.7 GB / 170 秒）。因此这里断言的是：
/// 父节点可拿到、`tagName()` 正确、`select()` 能在父节点内继续查子元素。
#[test]
fn element_parent_node_is_available_to_scripts() {
    let source = BookSource {
        book_source_url: "https://parent.test".into(),
        ..Default::default()
    };
    let html = r#"<div class="list"><p><a href="/1.html">第1章</a></p><p><a href="/2.html">第2章</a></p></div>"#;
    let out = with_source(&source, || {
        eval_js(
            r#"
var D = org.jsoup.Jsoup.parse(result);
var as = D.select('div.list a');
var out = [];
for (var i = 0; i < as.size(); i++) {
  var p = as.get(i).parentNode();
  out.push(p === null ? 'null' : (String(p.tagName()) + '>' + String(p.attr('class'))));
}
out.join('|')
"#,
            html,
            "",
        )
    })
    .unwrap();
    assert_eq!(out, "p>|p>", "parentNode() 应返回包裹 <a> 的 <p>，实际 {out}");
}

/// 父节点快照不能带整段内文（否则大目录会 O(n²) 膨胀）。
///
/// 这条盯住体积：父节点的 `outerHtml`/`innerHtml` 必须是空串，`text` 只含
/// 直属文本节点。改动这里之前先看 `parent_snapshot` 的注释。
#[test]
fn parent_snapshot_stays_small() {
    let source = BookSource {
        book_source_url: "https://parent-size.test".into(),
        ..Default::default()
    };
    let mut html = String::from("<div class=\"list\">");
    for i in 0..60 {
        html.push_str(&format!("<p><a href=\"/{i}.html\">第{i}章</a></p>"));
    }
    html.push_str("</div>");

    let out = with_source(&source, || {
        eval_js(
            r#"
var D = org.jsoup.Jsoup.parse(result);
var as = D.select('div.list a');
var p = as.get(0).parentNode();
String(p.outerHtml().length) + '|' + String(p.html().length) + '|' + String(p.text().length)
"#,
            &html,
            "",
        )
    })
    .unwrap();
    assert_eq!(out, "0|0|0", "父快照不应携带内文，实际 outerHtml|html|text = {out}");
}
