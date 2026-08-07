//! Custom `reader` URI-scheme handler.
//!
//! Serves cover images, EPUB assets, uploaded files and the bookSourceProxy
//! login iframe over `http://reader.localhost/...` so `<img>`/`<iframe>` in the
//! frontend keep loading synchronously without async blob-URL plumbing.
//!
//! On Windows the scheme origin is `http://reader.localhost`.

use crate::api::AppState;
use crate::error::error::{ApiResponse, AppError};
use crate::model::book_source::BookSource;
use crate::util::text::{normalize_source_url, repair_encoded_url};
use regex::{Captures, Regex};
use serde::Deserialize;
use std::collections::HashMap;
use tauri::http;
use tauri::Manager;
use url::Url;

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
        "/bookSourceClientLog" => book_source_client_log_route(query),
        "/cover" => cover_route(state, query).await,
        "/epub" => epub_route(state, query).await,
        "/files" => files_route(state, query).await,
        _ => http::Response::builder()
            .status(404)
            .body(Vec::new())
            .unwrap_or_default(),
    }
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
    builder.body(bytes).unwrap_or_else(|_| http::Response::new(Vec::new()))
}

fn mime_from_ext(path: &std::path::Path) -> &'static str {
    match path.extension().and_then(|e| e.to_str()).unwrap_or_default().to_ascii_lowercase().as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "svg" => "image/svg+xml",
        "avif" => "image/avif",
        "json" => "application/json",
        "txt" => "text/plain; charset=utf-8",
        _ => "application/octet-stream",
    }
}

// ─────────────────────────── bookSourceProxy ───────────────────────────

#[derive(Debug, Deserialize, Default)]
struct BookSourceProxyParam {
    #[serde(rename = "bookSourceUrl")]
    book_source_url: Option<String>,
    url: Option<String>,
}

async fn book_source_proxy_route(
    state: &AppState,
    query: &str,
    request: &tauri::http::Request<Vec<u8>>,
) -> tauri::http::Response<Vec<u8>> {
    let user_ns = "default";
    let q: BookSourceProxyParam = serde_urlencoded::from_str(query).unwrap_or_default();
    let Some(source_url) = q.book_source_url else {
        return error_response(400, "bookSourceUrl required");
    };
    let Some(raw_target_url) = q.url else {
        return error_response(400, "url required");
    };

    let source = match state
        .book_source_service
        .get(&user_ns, &source_url)
        .await
    {
        Ok(Some(source)) => source,
        _ => return error_response(404, "bookSource not found"),
    };

    if let Some(cookie) = request
        .headers()
        .get(http::header::COOKIE)
        .and_then(|v| v.to_str().ok())
    {
        state
            .book_service
            .set_source_cookie(&user_ns, &source.book_source_url, cookie)
            .await;
    }

    let target_url = match resolve_proxy_target_url(&raw_target_url, &source.book_source_url) {
        Ok(url) => url,
        Err(err) => return error_response(400, &err.to_string()),
    };
    let upstream_referer = extract_upstream_referer(request.headers());
    match forward_book_source_request(
        state,
        &source,
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

fn extract_upstream_referer(headers: &http::HeaderMap) -> Option<String> {
    let raw = headers.get(http::header::REFERER)?.to_str().ok()?;
    let referer = Url::parse(raw).ok()?;
    let params: HashMap<String, String> = referer.query_pairs().into_owned().collect();
    params.get("url").cloned().map(|v| repair_encoded_url(&v))
}

async fn forward_book_source_request(
    state: &AppState,
    source: &BookSource,
    method: &http::Method,
    headers: &http::HeaderMap,
    target_url: &str,
    upstream_referer: Option<&str>,
    body: &[u8],
) -> Result<tauri::http::Response<Vec<u8>>, AppError> {
    let client = state.book_service.http_client();
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

    if let Some(cookie) = headers.get(http::header::COOKIE) {
        builder = builder.header(http::header::COOKIE, cookie.clone());
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
    let upstream_set_cookies: Vec<String> = upstream
        .headers()
        .get_all(http::header::SET_COOKIE)
        .iter()
        .filter_map(|v| v.to_str().ok().map(|s| s.to_string()))
        .collect();
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
    for cookie in upstream_set_cookies {
        if let Some(rewritten) = rewrite_set_cookie_for_proxy(&cookie) {
            if let Ok(value) = http::HeaderValue::from_str(&rewritten) {
                response_builder = response_builder.header(http::header::SET_COOKIE, value);
            }
        }
    }
    response_builder = response_builder.header(http::header::CACHE_CONTROL, "no-store");

    let body = if is_html_response(content_type.as_deref(), &bytes) {
        let text = String::from_utf8_lossy(&bytes).to_string();
        rewrite_login_html(&text, &final_url, &source.book_source_url).into_bytes()
    } else {
        bytes.to_vec()
    };

    Ok(response_builder
        .body(body)
        .unwrap_or_else(|_| http::Response::new(Vec::new())))
}

fn should_forward_request_header(name: &str) -> bool {
    !matches!(
        name.to_ascii_lowercase().as_str(),
        "host" | "content-length" | "authorization" | "referer" | "origin" | "connection"
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
    let prefix = String::from_utf8_lossy(&body[..body.len().min(256)]).to_ascii_lowercase();
    prefix.contains("<html") || prefix.contains("<!doctype html")
}

fn rewrite_login_html(html: &str, upstream_url: &str, book_source_url: &str) -> String {
    let base_href = html_escape_attr(upstream_url);
    let proxy_script = build_proxy_script(upstream_url, book_source_url);
    let mut output = if html.contains("<head") {
        html.replace(
            "</head>",
            &format!(r#"<base href="{base_href}">{proxy_script}</head>"#),
        )
    } else {
        format!(
            r#"<!DOCTYPE html><html><head><base href="{base_href}">{proxy_script}</head><body>{html}</body></html>"#
        )
    };

    output = rewrite_proxy_actions(&output, upstream_url, book_source_url);
    output = rewrite_script_root_relative_urls(&output, upstream_url, book_source_url);
    output
}

fn build_proxy_script(upstream_url: &str, book_source_url: &str) -> String {
    let upstream_json = serde_json::to_string(upstream_url).unwrap_or_else(|_| "\"\"".to_string());
    let source_json = serde_json::to_string(book_source_url).unwrap_or_else(|_| "\"\"".to_string());
    format!(
        r#"<script>
(function() {{
  const upstreamBase = {upstream_json};
  const bookSourceUrl = {source_json};
  const proxyPath = "/reader3/bookSourceProxy";
  const alreadyProxyPattern = /^\/reader3\/bookSourceProxy(?:\?|$)/i;
  const skipPattern = /^(#|javascript:|data:|mailto:|tel:)/i;
  function toAbsolute(url) {{
    try {{ return new URL(url, upstreamBase).href; }} catch (_e) {{ return url; }}
  }}
  function toProxy(url) {{
    if (!url || skipPattern.test(url) || alreadyProxyPattern.test(url)) return url;
    const absolute = toAbsolute(url);
    if (String(absolute).indexOf("/reader3/bookSourceProxy?") !== -1) return absolute;
    const params = new URLSearchParams();
    params.set("bookSourceUrl", bookSourceUrl);
    params.set("url", absolute);
    return proxyPath + "?" + params.toString();
  }}
  window.__readerBookSourceProxy = {{ toProxy, upstreamBase }};
  const rawFetch = window.fetch ? window.fetch.bind(window) : null;
  if (rawFetch) {{
    window.fetch = function(input, init) {{
      try {{
        if (input instanceof Request) {{
          return rawFetch(new Request(toProxy(input.url), input), init);
        }}
        return rawFetch(toProxy(String(input)), init);
      }} catch (_e) {{
        return rawFetch(input, init);
      }}
    }};
  }}
  const rawOpen = XMLHttpRequest.prototype.open;
  XMLHttpRequest.prototype.open = function(method, url) {{
    arguments[1] = toProxy(String(url));
    return rawOpen.apply(this, arguments);
  }};
  document.addEventListener("submit", function(event) {{
    const form = event.target;
    if (!(form instanceof HTMLFormElement)) return;
    const action = form.getAttribute("action") || upstreamBase;
    form.setAttribute("action", toProxy(action));
  }}, true);
  document.addEventListener("click", function(event) {{
    const anchor = event.target && event.target.closest ? event.target.closest("a[href]") : null;
    if (!anchor) return;
    const href = anchor.getAttribute("href");
    if (!href || skipPattern.test(href)) return;
    anchor.setAttribute("href", toProxy(href));
  }}, true);
  function reportClientError(payload) {{
    try {{
      const params = new URLSearchParams();
      Object.entries(payload || {{}}).forEach(function(entry) {{
        const key = entry[0];
        const value = entry[1];
        if (value !== undefined && value !== null && value !== "") {{
          params.set(key, String(value));
        }}
      }});
      const url = "/reader3/bookSourceClientLog?" + params.toString();
      if (navigator.sendBeacon) {{
        navigator.sendBeacon(url);
      }} else if (rawFetch) {{
        rawFetch(url, {{ method: "POST" }});
      }}
    }} catch (_e) {{}}
  }}
  window.addEventListener("error", function(event) {{
    reportClientError({{
      message: event.message || "window error",
      source: event.filename || "",
      lineno: event.lineno || 0,
      colno: event.colno || 0,
      stack: event.error && event.error.stack ? event.error.stack : ""
    }});
  }});
  window.addEventListener("unhandledrejection", function(event) {{
    const reason = event.reason;
    reportClientError({{
      message: reason && reason.message ? reason.message : String(reason || "unhandled rejection"),
      stack: reason && reason.stack ? reason.stack : ""
    }});
  }});
}})();
</script>"#
    )
}

fn rewrite_proxy_actions(html: &str, upstream_url: &str, book_source_url: &str) -> String {
    let tag_re = Regex::new(r#"(?is)<[^>]+>"#).unwrap();
    let double_quoted = Regex::new(r#"(?i)\b(action|href|src)\s*=\s*"([^"]+)""#).unwrap();
    let single_quoted = Regex::new(r#"(?i)\b(action|href|src)\s*=\s*'([^']+)'"#).unwrap();

    tag_re
        .replace_all(html, |tag_caps: &Captures| {
            let tag = tag_caps.get(0).map(|m| m.as_str()).unwrap_or("");
            let output = double_quoted.replace_all(tag, |caps: &Captures| {
                rewrite_proxy_attr(&caps, upstream_url, book_source_url, "\"")
            });
            single_quoted
                .replace_all(&output, |caps: &Captures| {
                    rewrite_proxy_attr(&caps, upstream_url, book_source_url, "'")
                })
                .into_owned()
        })
        .into_owned()
}

fn rewrite_proxy_attr(
    caps: &Captures,
    upstream_url: &str,
    book_source_url: &str,
    quote: &str,
) -> String {
    let attr = caps.get(1).map(|m| m.as_str()).unwrap_or("href");
    let value = caps.get(2).map(|m| m.as_str()).unwrap_or("");
    let proxied = build_proxy_url(value, upstream_url, book_source_url)
        .unwrap_or_else(|| value.to_string());
    format!(r#"{attr}={quote}{proxied}{quote}"#)
}

fn build_proxy_url(raw_value: &str, upstream_url: &str, book_source_url: &str) -> Option<String> {
    let trimmed = raw_value.trim();
    if trimmed.is_empty()
        || trimmed.starts_with('#')
        || trimmed.starts_with("javascript:")
        || trimmed.starts_with("data:")
        || trimmed.starts_with("mailto:")
        || trimmed.starts_with("tel:")
        || trimmed.starts_with("/reader3/bookSourceProxy")
    {
        return None;
    }

    let absolute = Url::parse(trimmed)
        .or_else(|_| Url::parse(upstream_url).and_then(|base| base.join(trimmed)))
        .ok()?;
    let params = vec![
        format!("bookSourceUrl={}", urlencoding::encode(book_source_url)),
        format!("url={}", urlencoding::encode(absolute.as_str())),
    ];
    Some(format!("/reader3/bookSourceProxy?{}", params.join("&")))
}

fn rewrite_script_root_relative_urls(html: &str, upstream_url: &str, book_source_url: &str) -> String {
    let script_re = Regex::new(r#"(?is)<script\b[^>]*>.*?</script>"#).unwrap();
    let double_quoted = Regex::new(r#""(/[^"\\\s<]*)""#).unwrap();
    let single_quoted = Regex::new(r#"'(/[^'\\\s<]*)'"#).unwrap();

    script_re
        .replace_all(html, |script_caps: &Captures| {
            let script = script_caps.get(0).map(|m| m.as_str()).unwrap_or("");
            let output = double_quoted.replace_all(script, |caps: &Captures| {
                rewrite_script_url_literal(&caps, upstream_url, book_source_url, "\"")
            });
            single_quoted
                .replace_all(&output, |caps: &Captures| {
                    rewrite_script_url_literal(&caps, upstream_url, book_source_url, "'")
                })
                .into_owned()
        })
        .into_owned()
}

fn rewrite_script_url_literal(
    caps: &Captures,
    upstream_url: &str,
    book_source_url: &str,
    quote: &str,
) -> String {
    let raw_value = caps.get(1).map(|m| m.as_str()).unwrap_or("");
    let proxied = build_proxy_url(raw_value, upstream_url, book_source_url)
        .unwrap_or_else(|| raw_value.to_string());
    format!("{quote}{proxied}{quote}")
}

fn html_escape_attr(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('"', "&quot;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

fn rewrite_set_cookie_for_proxy(raw: &str) -> Option<String> {
    let mut parts = raw
        .split(';')
        .map(|part| part.trim())
        .filter(|part| !part.is_empty());
    let first = parts.next()?;
    if !first.contains('=') {
        return None;
    }

    let mut attrs = vec![
        first.to_string(),
        "Path=/reader3/bookSourceProxy".to_string(),
    ];
    for attr in parts {
        let lower = attr.to_ascii_lowercase();
        if lower.starts_with("domain=") || lower.starts_with("path=") || lower == "secure" {
            continue;
        }
        attrs.push(attr.to_string());
    }
    Some(attrs.join("; "))
}

// ─────────────────────────── bookSourceClientLog ───────────────────────────

#[derive(Debug, Deserialize, Default)]
struct BookSourceClientLogParam {
    message: Option<String>,
    source: Option<String>,
    lineno: Option<i64>,
    colno: Option<i64>,
    stack: Option<String>,
}

fn book_source_client_log_route(query: &str) -> tauri::http::Response<Vec<u8>> {
    let q: BookSourceClientLogParam = serde_urlencoded::from_str(query).unwrap_or_default();
    tracing::warn!(
        "bookSourceProxy client error: source={} line={} col={} message={} stack={}",
        q.source.as_deref().unwrap_or_default(),
        q.lineno.unwrap_or_default(),
        q.colno.unwrap_or_default(),
        q.message.as_deref().unwrap_or_default(),
        q.stack.as_deref().unwrap_or_default()
    );
    let body = serde_json::to_vec(&ApiResponse::ok(serde_json::json!({ "logged": true })))
        .unwrap_or_default();
    http::Response::builder()
        .status(200)
        .header(http::header::CONTENT_TYPE, "application/json")
        .body(body)
        .unwrap_or_default()
}