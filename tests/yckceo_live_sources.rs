use reader_rust::crawler::http_client::HttpClient;
use reader_rust::model::book_source::{book_source_from_value, BookSource};
use reader_rust::parser::rule_engine::RuleEngine;
use reader_rust::service::book_service::BookService;
use reader_rust::storage::cache::file_cache::FileCache;
use regex::Regex;
use serde_json::Value;
use std::collections::HashSet;
use std::env;
use std::time::Duration;
use tokio::time::sleep;
use uuid::Uuid;

const YCKCEO_INDEX: &str = "https://www.yckceo.com/yuedu/shuyuan/index.html";
const YCKCEO_JSON_PREFIX: &str = "https://www.yckceo.com/yuedu/shuyuan/json/id";
const YCKCEO_SMOKE_SEED_IDS: &[&str] = &["7124", "6990", "6931"];

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "hits the live YCKCeo source repository and public book sites"]
async fn yckceo_live_non_webview_sources_end_to_end() {
    let target_passes = env_usize("YCKCEO_TARGET_PASSES", 3);
    let max_candidates = env_usize("YCKCEO_MAX_CANDIDATES", 24);
    let pages = env_usize("YCKCEO_INDEX_PAGES", 1);

    let http = HttpClient::new(20, None).expect("http client");
    let ids = fetch_yckceo_ids(http.client(), pages)
        .await
        .expect("fetch YCKCeo index");
    assert!(!ids.is_empty(), "YCKCeo index returned no source ids");

    let cache_root = env::temp_dir().join(format!("reader-rust-yckceo-{}", Uuid::new_v4()));
    let service = BookService::new(
        http.clone(),
        RuleEngine::new().expect("rule engine"),
        FileCache::new(cache_root.join("cache")),
        cache_root.to_string_lossy().as_ref(),
    );

    let mut attempted = Vec::new();
    let mut passed = Vec::new();

    for id in ids {
        if attempted.len() >= max_candidates || passed.len() >= target_passes {
            break;
        }

        let Some((source, keyword)) = fetch_candidate_source(http.client(), &id)
            .await
            .unwrap_or_else(|err| {
                attempted.push(format!("{id}: fetch/import failed: {err}"));
                None
            })
        else {
            continue;
        };

        attempted.push(format!("{} ({})", source.book_source_name, id));
        match smoke_source(&service, &source, &keyword).await {
            Ok(pass) => {
                println!(
                    "PASS {} ({id}): keyword={}, book={}, chapters={}, content_chars={}",
                    source.book_source_name,
                    keyword,
                    pass.book_name,
                    pass.chapter_count,
                    pass.content_chars
                );
                passed.push(pass);
            }
            Err(err) => {
                println!("FAIL {} ({id}): {err}", source.book_source_name);
            }
        }

        sleep(Duration::from_millis(300)).await;
    }

    assert!(
        passed.len() >= target_passes,
        "expected at least {target_passes} YCKCeo non-WebView live sources to pass, got {}. attempted: {:?}",
        passed.len(),
        attempted
    );
}

async fn fetch_yckceo_ids(client: &reqwest::Client, pages: usize) -> anyhow::Result<Vec<String>> {
    let id_re = Regex::new(r"/yuedu/shuyuan/content/id/(\d+)\.html")?;
    let mut ids = Vec::new();
    let mut seen = HashSet::new();
    for id in YCKCEO_SMOKE_SEED_IDS {
        if seen.insert((*id).to_string()) {
            ids.push((*id).to_string());
        }
    }
    for page in 1..=pages.max(1) {
        let url = if page == 1 {
            YCKCEO_INDEX.to_string()
        } else {
            format!("{YCKCEO_INDEX}?page={page}")
        };
        let html = client
            .get(url)
            .send()
            .await?
            .error_for_status()?
            .text()
            .await?;
        for cap in id_re.captures_iter(&html) {
            let id = cap[1].to_string();
            if seen.insert(id.clone()) {
                ids.push(id);
            }
        }
    }
    Ok(ids)
}

async fn fetch_candidate_source(
    client: &reqwest::Client,
    id: &str,
) -> anyhow::Result<Option<(BookSource, String)>> {
    let url = format!("{YCKCEO_JSON_PREFIX}/{id}.json");
    let value: Value = client
        .get(url)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    let raw_source = value
        .as_array()
        .and_then(|items| items.first())
        .cloned()
        .unwrap_or(value);
    if !is_live_smoke_candidate(&raw_source) {
        return Ok(None);
    }

    let source = book_source_from_value(raw_source)?;
    let keyword = source
        .rule_search
        .as_ref()
        .and_then(|rule| rule.check_key_word.as_deref())
        .map(str::trim)
        .filter(|key| !key.is_empty())
        .unwrap_or("斗破苍穹")
        .to_string();

    Ok(Some((source, keyword)))
}

fn is_live_smoke_candidate(value: &Value) -> bool {
    let source_type = value
        .get("bookSourceType")
        .and_then(Value::as_i64)
        .unwrap_or(0);
    if source_type != 0 {
        return false;
    }

    let name = value
        .get("bookSourceName")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let blocked_names = [
        "🔞",
        "成人",
        "肉文",
        "第一版主",
        "禁忌",
        "涩",
        "H动漫",
        "91",
        "Porna",
        "cool18",
        "漫画",
        "听书",
        "订阅源",
    ];
    if blocked_names.iter().any(|marker| name.contains(marker)) {
        return false;
    }

    for key in [
        "searchUrl",
        "ruleSearch",
        "ruleBookInfo",
        "ruleToc",
        "ruleContent",
    ] {
        if value.get(key).is_none_or(Value::is_null) {
            return false;
        }
    }

    let raw = value.to_string().to_lowercase();
    let blocked_markers = [
        "webview",
        "startbrowser",
        "org.jsoup",
        "java.",
        "cookie.",
        "@js",
        "<js>",
        "@get",
        "@put",
        "logincheckjs",
    ];
    !blocked_markers
        .iter()
        .any(|marker| raw.contains(&marker.to_lowercase()))
}

struct SmokePass {
    book_name: String,
    chapter_count: usize,
    content_chars: usize,
}

async fn smoke_source(
    service: &BookService,
    source: &BookSource,
    keyword: &str,
) -> Result<SmokePass, String> {
    let books = service
        .search_book("yckceo-live", source, keyword, 1)
        .await
        .map_err(|err| format!("search failed: {err:?}"))?;
    let book = books
        .into_iter()
        .find(|book| !book.name.trim().is_empty() && !book.book_url.trim().is_empty())
        .ok_or_else(|| "search returned no usable books".to_string())?;

    let info = service
        .get_book_info("yckceo-live", source, &book.book_url)
        .await
        .map_err(|err| format!("book info failed for {}: {err:?}", book.book_url))?;
    let toc_url = info
        .toc_url
        .clone()
        .filter(|url| !url.trim().is_empty())
        .unwrap_or_else(|| book.book_url.clone());
    let (chapters, _) = service
        .get_chapter_list_first_page("yckceo-live", source, &toc_url)
        .await
        .map_err(|err| format!("toc failed for {toc_url}: {err:?}"))?;
    let chapter = chapters
        .iter()
        .find(|chapter| {
            !chapter.is_volume && !chapter.title.trim().is_empty() && !chapter.url.trim().is_empty()
        })
        .ok_or_else(|| "toc returned no readable chapter".to_string())?;
    let content = service
        .get_content("yckceo-live", &book.book_url, source, &chapter.url)
        .await
        .map_err(|err| format!("content failed for {}: {err:?}", chapter.url))?;
    let content_chars = content.trim().chars().count();
    if content_chars < 20 {
        return Err(format!(
            "content too short for {}: {content_chars} chars",
            chapter.url
        ));
    }

    Ok(SmokePass {
        book_name: if info.name.trim().is_empty() {
            book.name
        } else {
            info.name
        },
        chapter_count: chapters.len(),
        content_chars,
    })
}

fn env_usize(key: &str, default: usize) -> usize {
    env::var(key)
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(default)
}
