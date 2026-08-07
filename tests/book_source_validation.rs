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
