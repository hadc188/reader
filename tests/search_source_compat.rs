use reader_rust::crawler::http_client::HttpClient;
use reader_rust::crawler::url_analyzer::analyze_url;
use reader_rust::model::book_source::{book_source_from_value, BookSource};
use reader_rust::parser::js::take_js_logs;
use reader_rust::parser::rule_engine::RuleEngine;
use reader_rust::service::book_service::BookService;
use reader_rust::storage::cache::file_cache::FileCache;
use serde_json::{json, Value};

fn aggregate_source() -> BookSource {
    book_source_from_value(json!({
        "bookSourceUrl": "聚合测试", "bookSourceName": "聚合测试",
        "searchUrl": "@js:'data:;base64,'+java.base64Encode(JSON.stringify({key:key,page:page}))+',{\"type\":\"gysearch\"}'",
        "jsLib": "function request(key,page){if(key!=='斗破苍穹'||page!==2)throw new Error('search context lost');return JSON.stringify({data:[{book_id:'1',book_name:'斗破苍穹',author:'天蚕土豆',source:'猫眼'},{book_id:'1',book_name:'斗破苍穹',author:'天蚕土豆',source:'百度'}]});}",
        "ruleSearch": {
            "bookList": "<js>\nconst res=JSON.parse(java.hexDecodeToString(result));\nrequest(res.key,res.page);\n</js>\n$.data",
            "name": "$.book_name", "author": "$.author", "lastChapter": "{{$.source}} {{$.last_chapter_title}}",
            "bookUrl": "<js>\nlet book_id=result.book_id;\nlet sources=result.source;\nlet tab=result.tab||'小说';\nlet url=result.toc_url||'';\nlet detail={book_id:book_id,sources:sources,tab:tab,url:url};\n'data:;base64,'+java.base64Encode(JSON.stringify(detail))+',{\"type\":\"gydetail\"}';\n</js>"
        }
    })).unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn aggregate_search_decodes_data_and_runs_js_then_jsonpath() {
    let source = aggregate_source();
    let root = std::env::temp_dir().join(format!("reader-search-rule-{}", uuid::Uuid::new_v4()));
    let service = BookService::new(HttpClient::new(2, None).unwrap(), RuleEngine::new().unwrap(), FileCache::new(root.join("cache")), root.to_str().unwrap());
    let books = service.search_book("test", &source, "斗破苍穹", 2).await.unwrap();
    assert_eq!(books.len(), 2);
    assert_eq!(books[0].name, "斗破苍穹");
    assert_eq!(books[0].author, "天蚕土豆");
    assert_ne!(books[0].book_url, books[1].book_url);
    for (book, station) in books.iter().zip(["猫眼", "百度"]) {
        assert_eq!(book.last_chapter.as_deref().unwrap().trim(), station);
        let detail = analyze_url(&book.book_url, "", 1, &source.book_source_url, &source).unwrap();
        assert_eq!(detail.response_type.as_deref(), Some("gydetail"));
        use base64::Engine;
        let bytes = base64::engine::general_purpose::STANDARD.decode(detail.url.split_once(',').unwrap().1).unwrap();
        let value: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(value["sources"], station);
        assert_eq!(value["tab"], "小说");
        assert_eq!(value["book_id"], "1");
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn aggregate_candidates_open_details_chapters_and_content() {
    let mut source = aggregate_source();
    source.rule_book_info = Some(serde_json::from_value(json!({
        "init":"<js>const detail=JSON.parse(java.hexDecodeToString(result)); JSON.stringify({data:{book_name:'斗破苍穹',author:'天蚕土豆',source:detail.sources}})</js>$.data",
        "name":"$.book_name", "author":"$.author", "intro":"<js>'来自'+result.source</js>"
    })).unwrap());
    source.rule_toc = Some(serde_json::from_value(json!({
        "chapterList":"<js>const detail=JSON.parse(java.hexDecodeToString(result)); JSON.stringify({data:[{title:'第一章',item_id:'1',book_id:detail.book_id,source:detail.sources},{title:'第二章',item_id:'2',book_id:detail.book_id,source:detail.sources}]})</js>$.data",
        "chapterName":"$.title",
        "chapterUrl":"<js>let chapter={book_id:result.book_id,item_id:result.item_id,source:result.source}; 'data:;base64,'+java.base64Encode(JSON.stringify(chapter))+',{\"type\":\"gycontent\"}'</js>"
    })).unwrap());
    source.rule_content = Some(serde_json::from_value(json!({
        "content":"<js>const chapter=JSON.parse(java.hexDecodeToString(result)); JSON.stringify({content:chapter.source+'第'+chapter.item_id+'章：这是一段用于验证聚合书源的章节正文。'})</js>$.content"
    })).unwrap());
    let root = std::env::temp_dir().join(format!("reader-search-open-{}", uuid::Uuid::new_v4()));
    let service = BookService::new(HttpClient::new(2, None).unwrap(), RuleEngine::new().unwrap(), FileCache::new(root.join("cache")), root.to_str().unwrap());
    let candidates = service.search_book("test", &source, "斗破苍穹", 2).await.unwrap();
    let mut all_chapter_urls = std::collections::HashSet::new();
    for (candidate, station) in candidates.iter().zip(["猫眼", "百度"]) {
        let info = service.get_book_info("test", &source, &candidate.book_url).await.unwrap();
        assert_eq!(info.name, "斗破苍穹");
        assert_eq!(info.author, "天蚕土豆");
        assert_eq!(info.intro.as_deref(), Some(format!("来自{station}").as_str()));
        let (chapters, _) = service.get_chapter_list_first_page("test", &source, info.toc_url.as_deref().unwrap()).await.unwrap();
        assert_eq!(chapters.iter().map(|chapter| chapter.title.as_str()).collect::<Vec<_>>(), ["第一章", "第二章"]);
        for chapter in chapters {
            assert!(all_chapter_urls.insert(chapter.url.clone()));
            let spec = analyze_url(&chapter.url, "", 1, &source.book_source_url, &source).unwrap();
            assert_eq!(spec.response_type.as_deref(), Some("gycontent"));
            let content = service.get_content("test", &candidate.book_url, &source, &chapter.url).await.unwrap();
            assert!(content.contains(station) && content.contains("章节正文"));
        }
    }
    assert_eq!(all_chapter_urls.len(), 4);
}

#[test]
fn json_field_scripts_receive_objects_and_keep_javascript_operators() {
    let mut source = aggregate_source();
    source.js_lib = None;
    source.rule_search.as_mut().unwrap().book_list = Some("$.data".to_string());
    source.rule_search.as_mut().unwrap().author = Some("@js:JSON.parse(result).author || '未知'".to_string());
    let body = r#"{"data":[{"book_id":"1","book_name":"斗破苍穹","source":"猫眼","author":"天蚕土豆"},{"book_id":"2","book_name":"斗破苍穹","source":"百度"}]}"#;
    take_js_logs();
    let books = RuleEngine::new().unwrap().search_books(&source, body, "https://fixture.test/search");
    assert_eq!(books.len(), 2);
    assert_eq!(books[0].author, "天蚕土豆");
    assert_eq!(books[1].author, "未知");
    assert!(books.iter().all(|book| book.book_url.starts_with("data:;base64,")), "{:?}", take_js_logs());
    assert_ne!(books[0].book_url, books[1].book_url);
    assert!(books.iter().all(|book| book.book_url.ends_with(",{\"type\":\"gydetail\"}")));
}

#[test]
fn javascript_list_prelude_can_feed_html_selectors() {
    let source = book_source_from_value(json!({
        "bookSourceUrl":"https://fixture.test", "bookSourceName":"HTML pipeline",
        "ruleSearch":{
            "bookList":"<js>result.replace('placeholder','斗破苍穹')</js>\nli",
            "name":"a@text", "bookUrl":"a@href"
        }
    })).unwrap();
    let books = RuleEngine::new().unwrap().search_books(&source, "<ul><li><a href='/1'>placeholder</a></li><li><a href='/2'>第二本</a></li></ul>", "https://fixture.test");
    assert_eq!(books.iter().map(|book| book.name.as_str()).collect::<Vec<_>>(), ["斗破苍穹", "第二本"]);
}

#[tokio::test]
async fn old_collapsed_source_cache_is_rebuilt_without_reimporting_sources() {
    let root = std::env::temp_dir().join(format!("reader-search-cache-{}", uuid::Uuid::new_v4()));
    let service = BookService::new(HttpClient::new(2, None).unwrap(), RuleEngine::new().unwrap(), FileCache::new(root.join("cache")), root.to_str().unwrap());
    let book_url = "https://fixture.test/book/1";
    let old_dir = root.join("data").join("test").join("book_sources");
    tokio::fs::create_dir_all(&old_dir).await.unwrap();
    use md5::{Digest, Md5};
    let name = format!("{:x}", Md5::digest(book_url.as_bytes()));
    tokio::fs::write(old_dir.join(format!("{name}.json")), "[]").await.unwrap();
    assert!(service.load_book_sources_cache("test", book_url).await.unwrap().is_none());
    let candidates = vec![reader_rust::model::search::SearchBook {
        name: "斗破苍穹".to_string(), origin: "聚合测试".to_string(), book_url: "data:;base64,station-a".to_string(), ..Default::default()
    }, reader_rust::model::search::SearchBook {
        name: "斗破苍穹".to_string(), origin: "聚合测试".to_string(), book_url: "data:;base64,station-b".to_string(), ..Default::default()
    }];
    service.save_book_sources_cache("test", book_url, &candidates).await.unwrap();
    assert_eq!(service.load_book_sources_cache("test", book_url).await.unwrap().unwrap().len(), 2);
}

#[tokio::test]
async fn rejected_search_response_reports_status_instead_of_an_empty_success() {
    use axum::{http::StatusCode, routing::get, Router};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        axum::serve(listener, Router::new().route("/search", get(|| async { StatusCode::FORBIDDEN }))).await.unwrap();
    });
    let source = book_source_from_value(json!({
        "bookSourceUrl":base, "bookSourceName":"Search status fixture", "searchUrl":"/search",
        "ruleSearch":{"bookList":".item","name":"a@text","bookUrl":"a@href"}
    })).unwrap();
    let root = std::env::temp_dir().join(format!("reader-search-status-{}", uuid::Uuid::new_v4()));
    let service = BookService::new(HttpClient::new(2, None).unwrap(), RuleEngine::new().unwrap(), FileCache::new(root.join("cache")), root.to_str().unwrap());
    let error = service.search_book("test", &source, "斗破苍穹", 1).await.unwrap_err();
    assert!(error.to_string().contains("HTTP 403"));
    server.abort();
}

#[test]
fn repeated_title_links_and_empty_authors_preserve_all_matching_book_addresses() {
    let source = book_source_from_value(json!({
        "bookSourceUrl":"https://beta.example.com", "bookSourceName":"列表结构",
        "ruleSearch":{"bookList":".hotcontent .item||.novelslist2 li||dl", "name":"a@title||h3@text||a@text", "author":".s4@text||p@text##作者：", "bookUrl":"a@href"}
    })).unwrap();
    let rows = ["femn", "cjqq", "vxel", "ofrv", "ozzb"].iter().map(|id|format!(
        "<div class='generated item'><div class='image'><a href='/txt/{id}/' title='斗破苍穹'><img></a></div><dl><dt><a href='/txt/{id}/' title='斗破苍穹'>斗破苍穹</a></dt><div class='btm'>目录信息</div></dl></div>"
    )).collect::<String>();
    let books = RuleEngine::new().unwrap().search_books(&source, &format!("<div class='hotcontent'>{rows}</div>"), "https://beta.example.com/search/");
    assert_eq!(books.len(), 5);
    assert!(books.iter().all(|book| book.name == "斗破苍穹" && book.author.is_empty()));
    assert_eq!(books.iter().map(|book| &book.book_url).collect::<std::collections::HashSet<_>>().len(), 5);
}

/// 与 Legado `AnalyzeByJSoup.getElements` 相同的 CSS 列表选择语义。
/// 同一分类页的推荐卡片规则只命中 6 项，完整列表规则命中 44 项。
#[test]
fn explore_css_list_selection_matches_legado_jsoup() {
    let body = r#"
<div id="hotcontent"><div class="l rank">
<div class="item">推荐一</div><div class="item">推荐二</div><div class="item">推荐三</div>
<div class="item">推荐四</div><div class="item">推荐五</div><div class="item">推荐六</div>
</div></div>
<div id="newscontent"><ul>
<li>列表一</li><li>列表二</li><li>列表三</li><li>列表四</li><li>列表五</li><li>列表六</li>
<li>列表七</li><li>列表八</li><li>列表九</li><li>列表十</li><li>列表十一</li><li>列表十二</li>
<li>列表十三</li><li>列表十四</li><li>列表十五</li><li>列表十六</li><li>列表十七</li><li>列表十八</li>
<li>列表十九</li><li>列表二十</li><li>列表二十一</li><li>列表二十二</li><li>列表二十三</li><li>列表二十四</li>
<li>列表二十五</li><li>列表二十六</li><li>列表二十七</li><li>列表二十八</li><li>列表二十九</li><li>列表三十</li>
<li>列表三十一</li><li>列表三十二</li><li>列表三十三</li><li>列表三十四</li><li>列表三十五</li><li>列表三十六</li>
<li>列表三十七</li><li>列表三十八</li><li>列表三十九</li><li>列表四十</li><li>列表四十一</li><li>列表四十二</li>
<li>列表四十三</li><li>列表四十四</li>
</ul></div>
"#;
    let doc = reader_rust::parser::html::parse_document(&body);
    let recommended =
        reader_rust::parser::html::select_list(&doc, "#hotcontent .l.rank .item");
    let complete = reader_rust::parser::html::select_list(&doc, "#newscontent ul li");
    assert_eq!(recommended.len(), 6);
    assert_eq!(complete.len(), 44);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "reads the user-supplied source export and contacts its upstream search endpoints"]
async fn exported_sources_live_search() {
    let path = std::env::var("READER_SEARCH_SOURCE_FILE").unwrap_or_else(|_| "../reader-book-sources-20261010-154259.json".to_string());
    let values: Vec<Value> = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    let root = std::env::temp_dir().join(format!("reader-live-search-{}", uuid::Uuid::new_v4()));
    let http = HttpClient::new(20, None).unwrap();
    let service = BookService::new(http, RuleEngine::new().unwrap(), FileCache::new(root.join("cache")), root.to_str().unwrap());
    let mut tasks = tokio::task::JoinSet::new();
    for value in values {
        let source = book_source_from_value(value).unwrap();
        let service = service.clone();
        tasks.spawn(async move {
            let start = std::time::Instant::now();
            let result = tokio::time::timeout(std::time::Duration::from_secs(120), service.search_book("probe", &source, "斗破苍穹", 1)).await;
            let report = match result {
                Ok(Ok(books)) => json!({"source":source.book_source_name,"count":books.len(),"exactTitleCount":books.iter().filter(|book| book.name.split_whitespace().collect::<String>()=="斗破苍穹").count(),"authors":books.iter().filter(|book| book.name.split_whitespace().collect::<String>()=="斗破苍穹").map(|book| book.author.clone()).collect::<Vec<_>>(),"elapsedMs":start.elapsed().as_millis()}),
                Ok(Err(err)) => json!({"source":source.book_source_name,"error":format!("{err}").chars().take(180).collect::<String>(),"elapsedMs":start.elapsed().as_millis()}),
                Err(_) => json!({"source":source.book_source_name,"error":"search timed out after 120 seconds"}),
            };
            println!("SEARCH_REPORT {report}");
        });
    }
    while let Some(result) = tasks.join_next().await { result.unwrap(); }
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "inspects public HTML responses from the user-supplied source export"]
async fn exported_sources_http_diagnostics() {
    let values: Vec<Value> = serde_json::from_str(&std::fs::read_to_string("../reader-book-sources-20261010-154259.json").unwrap()).unwrap();
    let root = std::env::temp_dir().join(format!("reader-search-http-{}", uuid::Uuid::new_v4()));
    let http = HttpClient::new(20, None).unwrap();
    let direct = http.client_direct().unwrap();
    println!("NETWORK_REPORT {}", json!({"testUsesSystemProxy":http.active_proxy().is_some()}));
    let service = BookService::new(http, RuleEngine::new().unwrap(), FileCache::new(root.join("cache")), root.to_str().unwrap());
    for index in [1, 2] {
        let source = book_source_from_value(values[index].clone()).unwrap();
        if let (1, Ok(body)) = (index, std::fs::read_to_string("../.tmp-issue7/live-1-search.html")) {
            let books = RuleEngine::new().unwrap().search_books(&source, &body, "https://beta.example.com/search/");
            println!("FIXTURE_REPORT {}", json!({"source":source.book_source_name,"count":books.len(),"exactTitleCount":books.iter().filter(|b|b.name=="斗破苍穹").count()}));
        }
        match service.debug_source_step("probe", &source, "search", "斗破苍穹", "", "").await {
            Ok(trace) => {
                let doc = reader_rust::parser::html::parse_document(&trace.body);
                let title = reader_rust::parser::html::select_text(&doc,"title@text");
                let selector = source.rule_search.as_ref().unwrap().book_list.as_deref().unwrap();
                let count = reader_rust::parser::html::select_list(&doc,selector).len();
                std::fs::create_dir_all(&root).unwrap();
                let path = root.join(format!("source-{index}.html"));
                std::fs::write(&path,&trace.body).unwrap();
                println!("HTTP_REPORT {}", json!({"source":source.book_source_name,"status":trace.status,"url":trace.request_url,"title":title,"rows":count,"parsed":trace.result.as_array().map(Vec::len),"bodyBytes":trace.body.len(),"warnings":trace.warnings,"bodyFile":path}));
            }
            Err(err) => println!("HTTP_REPORT {}",json!({"source":source.book_source_name,"error":format!("{err}")})),
        }
        let headers = reader_rust::crawler::url_analyzer::parse_source_headers(source.header.as_deref().unwrap_or(""));
        if index == 1 {
            let mut prime = direct.get(&source.book_source_url);
            for (key,value) in &headers { prime = prime.header(key,value); }
            let _ = prime.send().await;
        }
        let mut request = if index == 1 {
            direct.post("https://beta.example.com/search/").header("Content-Type","application/x-www-form-urlencoded").body(format!("searchkey={}",urlencoding::encode("斗破苍穹")))
        } else {
            direct.get(format!("https://m.gamma.example.com/sou?wd={}",urlencoding::encode("斗破苍穹")))
        };
        for (key,value) in &headers { request = request.header(key,value); }
        match request.send().await {
            Ok(response) => {
                let status = response.status().as_u16();
                let url = response.url().to_string();
                let body = response.text().await.unwrap();
                let books = RuleEngine::new().unwrap().search_books(&source, &body, &url);
                println!("DIRECT_REPORT {}",json!({"source":source.book_source_name,"status":status,"count":books.len(),"exactTitleCount":books.iter().filter(|book|book.name=="斗破苍穹").count()}));
            }
            Err(err) => println!("DIRECT_REPORT {}",json!({"source":source.book_source_name,"error":format!("{err}")})),
        }
    }
}
