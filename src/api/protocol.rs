//! Custom `reader` URI-scheme handler.
//!
//! Serves cover images, EPUB assets, uploaded files and the bookSourceProxy
//! login iframe over the reader scheme so `<img>`/`<iframe>` in the frontend
//! keep loading synchronously without async blob-URL plumbing.
//!
//! The scheme origin is platform-dependent: WebView2 (Windows) serves custom
//! schemes as `http://reader.localhost`, WebKitGTK (Linux) and WKWebView
//! (macOS) as `reader://localhost`. See [`reader_scheme_origin`].

use crate::api::AppState;
use crate::error::error::{ApiResponse, AppError};
use crate::model::book_source::BookSource;
use crate::util::text::{normalize_source_url, repair_encoded_url};
use serde::Deserialize;
use regex::{Captures, Regex};
use std::collections::HashMap;
use tauri::http;
use tauri::Manager;
use url::Url;

/// Origin of the custom `reader` URI scheme, which differs per webview
/// backend: WebView2 (Windows/Android) serves custom schemes as
/// `http://<scheme>.localhost`, WebKitGTK/WKWebView (Linux/macOS) as
/// `<scheme>://localhost`. Mirrors the mapping in Tauri's injected
/// `convertFileSrc` used by the frontend (`frontend/src/api/scheme.ts`).
pub fn reader_scheme_origin() -> &'static str {
    if cfg!(any(target_os = "windows", target_os = "android")) {
        "http://reader.localhost"
    } else {
        "reader://localhost"
    }
}

/// Entry point registered via `register_asynchronous_uri_scheme_protocol("reader", ...)`.
pub fn handle_reader_scheme(
    ctx: tauri::UriSchemeContext<'_, tauri::Wry>,
    request: tauri::http::Request<Vec<u8>>,
    responder: tauri::UriSchemeResponder,
) {
    let path = request.uri().path().to_string();
    let query = request.uri().query().unwrap_or_default().to_string();
    let app_handle = ctx.app_handle().clone();

    tauri::async_runtime::spawn(async move {
        let state = app_handle.state::<AppState>();
        let response = route(&state, &path, &query, request).await;
        responder.respond(response);
    });
}

async fn route(
    state: &AppState,
    path: &str,
    query: &str,
    request: tauri::http::Request<Vec<u8>>,
) -> tauri::http::Response<Vec<u8>> {
    match path {
        "/bookSourceProxy" => book_source_proxy_route(state, query, &request).await,
        "/sourcePanelBridge" => source_panel_bridge_route(state, query, &request).await,
        "/sourcePanel" => source_panel_route(state, query, &request).await,
        "/cover" => cover_route(state, query).await,
        "/epub" => epub_route(state, query).await,
        "/files" => files_route(state, query).await,
        _ => http::Response::builder()
            .status(404)
            .body(Vec::new())
            .unwrap_or_default(),
    }
}

// ─────────────────────────── 书源面板 (showBrowser) ───────────────────────────

/// `${readerOrigin}/sourcePanel?panelId=...`: 面板 HTML(已注入宿主桥)。
///
/// 面板由后端托管而不是用 iframe `srcdoc`, 因为 `srcdoc` 是 `about:srcdoc` 源,
/// 无法同源访问 `/sourcePanelBridge`; 挂在 `reader.localhost` 下才能同步回调宿主。
async fn source_panel_route(
    _state: &AppState,
    query: &str,
    _request: &tauri::http::Request<Vec<u8>>,
) -> tauri::http::Response<Vec<u8>> {
    let params = parse_query(query);
    let Some(panel_id) = params.get("panelId") else {
        return text_response(400, "missing panelId");
    };
    let Some(html) = build_source_panel_page(panel_id) else {
        return text_response(404, "panel not found");
    };
    http::Response::builder()
        .status(200)
        .header(http::header::CONTENT_TYPE, "text/html; charset=utf-8")
        .header(http::header::CACHE_CONTROL, "no-store")
        .body(html.into_bytes())
        .unwrap_or_else(|_| http::Response::new(Vec::new()))
}

/// 组装面板页: 面板 HTML + 注入的宿主桥脚本。面板不存在时返回 `None`。
pub fn build_source_panel_page(panel_id: &str) -> Option<String> {
    let entry = crate::service::book_service::get_source_panel(panel_id)?;
    let bridge = build_source_panel_bridge_script(panel_id);
    Some(inject_before_head_end(&entry.html, &bridge))
}

/// 面板内桥的同步端点。请求体: `{ "action": "...", "payload": {...} }`
/// 响应体: `{ "ok": true, "value": ... }` 或 `{ "ok": false, "error": "..." }`
async fn source_panel_bridge_route(
    state: &AppState,
    query: &str,
    request: &tauri::http::Request<Vec<u8>>,
) -> tauri::http::Response<Vec<u8>> {
    // 信任边界: 面板 HTML 来自用户导入的书源, 而桥能触达宿主的网络与脚本能力。
    // 只有本应用自己的 origin 能调用, 外部网页(带具体 Origin)一律拒绝。
    let request_origin = request
        .headers()
        .get(http::header::ORIGIN)
        .and_then(|value| value.to_str().ok());
    if !panel_bridge_origin_allowed(request_origin) {
        return json_response(403, serde_json::json!({ "ok": false, "error": "forbidden origin" }));
    }
    // 无 Origin 头是同源请求(面板与桥同为 `reader` 协议 origin), 不需要 CORS 头;
    // 带 Origin 且已通过上面的校验时回显它, 绝不使用通配符。
    let allow_origin = request_origin.map(|_| reader_scheme_origin());

    let params = parse_query(query);
    let Some(panel_id) = params.get("panelId") else {
        return json_response_panel(
            400,
            serde_json::json!({ "ok": false, "error": "missing panelId" }),
            allow_origin,
        );
    };
    let Some(entry) = crate::service::book_service::get_source_panel(panel_id) else {
        return json_response_panel(
            404,
            serde_json::json!({ "ok": false, "error": "panel not found" }),
            allow_origin,
        );
    };
    let body: serde_json::Value = serde_json::from_slice(request.body()).unwrap_or_default();
    let action = body.get("action").and_then(|v| v.as_str()).unwrap_or("");
    let payload = body.get("payload").cloned().unwrap_or_default();

    let outcome = crate::service::book_service::handle_source_panel_bridge(
        &state.book_source_service,
        &entry,
        action,
        payload,
    )
    .await;
    match outcome {
        Ok(value) => json_response_panel(
            200,
            serde_json::json!({ "ok": true, "value": value }),
            allow_origin,
        ),
        Err(error) => json_response_panel(
            200,
            serde_json::json!({ "ok": false, "error": error }),
            allow_origin,
        ),
    }
}

/// 面板桥的调用者是否可信。
///
/// 放行两种情况: 不带 `Origin`(同源同步 XHR / 原生调用), 或 Origin 等于
/// `reader` 协议 origin(面板自身)。其余(如外部 https 页面)一律拒绝 ——
/// 否则任何本机网页都能借桥驱动宿主去请求书源接口或执行脚本。
fn panel_bridge_origin_allowed(origin: Option<&str>) -> bool {
    match origin {
        None => true,
        Some(value) => value.trim() == reader_scheme_origin(),
    }
}

fn parse_query(query: &str) -> HashMap<String, String> {
    url::form_urlencoded::parse(query.as_bytes())
        .map(|(k, v)| (k.into_owned(), v.into_owned()))
        .collect()
}

fn inject_before_head_end(html: &str, snippet: &str) -> String {
    if let Some(index) = html.find("</head>") {
        let mut out = String::with_capacity(html.len() + snippet.len());
        out.push_str(&html[..index]);
        out.push_str(snippet);
        out.push_str(&html[index..]);
        out
    } else if let Some(index) = html.find("</body>") {
        let mut out = String::with_capacity(html.len() + snippet.len());
        out.push_str(&html[..index]);
        out.push_str(snippet);
        out.push_str(&html[index..]);
        out
    } else {
        format!("{snippet}{html}")
    }
}

fn text_response(status: u16, message: &str) -> tauri::http::Response<Vec<u8>> {
    http::Response::builder()
        .status(status)
        .header(http::header::CONTENT_TYPE, "text/plain; charset=utf-8")
        .body(message.as_bytes().to_vec())
        .unwrap_or_else(|_| http::Response::new(Vec::new()))
}

fn json_response(status: u16, value: serde_json::Value) -> tauri::http::Response<Vec<u8>> {
    http::Response::builder()
        .status(status)
        .header(http::header::CONTENT_TYPE, "application/json; charset=utf-8")
        .header(http::header::CACHE_CONTROL, "no-store")
        .body(value.to_string().into_bytes())
        .unwrap_or_else(|_| http::Response::new(Vec::new()))
}

/// 面板桥专用响应。
///
/// 面板与桥同为 `reader` 协议 origin(因此请求通常不带 `Origin`)。只有调用方
/// 确实带了 `Origin` 时才回显**该已通过校验的 origin**, 不再使用通配符 ——
/// 通配符会让任意本机页面都能读走桥的响应。
fn json_response_panel(
    status: u16,
    value: serde_json::Value,
    allow_origin: Option<&str>,
) -> tauri::http::Response<Vec<u8>> {
    let mut builder = http::Response::builder()
        .status(status)
        .header(http::header::CONTENT_TYPE, "application/json; charset=utf-8")
        .header(http::header::CACHE_CONTROL, "no-store")
        .header(http::header::VARY, "Origin");
    if let Some(origin) = allow_origin {
        builder = builder.header(http::header::ACCESS_CONTROL_ALLOW_ORIGIN, origin);
    }
    builder
        .body(value.to_string().into_bytes())
        .unwrap_or_else(|_| http::Response::new(Vec::new()))
}

// ─────────────────────────── cover / epub / files ───────────────────────────
/// In-process LRU so re-rendered covers do not refetch. WebView2 does not
/// cache custom-scheme responses.
const COVER_CACHE_CAP: usize = 200;

fn cover_cache() -> &'static std::sync::Mutex<std::collections::HashMap<String, (Vec<u8>, String)>> {
    static CACHE: std::sync::OnceLock<
        std::sync::Mutex<std::collections::HashMap<String, (Vec<u8>, String)>>,
    > = std::sync::OnceLock::new();
    CACHE.get_or_init(|| std::sync::Mutex::new(std::collections::HashMap::new()))
}

async fn cover_route(state: &AppState, query: &str) -> tauri::http::Response<Vec<u8>> {
    let params: HashMap<String, String> =
        serde_urlencoded::from_str(query).unwrap_or_default();
    let url = params.get("path").map(String::as_str).unwrap_or_default();
    if url.is_empty() {
        return http::Response::builder()
            .status(404)
            .body(Vec::new())
            .unwrap_or_default();
    }

    let cache_key = url.to_string();
    if let Some((bytes, content_type)) = cover_cache()
        .lock()
        .ok()
        .and_then(|cache| cache.get(&cache_key).cloned())
    {
        return bytes_response(bytes, &content_type, Some("86400"));
    }

    // 本地 PDF 封面: /local-pdf/<hash>/cover.jpg → storage 下的书目录
    if let Some(rest) = url.strip_prefix("/local-pdf/") {
        return local_pdf_cover_route(state, rest).await;
    }

    // "public" namespace matches the original unauthenticated cover endpoint.
    match state.book_service.get_cover("public", url).await {
        Ok((bytes, content_type)) => {
            if let Ok(mut cache) = cover_cache().lock() {
                if cache.len() >= COVER_CACHE_CAP {
                    cache.clear();
                }
                cache.insert(cache_key, (bytes.clone(), content_type.clone()));
            }
            bytes_response(bytes, &content_type, Some("86400"))
        }
        Err(_) => http::Response::builder()
            .status(404)
            .body(Vec::new())
            .unwrap_or_default(),
    }
}

/// 本地 PDF 封面: rest = "<hash>/cover.<ext>"。hash 在 service 侧校验
/// (32 位 hex), 文件名只接受三个固定值, 无路径穿越面。
async fn local_pdf_cover_route(
    state: &AppState,
    rest: &str,
) -> tauri::http::Response<Vec<u8>> {
    let Some((hash, file_name)) = rest.split_once('/') else {
        return http::Response::builder()
            .status(404)
            .body(Vec::new())
            .unwrap_or_default();
    };
    if !matches!(file_name, "cover.jpg" | "cover.png" | "cover.jp2") {
        return http::Response::builder()
            .status(404)
            .body(Vec::new())
            .unwrap_or_default();
    }
    match state
        .local_pdf_book_service
        .read_cover_file("default", hash, file_name)
        .await
    {
        Ok((bytes, content_type)) => {
            bytes_response(bytes, &content_type, Some("private, max-age=86400"))
        }
        Err(_) => http::Response::builder()
            .status(404)
            .body(Vec::new())
            .unwrap_or_default(),
    }
}

async fn epub_route(state: &AppState, query: &str) -> tauri::http::Response<Vec<u8>> {
    let params: HashMap<String, String> =
        serde_urlencoded::from_str(query).unwrap_or_default();
    let book_url = params.get("bookUrl").map(String::as_str).unwrap_or_default();
    let path = params.get("path").map(String::as_str).unwrap_or_default();
    if book_url.is_empty() || path.is_empty() {
        return http::Response::builder()
            .status(404)
            .body(Vec::new())
            .unwrap_or_default();
    }
    match state
        .local_epub_book_service
        .get_asset("default", book_url, path)
        .await
    {
        Ok(asset) => bytes_response(asset.bytes, &asset.content_type, Some("private, max-age=3600")),
        Err(_) => http::Response::builder()
            .status(404)
            .body(Vec::new())
            .unwrap_or_default(),
    }
}

/// Serves user-uploaded files from `storage/assets/<ns>/<type>/<name>`.
/// The path query is the asset URL minus the leading slash, e.g.
/// `files?path=default/ai-maps/map.png`.
async fn files_route(state: &AppState, query: &str) -> tauri::http::Response<Vec<u8>> {
    let params: HashMap<String, String> =
        serde_urlencoded::from_str(query).unwrap_or_default();
    let Some(rel) = params.get("path") else {
        return http::Response::builder()
            .status(404)
            .body(Vec::new())
            .unwrap_or_default();
    };
    let rel = rel.trim_start_matches('/');
    if rel.is_empty() || rel.split('/').any(|seg| seg == ".." || seg.is_empty()) {
        return http::Response::builder()
            .status(400)
            .body(Vec::new())
            .unwrap_or_default();
    }
    let full = std::path::PathBuf::from(&state.config.storage_dir)
        .join("assets")
        .join(rel);
    match tokio::fs::read(&full).await {
        Ok(bytes) => {
            let content_type = mime_from_ext(&full).to_string();
            bytes_response(bytes, &content_type, None)
        }
        Err(_) => http::Response::builder()
            .status(404)
            .body(Vec::new())
            .unwrap_or_default(),
    }
}

fn bytes_response(
    bytes: Vec<u8>,
    content_type: &str,
    cache_control: Option<&str>,
) -> tauri::http::Response<Vec<u8>> {
    let mut builder = http::Response::builder().status(200);
    if let Ok(value) = http::HeaderValue::from_str(content_type) {
        builder = builder.header(http::header::CONTENT_TYPE, value);
    }
    if let Some(cc) = cache_control {
        if let Ok(value) = http::HeaderValue::from_str(cc) {
            builder = builder.header(http::header::CACHE_CONTROL, value);
        }
    }
    builder
        .header("access-control-allow-origin", "*")
        .header("cross-origin-resource-policy", "cross-origin")
        .body(bytes)
        .unwrap_or_else(|_| http::Response::new(Vec::new()))
}

fn mime_from_ext(path: &std::path::Path) -> &'static str {
    match path.extension().and_then(|e| e.to_str()).unwrap_or_default().to_ascii_lowercase().as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "svg" => "image/svg+xml",
        "ttf" => "font/ttf",
        "otf" => "font/otf",
        "woff" => "font/woff",
        "woff2" => "font/woff2",
        "avif" => "image/avif",
        "json" => "application/json",
        "txt" => "text/plain; charset=utf-8",
        _ => "application/octet-stream",
    }
}

// ─────────────────────────── bookSourceProxy ───────────────────────────

#[derive(Debug, Deserialize, Default)]
struct BookSourceProxyParam {
    #[serde(rename = "loginSession")]
    login_session: Option<String>,
    url: Option<String>,
}

async fn book_source_proxy_route(
    state: &AppState,
    query: &str,
    request: &tauri::http::Request<Vec<u8>>,
) -> tauri::http::Response<Vec<u8>> {
    let user_ns = "default";
    let q: BookSourceProxyParam = serde_urlencoded::from_str(query).unwrap_or_default();
    let Some(login_session) = q.login_session else {
        return error_response(403, "loginSession required");
    };
    let Some(raw_target_url) = q.url else {
        return error_response(400, "url required");
    };

    let Some(source_url) = state
        .book_service
        .source_for_login_session(&login_session)
        .await
    else {
        return error_response(403, "invalid or expired loginSession");
    };
    let source = match state
        .book_source_service
        .get(&user_ns, &source_url)
        .await
    {
        Ok(Some(source)) => source,
        _ => return error_response(404, "bookSource not found"),
    };

    let target_url = match resolve_proxy_target_url(&raw_target_url, &source.book_source_url) {
        Ok(url) => url,
        Err(err) => return error_response(400, &err.to_string()),
    };
    if !proxy_target_matches_source(&target_url, &source.book_source_url) {
        return error_response(403, "proxy target is outside the book source domain");
    }
    let upstream_referer = extract_upstream_referer(request.headers());
    match forward_book_source_request(
        state,
        &source,
        &login_session,
        request.method(),
        request.headers(),
        &target_url,
        upstream_referer.as_deref(),
        request.body(),
    )
    .await
    {
        Ok(response) => response,
        Err(err) => error_response(502, &err.to_string()),
    }
}

fn error_response(status: u16, message: &str) -> tauri::http::Response<Vec<u8>> {
    let body = serde_json::to_vec(&ApiResponse::<serde_json::Value>::err(message))
        .unwrap_or_default();
    http::Response::builder()
        .status(status)
        .header(http::header::CONTENT_TYPE, "application/json")
        .body(body)
        .unwrap_or_default()
}

fn resolve_proxy_target_url(
    raw_target_url: &str,
    book_source_url: &str,
) -> Result<String, AppError> {
    let repaired = repair_encoded_url(raw_target_url);
    if let Ok(url) = Url::parse(&repaired) {
        return Ok(url.to_string());
    }

    let base = normalize_source_url(book_source_url);
    let base = Url::parse(&base)
        .map_err(|e| AppError::BadRequest(format!("invalid bookSourceUrl: {}", e)))?;
    base.join(&repaired)
        .map(|u| u.to_string())
        .map_err(|e| AppError::BadRequest(format!("invalid proxy target url: {}", e)))
}

/// Login pages are rendered through the local URI scheme. Restrict every
/// proxied target to the source's host family so a valid login session cannot
/// be reused as a cross-domain request or SSRF primitive.
fn proxy_target_matches_source(target_url: &str, source_url: &str) -> bool {
    let Ok(target) = Url::parse(target_url) else {
        return false;
    };
    let Ok(source) = Url::parse(&normalize_source_url(source_url)) else {
        return false;
    };
    if !matches!(target.scheme(), "http" | "https")
        || !matches!(source.scheme(), "http" | "https")
    {
        return false;
    }
    let Some(target_host) = target.host_str() else {
        return false;
    };
    let Some(source_host) = source.host_str() else {
        return false;
    };
    let target_host = target_host.trim_end_matches('.').to_ascii_lowercase();
    let source_host = source_host.trim_end_matches('.').to_ascii_lowercase();
    match (source.port(), target.port()) {
        (Some(source_port), Some(target_port)) if source_port != target_port => return false,
        (Some(source_port), None) if target.port_or_known_default() != Some(source_port) => {
            return false
        }
        (None, Some(target_port)) if !matches!(target_port, 80 | 443) => return false,
        _ => {}
    }
    if target_host == source_host {
        return true;
    }

    // IP addresses and local development hosts have no meaningful parent
    // domain, so only the exact host is allowed for them.
    if target_host.parse::<std::net::IpAddr>().is_ok()
        || source_host.parse::<std::net::IpAddr>().is_ok()
        || source_host == "localhost"
        || source_host.ends_with(".localhost")
        || target_host == "localhost"
        || target_host.ends_with(".localhost")
    {
        return false;
    }

    let target_root = registrable_domain_from_host(&target_host);
    let source_root = registrable_domain_from_host(&source_host);
    !target_root.is_empty() && target_root == source_root
}

fn extract_upstream_referer(headers: &http::HeaderMap) -> Option<String> {
    let raw = headers.get(http::header::REFERER)?.to_str().ok()?;
    let referer = Url::parse(raw).ok()?;
    let params: HashMap<String, String> = referer.query_pairs().into_owned().collect();
    params.get("url").cloned().map(|v| repair_encoded_url(&v))
}

async fn forward_book_source_request(
    state: &AppState,
    source: &BookSource,
    login_session: &str,
    method: &http::Method,
    headers: &http::HeaderMap,
    target_url: &str,
    upstream_referer: Option<&str>,
    body: &[u8],
) -> Result<tauri::http::Response<Vec<u8>>, AppError> {
    let client = state
        .book_service
        .source_http_client("default", &source.book_source_url, None)
        .await?;
    let req_method = match *method {
        http::Method::GET => reqwest::Method::GET,
        http::Method::POST => reqwest::Method::POST,
        _ => return Err(AppError::BadRequest("unsupported proxy method".to_string())),
    };

    let mut builder = client.request(req_method, target_url);

    if let Some(header_str) = &source.header {
        if let Ok(source_headers) =
            serde_json::from_str::<HashMap<String, String>>(header_str)
        {
            for (k, v) in source_headers {
                builder = builder.header(k, v);
            }
        }
    }

    let mut has_content_type = false;
    let mut has_x_requested_with = false;
    for (name, value) in headers.iter() {
        if should_forward_request_header(name.as_str()) {
            if name.as_str().eq_ignore_ascii_case("content-type") {
                has_content_type = true;
            }
            if name.as_str().eq_ignore_ascii_case("x-requested-with") {
                has_x_requested_with = true;
            }
            builder = builder.header(name, value.clone());
        }
    }

    let referer_value = upstream_referer.unwrap_or(target_url);
    builder = builder.header(http::header::REFERER, referer_value);
    if let Ok(url) = Url::parse(referer_value) {
        let origin = format!("{}://{}", url.scheme(), url.host_str().unwrap_or_default());
        builder = builder.header(http::header::ORIGIN, origin);
    }

    if *method == http::Method::POST && !has_content_type {
        builder = builder.header(
            http::header::CONTENT_TYPE,
            "application/x-www-form-urlencoded; charset=UTF-8",
        );
    }
    if is_ajax_api_target(target_url) && !has_x_requested_with {
        builder = builder.header("X-Requested-With", "XMLHttpRequest");
    }

    if *method == http::Method::POST {
        builder = builder.body(body.to_vec());
    }

    tracing::info!(
        "bookSourceProxy upstream request: method={} target={} referer={} body_len={}",
        method,
        target_url,
        referer_value,
        body.len()
    );
    let upstream = builder.send().await.map_err(AppError::Http)?;
    let status = upstream.status();
    let final_url = upstream.url().to_string();
    let content_type = upstream
        .headers()
        .get(http::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());
    let bytes = upstream.bytes().await.map_err(AppError::Http)?;
    tracing::info!(
        "bookSourceProxy upstream response: method={} target={} status={} final_url={}",
        method,
        target_url,
        status,
        final_url
    );
    if is_ajax_api_target(target_url) {
        let preview = String::from_utf8_lossy(&bytes[..bytes.len().min(500)]).replace('\n', "\\n");
        tracing::info!(
            "bookSourceProxy upstream api body: target={} status={} preview={}",
            target_url,
            status,
            preview
        );
    }
    if status.is_client_error() || status.is_server_error() {
        let preview = String::from_utf8_lossy(&bytes[..bytes.len().min(400)]).replace('\n', "\\n");
        tracing::warn!(
            "bookSourceProxy upstream error body: target={} status={} preview={}",
            target_url,
            status,
            preview
        );
    }

    let mut response_builder = http::Response::builder().status(status.as_u16());
    if let Some(ct) = content_type.as_deref() {
        if let Ok(value) = http::HeaderValue::from_str(ct) {
            response_builder = response_builder.header(http::header::CONTENT_TYPE, value);
        }
    }
    // Keep upstream cookies inside the source-scoped client. Never expose
    // them to the shared reader.localhost browser origin.
    response_builder = response_builder
        .header(http::header::CACHE_CONTROL, "no-store")
        // Clear cookies left on reader.localhost by older releases. Current
        // login sessions live only in the backend source-scoped client.
        .header("Clear-Site-Data", "\"cookies\"");

    let body = if is_html_response(content_type.as_deref(), &bytes) {
        // 登录 iframe 运行在 reader.localhost 下；只注入请求代理脚本。
        // Cookie 始终留在后端按书源隔离的客户端中，不写入浏览器 origin。
        let text = String::from_utf8_lossy(&bytes).to_string();
        // 代理页的文档地址位于 reader.localhost，浏览器不会再按上游站点
        // 解析相对的 script/link/img/form 地址；先把同站静态资源改写回
        // bookSourceProxy，避免登录页出现无样式、无脚本或提交地址失效。
        let text = rewrite_login_html(
            &text,
            &final_url,
            &source.book_source_url,
            login_session,
        );
        let harvest = build_cookie_harvest_script(&source.book_source_url, login_session);
        let injected = if text.contains("</head>") {
            text.replace("</head>", &format!("{harvest}</head>"))
        } else if text.contains("</body>") {
            text.replace("</body>", &format!("{harvest}</body>"))
        } else {
            format!("{harvest}{text}")
        };
        injected.into_bytes()
    } else {
        bytes.to_vec()
    };

    Ok(response_builder
        .body(body)
        .unwrap_or_else(|_| http::Response::new(Vec::new())))
}

fn rewrite_login_html(
    html: &str,
    upstream_url: &str,
    book_source_url: &str,
    login_session: &str,
) -> String {
    let tag_re = Regex::new(
        r#"(?is)<(script|link|img|iframe|frame|source|video|audio|form)\b[^>]*>"#,
    )
    .expect("valid login resource tag regex");
    let double_attr = Regex::new(r#"(?i)\b(src|href|action)\s*=\s*"([^"]*)""#)
        .expect("valid login double attribute regex");
    let single_attr = Regex::new(r#"(?i)\b(src|href|action)\s*=\s*'([^']*)'"#)
        .expect("valid login single attribute regex");

    tag_re
        .replace_all(html, |tag_caps: &Captures| {
            let tag = tag_caps.get(0).map(|m| m.as_str()).unwrap_or_default();
            let output = double_attr.replace_all(tag, |attr_caps: &Captures| {
                rewrite_login_html_attr(
                    attr_caps,
                    upstream_url,
                    book_source_url,
                    login_session,
                    '"',
                )
            });
            single_attr
                .replace_all(&output, |attr_caps: &Captures| {
                    rewrite_login_html_attr(
                        attr_caps,
                        upstream_url,
                        book_source_url,
                        login_session,
                        '\'',
                    )
                })
                .into_owned()
        })
        .into_owned()
}

fn rewrite_login_html_attr(
    captures: &Captures,
    upstream_url: &str,
    book_source_url: &str,
    login_session: &str,
    quote: char,
) -> String {
    let attribute = captures.get(1).map(|m| m.as_str()).unwrap_or("src");
    let value = captures.get(2).map(|m| m.as_str()).unwrap_or_default();
    let rewritten = build_login_proxy_url(value, upstream_url, book_source_url, login_session)
        .unwrap_or_else(|| value.to_string());
    format!("{}={}{}{}", attribute, quote, rewritten, quote)
}

fn build_login_proxy_url(
    raw_value: &str,
    upstream_url: &str,
    book_source_url: &str,
    login_session: &str,
) -> Option<String> {
    let value = raw_value.trim();
    if value.is_empty()
        || value.starts_with('#')
        || value.starts_with("javascript:")
        || value.starts_with("data:")
        || value.starts_with("mailto:")
        || value.starts_with("tel:")
        || value.starts_with("/bookSourceProxy?")
    {
        return None;
    }

    let absolute = Url::parse(value)
        .or_else(|_| Url::parse(upstream_url).and_then(|base| base.join(value)))
        .ok()?;
    if !proxy_target_matches_source(absolute.as_str(), book_source_url) {
        // CDN、验证码等第三方资源保持直连；代理只负责同一书源域名，
        // 避免把受限的第三方请求改写成必然返回 403 的代理请求。
        return None;
    }
    Some(format!(
        "/bookSourceProxy?loginSession={}&url={}",
        urlencoding::encode(login_session),
        urlencoding::encode(absolute.as_str())
    ))
}

fn should_forward_request_header(name: &str) -> bool {
    !matches!(
        name.to_ascii_lowercase().as_str(),
        "host"
            | "content-length"
            | "authorization"
            | "cookie"
            | "referer"
            | "origin"
            | "connection"
    )
}

fn is_ajax_api_target(target_url: &str) -> bool {
    if let Ok(url) = Url::parse(target_url) {
        let path = url.path().to_ascii_lowercase();
        return path.ends_with("_api") || path.contains("/api/");
    }
    false
}

fn is_html_response(content_type: Option<&str>, body: &[u8]) -> bool {
    if let Some(ct) = content_type {
        if ct.to_ascii_lowercase().contains("text/html") {
            return true;
        }
    }
    // 避免 String 分配: 在原始 bytes 上做大小写不敏感匹配。
    // HTML 前缀是 ASCII, 多字节 UTF-8 的首字节 >= 0x80 不会与 "<html" 混淆。
    let prefix = &body[..body.len().min(256)];
    prefix
        .windows(5)
        .any(|w| w.eq_ignore_ascii_case(b"<html"))
        || prefix
            .windows(14)
            .any(|w| w.eq_ignore_ascii_case(b"<!doctype html"))
}

/// 书源面板(`java.showBrowser`)注入的宿主桥。
///
/// 插件面板通过 `window.java` / `window.cache` / `window.run` 回调宿主
/// (legado `WebJsExtensions`)。面板里 `java.ajax` 是**同步**调用, 而 Tauri IPC
/// 是异步的, 因此这里把 bridge 请求发到同源的 `/sourcePanelBridge`, 用同步
/// `XMLHttpRequest` 取回结果 —— 与 legado 的同步 Java 桥语义一致。
fn build_source_panel_bridge_script(panel_id: &str) -> String {
    let panel_id_json = serde_json::to_string(panel_id).unwrap_or_else(|_| "\"\"".to_string());
    format!(
        r#"<script>
(function () {{
  var panelId = {panel_id_json};
  var bridgeBase = "/sourcePanelBridge?panelId=" + encodeURIComponent(panelId);

  // 同步桥: 与 legado 的 Java 同步接口语义一致(插件大量使用同步 ajax)。
  //
  // 面板 iframe 保留 `allow-same-origin`(插件用 localStorage 缓存书籍信息,
  // opaque origin 下会抛 SecurityError)。桥与面板同为 reader 协议 origin,
  // 因此这是同源请求; 服务端仍会校验 Origin, 只放行 reader origin 自身。
  function bridgeSync(action, payload) {{
    var xhr = new XMLHttpRequest();
    var body = JSON.stringify({{ action: action, payload: payload || {{}} }});
    xhr.open("POST", bridgeBase, false); // false = 同步
    xhr.setRequestHeader("Content-Type", "text/plain;charset=UTF-8");
    try {{
      xhr.send(body);
    }} catch (e) {{
      return {{ ok: false, error: String(e) }};
    }}
    if (xhr.status < 200 || xhr.status >= 300) {{
      return {{ ok: false, error: "bridge HTTP " + xhr.status }};
    }}
    try {{
      return JSON.parse(xhr.responseText);
    }} catch (e) {{
      return {{ ok: false, error: "bridge bad response" }};
    }}
  }}

  // 通知宿主(父窗口): 面板要关闭 / 要换成另一个面板。
  // 面板 origin 为 reader.localhost, 父应用为 tauri.localhost, 用 postMessage 跨源通信。
  function postToHost(message) {{
    try {{
      message.panelId = message.panelId || panelId;
      window.parent.postMessage(message, "*");
    }} catch (e) {{}}
  }}

  // 面板内 console 转发到宿主日志, 便于排查
  var rawLog = console.log.bind(console);
  console.log = function () {{
    var parts = Array.prototype.slice.call(arguments).map(function (v) {{
      return typeof v === "string" ? v : JSON.stringify(v);
    }});
    bridgeSync("log", {{ message: parts.join(" ") }});
    rawLog.apply(null, arguments);
  }};
  window.onerror = function (msg, src, line) {{
    bridgeSync("log", {{ message: "面板 JS 错误: " + msg + " @" + line }});
  }};

  // 后端把 headers 当 JSON 文本解析(见 parser/js.rs 的 run_panel_request),
  // 而插件习惯写 java.get(url, {{'Referer': ...}}) 传对象, 直接透传会被后端的
  // 字符串取值丢掉, 表现为自定义头静默失效。这里统一序列化成文本。
  function hdrJson(h) {{
    if (h === null || h === undefined) return "";
    if (typeof h === "string") return h;
    try {{ return JSON.stringify(h); }} catch (e) {{ return ""; }}
  }}

  window.java = {{
    ajax: function (spec) {{
      var r = bridgeSync("ajax", {{ spec: String(spec) }});
      return r.ok ? r.value : "";
    }},
    get: function (url, headers) {{
      var r = bridgeSync("http", {{ method: "GET", url: String(url), body: "", headers: hdrJson(headers) }});
      return r.ok ? r.value : "";
    }},
    post: function (url, body, headers) {{
      var r = bridgeSync("http", {{ method: "POST", url: String(url), body: String(body || ""), headers: hdrJson(headers) }});
      return r.ok ? r.value : "";
    }},
    connect: function (url, headers) {{
      var r = bridgeSync("connect", {{ spec: String(url), headers: hdrJson(headers) }});
      return r.ok ? r.value : "";
    }},
    log: function (m) {{ bridgeSync("log", {{ message: String(m) }}); return m; }},
    toast: function (m) {{ bridgeSync("toast", {{ message: String(m) }}); }},
    longToast: function (m) {{ bridgeSync("toast", {{ message: String(m) }}); }},
    getCache: function (k) {{
      var r = bridgeSync("cacheGet", {{ key: String(k) }});
      return r.ok ? (r.value === undefined ? null : r.value) : null;
    }},
    putCache: function (k, v) {{ bridgeSync("cachePut", {{ key: String(k), value: String(v) }}); return true; }},
    // 面板需要与书源规则相同的加密能力(书源用它给接口签名)。
    // 缺了这些方法插件会降级到 CryptoJS 回退, 那条路径依赖注入 <script>, 容易失败。
    createSymmetricCrypto: function (transformation, key, iv) {{
      var t = String(transformation || "");
      var k = String(key == null ? "" : key);
      var v = String(iv == null ? "" : iv);
      function run(op, data, format) {{
        var r = bridgeSync("crypto", {{
          op: op, transformation: t, key: k, iv: v,
          data: String(data == null ? "" : data), format: format
        }});
        return r.ok ? String(r.value) : "";
      }}
      return {{
        encrypt: function (d) {{ return run("enc", d, "base64"); }},
        encryptBase64: function (d) {{ return run("enc", d, "base64"); }},
        encryptHex: function (d) {{ return run("enc", d, "hex"); }},
        decrypt: function (d) {{ return run("dec", d, "auto"); }},
        decryptStr: function (d) {{ return run("dec", d, "auto"); }},
        setIv: function (next) {{ v = String(next == null ? "" : next); return this; }}
      }};
    }},
    aesEncodeToBase64String: function (d, k, t, iv) {{
      return window.java.createSymmetricCrypto(t, k, iv).encryptBase64(d);
    }},
    aesEncodeToString: function (d, k, t, iv) {{
      return window.java.createSymmetricCrypto(t, k, iv).encryptHex(d);
    }},
    aesDecodeToString: function (d, k, t, iv) {{
      return window.java.createSymmetricCrypto(t, k, iv).decryptStr(d);
    }},
    desEncodeToBase64String: function (d, k, t, iv) {{
      return window.java.createSymmetricCrypto(t, k, iv).encryptBase64(d);
    }},
    desEncodeToString: function (d, k, t, iv) {{
      return window.java.createSymmetricCrypto(t, k, iv).encryptHex(d);
    }},
    desDecodeToString: function (d, k, t, iv) {{
      return window.java.createSymmetricCrypto(t, k, iv).decryptStr(d);
    }},
    tripleDESEncodeBase64Str: function (d, k, m, p, iv) {{
      return window.java.createSymmetricCrypto("DESede/" + m + "/" + p, k, iv).encryptBase64(d);
    }},
    tripleDESDecodeStr: function (d, k, m, p, iv) {{
      return window.java.createSymmetricCrypto("DESede/" + m + "/" + p, k, iv).decryptStr(d);
    }},
    // 面板里会检测这些哈希/编码方法是否存在
    md5Encode: function (v) {{ var r = bridgeSync("digest", {{ kind: "md5", input: String(v) }}); return r.ok ? String(r.value) : ""; }},
    base64Encode: function (v) {{ var r = bridgeSync("digest", {{ kind: "base64", input: String(v) }}); return r.ok ? String(r.value) : ""; }},
    base64Decode: function (v) {{ var r = bridgeSync("digest", {{ kind: "unbase64", input: String(v) }}); return r.ok ? String(r.value) : ""; }},
    // 面板内再次弹层: 后端登记为新面板, 由桥通知宿主切换
    showBrowser: function (url, html) {{
      var r = bridgeSync("showBrowser", {{ url: String(url), html: html == null ? "" : String(html) }});
      if (r.ok && r.value) {{
        postToHost({{ type: "showBrowser", panelId: String(r.value), url: String(url) }});
      }}
      return r.ok;
    }}
  }};

  window.cache = {{
    get: function (k) {{ return window.java.getCache(k); }},
    put: function (k, v) {{ return window.java.putCache(k, v); }},
    getFromMemory: function (k) {{ return window.java.getCache(k); }},
    putMemory: function (k, v) {{ return window.java.putCache(k, v); }},
    // 插件用它缓存 crypto-js 等大体积库(避免每次同步拉 CDN)
    getFile: function (k) {{
      var r = bridgeSync("cacheGet", {{ key: "file:" + String(k) }});
      return r.ok ? (r.value === undefined || r.value === null ? "" : r.value) : "";
    }},
    putFile: function (k, v) {{
      bridgeSync("cachePut", {{ key: "file:" + String(k), value: String(v) }});
      return v;
    }},
    "delete": function (k) {{ bridgeSync("cachePut", {{ key: String(k), value: "" }}); }}
  }};

  // window.run(script): 在宿主书源 jsLib 作用域里执行脚本, 返回 Promise
  window.run = function (script) {{
    return new Promise(function (resolve, reject) {{
      var r = bridgeSync("run", {{ script: String(script) }});
      if (r.ok) resolve(r.value);
      else reject(new Error(r.error || "run failed"));
    }});
  }};

  // 插件用 window.close() 关闭面板。
  // iframe 内 window.close() 通常无效(浏览器只允许关脚本自开的窗口),
  // 因此先通知宿主关掉弹层, 再尝试原生 close。
  var hostClose = window.close.bind(window);
  window.close = function () {{
    postToHost({{ type: "close" }});
    try {{ hostClose(); }} catch (e) {{}}
  }};
}})();
</script>"#
    )
}

/// 登录页请求代理脚本。Cookie 不暴露给页面，避免不同书源共享
/// reader.localhost origin。
fn build_cookie_harvest_script(book_source_url: &str, login_session: &str) -> String {
    let login_session_json =
        serde_json::to_string(login_session).unwrap_or_else(|_| "\"\"".to_string());
    // 从书源 URL 提取二级域(如 example.com), 作为「同站请求」判断依据:
    // 登录 iframe 内指向该域(及其子域)的 fetch/XHR 请求一律走代理转发,
    // 以便登录响应里的 Set-Cookie 被代理重写落到本 iframe 文档域被采集。
    // 第三方域(CDN/统计/广告)的请求不走代理, 避免无谓转发与跨域破坏。
    let book_domain = extract_registrable_domain(book_source_url);
    let book_domain_json = serde_json::to_string(&book_domain).unwrap_or_else(|_| "\"\"".to_string());
    format!(
        r#"<script>
(function() {{
  var loginSession = {login_session_json};
  // 登录目标站点的二级域(如 "example.com")。留空时退化为「代理所有非第三方请求」。
  var siteDomain = {book_domain_json};
  var proxyPath = "bookSourceProxy";
  var skip = /^(#|javascript:|data:|mailto:|tel:)/i;
  function toProxy(url) {{
    if (!url || skip.test(url)) return url;
    var params = new URLSearchParams();
    params.set("loginSession", loginSession);
    params.set("url", url);
    return proxyPath + "?" + params.toString();
  }}
  // 判断一个 URL 是否属于登录站点(应走代理转发以捕获 Set-Cookie):
  // - 相对路径 → 同站, 走代理
  // - 绝对 URL host === siteDomain 或以 .siteDomain 结尾 → 同站, 走代理
  // - 绝对 URL 但 host 不同 → 第三方(CDN/统计/OAuth 弹窗等), 直连不走代理
  function shouldProxy(rawUrl) {{
    if (!rawUrl || skip.test(rawUrl)) return false;
    // 相对路径(/api/login、api/x、./x、../x)与无协议(//host/path)中的相对部分
    if (/^(?:\.?\/|(?![a-z]+:))/i.test(rawUrl)) return true;
    try {{
      var u = new URL(rawUrl, location.href);
      if (!siteDomain) return false; // 无法判定站点域时不拦第三方
      var h = u.hostname.toLowerCase();
      return h === siteDomain || h.endsWith("." + siteDomain);
    }} catch (_e) {{
      return false;
    }}
  }}
  // fetch / XHR hook: 同站请求一律走代理, 以便登录接口的 Set-Cookie
  // 被代理重写落到本 iframe 文档域。不再只匹配 passport|login 等关键字,
  // 这样小网站的 /api/auth、/user/doLogin、/token 等登录接口也能被捕获。
  var rawFetch = window.fetch ? window.fetch.bind(window) : null;
  if (rawFetch) {{
    window.fetch = function(input, init) {{
      try {{
        // input 是 Request 对象时, 必须保留其 method/headers/body,
        // 否则登录提交(POST + form body)会退化成 GET, 登录直接失败。
        if (input instanceof Request) {{
          if (shouldProxy(input.url)) {{
            // 用 Request 构造器复制原请求, 由浏览器正确保留 body 流;
            // 不能把 Request 对象本身赋给 init.body, 那不是合法的 BodyInit。
            var proxied = new Request(toProxy(input.url), input);
            var opts = Object.assign({{}}, init || {{}}, {{credentials: "include"}});
            return rawFetch(proxied, opts);
          }}
          return rawFetch(input, init);
        }}
        var url = String(input);
        if (shouldProxy(url)) {{
          var opts = init || {{}};
          return rawFetch(toProxy(url), Object.assign({{}}, opts, {{credentials: "include"}}));
        }}
        return rawFetch(input, init);
      }} catch (_e) {{ return rawFetch(input, init); }}
    }};
  }}
  var rawXhrOpen = XMLHttpRequest.prototype.open;
  XMLHttpRequest.prototype.open = function(method, url) {{
    try {{
      if (shouldProxy(String(url))) {{
        arguments[1] = toProxy(String(url));
        this.withCredentials = true;
      }}
    }} catch (_e) {{}}
    return rawXhrOpen.apply(this, arguments);
  }};
}})();
</script>
"#,
    )
}

/// 从书源 URL 提取二级域(如 https://www.example.com → example.com),
/// 供采集脚本判断「同站请求」。无法解析时返回空串, 此时 hook 只代理相对路径请求。
fn extract_registrable_domain(source_url: &str) -> String {
    let parsed = match Url::parse(source_url) {
        Ok(u) => u,
        Err(_) => return String::new(),
    };
    let host = match parsed.host_str() {
        Some(h) => h,
        None => return String::new(),
    };
    registrable_domain_from_host(host)
}

fn registrable_domain_from_host(host: &str) -> String {
    let host = host.trim_end_matches('.').to_ascii_lowercase();
    let parts: Vec<&str> = host.split('.').filter(|part| !part.is_empty()).collect();
    if parts.len() < 2 {
        return host;
    }
    // Cover the common multi-label public suffixes without adding a runtime
    // dependency on a downloaded public suffix list.
    const TWO_LABEL_SUFFIXES: &[&str] = &[
        "co.uk", "org.uk", "com.cn", "net.cn", "org.cn", "com.au", "net.au",
        "co.jp", "co.kr", "com.br", "com.hk", "com.tw", "co.in",
    ];
    let suffix = format!("{}.{}", parts[parts.len() - 2], parts[parts.len() - 1]);
    if parts.len() >= 3 && TWO_LABEL_SUFFIXES.contains(&suffix.as_str()) {
        return parts[parts.len() - 3..].join(".");
    }
    parts[parts.len() - 2..].join(".")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reader_scheme_origin_matches_platform() {
        let origin = reader_scheme_origin();
        if cfg!(any(target_os = "windows", target_os = "android")) {
            assert_eq!(origin, "http://reader.localhost");
        } else {
            assert_eq!(origin, "reader://localhost");
        }
    }

    #[test]
    fn panel_bridge_rejects_foreign_origins() {
        // 同源(浏览器同步 XHR 不带 Origin)放行
        assert!(panel_bridge_origin_allowed(None));
        // 面板自身 origin 放行
        assert!(panel_bridge_origin_allowed(Some(reader_scheme_origin())));
        // 外部网页一律拒绝: 否则任何本机页面都能借桥驱动宿主的网络/脚本能力
        assert!(!panel_bridge_origin_allowed(Some("https://evil.example")));
        assert!(!panel_bridge_origin_allowed(Some("http://tauri.localhost")));
        // 不能因为大小写/空白差异被绕过
        assert!(!panel_bridge_origin_allowed(Some("HTTP://EVIL.EXAMPLE")));
        assert!(panel_bridge_origin_allowed(Some(&format!(
            "  {}  ",
            reader_scheme_origin()
        ))));
    }

    #[test]
    fn panel_bridge_response_echoes_origin_without_wildcard() {
        let echoed = json_response_panel(
            200,
            serde_json::json!({ "ok": true }),
            Some(reader_scheme_origin()),
        );
        assert_eq!(
            echoed
                .headers()
                .get(http::header::ACCESS_CONTROL_ALLOW_ORIGIN)
                .and_then(|v| v.to_str().ok()),
            Some(reader_scheme_origin())
        );
        // 同源请求不回 CORS 头, 更不能出现通配符
        let same_origin = json_response_panel(200, serde_json::json!({ "ok": true }), None);
        assert!(same_origin
            .headers()
            .get(http::header::ACCESS_CONTROL_ALLOW_ORIGIN)
            .is_none());
    }

    #[test]
    fn extract_registrable_domain_strips_subdomains() {
        assert_eq!(
            extract_registrable_domain("https://www.example.com"),
            "example.com"
        );
        assert_eq!(
            extract_registrable_domain("https://passport.example.com/login"),
            "example.com"
        );
        assert_eq!(
            extract_registrable_domain("http://m.example.org/book/1"),
            "example.org"
        );
        // 单段 host(localhost 等)原样返回
        assert_eq!(extract_registrable_domain("http://localhost:8080"), "localhost");
        // 无法解析 → 空串
        assert_eq!(extract_registrable_domain("not a url"), "");
    }

    #[test]
    fn harvest_script_proxies_same_site_and_relative_urls_only() {
        // 采集脚本应代理同站请求(含相对路径), 放行第三方域:
        // 验证注入到脚本的 siteDomain 与判断逻辑产物符合预期。
        let script = build_cookie_harvest_script(
            "https://m.example.site/book/1",
            "session-a",
        );
        // siteDomain 应被注入为 example.site
        assert!(
            script.contains(r#"var siteDomain = "example.site";"#),
            "脚本应注入 siteDomain, got snippet: {}",
            &script[..script.len().min(400)]
        );
        // 不再使用旧的 passport|login 关键字匹配
        assert!(
            !script.contains("passport|login|qidian|yuewen"),
            "脚本不应再用旧的关键字正则"
        );
        // shouldProxy 逻辑应存在
        assert!(script.contains("shouldProxy"), "脚本应包含 shouldProxy 函数");
        assert!(
            script.contains("endsWith"),
            "脚本应按域名后缀判断同站"
        );
        assert!(
            !script.contains("document.cookie") && !script.contains("message=harvest"),
            "登录代理脚本不能读取或上报浏览器 Cookie"
        );
        assert!(script.contains(r#"var loginSession = "session-a";"#));
        assert!(!script.contains("bookSourceUrl"));
        assert!(
            script.contains("new Request(toProxy(input.url), input)"),
            "Request 输入必须通过 Request 构造器复制, 保留 method/headers/body"
        );
        assert!(
            !script.contains("merged.body = input"),
            "不能把 Request 对象直接作为 fetch 的 body"
        );
    }

    #[test]
    fn harvest_script_handles_unparseable_source_domain() {
        // 书源 URL 无法解析时 siteDomain 为空串, shouldProxy 退化为只代理相对路径
        let script = build_cookie_harvest_script("not a url", "session-b");
        assert!(
            script.contains(r#"var siteDomain = "";"#),
            "无法解析时 siteDomain 应为空串"
        );
    }

    #[test]
    fn login_html_rewrites_same_source_resources() {
        let html = r#"<html><head>
          <script src="/static/app.js"></script>
          <link href='https://example.com/static/app.css'>
          <img src="https://cdn.example.net/logo.png">
          <form action="/user/login"></form>
        </head></html>"#;
        let output = rewrite_login_html(
            html,
            "https://www.example.com/login",
            "https://www.example.com",
            "session-a",
        );

        assert!(output.contains(
            "/bookSourceProxy?loginSession=session-a&url=https%3A%2F%2Fwww.example.com%2Fstatic%2Fapp.js"
        ));
        assert!(output.contains(
            "/bookSourceProxy?loginSession=session-a&url=https%3A%2F%2Fexample.com%2Fstatic%2Fapp.css"
        ));
        assert!(output.contains(
            "/bookSourceProxy?loginSession=session-a&url=https%3A%2F%2Fwww.example.com%2Fuser%2Flogin"
        ));
        assert!(output.contains("https://cdn.example.net/logo.png"));
    }

    #[test]
    fn proxy_target_is_limited_to_source_domain_family() {
        assert!(proxy_target_matches_source(
            "https://passport.example.com/login",
            "https://www.example.com"
        ));
        assert!(proxy_target_matches_source(
            "https://www.example.co.uk/api",
            "https://m.example.co.uk"
        ));
        assert!(!proxy_target_matches_source(
            "https://evil.example.net/steal",
            "https://www.example.com"
        ));
        assert!(!proxy_target_matches_source(
            "https://127.0.0.1:8080/",
            "https://localhost:3000"
        ));
        assert!(!proxy_target_matches_source(
            "https://source.example:9000/",
            "https://source.example"
        ));
        assert!(!proxy_target_matches_source(
            "https://other.localhost/",
            "https://app.localhost"
        ));
    }

    // ─────────────────── 书源面板 (showBrowser) ───────────────────

    #[test]
    fn panel_bridge_script_exposes_legado_api_surface() {
        let script = build_source_panel_bridge_script("panel-1");

        // 插件面板依赖的宿主接口 (legado WebJsExtensions)
        for expected in [
            "window.java",
            "window.cache",
            "window.run",
            "ajax:",
            "showBrowser:",
            "getFromMemory:",
            "getFile:",
            "putFile:",
        ] {
            assert!(script.contains(expected), "桥缺少 {expected}");
        }
        // 同步 XHR(插件大量使用同步 ajax)与免预检 Content-Type
        assert!(script.contains("false); // false = 同步"));
        assert!(script.contains("text/plain"));
        // panelId 需转义进脚本
        assert!(script.contains("\"panel-1\""));
    }

    /// 插件用 `typeof java.createSymmetricCrypto !== 'undefined'` 探测原生加密能力;
    /// 探测失败会降级到「注入 CryptoJS」的回退路径(那条路径在桌面端会直接失败,
    /// 导致面板所有 API 调用无签名 → 书籍信息为空、段评加载不出来)。
    #[test]
    fn panel_bridge_script_declares_crypto_methods() {
        let script = build_source_panel_bridge_script("panel-1");
        for method in [
            "createSymmetricCrypto:",
            "aesEncodeToBase64String:",
            "aesEncodeToString:",
            "aesDecodeToString:",
            "desEncodeToBase64String:",
            "desEncodeToString:",
            "desDecodeToString:",
            "md5Encode:",
            "base64Encode:",
            "base64Decode:",
        ] {
            assert!(script.contains(method), "桥缺少加密方法 {method}");
        }
        // 需要能完成对外的 crypto 桥调用
        assert!(script.contains("bridgeSync(\"crypto\""));
    }

    #[test]
    fn panel_bridge_script_does_not_patch_xhr() {
        // 曾经 patch XMLHttpRequest.prototype 导致桥自身的 XHR 也被拦截(自递归)。
        let script = build_source_panel_bridge_script("panel-1");
        assert!(!script.contains("XMLHttpRequest.prototype"));
    }

    #[test]
    fn source_panel_page_injects_bridge_into_head() {
        let id = crate::service::book_service::register_source_panel(
            crate::service::book_service::SourcePanelEntry {
                url: "https://panel.example/panel".to_string(),
                html: "<html><head><title>面板</title></head><body>内容</body></html>".to_string(),
                book_source_url: "https://source.example".to_string(),
                book_url: "https://source.example/book/1".to_string(),
                chapter_url: "https://source.example/book/1/1".to_string(),
                book_json: "{}".to_string(),
                chapter_json: "{}".to_string(),
            },
        );

        let page = build_source_panel_page(&id).expect("面板页应可构建");
        // 原内容保留
        assert!(page.contains("<title>面板</title>"));
        assert!(page.contains("内容"));
        // 桥注入到 </head> 之前
        let bridge_at = page.find("window.java").expect("桥应被注入");
        let head_end = page.find("</head>").expect("应有 </head>");
        assert!(bridge_at < head_end, "桥应注入在 </head> 之前");
        // 页面里带上自己的 panelId
        assert!(page.contains(&id));

        // 未登记的面板返回 None
        assert!(build_source_panel_page("no-such-panel").is_none());
    }

    #[test]
    fn panel_registry_keeps_newest_and_evicts_oldest() {
        let make = |url: &str| crate::service::book_service::SourcePanelEntry {
            url: url.to_string(),
            html: "<html></html>".to_string(),
            book_source_url: "https://evict.example".to_string(),
            book_url: String::new(),
            chapter_url: String::new(),
            book_json: "{}".to_string(),
            chapter_json: "{}".to_string(),
        };

        // 登记超过上限, 最旧的应被淘汰, 最新的应仍在
        let mut ids = Vec::new();
        for index in 0..12 {
            ids.push(crate::service::book_service::register_source_panel(make(&format!(
                "https://evict.example/{index}"
            ))));
        }
        let newest = ids.last().unwrap();
        let oldest = ids.first().unwrap();
        assert!(
            crate::service::book_service::get_source_panel(newest).is_some(),
            "最新面板不应被淘汰"
        );
        assert!(
            crate::service::book_service::get_source_panel(oldest).is_none(),
            "最旧面板应被淘汰(按插入顺序, 而非 HashMap 随机序)"
        );
    }
}
