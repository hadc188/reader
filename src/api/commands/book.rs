use crate::api::commands::{decode_header_file_name, decode_upload_body};
use crate::api::AppState;
use crate::error::error::{ApiResponse, AppError};
use crate::model::{book::Book, book_chapter::BookChapter, book_source::BookSource, search::SearchBook};
use crate::service::local_epub_book::{
    is_local_epub_origin, is_local_epub_url, LOCAL_EPUB_ORIGIN, MAX_EPUB_UPLOAD_BYTES,
};
use crate::service::local_pdf_book::{
    is_local_pdf_origin, is_local_pdf_url, MAX_PDF_UPLOAD_BYTES,
};
use crate::service::local_txt_book::{
    is_local_txt_origin, is_local_txt_url, LOCAL_TXT_ORIGIN, MAX_TXT_UPLOAD_BYTES,
};
use crate::util::text::{normalize_source_url, repair_encoded_url};
use futures::stream::FuturesUnordered;
use futures::StreamExt;
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};
use tokio::task::JoinSet;

const MAX_AVAILABLE_RESULT_LIMIT: usize = 100;
const DEFAULT_AVAILABLE_CONCURRENT_COUNT: usize = 16;
const MAX_AVAILABLE_CONCURRENT_COUNT: usize = 20;
/// 达到候选软上限后停止派发新的书源，已派发书源的结果完整返回。
/// `lastIndex` 始终停在完整书源边界，客户端通过 `hasMore` 继续扫描。
const AVAILABLE_SOURCE_SSE_SCAN_SOFT_LIMIT: usize = 60;

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
pub struct ClearSearchCoverCacheRequest {
    #[serde(rename = "coverUrls")]
    cover_urls: Vec<String>,
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
    #[serde(rename = "bookUrl")]
    pub book_url: Option<String>,
    #[serde(rename = "chapterUrl", alias = "url", alias = "href")]
    pub chapter_url: Option<String>,
    #[serde(rename = "bookSourceUrl", alias = "origin")]
    pub book_source_url: Option<String>,
    #[serde(rename = "bookSource")]
    pub book_source: Option<BookSource>,
    pub index: Option<i32>,
    pub refresh: Option<i32>,
    /// 本章标题，用于剥掉正文开头自带的重复标题。
    ///
    /// 可选：老前端/Web 端不带它时跳过纯文本去重（HTML 标题块仍会剥），
    /// 行为与改动前一致。标题由目录给出，正文里的那行可能少个空格，见
    /// `strip_leading_content_title`。
    #[serde(rename = "chapterTitle")]
    pub chapter_title: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct DeleteCacheRequest {
    url: Option<String>,
    #[serde(rename = "bookUrl")]
    book_url: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct CachedChapterUrlsRequest {
    #[serde(rename = "bookUrl", alias = "url")]
    book_url: Option<String>,
    #[serde(rename = "chapterUrls", default)]
    chapter_urls: Vec<String>,
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

/// 运行中的整书缓存任务，按 bookUrl 登记，供 cancel_cache_book 中断。
static CACHE_BOOK_CANCELLATIONS: Lazy<Mutex<HashMap<String, Arc<AtomicBool>>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CancelCacheBookRequest {
    pub book_url: String,
}

/// 运行中的搜索任务(多源搜索 / 单源搜索 / 可用源探测), 按前端生成的 taskId 登记,
/// 供 cancel_book_search 中断。
///
/// 与 `CACHE_BOOK_CANCELLATIONS` 的区别: 搜索的 SSE 任务里**没有**「结束」按钮,
/// 用户离开界面时前端只是关掉 channel, 后端完全感知不到。不登记取消标志的话,
/// 关掉界面后剩余书源仍会被逐个真实请求完。
static SEARCH_CANCELLATIONS: Lazy<Mutex<HashMap<String, Arc<AtomicBool>>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));

/// 取消请求先于任务登记到达的 taskId -> 记下取消的时间。
///
/// 前端是「发出搜索 invoke」后立刻可能关流并发取消: 那个搜索任务的登记还没跑到,
/// 直接查 `SEARCH_CANCELLATIONS` 会查不到而把取消丢掉, 搜索随后照常跑完全部书源。
/// 这里先把取消记下来, `SearchCancellationGuard::register` 登记时消费掉。
///
/// 记时间戳是为了能清理孤儿条目: 若后端其实**已经**发完 end(任务早已注销),
/// 而前端的取消到达晚了一步, 这条记录就永远等不到登记它的任务。
/// 见 [`SEARCH_CANCEL_REQUEST_TTL`]。
static SEARCH_CANCEL_REQUESTS: Lazy<Mutex<HashMap<String, std::time::Instant>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));

/// 孤儿取消记录的存活上限。
///
/// 只有「搜索任务早已结束、但取消命令晚到」这一种情况会留下孤儿记录, 而正常路径
/// (任务还在跑) 的登记发生在搜索 invoke 处理的最开头, 与取消之间只隔着一次 IPC,
/// 远小于该时限。所以超过这个时间仍没被消费的记录一定是孤儿, 可以安全丢弃。
const SEARCH_CANCEL_REQUEST_TTL: std::time::Duration = std::time::Duration::from_secs(60);

/// 丢弃过期的孤儿取消记录, 返回当前存活条数(仅用于日志/调试)。
fn prune_stale_search_cancel_requests(pending: &mut HashMap<String, std::time::Instant>) {
    let now = std::time::Instant::now();
    pending.retain(|_, recorded_at| now.saturating_duration_since(*recorded_at) < SEARCH_CANCEL_REQUEST_TTL);
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CancelBookSearchRequest {
    pub task_id: String,
}

/// 搜索任务的取消登记守卫: 任务结束时自动注销自己。
///
/// 用 Drop 而不是在每个 `return` 前手写移除 —— 这些 SSE 任务里提前返回的分支很多
/// (参数校验失败、书源为空、书籍不存在…), 漏掉任何一条都会在登记表里留下永久条目,
/// 反复搜索就会不断堆积。同名任务重复发起时取消旧任务(与缓存行为一致)。
struct SearchCancellationGuard {
    flag: Arc<AtomicBool>,
    registered: Option<(String, Arc<AtomicBool>)>,
}

/// 让守卫能跨任务共享同一个标志(如缓存命中分支的推送循环)。
///
/// 只有 `flag` 被克隆出去, `registered` 保持 None —— 注销动作必须唯一地由
/// 原始守卫的 Drop 负责, 否则副本析构会提前把登记注销掉。
impl Clone for SearchCancellationGuard {
    fn clone(&self) -> Self {
        Self {
            flag: self.flag.clone(),
            registered: None,
        }
    }
}

impl SearchCancellationGuard {
    /// `task_id` 缺失或为空时不登记: 调用方拿不到句柄, 行为与加取消之前一致。
    fn register(task_id: Option<&str>) -> Self {
        let Some(id) = task_id.map(str::trim).filter(|id| !id.is_empty()) else {
            return Self {
                flag: Arc::new(AtomicBool::new(false)),
                registered: None,
            };
        };
        let flag = Arc::new(AtomicBool::new(false));
        // 锁被 poison(持锁线程 panic)时按 poison 内的数据继续用, 而不是跳过登记 ——
        // 跳过会构造出 registered: Some(..) 却不在表里, 该任务的取消将静默失效。
        {
            let mut tasks = SEARCH_CANCELLATIONS
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            if let Some(previous) = tasks.insert(id.to_string(), flag.clone()) {
                previous.store(true, Ordering::Relaxed);
            }
        }
        // 取消已经先到了(前端一发完 invoke 就关流): 立刻置位, 这个搜索不会真的开始跑。
        let cancel_already_requested = SEARCH_CANCEL_REQUESTS
            .lock()
            .map(|mut pending| {
                // 顺带清掉等不到登记的孤儿记录, 避免反复搜索退出时无界增长。
                prune_stale_search_cancel_requests(&mut pending);
                pending.remove(id)
            })
            .ok()
            .flatten();
        if cancel_already_requested.is_some() {
            flag.store(true, Ordering::Relaxed);
        }
        Self {
            flag: flag.clone(),
            registered: Some((id.to_string(), flag)),
        }
    }

    fn is_cancelled(&self) -> bool {
        self.flag.load(Ordering::Relaxed)
    }
}

impl Drop for SearchCancellationGuard {
    fn drop(&mut self) {
        let Some((key, flag)) = self.registered.take() else {
            return;
        };
        if let Ok(mut tasks) = SEARCH_CANCELLATIONS.lock() {
            // 同一 taskId 已被新任务覆盖时保留新任务的标志, 不要误删。
            // pending 的清理也放在这个判定里: 新任务可能正等着那条取消记录,
            // 旧任务无权替它删掉 (see M4 / Arc::ptr_eq 语义)。
            let still_ours = tasks
                .get(&key)
                .map(|current| Arc::ptr_eq(current, &flag))
                .unwrap_or(false);
            if still_ours {
                tasks.remove(&key);
                SEARCH_CANCEL_REQUESTS
                    .lock()
                    .map(|mut pending| pending.remove(&key))
                    .ok();
            }
        }
    }
}

#[tauri::command]
pub async fn cancel_book_search(
    req: CancelBookSearchRequest,
) -> Result<ApiResponse<Value>, AppError> {
    let registered = SEARCH_CANCELLATIONS
        .lock()
        .ok()
        .and_then(|tasks| tasks.get(&req.task_id).cloned());
    match registered {
        Some(flag) => {
            flag.store(true, Ordering::Relaxed);
            Ok(ApiResponse::ok(serde_json::json!({ "cancelled": true })))
        }
        None => {
            // 任务还没登记(前端刚发完搜索就退出): 记下来, 等它登记时立即取消。
            SEARCH_CANCEL_REQUESTS
                .lock()
                .map(|mut pending| {
                    prune_stale_search_cancel_requests(&mut pending);
                    pending.insert(req.task_id.clone(), std::time::Instant::now());
                })
                .ok();
            Ok(ApiResponse::ok(serde_json::json!({ "cancelled": false })))
        }
    }
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
    #[serde(rename = "concurrentCount")]
    concurrent_count: Option<i32>,
    #[serde(rename = "taskId")]
    task_id: Option<String>,
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
    #[serde(rename = "taskId")]
    task_id: Option<String>,
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
    /// Preferred candidate count; a source's complete list is returned together.
    #[serde(rename = "resultLimit")]
    result_limit: Option<i32>,
    #[serde(rename = "concurrentCount")]
    concurrent_count: Option<i32>,
    #[serde(rename = "taskId")]
    task_id: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AvailableBookSourceResponse {
    books: Vec<SearchBook>,
    last_index: i32,
    has_more: bool,
    has_errors: bool,
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
                if s.is_enabled() {
                    out.push(s);
                }
            }
        }
        out
    } else {
        let mut list = state.book_source_service.list_enabled(&user_ns).await?;
        if let Some(ref group) = req.book_source_group {
            list.retain(|s| s.book_source_group.as_deref().unwrap_or("").contains(group));
        }
        list
    };

    let mut tasks: FuturesUnordered<_> = FuturesUnordered::new();
    for source in sources {
        let svc = state.book_service.clone();
        let k = key.clone();
        let user_ns = user_ns;
        tasks.push(tokio::spawn(async move {
            svc.search_book(&user_ns, &source, &k, page).await
        }));
    }
    // 用 FuturesUnordered 先完成先收集, 避免按提交顺序串行 await:
    // 旧实现 for t in tasks { t.await } 即使后面的源先完成也要等前面的。
    let mut results: Vec<crate::model::search::SearchBook> = Vec::new();
    while let Some(res) = tasks.next().await {
        if let Ok(Ok(list)) = res {
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

/// 书籍详情的共享实现。
///
/// 抽成 `&AppState` 入参是为了让 Tauri command 与 Web 端 HTTP handler
/// (`crate::server`) 走**同一份**逻辑 —— 两边只是传输方式不同, 分叉实现
/// 迟早会在本地书 / 书源回退这类分支上行为不一致。
pub async fn get_book_info_core(
    state: &AppState,
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
    if is_local_pdf_url(&url)
        || req
            .book_source_url
            .as_deref()
            .is_some_and(is_local_pdf_origin)
    {
        let book = state
            .local_pdf_book_service
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

/// `get_book_info` 的 Tauri 入口: 只是 core 的薄壳。
#[tauri::command]
pub async fn get_book_info(
    state: tauri::State<'_, AppState>,
    req: BookInfoRequest,
) -> Result<ApiResponse<serde_json::Value>, AppError> {
    get_book_info_core(&state, req).await
}

/// `get_chapter_list` 的共享实现 (Tauri command 与 Web HTTP handler 共用)。
pub async fn get_chapter_list_core(
    state: &AppState,
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

    if req
        .book_source_url
        .as_deref()
        .is_some_and(is_local_pdf_origin)
        || req.book_url.as_deref().is_some_and(is_local_pdf_url)
        || req.toc_url.as_deref().is_some_and(is_local_pdf_url)
    {
        let book_url = req
            .book_url
            .as_deref()
            .or(req.toc_url.as_deref())
            .ok_or_else(|| AppError::BadRequest("tocUrl or bookUrl required".to_string()))?;
        let chapters = state
            .local_pdf_book_service
            .get_chapter_list(&user_ns, &repair_encoded_url(book_url))
            .await?;
        return Ok(ApiResponse::ok(
            serde_json::to_value(chapters).unwrap_or_default(),
        ));
    }

    // 空串必须按"没有"处理。
    //
    // Legado 的 `Book.tocUrl` 是非空类型 `String`，从未打开过详情的书在备份里
    // 就是 `"tocUrl": ""`。前端原样透传后，`Some("")` 会命中 `if let Some(u)` 分支，
    // 把目录地址解析成空串 —— 于是既不回落到用 bookUrl 取详情（那是 else 分支），
    // 又拿空 URL 去抓目录，必然失败。表现就是「从手机端备份导入后打开书一直转，
    // 切换一次书源就好了」（换源会把 tocUrl 置空并重新取详情）。
    let req_toc_url = normalized_url_arg(req.toc_url.as_deref());
    let req_book_url = normalized_url_arg(req.book_url.as_deref());

    let source = resolve_book_source(
        &state,
        &user_ns,
        req.book_source_url.clone(),
        req.book_source.clone(),
        req_book_url.as_deref().or(req_toc_url.as_deref()),
    )
    .await?;
    let toc_url = if let Some(u) = req_toc_url {
        u
    } else if let Some(book_url) = req_book_url {
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

    // 刷新时只跳过缓存读取, 不先删缓存: 前端会自动后台刷新目录,
    // 失败的刷新(网络抖动/离线)必须保住旧缓存供下次离线打开; 成功时下方
    // save_chapter_list_cache 自然覆盖。
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
        let state_clone = state.clone();
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

/// `get_chapter_list` 的 Tauri 入口: 只是 core 的薄壳。
#[tauri::command]
pub async fn get_chapter_list(
    state: tauri::State<'_, AppState>,
    req: ChapterListRequest,
) -> Result<ApiResponse<serde_json::Value>, AppError> {
    get_chapter_list_core(&state, req).await
}

/// `get_book_content` 的共享实现。
///
/// `on_stage` 是加载阶段回调。Tauri 侧传 `ipc::Channel`, Web 侧传 `None`
/// (一次性 HTTP 响应没有可推送的中间状态); 传 `None` 时逻辑完全一致, 只是
/// 不产生进度事件。
pub async fn get_book_content_core(
    state: &AppState,
    req: BookContentRequest,
    on_stage: Option<&(dyn Fn(crate::service::book_service::ContentLoadStage) + Send + Sync)>,
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

    if req
        .book_source_url
        .as_deref()
        .is_some_and(is_local_pdf_origin)
        || req.chapter_url.as_deref().is_some_and(is_local_pdf_url)
    {
        let url = req
            .chapter_url
            .as_deref()
            .ok_or_else(|| AppError::BadRequest("chapterUrl required".to_string()))?;
        let chapter_url = if is_local_pdf_url(url) && !url.contains('#') {
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
            .local_pdf_book_service
            .get_content(&user_ns, &chapter_url)
            .await?;
        return Ok(ApiResponse::ok(serde_json::Value::String(content)));
    }

    // Determine book_url and chapter_url
    let (book_url, chapter_url) = if let Some(url) = &req.chapter_url {
        // Check if url looks like a book URL (not a chapter URL) and we have an index
        if req.book_url.is_none()
            && req.index.is_some()
            && !url.contains("/read/")
            && !url.contains("/chapter/")
        {
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
            // 详情规则没取到目录地址时必须回落到书籍地址本身（Legado
            // `BookInfo.kt:158` 同款兜底）。空串会让下面的抓取请求直接失败。
            let toc_url = book_info
                .toc_url
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(repair_encoded_url)
                .unwrap_or_else(|| url.clone());

            // If refresh is requested, delete chapter list cache first
            if do_refresh {
                let _ = state
                    .book_service
                    .delete_chapter_list_cache(&user_ns, &toc_url)
                    .await;
            }

            let mut chapters = state
                .book_service
                .get_chapter_list_with_cache(&user_ns, &source, &toc_url, do_refresh)
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
                    .get_chapter_list_with_cache(&user_ns, &source, &toc_url, true)
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
            // Prefer the explicit book URL so offline reads use the same cache
            // directory that was used when the chapter was cached.
            let book_url = if let Some(book_url) = &req.book_url {
                book_url.clone()
            } else if let Ok(Some(shelf_book)) = state
                .book_service
                .get_shelf_book_by_chapter(&user_ns, url)
                .await
            {
                shelf_book.book_url
            } else {
                url.clone()
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
        Some(&book_url),
    )
    .await?;

    // 只让这一章的"刷新"失效: 清掉本章缓存后, 下面的读取必然未命中并重新联网。
    // 早先这里删的是整本书的缓存, 翻一章刷新会把其它已读章节一并丢掉, 对照
    // legado 的 `delContent(book, bookChapter)`(同样只删单章)。
    if do_refresh {
        let _ = state
            .book_service
            .delete_chapter_cache(&user_ns, &book_url, &chapter_url)
            .await;
    }

    // 阶段回调: 本地已缓存时后端直接返回, 不会有任何阶段事件
    let notify_stage = |stage: crate::service::book_service::ContentLoadStage| {
        if let Some(cb) = on_stage {
            cb(stage);
        }
    };
    let content = state
        .book_service
        .get_content_with_stage(
            &user_ns,
            &book_url,
            &source,
            &chapter_url,
            req.chapter_title.as_deref(),
            Some(&notify_stage),
        )
        .await?;
    Ok(ApiResponse::ok(serde_json::Value::String(content)))
}

/// `get_book_content` 的 Tauri 入口: 把 IPC 通道适配成 core 的回调。
#[tauri::command]
pub async fn get_book_content(
    state: tauri::State<'_, AppState>,
    req: BookContentRequest,
    // 加载阶段通道: 前端总是传(Tauri 的 Channel 不支持 Option),
    // 不关心进度时忽略消息即可。
    on_stage: tauri::ipc::Channel<serde_json::Value>,
) -> Result<ApiResponse<serde_json::Value>, AppError> {
    let sink = |stage: crate::service::book_service::ContentLoadStage| {
        let _ = on_stage.send(serde_json::json!({ "stage": stage }));
    };
    get_book_content_core(&state, req, Some(&sink)).await
}

/// `delete_book_cache` 的共享实现 (Tauri command 与 Web HTTP handler 共用)。
pub async fn delete_book_cache_core(
    state: &AppState,
    req: DeleteCacheRequest,
) -> Result<ApiResponse<serde_json::Value>, AppError> {
    let user_ns = "default";

    // Get book_url (prefer bookUrl, fallback to url)
    let book_url = req
        .book_url
        .or(req.url)
        .ok_or_else(|| AppError::BadRequest("bookUrl required".to_string()))?;

    // Delete all chapter content cache for this book
    let deleted_content = state
        .book_service
        .delete_book_cache(&user_ns, &book_url)
        .await?;

    // 目录缓存**不在这里删**。
    //
    // 「清本地」清的是正文本地缓存, 目录缓存不是正文: 它是打开这本书的先决条件。
    // 一并删掉之后, 再次点开这本书要先把整个目录同步联网抓一遍 —— 单次请求超时
    // 15s、失败重试 2 次, 分页目录还要逐页串行, 于是一分钟内既进不去也退不出
    // (阅读页的离开守卫会等进度写入, 而进度写入当时也要读目录)。目录需要重抓时
    // 走 `get_chapter_list` 的 `refresh: 1` 入口, 不靠删缓存来触发。
    let _ = state
        .book_service
        .delete_book_sources_cache(&user_ns, &book_url)
        .await;
    if let Ok(Some(shelf_book)) = state.book_service.get_shelf_book(&user_ns, &book_url).await {
        if let Some(cover_url) = shelf_book.cover_url.as_deref() {
            let _ = state
                .book_service
                .delete_cover_cache("public", cover_url)
                .await;
        }
        if let Some(cover_url) = shelf_book.custom_cover_url.as_deref() {
            let _ = state
                .book_service
                .delete_cover_cache("public", cover_url)
                .await;
        }
    }

    Ok(ApiResponse::ok(serde_json::json!({
        "deleted": true,
        "contentCache": deleted_content,
        // 目录缓存始终保留, 因此这里恒为 false。
        "chapterListCache": false
    })))
}

/// `delete_book_cache` 的 Tauri 入口: 只是 core 的薄壳。
#[tauri::command]
pub async fn delete_book_cache(
    state: tauri::State<'_, AppState>,
    req: DeleteCacheRequest,
) -> Result<ApiResponse<serde_json::Value>, AppError> {
    delete_book_cache_core(&state, req).await
}

/// Search covers use the unauthenticated `public` namespace. `delete_book`
/// handles shelf books, but search results that were never saved have no book
/// lifecycle event, so the search UI explicitly releases its cover URLs when a
/// search session ends.
#[tauri::command]
pub async fn clear_search_cover_cache(
    state: tauri::State<'_, AppState>,
    req: ClearSearchCoverCacheRequest,
) -> Result<ApiResponse<serde_json::Value>, AppError> {
    let unique_urls: Vec<String> = req
        .cover_urls
        .into_iter()
        .map(|url| url.trim().to_string())
        .filter(|url| !url.is_empty())
        .collect::<std::collections::HashSet<_>>()
        .into_iter()
        .collect();
    let deleted = unique_urls.len();
    state
        .book_service
        .delete_cover_caches("public", &unique_urls)
        .await?;
    crate::api::protocol::remove_cover_cache_urls(&unique_urls);
    Ok(ApiResponse::ok(serde_json::json!({ "deleted": deleted })))
}

/// `get_cached_chapter_urls` 的共享实现 (Tauri command 与 Web HTTP handler 共用)。
pub async fn get_cached_chapter_urls_core(
    state: &AppState,
    req: CachedChapterUrlsRequest,
) -> Result<ApiResponse<Value>, AppError> {
    let user_ns = "default";
    let book_url = req
        .book_url
        .ok_or_else(|| AppError::BadRequest("bookUrl required".to_string()))?;
    let cached_urls = state
        .book_service
        .cached_chapter_urls(&user_ns, &book_url, &req.chapter_urls)
        .await?;
    Ok(ApiResponse::ok(serde_json::json!(cached_urls)))
}

#[tauri::command]
pub async fn get_cached_chapter_urls(
    state: tauri::State<'_, AppState>,
    req: CachedChapterUrlsRequest,
) -> Result<ApiResponse<Value>, AppError> {
    get_cached_chapter_urls_core(&state, req).await
}

/// `get_chapter_word_counts` 的共享实现 (Tauri command 与 Web HTTP handler 共用)。
pub async fn get_chapter_word_counts_core(
    state: &AppState,
    req: CachedChapterUrlsRequest,
) -> Result<ApiResponse<Value>, AppError> {
    let user_ns = "default";
    let book_url = req
        .book_url
        .ok_or_else(|| AppError::BadRequest("bookUrl required".to_string()))?;
    let counts = state
        .book_service
        .chapter_word_counts(&user_ns, &book_url, &req.chapter_urls)
        .await?;
    Ok(ApiResponse::ok(serde_json::json!(counts)))
}

#[tauri::command]
pub async fn get_chapter_word_counts(
    state: tauri::State<'_, AppState>,
    req: CachedChapterUrlsRequest,
) -> Result<ApiResponse<Value>, AppError> {
    get_chapter_word_counts_core(&state, req).await
}

/// `get_bookshelf` 的共享实现 (Tauri command 与 Web HTTP handler 共用)。
pub async fn get_bookshelf_core(state: &AppState) -> Result<ApiResponse<serde_json::Value>, AppError> {
    let user_ns = "default";
    let list = state.book_service.get_bookshelf(&user_ns).await?;
    Ok(ApiResponse::ok(
        serde_json::to_value(list).unwrap_or_default(),
    ))
}

#[tauri::command]
pub async fn get_bookshelf(
    state: tauri::State<'_, AppState>,
) -> Result<ApiResponse<serde_json::Value>, AppError> {
    get_bookshelf_core(&state).await
}

#[tauri::command]
pub async fn upload_txt_book(
    state: tauri::State<'_, AppState>,
    request: tauri::ipc::Request<'_>,
) -> Result<ApiResponse<Value>, AppError> {
    let user_ns = "default";
    // 顺序与改动前一致：先校验大小，再解文件名。
    // `decode_header_file_name` 会因缺 header/空名报错，若放在大小校验之前，
    // "超大文件 + 坏文件名"的报错文案就会与原实现不同。
    let file = decode_upload_body(request.body())?;
    if file.len() > MAX_TXT_UPLOAD_BYTES {
        return Err(AppError::BadRequest("TXT 文件不能超过 50MB".to_string()));
    }
    let file_name = decode_header_file_name(&request)?;
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
    request: tauri::ipc::Request<'_>,
) -> Result<ApiResponse<Value>, AppError> {
    let user_ns = "default";
    let file = decode_upload_body(request.body())?;
    if file.len() > MAX_EPUB_UPLOAD_BYTES {
        return Err(AppError::BadRequest("EPUB 文件不能超过 80MB".to_string()));
    }
    let file_name = decode_header_file_name(&request)?;
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
pub async fn upload_pdf_book(
    state: tauri::State<'_, AppState>,
    request: tauri::ipc::Request<'_>,
) -> Result<ApiResponse<Value>, AppError> {
    let user_ns = "default";
    let file = decode_upload_body(request.body())?;
    if file.len() > MAX_PDF_UPLOAD_BYTES {
        return Err(AppError::BadRequest("PDF 文件不能超过 100MB".to_string()));
    }
    let file_name = decode_header_file_name(&request)?;
    let book = state
        .local_pdf_book_service
        .import_pdf_book(&user_ns, &file_name, &file)
        .await?;
    let saved = state.book_service.save_book(&user_ns, book).await?;
    Ok(ApiResponse::ok(
        serde_json::to_value(saved).unwrap_or_default(),
    ))
}

/// `save_book` 的共享实现 (Tauri command 与 Web HTTP handler 共用)。
///
/// 这里的每一段都不是"顺手加的校验": 书源候选按启用列表过滤、缺 `tocUrl`
/// 时回取详情、本地书改名后迁移阅读统计 —— 任何一条漏在 Web 侧, 同一本书在
/// 浏览器里就会和桌面端表现出不同状态。所以 Web handler 必须调它, 不能
/// 直接 `book_service.save_book()`。
pub async fn save_book_core(
    state: &AppState,
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

    let previous_local_identity = state
        .book_service
        .get_shelf_book(&user_ns, &req.book_url)
        .await?
        .filter(|book| is_local_book(book))
        .map(|book| {
            (
                book.name.trim().to_string(),
                book.author.trim().to_string(),
            )
        });

    if let Some(candidates) = req.source_candidates.as_mut() {
        let enabled_urls = state
            .book_source_service
            .list_enabled(&user_ns)
            .await?
            .into_iter()
            .map(|source| normalize_source_url(&source.book_source_url))
            .collect::<std::collections::HashSet<_>>();
        candidates.retain(|candidate| {
            enabled_urls.contains(&normalize_source_url(&candidate.origin))
        });
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
    let previous_identity = previous_local_identity
        .as_ref()
        .map(|(name, author)| (name.as_str(), author.as_str()));
    if local_book_rename_needs_stats_migration(previous_identity, &saved) {
        // 书架已经写入成功, 这一步只是把历史统计改挂到新名字上。失败必须吞掉:
        // 若在这里返回错误, 前端会显示"保存失败"而书名其实已经改了; 用户重试时
        // 旧名已等于新名, 迁移条件不再成立 —— 历史时长就永久留在旧书名下。
        if let Err(err) = state
            .reading_stats_service
            .rename_book_stats(&user_ns, &saved.book_url, &saved.name, &saved.author)
            .await
        {
            tracing::warn!(
                book_url = %saved.book_url,
                error = %err,
                "本地书改名后迁移阅读统计失败, 历史时长仍挂在旧书名下"
            );
        }
    }
    Ok(ApiResponse::ok(
        serde_json::to_value(saved).unwrap_or_default(),
    ))
}

/// `save_book` 的 Tauri 入口: 只是 core 的薄壳。
#[tauri::command]
pub async fn save_book(
    state: tauri::State<'_, AppState>,
    req: Book,
) -> Result<ApiResponse<Value>, AppError> {
    save_book_core(&state, req).await
}

/// `save_books` 的共享实现 (批量导入/同步走这里)。
pub async fn save_books_core(
    state: &AppState,
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

/// `save_books` 的 Tauri 入口: 只是 core 的薄壳。
#[tauri::command]
pub async fn save_books(
    state: tauri::State<'_, AppState>,
    req: Vec<Book>,
) -> Result<ApiResponse<Value>, AppError> {
    save_books_core(&state, req).await
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
    let old_book_url_clone = old_book_url.clone();
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

    // 换源前从旧书缓存读取候选, 既用于元数据回填, 也用于为新书 URL 预置缓存。
    let mut candidates_for_seed: Vec<SearchBook> = Vec::new();
    if let Some(candidates) = state
        .book_service
        .load_book_sources_cache(&user_ns, &old_book_url)
        .await?
    {
        if let Some(candidate) = candidates
            .clone()
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
        candidates_for_seed = candidates;
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

    // 换源后为「新书 URL」预置一份可读书源缓存, 包含换源前已知的全部候选。
    // 否则新书第一次打开换源面板只会触发全新扫描: 热门书可能因部分源搜到
    // 但漏掉发起方源自身被前端过滤, 导致「从 A 换到 B 后 B 看到的不含 A」。
    // 用同一作品的候选集(去重、剔除当前源)预填, 让换源可立即双向看到。
    let mut seeded: Vec<SearchBook> = Vec::new();
    let mut seen_seed = std::collections::HashSet::new();
    for candidate in candidates_for_seed {
        if is_current_source_candidate(&candidate, &saved) {
            continue;
        }
        if seen_seed.insert(available_source_sse_result_key(&candidate)) {
            seeded.push(candidate);
        }
    }
    if old_book_url_clone != saved.book_url && !seeded.is_empty() {
        let _ = state
            .book_service
            .save_book_sources_cache(&user_ns, &saved.book_url, &seeded)
            .await;
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
    cleanup_local_book_files(&state, &user_ns, &removed_books).await;
    Ok(ApiResponse::ok(serde_json::json!({"deleted": count})))
}

/// `save_book_progress` 的共享实现 (Tauri command 与 Web HTTP handler 共用)。
pub async fn save_book_progress_core(
    state: &AppState,
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
    } else if is_local_pdf_origin(&shelf_book.origin) || is_local_pdf_url(&shelf_book.book_url) {
        if let Ok(chapters) = state
            .local_pdf_book_service
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
    } else if let Some(toc_url) = shelf_book
        .toc_url
        .as_deref()
        .filter(|url| !url.trim().is_empty())
    {
        // 只用**目录缓存**补章节标题, 绝不在这里联网抓目录。
        //
        // 保存阅读进度是每几秒一次、且离开阅读页时必须完成的高频操作。此前这里走
        // `get_chapter_list`, 目录缓存缺失时会同步抓整棵目录(单请求 15s 超时 ×
        // 3 次尝试, 分页目录还要逐页串行), 于是「清本地」之后点开书再返回, 导航会
        // 被离开守卫卡到近一分钟。缓存没有就跳过 —— 标题只是展示信息, 下次真正
        // 加载目录时会自然补上。
        if let Ok(Some(chapters)) = state
            .book_service
            .load_chapter_list_cache(&user_ns, toc_url)
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

/// `save_book_progress` 的 Tauri 入口: 只是 core 的薄壳。
#[tauri::command]
pub async fn save_book_progress(
    state: tauri::State<'_, AppState>,
    req: SaveBookProgressRequest,
) -> Result<ApiResponse<Value>, AppError> {
    save_book_progress_core(&state, req).await
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

/// `get_shelf_book_with_cache_info` 的共享实现 (Tauri command 与 Web HTTP handler 共用)。
pub async fn get_shelf_book_with_cache_info_core(
    state: &AppState,
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
        let state_clone = state.clone();
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

/// `get_shelf_book_with_cache_info` 的 Tauri 入口: 只是 core 的薄壳。
#[tauri::command]
pub async fn get_shelf_book_with_cache_info(
    state: tauri::State<'_, AppState>,
) -> Result<ApiResponse<Value>, AppError> {
    get_shelf_book_with_cache_info_core(&state).await
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

    // 阅读页可传入明确的章节地址；首页未传时从书架保存的阅读进度开始。
    let start_ch_url = req.toc_url.as_deref();
    let saved_start_index = book.dur_chapter_index;
    let cache_count = req.count.unwrap_or(0); // 0 means all

    let mut chapters = state
        .book_service
        .get_chapter_list(&user_ns, &source, &root_toc_url)
        .await?;

    let chapter_urls = chapters
        .iter()
        .map(|chapter| chapter.url.clone())
        .collect::<Vec<_>>();
    let start_index = resolve_cache_start_index(
        &chapter_urls,
        start_ch_url,
        saved_start_index,
    );
    if start_index > 0 {
        chapters = chapters.split_off(start_index);
    }

    // Limit count if requested
    if cache_count > 0 && cache_count < chapters.len() as i32 {
        chapters.truncate(cache_count as usize);
    }

    if chapters.is_empty() {
        return Err(AppError::BadRequest("没有找到需要缓存的章节".to_string()));
    }

    let state_clone = state.inner().clone();
    // 包成 Arc: 书源里可能带几百 KB 的 jsLib, 逐章 clone 一次在千章书上就是
    // 数百 MB 的无谓拷贝。调度器每章只需要一份共享引用。
    let source_clone = Arc::new(source.clone());
    let book_url_clone = book.book_url.clone();
    let user_ns_clone = user_ns;

    // 登记取消标志；同一本书重复发起时取消旧任务。
    let cancel_flag = Arc::new(AtomicBool::new(false));
    if let Ok(mut tasks) = CACHE_BOOK_CANCELLATIONS.lock() {
        if let Some(previous) = tasks.insert(book_url_clone.clone(), cancel_flag.clone()) {
            previous.store(true, Ordering::Relaxed);
        }
    }

    let registry_key = book_url_clone.clone();
    let cleanup_flag = cancel_flag.clone();
    tokio::spawn(async move {
        // 目录里可能有重复的章节地址（同一章被列多次）：去重后再算总数，
        // 否则同一章被缓存两次会被计两次，进度会出现 X > Y 或比例虚高。
        let chapters = dedup_chapters(chapters);
        let total = chapters.len();
        let all_urls: Vec<String> = chapters.iter().map(|ch| ch.url.clone()).collect();
        let cancelled = || cancel_flag.load(Ordering::Relaxed);

        async {
            // 批量取回「已缓存章节」地址集合：原先逐章 is_chapter_cached().await，
            // 几千章就要几千次顺序 await，这段时间界面停在 0/… 看起来像卡死。
            // 同一份集合随后用于过滤待缓存章节，省掉第二次逐章查询。
            let cached_urls: std::collections::HashSet<String> = if refresh {
                std::collections::HashSet::new()
            } else {
                state_clone
                    .book_service
                    .cached_chapter_urls(&user_ns_clone, &book_url_clone, &all_urls)
                    .await
                    .unwrap_or_default()
                    .into_iter()
                    .collect()
            };
            if cancelled() {
                emit_cache_end(&on_event, total, 0, 0, 0, true);
                return;
            }
            let mut cached_count = cached_urls.len();
            let mut success = 0usize;
            let mut failed = 0usize;
            let mut aborted = false;
            let _ = on_event.send(serde_json::json!({
                "event": "data",
                "totalChapters": total,
                "cachedCount": cached_count,
                "successCount": success,
                "failedCount": failed
            }));

            let tasks_pending: Vec<String> = all_urls
                .into_iter()
                .filter(|url| !cached_urls.contains(url))
                .collect();

            // 边派发边收集：并发窗口保持填满，每完成一章立刻上报进度。
            let svc = state_clone.book_service.clone();
            let src = source_clone.clone();
            let b_url = book_url_clone.clone();
            let (run_success, run_failed, run_aborted) = run_cache_tasks(
                tasks_pending,
                concurrent,
                &cancelled,
                move |url| {
                    let svc = svc.clone();
                    let src = src.clone();
                    let b_url = b_url.clone();
                    async move { svc.cache_chapter(&user_ns_clone, &b_url, &src, &url, refresh).await }
                },                |success, failed| {
                    // success/failed 为该调度器内的累计值，映射到全局计数
                    let _ = on_event.send(serde_json::json!({
                        "event": "data",
                        "totalChapters": total,
                        "cachedCount": cached_count + success,
                        "successCount": success,
                        "failedCount": failed
                    }));
                },
            )
            .await;
            success += run_success;
            failed += run_failed;
            cached_count += run_success;
            aborted = aborted || run_aborted;

            emit_cache_end(
                &on_event,
                total,
                cached_count,
                success,
                failed,
                aborted || cancelled(),
            );
        }
        .await;

        // 移除登记；若已被同书新任务覆盖则保留新任务的标志。
        if let Ok(mut tasks) = CACHE_BOOK_CANCELLATIONS.lock() {
            if let Some(current) = tasks.get(&registry_key) {
                if Arc::ptr_eq(current, &cleanup_flag) {
                    tasks.remove(&registry_key);
                }
            }
        }
    });

    Ok(())
}

/// 按章节地址去重（保持原有顺序，保留首次出现的那条）。
///
/// 目录里同一章可能被列多次（分卷/重复锚点）。不去重的话同一章会被缓存两次并
/// 各计一次进度，出现 `cachedCount > totalChapters` 或比例虚高。
fn dedup_chapters(chapters: Vec<BookChapter>) -> Vec<BookChapter> {
    let mut seen = std::collections::HashSet::new();
    chapters
        .into_iter()
        .filter(|chapter| seen.insert(chapter.url.clone()))
        .collect()
}

/// 并发缓存章节的调度器：**边派发边收集**，每完成一章回调一次累计进度。
///
/// 抽成独立函数是为了能被测试覆盖：「先派发完全部、再统一收集」的写法会让整个
/// 缓存过程没有任何进度输出 —— 派发循环被并发窗口卡住时，已完成的任务没人收集，
/// 用户看到的是进度条长时间不动、最后一步跳满。
async fn run_cache_tasks<F, Fut>(
    urls: Vec<String>,
    concurrent: usize,
    cancelled: impl Fn() -> bool,
    load: F,
    mut on_progress: impl FnMut(usize, usize),
) -> (usize, usize, bool)
where
    F: Fn(String) -> Fut + Send + 'static,
    Fut: std::future::Future<Output = Result<(), AppError>> + Send + 'static,
{
    let concurrent = concurrent.max(1);
    let mut tasks: FuturesUnordered<tokio::task::JoinHandle<Result<(), AppError>>> =
        FuturesUnordered::new();
    let mut pending = urls.into_iter();
    let mut success = 0usize;
    let mut failed = 0usize;
    let mut aborted = false;

    loop {
        // 取消检测放在最前面：即使 `pending` 已空（章节都派发完了），只要还有在途
        // 请求也要把 aborted 反映出来，否则返回值无法独立表达"是否被取消"。
        if !aborted && cancelled() {
            aborted = true;
        }
        // 补满并发窗口。取消后不再补派，已在途的请求等它自然结束（受 HTTP 超时约束）。
        while !aborted && tasks.len() < concurrent {
            match pending.next() {
                Some(url) => tasks.push(tokio::spawn(load(url))),
                None => break,
            }
        }

        let Some(task) = tasks.next().await else { break };
        match task {
            Ok(Ok(())) => success += 1,
            _ => failed += 1,
        }
        on_progress(success, failed);
    }

    (success, failed, aborted)
}

fn emit_cache_end(
    on_event: &tauri::ipc::Channel<serde_json::Value>,
    total: usize,
    cached_count: usize,
    success: usize,
    failed: usize,
    aborted: bool,
) {
    let _ = on_event.send(serde_json::json!({
        "event": "end",
        "totalChapters": total,
        "cachedCount": cached_count,
        "successCount": success,
        "failedCount": failed,
        "aborted": aborted
    }));
}

#[tauri::command]
pub async fn cancel_cache_book(
    req: CancelCacheBookRequest,
) -> Result<ApiResponse<serde_json::Value>, AppError> {
    let cancelled = CACHE_BOOK_CANCELLATIONS
        .lock()
        .ok()
        .and_then(|tasks| tasks.get(&req.book_url).cloned())
        .map(|flag| {
            flag.store(true, Ordering::Relaxed);
            true
        })
        .unwrap_or(false);
    Ok(ApiResponse::ok(serde_json::json!({ "cancelled": cancelled })))
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
    // 前端离开搜索界面时只能关掉 channel, 后端感知不到; 登记取消标志后,
    // cancel_book_search 才能让这个任务停下并掐断剩余书源。
    let cancel_guard = SearchCancellationGuard::register(req.task_id.as_deref());

    tokio::spawn(async move {
        if key.trim().is_empty() {
            let _ = on_event.send(json_err("请输入搜索关键字"));
            let _ = on_event.send(json_end(last_index));
            return;
        }

        let sources = if let Some(url) = book_source_url {
            match state_clone.book_source_service.get(&user_ns, &url).await {
                Ok(Some(s)) if s.is_enabled() => vec![s],
                _ => {
                    let _ = on_event.send(json_err("书源未启用或已删除"));
                    let _ = on_event.send(json_end(last_index));
                    return;
                }
            }
        } else {
            match state_clone.book_source_service.list_enabled(&user_ns).await {
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
            // 取消检查必须在等待任务完成之前: 放到 tasks.next().await 之后的话,
            // 要等某个源搜完才会轮询到, 前端离开界面后的响应会延迟一整个源请求。
            if cancel_guard.is_cancelled() {
                abort_all_search_tasks(&mut tasks);
                break;
            }
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

            // 用 select 让取消能在等待期间被发现; 否则整个 await 期间无法响应。
            let next = tokio::select! {
                res = tasks.next() => res,
                _ = tokio::time::sleep(std::time::Duration::from_millis(100)) => continue,
            };
            if let Some(res) = next {
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
    let cancel_guard = SearchCancellationGuard::register(req.task_id.as_deref());

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

        let sources = match state_clone.book_source_service.list_enabled(&user_ns).await {
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
        let mut seen = std::collections::HashSet::new();
        let mut search_failed = false;

        while (idx as usize) < sources.len() || !tasks.is_empty() {
            if cancel_guard.is_cancelled() {
                abort_all_search_tasks(&mut tasks);
                // 必须补一个 end: 否则将来若有「停止搜索」按钮(只发取消、不关流),
                // 前端的 isSearching 会永远卡在加载态。这里同时跳过后面的缓存写入,
                // 避免把半截结果固化 (与 multi/available 两处行为对齐)。
                let _ = on_event.send(json_end(last_idx));
                return;
            }
            while tasks.len() < concurrent && (idx as usize) < sources.len() && total < search_size {
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
                idx += 1;
            }

            let next = tokio::select! {
                res = tasks.next() => res,
                _ = tokio::time::sleep(std::time::Duration::from_millis(100)) => continue,
            };
            if let Some(res) = next {
                if let Ok((cur_idx, Ok(list), target_name, target_author)) = res {
                    last_idx = last_idx.max(cur_idx);
                    let batch = take_available_source_sse_matches(
                        list, &target_name, &target_author, &mut seen, usize::MAX,
                    );
                    if !batch.is_empty() {
                        total += batch.len();
                        all_results.extend(batch.clone());
                        let payload = serde_json::json!({"event": "data", "lastIndex": cur_idx, "data": batch});
                        let _ = on_event.send(payload);
                    }
                } else {
                    search_failed = true;
                }
            } else {
                break;
            }
        }

        last_idx = last_idx.max(idx - 1);
        if last_index < 0 && idx as usize >= sources.len() && tasks.is_empty()
            && !search_failed && (refresh || !all_results.is_empty())
        {
            let _ = state_clone
                .book_service
                .save_book_sources_cache(&user_ns, &book.book_url, &all_results)
                .await;
        }
        let _ = on_event.send(serde_json::json!({
            "event": "end", "lastIndex": last_idx,
            "hasMore": (idx as usize) < sources.len(), "hasErrors": search_failed
        }));
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
    let sources = state.book_source_service.list_enabled(&user_ns).await?;
    let enabled_source_url_set = enabled_source_urls(&sources);

    if !paged_request {
        if let Some(ref url) = book_url {
            if let Some(list) = state
                .book_service
                .load_book_sources_cache(&user_ns, url)
                .await?
            {
                let list = retain_enabled_source_books(list, &enabled_source_url_set);
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
    if sources.is_empty() {
        if paged_request {
            return Ok(ApiResponse::ok(
                serde_json::to_value(build_available_book_source_response(
                    Vec::new(),
                    req.last_index.unwrap_or(-1),
                    false,
                    false,
                ))
                .unwrap_or_default(),
            ));
        }
        return Ok(ApiResponse::ok(serde_json::json!([])));
    }

    let mut result: Vec<SearchBook> = Vec::new();
    let mut search_failed = false;
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
                        search_failed = true;
                        tracing::debug!(
                            "getAvailableBookSource search failed at source index {}: {:?}",
                            source_index,
                            err
                        );
                        Vec::new()
                    }
                };
                batch_results.push((source_index, matches));
            } else {
                search_failed = true;
            }
        }

        batch_results.sort_by_key(|(source_index, _)| *source_index);
        for (source_index, matches) in batch_results {
            if result.len() >= result_limit {
                break;
            }
            last_index = source_index;
            result.extend(matches);
        }

        cursor = batch_end;
        if result.len() >= result_limit {
            break;
        }
        // Failed parsing tasks also consume their source index.
        last_index = batch_end as i32 - 1;
    }

    let has_more = (last_index + 1).max(0) < sources.len() as i32;
    if !has_more && !search_failed && req.last_index.unwrap_or(-1) < 0 {
        // 缓存保留聚合源的其他书籍地址，完整列表才在再次打开面板时复用。
        let cache_list: Vec<SearchBook> = result
            .iter()
            .filter(|candidate| !is_current_source_candidate(candidate, &book))
            .cloned()
            .collect();
        match state.book_source_service.list_enabled(&user_ns).await {
            Ok(current_sources) if enabled_source_urls(&current_sources) == enabled_source_url_set => {
                let _ = state
                    .book_service
                    .save_book_sources_cache(&user_ns, &book.book_url, &cache_list)
                    .await;
            }
            Ok(_) => {
                tracing::debug!(
                    "getAvailableBookSource skipped stale cache write after source changes"
                );
            }
            Err(err) => {
                tracing::warn!(
                    "getAvailableBookSource skipped cache write because source list could not be verified: {:?}",
                    err
                );
            }
        }
    }

    if paged_request || search_failed {
        let response =
            build_available_book_source_response(result, last_index, has_more, search_failed);
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
    // 登记必须放在所有提前 return 之前: 下面的「缓存命中」分支也会提前 return,
    // 若登记晚于它, 该 taskId 永远不会进入取消表 —— 前端离开界面时发出的取消
    // 只能落进 pending 而无人消费(既泄漏记录, 也让这段缓存推送无法被取消)。
    let cancel_guard = SearchCancellationGuard::register(req.task_id.as_deref());
    let mut sources = state.book_source_service.list_enabled(&user_ns).await?;
    let enabled_source_url_set = enabled_source_urls(&sources);

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
    // The detail view already loads the current source's catalog. Searching
    // that same source again competes with the catalog for its rate limit and
    // can push later book opens behind stale work.
    sources.retain(|source| {
        source_identity_url(&source.book_source_url) != source_identity_url(&book.origin)
    });

    if !refresh && last_index_start < 0 {
        if let Some(ref url) = book_url {
            if let Some(cached) = state
                .book_service
                .load_book_sources_cache(&user_ns, url)
                .await?
            {
                let cached = retain_enabled_source_books(cached, &enabled_source_url_set);
                let cached = take_available_source_cached_matches(
                    cached,
                    usize::MAX,
                );
                let on_event_clone = on_event.clone();
                // 缓存条目多时这段推送本身也可能耗时; 用户离开界面后应能停下。
                //
                // 这里必须 **move 守卫本体**, 不能只 clone 一个副本: clone 出的副本
                // `registered` 为 None(不负责注销), 而原守卫若在此 `return Ok(())` 时
                // Drop, 就会立刻把该 taskId 从取消表移除 —— 之后前端发的
                // cancel_book_search 查不到登记, 只能落进 pending 且无人消费(等 TTL),
                // 那个仍在推送的任务持有的 flag 便再没有人能置位, 与"用户离开界面后
                // 应能停下"的承诺不符。move 进去后, 注销发生在推送真正结束
                // (或 channel 关闭而提前 return) 的那一刻。
                tokio::spawn(async move {
                    let mut last_index = -1;
                    for book in cached {
                        if cancel_guard.is_cancelled() {
                            return;
                        }
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
        let scan_limit = effective_available_result_limit(req.result_limit)
            .min(AVAILABLE_SOURCE_SSE_SCAN_SOFT_LIMIT);
        let mut all_results: Vec<SearchBook> = Vec::new();
        let mut seen = std::collections::HashSet::<String>::new();
        let mut tasks = JoinSet::new();
        let mut cancelled = false;
        let mut search_failed = false;

        while next_index < sources.len() || !tasks.is_empty() {
            if cancel_guard.is_cancelled() {
                cancelled = true;
                tasks.abort_all();
                break;
            }
            while tasks.len() < concurrent_count
                && next_index < sources.len()
                && all_results.len() < scan_limit
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
            if tasks.is_empty() {
                break;
            }

            let next = tokio::select! {
                res = tasks.join_next() => res,
                _ = tokio::time::sleep(std::time::Duration::from_millis(100)) => continue,
            };
            match next {
                Some(Ok((source_index, search_result, target_name, target_author))) => {
                    last_idx = last_idx.max(source_index);
                    match search_result {
                        Ok(list) => {
                            let matches = take_available_source_sse_matches(
                                list,
                                &target_name,
                                &target_author,
                                &mut seen,
                                usize::MAX,
                            );
                            // Return every candidate from each completed source. A
                            // source-index cursor cannot resume halfway through its list.
                            for candidate in matches {
                                all_results.push(candidate.clone());
                                let payload = serde_json::json!({
                                    "event": "data",
                                    "lastIndex": source_index,
                                    "hasMore": next_index < sources.len() || !tasks.is_empty(),
                                    "data": [candidate]
                                });
                                if on_event.send(payload).is_err() {
                                    tasks.abort_all();
                                    return;
                                }
                            }
                        }
                        Err(err) => {
                            search_failed = true;
                            tracing::debug!(
                                "getAvailableBookSourceSSE search failed at source index {}: {:?}",
                                source_index,
                                err
                            );
                        }
                    }
                }
                Some(Err(err)) => {
                    search_failed = true;
                    tracing::debug!("getAvailableBookSourceSSE task join failed: {:?}", err);
                }
                None => break,
            }
        }
        if !cancelled {
            last_idx = next_index as i32 - 1;
        }

        // 达到后台软上限时，仍可能有尚未扫描的书源；保留“加载更多”，
        // 让前端可以从最后一个已处理的源继续扫描，而不是把剩余书源隐藏掉。
        let has_more = next_index < sources.len();
        let final_last_idx = last_idx;

        // 完整扫描后保存全部候选。仅排除当前书籍地址，聚合源内的其他地址保留。
        // 被取消时同样不写: 这份结果是半截的, 写进去会把「还没扫到的源」当成
        // 「搜不到这本书」固化下来 (与 `has_more` 同理)。
        if last_index_start < 0 && !has_more && !cancelled && !search_failed {
            let cache_list: Vec<SearchBook> = all_results
                .iter()
                .filter(|candidate| !is_current_source_candidate(candidate, &book))
                .cloned()
                .collect();
            match state_clone.book_source_service.list_enabled(&user_ns).await {
                Ok(current_sources) if enabled_source_urls(&current_sources) == enabled_source_url_set => {
                    let _ = state_clone
                        .book_service
                        .save_book_sources_cache(&user_ns, &book.book_url, &cache_list)
                        .await;
                }
                Ok(_) => {
                    tracing::debug!(
                        "getAvailableBookSourceSSE skipped stale cache write after source changes"
                    );
                }
                Err(err) => {
                    tracing::warn!(
                        "getAvailableBookSourceSSE skipped cache write because source list could not be verified: {:?}",
                        err
                    );
                }
            }
        }

        let _ = on_event.send(serde_json::json!({
            "event": "end",
            "lastIndex": final_last_idx,
            "hasMore": has_more,
            "hasErrors": search_failed
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

/// 取消搜索时掐掉所有在途的单源搜索任务。
///
/// **已知限制(非立即停止)**: `JoinHandle::abort` 只在任务的下一个 await 点生效。单源
/// 解析 `parser.search_books` 是同步调用, `js:` 规则里还会走 blocking HTTP(30s 超时),
/// 正卡在这一段的任务会把当前这一步跑完才结束。所以取消保证的是「不再派发新书源,
/// 且已派发的不再往下走」, 而不是「在途请求立刻断开」。
///
/// 只用于「即将 drop 掉集合」的场景: 这里逐个 abort 后调用方立即 break/return,
/// 不再 poll 这些句柄。
fn abort_all_search_tasks<T>(tasks: &mut FuturesUnordered<tokio::task::JoinHandle<T>>) {
    for task in tasks.iter() {
        task.abort();
    }
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
            // 书源地址在书源表里是**主键字符串**，而书架的 `origin` 来自备份/详情页，
            // 两边经常只差一个尾斜杠（Legado 导出 `https://site.com/`，详情页规则
            // 拼出来的可能是 `https://site.com`）。精确查失败后必须按同一口径比较，
            // 否则会误报「书源不存在」——用户看到的是打开书一直转圈或换源列表空白。
            let identity = source_identity_url(&normalized);
            let sources = state.book_source_service.list(&user_ns).await?;
            if let Some(src) = sources
                .into_iter()
                .find(|s| source_identity_url(&s.book_source_url) == identity)
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
                let identity = source_identity_url(&shelf_origin);
                let sources = state.book_source_service.list(&user_ns).await?;
                if let Some(src) = sources
                    .into_iter()
                    .find(|s| source_identity_url(&s.book_source_url) == identity)
                {
                    return Ok(src);
                }
            }
        }
    }

    // Auto-discovery from book_url
    if let Some(b_url) = book_url {
        let b_host = match url::Url::parse(b_url) {
            Ok(u) => u.host_str().unwrap_or_default().to_lowercase(),
            Err(_) => "".to_string(),
        };
        if !b_host.is_empty() {
            // Extract root domain for comparison (e.g., "novelx" from "m.novelx.com")
            let b_root = extract_root_domain(&b_host);
            let sources = state.book_source_service.list(&user_ns).await?;
            for s in sources {
                let normalized_source_url = normalize_source_url(&s.book_source_url);
                if let Ok(s_url) = url::Url::parse(&normalized_source_url) {
                    if let Some(s_host) = s_url.host_str().map(str::to_lowercase) {
                        // Match by exact host or by root domain
                        let s_root = extract_root_domain(&s_host);
                        if b_host == s_host
                            || b_host.ends_with(&format!(".{s_host}"))
                            || s_host.ends_with(&format!(".{b_host}"))
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

/// Extract root domain for matching (e.g., "novelx" from "m.novelx.com" or "m.novelx.net")
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
    // 空串/空白目录地址必须当成"没有"：换源时上面刚把 toc_url 置为 None，
    // 若详情规则也没给出目录地址，`info.toc_url` 可能仍是 `Some("")`
    // （Legado 的 Kotlin 非空 String 在这里被序列化成空串）。写回去就变成
    // 「有 tocUrl 但是空」的状态 —— 打开书时既不补详情、又拿空 URL 抓目录，
    // 于是一直转圈。
    let info_toc_url = info
        .toc_url
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    if target.toc_url.as_deref().map(str::trim).unwrap_or("").is_empty() {
        target.toc_url = info_toc_url;
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
        } else if is_local_pdf_origin(&book.origin) || is_local_pdf_url(&book.book_url) {
            if let Err(err) = state
                .local_pdf_book_service
                .delete_book_files(user_ns, &book.book_url)
                .await
            {
                tracing::warn!(
                    "failed to delete local pdf book files for {}: {:?}",
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
        .map(|limit| limit.clamp(1, MAX_AVAILABLE_RESULT_LIMIT as i32) as usize)
        .unwrap_or(usize::MAX)
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
    books: Vec<SearchBook>,
    last_index: i32,
    has_more: bool,
    has_errors: bool,
) -> AvailableBookSourceResponse {
    // lastIndex addresses sources, so trimming a source's candidates here
    // would make the omitted books unreachable on the next request.
    AvailableBookSourceResponse {
        books,
        last_index,
        has_more,
        has_errors,
    }
}

fn cache_count_for_shelf_display(book: &Book, cached_count: usize) -> usize {
    if is_local_book(book) {
        0
    } else {
        cached_count
    }
}

fn resolve_cache_start_index(
    chapter_urls: &[String],
    requested_start_url: Option<&str>,
    saved_start_index: Option<i32>,
) -> usize {
    if chapter_urls.is_empty() {
        return 0;
    }
    if let Some(index) = requested_start_url
        .and_then(|url| chapter_urls.iter().position(|chapter_url| chapter_url == url))
    {
        return index;
    }
    (saved_start_index.unwrap_or(0).max(0) as usize).min(chapter_urls.len() - 1)
}

fn is_local_book(book: &Book) -> bool {
    is_local_txt_origin(&book.origin)
        || is_local_txt_url(&book.book_url)
        || is_local_epub_origin(&book.origin)
        || is_local_epub_url(&book.book_url)
        || is_local_pdf_origin(&book.origin)
        || is_local_pdf_url(&book.book_url)
}

/// 保存本地书后是否需要把历史统计改挂到新的书名/作者上。
///
/// 三个条件缺一不可:
/// - 本地书: 远程书的身份由 `book_url` 决定, 换书源时统计靠书名分组已经能合并,
///   而本地书改名前后 `book_url` 不变, 只能靠这里主动迁移;
/// - 书名或作者真的变了: 值没变时跑一次 UPDATE 是无意义的写入;
/// - 是从书架里已有的条目改名而来(`previous` 为 Some): 首次加入书架没有历史行
///   可迁移, 此时不该去动别的书的统计。
fn local_book_rename_needs_stats_migration(
    previous_identity: Option<(&str, &str)>,
    saved: &Book,
) -> bool {
    let Some((old_name, old_author)) = previous_identity else {
        return false;
    };
    is_local_book(saved)
        && (old_name.trim() != saved.name.trim() || old_author.trim() != saved.author.trim())
}

fn is_current_source_candidate(candidate: &SearchBook, current: &Book) -> bool {
    source_identity_url(&candidate.origin) == source_identity_url(&current.origin)
        && candidate.book_url == current.book_url
}

fn available_source_sse_result_key(book: &SearchBook) -> String {
    format!("{}::{}", source_identity_url(&book.origin), book.book_url)
}

fn enabled_source_urls(sources: &[BookSource]) -> std::collections::HashSet<String> {
    sources
        .iter()
        .map(|source| source_identity_url(&source.book_source_url))
        .collect()
}

fn source_identity_url(value: &str) -> String {
    let normalized = normalize_source_url(value);
    if normalized.starts_with("http://") || normalized.starts_with("https://") {
        normalized.trim_end_matches('/').to_string()
    } else {
        normalized
    }
}

/// 请求里的可选 URL 参数：空白串一律视为"未提供"。
///
/// 前端会把书架记录原样回传，而 Legado 的书架/备份里 `tocUrl` 常是 `""`
/// （Kotlin 侧 `var tocUrl: String = ""`，非空类型）。若把 `Some("")` 当成
/// 有效值，调用链就会拿空 URL 去抓目录，既不回落也不报可读错误。
fn normalized_url_arg(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(repair_encoded_url)
}

fn retain_enabled_source_books(
    books: Vec<SearchBook>,
    enabled_urls: &std::collections::HashSet<String>,
) -> Vec<SearchBook> {
    books
        .into_iter()
        .filter(|book| enabled_urls.contains(&source_identity_url(&book.origin)))
        .collect()
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
        cache_count_for_shelf_display, dedup_chapters, fallback_available_book,
        merge_book, normalized_url_arg,
        resolve_cache_start_index, retain_enabled_source_books, run_cache_tasks,
        should_use_available_source_cache, is_current_source_candidate,
        effective_available_result_limit,
        local_book_rename_needs_stats_migration,
        source_identity_url,
        take_available_source_cached_matches,
        take_available_source_sse_matches, GetAvailableBookSourceRequest,
        SearchCancellationGuard, SEARCH_CANCELLATIONS, SEARCH_CANCEL_REQUESTS,
        SEARCH_CANCEL_REQUEST_TTL,
    };
    use crate::error::error::AppError;
    use crate::model::book_chapter::BookChapter;
    use crate::model::{book::Book, search::SearchBook};
    use futures::StreamExt;
    use std::collections::HashSet;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};

    /// 目录里重复的章节地址必须去重：否则同一章被缓存两次、各计一次进度，
    /// 会出现 `cachedCount > totalChapters`。
    #[test]
    fn dedup_chapters_keeps_first_occurrence() {
        let chapters = vec![
            BookChapter { title: "第一章".into(), url: "u1".into(), ..Default::default() },
            BookChapter { title: "第一章(重复)".into(), url: "u1".into(), ..Default::default() },
            BookChapter { title: "第二章".into(), url: "u2".into(), ..Default::default() },
            BookChapter { title: "第一章(再次)".into(), url: "u1".into(), ..Default::default() },
            BookChapter { title: "第三章".into(), url: "u3".into(), ..Default::default() },
        ];

        let deduped = dedup_chapters(chapters);

        let urls: Vec<&str> = deduped.iter().map(|c| c.url.as_str()).collect();
        assert_eq!(urls, vec!["u1", "u2", "u3"], "应保留首次出现并维持原顺序");
        assert_eq!(deduped[0].title, "第一章", "重复项应保留首次出现的那条");
    }

    #[test]
    fn dedup_chapters_leaves_unique_list_untouched() {
        let chapters: Vec<BookChapter> = (0..5)
            .map(|i| BookChapter {
                title: format!("第{i}章"),
                url: format!("u{i}"),
                ..Default::default()
            })
            .collect();
        let deduped = dedup_chapters(chapters);
        assert_eq!(deduped.len(), 5);
    }

    fn local_txt_book(name: &str, author: &str) -> Book {
        Book {
            name: name.to_string(),
            author: author.to_string(),
            book_url: "local-txt:abc123".to_string(),
            origin: "local-txt".to_string(),
            ..Default::default()
        }
    }

    /// 书架详情页把本地书改了个名字: 必须迁移历史统计, 否则旧名字下的时长
    /// 会和改名后的阅读记录分成两条。
    #[test]
    fn local_book_rename_migrates_stats() {
        let saved = local_txt_book("新书名", "本地导入");
        assert!(local_book_rename_needs_stats_migration(
            Some(("旧书名", "本地导入")),
            &saved,
        ));
    }

    /// 只改作者也算改名: 统计展示按 (书名, 作者) 联合分组, 作者变了照样会被拆开。
    #[test]
    fn local_book_author_change_migrates_stats() {
        let saved = local_txt_book("旧书名", "新作者");
        assert!(local_book_rename_needs_stats_migration(
            Some(("旧书名", "旧作者")),
            &saved,
        ));
    }

    /// 值没变(含首尾空白差异收敛后相等)时不该跑一次无谓的 UPDATE。
    #[test]
    fn unchanged_local_book_identity_skips_migration() {
        let saved = local_txt_book("旧书名", "本地导入");
        assert!(!local_book_rename_needs_stats_migration(
            Some(("旧书名", "本地导入")),
            &saved,
        ));
        assert!(!local_book_rename_needs_stats_migration(
            Some((" 旧书名 ", " 本地导入 ")),
            &saved,
        ));
    }

    /// 首次加入书架没有历史行可迁移, 不能借这条路径去动别的书的统计。
    #[test]
    fn first_save_to_shelf_skips_migration() {
        let saved = local_txt_book("新书名", "本地导入");
        assert!(!local_book_rename_needs_stats_migration(None, &saved));
    }

    /// 远程书的身份由 book_url 决定, 换源时统计靠书名分组已经能合并;
    /// 这里只处理本地书, 免得给远程书的每次编辑都加一次全表 UPDATE。
    #[test]
    fn remote_book_rename_skips_migration() {
        let saved = Book {
            name: "新书名".to_string(),
            author: "作者".to_string(),
            book_url: "https://source.example/book/1".to_string(),
            origin: "https://source.example/".to_string(),
            ..Default::default()
        };
        assert!(!local_book_rename_needs_stats_migration(
            Some(("旧书名", "作者")),
            &saved,
        ));
    }

    /// `aborted` 必须能独立表达"被取消"：即使章节已全部派发完（`pending` 为空），
    /// 只要有在途请求且此时已被取消，返回值也要是 true。
    #[tokio::test]
    async fn cache_scheduler_reports_aborted_when_cancelled_after_dispatch() {
        let urls: Vec<String> = (0..4).map(|i| format!("ch{i}")).collect();
        let cancel_now = Arc::new(AtomicBool::new(false));

        let (_success, _failed, aborted) = run_cache_tasks(
            urls,
            4, // 窗口足够大：首批就会把全部章节派发完
            {
                let cancel_now = cancel_now.clone();
                move || cancel_now.load(Ordering::SeqCst)
            },
            |_url| async move {
                tokio::time::sleep(std::time::Duration::from_millis(5)).await;
                Ok(())
            },
            {
                // 第一次进度回调时立即取消：此刻 pending 已空，只剩在途请求
                let cancel_now = cancel_now.clone();
                move |_success, _failed| cancel_now.store(true, Ordering::SeqCst)
            },
        )
        .await;

        assert!(aborted, "pending 已空但被取消时, 返回值也应表达 aborted");
    }


    ///
    /// 回归点：早期实现是「先把所有章节派发完、再统一收集结果」。那种写法也会
    /// emit 每章一次，但首次 emit 发生在**全部派发之后** —— 用户看到的是进度条
    /// 长时间不动、最后一步跳满。所以判据不是 emit 次数，而是「首次上报时还有
    /// 章节没被派发出去」。
    #[tokio::test]
    async fn cache_scheduler_reports_progress_while_still_dispatching() {
        let urls: Vec<String> = (0..10).map(|i| format!("https://example.test/ch/{i}")).collect();
        let reported = Arc::new(Mutex::new(Vec::new()));
        // 已被派发（进入 load）的章节数
        let dispatched = Arc::new(AtomicUsize::new(0));
        let first_emit_dispatched = Arc::new(Mutex::new(None));
        let inflight = Arc::new(AtomicUsize::new(0));
        let peak = Arc::new(AtomicUsize::new(0));

        let (success, failed, aborted) = run_cache_tasks(
            urls,
            3,
            || false,
            {
                let inflight = inflight.clone();
                let peak = peak.clone();
                let dispatched = dispatched.clone();
                move |_url| {
                    dispatched.fetch_add(1, Ordering::SeqCst);
                    let inflight = inflight.clone();
                    let peak = peak.clone();
                    async move {
                        let now = inflight.fetch_add(1, Ordering::SeqCst) + 1;
                        peak.fetch_max(now, Ordering::SeqCst);
                        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
                        inflight.fetch_sub(1, Ordering::SeqCst);
                        Ok(())
                    }
                }
            },
            {
                let reported = reported.clone();
                let dispatched = dispatched.clone();
                let first_emit_dispatched = first_emit_dispatched.clone();
                move |success, _failed| {
                    reported.lock().unwrap().push(success);
                    let mut first = first_emit_dispatched.lock().unwrap();
                    if first.is_none() {
                        *first = Some(dispatched.load(Ordering::SeqCst));
                    }
                }
            },
        )
        .await;

        assert_eq!(success, 10);
        assert_eq!(failed, 0);
        assert!(!aborted);

        let total_dispatched = dispatched.load(Ordering::SeqCst);
        let first_emit_dispatched = first_emit_dispatched.lock().unwrap();
        let first_emit_dispatched = first_emit_dispatched.expect("应至少上报过一次进度");
        assert!(
            first_emit_dispatched < total_dispatched,
            "首次进度上报时只派发了 {first_emit_dispatched}/{total_dispatched} 章, \
             说明进度是边派发边报的; 若两者相等则退回到了「先派发完再收集」"
        );

        let reported = reported.lock().unwrap();
        assert_eq!(
            reported.len(),
            10,
            "每完成一章都应上报一次进度, 实际: {reported:?}"
        );
        assert!(
            reported.windows(2).all(|w| w[1] > w[0]),
            "进度应逐章递增, 实际: {reported:?}"
        );
        assert_eq!(*reported.last().unwrap(), 10);
        drop(reported);
        assert!(peak.load(Ordering::SeqCst) <= 3, "并发不应超过窗口上限");
    }

    /// 判别力自检：把「先派发完全部、再统一收集」的旧写法内联复现一遍，
    /// 确认上面那条判据确实能识别出它（首次上报时全都已派发完 = 进度会卡住）。
    /// 没有这个测试，上面那条断言可能只是恰好通过，抓不住回归。
    #[tokio::test]
    async fn dispatch_first_implementation_would_freeze_progress() {
        let urls: Vec<String> = (0..10).map(|i| format!("ch{i}")).collect();
        let dispatched = Arc::new(AtomicUsize::new(0));

        // —— 旧写法：一次性把全部章节派发出去 ——
        let mut tasks: futures::stream::FuturesUnordered<_> = futures::stream::FuturesUnordered::new();
        for _url in urls {
            dispatched.fetch_add(1, Ordering::SeqCst);
            tasks.push(tokio::spawn(async {
                tokio::time::sleep(std::time::Duration::from_millis(2)).await;
                Ok::<(), AppError>(())
            }));
        }
        let total_dispatched = dispatched.load(Ordering::SeqCst);

        // 收集阶段才开始上报 —— 此时所有章节早已派发完毕
        let mut first_emit_dispatched = None;
        while let Some(_done) = tasks.next().await {
            if first_emit_dispatched.is_none() {
                first_emit_dispatched = Some(dispatched.load(Ordering::SeqCst));
            }
        }

        assert_eq!(total_dispatched, 10);
        assert_eq!(
            first_emit_dispatched,
            Some(total_dispatched),
            "旧写法下首次进度上报时所有章节都已派发 —— 这正是进度条长时间不动的原因"
        );
    }

    #[tokio::test]
    async fn cache_scheduler_counts_failures_and_keeps_going() {
        let urls: Vec<String> = (0..6).map(|i| format!("ch{i}")).collect();
        let seen_failed = Arc::new(Mutex::new(Vec::new()));

        let (success, failed, aborted) = run_cache_tasks(
            urls,
            2,
            || false,
            |url| async move {
                if url.ends_with('0') {
                    Err(AppError::BadRequest("正文解析为空, 未写入缓存".to_string()))
                } else {
                    Ok(())
                }
            },
            {
                let seen_failed = seen_failed.clone();
                move |_success, failed| seen_failed.lock().unwrap().push(failed)
            },
        )
        .await;

        assert_eq!(failed, 1, "失败章节应计入失败数");
        assert_eq!(success, 5);
        assert!(!aborted);
        let seen_failed = seen_failed.lock().unwrap();
        assert_eq!(*seen_failed.last().unwrap(), 1, "失败数应出现在进度里");
    }

    #[tokio::test]
    async fn cache_scheduler_stops_dispatching_after_cancel() {
        let urls: Vec<String> = (0..20).map(|i| format!("ch{i}")).collect();
        let cancelled = Arc::new(AtomicBool::new(false));

        let (success, _failed, aborted) = run_cache_tasks(
            urls,
            2,
            {
                let cancelled = cancelled.clone();
                move || cancelled.load(Ordering::SeqCst)
            },
            |_url| async move {
                tokio::time::sleep(std::time::Duration::from_millis(2)).await;
                Ok(())
            },
            {
                let cancelled = cancelled.clone();
                move |success, _failed| {
                    if success >= 2 {
                        cancelled.store(true, Ordering::SeqCst)
                    }
                }
            },
        )
        .await;

        assert!(aborted, "取消后应标记 aborted");
        assert!(success < 20, "取消后不应继续跑完剩余章节, 实际 {success}");
    }

    /// 搜索任务结束时必须自动注销自己, 否则反复搜索会不断堆积登记条目。
    #[test]
    fn search_cancellation_guard_unregisters_on_drop() {
        let task_id = "test-guard-unregister";
        {
            let _guard = SearchCancellationGuard::register(Some(task_id));
            let registered = SEARCH_CANCELLATIONS
                .lock()
                .map(|tasks| tasks.contains_key(task_id))
                .unwrap_or(false);
            assert!(registered, "登记后应能在取消表里查到");
        }
        let leaked = SEARCH_CANCELLATIONS
            .lock()
            .map(|tasks| tasks.contains_key(task_id))
            .unwrap_or(false);
        assert!(!leaked, "守卫 Drop 后应已注销");
    }

    /// 前端「刚发出搜索就退出」时, 取消会先于任务登记到达; 这种取消不能被丢掉。
    #[test]
    fn search_cancel_arriving_before_registration_is_honoured() {
        let task_id = "test-cancel-before-register";
        // 模拟取消先到: 表里还没有这个任务, 应落到 pending 里。
        assert!(
            SEARCH_CANCELLATIONS
                .lock()
                .map(|tasks| !tasks.contains_key(task_id))
                .unwrap_or(false)
        );
        SEARCH_CANCEL_REQUESTS
            .lock()
            .map(|mut pending| pending.insert(task_id.to_string(), std::time::Instant::now()))
            .ok();

        let guard = SearchCancellationGuard::register(Some(task_id));

        assert!(guard.is_cancelled(), "登记时应消费掉先到的取消请求");
        assert!(
            SEARCH_CANCEL_REQUESTS
                .lock()
                .map(|pending| !pending.contains_key(task_id))
                .unwrap_or(true),
            "pending 里的取消请求应已被消费, 不能被下一个任务复用"
        );
    }

    /// 「任务早已结束、取消晚到」会留下等不到登记的孤儿记录, 必须有 TTL 兜底,
    /// 否则反复「搜索后立刻退出」会让 pending 无界增长。
    #[test]
    fn stale_search_cancel_requests_are_pruned() {
        let mut pending: std::collections::HashMap<String, std::time::Instant> =
            std::collections::HashMap::new();
        pending.insert("orphan".to_string(), std::time::Instant::now() - SEARCH_CANCEL_REQUEST_TTL * 2);
        pending.insert("fresh".to_string(), std::time::Instant::now());

        super::prune_stale_search_cancel_requests(&mut pending);

        assert!(!pending.contains_key("orphan"), "超龄孤儿记录应被清掉");
        assert!(pending.contains_key("fresh"), "未超龄的记录不能误删");
    }

    /// 旧任务被同 id 新任务覆盖后 Drop, 不能把新任务正等待的取消记录删掉。
    #[test]
    fn drop_keeps_pending_owned_by_newer_task() {
        let task_id = "test-drop-keeps-newer-pending";
        {
            let _old = SearchCancellationGuard::register(Some(task_id));
            // 新任务覆盖同一 id。
            let _new = SearchCancellationGuard::register(Some(task_id));
            // 模拟新任务的取消记录就位。
            SEARCH_CANCEL_REQUESTS
                .lock()
                .map(|mut pending| pending.insert(task_id.to_string(), std::time::Instant::now()))
                .ok();

            drop(_old);

            assert!(
                SEARCH_CANCEL_REQUESTS
                    .lock()
                    .map(|pending| pending.contains_key(task_id))
                    .unwrap_or(false),
                "旧任务 Drop 不应删掉新任务正在等待的取消记录"
            );
            SEARCH_CANCEL_REQUESTS
                .lock()
                .map(|mut pending| pending.remove(task_id))
                .ok();
        }
        assert!(
            SEARCH_CANCELLATIONS
                .lock()
                .map(|tasks| !tasks.contains_key(task_id))
                .unwrap_or(true),
            "两个守卫都析构后应清空该 id 的登记"
        );
    }

    /// 没有 taskId 时不登记: 老调用方(不传 taskId)行为不变, 也不会污染取消表。
    ///
    /// 断言只针对这个守卫自身, 不检查全局表是否为空 —— 测试是并行跑的, 别的用例
    /// 可能正持有登记条目。
    #[test]
    fn search_cancellation_guard_ignores_blank_task_id() {
        for value in [None, Some(""), Some("   ")] {
            let guard = SearchCancellationGuard::register(value);
            assert!(!guard.is_cancelled());
            assert!(guard.registered.is_none(), "空白 taskId 不应登记");
        }
    }

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
    fn available_book_source_response_preserves_complete_source_results() {
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

        let response = build_available_book_source_response(books, 11, false, true);

        assert_eq!(response.books.len(), 25);
        assert_eq!(response.books[0].name, "Book 0");
        assert_eq!(response.books[24].name, "Book 24");
        assert_eq!(response.last_index, 11);
        assert!(!response.has_more);
        assert!(response.has_errors);
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
    fn homepage_cache_starts_from_saved_reading_chapter() {
        let chapter_urls = (0..10)
            .map(|index| format!("chapter-{index}"))
            .collect::<Vec<_>>();

        assert_eq!(resolve_cache_start_index(&chapter_urls, None, Some(6)), 6);
    }

    #[test]
    fn explicit_cache_start_takes_priority_over_saved_progress() {
        let chapter_urls = (0..10)
            .map(|index| format!("chapter-{index}"))
            .collect::<Vec<_>>();

        assert_eq!(
            resolve_cache_start_index(&chapter_urls, Some("chapter-3"), Some(6)),
            3,
        );
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
            author: Some("另一作者".to_string()),
            origin: Some("https://source.test".to_string()),
            refresh: None,
            last_index: None,
            result_limit: None,
            concurrent_count: None,
            task_id: None,
        };

        let book = fallback_available_book(&req).expect("fallback book");

        assert_eq!(book.book_url, "https://example.test/book/1");
        assert_eq!(book.name, "深空彼岸");
        assert_eq!(book.author, "另一作者");
        assert_eq!(book.origin, "https://source.test");
    }

    #[test]
    fn available_source_candidates_keep_each_aggregate_station() {
        let books: Vec<SearchBook> = (0..35).map(|index| SearchBook {
            name: "斗破苍穹".to_string(),
            author: if index % 2 == 0 { String::new() } else { "天蚕土豆_qd22".to_string() },
            origin: "聚合测试".to_string(),
            book_url: format!("data:;base64,station-{index}"),
            ..SearchBook::default()
        }).collect();
        let mut seen = HashSet::new();
        let matches = take_available_source_sse_matches(books.clone(), "斗破 苍穹", "天蚕土豆", &mut seen, usize::MAX);
        assert_eq!(matches.len(), 35);
        assert!(take_available_source_sse_matches(books, "斗破苍穹", "天蚕土豆", &mut seen, usize::MAX).is_empty());
        assert_eq!(effective_available_result_limit(None), usize::MAX);
    }

    #[test]
    fn current_candidate_filter_preserves_other_addresses_from_same_source() {
        let current = Book { origin: "https://aggregate.test/".to_string(), book_url: "data:;base64,current".to_string(), ..Book::default() };
        let mut candidate = SearchBook { origin: "https://aggregate.test".to_string(), book_url: current.book_url.clone(), ..SearchBook::default() };
        assert!(is_current_source_candidate(&candidate, &current));
        candidate.book_url = "data:;base64,other".to_string();
        assert!(!is_current_source_candidate(&candidate, &current));
    }

    #[test]
    fn available_book_source_cache_includes_current_source() {
        let cached = vec![SearchBook {
            name: "深空彼岸".to_string(),
            author: "作者：另一作者".to_string(),
            origin: "https://m.reader-example.com/".to_string(),
            book_url: "https://m.reader-example.com/book2986/".to_string(),
            ..SearchBook::default()
        }];

        let matches = take_available_source_cached_matches(cached, 5);

        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].origin, "https://m.reader-example.com/");
    }

    #[test]
    fn available_book_source_cache_returns_all_cached_matches() {
        let cached: Vec<SearchBook> = (0..25)
            .map(|index| SearchBook {
                origin: format!("source-{index}"),
                book_url: format!("book-{index}"),
                ..SearchBook::default()
            })
            .collect();

        let matches = take_available_source_cached_matches(cached, usize::MAX);

        assert_eq!(matches.len(), 25);
    }

    #[test]
    fn available_book_source_empty_cache_stays_empty() {
        let matches = take_available_source_cached_matches(Vec::new(), 5);

        assert!(matches.is_empty());
    }

    #[test]
    fn available_book_source_cache_removes_disabled_and_deleted_sources() {
        let cached = vec![
            SearchBook {
                origin: "enabled-source".to_string(),
                book_url: "book-1".to_string(),
                ..SearchBook::default()
            },
            SearchBook {
                origin: "deleted-source".to_string(),
                book_url: "book-2".to_string(),
                ..SearchBook::default()
            },
        ];
        let enabled = HashSet::from(["enabled-source".to_string()]);

        let matches = retain_enabled_source_books(cached, &enabled);

        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].origin, "enabled-source");
    }

    #[test]
    fn available_book_source_matches_source_urls_with_trailing_slashes() {
        let cached = vec![SearchBook {
            origin: "https://enabled.example/".to_string(),
            book_url: "book-1".to_string(),
            ..SearchBook::default()
        }];
        let enabled = HashSet::from(["https://enabled.example".to_string()]);

        let matches = retain_enabled_source_books(cached, &enabled);

        assert_eq!(matches.len(), 1);
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

    /// 空串/纯空白的 `tocUrl` 必须按"没有提供"处理。
    ///
    /// Legado 的 `Book.tocUrl` 是 Kotlin 非空 `String`，从手机端备份导入的书架里
    /// 就是 `"tocUrl": ""`。若把它当成有效 URL，目录请求既不会回落到 bookUrl 取详情，
    /// 又会拿空地址去抓页面 —— 用户看到的是打开书一直转圈。
    #[test]
    fn normalized_url_arg_treats_blank_url_as_missing() {
        assert_eq!(normalized_url_arg(None), None);
        assert_eq!(normalized_url_arg(Some("")), None);
        assert_eq!(normalized_url_arg(Some("   \t\n")), None);
        assert_eq!(
            normalized_url_arg(Some("  https://example.test/toc  ")),
            Some("https://example.test/toc".to_string())
        );
    }

    /// 书源身份比较必须忽略尾斜杠，否则备份里常见的一处差异会让整本书打不开。
    #[test]
    fn source_identity_url_ignores_trailing_slash() {
        assert_eq!(
            source_identity_url("https://example.test/"),
            source_identity_url("https://example.test")
        );
        assert_ne!(
            source_identity_url("https://example.test/a"),
            source_identity_url("https://example.test/b")
        );
        // 非 http(s) 地址保持原样，不做斜杠裁剪（本地书标识等依赖原值）。
        assert_eq!(source_identity_url("local-txt:abc"), "local-txt:abc");
    }

    /// 换源时详情规则没给出目录地址，不能把空串写进书架。
    #[test]
    fn merge_book_never_writes_blank_toc_url() {
        let mut target = Book {
            name: "测试书".into(),
            author: "作者".into(),
            book_url: "https://example.test/book".into(),
            origin: "https://example.test".into(),
            toc_url: None,
            ..Book::default()
        };
        let info = Book {
            toc_url: Some("   ".into()),
            ..Book::default()
        };

        merge_book(&mut target, info);

        assert_eq!(target.toc_url, None, "空白目录地址必须归一成 None");
    }

    /// 详情给出真实目录地址时要补进书架（换源后再次打开才不用重复取详情）。
    #[test]
    fn merge_book_fills_missing_toc_url() {
        let mut target = Book {
            name: "测试书".into(),
            book_url: "https://example.test/book".into(),
            origin: "https://example.test".into(),
            toc_url: None,
            ..Book::default()
        };
        let info = Book {
            toc_url: Some("https://example.test/book/catalog".into()),
            ..Book::default()
        };

        merge_book(&mut target, info);

        assert_eq!(
            target.toc_url.as_deref(),
            Some("https://example.test/book/catalog")
        );
    }
}
