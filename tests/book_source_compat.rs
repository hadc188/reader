use axum::{extract::State, http::HeaderMap, response::Html, routing::get, Router};
use reader_rust::crawler::fetcher::HttpMethod;
use reader_rust::crawler::http_client::HttpClient;
use reader_rust::crawler::url_analyzer::analyze_url;
use reader_rust::model::book_source::{book_source_from_value, BookSource};
use reader_rust::model::rule::{ContentRule, SearchRule, TocRule};
use reader_rust::parser::rule_engine::RuleEngine;
use reader_rust::parser::{html, jsonpath};
use reader_rust::service::book_service::BookService;
use reader_rust::storage::cache::file_cache::FileCache;
use serde_json::json;
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};
use uuid::Uuid;

#[test]
fn book_source_deserializes_stringified_rule_objects() {
    let source: BookSource = serde_json::from_value(json!({
        "bookSourceName": "String rules",
        "bookSourceUrl": "https://example.test",
        "ruleSearch": "{\"bookList\":\".item\",\"name\":\".title@text\"}"
    }))
    .unwrap();

    let rule = source.rule_search.unwrap();
    assert_eq!(rule.book_list.as_deref(), Some(".item"));
    assert_eq!(rule.name.as_deref(), Some(".title@text"));
}

#[test]
fn legacy_book_source_fields_are_migrated_to_current_shape() {
    let source = book_source_from_value(json!({
        "bookSourceName": "Legacy",
        "bookSourceUrl": "https://legacy.example",
        "httpUserAgent": "LegacyUA",
        "ruleSearchUrl": "/search?keyword=searchKey&page=searchPage@Header:{\"X-Legacy\":\"1\"}",
        "ruleSearchList": ".item",
        "ruleSearchName": ".name@text",
        "ruleSearchAuthor": ".author@text",
        "ruleBookName": "h1@text",
        "ruleChapterList": "a",
        "ruleChapterName": "a@text",
        "ruleContentUrl": "a@href",
        "ruleBookContent": "#content@text"
    }))
    .unwrap();

    assert_eq!(
        source.header.as_deref(),
        Some("{\"User-Agent\":\"LegacyUA\"}")
    );
    assert_eq!(
        source.search_url.as_deref(),
        Some("/search?keyword={{key}}&page={{page}},{\"headers\":{\"X-Legacy\":\"1\"}}")
    );
    assert_eq!(
        source
            .rule_search
            .as_ref()
            .and_then(|rule| rule.book_list.as_deref()),
        Some(".item")
    );
    assert_eq!(
        source
            .rule_book_info
            .as_ref()
            .and_then(|rule| rule.name.as_deref()),
        Some("h1@text")
    );
    assert_eq!(
        source
            .rule_toc
            .as_ref()
            .and_then(|rule| rule.chapter_url.as_deref()),
        Some("a@href")
    );
    assert_eq!(
        source
            .rule_content
            .as_ref()
            .and_then(|rule| rule.content.as_deref()),
        Some("#content@text")
    );
}

#[test]
fn book_source_accepts_numeric_metadata_as_strings() {
    let source = book_source_from_value(json!({
        "bookSourceName": "String metadata",
        "bookSourceUrl": "https://metadata.example",
        "lastUpdateTime": "1778603539900",
        "respondTime": "180000"
    }))
    .unwrap();

    assert_eq!(source.last_update_time, Some(1_778_603_539_900));
    assert_eq!(source.respond_time, Some(180_000));
}

#[test]
fn url_analyzer_supports_inline_js_page_choices_headers_and_response_type() {
    let source = BookSource {
        book_source_name: "URL compat".to_string(),
        book_source_url: "https://a.test/root/".to_string(),
        header: Some("@js:JSON.stringify({'X-Token':'ok'})".to_string()),
        ..Default::default()
    };

    let spec = analyze_url(
        "/search?q={{key}}&page=<1,2,3>,{\"headers\":{\"Referer\":\"https://a.test\"},\"retry\":3,\"type\":\"hex\"}",
        "斗破",
        2,
        &source.book_source_url,
        &source,
    )
    .unwrap();

    assert_eq!(spec.method, HttpMethod::GET);
    assert_eq!(
        spec.url,
        "https://a.test/search?q=%E6%96%97%E7%A0%B4&page=2"
    );
    assert_eq!(spec.retry, 3);
    assert_eq!(spec.response_type.as_deref(), Some("hex"));
    assert!(spec
        .headers
        .iter()
        .any(|(name, value)| name == "X-Token" && value == "ok"));
    assert!(spec
        .headers
        .iter()
        .any(|(name, value)| name == "Referer" && value == "https://a.test"));
    assert!(spec
        .headers
        .iter()
        .any(|(name, _)| name.eq_ignore_ascii_case("user-agent")));
}

#[test]
fn url_analyzer_encodes_get_query_with_declared_charset() {
    let source = BookSource {
        book_source_name: "GBK query".to_string(),
        book_source_url: "https://b.faloo.com".to_string(),
        ..Default::default()
    };

    let spec = analyze_url(
        "/l/0/1.html?t=1&k={{key}},{\"charset\":\"gbk\"}",
        "斗破",
        1,
        &source.book_source_url,
        &source,
    )
    .unwrap();

    assert_eq!(spec.charset.as_deref(), Some("gbk"));
    assert!(
        spec.url.ends_with("/l/0/1.html?t=1&k=%B6%B7%C6%C6"),
        "{}",
        spec.url
    );
}

#[test]
fn url_analyzer_accepts_raw_newlines_in_option_strings() {
    let source = BookSource {
        book_source_name: "Relaxed options".to_string(),
        book_source_url: "https://options.example".to_string(),
        ..Default::default()
    };

    let spec = analyze_url(
        r#"https://options.example/post,{
  "method": "POST",
  "headers": {"Content-Type": "text/plain"},
  "body": "line1
line2"
}"#,
        "",
        1,
        &source.book_source_url,
        &source,
    )
    .unwrap();

    assert_eq!(spec.method, HttpMethod::POST);
    assert_eq!(spec.body.as_deref(), Some("line1\nline2"));
}

#[test]
fn url_analyzer_supports_single_brace_key_and_page_placeholders() {
    let source = BookSource {
        book_source_name: "Legacy placeholders".to_string(),
        book_source_url: "https://m.cuoceng.com".to_string(),
        ..Default::default()
    };

    let spec = analyze_url(
        "/book/so/{key}/{page}.html",
        "星门",
        3,
        &source.book_source_url,
        &source,
    )
    .unwrap();

    assert_eq!(
        spec.url,
        "https://m.cuoceng.com/book/so/%E6%98%9F%E9%97%A8/3.html"
    );
}

#[test]
fn html_rule_split_ignores_delimiters_inside_attribute_selectors() {
    let doc = html::parse_document(r#"<div data-x="a&&b">Bad</div><span>Good</span>"#);

    assert_eq!(
        html::select_text_list(&doc, r#"div[data-x="a&&b"]@text||span@text"#),
        vec!["Bad".to_string()]
    );
}

#[test]
fn jsonpath_supports_embedded_path_templates() {
    let value = json!({"data":{"name":"书名","author":"作者"}});

    assert_eq!(
        jsonpath::jsonpath_first_string(&value, "作者：{$.data.author}"),
        Some("作者：作者".to_string())
    );
}

#[test]
fn chapter_list_strips_css_mode_prefix() {
    let engine = RuleEngine::new().unwrap();
    let source = BookSource {
        book_source_name: "TOC".to_string(),
        book_source_url: "https://toc.example".to_string(),
        rule_toc: Some(TocRule {
            chapter_list: Some("@css:.dirList li a".to_string()),
            chapter_name: Some("text".to_string()),
            chapter_url: Some("href".to_string()),
            ..Default::default()
        }),
        ..Default::default()
    };

    let (chapters, _) = engine.chapter_list(
        &source,
        r#"<ul class="dirList"><li><a href="/c1.html">第一章</a></li></ul>"#,
        "https://toc.example/book/",
    );

    assert_eq!(chapters.len(), 1);
    assert_eq!(chapters[0].title, "第一章");
    assert_eq!(chapters[0].url, "https://toc.example/c1.html");
}

#[test]
fn all_in_one_regex_keeps_group_zero_literal() {
    let source = BookSource {
        book_source_name: "Regex".to_string(),
        book_source_url: "https://regex.example".to_string(),
        rule_search: Some(SearchRule {
            book_list: Some(r#":<a href="([^"]+)">([^<]+)</a>"#.to_string()),
            name: Some("$0".to_string()),
            book_url: Some("$1".to_string()),
            author: Some("$2".to_string()),
            ..Default::default()
        }),
        ..Default::default()
    };

    let results = RuleEngine::new().unwrap().search_books(
        &source,
        r#"<li><a href="/1">第一章</a></li>"#,
        "https://regex.example",
    );

    assert_eq!(results.len(), 1);
    assert_eq!(results[0].name, "$0");
    assert_eq!(results[0].book_url, "https://regex.example/1");
    assert_eq!(results[0].author, "第一章");
}

#[test]
fn explore_books_falls_back_to_search_rule_when_rule_explore_is_empty() {
    let source = BookSource {
        book_source_name: "Explore fallback".to_string(),
        book_source_url: "https://fallback.example".to_string(),
        rule_explore: Some(SearchRule::default()),
        rule_search: Some(SearchRule {
            book_list: Some("$.data[*]".to_string()),
            name: Some("$.novelName".to_string()),
            author: Some("$.authorName".to_string()),
            book_url: Some("/novel/{{$.novelId}}".to_string()),
            ..Default::default()
        }),
        ..Default::default()
    };

    let books = RuleEngine::new().unwrap().explore_books(
        &source,
        r#"{"data":[{"novelName":"书海结果","authorName":"作者","novelId":"abc"}]}"#,
        "https://fallback.example",
    );

    assert_eq!(books.len(), 1);
    assert_eq!(books[0].name, "书海结果");
    assert_eq!(books[0].book_url, "https://fallback.example/novel/abc");
}

async fn search_page(headers: HeaderMap) -> Html<&'static str> {
    if headers
        .get("x-search-token")
        .and_then(|value| value.to_str().ok())
        == Some("ok")
    {
        Html(
            r#"<div class="item"><a class="title" href="/book/1">Raw</a><span class="author">Tester</span></div>"#,
        )
    } else {
        Html("")
    }
}

#[tokio::test]
async fn search_pipeline_uses_url_analyzer_final_url_and_login_check_js() {
    let app = Router::new().route("/search", get(search_page));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    let storage_dir =
        std::env::temp_dir().join(format!("reader-rust-search-compat-{}", Uuid::new_v4()));
    let service = BookService::new(
        HttpClient::new(5, None).unwrap(),
        RuleEngine::new().unwrap(),
        FileCache::new(storage_dir.join("cache")),
        storage_dir.to_str().unwrap(),
    );
    let source = BookSource {
        book_source_name: "Runtime".to_string(),
        book_source_url: format!("http://{}", addr),
        search_url: Some(
            "/search?q={{key}}&page=<1,2>,{\"headers\":{\"X-Search-Token\":\"ok\"}}"
                .to_string(),
        ),
        login_check_js: Some(
            r#"JSON.stringify({body: result.body.replace("Raw", "Checked"), url: result.url, code: result.code, headers: result.headers, isSuccessful: result.isSuccessful})"#.to_string(),
        ),
        rule_search: Some(SearchRule {
            book_list: Some(".item".to_string()),
            name: Some(".title@text".to_string()),
            author: Some(".author@text".to_string()),
            book_url: Some(".title@href".to_string()),
            ..Default::default()
        }),
        ..Default::default()
    };

    let books = service
        .search_book("default", &source, "斗破", 2)
        .await
        .unwrap();

    server.abort();
    let _ = tokio::fs::remove_dir_all(&storage_dir).await;

    assert_eq!(books.len(), 1);
    assert_eq!(books[0].name, "Checked");
    assert_eq!(books[0].book_url, format!("http://{}/book/1", addr));
}

async fn content_page_one() -> Html<&'static str> {
    Html(
        r#"<html><body><div id="content">第一页正文</div><a class="next" href="/chapters/1-2.html">下一页</a></body></html>"#,
    )
}

async fn content_page_two() -> Html<&'static str> {
    Html(
        r#"<html><body><div id="content">第二页正文</div><a class="next" href="/chapters/2.html">下一章</a></body></html>"#,
    )
}

async fn next_chapter_page(State(hits): State<Arc<AtomicUsize>>) -> Html<&'static str> {
    hits.fetch_add(1, Ordering::SeqCst);
    Html(r#"<html><body><div id="content">第二章正文</div></body></html>"#)
}

#[tokio::test]
async fn content_pagination_stops_before_next_chapter_url() {
    let next_chapter_hits = Arc::new(AtomicUsize::new(0));
    let app = Router::new()
        .route("/chapters/1.html", get(content_page_one))
        .route("/chapters/1-2.html", get(content_page_two))
        .route("/chapters/2.html", get(next_chapter_page))
        .with_state(next_chapter_hits.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    let storage_dir =
        std::env::temp_dir().join(format!("reader-rust-content-test-{}", Uuid::new_v4()));
    let service = BookService::new(
        HttpClient::new(5, None).unwrap(),
        RuleEngine::new().unwrap(),
        FileCache::new(storage_dir.join("cache")),
        storage_dir.to_str().unwrap(),
    );
    let source = BookSource {
        book_source_name: "Content pagination".to_string(),
        book_source_url: format!("http://{}", addr),
        rule_content: Some(ContentRule {
            content: Some("#content@text".to_string()),
            next_content_url: Some("a.next@href".to_string()),
            ..Default::default()
        }),
        ..Default::default()
    };

    let content = service
        .get_content(
            "default",
            &format!("http://{}/books/1", addr),
            &source,
            &format!("http://{}/chapters/1.html", addr),
        )
        .await
        .unwrap();

    server.abort();
    let _ = tokio::fs::remove_dir_all(&storage_dir).await;

    assert!(content.contains("第一页正文"));
    assert!(content.contains("第二页正文"));
    assert!(!content.contains("第二章正文"));
    assert_eq!(next_chapter_hits.load(Ordering::SeqCst), 0);
}

#[test]
fn explore_kinds_support_text_and_js_rules() {
    let service = BookService::new(
        HttpClient::new(5, None).unwrap(),
        RuleEngine::new().unwrap(),
        FileCache::new(std::env::temp_dir().join("reader-rust-explore-kinds")),
        std::env::temp_dir().to_str().unwrap(),
    );
    let text_source = BookSource {
        book_source_name: "Explore".to_string(),
        book_source_url: "https://explore.example".to_string(),
        explore_url: Some("排行::/rank&&完本::/done".to_string()),
        ..Default::default()
    };
    let js_source = BookSource {
        explore_url: Some(
            r#"@js:JSON.stringify([{title:"玄幻",url:"/xuanhuan",style:{layout:"grid"}}])"#
                .to_string(),
        ),
        ..text_source.clone()
    };

    let text = service.explore_kinds(&text_source).unwrap();
    let js = service.explore_kinds(&js_source).unwrap();

    assert_eq!(text.len(), 2);
    assert_eq!(text[0].title, "排行");
    assert_eq!(text[0].url.as_deref(), Some("/rank"));
    assert_eq!(js.len(), 1);
    assert_eq!(js[0].title, "玄幻");
    assert_eq!(
        js[0].style.as_ref().and_then(|style| style.get("layout")),
        Some(&json!("grid"))
    );
}

#[test]
fn explore_kinds_accept_relaxed_style_objects() {
    let service = BookService::new(
        HttpClient::new(5, None).unwrap(),
        RuleEngine::new().unwrap(),
        FileCache::new(std::env::temp_dir().join("reader-rust-explore-kinds-relaxed")),
        std::env::temp_dir().to_str().unwrap(),
    );
    let source = BookSource {
        book_source_name: "Relaxed explore".to_string(),
        book_source_url: "https://explore.example".to_string(),
        explore_url: Some(
            r#"[{"title":"排行🏷榜单","url":"","style":<"layout_flexBasisPercent":1,"layout_flexGrow":1>},{"title":"总排行榜","url":"/rank/","style":<"layout_flexBasisPercent":0.4,"layout_flexGrow":1>}]"#
                .to_string(),
        ),
        ..Default::default()
    };

    let kinds = service.explore_kinds(&source).unwrap();

    assert_eq!(kinds.len(), 2);
    assert_eq!(kinds[0].title, "排行🏷榜单");
    assert_eq!(kinds[0].url.as_deref(), Some(""));
    assert_eq!(kinds[1].title, "总排行榜");
    assert_eq!(kinds[1].url.as_deref(), Some("/rank/"));
    assert_eq!(
        kinds[1]
            .style
            .as_ref()
            .and_then(|style| style.get("layout_flexBasisPercent")),
        Some(&json!(0.4))
    );
}

#[test]
fn explore_kinds_accept_relaxed_angle_item_objects() {
    let service = BookService::new(
        HttpClient::new(5, None).unwrap(),
        RuleEngine::new().unwrap(),
        FileCache::new(std::env::temp_dir().join("reader-rust-explore-kinds-angle-items")),
        std::env::temp_dir().to_str().unwrap(),
    );
    let source = BookSource {
        book_source_name: "Relaxed explore items".to_string(),
        book_source_url: "https://explore.example".to_string(),
        explore_url: Some(
            r#"[<"style":<"layout_flexBasisPercent":1.0,"layout_flexGrow":1>,"title":"书 库","url":"/book/category/catalog.html">,<"style":<"layout_flexBasisPercent":0.25,"layout_flexGrow":1>,"title":"排 行","url":"/book/ranking.html">,<"style":<"layout_flexBasisPercent":0.25,"layout_flexGrow":1>,"title":">","url":"/ranking/hits/2.html">]"#
                .to_string(),
        ),
        ..Default::default()
    };

    let kinds = service.explore_kinds(&source).unwrap();

    assert_eq!(kinds.len(), 3);
    assert_eq!(kinds[0].title, "书 库");
    assert_eq!(kinds[0].url.as_deref(), Some("/book/category/catalog.html"));
    assert_eq!(kinds[1].title, "排 行");
    assert_eq!(kinds[1].url.as_deref(), Some("/book/ranking.html"));
    assert_eq!(kinds[2].title, ">");
    assert_eq!(kinds[2].url.as_deref(), Some("/ranking/hits/2.html"));
    assert_eq!(
        kinds[1]
            .style
            .as_ref()
            .and_then(|style| style.get("layout_flexBasisPercent")),
        Some(&json!(0.25))
    );
}
