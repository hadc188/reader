use axum::{http::HeaderMap, response::Json, routing::get, Router};
use reader_rust::crawler::http_client::HttpClient;
use reader_rust::model::{book_source::BookSource, rule::TocRule};
use reader_rust::parser::rule_engine::RuleEngine;
use reader_rust::service::book_service::BookService;
use reader_rust::storage::cache::file_cache::FileCache;
use serde_json::{json, Value};
use uuid::Uuid;

async fn protected_toc(headers: HeaderMap) -> Json<Value> {
    let has_source_token = headers
        .get("x-source-token")
        .and_then(|value| value.to_str().ok())
        == Some("secret");

    if has_source_token {
        Json(json!({
            "data": {
                "list": [
                    {
                        "chapterName": "第一章",
                        "path": "https://chapters.example/1"
                    }
                ]
            }
        }))
    } else {
        Json(json!({
            "code": 4005,
            "msg": "认证失败",
            "data": {
                "list": []
            }
        }))
    }
}

#[tokio::test]
async fn chapter_list_requests_include_legacy_source_headers() {
    let app = Router::new().route("/toc", get(protected_toc));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    let storage_dir =
        std::env::temp_dir().join(format!("reader-rust-header-test-{}", Uuid::new_v4()));
    let cache = FileCache::new(storage_dir.join("cache"));
    let service = BookService::new(
        HttpClient::new(5, None).unwrap(),
        RuleEngine::new().unwrap(),
        cache,
        storage_dir.to_str().unwrap(),
    );

    let source = BookSource {
        book_source_name: "Header protected source".to_string(),
        book_source_url: format!("http://{}", addr),
        header: Some("{'X-Source-Token':'secret'}".to_string()),
        rule_toc: Some(TocRule {
            chapter_list: Some("$.data.list[*]".to_string()),
            chapter_name: Some("$.chapterName".to_string()),
            chapter_url: Some("$.path".to_string()),
            ..Default::default()
        }),
        ..Default::default()
    };

    let chapters = service
        .get_chapter_list_with_cache("default", &source, &format!("http://{}/toc", addr), true)
        .await
        .unwrap();

    server.abort();
    let _ = tokio::fs::remove_dir_all(&storage_dir).await;

    assert_eq!(chapters.len(), 1);
    assert_eq!(chapters[0].title, "第一章");
    assert_eq!(chapters[0].url, "https://chapters.example/1");
}
