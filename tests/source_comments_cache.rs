use base64::{engine::general_purpose::STANDARD, Engine};
use reader_rust::crawler::http_client::HttpClient;
use reader_rust::model::{
    book_chapter::BookChapter,
    book_source::{book_source_from_value, BookSource},
};
use reader_rust::parser::rule_engine::RuleEngine;
use reader_rust::service::book_service::BookService;
use reader_rust::storage::cache::file_cache::FileCache;
use reader_rust::util::hash::md5_hex;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

fn service() -> (BookService, FileCache, PathBuf) {
    let root = std::env::temp_dir().join(format!("reader-comment-cache-{}", uuid::Uuid::new_v4()));
    let cache = FileCache::new(root.join("cache"));
    let service = BookService::new(
        HttpClient::new(2, None).unwrap(),
        RuleEngine::new().unwrap(),
        cache.clone(),
        root.to_str().unwrap(),
    );
    (service, cache, root)
}

async fn write_previous_catalog(root: &Path, toc_url: &str, chapter_url: &str) {
    let directory = root.join("data/test/chapters");
    tokio::fs::create_dir_all(&directory).await.unwrap();
    let chapters = vec![BookChapter {
        title: "旧目录".into(),
        url: chapter_url.into(),
        ..Default::default()
    }];
    tokio::fs::write(
        directory.join(format!("{}.v2.json", md5_hex(toc_url))),
        serde_json::to_vec(&chapters).unwrap(),
    )
    .await
    .unwrap();
}

#[tokio::test]
async fn aggregate_catalog_rebuilds_review_urls_once_without_clearing_content() {
    let (service, cache, root) = service();
    let payload = json!({
        "book_id": "fixture-book",
        "data": [{"title": "第一章", "item_id": "fixture-chapter"}]
    });
    let toc_url = format!(
        "data:;base64,{},{{\"type\": \"gycatalog\"}}",
        STANDARD.encode(payload.to_string())
    );
    // Before java.getCookie was available, this URL rule could fail and cache
    // its fallback URL. Rebuilding the catalog must run the repaired rule.
    write_previous_catalog(&root, &toc_url, "data:;base64,e30=").await;
    let old_chapter_url = "data:;base64,e30=";
    cache
        .put("test", &md5_hex(&toc_url), old_chapter_url, "已下载正文")
        .await
        .unwrap();
    assert!(service
        .load_chapter_list_cache("test", &toc_url)
        .await
        .unwrap()
        .is_none());
    assert!(!service.chapter_list_cache_exists("test", &toc_url).await);

    let source = book_source_from_value(json!({
        "bookSourceUrl": format!("https://catalog-cache-{}.test", uuid::Uuid::new_v4()),
        "ruleToc": {
            "chapterList": "<js>var catalog=JSON.parse(java.hexDecodeToString(result));java.put('book_id',catalog.book_id);JSON.stringify(catalog)</js>$.data",
            "chapterName": "$.title",
            "chapterUrl": r#"<js>
var bookId = java.get('book_id');
var sessionId = java.getCookie('theta.example.com', 'sessionid');
var payload = {book_id:bookId,item_id:result.item_id};
'data:;base64,'+java.base64Encode(JSON.stringify(payload))+','+JSON.stringify({
    type:'gycontent',
    js:"book ? result : 'https://reviews.fixture.test/get_review?book_id="+bookId+"&item_id="+result.item_id+"&ssionid="+sessionId+"'"
})
</js>"#
        }
    }))
    .unwrap();
    let chapters = service
        .get_chapter_list("test", &source, &toc_url)
        .await
        .unwrap();
    assert_eq!(chapters.len(), 1);
    assert_eq!(chapters[0].title, "第一章");
    assert!(chapters[0]
        .url
        .contains("get_review?book_id=fixture-book&item_id=fixture-chapter"));
    let encoded = chapters[0].url.split(',').nth(1).unwrap();
    let payload: Value = serde_json::from_slice(&STANDARD.decode(encoded).unwrap()).unwrap();
    assert_eq!(
        payload,
        json!({"book_id":"fixture-book","item_id":"fixture-chapter"})
    );
    assert!(service.chapter_list_cache_exists("test", &toc_url).await);

    // A subsequent request must reuse the upgraded catalog, even if parsing
    // would fail, and appending a page must use that same upgraded file.
    let cached = service
        .get_chapter_list("test", &BookSource::default(), &toc_url)
        .await
        .unwrap();
    assert_eq!(cached[0].url, chapters[0].url);
    let appended = service
        .append_chapter_list_cache(
            "test",
            &toc_url,
            &vec![BookChapter {
                title: "第二章".into(),
                url: "https://fixture.test/2".into(),
                ..Default::default()
            }],
        )
        .await
        .unwrap();
    assert_eq!(appended.len(), 2);
    assert_eq!(appended[1].index, 1);
    service
        .delete_chapter_list_cache("test", &toc_url)
        .await
        .unwrap();
    assert!(service
        .load_chapter_list_cache("test", &toc_url)
        .await
        .unwrap()
        .is_none());
    assert_eq!(
        cache
            .get("test", &md5_hex(&toc_url), old_chapter_url)
            .await
            .unwrap()
            .as_deref(),
        Some("已下载正文")
    );
    tokio::fs::remove_dir_all(root).await.unwrap();
}

#[tokio::test]
async fn unrelated_catalogs_continue_using_existing_cache() {
    let (service, _, root) = service();
    for toc_url in [
        "https://fixture.test/toc",
        "https://fixture.test/toc,{\"type\":\"gycatalog\"}",
        "data:;base64,e30=,{\"type\":\"another-catalog\",\"note\":\"gycatalog\"}",
        "data:;base64,e30=",
    ] {
        write_previous_catalog(&root, toc_url, "https://fixture.test/kept").await;
        let chapters = service
            .get_chapter_list("test", &BookSource::default(), toc_url)
            .await
            .unwrap();
        assert_eq!(chapters.len(), 1);
        assert_eq!(chapters[0].url, "https://fixture.test/kept");
        assert!(service.chapter_list_cache_exists("test", toc_url).await);
        service
            .delete_chapter_list_cache("test", toc_url)
            .await
            .unwrap();
        assert!(!service.chapter_list_cache_exists("test", toc_url).await);
    }
    tokio::fs::remove_dir_all(root).await.unwrap();
}

#[tokio::test]
async fn cached_content_is_served_without_refetch_and_fresh_content_carries_comments() {
    let (service, cache, root) = service();
    let source = book_source_from_value(json!({
        "bookSourceUrl": format!("https://content-cache-{}.test", uuid::Uuid::new_v4()),
        "jsLib": "function addComment(text){return text+'<img src=\"https://fixture.test/count.svg\" click=\"showCommentPanel()\">';}",
        "ruleContent": {"content": ".content@text@js:addComment(result)"}
    }))
    .unwrap();
    let book_url = "https://fixture.test/book";
    let chapter_url = format!(
        "data:text/html;base64,{}",
        STANDARD.encode("<div class='content'>在线正文</div>")
    );
    cache
        .put("test", &md5_hex(book_url), &chapter_url, "已有正文")
        .await
        .unwrap();

    // 缓存命中即返回本地正文, 不再联网 —— 与 legado 的 BookHelp.getContent 一致。
    assert_eq!(
        service
            .get_content_with_stage("test", book_url, &source, &chapter_url, None, None)
            .await
            .unwrap(),
        "已有正文"
    );

    // 想让这一章重新抓取, 先删掉本章缓存; 新正文必须带上段评(jsLib 里的 addComment)。
    service
        .delete_chapter_cache("test", book_url, &chapter_url)
        .await
        .unwrap();
    let content = service
        .get_content("test", book_url, &source, &chapter_url)
        .await
        .unwrap();
    assert!(content.starts_with("在线正文"));
    assert!(content.contains("click=\"showCommentPanel()\""));
    assert_eq!(
        cache
            .get("test", &md5_hex(book_url), &chapter_url)
            .await
            .unwrap()
            .as_deref(),
        Some(content.as_str())
    );

    // 未缓存的章节抓取失败就是失败, 不能被伪装成"读到了正文"。
    let unavailable_url = "data:;base64,invalid!!";
    assert!(
        service
            .get_content("test", book_url, &source, unavailable_url)
            .await
            .is_err()
    );
    assert!(!service.is_chapter_cached("test", book_url, unavailable_url).await);
    tokio::fs::remove_dir_all(root).await.unwrap();
}
