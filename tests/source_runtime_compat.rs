use axum::{http::{header, HeaderMap}, routing::get, Router};
use reader_rust::{
    crawler::{fetcher::{fetch_with_client, RequestSpec}, http_client::HttpClient},
    model::book_source::{book_source_from_value, BookSource},
    parser::{js::{eval_js, with_source}, rule_engine::RuleEngine},
    service::book_service::BookService, storage::cache::file_cache::FileCache,
};
use serde_json::json;
use std::path::Path;

fn service(root: &Path) -> BookService {
    BookService::new(HttpClient::new(5, None).unwrap(), RuleEngine::new().unwrap(), FileCache::new(root.join("cache")), root.to_str().unwrap())
}

#[test]
fn java_dependencies_exist_during_library_initialization_and_stay_stable() {
    let source = BookSource {
        book_source_url: "https://java-library.test".into(),
        js_lib: Some("var JB=Packages.java.math.BigInteger; var MASK=new JB('18446744073709551615'); var EMPTY=new Packages.java.lang.String('').getBytes(); function mask(v){return new JB(v).and(MASK).toString(16)}".into()),
        ..Default::default()
    };
    for _ in 0..2 {
        let result = with_source(&source, || eval_js("JSON.stringify([mask('-1'), new Packages.java.math.BigInteger('1') instanceof JB, Packages.java.util.Arrays.copyOf(EMPTY,3)])", "", "")).unwrap();
        assert_eq!(result, r#"["ffffffffffffffff",true,[0,0,0]]"#);
    }
}

#[test]
fn java_bytes_digest_cipher_and_streams_match_known_values() {
    let result = eval_js(r#"
        const J=Packages.java, C=Packages.javax.crypto;
        const key=new J.lang.String('0123456789abcdef01234567').getBytes('UTF-8');
        const cipher=C.Cipher.getInstance('AES/ECB/PKCS5Padding');
        cipher.init(1,new C.spec.SecretKeySpec(key,'AES'));
        const encrypted=cipher.doFinal(new J.lang.String('abc').getBytes());
        const digest=J.security.MessageDigest.getInstance('SHA-256').digest(new J.lang.String('abc').getBytes());
        function hex(bytes){return bytes.map(function(b){return ('0'+(b&255).toString(16)).slice(-2)}).join('')}
        JSON.stringify([hex(encrypted),hex(digest),String(new J.lang.String([-42,-48,-50,-60],'GBK'))]);
    "#, "", "").unwrap();
    assert_eq!(serde_json::from_str::<serde_json::Value>(&result).unwrap(), json!([
        "377f35478afd40126bdcb4d5f9be9ee6",
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad",
        "中文"
    ]));
    // A second evaluation may declare the same lexical names.
    assert_eq!(eval_js("const J=Packages.java; String(new J.lang.String('继续'))", "", "").unwrap(), "继续");
    let gz = [31,139,8,0,0,0,0,0,2,3,203,72,205,201,201,7,0,134,166,16,54,5,0,0,0];
    let code = format!(r#"var J=Packages.java; var stream=new J.util.zip.GZIPInputStream(new J.io.ByteArrayInputStream({})); var out=new J.io.ByteArrayOutputStream(); var b=[0,0]; var n; while((n=stream.read(b))>0) out.write(b,0,n); String(new J.lang.String(out.toByteArray(),'UTF-8'))"#, json!(gz));
    assert_eq!(eval_js(&code, "", "").unwrap(), "hello");
}

#[tokio::test(flavor="multi_thread", worker_threads=2)]
async fn script_and_native_requests_share_cookies_and_response_decoding() {
    let router = Router::new()
        .route("/prime", get(|| async { ([(header::SET_COOKIE,"session=ready; Path=/")], "ok") }))
        .route("/search", get(|headers: HeaderMap| async move {
            if headers.get(header::COOKIE).and_then(|v|v.to_str().ok()).unwrap_or("").contains("session=ready") {
                "<div class='item'><a href='/book'>示例书</a></div>"
            } else { "<p>请登录</p>" }
        }))
        .route("/gbk", get(|| async { ([(header::CONTENT_TYPE,"text/plain; charset=gbk")], vec![0xd6u8,0xd0,0xce,0xc4]) }));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    let root = std::env::temp_dir().join(format!("reader-runtime-{}", uuid::Uuid::new_v4()));
    let svc = service(&root);
    let source = book_source_from_value(json!({
        "bookSourceUrl":base,"bookSourceName":"Session",
        "searchUrl":"@js:cookie.removeCookie(baseUrl); java.ajax(baseUrl+'/prime'); baseUrl+'/search'",
        "ruleSearch":{"bookList":".item","name":"a@text","bookUrl":"a@href"}
    })).unwrap();
    let books = svc.search_book("default", &source, "示例", 1).await.unwrap();
    assert_eq!(books.len(),1);
    let cookie = svc.source_scope("default", &source, || eval_js("cookie.getCookie(baseUrl)", "", &base)).unwrap().unwrap();
    assert!(cookie.contains("session=ready"));
    let text = svc.source_scope("default", &source, || eval_js("java.ajax(baseUrl+'/gbk')", "", &base)).unwrap().unwrap();
    assert_eq!(text,"中文");
    let other = BookSource { book_source_url:format!("{base}/other"), ..source.clone() };
    let cookie = svc.source_scope("default", &other, || eval_js("cookie.getCookie(baseUrl)", "", &base)).unwrap().unwrap();
    assert!(cookie.is_empty());
    drop(svc);
    let restored = service(&root);
    let cookie = restored.source_scope("default", &source, || eval_js("cookie.getCookie(baseUrl)", "", &base)).unwrap().unwrap();
    assert!(cookie.contains("session=ready"));
    restored.clear_source_cookie("default", &base).await;
    let cookie = restored.source_scope("default", &source, || eval_js("cookie.getCookie(baseUrl)", "", &base)).unwrap().unwrap();
    assert!(cookie.is_empty());
    server.abort();
}

#[tokio::test]
async fn data_urls_obey_binary_response_options() {
    let client=HttpClient::new(5,None).unwrap().client();
    let response=fetch_with_client(&client,RequestSpec {url:"data:text/plain;base64,5Lit5paH".into(),..Default::default()}).await.unwrap();
    assert_eq!(response.body,"中文");
    let response=fetch_with_client(&client,RequestSpec {url:"data:application/octet-stream;base64,AP8=".into(),response_type:Some("1".into()),..Default::default()}).await.unwrap();
    assert_eq!(response.body,"00ff");
    assert_eq!(eval_js("java.ajax('data:application/json,{\"value\":1}')", "", "").unwrap(), r#"{"value":1}"#);
}

#[tokio::test(flavor="multi_thread", worker_threads=2)]
async fn keyword_and_page_are_bound_in_background_search_parsing() {
    let root=std::env::temp_dir().join(format!("reader-search-context-{}",uuid::Uuid::new_v4()));
    let svc=service(&root);
    let source=book_source_from_value(json!({
        "bookSourceUrl":"https://search-context.test", "bookSourceName":"Search bindings",
        "searchUrl":"data:text/plain,fixture",
        "ruleSearch":{"bookList":"@js:[{name:key,author:String(page),url:'https://search-context.test/book'}]", "name":"$.name", "author":"$.author", "bookUrl":"$.url"}
    })).unwrap();
    let books=svc.search_book("default",&source,"示例查询",3).await.unwrap();
    assert_eq!(books[0].name,"示例查询");
    assert_eq!(books[0].author,"3");
    let debug=svc.debug_source_step("default",&source,"search","调试查询","","").await.unwrap();
    assert_eq!(debug.result[0]["name"],"调试查询");
}

#[tokio::test]
async fn offline_cache_keeps_libraries_required_to_decode_chapters() {
    let root=std::env::temp_dir().join(format!("reader-cache-library-{}",uuid::Uuid::new_v4()));
    let svc=service(&root);
    let source=book_source_from_value(json!({
        "bookSourceUrl":"https://cache-library.test", "bookSourceName":"Required library",
        "jsLib":"function decodeChapter(){return '解码后的正文'}",
        "ruleContent":{"content":"@js:decodeChapter()"}
    })).unwrap();
    let content=svc.get_content_with_stage("default","https://cache-library.test/book",&source,"data:text/plain,fixture",None,None).await.unwrap();
    assert_eq!(content,"解码后的正文");
}

#[tokio::test(flavor="multi_thread", worker_threads=2)]
async fn script_requests_observe_the_source_request_interval() {
    let starts=std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let recorded=starts.clone();
    let router=Router::new().route("/",get(move || {
        recorded.lock().unwrap().push(std::time::Instant::now());
        async { "ok" }
    }));
    let listener=tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base=format!("http://{}",listener.local_addr().unwrap());
    let server=tokio::spawn(async move {axum::serve(listener,router).await.unwrap()});
    let source=BookSource {book_source_url:base.clone(),concurrent_rate:Some("70".into()),..Default::default()};
    let root=std::env::temp_dir().join(format!("reader-script-rate-{}",uuid::Uuid::new_v4()));
    let svc=service(&root);
    svc.source_scope("default",&source,||eval_js("java.ajax(baseUrl); java.ajax(baseUrl)","",&base)).unwrap().unwrap();
    let starts=starts.lock().unwrap();
    assert_eq!(starts.len(),2);
    assert!(starts[1].duration_since(starts[0]) >= std::time::Duration::from_millis(60));
    server.abort();
}

#[tokio::test(flavor="multi_thread", worker_threads=2)]
async fn empty_search_does_not_invalidate_a_cookie_or_change_the_saved_session() {
    let router=Router::new().route("/",get(||async {"<html><body>没有匹配的书籍</body></html>"}));
    let listener=tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base=format!("http://{}",listener.local_addr().unwrap());
    let server=tokio::spawn(async move {axum::serve(listener,router).await.unwrap()});
    let source=book_source_from_value(json!({"bookSourceName":"Cookie check","bookSourceUrl":base,"searchUrl":"/","ruleSearch":{"bookList":".book","name":"a@text"}})).unwrap();
    let root=std::env::temp_dir().join(format!("reader-cookie-check-{}",uuid::Uuid::new_v4()));
    let svc=service(&root);
    svc.set_source_cookie("default",&base,"session=original").await.unwrap();
    assert!(!svc.validate_source_cookie("default",&source,"session=candidate","书名").await.unwrap());
    let saved=svc.source_scope("default",&source,||eval_js("cookie.getCookie(baseUrl)","",&base)).unwrap().unwrap();
    assert_eq!(saved,"session=original");
    assert!(svc.set_source_cookie("default",&base,"invalid").await.is_err());
    let saved=svc.source_scope("default",&source,||eval_js("cookie.getCookie(baseUrl)","",&base)).unwrap().unwrap();
    assert_eq!(saved,"session=original");
    server.abort();
}

#[tokio::test(flavor="multi_thread", worker_threads=2)]
async fn login_form_actions_update_fields_and_persist_source_state() {
    let root=std::env::temp_dir().join(format!("reader-login-{}",uuid::Uuid::new_v4()));
    let svc=service(&root);
    let source=book_source_from_value(json!({
        "bookSourceUrl":"https://login-flow.test", "bookSourceName":"Login",
        "loginUi":r#"[{"name":"设备","type":"text"},{"name":"设置","type":"button","action":"configure()"}]"#,
        "jsLib":"var initial = new Packages.java.lang.String('已配置').toString();",
        "loginUrl":"var result = typeof result !== 'undefined' && result ? result : {}; function configure(){source.setVariable(JSON.stringify({device:'fixture'}));java.upLoginData({'设备':initial});java.longToast('设置完成')} function login(){source.putLoginHeader(JSON.stringify({'X-Device':result['设备']}))}"
    })).unwrap();
    let view=svc.login_book_source(&source).await.unwrap();
    assert_eq!(view["mode"],"form");
    let action=svc.run_source_login(&source,Some("button:1".into()),Some(serde_json::Map::new())).await.unwrap();
    assert_eq!(action["values"]["设备"],"已配置");
    assert_eq!(action["messages"][0],"设置完成");
    let values=action["values"].as_object().unwrap().clone();
    svc.run_source_login(&source,Some("login".into()),Some(values)).await.unwrap();
    drop(svc);
    let restored=service(&root);
    assert_eq!(restored.login_book_source(&source).await.unwrap()["values"]["设备"],"已配置");
    let header=restored.source_scope("default",&source,||eval_js("source.getLoginHeader()","","")).unwrap().unwrap();
    assert_eq!(serde_json::from_str::<serde_json::Value>(&header).unwrap()["X-Device"],"已配置");
}
