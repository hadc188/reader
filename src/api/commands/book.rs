use crate::api::AppState;
use crate::error::error::{ApiResponse, AppError};
use crate::model::{book::Book, book_source::BookSource, search::SearchBook};
use crate::service::local_epub_book::{
    is_local_epub_origin, is_local_epub_url, LOCAL_EPUB_ORIGIN, MAX_EPUB_UPLOAD_BYTES,
};
use crate::service::local_txt_book::{
    is_local_txt_origin, is_local_txt_url, LOCAL_TXT_ORIGIN, MAX_TXT_UPLOAD_BYTES,
};
use crate::util::text::{normalize_source_url, repair_encoded_url};
use futures::stream::FuturesUnordered;
use futures::StreamExt;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::task::JoinSet;

const DEFAULT_AVAILABLE_RESULT_LIMIT: usize = 20;
const MAX_AVAILABLE_RESULT_LIMIT: usize = 100;
const DEFAULT_AVAILABLE_CONCURRENT_COUNT: usize = 16;
const MAX_AVAILABLE_CONCURRENT_COUNT: usize = 20;
const AVAILABLE_SOURCE_SSE_RESULT_LIMIT: usize = 20;

#[derive(Debug, Deserialize)]
pub struct SearchBookRequest {
    key: Option<String>,
    page: Option<i32>,
    #[serde(rename = "bookSourceUrl")]
    book_source_url: Option<String>,
    #[serde(rename = "bookSource")]
    book_source: Option<BookSource>,
}

#[derive(Debug, Deserialize)]
pub struct SearchBookMultiRequest {
    key: Option<String>,
    page: Option<i32>,
    #[serde(rename = "bookSourceUrls")]
    book_source_urls: Option<Vec<String>>,
    #[serde(rename = "bookSourceGroup")]
    book_source_group: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ExploreBookRequest {
    #[serde(rename = "ruleFindUrl")]
    rule_find_url: Option<String>,
    page: Option<i32>,
    #[serde(rename = "bookSourceUrl")]
    book_source_url: Option<String>,
    #[serde(rename = "bookSource")]
    book_source: Option<BookSource>,
}
#[derive(Debug, Deserialize)]
pub struct BookInfoRequest {
    pub url: Option<String>,
    #[serde(rename = "bookSourceUrl", alias = "origin")]
    pub book_source_url: Option<String>,
    #[serde(rename = "bookSource")]
    pub book_source: Option<BookSource>,
}

#[derive(Debug, Deserialize)]
pub struct ChapterListRequest {
    #[serde(rename = "tocUrl")]
    pub toc_url: Option<String>,
    #[serde(rename = "bookUrl", alias = "url")]
    pub book_url: Option<String>,
    #[serde(rename = "bookSourceUrl", alias = "origin")]
    pub book_source_url: Option<String>,
    #[serde(rename = "bookSource")]
    pub book_source: Option<BookSource>,
    pub refresh: Option<i32>,
}

#[derive(Debug, Deserialize)]
pub struct BookContentRequest {
    #[serde(rename = "chapterUrl", alias = "url", alias = "href")]
    pub chapter_url: Option<String>,
    #[serde(rename = "bookSourceUrl", alias = "origin")]
    pub book_source_url: Option<String>,
    #[serde(rename = "bookSource")]
    pub book_source: Option<BookSource>,
    pub index: Option<i32>,
    pub refresh: Option<i32>,
}

#[derive(Debug, Deserialize)]
pub struct DeleteCacheRequest {
    #[serde(rename = "chapterUrl")]
    chapter_url: Option<String>,
    url: Option<String>,
    #[serde(rename = "bookUrl")]
    book_url: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct SaveBookProgressRequest {
    url: Option<String>,
    #[serde(rename = "bookUrl")]
    book_url: Option<String>,
    index: Option<i32>,
    position: Option<i32>,
    #[serde(rename = "searchBook")]
    search_book: Option<SearchBookRef>,
}

#[derive(Debug, Deserialize)]
pub struct SearchBookRef {
    #[serde(rename = "bookUrl")]
    book_url: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct GetShelfBookRequest {
    url: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct CacheBookRequest {
    url: Option<String>,
    #[serde(rename = "bookUrl")]
    book_url: Option<String>,
    #[serde(rename = "tocUrl")]
    toc_url: Option<String>,
    count: Option<i32>,
    refresh: Option<i32>,
    #[serde(rename = "concurrentCount")]
    concurrent_count: Option<i32>,
}

#[derive(Debug, Deserialize)]
pub struct SearchBookMultiSseRequest {
    key: Option<String>,
    #[serde(rename = "bookSourceUrl")]
    book_source_url: Option<String>,
    #[serde(rename = "bookSourceGroup")]
    book_source_group: Option<String>,
    #[serde(rename = "lastIndex")]
    last_index: Option<i32>,
    #[serde(rename = "searchSize")]
    search_size: Option<i32>,
    #[serde(rename = "concurrentCount")]
    concurrent_count: Option<i32>,
}

#[derive(Debug, Deserialize)]
pub struct SearchBookSourceSseRequest {
    url: Option<String>,
    #[serde(rename = "bookSourceGroup")]
    book_source_group: Option<String>,
    #[serde(rename = "lastIndex")]
    last_index: Option<i32>,
    #[serde(rename = "searchSize")]
    search_size: Option<i32>,
    refresh: Option<i32>,
}

#[derive(Debug, Deserialize)]
pub struct BookSourceDebugRequest {
    #[serde(rename = "bookSourceUrl")]
    book_source_url: Option<String>,
    keyword: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct GetAvailableBookSourceRequest {
    url: Option<String>,
    name: Option<String>,
    author: Option<String>,
    #[serde(alias = "bookSourceUrl")]
    origin: Option<String>,
    refresh: Option<i32>,
    #[serde(rename = "lastIndex")]
    last_index: Option<i32>,
    #[serde(rename = "resultLimit")]
    result_limit: Option<i32>,
    #[serde(rename = "concurrentCount")]
    concurrent_count: Option<i32>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AvailableBookSourceResponse {
    books: Vec<SearchBook>,
    last_index: i32,
    has_more: bool,
}

#[derive(Debug, Deserialize, Default)]
pub struct SetBookSourceRequest {
    #[serde(rename = "bookUrl", alias = "url")]
    book_url: Option<String>,
    #[serde(rename = "newUrl")]
    new_url: Option<String>,
    #[serde(rename = "bookSourceUrl")]
    book_source_url: Option<String>,
    // Fallback book metadata, used when the book is not yet on the shelf.
    name: Option<String>,
    author: Option<String>,
    #[serde(rename = "coverUrl")]
    cover_url: Option<String>,
    intro: Option<String>,
    kind: Option<String>,
    #[serde(rename = "latestChapterTitle")]
    latest_chapter_title: Option<String>,
    #[serde(rename = "durChapterIndex")]
    dur_chapter_index: Option<i32>,
    #[serde(rename = "durChapterTitle")]
    dur_chapter_title: Option<String>,
    #[serde(rename = "durChapterPos")]
    dur_chapter_pos: Option<i32>,
    #[serde(rename = "durChapterTime")]
    dur_chapter_time: Option<i64>,
}

#[tauri::command]
pub async fn search_book(
    state: tauri::State<'_, AppState>,
    req: SearchBookRequest,
) -> Result<ApiResponse<serde_json::Value>, AppError> {
    let user_ns = "default";
    let key = req
        .key
        .ok_or_else(|| AppError::BadRequest("key required".to_string()))?;
    let page = req.page.unwrap_or(1);
    let source =
        resolve_book_source(&state, &user_ns, req.book_source_url, req.book_source, None).await?;
    let books = state
        .book_service
        .search_book(&user_ns, &source, &key, page)
        .await
        .map_err(|e| {
            tracing::error!("search_book failed: {:?}", e);
            e
        })?;
    Ok(ApiResponse::ok(
        serde_json::to_value(books).unwrap_or_default(),
    ))
}

#[tauri::command]
pub async fn search_book_multi(
    state: tauri::State<'_, AppState>,
    req: SearchBookMultiRequest,
) -> Result<ApiResponse<serde_json::Value>, AppError> {
    let user_ns = "default";
    let key = req
        .key
        .ok_or_else(|| AppError::BadRequest("key required".to_string()))?;
    let page = req.page.unwrap_or(1);

    let sources = if let Some(urls) = req.book_source_urls {
        let mut out = Vec::new();
        for url in urls {
            if let Some(s) = state.book_source_service.get(&user_ns, &url).await? {
                out.push(s);
            }
        }
        out
    } else {
        let mut list = state.book_source_service.list(&user_ns).await?;
        if let Some(ref group) = req.book_source_group {
            list.retain(|s| s.book_source_group.as_deref().unwrap_or("").contains(group));
        }
        list
    };

    let mut tasks = Vec::new();
    for source in sources {
        let svc = state.book_service.clone();
        let k = key.clone();
        let user_ns = user_ns;
        tasks.push(tokio::spawn(async move {
            svc.search_book(&user_ns, &source, &k, page).await
        }));
    }
    let mut results: Vec<crate::model::search::SearchBook> = Vec::new();
    for t in tasks {
        if let Ok(Ok(list)) = t.await {
            results.extend(list);
        }
    }

    // Merge books with same name and author
    let merged = merge_search_results(results, &key);

    Ok(ApiResponse::ok(
        serde_json::to_value(merged).unwrap_or_default(),
    ))
}

/// Merge search results from different book sources for the same book.
/// Dedup key is the normalized `name|author` (see `SearchBook::merge_key`).
/// Results are ranked Legado-style: exact title/author match first, then
/// kind-tag match, then substring match, then everything else; within each
/// tier, books found in more sources rank higher.
fn merge_search_results(
    results: Vec<crate::model::search::SearchBook>,
    search_key: &str,
) -> Vec<crate::model::search::SearchBook> {
    use crate::model::search::SearchBook;
    use std::collections::HashMap;

    let mut merged: HashMap<String, SearchBook> = HashMap::new();

    for book in results {
        let key = book.merge_key();

        if let Some(existing) = merged.get_mut(&key) {
            // Add this source to the existing book
            if let Some(ref mut urls) = existing.book_source_urls {
                if !urls.contains(&book.origin) {
                    urls.push(book.origin.clone());
                }
            } else {
                existing.book_source_urls =
                    Some(vec![existing.origin.clone(), book.origin.clone()]);
            }

            // Fill in missing fields from this source
            if existing.cover_url.is_none() && book.cover_url.is_some() {
                existing.cover_url = book.cover_url;
            }
            if existing.intro.is_none() && book.intro.is_some() {
                existing.intro = book.intro;
            }
            if existing.kind.is_none() && book.kind.is_some() {
                existing.kind = book.kind;
            }
            if existing.last_chapter.is_none() && book.last_chapter.is_some() {
                existing.last_chapter = book.last_chapter;
            }
            if existing.update_time.is_none() && book.update_time.is_some() {
                existing.update_time = book.update_time;
            }
        } else {
            merged.insert(key, book);
        }
    }

    let mut result: Vec<SearchBook> = merged.into_values().collect();
    result.sort_by(|a, b| search_result_rank(a, b, search_key));
    result
}

/// Legado-style relevance rank: exact title/author match > kind-tag match >
/// substring match > other; within a tier, more sources wins.
fn search_result_rank(
    a: &crate::model::search::SearchBook,
    b: &crate::model::search::SearchBook,
    search_key: &str,
) -> std::cmp::Ordering {
    let tier = |book: &crate::model::search::SearchBook| -> u8 {
        let name = book.name.trim();
        let author = book.author.trim();
        if name == search_key || author == search_key {
            0
        } else if book.kind.as_deref().map(|k| k.contains(search_key)).unwrap_or(false) {
            1
        } else if name.contains(search_key) || author.contains(search_key) {
            2
        } else {
            3
        }
    };
    let ta = tier(a);
    let tb = tier(b);
    ta.cmp(&tb).then_with(|| {
        let na = a.book_source_urls.as_ref().map(|u| u.len()).unwrap_or(1);
        let nb = b.book_source_urls.as_ref().map(|u| u.len()).unwrap_or(1);
        nb.cmp(&na)
    })
}

#[tauri::command]
pub async fn explore_book(
    state: tauri::State<'_, AppState>,
    req: ExploreBookRequest,
) -> Result<ApiResponse<serde_json::Value>, AppError> {
    let user_ns = "default";
    let rule_find_url = req
        .rule_find_url
        .ok_or_else(|| AppError::BadRequest("ruleFindUrl required".to_string()))?;
    let page = req.page.unwrap_or(1);
    let source = resolve_book_source(
        &state,
        &user_ns,
        req.book_source_url,
        req.book_source,
        Some(&rule_find_url),
    )
    .await?;
    let list = state
        .book_service
        .explore_book(&user_ns, &source, &rule_find_url, page)
        .await?;
    Ok(ApiResponse::ok(
        serde_json::to_value(list).unwrap_or_default(),
    ))
}

#[tauri::command]
pub async fn get_book_info(
    state: tauri::State<'_, AppState>,
    req: BookInfoRequest,
) -> Result<ApiResponse<serde_json::Value>, AppError> {
    let user_ns = "default";

    let url = req
        .url
        .ok_or_else(|| AppError::BadRequest("url required".to_string()))?;
    let url = repair_encoded_url(&url);
    if is_local_txt_url(&url)
        || req
            .book_source_url
            .as_deref()
            .is_some_and(is_local_txt_origin)
    {
        let book = state
            .local_txt_book_service
            .get_book_info(&user_ns, &url)
            .await?;
        return Ok(ApiResponse::ok(
            serde_json::to_value(book).unwrap_or_default(),
        ));
    }
    if is_local_epub_url(&url)
        || req
            .book_source_url
            .as_deref()
            .is_some_and(is_local_epub_origin)
    {
        let book = state
            .local_epub_book_service
            .get_book_info(&user_ns, &url)
            .await?;
        return Ok(ApiResponse::ok(
            serde_json::to_value(book).unwrap_or_default(),
        ));
    }
    let source = resolve_book_source(
        &state,
        &user_ns,
        req.book_source_url,
        req.book_source,
        Some(&url),
    )
    .await?;
    let book = state
        .book_service
        .get_book_info(&user_ns, &source, &url)
        .await?;
    Ok(ApiResponse::ok(
        serde_json::to_value(book).unwrap_or_default(),
    ))
}

#[tauri::command]
pub async fn get_chapter_list(
    state: tauri::State<'_, AppState>,
    req: ChapterListRequest,
) -> Result<ApiResponse<serde_json::Value>, AppError> {
    let user_ns = "default";

    let do_refresh = req.refresh.unwrap_or(0) > 0;

    if req
        .book_source_url
        .as_deref()
        .is_some_and(is_local_txt_origin)
        || req.book_url.as_deref().is_some_and(is_local_txt_url)
        || req.toc_url.as_deref().is_some_and(is_local_txt_url)
    {
        let book_url = req
            .book_url
            .as_deref()
            .or(req.toc_url.as_deref())
            .ok_or_else(|| AppError::BadRequest("tocUrl or bookUrl required".to_string()))?;
        let chapters = state
            .local_txt_book_service
            .get_chapter_list(&user_ns, &repair_encoded_url(book_url))
            .await?;
        return Ok(ApiResponse::ok(
            serde_json::to_value(chapters).unwrap_or_default(),
        ));
    }

    if req
        .book_source_url
        .as_deref()
        .is_some_and(is_local_epub_origin)
        || req.book_url.as_deref().is_some_and(is_local_epub_url)
        || req.toc_url.as_deref().is_some_and(is_local_epub_url)
    {
        let book_url = req
            .book_url
            .as_deref()
            .or(req.toc_url.as_deref())
            .ok_or_else(|| AppError::BadRequest("tocUrl or bookUrl required".to_string()))?;
        let chapters = state
            .local_epub_book_service
            .get_chapter_list(&user_ns, &repair_encoded_url(book_url))
            .await?;
        return Ok(ApiResponse::ok(
            serde_json::to_value(chapters).unwrap_or_default(),
        ));
    }

    let source = resolve_book_source(
        &state,
        &user_ns,
        req.book_source_url.clone(),
        req.book_source.clone(),
        req.book_url.as_deref().or(req.toc_url.as_deref()),
    )
    .await?;
    let toc_url = if let Some(u) = req.toc_url {
        repair_encoded_url(&u)
    } else if let Some(book_url) = req.book_url {
        let book_url = repair_encoded_url(&book_url);
        let book = state
            .book_service
            .get_book_info(&user_ns, &source, &book_url)
            .await?;
        repair_encoded_url(book.toc_url.as_deref().unwrap_or(&book_url))
    } else {
        return Err(AppError::BadRequest(
            "tocUrl or bookUrl required".to_string(),
        ));
    };

    // Check if we have cached chapters
    if do_refresh {
        let _ = state
            .book_service
            .delete_chapter_list_cache(&user_ns, &toc_url)
            .await;
    }

    if !do_refresh {
        if let Ok(Some(cached)) = state
            .book_service
            .load_chapter_list_cache(&user_ns, &toc_url)
            .await
        {
            if !cached.is_empty() {
                return Ok(ApiResponse::ok(
                    serde_json::to_value(cached).unwrap_or_default(),
                ));
            }
        }
    }

    // Get first page of chapters
    let (chapters, pagination) = state
        .book_service
        .get_chapter_list_first_page(&user_ns, &source, &toc_url)
        .await?;
    tracing::info!(
        "get_chapter_list first page: toc_url={} chapters={} pending_urls={}",
        toc_url,
        chapters.len(),
        pagination.pending_urls.len()
    );

    // Save first page to cache immediately
    let _ = state
        .book_service
        .save_chapter_list_cache(&user_ns, &toc_url, &chapters)
        .await;

    // If there are more pages to fetch, do it in background
    if !pagination.pending_urls.is_empty() {
        let state_clone = state.inner().clone();
        let user_ns_clone = user_ns;
        let toc_url_clone = toc_url.clone();

        tokio::spawn(async move {
            tracing::debug!("starting background chapter fetch");
            match state_clone
                .book_service
                .fetch_remaining_chapters(pagination)
                .await
            {
                Ok(remaining) => {
                    if !remaining.is_empty() {
                        // Append to cache
                        match state_clone
                            .book_service
                            .append_chapter_list_cache(&user_ns_clone, &toc_url_clone, &remaining)
                            .await
                        {
                            Ok(all_chapters) => {
                                tracing::debug!(
                                    "background chapter fetch complete: {}",
                                    all_chapters.len()
                                );
                            }
                            Err(e) => {
                                tracing::warn!(
                                    "failed to append background chapters to cache: {:?}",
                                    e
                                );
                            }
                        }
                    }
                }
                Err(e) => {
                    tracing::warn!("background chapter fetch failed: {:?}", e);
                }
            }
        });
    }

    Ok(ApiResponse::ok(
        serde_json::to_value(chapters).unwrap_or_default(),
    ))
}

#[tauri::command]
pub async fn get_book_content(
    state: tauri::State<'_, AppState>,
    req: BookContentRequest,
) -> Result<ApiResponse<serde_json::Value>, AppError> {
    let user_ns = "default";

    let do_refresh = req.refresh.unwrap_or(0) > 0;

    if req
        .book_source_url
        .as_deref()
        .is_some_and(is_local_txt_origin)
        || req.chapter_url.as_deref().is_some_and(is_local_txt_url)
    {
        let url = req
            .chapter_url
            .as_deref()
            .ok_or_else(|| AppError::BadRequest("chapterUrl required".to_string()))?;
        let chapter_url = if is_local_txt_url(url) && !url.contains('#') {
            let index = req.index.unwrap_or(0).max(0) as usize;
            format!(
                "{}#{}",
                repair_encoded_url(url).trim_end_matches('#'),
                index
            )
        } else {
            repair_encoded_url(url)
        };
        let content = state
            .local_txt_book_service
            .get_content(&user_ns, &chapter_url)
            .await?;
        return Ok(ApiResponse::ok(serde_json::Value::String(content)));
    }

    if req
        .book_source_url
        .as_deref()
        .is_some_and(is_local_epub_origin)
        || req.chapter_url.as_deref().is_some_and(is_local_epub_url)
    {
        let url = req
            .chapter_url
            .as_deref()
            .ok_or_else(|| AppError::BadRequest("chapterUrl required".to_string()))?;
        let chapter_url = if is_local_epub_url(url) && !url.contains('#') {
            let index = req.index.unwrap_or(0).max(0) as usize;
            format!(
                "{}#{}",
                repair_encoded_url(url).trim_end_matches('#'),
                index
            )
        } else {
            repair_encoded_url(url)
        };
        let content = state
            .local_epub_book_service
            .get_content(&user_ns, &chapter_url)
            .await?;
        return Ok(ApiResponse::ok(serde_json::Value::String(content)));
    }

    // Determine book_url and chapter_url
    let (book_url, chapter_url) = if let Some(url) = &req.chapter_url {
        // Check if url looks like a book URL (not a chapter URL) and we have an index
        if req.index.is_some() && !url.contains("/read/") && !url.contains("/chapter/") {
            // url is bookUrl, need to get chapter from index
            let source = resolve_book_source(
                &state,
                &user_ns,
                req.book_source_url.clone(),
                req.book_source.clone(),
                Some(url),
            )
            .await?;
            let book_info = state
                .book_service
                .get_book_info(&user_ns, &source, url)
                .await?;
            let toc_url = book_info.toc_url.as_deref().unwrap_or(url);

            // If refresh is requested, delete chapter list cache first
            if do_refresh {
                let _ = state
                    .book_service
                    .delete_chapter_list_cache(&user_ns, toc_url)
                    .await;
            }

            let mut chapters = state
                .book_service
                .get_chapter_list_with_cache(&user_ns, &source, toc_url, do_refresh)
                .await?;
            let idx = req.index.unwrap() as usize;

            if idx >= chapters.len() {
                // If index is out of range, it's possible our cache was partial (first page only).
                // Try a forced refresh to get the full list synchronously.
                tracing::info!(
                    "Index {} out of range (len={}). Attempting forced refresh for {}",
                    idx,
                    chapters.len(),
                    toc_url
                );
                chapters = state
                    .book_service
                    .get_chapter_list_with_cache(&user_ns, &source, toc_url, true)
                    .await?;

                if idx >= chapters.len() {
                    return Err(AppError::BadRequest(format!(
                        "chapter index out of range (max: {})",
                        chapters.len()
                    )));
                }
            }
            (url.clone(), chapters[idx].url.clone())
        } else {
            // url is chapterUrl, try to find book_url from shelf
            let book_url = if let Ok(Some(shelf_book)) = state
                .book_service
                .get_shelf_book_by_chapter(&user_ns, url)
                .await
            {
                shelf_book.book_url
            } else {
                url.clone() // fallback to using chapter url as book key
            };
            (book_url, url.clone())
        }
    } else {
        return Err(AppError::BadRequest("chapterUrl required".to_string()));
    };

    let source = resolve_book_source(
        &state,
        &user_ns,
        req.book_source_url,
        req.book_source,
        Some(&chapter_url),
    )
    .await?;

    // If refresh is requested, delete this chapter's cache before fetching
    if do_refresh {
        let _ = state
            .book_service
            .delete_book_cache(&user_ns, &book_url)
            .await;
    }

    let content = state
        .book_service
        .get_content(&user_ns, &book_url, &source, &chapter_url)
        .await?;
    Ok(ApiResponse::ok(serde_json::Value::String(content)))
}

#[tauri::command]
pub async fn delete_book_cache(
    state: tauri::State<'_, AppState>,
    req: DeleteCacheRequest,
) -> Result<ApiResponse<serde_json::Value>, AppError> {
    let user_ns = "default";

    // Get book_url (prefer bookUrl, fallback to url)
    let book_url = req
        .book_url
        .or(req.url)
        .ok_or_else(|| AppError::BadRequest("bookUrl required".to_string()))?;

    let mut deleted_chapter_list = false;

    // Delete all chapter content cache for this book
    let deleted_content = state
        .book_service
        .delete_book_cache(&user_ns, &book_url)
        .await?;

    // Try to delete chapter list cache by shelf book toc_url first, then book_url fallback
    let mut candidate_toc_urls = vec![book_url.clone()];
    if let Ok(Some(shelf_book)) = state.book_service.get_shelf_book(&user_ns, &book_url).await {
        if let Some(toc_url) = shelf_book.toc_url {
            if !candidate_toc_urls.contains(&toc_url) {
                candidate_toc_urls.push(toc_url);
            }
        }
    }

    for toc_url in candidate_toc_urls {
        if state
            .book_service
            .chapter_list_cache_exists(&user_ns, &toc_url)
            .await
        {
            state
                .book_service
                .delete_chapter_list_cache(&user_ns, &toc_url)
                .await?;
            deleted_chapter_list = true;
        }
    }

    let _ = state
        .book_service
        .delete_book_sources_cache(&user_ns, &book_url)
        .await;

    Ok(ApiResponse::ok(serde_json::json!({
        "deleted": true,
        "contentCache": deleted_content,
        "chapterListCache": deleted_chapter_list
    })))
}

#[tauri::command]
pub async fn get_bookshelf(
    state: tauri::State<'_, AppState>,
) -> Result<ApiResponse<serde_json::Value>, AppError> {
    let user_ns = "default";
    let list = state.book_service.get_bookshelf(&user_ns).await?;
    Ok(ApiResponse::ok(
        serde_json::to_value(list).unwrap_or_default(),
    ))
}

#[tauri::command]
pub async fn upload_txt_book(
    state: tauri::State<'_, AppState>,
    file: Vec<u8>,
    file_name: String,
) -> Result<ApiResponse<Value>, AppError> {
    let user_ns = "default";
    if file.len() > MAX_TXT_UPLOAD_BYTES {
        return Err(AppError::BadRequest("TXT 文件不能超过 50MB".to_string()));
    }
    let book = state
        .local_txt_book_service
        .import_txt_book(&user_ns, &file_name, &file)
        .await?;
    let saved = state.book_service.save_book(&user_ns, book).await?;
    Ok(ApiResponse::ok(
        serde_json::to_value(saved).unwrap_or_default(),
    ))
}

#[tauri::command]
pub async fn upload_epub_book(
    state: tauri::State<'_, AppState>,
    file: Vec<u8>,
    file_name: String,
) -> Result<ApiResponse<Value>, AppError> {
    let user_ns = "default";
    if file.len() > MAX_EPUB_UPLOAD_BYTES {
        return Err(AppError::BadRequest("EPUB 文件不能超过 80MB".to_string()));
    }
    let book = state
        .local_epub_book_service
        .import_epub_book(&user_ns, &file_name, &file)
        .await?;
    let saved = state.book_service.save_book(&user_ns, book).await?;
    Ok(ApiResponse::ok(
        serde_json::to_value(saved).unwrap_or_default(),
    ))
}

#[tauri::command]
pub async fn save_book(
    state: tauri::State<'_, AppState>,
    mut req: Book,
) -> Result<ApiResponse<Value>, AppError> {
    let user_ns = "default";
    if req.book_url.trim().is_empty() {
        return Err(AppError::BadRequest("bookUrl required".to_string()));
    }
    if req.origin.trim().is_empty() {
        return Err(AppError::BadRequest("origin required".to_string()));
    }

    req.book_url = repair_encoded_url(&req.book_url);
    req.origin = normalize_source_url(&req.origin);
    if let Some(toc_url) = &req.toc_url {
        req.toc_url = Some(repair_encoded_url(toc_url));
    }

    if req.toc_url.is_none() || req.name.trim().is_empty() {
        if let Some(source) = state
            .book_source_service
            .get(&user_ns, &req.origin)
            .await?
        {
            match state
                .book_service
                .get_book_info(&user_ns, &source, &req.book_url)
                .await
            {
                Ok(info) => {
                    tracing::info!(
                        "save_book get_book_info OK: name={} toc_url={:?}",
                        info.name,
                        info.toc_url
                    );
                    merge_book(&mut req, info);
                }
                Err(e) => {
                    tracing::warn!("save_book get_book_info FAILED: {e:?}");
                }
            }
        }
    }
    tracing::info!(
        "save_book final: name={} toc_url={:?} book_url={}",
        req.name,
        req.toc_url,
        req.book_url
    );

    let saved = state.book_service.save_book(&user_ns, req).await?;
    Ok(ApiResponse::ok(
        serde_json::to_value(saved).unwrap_or_default(),
    ))
}

#[tauri::command]
pub async fn save_books(
    state: tauri::State<'_, AppState>,
    mut req: Vec<Book>,
) -> Result<ApiResponse<Value>, AppError> {
    let user_ns = "default";

    for book in &mut req {
        if book.book_url.trim().is_empty() {
            return Err(AppError::BadRequest("bookUrl required".to_string()));
        }
        if book.origin.trim().is_empty() {
            return Err(AppError::BadRequest("origin required".to_string()));
        }

        book.book_url = repair_encoded_url(&book.book_url);
        book.origin = normalize_source_url(&book.origin);
        if let Some(toc_url) = &book.toc_url {
            book.toc_url = Some(repair_encoded_url(toc_url));
        }
    }

    let saved = state.book_service.save_books(&user_ns, req).await?;
    Ok(ApiResponse::ok(
        serde_json::to_value(saved).unwrap_or_default(),
    ))
}

#[tauri::command]
pub async fn set_book_source(
    state: tauri::State<'_, AppState>,
    req: SetBookSourceRequest,
) -> Result<ApiResponse<Value>, AppError> {
    let user_ns = "default";

    let old_book_url = req
        .book_url
        .filter(|v| !v.trim().is_empty())
        .ok_or_else(|| AppError::BadRequest("bookUrl required".to_string()))?;
    let new_book_url = req
        .new_url
        .filter(|v| !v.trim().is_empty())
        .ok_or_else(|| AppError::BadRequest("newUrl required".to_string()))?;
    let new_source_url = req
        .book_source_url
        .clone()
        .filter(|v| !v.trim().is_empty())
        .ok_or_else(|| AppError::BadRequest("bookSourceUrl required".to_string()))?;

    // Prefer the shelf record (preserves reading progress); otherwise build a
    // base Book from the request so switching works for books not yet shelved.
    let shelf_book = state
        .book_service
        .get_shelf_book(&user_ns, &old_book_url)
        .await?;
    let mut updated = match shelf_book {
        Some(b) => b,
        None => {
            let mut base = Book::default();
            base.book_url = old_book_url.clone();
            base.origin = req.book_source_url.clone().unwrap_or_default();
            base.name = req.name.clone().unwrap_or_default();
            base.author = req.author.clone().unwrap_or_default();
            base.cover_url = req.cover_url.clone();
            base.intro = req.intro.clone();
            base.kind = req.kind.clone();
            base.latest_chapter_title = req.latest_chapter_title.clone();
            base.dur_chapter_index = req.dur_chapter_index;
            base.dur_chapter_title = req.dur_chapter_title.clone();
            base.dur_chapter_pos = req.dur_chapter_pos;
            base.dur_chapter_time = req.dur_chapter_time;
            base
        }
    };
    let new_source = state
        .book_source_service
        .get(&user_ns, &new_source_url)
        .await?
        .ok_or_else(|| AppError::BadRequest("书源不存在".to_string()))?;

    updated.book_url = new_book_url.clone();
    updated.origin = new_source.book_source_url.clone();
    updated.origin_name = Some(new_source.book_source_name.clone());
    updated.toc_url = None;

    if let Some(candidates) = state
        .book_service
        .load_book_sources_cache(&user_ns, &old_book_url)
        .await?
    {
        if let Some(candidate) = candidates
            .into_iter()
            .find(|item| item.book_url == new_book_url)
        {
            if !candidate.name.trim().is_empty() {
                updated.name = candidate.name;
            }
            if !candidate.author.trim().is_empty() {
                updated.author = candidate.author;
            }
            updated.cover_url = candidate.cover_url.or(updated.cover_url);
            updated.intro = candidate.intro.or(updated.intro);
            updated.kind = candidate.kind.or(updated.kind);
            updated.latest_chapter_title = candidate.last_chapter.or(updated.latest_chapter_title);
        }
    }

    match state
        .book_service
        .get_book_info(&user_ns, &new_source, &new_book_url)
        .await
    {
        Ok(info) => merge_book(&mut updated, info),
        Err(err) => {
            tracing::warn!(
                "setBookSource: failed to refresh metadata for {} via {}: {:?}",
                new_book_url,
                new_source.book_source_url,
                err
            );
        }
    }

    let saved = state.book_service.save_book(&user_ns, updated).await?;
    if old_book_url != saved.book_url {
        let delete_old = Book {
            book_url: old_book_url,
            ..Book::default()
        };
        let _ = state.book_service.delete_book(&user_ns, &delete_old).await;
    }

    Ok(ApiResponse::ok(
        serde_json::to_value(saved).unwrap_or_default(),
    ))
}

#[tauri::command]
pub async fn delete_book(
    state: tauri::State<'_, AppState>,
    req: Book,
) -> Result<ApiResponse<Value>, AppError> {
    let user_ns = "default";
    let removed_books = find_matching_books(&state, &user_ns, std::slice::from_ref(&req)).await?;
    let deleted = state.book_service.delete_book(&user_ns, &req).await?;
    if !deleted {
        return Err(AppError::BadRequest("书架书籍不存在".to_string()));
    }
    cleanup_ai_book_memories(&state, &user_ns, &removed_books).await;
    cleanup_local_book_files(&state, &user_ns, &removed_books).await;
    Ok(ApiResponse::ok(serde_json::json!("删除书籍成功")))
}

#[tauri::command]
pub async fn delete_books(
    state: tauri::State<'_, AppState>,
    req: Vec<Book>,
) -> Result<ApiResponse<Value>, AppError> {
    let user_ns = "default";
    let removed_books = find_matching_books(&state, &user_ns, &req).await?;
    let count = state.book_service.delete_books(&user_ns, req).await?;
    cleanup_ai_book_memories(&state, &user_ns, &removed_books).await;
    cleanup_local_book_files(&state, &user_ns, &removed_books).await;
    Ok(ApiResponse::ok(serde_json::json!({"deleted": count})))
}

#[tauri::command]
pub async fn save_book_progress(
    state: tauri::State<'_, AppState>,
    req: SaveBookProgressRequest,
) -> Result<ApiResponse<Value>, AppError> {
    let user_ns = "default";
    let book_url = req
        .url
        .or(req.book_url)
        .or(req.search_book.and_then(|s| s.book_url))
        .ok_or_else(|| AppError::BadRequest("url required".to_string()))?;
    let book_url = repair_encoded_url(&book_url);
    let index = req
        .index
        .ok_or_else(|| AppError::BadRequest("index required".to_string()))?;

    let shelf_book = state
        .book_service
        .get_shelf_book(&user_ns, &book_url)
        .await?
        .ok_or_else(|| AppError::BadRequest("书籍未加入书架".to_string()))?;

    let mut updated = shelf_book.clone();
    let mut chapter_title: Option<String> = None;
    if is_local_txt_origin(&shelf_book.origin) || is_local_txt_url(&shelf_book.book_url) {
        if let Ok(chapters) = state
            .local_txt_book_service
            .get_chapter_list(&user_ns, &shelf_book.book_url)
            .await
        {
            if index >= 0 && (index as usize) < chapters.len() {
                chapter_title = Some(chapters[index as usize].title.clone());
            }
            updated.total_chapter_num = Some(chapters.len() as i32);
            if let Some(last) = chapters.last() {
                updated.latest_chapter_title = Some(last.title.clone());
            }
        }
    } else if is_local_epub_origin(&shelf_book.origin) || is_local_epub_url(&shelf_book.book_url) {
        if let Ok(chapters) = state
            .local_epub_book_service
            .get_chapter_list(&user_ns, &shelf_book.book_url)
            .await
        {
            if index >= 0 && (index as usize) < chapters.len() {
                chapter_title = Some(chapters[index as usize].title.clone());
            }
            updated.total_chapter_num = Some(chapters.len() as i32);
            if let Some(last) = chapters.last() {
                updated.latest_chapter_title = Some(last.title.clone());
            }
        }
    } else if let (Some(toc_url), Ok(Some(source))) = (
        shelf_book.toc_url.clone(),
        state
            .book_source_service
            .get(&user_ns, &shelf_book.origin)
            .await,
    ) {
        if let Ok(chapters) = state
            .book_service
            .get_chapter_list(&user_ns, &source, &toc_url)
            .await
        {
            if index >= 0 && (index as usize) < chapters.len() {
                chapter_title = Some(chapters[index as usize].title.clone());
            }
            updated.total_chapter_num = Some(chapters.len() as i32);
            if let Some(last) = chapters.last() {
                updated.latest_chapter_title = Some(last.title.clone());
            }
        }
    }
    updated.dur_chapter_index = Some(index);
    updated.dur_chapter_time = Some(crate::util::time::now_ts());
    if let Some(title) = chapter_title {
        updated.dur_chapter_title = Some(title);
    }
    if let Some(pos) = req.position {
        updated.dur_chapter_pos = Some(pos);
    }

    let _ = state.book_service.save_book(&user_ns, updated).await?;
    Ok(ApiResponse::ok(serde_json::json!("")))
}

#[tauri::command]
pub async fn get_shelf_book(
    state: tauri::State<'_, AppState>,
    req: GetShelfBookRequest,
) -> Result<ApiResponse<Value>, AppError> {
    let user_ns = "default";
    let url = req
        .url
        .ok_or_else(|| AppError::BadRequest("url required".to_string()))?;
    let book = state
        .book_service
        .get_shelf_book(&user_ns, &repair_encoded_url(&url))
        .await?
        .ok_or_else(|| AppError::BadRequest("书籍不存在".to_string()))?;
    Ok(ApiResponse::ok(
        serde_json::to_value(book).unwrap_or_default(),
    ))
}

#[tauri::command]
pub async fn get_shelf_book_with_cache_info(
    state: tauri::State<'_, AppState>,
) -> Result<ApiResponse<Value>, AppError> {
    let user_ns = "default";
    let books = state.book_service.get_bookshelf(&user_ns).await?;
    let mut result: Vec<Value> = Vec::with_capacity(books.len());
    let mut prefetch_books = Vec::new();

    for book in books {
        let mut cached_count = 0usize;

        if is_local_book(&book) {
            let mut val = serde_json::to_value(&book).unwrap_or(serde_json::json!({}));
            if let Value::Object(ref mut map) = val {
                map.insert(
                    "cachedChapterCount".to_string(),
                    serde_json::json!(cache_count_for_shelf_display(&book, cached_count)),
                );
            }
            result.push(val);
            continue;
        }

        let candidate_toc_urls = if let Some(toc_url) = &book.toc_url {
            vec![toc_url.clone(), book.book_url.clone()]
        } else {
            vec![book.book_url.clone()]
        };

        let mut found_cached_chapters = false;
        for toc_url in candidate_toc_urls {
            if let Ok(Some(chapters)) = state
                .book_service
                .load_chapter_list_cache(&user_ns, &toc_url)
                .await
            {
                let urls: Vec<String> = chapters.into_iter().map(|c| c.url).collect();
                cached_count = state
                    .book_service
                    .cached_chapter_count(&user_ns, &book.book_url, &urls)
                    .await
                    .unwrap_or(0);
                found_cached_chapters = true;
                break;
            }
        }

        if !found_cached_chapters {
            prefetch_books.push(book.clone());
        }

        let mut val = serde_json::to_value(&book).unwrap_or(serde_json::json!({}));
        if let Value::Object(ref mut map) = val {
            map.insert(
                "cachedChapterCount".to_string(),
                serde_json::json!(cached_count),
            );
        }
        result.push(val);
    }

    if !prefetch_books.is_empty() {
        let state_clone = state.inner().clone();
        let user_ns_clone = user_ns;
        tokio::spawn(async move {
            for book in prefetch_books {
                if let Ok(Some(source)) = state_clone
                    .book_source_service
                    .get(&user_ns_clone, &book.origin)
                    .await
                {
                    let mut toc_url = book.toc_url.clone();
                    if toc_url.is_none() {
                        if let Ok(info) = state_clone
                            .book_service
                            .get_book_info(&user_ns_clone, &source, &book.book_url)
                            .await
                        {
                            toc_url = info.toc_url.or(Some(book.book_url.clone()));
                        }
                    }
                    if let Some(toc_url) = toc_url.or(Some(book.book_url.clone())) {
                        let _ = state_clone
                            .book_service
                            .get_chapter_list(&user_ns_clone, &source, &toc_url)
                            .await;
                    }
                }
            }
        });
    }

    Ok(ApiResponse::ok(
        serde_json::to_value(result).unwrap_or_default(),
    ))
}

#[tauri::command]
pub async fn get_invalid_book_sources(
    state: tauri::State<'_, AppState>,
) -> Result<ApiResponse<Value>, AppError> {
    let user_ns = "default";
    let path = std::path::PathBuf::from(&state.config.storage_dir)
        .join("cache")
        .join("invalid_book_sources")
        .join(format!("{}.json", user_ns));
    if !path.exists() {
        return Ok(ApiResponse::ok(serde_json::json!([])));
    }
    let data = tokio::fs::read_to_string(path)
        .await
        .map_err(|e| AppError::Internal(e.into()))?;
    let val: Value =
        serde_json::from_str(&data).map_err(|e| AppError::BadRequest(e.to_string()))?;
    if let Value::Array(_) = val {
        Ok(ApiResponse::ok(val))
    } else {
        Ok(ApiResponse::ok(serde_json::json!([val])))
    }
}

#[tauri::command]
pub async fn cache_book_sse(
    state: tauri::State<'_, AppState>,
    req: CacheBookRequest,
    on_event: tauri::ipc::Channel<serde_json::Value>,
) -> Result<(), AppError> {
    let user_ns = "default";
    let book_url = req
        .url
        .or(req.book_url)
        .ok_or_else(|| AppError::BadRequest("url required".to_string()))?;
    let refresh = req.refresh.unwrap_or(0) > 0;
    let concurrent = req.concurrent_count.unwrap_or(24).max(1) as usize;

    let book = state
        .book_service
        .get_shelf_book(&user_ns, &book_url)
        .await?
        .ok_or_else(|| AppError::BadRequest("请先加入书架".to_string()))?;

    if book.origin.trim().is_empty() {
        return Err(AppError::BadRequest("未配置书源".to_string()));
    }
    let source = state
        .book_source_service
        .get(&user_ns, &book.origin)
        .await?
        .ok_or_else(|| AppError::BadRequest("书源不存在".to_string()))?;

    // The root TOC url for the book (for fetching the full list)
    let root_toc_url = book
        .toc_url
        .clone()
        .unwrap_or_else(|| book.book_url.clone());

    // The starting chapter URL for caching (from query params)
    let start_ch_url = req.toc_url.clone();
    let cache_count = req.count.unwrap_or(0); // 0 means all

    let mut chapters = state
        .book_service
        .get_chapter_list(&user_ns, &source, &root_toc_url)
        .await?;

    // If a starting URL is provided, narrow down the list
    if let Some(ch_url) = start_ch_url {
        if let Some(idx) = chapters.iter().position(|c| c.url == ch_url) {
            chapters = chapters.split_off(idx);
        }
    }

    // Limit count if requested
    if cache_count > 0 && cache_count < chapters.len() as i32 {
        chapters.truncate(cache_count as usize);
    }

    if chapters.is_empty() {
        return Err(AppError::BadRequest("没有找到需要缓存的章节".to_string()));
    }

    let book_url = book.book_url.clone();
    let state_clone = state.inner().clone();
    let source_clone = source.clone();
    let book_url_clone = book_url.clone();
    let user_ns_clone = user_ns;

    tokio::spawn(async move {
        let mut cached_count = 0usize;
        if !refresh {
            for ch in &chapters {
                if state_clone
                    .book_service
                    .is_chapter_cached(&user_ns_clone, &book_url_clone, &ch.url)
                    .await
                {
                    cached_count += 1;
                }
            }
        }
        let mut success = 0usize;
        let mut failed = 0usize;
        let _ = on_event.send(serde_json::json!({
            "event": "data",
            "cachedCount": cached_count,
            "successCount": success,
            "failedCount": failed
        }));

        let sem = std::sync::Arc::new(tokio::sync::Semaphore::new(concurrent));
        let mut tasks: FuturesUnordered<_> = FuturesUnordered::new();
        for ch in chapters {
            let already_cached = !refresh
                && state_clone
                    .book_service
                    .is_chapter_cached(&user_ns_clone, &book_url_clone, &ch.url)
                    .await;
            if already_cached {
                continue;
            }
            let permit = match sem.clone().acquire_owned().await {
                Ok(p) => p,
                Err(_) => {
                    failed += 1;
                    continue;
                }
            };
            let svc = state_clone.book_service.clone();
            let src = source_clone.clone();
            let url = ch.url.clone();
            let b_url = book_url_clone.clone();
            let refresh_flag = refresh;
            let u_ns = user_ns_clone;
            tasks.push(tokio::spawn(async move {
                let _permit = permit;
                svc.cache_chapter(&u_ns, &b_url, &src, &url, refresh_flag)
                    .await
            }));
        }

        while let Some(task) = tasks.next().await {
            match task {
                Ok(Ok(_)) => {
                    success += 1;
                    cached_count += 1;
                }
                _ => {
                    failed += 1;
                }
            }
            let _ = on_event.send(serde_json::json!({
                "event": "data",
                "cachedCount": cached_count,
                "successCount": success,
                "failedCount": failed
            }));
        }

        let _ = on_event.send(serde_json::json!({
            "event": "end",
            "cachedCount": cached_count,
            "successCount": success,
            "failedCount": failed
        }));
    });

    Ok(())
}

#[tauri::command]
pub async fn search_book_multi_sse(
    state: tauri::State<'_, AppState>,
    req: SearchBookMultiSseRequest,
    on_event: tauri::ipc::Channel<serde_json::Value>,
) -> Result<(), AppError> {
    let user_ns = "default";
    let key = req.key.unwrap_or_default();
    let last_index = req.last_index.unwrap_or(-1);
    let concurrent = req.concurrent_count.unwrap_or(24).max(1) as usize;
    let book_source_url =
        req.book_source_url
            .clone()
            .and_then(|u| if u.trim().is_empty() { None } else { Some(u) });
    let book_source_group =
        req.book_source_group
            .clone()
            .and_then(|g| if g.trim().is_empty() { None } else { Some(g) });

    let state_clone = state.inner().clone();

    tokio::spawn(async move {
        if key.trim().is_empty() {
            let _ = on_event.send(json_err("请输入搜索关键字"));
            let _ = on_event.send(json_end(last_index));
            return;
        }

        let sources = if let Some(url) = book_source_url {
            match state_clone.book_source_service.get(&user_ns, &url).await {
                Ok(Some(s)) => vec![s],
                _ => {
                    let _ = on_event.send(json_err("未配置书源"));
                    let _ = on_event.send(json_end(last_index));
                    return;
                }
            }
        } else {
            match state_clone.book_source_service.list(&user_ns).await {
                Ok(mut list) => {
                    if let Some(ref group) = book_source_group {
                        list.retain(|s| {
                            s.book_source_group.as_deref().unwrap_or("").contains(group)
                        });
                    }
                    if list.is_empty() {
                        let _ = on_event.send(json_err("未配置书源或分组为空"));
                        let _ = on_event.send(json_end(last_index));
                        return;
                    }
                    list
                }
                _ => {
                    let _ = on_event.send(json_err("未配置书源"));
                    let _ = on_event.send(json_end(last_index));
                    return;
                }
            }
        };

        let mut idx = last_index + 1;
        let mut last_idx = last_index;
        let mut tasks: FuturesUnordered<_> = FuturesUnordered::new();

        // Query EVERY source and emit each source's RAW results. We must NOT
        // dedup server-side: the frontend merges rows by normalized name|author
        // and accumulates the per-book source list, so a book found in N sources
        // shows once with N origins. Deduping here would drop the later sources'
        // rows before the frontend ever sees them.
        while (idx as usize) < sources.len() || !tasks.is_empty() {
            while tasks.len() < concurrent && (idx as usize) < sources.len() {
                let source = sources[idx as usize].clone();
                let svc = state_clone.book_service.clone();
                let k = key.clone();
                let cur_idx = idx;
                let user_ns_value = user_ns;
                tasks.push(tokio::spawn(async move {
                    let res = svc.search_book(&user_ns_value, &source, &k, 1).await;
                    (cur_idx, source.book_source_name, res)
                }));
                idx += 1;
            }

            if let Some(res) = tasks.next().await {
                match res {
                    Ok((cur_idx, _source_name, Ok(list))) => {
                        last_idx = cur_idx;
                        if !list.is_empty() {
                            let payload = serde_json::json!({"event": "data", "lastIndex": cur_idx, "data": list});
                            let _ = on_event.send(payload);
                        }
                    }
                    Ok((cur_idx, _source_name, Err(e))) => {
                        last_idx = cur_idx;
                        tracing::error!("search_book error from {}: {:?}", _source_name, e);
                    }
                    Err(e) => {
                        tracing::error!("task join error: {:?}", e);
                    }
                }
            } else {
                break;
            }
        }

        let _ = on_event.send(json_end(last_idx));
    });

    Ok(())
}

#[tauri::command]
pub async fn search_book_source_sse(
    state: tauri::State<'_, AppState>,
    req: SearchBookSourceSseRequest,
    on_event: tauri::ipc::Channel<serde_json::Value>,
) -> Result<(), AppError> {
    let user_ns = "default";
    let book_url = req.url.unwrap_or_default();
    let last_index = req.last_index.unwrap_or(-1);
    let search_size = req.search_size.unwrap_or(30).max(1) as usize;
    let refresh = req.refresh.unwrap_or(0) > 0;
    let concurrent = std::cmp::max(search_size * 2, 24);
    let book_source_group =
        req.book_source_group
            .clone()
            .and_then(|g| if g.trim().is_empty() { None } else { Some(g) });

    let state_clone = state.inner().clone();

    tokio::spawn(async move {
        if book_url.trim().is_empty() {
            let _ = on_event.send(json_err("请输入书籍链接"));
            let _ = on_event.send(json_end(last_index));
            return;
        }

        let book = match state_clone
            .book_service
            .get_shelf_book(&user_ns, &book_url)
            .await
        {
            Ok(Some(b)) => b,
            _ => {
                let _ = on_event.send(json_err("书籍信息错误"));
                let _ = on_event.send(json_end(last_index));
                return;
            }
        };

        let sources = match state_clone.book_source_service.list(&user_ns).await {
            Ok(mut list) => {
                if let Some(ref group) = book_source_group {
                    list.retain(|s| s.book_source_group.as_deref().unwrap_or("").contains(group));
                }
                if list.is_empty() {
                    let _ = on_event.send(json_err("未配置书源或分组为空"));
                    let _ = on_event.send(json_end(last_index));
                    return;
                }
                list
            }
            _ => {
                let _ = on_event.send(json_err("未配置书源"));
                let _ = on_event.send(json_end(last_index));
                return;
            }
        };

        let mut idx = last_index + 1;
        let mut last_idx = last_index;
        let mut total = 0usize;
        let mut tasks: FuturesUnordered<_> = FuturesUnordered::new();
        let mut all_results: Vec<crate::model::search::SearchBook> = Vec::new();

        while (idx as usize) < sources.len() || !tasks.is_empty() {
            while tasks.len() < concurrent && (idx as usize) < sources.len() {
                let source = sources[idx as usize].clone();
                let svc = state_clone.book_service.clone();
                let target_name = book.name.clone();
                let target_author = book.author.clone();
                let cur_idx = idx;
                let user_ns_value = user_ns;
                tasks.push(tokio::spawn(async move {
                    let res = svc
                        .search_book(&user_ns_value, &source, &target_name, 1)
                        .await;
                    (cur_idx, res, target_name, target_author)
                }));
                last_idx = idx;
                idx += 1;
            }

            if let Some(res) = tasks.next().await {
                if let Ok((cur_idx, Ok(list), target_name, target_author)) = res {
                    let mut batch = Vec::new();
                    for b in list {
                        if b.name == target_name && b.author == target_author {
                            batch.push(b);
                        }
                    }
                    if !batch.is_empty() {
                        total += batch.len();
                        all_results.extend(batch.clone());
                        let payload = serde_json::json!({"event": "data", "lastIndex": cur_idx, "data": batch});
                        let _ = on_event.send(payload);
                    }
                    if total >= search_size {
                        break;
                    }
                }
            } else {
                break;
            }
        }

        if refresh || !all_results.is_empty() {
            let _ = state_clone
                .book_service
                .save_book_sources_cache(&user_ns, &book.book_url, &all_results)
                .await;
        }
        let _ = on_event.send(json_end(last_idx));
    });

    Ok(())
}

#[tauri::command]
pub async fn get_available_book_source(
    state: tauri::State<'_, AppState>,
    req: GetAvailableBookSourceRequest,
) -> Result<ApiResponse<Value>, AppError> {
    let user_ns = "default";
    let refresh = req.refresh.unwrap_or(0) > 0;
    let paged_request =
        !should_use_available_source_cache(refresh, req.result_limit, req.last_index);
    let result_limit = if paged_request {
        effective_available_result_limit(req.result_limit)
    } else {
        usize::MAX
    };
    let concurrent_count = effective_available_concurrent_count(req.concurrent_count);

    // Try to find book by URL first, then by name+author
    let book_url = req.url.clone();

    if !paged_request {
        if let Some(ref url) = book_url {
            if let Some(list) = state
                .book_service
                .load_book_sources_cache(&user_ns, url)
                .await?
            {
                return Ok(ApiResponse::ok(
                    serde_json::to_value(list).unwrap_or_default(),
                ));
            }
        }
    }

    // Find book on shelf - try URL first, then name+author
    let book = if let Some(ref url) = book_url {
        state.book_service.get_shelf_book(&user_ns, url).await?
    } else {
        None
    };

    // If not found by URL, try name+author
    let book = match book {
        Some(b) => Some(b),
        None => {
            if let (Some(name), Some(author)) = (&req.name, &req.author) {
                state
                    .book_service
                    .find_shelf_book_by_name_author(&user_ns, name, author)
                    .await?
            } else {
                None
            }
        }
    };
    let book = book.or_else(|| fallback_available_book(&req));

    let book = book.ok_or_else(|| AppError::BadRequest("书籍信息错误".to_string()))?;
    let sources = state.book_source_service.list(&user_ns).await?;
    if sources.is_empty() {
        if paged_request {
            return Ok(ApiResponse::ok(
                serde_json::to_value(build_available_book_source_response(
                    Vec::new(),
                    req.last_index.unwrap_or(-1),
                    false,
                    req.result_limit,
                ))
                .unwrap_or_default(),
            ));
        }
        return Ok(ApiResponse::ok(serde_json::json!([])));
    }

    let mut result: Vec<SearchBook> = Vec::new();
    let mut cursor = (req.last_index.unwrap_or(-1) + 1).max(0) as usize;
    let mut last_index = req.last_index.unwrap_or(-1);

    while cursor < sources.len() {
        let batch_end = (cursor + concurrent_count).min(sources.len());
        let mut tasks: FuturesUnordered<_> = FuturesUnordered::new();
        for source_index in cursor..batch_end {
            let source = sources[source_index].clone();
            let svc = state.book_service.clone();
            let name = book.name.clone();
            let author = book.author.clone();
            let user_ns_value = user_ns;
            tasks.push(tokio::spawn(async move {
                let res = svc.search_book(&user_ns_value, &source, &name, 1).await;
                (source_index as i32, res, name, author)
            }));
        }

        let mut batch_results = Vec::new();
        while let Some(res) = tasks.next().await {
            if let Ok((source_index, search_result, name, author)) = res {
                let matches = match search_result {
                    Ok(list) => list
                        .into_iter()
                        .filter(|b| available_source_matches_target(b, &name, &author))
                        .collect::<Vec<_>>(),
                    Err(err) => {
                        tracing::debug!(
                            "getAvailableBookSource search failed at source index {}: {:?}",
                            source_index,
                            err
                        );
                        Vec::new()
                    }
                };
                batch_results.push((source_index, matches));
            }
        }

        batch_results.sort_by_key(|(source_index, _)| *source_index);
        for (source_index, matches) in batch_results {
            if result.len() >= result_limit {
                break;
            }
            last_index = source_index;
            for book in matches {
                if result.len() >= result_limit {
                    break;
                }
                result.push(book);
            }
        }

        cursor = batch_end;
        if result.len() >= result_limit {
            break;
        }
    }

    let has_more = (last_index + 1).max(0) < sources.len() as i32;
    if !has_more && req.last_index.unwrap_or(-1) < 0 {
        let _ = state
            .book_service
            .save_book_sources_cache(&user_ns, &book.book_url, &result)
            .await;
    }

    if paged_request {
        let response =
            build_available_book_source_response(result, last_index, has_more, req.result_limit);
        return Ok(ApiResponse::ok(
            serde_json::to_value(response).unwrap_or_default(),
        ));
    }

    Ok(ApiResponse::ok(
        serde_json::to_value(result).unwrap_or_default(),
    ))
}

#[tauri::command]
pub async fn get_available_book_source_sse(
    state: tauri::State<'_, AppState>,
    req: GetAvailableBookSourceRequest,
    on_event: tauri::ipc::Channel<serde_json::Value>,
) -> Result<(), AppError> {
    let user_ns = "default";
    let refresh = req.refresh.unwrap_or(0) > 0;
    let last_index_start = req.last_index.unwrap_or(-1);
    let concurrent_count = effective_available_concurrent_count(req.concurrent_count);
    let book_url = req.url.clone();

    let book = if let Some(ref url) = book_url {
        state.book_service.get_shelf_book(&user_ns, url).await?
    } else {
        None
    };
    let book = match book {
        Some(b) => b,
        None => if let (Some(name), Some(author)) = (&req.name, &req.author) {
            state
                .book_service
                .find_shelf_book_by_name_author(&user_ns, name, author)
                .await?
        } else {
            None
        }
        .or_else(|| fallback_available_book(&req))
        .ok_or_else(|| AppError::BadRequest("书籍信息错误".to_string()))?,
    };

    if !refresh && last_index_start < 0 {
        if let Some(ref url) = book_url {
            if let Some(cached) = state
                .book_service
                .load_book_sources_cache(&user_ns, url)
                .await?
            {
                let cached = take_available_source_cached_matches(
                    cached,
                    AVAILABLE_SOURCE_SSE_RESULT_LIMIT,
                );
                if cached.is_empty() {
                    // Ignore stale caches that only contain the current source.
                } else {
                    let on_event_clone = on_event.clone();
                    tokio::spawn(async move {
                        let mut last_index = -1;
                        for book in cached {
                            last_index += 1;
                            let payload = serde_json::json!({
                                "event": "data",
                                "lastIndex": last_index,
                                "hasMore": false,
                                "data": [book]
                            });
                            if on_event_clone.send(payload).is_err() {
                                return;
                            }
                        }
                        let _ = on_event_clone.send(serde_json::json!({
                            "event": "end",
                            "lastIndex": last_index,
                            "hasMore": false
                        }));
                    });
                    return Ok(());
                }
            }
        }
    }

    let sources = state.book_source_service.list(&user_ns).await?;
    let state_clone = state.inner().clone();
    tokio::spawn(async move {
        if sources.is_empty() {
            let _ = on_event.send(serde_json::json!({
                "event": "end",
                "lastIndex": last_index_start,
                "hasMore": false
            }));
            return;
        }

        let mut next_index = (last_index_start + 1).max(0) as usize;
        let mut last_idx = last_index_start;
        let mut emitted = 0usize;
        let mut all_results: Vec<SearchBook> = Vec::new();
        let mut seen = std::collections::HashSet::<String>::new();
        let mut tasks = JoinSet::new();

        while next_index < sources.len() || !tasks.is_empty() {
            while tasks.len() < concurrent_count
                && next_index < sources.len()
                && emitted < AVAILABLE_SOURCE_SSE_RESULT_LIMIT
            {
                let source_index = next_index;
                let source = sources[source_index].clone();
                let svc = state_clone.book_service.clone();
                let target_name = book.name.clone();
                let target_author = book.author.clone();
                let user_ns_value = user_ns;
                tasks.spawn(async move {
                    let res = svc
                        .search_book(&user_ns_value, &source, &target_name, 1)
                        .await;
                    (source_index as i32, res, target_name, target_author)
                });
                next_index += 1;
            }

            if emitted >= AVAILABLE_SOURCE_SSE_RESULT_LIMIT || tasks.is_empty() {
                break;
            }

            match tasks.join_next().await {
                Some(Ok((source_index, search_result, target_name, target_author))) => {
                    last_idx = last_idx.max(source_index);
                    match search_result {
                        Ok(list) => {
                            let matches = take_available_source_sse_matches(
                                list,
                                &target_name,
                                &target_author,
                                &mut seen,
                                AVAILABLE_SOURCE_SSE_RESULT_LIMIT - emitted,
                            );
                            for book in matches {
                                emitted += 1;
                                all_results.push(book.clone());
                                let payload = serde_json::json!({
                                    "event": "data",
                                    "lastIndex": source_index,
                                    "hasMore": next_index < sources.len() || !tasks.is_empty(),
                                    "data": [book]
                                });
                                if on_event.send(payload).is_err() {
                                    tasks.abort_all();
                                    return;
                                }
                                if emitted >= AVAILABLE_SOURCE_SSE_RESULT_LIMIT {
                                    break;
                                }
                            }
                        }
                        Err(err) => {
                            tracing::debug!(
                                "getAvailableBookSourceSSE search failed at source index {}: {:?}",
                                source_index,
                                err
                            );
                        }
                    }
                }
                Some(Err(err)) => {
                    tracing::debug!("getAvailableBookSourceSSE task join failed: {:?}", err);
                }
                None => break,
            }
        }

        let has_more = next_index < sources.len() || !tasks.is_empty();
        if has_more {
            tasks.abort_all();
        }
        let final_last_idx = if has_more {
            last_idx.max(next_index as i32 - 1)
        } else {
            last_idx
        };

        // Persist whatever matched so far on a fresh (non-paged) search. We
        // intentionally save even when has_more is true, otherwise a popular
        // book that hits the result limit before all sources are queried never
        // gets cached and every panel open triggers a full re-search.
        if last_index_start < 0 {
            let _ = state_clone
                .book_service
                .save_book_sources_cache(&user_ns, &book.book_url, &all_results)
                .await;
        }

        let _ = on_event.send(serde_json::json!({
            "event": "end",
            "lastIndex": final_last_idx,
            "hasMore": has_more
        }));
    });

    Ok(())
}

#[tauri::command]
pub async fn book_source_debug_sse(
    state: tauri::State<'_, AppState>,
    req: BookSourceDebugRequest,
    on_event: tauri::ipc::Channel<serde_json::Value>,
) -> Result<(), AppError> {
    let user_ns = "default";
    let book_source_url = req.book_source_url.unwrap_or_default();
    let keyword = req.keyword.unwrap_or_default();

    let state_clone = state.inner().clone();

    tokio::spawn(async move {
        if book_source_url.trim().is_empty() {
            let _ = on_event.send(json_err("未配置书源"));
            let _ = on_event.send(json_end(0));
            return;
        }
        let source = match state_clone
            .book_source_service
            .get(&user_ns, &book_source_url)
            .await
        {
            Ok(Some(s)) => s,
            _ => {
                let _ = on_event.send(json_err("未配置书源"));
                let _ = on_event.send(json_end(0));
                return;
            }
        };
        let keyword = if keyword.trim().is_empty() {
            source
                .rule_search
                .as_ref()
                .and_then(|rule| rule.check_key_word.clone())
                .unwrap_or_default()
        } else {
            keyword.clone()
        };
        if keyword.trim().is_empty() {
            let _ = on_event.send(json_err("请输入搜索关键词"));
            let _ = on_event.send(json_end(0));
            return;
        }
        let _ = on_event.send(json_msg("start search"));
        match state_clone
            .book_service
            .search_book(&user_ns, &source, &keyword, 1)
            .await
        {
            Ok(list) => {
                let msg = format!("found {} items", list.len());
                let _ = on_event.send(json_msg(&msg));
                let _ = on_event.send(serde_json::json!({"event": "data", "data": list}));
            }
            Err(e) => {
                let _ = on_event.send(json_err(&e.to_string()));
            }
        }
        let _ = on_event.send(json_end(0));
    });

    Ok(())
}

fn json_err(msg: &str) -> Value {
    serde_json::json!({"event": "error", "errorMsg": msg})
}

fn json_end(last_index: i32) -> Value {
    serde_json::json!({"event": "end", "lastIndex": last_index})
}

fn json_msg(msg: &str) -> Value {
    serde_json::json!({"event": "data", "msg": msg})
}

async fn resolve_book_source(
    state: &AppState,
    user_ns: &str,
    book_source_url: Option<String>,
    book_source: Option<BookSource>,
    book_url: Option<&str>,
) -> Result<BookSource, AppError> {
    if let Some(src) = book_source {
        return Ok(src);
    }
    if book_source_url.as_deref().is_some_and(is_local_txt_origin)
        || book_url.is_some_and(is_local_txt_url)
    {
        return Ok(BookSource {
            book_source_name: "本地 TXT".to_string(),
            book_source_url: LOCAL_TXT_ORIGIN.to_string(),
            ..BookSource::default()
        });
    }
    if book_source_url.as_deref().is_some_and(is_local_epub_origin)
        || book_url.is_some_and(is_local_epub_url)
    {
        return Ok(BookSource {
            book_source_name: "本地 EPUB".to_string(),
            book_source_url: LOCAL_EPUB_ORIGIN.to_string(),
            ..BookSource::default()
        });
    }
    if let Some(url) = &book_source_url {
        let normalized = normalize_source_url(url);
        if !normalized.is_empty() {
            if let Some(src) = state.book_source_service.get(&user_ns, &normalized).await? {
                return Ok(src);
            }
            let sources = state.book_source_service.list(&user_ns).await?;
            if let Some(src) = sources
                .into_iter()
                .find(|s| normalize_source_url(&s.book_source_url) == normalized)
            {
                return Ok(src);
            }
            return Err(AppError::NotFound("bookSource not found".to_string()));
        }
    }

    // Try to find book_source_url from shelf book
    if let Some(b_url) = book_url {
        if let Ok(Some(shelf_book)) = state.book_service.get_shelf_book(&user_ns, b_url).await {
            let shelf_origin = normalize_source_url(&shelf_book.origin);
            if !shelf_origin.is_empty() {
                if let Some(src) = state
                    .book_source_service
                    .get(&user_ns, &shelf_origin)
                    .await?
                {
                    return Ok(src);
                }
                let sources = state.book_source_service.list(&user_ns).await?;
                if let Some(src) = sources
                    .into_iter()
                    .find(|s| normalize_source_url(&s.book_source_url) == shelf_origin)
                {
                    return Ok(src);
                }
            }
        }
    }

    // Auto-discovery from book_url
    if let Some(b_url) = book_url {
        let b_host = match url::Url::parse(b_url) {
            Ok(u) => u.host_str().unwrap_or_default().to_string(),
            Err(_) => "".to_string(),
        };
        if !b_host.is_empty() {
            // Extract root domain for comparison (e.g., "22biqu" from "m.22biqu.com")
            let b_root = extract_root_domain(&b_host);
            let sources = state.book_source_service.list(&user_ns).await?;
            for s in sources {
                let normalized_source_url = normalize_source_url(&s.book_source_url);
                if let Ok(s_url) = url::Url::parse(&normalized_source_url) {
                    if let Some(s_host) = s_url.host_str() {
                        // Match by exact host or by root domain
                        let s_root = extract_root_domain(s_host);
                        if b_host.ends_with(s_host)
                            || s_host.ends_with(&b_host)
                            || (b_root == s_root && !b_root.is_empty())
                        {
                            return Ok(s);
                        }
                    }
                }
            }
        }
    }

    Err(AppError::BadRequest(
        "bookSource or bookSourceUrl required, and auto-discovery failed".to_string(),
    ))
}

/// Extract root domain for matching (e.g., "22biqu" from "m.22biqu.com" or "m.22biqu.net")
fn extract_root_domain(host: &str) -> String {
    let parts: Vec<&str> = host.split('.').collect();
    if parts.len() >= 2 {
        parts[parts.len() - 2].to_string()
    } else {
        host.to_string()
    }
}

fn merge_book(target: &mut Book, info: Book) {
    if target.name.trim().is_empty() {
        target.name = info.name;
    }
    if target.author.trim().is_empty() {
        target.author = info.author;
    }
    if target.cover_url.is_none() {
        target.cover_url = info.cover_url;
    }
    if target.toc_url.is_none() {
        target.toc_url = info.toc_url;
    }
    if target.intro.is_none() {
        target.intro = info.intro;
    }
    if target.latest_chapter_title.is_none() {
        target.latest_chapter_title = info.latest_chapter_title;
    }
    if target.word_count.is_none() {
        target.word_count = info.word_count;
    }
    if target.origin_name.is_none() {
        target.origin_name = info.origin_name;
    }
    if target.kind.is_none() {
        target.kind = info.kind;
    }
    if target.update_time.is_none() {
        target.update_time = info.update_time;
    }
}

#[tauri::command]
pub async fn get_txt_toc_rules() -> Result<ApiResponse<Vec<serde_json::Value>>, AppError> {
    Ok(ApiResponse::ok(vec![]))
}

async fn find_matching_books(
    state: &AppState,
    user_ns: &str,
    targets: &[Book],
) -> Result<Vec<Book>, AppError> {
    let shelf_books = state.book_service.get_bookshelf(user_ns).await?;
    Ok(shelf_books
        .into_iter()
        .filter(|shelf_book| {
            targets
                .iter()
                .any(|target| book_matches_delete_target(shelf_book, target))
        })
        .collect())
}

async fn cleanup_ai_book_memories(state: &AppState, user_ns: &str, books: &[Book]) {
    for book in books {
        if book.book_url.trim().is_empty() {
            continue;
        }
        let _ = state.ai_book_service.delete(user_ns, &book.book_url).await;
    }
}

async fn cleanup_local_book_files(state: &AppState, user_ns: &str, books: &[Book]) {
    for book in books {
        if is_local_txt_origin(&book.origin) || is_local_txt_url(&book.book_url) {
            if let Err(err) = state
                .local_txt_book_service
                .delete_book_files(user_ns, &book.book_url)
                .await
            {
                tracing::warn!(
                    "failed to delete local txt book files for {}: {:?}",
                    book.book_url,
                    err
                );
            }
        } else if is_local_epub_origin(&book.origin) || is_local_epub_url(&book.book_url) {
            if let Err(err) = state
                .local_epub_book_service
                .delete_book_files(user_ns, &book.book_url)
                .await
            {
                tracing::warn!(
                    "failed to delete local epub book files for {}: {:?}",
                    book.book_url,
                    err
                );
            }
        }
    }
}

fn book_matches_delete_target(shelf_book: &Book, target: &Book) -> bool {
    if !target.book_url.is_empty() && shelf_book.book_url == target.book_url {
        return true;
    }
    if is_local_book(shelf_book) || is_local_book(target) {
        return false;
    }
    !target.name.is_empty()
        && !target.author.is_empty()
        && shelf_book.name == target.name
        && shelf_book.author == target.author
}

fn effective_available_result_limit(result_limit: Option<i32>) -> usize {
    result_limit
        .unwrap_or(DEFAULT_AVAILABLE_RESULT_LIMIT as i32)
        .clamp(1, MAX_AVAILABLE_RESULT_LIMIT as i32) as usize
}

fn effective_available_concurrent_count(concurrent_count: Option<i32>) -> usize {
    concurrent_count
        .unwrap_or(DEFAULT_AVAILABLE_CONCURRENT_COUNT as i32)
        .clamp(1, MAX_AVAILABLE_CONCURRENT_COUNT as i32) as usize
}

fn should_use_available_source_cache(
    refresh: bool,
    result_limit: Option<i32>,
    last_index: Option<i32>,
) -> bool {
    !refresh && result_limit.is_none() && last_index.is_none()
}

fn fallback_available_book(req: &GetAvailableBookSourceRequest) -> Option<Book> {
    let book_url = req.url.as_deref()?.trim();
    let name = req.name.as_deref()?.trim();
    if book_url.is_empty() || name.is_empty() {
        return None;
    }

    let origin = req.origin.as_deref().unwrap_or_default().trim();
    Some(Book {
        book_url: repair_encoded_url(book_url),
        name: name.to_string(),
        author: req.author.as_deref().unwrap_or_default().trim().to_string(),
        origin: if origin.is_empty() {
            String::new()
        } else {
            normalize_source_url(origin)
        },
        ..Book::default()
    })
}

fn build_available_book_source_response(
    mut books: Vec<SearchBook>,
    last_index: i32,
    has_more: bool,
    result_limit: Option<i32>,
) -> AvailableBookSourceResponse {
    let limit = effective_available_result_limit(result_limit);
    let has_more = has_more || books.len() > limit;
    books.truncate(limit);
    AvailableBookSourceResponse {
        books,
        last_index,
        has_more,
    }
}

fn cache_count_for_shelf_display(book: &Book, cached_count: usize) -> usize {
    if is_local_book(book) {
        0
    } else {
        cached_count
    }
}

fn is_local_book(book: &Book) -> bool {
    is_local_txt_origin(&book.origin)
        || is_local_txt_url(&book.book_url)
        || is_local_epub_origin(&book.origin)
        || is_local_epub_url(&book.book_url)
}

fn local_book_limit_exceeded(books: &[Book], incoming_book_url: &str, limit: u32) -> bool {
    if limit == 0 {
        return false;
    }
    if books
        .iter()
        .any(|book| is_local_book(book) && book.book_url == incoming_book_url)
    {
        return false;
    }
    books.iter().filter(|book| is_local_book(book)).count() >= limit as usize
}

fn available_source_sse_result_key(book: &SearchBook) -> String {
    format!("{}::{}", book.origin, book.book_url)
}

fn available_source_matches_target(
    book: &SearchBook,
    target_name: &str,
    _target_author: &str,
) -> bool {
    // Match on exact (whitespace-normalized) title only. Author differences are
    // tolerated so sources that spell the author slightly differently still
    // surface; author is used only for ranking, not filtering.
    normalize_available_book_name(&book.name) == normalize_available_book_name(target_name)
}

fn normalize_available_book_name(value: &str) -> String {
    value.chars().filter(|ch| !ch.is_whitespace()).collect()
}

fn take_available_source_cached_matches(
    cached: Vec<SearchBook>,
    limit: usize,
) -> Vec<SearchBook> {
    let mut matches = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for book in cached {
        if matches.len() >= limit {
            break;
        }
        if seen.insert(available_source_sse_result_key(&book)) {
            matches.push(book);
        }
    }
    matches
}

fn take_available_source_sse_matches(
    books: Vec<SearchBook>,
    target_name: &str,
    target_author: &str,
    seen: &mut std::collections::HashSet<String>,
    limit: usize,
) -> Vec<SearchBook> {
    if limit == 0 {
        return Vec::new();
    }

    let mut matches = Vec::new();
    for book in books {
        if matches.len() >= limit {
            break;
        }
        if !available_source_matches_target(&book, target_name, target_author) {
            continue;
        }
        if seen.insert(available_source_sse_result_key(&book)) {
            matches.push(book);
        }
    }
    matches
}

#[cfg(test)]
mod tests {
    use super::{
        book_matches_delete_target, build_available_book_source_response,
        cache_count_for_shelf_display, fallback_available_book, local_book_limit_exceeded,
        should_use_available_source_cache, take_available_source_cached_matches,
        take_available_source_sse_matches, GetAvailableBookSourceRequest,
    };
    use crate::model::{book::Book, search::SearchBook};
    use std::collections::HashSet;

    #[test]
    fn delete_target_matches_by_book_url() {
        let shelf_book = Book {
            book_url: "https://example.test/book/1".to_string(),
            name: "A".to_string(),
            author: "B".to_string(),
            ..Book::default()
        };
        let target = Book {
            book_url: "https://example.test/book/1".to_string(),
            ..Book::default()
        };

        assert!(book_matches_delete_target(&shelf_book, &target));
    }

    #[test]
    fn delete_target_matches_by_name_and_author() {
        let shelf_book = Book {
            book_url: "https://example.test/book/1".to_string(),
            name: "A".to_string(),
            author: "B".to_string(),
            ..Book::default()
        };
        let target = Book {
            name: "A".to_string(),
            author: "B".to_string(),
            ..Book::default()
        };

        assert!(book_matches_delete_target(&shelf_book, &target));
    }

    #[test]
    fn available_book_source_limits_result_count_and_reports_more() {
        let books: Vec<SearchBook> = (0..25)
            .map(|index| SearchBook {
                name: format!("Book {index}"),
                author: if index == 3 {
                    "作者：Author".to_string()
                } else {
                    "Author".to_string()
                },
                ..SearchBook::default()
            })
            .collect();

        let response = build_available_book_source_response(books, 11, true, Some(20));

        assert_eq!(response.books.len(), 20);
        assert_eq!(response.books[0].name, "Book 0");
        assert_eq!(response.books[19].name, "Book 19");
        assert_eq!(response.last_index, 11);
        assert!(response.has_more);
    }

    #[test]
    fn local_txt_books_do_not_report_remote_cache_count_for_shelf_display() {
        let book = Book {
            origin: "local-txt".to_string(),
            book_url: "local-txt:abc".to_string(),
            ..Book::default()
        };

        assert_eq!(cache_count_for_shelf_display(&book, 42), 0);
    }

    #[test]
    fn local_book_limit_counts_txt_and_epub_but_allows_existing_book() {
        let books = vec![
            Book {
                origin: "local-txt".to_string(),
                book_url: "local-txt:abc".to_string(),
                ..Book::default()
            },
            Book {
                origin: "local-epub".to_string(),
                book_url: "local-epub:def".to_string(),
                ..Book::default()
            },
            Book {
                origin: "https://source.test".to_string(),
                book_url: "https://source.test/book/1".to_string(),
                ..Book::default()
            },
        ];

        assert!(!local_book_limit_exceeded(&books, "local-epub:new", 0));
        assert!(local_book_limit_exceeded(&books, "local-epub:new", 2));
        assert!(!local_book_limit_exceeded(&books, "local-epub:def", 2));
        assert!(!local_book_limit_exceeded(&books, "local-epub:new", 3));
    }

    #[test]
    fn available_book_source_paged_requests_skip_complete_cache() {
        assert!(should_use_available_source_cache(false, None, None));
        assert!(!should_use_available_source_cache(true, None, None));
        assert!(!should_use_available_source_cache(false, Some(20), None));
        assert!(!should_use_available_source_cache(false, None, Some(0)));
    }

    #[test]
    fn available_book_source_can_fallback_to_request_book() {
        let req = GetAvailableBookSourceRequest {
            url: Some("https://example.test/book/1".to_string()),
            name: Some("深空彼岸".to_string()),
            author: Some("辰东".to_string()),
            origin: Some("https://source.test".to_string()),
            refresh: None,
            last_index: None,
            result_limit: None,
            concurrent_count: None,
        };

        let book = fallback_available_book(&req).expect("fallback book");

        assert_eq!(book.book_url, "https://example.test/book/1");
        assert_eq!(book.name, "深空彼岸");
        assert_eq!(book.author, "辰东");
        assert_eq!(book.origin, "https://source.test");
    }

    #[test]
    fn available_book_source_cache_includes_current_source() {
        let cached = vec![SearchBook {
            name: "深空彼岸".to_string(),
            author: "作者：辰东".to_string(),
            origin: "https://m.22biqu.com/".to_string(),
            book_url: "https://m.22biqu.com/biqu2986/".to_string(),
            ..SearchBook::default()
        }];

        let matches = take_available_source_cached_matches(cached, 5);

        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].origin, "https://m.22biqu.com/");
    }

    #[test]
    fn available_book_source_sse_takes_matches_until_limit() {
        let books: Vec<SearchBook> = (0..8)
            .map(|index| SearchBook {
                name: if index == 0 {
                    "Other".to_string()
                } else {
                    "Book".to_string()
                },
                author: if index == 3 {
                    "作者：Author".to_string()
                } else {
                    "Author".to_string()
                },
                origin: if index == 2 {
                    "current-source".to_string()
                } else {
                    format!("source-{index}")
                },
                book_url: format!("book-{index}"),
                ..SearchBook::default()
            })
            .collect();
        let mut seen = HashSet::new();

        let matches = take_available_source_sse_matches(
            books,
            "Book",
            "Author",
            &mut seen,
            5,
        );

        assert_eq!(matches.len(), 5);
        assert_eq!(matches[0].origin, "source-1");
        assert_eq!(matches[1].origin, "current-source");
        assert_eq!(matches[2].origin, "source-3");
        assert_eq!(matches[4].origin, "source-5");
        assert_eq!(seen.len(), 5);
    }
}
