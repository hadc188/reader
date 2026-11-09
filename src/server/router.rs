//! Web 端路由表。
//!
//! 每个 handler 都是 `crate::api::commands::*_core` 的薄适配: 解析请求 →
//! 调用 core → 包成 `ApiResponse` JSON。**不在这里写任何业务分支** ——
//! 一旦出现第二份判断逻辑, 桌面端与浏览器就会在本地书、书源回退这些边界上
//! 行为分叉。
//!
//! 路由前缀 `/reader3` 与前端 `api/invoke.ts` 的 `COMMAND_BY_PATH` 完全一致,
//! 因此 Web 端的传输层可以把同样的路径直接 fetch 过来, 不需要另建一套命名。

use crate::api::commands::book as book_cmd;
use crate::api::commands::book_group as book_group_cmd;
use crate::api::commands::book_source as book_source_cmd;
use crate::api::commands::bookmark as bookmark_cmd;
use crate::api::commands::reading_stats as reading_stats_cmd;
use crate::api::commands::replace_rule as replace_rule_cmd;
use crate::api::commands::update as update_cmd;
use crate::api::AppState;
use crate::error::error::{ApiResponse, AppError};
use crate::server::assets::StaticSource;
use crate::server::WebOptions;
use crate::service::local_epub_book::MAX_EPUB_UPLOAD_BYTES;
use crate::service::local_pdf_book::MAX_PDF_UPLOAD_BYTES;
use crate::service::local_txt_book::MAX_TXT_UPLOAD_BYTES;
use axum::{
    body::Body,
    extract::{DefaultBodyLimit, Multipart, State},
    http::{StatusCode, Uri},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use serde_json::Value;

/// multipart 信封(边界、字段头、CRLF)的余量。
///
/// `DefaultBodyLimit` 卡的是**整个请求体**, 而 `MAX_*_UPLOAD_BYTES` 卡的是
/// 文件本身; 两者之间差着 boundary 与字段头那几十字节。不留余量的话, 恰好
/// 等于上限的文件会在 extractor 层被 413 拦下, 而业务代码里的"不能超过 50MB"
/// 提示永远看不到。1KB 足够覆盖这些固定开销。
const MULTIPART_OVERHEAD: usize = 1024;

/// `/reader3` 那批 JSON 接口的请求体上限。
///
/// axum 0.7 对 `Json` extractor 默认只放行 2MB（`DefaultBodyLimit::max(2MB)`）。
/// 单条书源常有几十 KB，两千条级别的书源集/规则集轻松超过 2MB —— 批量导入会
/// 在 extractor 层被 413 拒掉，且错误发生在业务代码之前，提示只有一句
/// "Failed to buffer the request body"。这里按"整份 JSON 集合"的量级放宽到
/// 64MB：实测最大的在线书源集 8.9MB / 836 条、单条最大 1.9MB（含大段 jsLib 的源），
/// 64MB 留足余量，同时仍是一条能挡住误传几百 MB 文件的边界。
///
/// 不设成无限：`/reader3` 没有鉴权，放开到局域网时任何同网段设备都能打；
/// 一条限值至少保证不会因为一个畸形请求把内存吃干。
const JSON_BODY_LIMIT_BYTES: usize = 64 * 1024 * 1024;

/// 给上传路由算 body 上限。抽成函数是为了能单测"这层真的存在" ——
/// 具体回归见 `tests::upload_routes_lift_the_two_megabyte_default`。
fn multipart_body_limit(max_file_bytes: usize) -> DefaultBodyLimit {
    DefaultBodyLimit::max(max_file_bytes.saturating_add(MULTIPART_OVERHEAD))
}

/// 构造三个上传路由, 各自套上**匹配该格式**的 body 上限。
///
/// axum 0.7 的 `Multipart` 默认把请求体限制在 2MB(`multipart.rs:58` 原文:
/// "by default, `Multipart` limits the request body size to 2MB")。而这个项目
/// 明明声明支持 50/80/100MB 的本地书 —— 默认值一开着, 超过 2MB 的书会在
/// extractor 层直接 413, handler 里的 `MAX_*_UPLOAD_BYTES` 判断形同虚设,
/// 前端只看得到 "请求失败(413)" 这种没头没尾的提示。
///
/// 上限按**路由**分开设, 而不是给整个 Router 一个 100MB 的层: 普通 JSON 接口
/// 不该因为"上传要大"而一起放宽, 否则一个畸形请求就能让进程吃掉上百 MB。
fn upload_router(state: AppState) -> Router<AppState> {
    Router::new()
        .route(
            "/reader3/uploadTxtBook",
            post(upload_txt_book).layer(multipart_body_limit(MAX_TXT_UPLOAD_BYTES)),
        )
        .route(
            "/reader3/uploadEpubBook",
            post(upload_epub_book).layer(multipart_body_limit(MAX_EPUB_UPLOAD_BYTES)),
        )
        .route(
            "/reader3/uploadPdfBook",
            post(upload_pdf_book).layer(multipart_body_limit(MAX_PDF_UPLOAD_BYTES)),
        )
        .with_state(state)
}

/// 组装完整的 axum 路由。
///
/// `options` 目前只影响监听地址(在 `WebServer::start` 里已用完), 路由本身
/// 不再依赖任何鉴权配置 —— 与手机端一致, 不做账号体系。
pub fn build(state: AppState, _options: WebOptions, static_source: Option<StaticSource>) -> Router {
    let api = Router::new()
        // ─── 书架与阅读 ───
        .route("/reader3/getBookshelf", get(get_bookshelf))
        .route(
            "/reader3/getShelfBookWithCacheInfo",
            get(get_shelf_book_with_cache_info),
        )
        .route("/reader3/getBookInfo", post(get_book_info))
        .route("/reader3/getChapterList", post(get_chapter_list))
        .route("/reader3/getBookContent", post(get_book_content))
        .route("/reader3/saveBook", post(save_book))
        .route("/reader3/saveBooks", post(save_books))
        .route("/reader3/saveBookProgress", post(save_book_progress))
        .route("/reader3/deleteBookCache", post(delete_book_cache))
        .route("/reader3/getCachedChapterUrls", post(get_cached_chapter_urls))
        .route(
            "/reader3/getChapterWordCounts",
            post(get_chapter_word_counts),
        )
        // ─── 用户配置 ───
        //
        // 这里**没有** getUserConfig/saveUserConfig: 前端从不调用它们, 阅读偏好
        // 存在 localStorage(`readerReadConfig*`), 服务端这份是历史残留。挂着一个
        // 没人用的路由比不挂更糟 —— 下一个人会以为"Web 端阅读偏好是同步的"。
        // 真要跨设备同步, 那是另一件事, 得先把前端改成读写服务端。
        // ─── 书源 ───
        .route("/reader3/getBookSources", get(get_book_sources))
        .route("/reader3/getBookSource", post(get_book_source))
        .route("/reader3/saveBookSource", post(save_book_source))
        .route("/reader3/saveBookSources", post(save_book_sources))
        .route("/reader3/deleteBookSource", post(delete_book_source))
        .route("/reader3/deleteBookSources", post(delete_book_sources))
        // ─── 分组 (书架按组显示需要) ───
        .route("/reader3/getBookGroups", get(get_book_groups))
        // ─── 阅读统计 (最近阅读 / 统计页) ───
        .route("/reader3/getReadingStatsSummary", get(get_reading_stats_summary))
        .route("/reader3/addReadingStats", post(add_reading_stats))
        // ─── 其它只读接口 ───
        .route("/reader3/getBookmarks", get(get_bookmarks))
        .route("/reader3/getReplaceRules", get(get_replace_rules))
        .route("/reader3/getVersionUpdate", get(get_version_update))
        // ─── 阅读页内的日常写操作 ───
        //
        // 书签与替换规则是**阅读过程中**会用到的东西: 划词菜单有「加入书签」
        // 「按本书替换/按书源替换」, 目录面板有书签管理。只给读接口不给写接口,
        // 浏览器里这些按钮点了就是 404。它们只动本地的两份 JSON, 与「换源」
        // 「分组管理」这类依赖书源网络/批量语义的操作不是一回事。
        .route("/reader3/saveBookmark", post(save_bookmark))
        .route("/reader3/saveBookmarks", post(save_bookmarks))
        .route("/reader3/deleteBookmark", post(delete_bookmark))
        .route("/reader3/deleteBookmarks", post(delete_bookmarks))
        .route("/reader3/saveReplaceRule", post(save_replace_rule))
        .route("/reader3/saveReplaceRules", post(save_replace_rules))
        .route("/reader3/deleteReplaceRule", post(delete_replace_rule))
        .route("/reader3/deleteReplaceRules", post(delete_replace_rules))
        // ─── 资源 ───
        .route("/cover", get(cover))
        .route("/pdf", get(pdf))
        .route("/epub", get(epub))
        .route("/files", get(files))
        .route("/api/health", get(health))
        // ─── 本地书上传 (WiFi 传书), 各自带匹配的 body 上限 ───
        .merge(upload_router(state.clone()))
        // JSON 批量导入放宽到 64MB。这里必须在 `with_state` 之前、且作为整个
        // `api` 路由的 layer：逐条 `.route(...)` 上挂要写 28 次，漏一个就又是
        // 2MB。上传路由自带更严的 multipart 上限，叠加后取较小者，不受影响。
        .layer(DefaultBodyLimit::max(JSON_BODY_LIMIT_BYTES))
        .with_state(state);

    // 不做鉴权, 与手机端 Legado 的 HTTP Server 一致:
    // 桌面应用常驻本机, 用户自己开端口自己用, 加登录页只是多一道门。
    // 想限制访问范围就把监听地址留在回环; 需要局域网时明确打开开关。
    match static_source {
        Some(source) => api.merge(crate::server::assets::router(source)),
        None => api,
    }
}

// ───────────────────────── 基础 ─────────────────────────

async fn health() -> impl IntoResponse {
    Json(serde_json::json!({ "ok": true, "app": "reader-web" }))
}

/// 把 core 的 `Result<ApiResponse<T>, AppError>` 映射成 HTTP 响应。
///
/// 前端传输层 (`api/transport/http.ts`) 依赖这个信封解包, 与 Tauri 侧的
/// `invoke` 解包逻辑共用同一段代码 —— 所以这里必须保持 `{isSuccess,errorMsg,data}`
/// 的形状, 不能改成 REST 风格的状态码约定。
fn envelope<T: serde::Serialize>(result: Result<ApiResponse<T>, AppError>) -> Response {
    match result {
        Ok(body) => Json(body).into_response(),
        Err(err) => {
            let status = match &err {
                AppError::NotFound(_) => StatusCode::NOT_FOUND,
                AppError::BadRequest(_) => StatusCode::BAD_REQUEST,
                _ => StatusCode::INTERNAL_SERVER_ERROR,
            };
            (status, Json(ApiResponse::<T>::err(err.to_string()))).into_response()
        }
    }
}

// ───────────────────────── 书架 ─────────────────────────

async fn get_bookshelf(State(state): State<AppState>) -> Response {
    envelope(book_cmd::get_bookshelf_core(&state).await)
}

/// 书架 + 缓存章数。
///
/// `?prefetch=false` 关闭「缺目录缓存就后台联网抓一遍」—— 阅读页只要那个
/// 数字，不需要为此向几十个站点发请求（见 core 的说明）。缺省保持 true，
/// 与桌面端 `get_shelf_book_with_cache_info` 的默认值一致。
async fn get_shelf_book_with_cache_info(State(state): State<AppState>, uri: Uri) -> Response {
    let allow_prefetch = uri
        .query()
        .and_then(|query| {
            serde_urlencoded::from_str::<Vec<(String, String)>>(query)
                .ok()
                .and_then(|pairs| {
                    pairs.into_iter().find_map(|(key, value)| {
                        (key == "prefetch").then(|| {
                            !matches!(value.trim().to_ascii_lowercase().as_str(), "false" | "0")
                        })
                    })
                })
        })
        .unwrap_or(true);
    envelope(book_cmd::get_shelf_book_with_cache_info_core(&state, allow_prefetch).await)
}

// ───────────────────────── 书籍 ─────────────────────────

async fn get_book_info(
    State(state): State<AppState>,
    Json(req): Json<book_cmd::BookInfoRequest>,
) -> Response {
    envelope(book_cmd::get_book_info_core(&state, req).await)
}

async fn get_chapter_list(
    State(state): State<AppState>,
    Json(req): Json<book_cmd::ChapterListRequest>,
) -> Response {
    envelope(book_cmd::get_chapter_list_core(&state, req).await)
}

async fn get_book_content(
    State(state): State<AppState>,
    Json(req): Json<book_cmd::BookContentRequest>,
) -> Response {
    // Web 端不推加载阶段: 一次性响应没有中间态可推送。传 None 时 core 的
    // 其余逻辑与 Tauri 完全一致。
    envelope(book_cmd::get_book_content_core(&state, req, None).await)
}

/// 书保存: 必须走 `save_book_core`。
///
/// 早先这里直接调 `book_service.save_book()`, 省掉了书源候选过滤、缺 `tocUrl`
/// 时回取详情、本地书改名后迁移统计 —— 于是同一本书在浏览器里保存和在桌面端
/// 保存会得到不同状态。请求体先落成 `Value` 再 `from_value`, 是为了让反序列化
/// 失败走 400 而不是 axum 默认的 422 纯文本。
async fn save_book(State(state): State<AppState>, Json(req): Json<Value>) -> Response {
    let book = match serde_json::from_value::<crate::model::book::Book>(req) {
        Ok(book) => book,
        Err(err) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(ApiResponse::<Value>::err(format!("书籍数据无效: {err}"))),
            )
                .into_response()
        }
    };
    envelope(book_cmd::save_book_core(&state, book).await)
}

async fn save_books(State(state): State<AppState>, Json(req): Json<Value>) -> Response {
    let books = match serde_json::from_value::<Vec<crate::model::book::Book>>(req) {
        Ok(books) => books,
        Err(err) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(ApiResponse::<Value>::err(format!("书籍数据无效: {err}"))),
            )
                .into_response()
        }
    };
    envelope(book_cmd::save_books_core(&state, books).await)
}

async fn save_book_progress(
    State(state): State<AppState>,
    Json(req): Json<book_cmd::SaveBookProgressRequest>,
) -> Response {
    envelope(book_cmd::save_book_progress_core(&state, req).await)
}

async fn delete_book_cache(
    State(state): State<AppState>,
    Json(req): Json<book_cmd::DeleteCacheRequest>,
) -> Response {
    envelope(book_cmd::delete_book_cache_core(&state, req).await)
}

async fn get_cached_chapter_urls(
    State(state): State<AppState>,
    Json(req): Json<book_cmd::CachedChapterUrlsRequest>,
) -> Response {
    envelope(book_cmd::get_cached_chapter_urls_core(&state, req).await)
}

async fn get_chapter_word_counts(
    State(state): State<AppState>,
    Json(req): Json<book_cmd::CachedChapterUrlsRequest>,
) -> Response {
    envelope(book_cmd::get_chapter_word_counts_core(&state, req).await)
}

fn bad_request(message: &str) -> Response {
    (
        StatusCode::BAD_REQUEST,
        Json(ApiResponse::<Value>::err(message)),
    )
        .into_response()
}

// ───────────────────────── 书签 / 替换规则 ─────────────────────────

async fn get_bookmarks(State(state): State<AppState>) -> Response {
    envelope(bookmark_cmd::get_bookmarks_core(&state).await)
}

async fn save_bookmark(
    State(state): State<AppState>,
    Json(req): Json<crate::model::bookmark::Bookmark>,
) -> Response {
    envelope(bookmark_cmd::save_bookmark_core(&state, req).await)
}

async fn save_bookmarks(
    State(state): State<AppState>,
    Json(req): Json<Vec<crate::model::bookmark::Bookmark>>,
) -> Response {
    envelope(bookmark_cmd::save_bookmarks_core(&state, req).await)
}

async fn delete_bookmark(
    State(state): State<AppState>,
    Json(req): Json<crate::model::bookmark::Bookmark>,
) -> Response {
    envelope(bookmark_cmd::delete_bookmark_core(&state, req).await)
}

async fn delete_bookmarks(
    State(state): State<AppState>,
    Json(req): Json<Vec<crate::model::bookmark::Bookmark>>,
) -> Response {
    envelope(bookmark_cmd::delete_bookmarks_core(&state, req).await)
}

async fn get_replace_rules(State(state): State<AppState>) -> Response {
    envelope(replace_rule_cmd::get_replace_rules_core(&state).await)
}

async fn save_replace_rule(
    State(state): State<AppState>,
    Json(req): Json<crate::model::replace_rule::ReplaceRule>,
) -> Response {
    envelope(replace_rule_cmd::save_replace_rule_core(&state, req).await)
}

async fn save_replace_rules(
    State(state): State<AppState>,
    Json(req): Json<Vec<crate::model::replace_rule::ReplaceRule>>,
) -> Response {
    envelope(replace_rule_cmd::save_replace_rules_core(&state, req).await)
}

async fn delete_replace_rule(
    State(state): State<AppState>,
    Json(req): Json<crate::model::replace_rule::ReplaceRule>,
) -> Response {
    envelope(replace_rule_cmd::delete_replace_rule_core(&state, req).await)
}

async fn delete_replace_rules(
    State(state): State<AppState>,
    Json(req): Json<Vec<crate::model::replace_rule::ReplaceRule>>,
) -> Response {
    envelope(replace_rule_cmd::delete_replace_rules_core(&state, req).await)
}

// ───────────────────────── 书源 ─────────────────────────

async fn get_book_sources(State(state): State<AppState>) -> Response {
    envelope(book_source_cmd::get_book_sources_core(&state).await)
}

async fn get_book_source(
    State(state): State<AppState>,
    Json(req): Json<book_source_cmd::BookSourceUrlParam>,
) -> Response {
    envelope(book_source_cmd::get_book_source_core(&state, req).await)
}

async fn save_book_source(State(state): State<AppState>, Json(req): Json<Value>) -> Response {
    envelope(book_source_cmd::save_book_source_core(&state, req).await)
}

async fn save_book_sources(State(state): State<AppState>, Json(req): Json<Value>) -> Response {
    envelope(book_source_cmd::save_book_sources_core(&state, req).await)
}

async fn delete_book_source(
    State(state): State<AppState>,
    Json(req): Json<book_source_cmd::BookSourceUrlParam>,
) -> Response {
    envelope(book_source_cmd::delete_book_source_core(&state, req).await)
}

async fn delete_book_sources(
    State(state): State<AppState>,
    Json(req): Json<Vec<book_source_cmd::BookSourceUrlParam>>,
) -> Response {
    envelope(book_source_cmd::delete_book_sources_core(&state, req).await)
}

// ───────────────────────── 本地书上传 ─────────────────────────

/// 从 multipart 请求里取出第一个文件字段。
///
/// 桌面端走的是 Tauri 的 octet-stream 快速通道 + `x-reader-file-name` header;
/// 浏览器里没有这套约定, 用标准 multipart 最省事, 前端 `http.ts` 的
/// `upload()` 直接 `FormData` 即可。
async fn read_upload(
    multipart: &mut axum::extract::Multipart,
) -> Result<(String, Vec<u8>), Response> {
    while let Some(field) = multipart.next_field().await.map_err(|err| {
        (
            StatusCode::BAD_REQUEST,
            Json(ApiResponse::<Value>::err(format!("上传数据无效: {err}"))),
        )
            .into_response()
    })? {
        if field.name() != Some("file") {
            continue;
        }
        let file_name = field
            .file_name()
            .map(|name| name.to_string())
            .unwrap_or_else(|| "未命名".to_string());
        let bytes = field.bytes().await.map_err(|err| {
            (
                StatusCode::BAD_REQUEST,
                Json(ApiResponse::<Value>::err(format!("读取上传内容失败: {err}"))),
            )
                .into_response()
        })?;
        return Ok((file_name, bytes.to_vec()));
    }
    Err((
        StatusCode::BAD_REQUEST,
        Json(ApiResponse::<Value>::err("缺少 file 字段")),
    )
        .into_response())
}

/// 导入本地书的两步: 先由各格式 service 落盘解析, 再写进书架。
///
/// 桌面端 (Tauri command) 里这两步是内联写的; 这里抽成 helper, 三种格式
/// 共用, 避免复制粘贴时漏掉 `save_book`。
async fn save_imported_book(
    state: &AppState,
    imported: Result<crate::model::book::Book, AppError>,
) -> Result<ApiResponse<Value>, AppError> {
    let book = imported?;
    let saved = state.book_service.save_book("default", book).await?;
    Ok(ApiResponse::ok(
        serde_json::to_value(saved).unwrap_or_default(),
    ))
}

async fn upload_txt_book(State(state): State<AppState>, mut multipart: Multipart) -> Response {
    let (file_name, bytes) = match read_upload(&mut multipart).await {
        Ok(v) => v,
        Err(resp) => return resp,
    };
    if bytes.len() > MAX_TXT_UPLOAD_BYTES {
        return bad_request("TXT 文件不能超过 50MB");
    }
    let imported = state
        .local_txt_book_service
        .import_txt_book("default", &file_name, &bytes)
        .await;
    envelope(save_imported_book(&state, imported).await)
}

async fn upload_epub_book(State(state): State<AppState>, mut multipart: Multipart) -> Response {
    let (file_name, bytes) = match read_upload(&mut multipart).await {
        Ok(v) => v,
        Err(resp) => return resp,
    };
    if bytes.len() > MAX_EPUB_UPLOAD_BYTES {
        return bad_request("EPUB 文件不能超过 80MB");
    }
    let imported = state
        .local_epub_book_service
        .import_epub_book("default", &file_name, &bytes)
        .await;
    envelope(save_imported_book(&state, imported).await)
}

async fn upload_pdf_book(State(state): State<AppState>, mut multipart: Multipart) -> Response {
    let (file_name, bytes) = match read_upload(&mut multipart).await {
        Ok(v) => v,
        Err(resp) => return resp,
    };
    if bytes.len() > MAX_PDF_UPLOAD_BYTES {
        return bad_request("PDF 文件不能超过 100MB");
    }
    let imported = state
        .local_pdf_book_service
        .import_pdf_book("default", &file_name, &bytes)
        .await;
    envelope(save_imported_book(&state, imported).await)
}

// ───────────────────────── 只读辅助接口 ─────────────────────────

async fn get_book_groups(State(state): State<AppState>) -> Response {
    envelope(book_group_cmd::get_book_groups_core(&state).await)
}

async fn get_reading_stats_summary(State(state): State<AppState>) -> Response {
    envelope(reading_stats_cmd::get_reading_stats_summary_core(&state).await)
}

async fn add_reading_stats(
    State(state): State<AppState>,
    Json(req): Json<reading_stats_cmd::AddReadingStatsRequest>,
) -> Response {
    envelope(reading_stats_cmd::add_reading_stats_core(&state, req).await)
}

/// 版本检查在 Web 端只读展示, 不提供"立即更新" —— 浏览器里没有可替换的
/// 可执行文件。
///
/// `force` 用真正的 query 解析, 不用 `q.contains("force=true")`: 后者会把
/// `?notforce=true`、`?force=truex` 都当成命中, 也会漏掉 `force=True`。这里
/// 只认 `force` 这一个键, 值为 `true` / `1`(不区分大小写)。
async fn get_version_update(State(state): State<AppState>, uri: Uri) -> Response {
    let force = uri
        .query()
        .and_then(|query| {
            serde_urlencoded::from_str::<Vec<(String, String)>>(query)
                .ok()
                .and_then(|pairs| {
                    pairs.into_iter().find_map(|(key, value)| {
                        (key == "force").then(|| matches!(
                            value.trim().to_ascii_lowercase().as_str(),
                            "true" | "1"
                        ))
                    })
                })
        })
        .unwrap_or(false);
    envelope(
        update_cmd::get_version_update_core(
            &state,
            update_cmd::VersionUpdateQuery {
                force: Some(force),
            },
        )
        .await,
    )
}

// ───────────────────────── 资源 ─────────────────────────

/// 把 `tauri::http::Response<Vec<u8>>` 转成 axum 响应。
///
/// 已有的 `/cover` `/pdf` `/epub` `/files` 实现全部返回 Tauri 的 http 类型
/// (它本质就是 `http` crate 的别名), 因此只需要搬状态码与头, 不必重写取数逻辑。
fn from_tauri_response(resp: tauri::http::Response<Vec<u8>>) -> Response {
    let (parts, body) = resp.into_parts();
    let mut builder = Response::builder().status(parts.status);
    for (name, value) in parts.headers.iter() {
        builder = builder.header(name, value);
    }
    builder
        .body(Body::from(body))
        .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response())
}

fn query_string(uri: &Uri) -> String {
    uri.query().unwrap_or_default().to_string()
}

async fn cover(State(state): State<AppState>, uri: Uri) -> Response {
    from_tauri_response(crate::api::protocol::cover_route(&state, &query_string(&uri)).await)
}

async fn pdf(State(state): State<AppState>, uri: Uri) -> Response {
    from_tauri_response(crate::api::protocol::pdf_route(&state, &query_string(&uri)).await)
}

async fn epub(State(state): State<AppState>, uri: Uri) -> Response {
    from_tauri_response(crate::api::protocol::epub_route(&state, &query_string(&uri)).await)
}

async fn files(State(state): State<AppState>, uri: Uri) -> Response {
    from_tauri_response(crate::api::protocol::files_route(&state, &query_string(&uri)).await)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::Request;
    use tower::ServiceExt;

    /// 一份 3MB 的 multipart 请求体。
    ///
    /// 3MB 这个数字是关键: axum 的 `Multipart` 默认上限是 2MB, 而之前三个上传
    /// 路由都没有覆盖它, 于是"支持 50MB TXT"实际是"最多 2MB"。用刚过默认值、
    /// 又远小于真实上限的体积, 能精确区分"没有上限"和"有正确上限"。
    fn multipart_body(file_bytes: usize) -> (String, Vec<u8>) {
        const BOUNDARY: &str = "reader-test-boundary";
        let mut body = Vec::with_capacity(file_bytes + 256);
        body.extend_from_slice(
            format!(
                "--{BOUNDARY}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"book.txt\"\r\n\r\n"
            )
            .as_bytes(),
        );
        body.extend(std::iter::repeat(b'a').take(file_bytes));
        body.extend_from_slice(format!("\r\n--{BOUNDARY}--\r\n").as_bytes());
        (format!("multipart/form-data; boundary={BOUNDARY}"), body)
    }

    /// 只把 multipart 读完就返回, 不碰 AppState —— 这条测试关心的是
    /// **extractor 层的体积上限**, 与业务无关。
    ///
    /// 注意错误必须如实转成状态码: 第一次写这条测试时用了
    /// `next_field().await.unwrap_or(None)`, 于是超限错误被静默当成"没有更多字段",
    /// 请求返回 200 —— 测试因此"通过"了一个错误的前提。真实 handler 里
    /// (`read_upload`)也是把错误映射成响应的, 这里保持一致。
    async fn echo_size(mut multipart: Multipart) -> Response {
        let mut total = 0usize;
        loop {
            let field = match multipart.next_field().await {
                Ok(Some(field)) => field,
                Ok(None) => break,
                Err(err) => return (err.status(), err.body_text()).into_response(),
            };
            match field.bytes().await {
                Ok(bytes) => total += bytes.len(),
                Err(err) => return (err.status(), err.body_text()).into_response(),
            }
        }
        (StatusCode::OK, total.to_string()).into_response()
    }

    async fn post_body(app: Router, byte_len: usize) -> StatusCode {
        let (content_type, body) = multipart_body(byte_len);
        let request = Request::builder()
            .method("POST")
            .uri("/upload")
            .header("content-type", content_type)
            .body(Body::from(body))
            .unwrap();
        app.oneshot(request).await.unwrap().status()
    }

    /// 回归: 上传路由必须显式放宽 axum 的 2MB 默认上限。
    ///
    /// 修复前 `upload_txt_book` 之类的 handler 里写着 "TXT 文件不能超过 50MB",
    /// 但请求根本到不了那里 —— `Multipart` 的默认 `DefaultBodyLimit` 会在
    /// 2MB 处直接把请求判成 413, 前端只看到一句"请求失败(413)"。
    #[tokio::test]
    async fn upload_routes_lift_the_two_megabyte_default() {
        const THREE_MB: usize = 3 * 1024 * 1024;

        // 对照组: 不加上限时 3MB 会被拦下 —— 证明这个数字确实越过默认值,
        // 否则下面的断言会因为"默认值本来就够大"而失去意义。
        let default_app: Router = Router::new().route("/upload", post(echo_size));
        assert_eq!(
            post_body(default_app, THREE_MB).await,
            StatusCode::PAYLOAD_TOO_LARGE,
            "3MB 应当超过 axum Multipart 的 2MB 默认上限"
        );

        // 被测: 套上与真实路由相同的上限后应当放行。
        let lifted_app: Router = Router::new().route(
            "/upload",
            post(echo_size).layer(multipart_body_limit(MAX_TXT_UPLOAD_BYTES)),
        );
        assert_eq!(
            post_body(lifted_app, THREE_MB).await,
            StatusCode::OK,
            "放宽后 3MB 的 TXT 必须能进 handler"
        );
    }

    /// 上限仍然存在, 只是从 2MB 挪到了各格式自己的值: 超限的请求应该在
    /// extractor 层被 413 拒掉, 不该先整个读进内存再判断。
    #[tokio::test]
    async fn upload_routes_still_reject_oversized_bodies() {
        let app: Router = Router::new().route(
            "/upload",
            post(echo_size).layer(multipart_body_limit(1024)),
        );
        assert_eq!(
            post_body(app, 4 * 1024).await,
            StatusCode::PAYLOAD_TOO_LARGE
        );
    }

    #[test]
    fn body_limit_leaves_room_for_multipart_envelope() {
        // 恰好等于业务上限的文件不能被信封开销挤掉。
        assert!(MULTIPART_OVERHEAD >= 256);
    }

    /// 回归: `/reader3` 的 JSON 批量导入必须能超过 axum 的 2MB 默认上限。
    ///
    /// 修复前保存 300 条书源（约 2.4MB）会被 extractor 层直接 413，前端只看到
    /// "请求失败"，而业务代码里根本没有跟大小相关的分支 —— 用户会以为是书源
    /// 数据坏了。这里用 4MB 的 JSON 打到一个只挂 `DefaultBodyLimit` 的路由上，
    /// 断言它能进 handler。
    #[tokio::test]
    async fn json_routes_lift_the_two_megabyte_default() {
        const FOUR_MB: usize = 4 * 1024 * 1024;

        async fn echo_json_len(body: axum::body::Bytes) -> StatusCode {
            let _ = body.len();
            StatusCode::OK
        }

        let app: Router = Router::new()
            .route("/saveBookSources", post(echo_json_len))
            .layer(DefaultBodyLimit::max(JSON_BODY_LIMIT_BYTES));

        let request = Request::builder()
            .method("POST")
            .uri("/saveBookSources")
            .header("content-type", "application/json")
            .body(Body::from(format!(
                r#"{{"pad":"{}"}}"#,
                "x".repeat(FOUR_MB)
            )))
            .unwrap();
        assert_eq!(
            app.oneshot(request).await.unwrap().status(),
            StatusCode::OK,
            "4MB 的 JSON 批量导入不该再被 2MB 默认上限拦下"
        );
    }

    /// 放宽仍有边界: 超过 `JSON_BODY_LIMIT_BYTES` 的请求必须在 extractor 层被
    /// 拒掉, 不能先整个读进内存。这里把上限临时压到 1KB 验证同一层生效。
    #[tokio::test]
    async fn json_routes_still_reject_oversized_bodies() {
        async fn echo_json_len(body: axum::body::Bytes) -> StatusCode {
            let _ = body.len();
            StatusCode::OK
        }

        let app: Router = Router::new()
            .route("/saveBookSources", post(echo_json_len))
            .layer(DefaultBodyLimit::max(1024));

        let request = Request::builder()
            .method("POST")
            .uri("/saveBookSources")
            .header("content-type", "application/json")
            .body(Body::from(format!(r#"{{"pad":"{}"}}"#, "x".repeat(4096))))
            .unwrap();
        assert_eq!(
            app.oneshot(request).await.unwrap().status(),
            StatusCode::PAYLOAD_TOO_LARGE
        );
    }
}
