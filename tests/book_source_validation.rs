use axum::{response::Html, routing::get, Router};
use reader_rust::crawler::http_client::HttpClient;
use reader_rust::model::book_source::BookSource;
use reader_rust::model::rule::SearchRule;
use reader_rust::parser::rule_engine::RuleEngine;
use reader_rust::service::book_service::BookService;
use reader_rust::service::book_source_service::set_invalid_book_source_group;
use reader_rust::storage::cache::file_cache::FileCache;
use uuid::Uuid;

async fn search_ok() -> Html<&'static str> {
    Html(
        r#"<div class="item"><a class="title" href="/book/1">搜索书</a><span class="author">作者</span></div>"#,
    )
}

async fn explore_ok() -> Html<&'static str> {
    Html(
        r#"<div class="item"><a class="title" href="/book/2">书海书</a><span class="author">作者</span></div>"#,
    )
}

async fn empty_page() -> Html<&'static str> {
    Html("")
}

#[tokio::test]
async fn source_availability_is_valid_when_search_or_explore_has_results() {
    let app = Router::new()
        .route("/search-ok", get(search_ok))
        .route("/explore-ok", get(explore_ok))
        .route("/empty", get(empty_page));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    let storage_dir =
        std::env::temp_dir().join(format!("reader-rust-source-validation-{}", Uuid::new_v4()));
    let service = BookService::new(
        HttpClient::new(5, None).unwrap(),
        RuleEngine::new().unwrap(),
        FileCache::new(storage_dir.join("cache")),
        storage_dir.to_str().unwrap(),
    );

    let search_valid = source_with_routes(&format!("http://{}", addr), "/search-ok", "/empty");
    let explore_valid = source_with_routes(&format!("http://{}", addr), "/empty", "/explore-ok");
    let invalid = source_with_routes(&format!("http://{}", addr), "/empty", "/empty");

    let search_result = service
        .test_book_source_availability("default", &search_valid, Some("书"))
        .await;
    let explore_result = service
        .test_book_source_availability("default", &explore_valid, Some("书"))
        .await;
    let invalid_result = service
        .test_book_source_availability("default", &invalid, Some("书"))
        .await;

    server.abort();
    let _ = tokio::fs::remove_dir_all(&storage_dir).await;

    assert!(search_result.valid);
    assert!(search_result.search_ok);
    assert!(!search_result.explore_ok);

    assert!(explore_result.valid);
    assert!(!explore_result.search_ok);
    assert!(explore_result.explore_ok);

    assert!(!invalid_result.valid);
    assert!(!invalid_result.search_ok);
    assert!(!invalid_result.explore_ok);
}

#[test]
fn invalid_group_marker_is_added_and_removed_without_overwriting_other_groups() {
    let mut source = BookSource {
        book_source_name: "Grouped".to_string(),
        book_source_url: "https://grouped.example".to_string(),
        book_source_group: Some("小说,精品".to_string()),
        ..Default::default()
    };

    assert!(set_invalid_book_source_group(&mut source, true));
    assert_eq!(source.book_source_group.as_deref(), Some("小说,精品,失效"));
    assert!(!set_invalid_book_source_group(&mut source, true));

    assert!(set_invalid_book_source_group(&mut source, false));
    assert_eq!(source.book_source_group.as_deref(), Some("小说,精品"));
    assert!(!set_invalid_book_source_group(&mut source, false));
}

fn source_with_routes(base: &str, search_path: &str, explore_path: &str) -> BookSource {
    BookSource {
        book_source_name: format!("Source {search_path} {explore_path}"),
        book_source_url: base.to_string(),
        search_url: Some(search_path.to_string()),
        explore_url: Some(format!("榜单::{explore_path}")),
        rule_search: Some(list_rule()),
        rule_explore: Some(list_rule()),
        ..Default::default()
    }
}

fn list_rule() -> SearchRule {
    SearchRule {
        check_key_word: Some("书".to_string()),
        book_list: Some(".item".to_string()),
        name: Some(".title@text".to_string()),
        author: Some(".author@text".to_string()),
        book_url: Some(".title@href".to_string()),
        ..Default::default()
    }
}

/// 永远不返回的页面: 用来验证取消能真正中断在途请求(而不是等 HTTP 超时)。
async fn hang() -> Html<&'static str> {
    tokio::time::sleep(std::time::Duration::from_secs(60)).await;
    Html("never")
}

/// 取消后返回的可用性必须带 `cancelled = true`。
///
/// 这是「不要把用户主动中止当成站点不可用」的守门测试: 调用方据此跳过写库,
/// 否则一个完全正常的书源会在中止时被标成失效。
#[tokio::test]
async fn cancelled_availability_is_marked_and_not_treated_as_invalid() {
    use std::sync::atomic::AtomicBool;
    use std::sync::Arc;

    let app = Router::new()
        .route("/search-ok", get(search_ok))
        .route("/empty", get(empty_page))
        .route("/hang", get(hang));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    let storage_dir =
        std::env::temp_dir().join(format!("reader-rust-cancel-{}", Uuid::new_v4()));
    let service = BookService::new(
        HttpClient::new(5, None).unwrap(),
        RuleEngine::new().unwrap(),
        FileCache::new(storage_dir.join("cache")),
        storage_dir.to_str().unwrap(),
    );

    let source = source_with_routes(&format!("http://{}", addr), "/hang", "/empty");
    // 先置位, 模拟用户点"中止"后再进入检测。
    let cancel = Arc::new(AtomicBool::new(true));

    let started = std::time::Instant::now();
    let result = service
        .test_book_source_availability_cancellable("default", &source, Some("书"), Some(&cancel))
        .await;
    let elapsed = started.elapsed();

    server.abort();
    let _ = tokio::fs::remove_dir_all(&storage_dir).await;

    assert!(result.cancelled, "取消后必须标记 cancelled, 否则会被误标失效并写库");
    assert!(!result.valid);
    // 页面会挂 60s; 取消已置位时必须在超时前就返回, 证明没有傻等。
    assert!(
        elapsed < std::time::Duration::from_secs(10),
        "取消应立即返回, 实际耗时 {elapsed:?}"
    );
    // 置位取消不能算成"检测过且失败"。
    assert!(result.search_error.is_none(), "取消不应被记录成搜索错误: {:?}", result.search_error);
}

/// 取消必须能中断**在途**请求: 先开始检测, 再置位取消。
///
/// 用挂起 60s 的页面, 若取消无效就会一直等到超时; 这里要求显著早于超时返回。
#[tokio::test]
async fn cancel_interrupts_in_flight_request() {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;

    let app = Router::new()
        .route("/hang", get(hang))
        .route("/empty", get(empty_page));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    let storage_dir =
        std::env::temp_dir().join(format!("reader-rust-cancel-inflight-{}", Uuid::new_v4()));
    let service = BookService::new(
        // 超时给足 30s: 如果取消没生效, 测试就会真的等这么久而超时失败。
        HttpClient::new(30, None).unwrap(),
        RuleEngine::new().unwrap(),
        FileCache::new(storage_dir.join("cache")),
        storage_dir.to_str().unwrap(),
    );

    let source = source_with_routes(&format!("http://{}", addr), "/hang", "/empty");
    let cancel = Arc::new(AtomicBool::new(false));

    // 请求发出后再取消(让任务真正卡在 in-flight 的 await 上)。
    let canceller = {
        let cancel = cancel.clone();
        tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(300)).await;
            cancel.store(true, Ordering::Relaxed);
        })
    };

    let started = std::time::Instant::now();
    let result = service
        .test_book_source_availability_cancellable("default", &source, Some("书"), Some(&cancel))
        .await;
    let elapsed = started.elapsed();
    canceller.abort();

    server.abort();
    let _ = tokio::fs::remove_dir_all(&storage_dir).await;

    assert!(result.cancelled);
    assert!(
        elapsed < std::time::Duration::from_secs(10),
        "取消应中断在途请求(而不是等 30s 超时), 实际耗时 {elapsed:?}"
    );
}

/// 取消不能把该源的限速 slot 泄漏成"永久占用"。
///
/// 配了 `concurrentRate` 的源只有一个 rate slot。若取消路径漏归还 `in_flight`,
/// 该源之后**所有**请求都会卡在限速循环里死等 —— 比"中止慢一点"严重得多。
/// 这里用大延迟 + 取消, 然后确认后续请求仍能正常拿到 slot 并完成。
#[tokio::test]
async fn cancel_does_not_leak_rate_limit_slot() {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;

    let app = Router::new()
        .route("/search-ok", get(search_ok))
        .route("/empty", get(empty_page))
        .route("/hang", get(hang));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    let storage_dir =
        std::env::temp_dir().join(format!("reader-rust-cancel-rate-{}", Uuid::new_v4()));
    let service = BookService::new(
        HttpClient::new(30, None).unwrap(),
        RuleEngine::new().unwrap(),
        FileCache::new(storage_dir.join("cache")),
        storage_dir.to_str().unwrap(),
    );

    // 大限速间隔: 若 slot 被泄漏, 后续请求会在这里死等到超出测试时间。
    let mut rate_limited = source_with_routes(&format!("http://{}", addr), "/hang", "/empty");
    rate_limited.concurrent_rate = Some("2000".to_string());
    let cancel = Arc::new(AtomicBool::new(false));
    let canceller = {
        let cancel = cancel.clone();
        tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(300)).await;
            cancel.store(true, Ordering::Relaxed);
        })
    };

    let first = service
        .test_book_source_availability_cancellable(
            "default",
            &rate_limited,
            Some("书"),
            Some(&cancel),
        )
        .await;
    canceller.abort();
    assert!(first.cancelled, "第一次应在取消后返回");

    // 关键断言: 取消后该源的 slot 必须是空闲的, 后续请求能正常完成。
    let mut ok_source = source_with_routes(&format!("http://{}", addr), "/search-ok", "/empty");
    ok_source.concurrent_rate = Some("2000".to_string());
    let second = tokio::time::timeout(
        std::time::Duration::from_secs(10),
        service.test_book_source_availability("default", &ok_source, Some("书")),
    )
    .await;

    server.abort();
    let _ = tokio::fs::remove_dir_all(&storage_dir).await;

    let second = second.expect("slot 泄漏了: 取消后后续请求一直卡在限速等待");
    assert!(second.valid, "取消后该源仍应能被正常检测");
}
