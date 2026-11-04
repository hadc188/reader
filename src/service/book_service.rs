use crate::crawler::{
    fetcher::{fetch_with_client, FetchResponse, RequestSpec, StrResponse},
    http_client::{HttpClient, ProxyMode, ProxyStatus},
    url_analyzer::{analyze_url, split_url_options},
};
use crate::error::error::AppError;
use crate::model::{
    book::Book,
    book_chapter::BookChapter,
    book_source::{BookSource, ExploreKind},
    search::SearchBook,
};
use crate::parser::js::{
    eval_js_with_bindings, take_js_logs, with_book_context, with_book_url, with_source,
    JsBookContext,
};
use crate::parser::rule_engine::RuleEngine;
use crate::storage::cache::file_cache::FileCache;
use crate::service::local_pdf_book::{is_local_pdf_origin, is_local_pdf_url};
use crate::util::hash::md5_hex;
use crate::util::text::{normalize_source_url, repair_encoded_url, visible_char_count};
use serde_json::json;
use regex::Regex;
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime};
use tokio::fs;
use tokio::sync::{Mutex, RwLock};
use tokio::time::{sleep, Instant};

/// 书源 JS 调用 `java.showBrowser(url, html)` 时捕获的请求, 供前端弹出面板。
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ShowBrowserRequest {
    pub url: String,
    pub html: Option<String>,
    pub preload_js: Option<String>,
    pub options: Option<serde_json::Value>,
    pub title: Option<String>,
    #[serde(default)]
    pub await_result: bool,
    /// 后端托管的面板 id(前端按 `${readerOrigin}/sourcePanel?panelId=...` 加载)。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub panel_id: Option<String>,
    /// 用**独立窗口**打开，而不是 iframe 面板。
    ///
    /// 只用于「看一眼就走」的链接（注册页、用户后台、网页版阅读页等）：这类
    /// 上游站点常带 `X-Frame-Options: SAMEORIGIN` / CSP `frame-ancestors`，或
    /// 用前端安全 SDK 检测「是否被 iframe 嵌入」并整页替换成拦截提示；独立窗口
    /// 里 `window.top === window`，这些检测都不触发。
    ///
    /// 与面板的区别还在于**不回传数据**：前端打开窗口后立即返回，
    /// 调用脚本拿到空页继续执行。
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub external_window: bool,
}

thread_local! {
    /// `showBrowser` 由 JS 同步调用, 结果先落在线程本地, 由调用方取走。
    static SHOW_BROWSER_REQUEST: std::cell::RefCell<Option<ShowBrowserRequest>> =
        const { std::cell::RefCell::new(None) };
}

/// 供 js.rs 的原生 `__show_browser` 原语写入。
pub fn set_show_browser_request(request: ShowBrowserRequest) {
    SHOW_BROWSER_REQUEST.with(|cell| *cell.borrow_mut() = Some(request));
}

/// 取走当前线程捕获的 `showBrowser` 请求(取走即清空, 不跨次调用泄漏)。
pub fn take_show_browser_request() -> Option<ShowBrowserRequest> {
    SHOW_BROWSER_REQUEST.with(|cell| cell.borrow_mut().take())
}

fn clear_show_browser_request() {
    SHOW_BROWSER_REQUEST.with(|cell| *cell.borrow_mut() = None);
}

/// 已打开的 `java.showBrowser` 面板: 面板 HTML 由后端托管, 前端只需按 id 加载
/// `${readerOrigin}/sourcePanel?panelId=...`, 面板内的桥脚本即可同源回调宿主。
#[derive(Clone, Default)]
pub struct SourcePanelEntry {
    pub url: String,
    pub html: String,
    pub preload_js: Option<String>,
    pub http: Option<crate::crawler::source_session::SourceHttpContext>,
    /// 归属书源, 面板内 `window.run` 需要在其 jsLib 作用域执行。
    pub book_source_url: String,
    pub book_url: String,
    pub chapter_url: String,
    /// `book` / `chapter` 绑定(JSON), 面板脚本 `writeConfig(chapter, ...)` 需要。
    pub book_json: String,
    pub chapter_json: String,
    /// 面板内的**二级链接**是否交给系统浏览器打开。
    ///
    /// RSS 单 URL 源的面板只托管首屏 HTML(在 `reader` origin 下渲染, 所以能正常
    /// 显示); 页面里的链接指向上游站点, 若在 iframe 内直接导航, 上游的
    /// `X-Frame-Options` / CSP `frame-ancestors` 会让 WebView 拒绝渲染 —— 表现为
    /// **白屏**。
    ///
    /// 书源登录面板必须保持 `false`: 登录/注册流程依赖 iframe 内导航。
    pub external_links: bool,
}

static SOURCE_PANELS: std::sync::LazyLock<std::sync::Mutex<Vec<(String, SourcePanelEntry)>>> =
    std::sync::LazyLock::new(|| std::sync::Mutex::new(Vec::new()));

/// 同时保留的面板数上限(插件通常只有 1~2 层弹层)。
const MAX_SOURCE_PANELS: usize = 8;

/// 登记一个面板并返回其 id。
pub fn register_source_panel(entry: SourcePanelEntry) -> String {
    let id = uuid::Uuid::new_v4().simple().to_string();
    let mut panels = SOURCE_PANELS.lock().unwrap_or_else(|e| e.into_inner());
    // 按插入顺序淘汰最旧的(用 Vec 而非 HashMap: 后者无序, 淘汰会变成随机的,
    // 可能剔掉正在显示的面板导致其后续交互 404)
    if panels.len() >= MAX_SOURCE_PANELS {
        let (old_id, _) = panels.remove(0);
        crate::service::source_browser::cancel_browser(&old_id);
    }
    panels.push((id.clone(), entry));
    id
}

pub fn remove_source_panel(panel_id: &str) {
    SOURCE_PANELS.lock().unwrap_or_else(|e| e.into_inner()).retain(|(id, _)| id != panel_id);
}

pub fn get_source_panel(panel_id: &str) -> Option<SourcePanelEntry> {
    SOURCE_PANELS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .iter()
        .find(|(id, _)| id == panel_id)
        .map(|(_, entry)| entry.clone())
}

/// 把面板页写入的 `document.cookie` 落到书源 cookie jar。
///
/// 面板页(如某聚合源的登录页)用 `document.cookie = 'user_token=…'` 记录会话,
/// 而上游后续页面由**服务端**读 Cookie 判断登录态。只写浏览器的话这份 cookie
/// 留在面板 origin 下, 转发请求带不上, 上游就把页面踢回登录页(用户看到
/// 「登录过期」)。这里按面板当前的上游地址写入 jar, 让后续转发带上它。
///
/// 目标地址必须落在书源域内 —— 否则面板页脚本就能把会话写到任意域。
pub(crate) fn set_panel_cookie(entry: &SourcePanelEntry, raw_url: &str, cookie: &str) -> Result<(), String> {
    let head = cookie.split(';').next().unwrap_or("").trim();
    let Some((name, value)) = head.split_once('=') else {
        return Err("Cookie 格式无效".to_string());
    };
    let name = name.trim();
    if name.is_empty() {
        return Err("Cookie 格式无效".to_string());
    }
    // 控制字符在写入处就拒掉, 不依赖下层只做 header 值校验。
    if name.chars().any(char::is_control) || value.chars().any(char::is_control) {
        return Err("Cookie 含控制字符".to_string());
    }
    let http = entry.http.clone().map(Ok).unwrap_or_else(|| {
        crate::parser::js::source_http_context(&entry.book_source_url)
    }).map_err(|e| e.to_string())?;
    // 以面板当前的上游地址为域写入; 页面可能在子路径下, 域名与端口仍取自它。
    let target = if raw_url.is_empty() { entry.url.clone() } else { raw_url.to_string() };
    let url = crate::api::protocol::resolve_panel_cookie_url(&target, &entry.url)
        .map_err(|e| e.to_string())?;
    if !crate::api::protocol::proxy_target_matches_source(url.as_str(), &entry.url) {
        return Err("Cookie 目标地址不在书源域内".to_string());
    }
    // Cookie 不区分 scheme: 把 https 会话写进 http 目标会让后续明文请求带上它。
    // 只在此处拦降级, 不改共享的域判定函数(那会影响既有代理行为)。
    let source_is_https = crate::api::protocol::resolve_panel_cookie_url(&entry.url, &entry.url)
        .map(|u| u.scheme() == "https").unwrap_or(false);
    if source_is_https && url.scheme() == "http" {
        return Err("不允许把 https 会话写入 http 目标".to_string());
    }
    // 空值代表页面删除了该 cookie(退出登录), 必须从 jar 移除, 否则空值会被当成
    // 有效值发给上游。
    if value.trim().is_empty() {
        return http.session.cookies.remove(&url, name).map_err(|e| e.to_string());
    }
    http.session.cookies.set(&url, head).map_err(|e| e.to_string())
}

/// 处理面板内 `window.java` / `window.cache` / `window.run` 的同步桥请求。
///
/// 与 legado `WebJsExtensions` 对齐: `ajax`/`get`/`post`/`connect` 走书源网络层,
/// `run` 在书源 jsLib 作用域执行脚本, `cache` 复用书源变量存储。
pub async fn handle_source_panel_bridge(
    book_source_service: &crate::service::book_source_service::BookSourceService,
    entry: &SourcePanelEntry,
    action: &str,
    payload: serde_json::Value,
) -> Result<serde_json::Value, String> {
    let text_of = |key: &str| -> String {
        payload
            .get(key)
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string()
    };

    match action {
        "log" => {
            crate::parser::js::push_js_log(format!("[panel] {}", text_of("message")));
            Ok(serde_json::Value::Null)
        }
        "toast" => {
            let message = text_of("message");
            crate::parser::js::push_js_log(format!("[panel.toast] {message}"));
            Ok(serde_json::Value::String(message))
        }
        "close" => {
            Ok(serde_json::Value::Null)
        }
        "sourceGet" | "sourceSet" => {
            let key = text_of("key");
            if !matches!(key.as_str(), "variable" | "loginInfo" | "loginHeader") {
                return Err("未知的书源状态".to_string());
            }
            let http = entry.http.clone().map(Ok).unwrap_or_else(|| {
                crate::parser::js::source_http_context(&entry.book_source_url)
            }).map_err(|e| e.to_string())?;
            if action == "sourceSet" {
                let value = payload.get("value").and_then(|v| v.as_str());
                http.session.put(&key, value).map_err(|e| e.to_string())?;
                Ok(json!(true))
            } else {
                Ok(json!(http.session.get(&key)))
            }
        }
        // 面板页 JS 用 `document.cookie = 'k=v'` 记录登录态(某聚合源的登录页写
        // `user_token`), 而上游后续页面由**服务端**读 Cookie 判断登录, 只留在浏览器里
        // 转发请求带不上, 上游会当作未登录并把页面踢回登录页(表现为「登录过期」)。
        // 这里把它落到书源 jar, 随后的转发请求就能带上。
        "cookieSet" => match set_panel_cookie(entry, &text_of("url"), &text_of("cookie")) {
            Ok(()) => Ok(json!(true)),
            Err(error) => {
                // 域校验拒绝/格式错误等要留痕: 「登录过期」类问题的现场诊断依赖这条日志。
                crate::parser::js::push_js_log(format!("[panel.cookieSet] {error}"));
                Err(error)
            }
        },
        "cacheGet" => {
            let key = text_of("key");
            Ok(crate::parser::js::get_panel_cache(&entry.book_source_url, &key)
                .map(serde_json::Value::String)
                .unwrap_or(serde_json::Value::Null))
        }
        "cachePut" => {
            let key = text_of("key");
            let value = text_of("value");
            crate::parser::js::put_panel_cache(&entry.book_source_url, &key, &value);
            Ok(serde_json::Value::Null)
        }
        "run" => {
            let script = text_of("script");
            let source = book_source_service
                .get("default", &entry.book_source_url)
                .await
                .map_err(|e| e.to_string())?
                .ok_or_else(|| "bookSource not found".to_string())?;
            let entry = entry.clone();
            // 面板脚本形如 `writeConfig(chapter, 'k', v, java)`, 需要 book/chapter 绑定;
            // 直接用面板登记时的书籍上下文, 否则脚本里的 `chapter` 会是空对象。
            let book_ctx = crate::parser::js::JsBookContext {
                book: serde_json::from_str(&entry.book_json).unwrap_or_else(|_| json!({})),
                chapter: serde_json::from_str(&entry.chapter_json).unwrap_or_else(|_| json!({})),
            };
            let output = tokio::task::spawn_blocking(move || {
                let execute = || crate::parser::js::with_book_context(Some(book_ctx), || {
                    crate::parser::js::with_source(&source, || {
                        crate::parser::js::with_book_url(Some(&entry.book_url), || {
                            take_js_logs();
                            clear_show_browser_request();
                            crate::parser::js::take_login_effects();
                            let result = crate::parser::js::eval_js(&script, "", &entry.url);
                            let browser = take_show_browser_request();
                            let logs = take_js_logs();
                            let effects = crate::parser::js::take_login_effects();
                            let result = result?;
                            let browser = browser.map(|request| {
                                crate::service::source_browser::register_browser_request(request, entry.clone())
                            }).transpose()?;
                            Ok::<_, anyhow::Error>(json!({"result":result,"showBrowser":browser,"logs":logs,"loginEffects":effects}))
                        })
                    })
                });
                match entry.http.clone() {
                    Some(http) => crate::parser::js::with_http_context(http, execute),
                    None => execute(),
                }
            })
            .await
            .map_err(|e| e.to_string())?;
            output.map_err(|e| format!("脚本执行失败: {e}"))
        }
        "ajax" | "connect" | "http" | "fetch" => {
            let source = book_source_service
                .get("default", &entry.book_source_url)
                .await
                .map_err(|e| e.to_string())?
                .ok_or_else(|| "bookSource not found".to_string())?;
            let action = action.to_string();
            let payload = payload.clone();
            let entry = entry.clone();
            let result = tokio::task::spawn_blocking(move || {
                let execute = || {
                    if action == "fetch" {
                        crate::parser::js::run_panel_fetch(&source, &entry.url, &action, &payload)
                    } else {
                        crate::parser::js::run_panel_request(&source, &entry.url, &action, &payload).map(serde_json::Value::String)
                    }
                };
                match entry.http.clone() {
                    Some(http) => crate::parser::js::with_http_context(http, execute),
                    None => execute(),
                }
            })
            .await
            .map_err(|e| e.to_string())?;
            result.map_err(|e| e.to_string())
        }
        // 面板内再次调 `java.showBrowser`: 登记为新面板并回传 panelId,
        // 由桥通过 postMessage 通知宿主换成新面板(嵌套弹层)。
        "showBrowser" => {
            let options = payload.get("options").and_then(|value| {
                if let Some(text) = value.as_str() { serde_json::from_str::<serde_json::Value>(text).ok() }
                else { Some(value.clone()) }
            }).filter(|value| value.is_object());
            let request = crate::service::source_browser::register_browser_request(ShowBrowserRequest {
                url: text_of("url"),
                html: payload.get("html").and_then(|v| v.as_str()).map(str::to_string),
                preload_js: payload.get("preloadJs").and_then(|v| v.as_str()).map(str::to_string),
                options, title: None, await_result: false, panel_id: None, external_window: false,
            }, entry.clone()).map_err(|e| e.to_string())?;
            Ok(json!(request))
        }
        // 面板签名用的对称加密(与书源规则的 `java.createSymmetricCrypto` 同一实现)。
        // 部分书源接口靠它签请求, 缺了这个插件只能降级到 CryptoJS 注入, 容易失败。
        "crypto" => crate::parser::js::panel_symmetric_crypto(&payload)
            .map(serde_json::Value::String)
            .map_err(|e| e.to_string()),
        "digest" => crate::parser::js::panel_digest(&payload)
            .map(serde_json::Value::String)
            .map_err(|e| e.to_string()),
        other => Err(format!("未知的面板桥操作: {other}")),
    }
}

/// 章节内容加载阶段: 让前端显示"正在做什么", 避免长时间无反馈看起来像卡死。
///
/// 只用于**阅读时的单章加载**。缓存是并发多章的(见 `cache_book_sse`),
/// 同一时刻不同章节处在不同阶段, 单一阶段对它没有意义。
#[derive(Clone, Copy, Debug, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ContentLoadStage {
    /// 正在请求书源章节页
    Fetching,
    /// 正在执行正文规则(含书源脚本与段评解析), 通常是最耗时的一步
    Parsing,
    /// 正在写入本地缓存
    Caching,
}

/// State for background chapter fetching
#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub struct ChapterPagination {
    pub user_ns: String,
    pub source: BookSource,
    pub toc_url: String,
    pub visited_urls: Vec<String>,
    pub pending_urls: Vec<String>,
    pub seen_chapter_urls: Vec<String>,
    pub next_index: i32,
}

/// 取消信号: 测试书源等后台任务用它中断在途请求。
///
/// 用 `AtomicBool` 而不是 `CancellationToken`, 与既有的 `CACHE_BOOK_CANCELLATIONS`
/// / `SEARCH_CANCELLATIONS` 保持同一套模式。
pub type CancelSignal = Arc<std::sync::atomic::AtomicBool>;

/// 检查取消标志(集中在两处 loop 里用, 避免每处都写一遍 `load(Relaxed)`)。
fn is_cancelled(signal: Option<&CancelSignal>) -> bool {
    signal.is_some_and(|flag| flag.load(std::sync::atomic::Ordering::Relaxed))
}

/// 判断一个书源会话目录是否「什么都没有」。
///
/// `SourceSession` 只写 `cookies.json` 与 `state.json`，两者在无内容时分别是
/// `{}` 和 `{}`（或空串）。只要其中任意一个有实质内容（登录态、`variable`、
/// `loginInfo` 等）就判定为非空 —— 宁可多留一个目录，也不能删掉用户的登录态。
async fn source_state_dir_is_empty(dir: &std::path::Path) -> bool {
    for name in ["cookies.json", "state.json"] {
        let Ok(raw) = tokio::fs::read_to_string(dir.join(name)).await else {
            // 读不到（不存在/权限）就当它没有内容, 继续看下一个文件。
            continue;
        };
        let trimmed = raw.trim();
        if trimmed.is_empty() || trimmed == "{}" || trimmed == "[]" || trimmed == "null" {
            continue;
        }
        return false;
    }
    true
}

/// 阅读页统一渲染章节标题；正文若自带 `<h1>` 等标题块会重复显示。
/// 只剥离开头第一个明确标题元素，保留图片和普通正文。
fn strip_leading_content_title(content: &str) -> String {
    let trimmed_start = content.trim_start_matches(|ch: char| {
        ch.is_whitespace() || matches!(ch, '\u{feff}' | '\u{200b}' | '\u{200c}' | '\u{200d}')
    });
    let trimmed_start = trimmed_start
        .strip_prefix("&nbsp;")
        .map(str::trim_start)
        .unwrap_or(trimmed_start);
    let Some(opening_end) = trimmed_start.find('>') else {
        return content.to_string();
    };
    let opening = &trimmed_start[..=opening_end];
    let tag = opening
        .trim_start_matches('<')
        .split(|ch: char| ch.is_whitespace() || ch == '>' || ch == '/')
        .next()
        .unwrap_or("")
        .to_ascii_lowercase();
    let is_heading = matches!(tag.as_str(), "h1" | "h2" | "h3" | "h4" | "h5" | "h6");
    let lower_opening = opening.to_ascii_lowercase();
    let is_title_class = tag == "p"
        && (lower_opening.contains("chapter-title")
            || lower_opening.contains("chapter_title")
            || lower_opening.contains("class=\"title")
            || lower_opening.contains("class='title"));
    // `.con@html` returns the container's outer HTML, so the first node is
    // usually `<div class="con">` and the chapter heading lives inside it.
    if matches!(tag.as_str(), "div" | "section" | "article") {
        let close_tag = format!("</{tag}>");
        let lower = trimmed_start.to_ascii_lowercase();
        let Some(close_offset) = lower[opening_end + 1..].find(&close_tag) else {
            return content.to_string();
        };
        let inner_end = opening_end + 1 + close_offset;
        let inner = &trimmed_start[opening_end + 1..inner_end];
        let stripped_inner = strip_leading_content_title(inner);
        if stripped_inner == inner {
            return content.to_string();
        }
        return format!(
            "{}{}{}",
            &trimmed_start[..opening_end + 1],
            stripped_inner,
            &trimmed_start[inner_end..]
        );
    }
    let close_tag = format!("</{tag}>");
    let lower = trimmed_start.to_ascii_lowercase();
    let Some(close_offset) = lower[opening_end + 1..].find(&close_tag) else {
        return content.to_string();
    };
    let end = opening_end + 1 + close_offset + close_tag.len();
    let inner_html = &trimmed_start[opening_end + 1..opening_end + 1 + close_offset];
    static TITLE_TAG_RE: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
    let title_tag_re = TITLE_TAG_RE.get_or_init(|| Regex::new(r"<[^>]+>").expect("title tag regex"));
    let inner_text = title_tag_re.replace_all(inner_html, "").into_owned();
    let inner_text = inner_text.trim();
    static CHAPTER_HEADING_RE: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
    let chapter_heading_re = CHAPTER_HEADING_RE
        .get_or_init(|| Regex::new(r"^第[0-9一二三四五六七八九十百千零〇两]+[章节回卷]").expect("chapter heading regex"));
    let chapter_heading = tag == "p"
        && inner_text.chars().count() <= 80
        && chapter_heading_re.is_match(inner_text);
    if !is_heading && !is_title_class && !chapter_heading {
        return content.to_string();
    }
    trimmed_start[end..].trim_start().to_string()
}

/// 取消错误标记: 让调用方能把它与「站点请求失败」区分开。
///
/// 测试书源必须区分这两者 —— 把用户主动中止当成检测失败会把可用书源误标为失效。
pub const CANCELLED_ERR: &str = "cancelled by user";

static SOURCE_WINDOW_STORAGE: std::sync::LazyLock<
    std::sync::Mutex<std::collections::HashMap<String, Vec<(String, String)>>>,
> = std::sync::LazyLock::new(|| std::sync::Mutex::new(std::collections::HashMap::new()));

/// 测试用：直接写入某个 origin 的 localStorage 快照。
///
/// 不加 `#[cfg(test)]`：集成测试（`tests/*.rs`）链接的是库的**非测试**构建，
/// 带该标注会在那边看不到符号。
pub fn set_source_window_storage_for_test(origin_url: &str, entries: &[(String, String)]) {
    let Ok(url) = url::Url::parse(origin_url.trim()) else {
        return;
    };
    SOURCE_WINDOW_STORAGE
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .insert(url.origin().ascii_serialization(), entries.to_vec());
}

pub fn source_window_storage_for(source_url: &str) -> Vec<(String, String)> {
    let Ok(url) = url::Url::parse(source_url.trim()) else {
        return Vec::new();
    };
    let origin = url.origin().ascii_serialization();
    if origin == "null" {
        return Vec::new();
    }
    SOURCE_WINDOW_STORAGE
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get(&origin)
        .cloned()
        .unwrap_or_default()
}

#[derive(Clone)]
pub struct BookService {
    http: HttpClient,
    parser: RuleEngine,
    cache: FileCache,
    storage_dir: PathBuf,
    login_sessions: Arc<RwLock<HashMap<String, SourceLoginSession>>>,
    rate_states: Arc<RwLock<HashMap<String, RateState>>>,
    bookshelf_write_lock: Arc<Mutex<()>>,
}

#[derive(Clone, Default)]
struct RateState {
    in_flight: bool,
    last_start: Option<Instant>,
    window_starts: Vec<Instant>,
}

struct RateSlotRelease {
    service: BookService,
    source: BookSource,
}

impl Drop for RateSlotRelease {
    fn drop(&mut self) {
        let Ok(handle) = tokio::runtime::Handle::try_current() else {
            return;
        };
        let service = self.service.clone();
        let source = self.source.clone();
        handle.spawn(async move {
            service.finish_rate(&source).await;
        });
    }
}

/// 发现页控件 `action` 的执行结果。
///
/// `action` 脚本通常只产生**副作用**（刷新发现页、打开登录面板、发起搜索），
/// 返回值往往为空，所以真正要传回前端的是这里的一组意图标志。
#[derive(Debug, Clone, Default, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExploreActionOutcome {
    /// 脚本的返回值（多数 action 为空串）。
    pub value: String,
    /// `java.refreshExplore()` 被调用：前端应重新拉分类与列表。
    pub explore_refresh: bool,
    /// `java.reLoginView()` / `source.login()`：前端应重开登录面板。
    pub login_refresh: bool,
    pub login: bool,
    /// `java.open(...)` 请求打开的面板，前端据此跳转。
    pub open_urls: Option<String>,
    /// `java.open` / `java.searchBook` / `java.openBook` 登记的待执行动作，
    /// 形如 `{"kind":"search","payload":{...}}`。
    pub intents: Vec<serde_json::Value>,
    /// `java.startBrowser` / `java.showBrowser` 弹出的面板（legado 的
    /// BottomWebViewDialog）。
    ///
    /// 缺了它，"更新书源""书源设置""刷新发现页设置"这类按钮点了毫无反应 ——
    /// 脚本已经把面板 HTML 准备好了，只是没人把它交给前端渲染。
    pub show_browser: Option<ShowBrowserRequest>,
    /// 脚本写入的 UI 值（如 `java.upLoginData`），前端可回填到控件。
    pub values: serde_json::Map<String, serde_json::Value>,
    /// 脚本产生的提示（`java.toast` / `java.longToast`）。
    ///
    /// "更新配置""已是最新配置"这类反馈全靠它 —— 早先被丢掉，用户以为按钮坏了。
    pub messages: Vec<String>,
    pub logs: Vec<String>,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BookSourceAvailability {
    pub book_source_url: String,
    pub book_source_name: String,
    pub valid: bool,
    pub search_ok: bool,
    pub explore_ok: bool,
    pub keyword: String,
    pub explore_url: Option<String>,
    pub search_error: Option<String>,
    pub explore_error: Option<String>,
    /// 用户主动中止导致本次检测未完成。
    ///
    /// 调用方必须跳过"按结果写库": 这不是"站点不可用", 把它当失效会把好源标坏。
    /// 统计时也要排除, 否则中止会让"失效数"虚高。
    #[serde(default)]
    pub cancelled: bool,
}

/// 构造一个"已取消"的占位结果(不写库、不计入有效/失效)。
fn cancelled_availability(source: &BookSource, keyword: &str) -> BookSourceAvailability {
    BookSourceAvailability {
        book_source_url: source.book_source_url.clone(),
        book_source_name: source.book_source_name.clone(),
        valid: false,
        search_ok: false,
        explore_ok: false,
        keyword: keyword.to_string(),
        explore_url: None,
        search_error: None,
        explore_error: None,
        cancelled: true,
    }
}

/// A single step of the source debugger: what URL was fetched, the raw response
/// (truncated) and the parsed result.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DebugTrace {
    pub request_url: String,
    pub status: u16,
    pub body: String,
    pub result: serde_json::Value,
    /// 反爬/异常特征提示, 帮助区分「站点拦截」与「规则写错」。
    pub warnings: Vec<String>,
    /// 响应头(Set-Cookie / Location 等), 辅助判断 UA 切换与重定向。
    pub headers: Vec<(String, String)>,
}

#[derive(Clone)]
struct SourceLoginSession {
    source_url: String,
    created_at: Instant,
}

/// Detect anti-crawler / degradation markers so the debugger can tell「被站点拦截」
/// apart from「规则写错」. Factors collected from real-site debugging sessions:
/// - 202 + `var buid` = PC 站验证壳(要浏览器指纹)
/// - 验证码页 / WAF tunnel
/// - 移动站被重定向到 PC 站(`source=m_jump`)
/// - 非 HTML 的极小响应(代理网关 / 错误页)
fn detect_anti_crawler(
    status: u16,
    body: &str,
    headers: &[(String, String)],
) -> Vec<String> {
    let mut warnings = Vec::new();
    let lower = body.to_lowercase();

    if status >= 400 {
        warnings.push(format!("HTTP {status}, 疑似被拦截或页面异常"));
    }
    if status == 202 || lower.contains("var buid") {
        warnings.push("反爬验证壳(202 / var buid), 站点要求浏览器指纹, 普通请求必被拦".to_string());
    }
    if lower.contains("turing.captcha") || lower.contains("tcaptcha") || lower.contains("验证码")
    {
        warnings.push("触发验证码(WAF), 需要真人验证".to_string());
    }
    if lower.contains("source=m_jump") || lower.contains("m_jump") {
        warnings.push("被重定向到 PC 站(UA 触发移动→PC 跳转)".to_string());
    }
    let is_html = headers.iter().any(|(k, v)| {
        k.eq_ignore_ascii_case("content-type") && v.to_lowercase().contains("text/html")
    });
    if !is_html && body.len() < 2000 {
        warnings.push("响应非 HTML 且过小, 可能被代理/网关拦截".to_string());
    }
    warnings
}

/// 判断响应是否像「代理出口被站点 WAF 拦下」的拒绝页。
///
/// 判据保守: 只在状态码明确是代理/网关拒绝(403/407/451), 且正文小得不像任何
/// 真实页面时才成立。正常情况下直连重试的开销是零 —— 不满足条件直接返回原响应。
fn response_looks_proxy_blocked(response: &FetchResponse) -> bool {
    if !matches!(response.status, 403 | 407 | 451) {
        return false;
    }
    // 拒绝页通常只有几百字节(纯文本 403 页); 真实页面即使精简也不少于此。
    response.body.len() < 4096
}

/// 封面请求是否需要改用直连重试。
///
/// 只在**确实走了代理**、且要么请求直接失败(超时/连接被拒)、要么拿到
/// 403/407/451 这类 WAF 拒绝码时才重试一次。其它状态码(含 404/500)不重试:
/// 那不是"代理被拦", 重发只会把封面加载时间翻倍。
///
/// 与正文链路的 [`response_looks_proxy_blocked`] 同一思路, 但封面在读取响应体
/// 之前就要决定是否重试, 因此这里只看状态码, 不做体积判断。
fn cover_needs_direct_retry(proxy_active: bool, status: Option<u16>) -> bool {
    proxy_active && matches!(status, None | Some(403) | Some(407) | Some(451))
}

/// Build a DebugTrace from a fetched response, attaching anti-crawler warnings
/// and the response headers for the debugger UI.
fn debug_trace_from(res: &FetchResponse, result: serde_json::Value) -> DebugTrace {
    let warnings = detect_anti_crawler(res.status, &res.body, &res.headers);
    DebugTrace {
        request_url: res.url.clone(),
        status: res.status,
        body: truncate_trace_body(&res.body),
        result,
        warnings,
        headers: res.headers.clone(),
    }
}

/// Keep the raw response readable in the debugger UI without flooding it.
fn truncate_trace_body(body: &str) -> String {
    const MAX: usize = 50_000;
    if body.len() <= MAX {
        body.to_string()
    } else {
        // body[..MAX] 若 MAX 落在多字节 UTF-8 字符中间会 panic。
        // 用 floor_char_boundary 回退到最近的字符边界。
        let mut end = MAX;
        while end > 0 && !body.is_char_boundary(end) {
            end -= 1;
        }
        let mut s = body[..end].to_string();
        s.push_str("\n…[截断]");
        s
    }
}

impl BookService {
    pub fn new(http: HttpClient, parser: RuleEngine, cache: FileCache, storage_dir: &str) -> Self {
        let storage_dir = PathBuf::from(storage_dir);
        http.set_source_storage_dir(storage_dir.join("source-state"));
        Self {
            http,
            parser,
            cache,
            storage_dir,
            login_sessions: Arc::new(RwLock::new(HashMap::new())),
            rate_states: Arc::new(RwLock::new(HashMap::new())),
            bookshelf_write_lock: Arc::new(Mutex::new(())),
        }
    }

    pub fn http_client(&self) -> reqwest::Client {
        self.http.client()
    }

    /// 当前生效的 HTTP 代理地址（形如 `http://host:port`）。
    ///
    /// 给不走 `reqwest` 的通道用：Edge 朗读是 WebSocket，连接由 tungstenite 自己建，
    /// 拿不到 `reqwest::Client` 上的代理配置，必须显式把地址传下去，否则「设置里配了
    /// 代理」的用户会出现「音色能列、合成必失败」。
    pub fn active_proxy(&self) -> Option<String> {
        self.http.active_proxy()
    }

    /// Return a client isolated to one book source. Its cookie jar is never
    /// shared with another source or with the app's generic HTTP client.
    pub async fn source_http_client(
        &self, user_ns: &str, source_url: &str, proxy: Option<&str>,
    ) -> Result<reqwest::Client, AppError> {
        Ok(self.http.source_context(user_ns, source_url, proxy).map_err(AppError::Internal)?.client)
    }

    pub fn source_scope<T>(&self, user_ns: &str, source: &BookSource, f: impl FnOnce() -> T) -> Result<T, AppError> {
        let context = self.http.source_context(user_ns, &source.book_source_url, None).map_err(AppError::Internal)?;
        Ok(crate::parser::js::with_http_context(context, || with_source(source, f)))
    }

    async fn run_source_task<T: Send + 'static>(
        &self, user_ns: &str, source: &BookSource,
        f: impl FnOnce(&BookSource) -> T + Send + 'static,
    ) -> Result<T, AppError> {
        let context = self.http.source_context(user_ns, &source.book_source_url, None).map_err(AppError::Internal)?;
        let source = source.clone();
        tokio::task::spawn_blocking(move || crate::parser::js::with_http_context(context, || with_source(&source, || f(&source))))
            .await.map_err(|err| AppError::Internal(anyhow::anyhow!("书源任务失败: {err}")))
    }

    async fn analyze_request(&self, user_ns: &str, source: &BookSource, rule: &str, key: &str, page: i32, base: &str) -> Result<RequestSpec, AppError> {
        let (rule, key, base) = (rule.to_string(), key.to_string(), base.to_string());
        self.run_source_task(user_ns, source, move |source| crate::parser::js::with_search_context(&key, page, || analyze_url(&rule, &key, page, &base, source))).await?
    }

    async fn parse_toc_page(&self, user_ns: &str, source: &BookSource, res: &FetchResponse, book_url: &str) -> Result<(Vec<BookChapter>, Vec<String>), AppError> {
        let mut book_ctx = self.js_book_context(user_ns, book_url, "").await.unwrap_or_default();
        book_ctx.book["tocUrl"] = json!(book_url);
        let source_key = book_ctx.book["bookUrl"].as_str().unwrap_or(book_url).to_string();
        let (body, url) = (res.body.clone(), if book_url.trim().is_empty() { res.url.clone() } else { book_url.to_string() });
        self.run_source_task(user_ns, source, move |source| {
            with_book_context(Some(book_ctx), || {
                RuleEngine::new().unwrap().chapter_list(source, &body, &url, Some(&source_key))
            })
        }).await
    }

    async fn parse_info_page(&self, user_ns: &str, source: &BookSource, res: &FetchResponse, book_url: &str) -> Result<Book, AppError> {
        // 详情响应为空时给出可诊断的提示: 规则里的 `result` 会是空串,
        // 表现为书源脚本自己抛出的公式化异常(如"详情bookUrl为空:"),
        // 掩盖了真正原因(线路失效/站点返回空页/被限流)。
        if res.body.trim().is_empty() {
            return Err(AppError::BadRequest(format!(
                "详情页响应为空（HTTP {}，{}）。请检查书源线路是否可用或稍后重试",
                res.status,
                if res.url.is_empty() { book_url.to_string() } else { res.url.clone() }
            )));
        }
        let book_ctx = self.js_book_context(user_ns, book_url, "").await.unwrap_or_default();
        let read_config = book_ctx.book.get("readConfig").cloned()
            .and_then(|value| serde_json::from_value(value).ok());
        // 书架里已有的书名：详情规则取不到名字时用它兜底（见下方说明）。
        let shelf_name = book_ctx.book.get("name").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let (body, url, book_url) = (res.body.clone(), res.url.clone(), book_url.to_string());
        self.run_source_task(user_ns, source, move |source| {
            take_js_logs();
            let mut book = with_book_context(Some(book_ctx), || {
                RuleEngine::new().unwrap().book_info(source, &body, &url, &book_url)
            });
            book.read_config = read_config;
            apply_book_read_config_override(&mut book);
            let logs = take_js_logs();

            // 书名取不到时的兜底与报错策略。
            //
            // legado 的 `BookInfo.analyzeBookInfo`（BookInfo.kt:68-73）在详情规则
            // 拿不到名字时**保留书籍原有名字**，不把整次解析判为失败：
            //   `if (it.isNotEmpty() && (mCanReName || book.name.isEmpty())) book.name = it`
            //
            // 书源的详情规则常在开头做登录态假设，例如该源 `ruleBookInfo.name` 的
            // 第一行 `result.match(/Ark.CSRF_TOKEN = '([^']+)'/)[1]` —— 未登录时
            // 抛 `cannot read property of null`，脚本中断、名字为空。此时若直接报
            // 「详情解析失败」，用户就会「打开详细页就报错」；而目录、正文其实都能
            // 正常取到（`tocUrl` 是独立字段）。
            //
            // 所以：能从书架拿到名字就用它，否则退回 URL 尾段；真的什么都拿不到
            //（且脚本报错）才提示失败。
            if book.name.trim().is_empty() {
                if !shelf_name.trim().is_empty() {
                    book.name = shelf_name;
                } else {
                    // 退回 URL 的**路径末段**（先剥掉 query/fragment 再取最后一段）。
                    // 顺序反了会拿到 `?dcs=…` 这种查询串、剥完变空串，兜底就失效了。
                    //
                    // 还必须先剥掉 legado 的 URL 选项 `,{"body":…,"method":"POST"}`：
                    // 书源的 bookUrl 规则常返回「地址 + 请求选项」的复合串，不剥就会
                    // 把整段选项当成书名显示出来（界面里书名变成
                    // `info,{"body":"user_id=…"}`）。
                    let url_only = crate::crawler::url_analyzer::split_url_options(&book_url).0;
                    let without_query = url_only
                        .split(['?', '#'])
                        .next()
                        .unwrap_or("")
                        .trim_end_matches('/');
                    let fallback = without_query.rsplit('/').next().unwrap_or("").to_string();
                    if !fallback.is_empty() {
                        book.name = fallback;
                    } else if let Some(error) = first_js_error(&logs) {
                        return Err(AppError::BadRequest(format!("书籍详情解析失败: {error}")));
                    }
                }
            }
            Ok(book)
        }).await?
    }

    async fn check_source_response(&self, user_ns: &str, source: &BookSource, res: FetchResponse) -> Result<FetchResponse, AppError> {
        if source.login_check_js.as_deref().unwrap_or("").trim().is_empty() { return Ok(res); }
        let retry_url = res.url.clone();
        let (checked, login_requested) = self.run_source_task(user_ns, source, move |source| apply_login_check_js(source, res)).await??;
        // `source.login()`: 校验脚本已请求重新登录(典型: 响应 401/40101)。
        // 用已保存的登录信息重跑 loginUrl, 再用同一个 URL 重取一次。
        if !login_requested { return Ok(checked); }
        if let Err(err) = self.run_source_login(source, Some("login".into()), None).await {
            tracing::warn!("source.login() 重新登录失败: {err:?}");
            return Ok(checked);
        }
        let spec = match crate::crawler::url_analyzer::analyze_url(&retry_url, "", 1, &retry_url, source) {
            Ok(spec) => spec,
            Err(err) => {
                tracing::warn!("source.login() 重取请求构造失败: {err:?}");
                return Ok(checked);
            }
        };
        match self.fetch_with_rate(user_ns, source, spec).await {
            Ok(retried) => Ok(retried),
            Err(err) => {
                tracing::warn!("source.login() 重取响应失败: {err:?}");
                Ok(checked)
            }
        }
    }

    pub async fn create_source_login_session(&self, source_url: &str) -> String {
        let token = uuid::Uuid::new_v4().simple().to_string();
        let source_url = normalize_source_url(source_url);
        let mut sessions = self.login_sessions.write().await;
        sessions.retain(|_, session| {
            session.source_url != source_url
                && session.created_at.elapsed() <= Duration::from_secs(30 * 60)
        });
        sessions.insert(
            token.clone(),
            SourceLoginSession {
                source_url,
                created_at: Instant::now(),
            },
        );
        token
    }

    pub async fn source_for_login_session(&self, token: &str) -> Option<String> {
        let mut sessions = self.login_sessions.write().await;
        sessions.retain(|_, session| session.created_at.elapsed() <= Duration::from_secs(30 * 60));
        sessions.get(token).map(|session| session.source_url.clone())
    }

    pub async fn configure_network_proxy(
        &self,
        mode: ProxyMode,
        proxy: Option<&str>,
    ) -> anyhow::Result<ProxyStatus> {
        let status = self.http.configure_proxy(mode, proxy)?;
        // A proxy switch invalidates any in-flight login preview tokens. This
        // prevents a session created under one network path from being reused
        // after the request routing has changed.
        self.login_sessions.write().await.clear();
        Ok(status)
    }

    pub async fn set_source_cookie(&self, user_ns: &str, source_url: &str, cookie: &str) -> Result<(), AppError> {
        let context = self.http.source_context(user_ns, source_url, None).map_err(AppError::Internal)?;
        let url = url::Url::parse(source_url).map_err(|_| AppError::BadRequest("书源地址无效".to_string()))?;
        context.session.cookies.replace(&url, cookie).map_err(AppError::Internal)?;
        Ok(())
    }

    /// 登录页(`/__login/<s>/`)写入单条 cookie 到该书源的 jar。
    ///
    /// 与 `set_panel_cookie` 同源：登录页 iframe 跑在 reader origin 下，
    /// `document.cookie` 与上游站点完全隔离，页面自己写的 token 只留在浏览器里，
    /// 后续转发请求带不上、页面自己读也读不到。这里把单条 `名字=值` 落到 jar。
    ///
    /// 与手动 `set_source_cookie` 的区别：那个用 `replace`（整批覆盖），这个是
    /// 逐条 `set`，因为登录页会分多次写不同 cookie（token、deviceId…）。
    /// 把**独立登录窗口**里读到的 Cookie 批量写进书源 jar。
    ///
    /// 与 `set_source_login_cookie` 的区别：
    ///   1. 一次写多条（窗口里登录后通常有好几个 Cookie）；
    ///   2. 域取自 Cookie 自身（`domain`），而不是书源地址 —— 独立窗口打开的是
    ///      上游真实站点，Cookie 的域可能与之不同（`www` vs 裸域、auth 子域）；
    ///   3. 书源地址本身可能**不是 URL**（一类聚合源用 jsLib 在运行时拼线路，
    ///      `bookSourceUrl` 只是一句标识而非网址），那种情况下 `url::Url::parse`
    ///      会失败，只能完全依赖 Cookie 的域。
    /// 保存独立窗口里读到的源码页 `localStorage`（按**上游 origin** 隔离）。
    ///
    /// 用途见 `webview_cookies::read_local_storage`：一类书源的段评页是 SPA，
    /// 鉴权除了 Cookie 里的 token，还要 `localStorage` 里的设备 id。面板挂在
    /// `reader.localhost` 下与上游 origin 隔离，不注入就只能现生成假 id。
    ///
    /// 键形如 `__source_storage__<origin>`，值是一行一个 `key\tvalue`——
    /// 用制表符而不是 JSON：值里可能有任意引号/换行，JSON 需要来回转义两次。
    pub async fn set_source_window_storage(&self, source_url: &str, entries: &[(String, String)]) {
        let Ok(url) = url::Url::parse(source_url.trim()) else {
            return;
        };
        let origin = url.origin().ascii_serialization();
        if origin == "null" || entries.is_empty() {
            return;
        }
        SOURCE_WINDOW_STORAGE
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(origin, entries.to_vec());
    }

/// 读出某个上游 origin 的源码页 `localStorage`（`set_source_window_storage` 的逆向）。
///
/// 面板渲染（`protocol::render_source_panel_page`）没有 `BookService` 实例，
/// 因此这份数据单独放在进程级表里，按 origin 存取。

/// 独立窗口收走的 localStorage，按**上游 origin** 存。
///
/// 为什么不用 `BookService` 的会话：面板渲染是 `protocol` 层的纯函数调用，
/// 拿不到 service；而这份数据的生命周期正好是「窗口关闭 → 面板渲染」，
/// 进程级表足够，也不需要落盘（值本身来自浏览器，重启后窗口会重新写）。

    /// 读出某个书源 jar 里的**全部** Cookie（供注入验证窗口）。
    ///
    /// 与 [`Self::set_source_window_cookies`] 方向相反：那个是「窗口 → jar」，
    /// 这个是「jar → 窗口」。对应 legado `AppCookieManager.applyToWebView(url)`。
    ///
    /// 按书源地址本身取即可：`cookie_store` 的 `iter_any` 不做域名过滤，
    /// 拿到的就是这个源自己的会话（每个源一个独立 jar，不会串台）。
    pub async fn source_session_cookies(
        &self,
        source_url: &str,
    ) -> Result<Vec<crate::service::webview_cookies::CookieRecord>, String> {
        let context = self
            .http
            .source_context("default", source_url, None)
            .map_err(|e| e.to_string())?;
        Ok(context.session.cookies.snapshot())
    }

    pub async fn set_source_window_cookies(
        &self,
        source_url: &str,
        cookies: &[crate::service::webview_cookies::CookieRecord],
    ) -> Result<usize, String> {
        let context = self
            .http
            .source_context("default", source_url, None)
            .map_err(|e| e.to_string())?;
        // 写入用的请求地址：作用域由 cookie 自己的 Domain 属性决定，这个 URL
        // 只用于「host-only cookie 归属哪个 host」，所以取书源地址的 host。
        let anchor = url::Url::parse(source_url.trim())
            .or_else(|_| {
                url::Url::parse(&format!("https://{}", source_url.trim().trim_start_matches('/')))
            })
            .map_err(|e| format!("书源地址无效: {e}"))?;
        let mut written = 0;
        for cookie in cookies {
            if cookie.name.is_empty() || cookie.name.chars().any(char::is_control) {
                continue;
            }
            if cookie.value.chars().any(char::is_control) {
                continue;
            }
            // `Cookie.Domain` 有时带前导点（`.pili45.com`），那正是站点声明的作用域；
            // 原样交给 `set_detailed`，不能再像早先那样 `trim_start_matches('.')`
            // 后靠写入 URL 猜作用域 —— 那会把 `cf_clearance` 钉成 host-only。
            let domain = cookie.domain.trim().to_string();
            // 没有 Domain 属性的 host-only cookie：没有 host 就无从归属，跳过。
            if domain.is_empty() && cookie.path.is_empty() {
                continue;
            }
            let path = if cookie.path.starts_with('/') { cookie.path.clone() } else { "/".to_string() };
            // 空值代表站点删除了该 Cookie（退出登录）：从 jar 移除同名项，
            // 而不是写一个空值进去。
            if cookie.value.trim().is_empty() {
                let Ok(url) = url::Url::parse(&format!(
                    "https://{}{}",
                    domain.trim_start_matches('.').trim_start_matches('.'),
                    path
                )) else {
                    continue;
                };
                let _ = context.session.cookies.remove(&url, &cookie.name);
                continue;
            }
            if context
                .session
                .cookies
                .set_detailed(
                    &cookie.name,
                    &cookie.value,
                    &domain,
                    &path,
                    cookie.secure,
                    cookie.http_only,
                    &anchor,
                )
                .is_ok()
            {
                written += 1;
            }
        }
        Ok(written)
    }

    pub async fn set_source_login_cookie(
        &self,
        source_url: &str,
        cookie: &str,
    ) -> Result<(), String> {
        let head = cookie.split(';').next().unwrap_or("").trim();
        let Some((name, value)) = head.split_once('=') else {
            return Err("Cookie 格式无效".to_string());
        };
        let name = name.trim();
        if name.is_empty() {
            return Err("Cookie 格式无效".to_string());
        }
        if name.chars().any(char::is_control) || value.chars().any(char::is_control) {
            return Err("Cookie 含控制字符".to_string());
        }
        let context = self
            .http
            .source_context("default", source_url, None)
            .map_err(|e| e.to_string())?;
        // 目标域固定取书源地址: 登录页脚本不能挑域名写会话。
        let url = url::Url::parse(source_url).map_err(|e| e.to_string())?;
        // 空值代表页面删除该 cookie(退出登录), 必须从 jar 移除而非写成空值。
        if value.trim().is_empty() {
            return context
                .session
                .cookies
                .remove(&url, name)
                .map_err(|e| e.to_string());
        }
        context
            .session
            .cookies
            .set(&url, head)
            .map_err(|e| e.to_string())
    }

    pub async fn clear_source_cookie(&self, user_ns: &str, source_url: &str) {
        if let Ok(context) = self.http.source_context(user_ns, source_url, None) {
            let _ = context.session.cookies.clear();
            let _ = context.session.put("loginHeader", None);
        }
        let source_url = normalize_source_url(source_url);
        self.login_sessions.write().await.retain(|_, session| session.source_url != source_url);
    }

    /// 书源被删除/批量删除时，把这个书源的持久化会话目录也删掉。
    ///
    /// 与 `clear_source_cookie` 分开的原因：那个只清 cookie 内容，书源还在、
    /// 之后可能重新登录，目录留着是对的。而书源删掉之后，`source-state/<md5>`
    /// 里只剩没人会再读的空壳，必须连目录一起回收 —— 否则每次导入一批书源再
    /// 删掉，都会在磁盘上攒一层永远不用的目录（实测某数据目录堆了 907 个）。
    pub async fn remove_source_state(&self, user_ns: &str, source_url: &str) {
        // 目录名与 `SourceSession::new` 必须完全一致：`md5("{ns}::{normalized}")`。
        // 两边任一处改名都会让这里静默失效，所以两边都依赖 `normalize_source_url`。
        let key = format!("{user_ns}::{}", normalize_source_url(source_url));
        let dir = self.storage_dir.join("source-state").join(md5_hex(&key));
        if dir.exists() {
            match tokio::fs::remove_dir_all(&dir).await {
                Ok(()) => {}
                Err(err) => tracing::warn!(
                    dir = %dir.display(),
                    "删除书源会话目录失败: {err}"
                ),
            }
        }
    }

    pub async fn clear_source_cookies(&self, user_ns: &str, source_urls: &[String]) {
        for url in source_urls { self.clear_source_cookie(user_ns, url).await; }
    }

    /// 批量删除书源的持久化会话目录（书源被删掉后调用）。
    pub async fn remove_source_states(&self, user_ns: &str, source_urls: &[String]) {
        for url in source_urls { self.remove_source_state(user_ns, url).await; }
    }

    /// 回收**空壳**的书源会话目录（`source-state/<md5>`）。
    ///
    /// 正常删除书源现在会连目录一起删（见 `remove_source_state`），但历史版本
    /// 只清 cookie 内容不删目录，于是留下大量只有 `{}` 的空壳 —— 实测某数据
    /// 目录 907 个里 870 个是空的（实际书源只有几十个）。
    ///
    /// 只删**内容确实为空**且**闲置够久**的：有 cookie 的目录是用户登录态，
    /// 哪怕对应书源眼下不在库里也要留着（可能只是暂时停用或稍后重新导入）。
    pub async fn clean_empty_source_state_dirs(&self, max_age: Duration) -> usize {
        let root = self.storage_dir.join("source-state");
        let mut entries = match tokio::fs::read_dir(&root).await {
            Ok(entries) => entries,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return 0,
            Err(err) => {
                tracing::warn!(dir = %root.display(), "读取书源会话目录失败: {err}");
                return 0;
            }
        };
        let now = std::time::SystemTime::now();
        let mut removed = 0usize;
        while let Ok(Some(entry)) = entries.next_entry().await {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            let modified = entry
                .metadata()
                .await
                .ok()
                .and_then(|meta| meta.modified().ok())
                .unwrap_or(now);
            if now.duration_since(modified).unwrap_or_default() < max_age {
                continue;
            }
            if !source_state_dir_is_empty(&path).await {
                continue;
            }
            match tokio::fs::remove_dir_all(&path).await {
                Ok(()) => removed += 1,
                Err(err) => tracing::warn!(
                    dir = %path.display(),
                    "删除空壳书源会话目录失败: {err}"
                ),
            }
        }
        removed
    }

    /// Validate in an isolated session. A search without matches cannot tell
    /// whether the candidate cookie is valid; only an explicit login check can.
    pub async fn validate_source_cookie(
        &self, user_ns: &str, source: &BookSource, cookie: &str, keyword: &str,
    ) -> Result<bool, AppError> {
        let candidate_http = HttpClient::new(30, self.http.active_proxy()).map_err(AppError::Internal)?;
        // `user_ns` 以前是 `cookie-check-{uuid}`，而 `SourceSession::new` 会按
        // `md5(user_ns::source_url)` 建目录 —— 每验证一次 cookie 就永久留下一个
        // 空目录（实测某数据目录里堆了 907 个，实际书源只有几十个）。
        // 这里用固定命名空间：同一书源反复验证复用同一个目录，量级降到与书源数同阶。
        let context = candidate_http.source_context("cookie-check", &source.book_source_url, None).map_err(AppError::Internal)?;
        let url = url::Url::parse(&source.book_source_url).map_err(|_| AppError::BadRequest("书源地址无效".into()))?;
        context.session.cookies.set(&url, cookie).map_err(AppError::Internal)?;
        let current = self.http.source_context(user_ns, &source.book_source_url, None).map_err(AppError::Internal)?;
        for key in ["variable", "loginInfo"] {
            if let Some(value) = current.session.get(key) { context.session.put(key, Some(&value)).map_err(AppError::Internal)?; }
        }
        let Some(rule) = source.search_url.as_ref().filter(|v| !v.trim().is_empty()).cloned() else { return Ok(false) };
        let source_copy = source.clone();
        let keyword = keyword.to_string();
        let analyze_context = context.clone();
        let mut spec = tokio::task::spawn_blocking(move || crate::parser::js::with_http_context(analyze_context, || {
            analyze_url(&rule, &keyword, 1, &source_copy.book_source_url, &source_copy)
        })).await.map_err(|err| AppError::Internal(err.into()))??;
        spec.headers.retain(|(name, _)| !name.eq_ignore_ascii_case("cookie"));
        spec.headers.push(("Cookie".into(), cookie.trim().to_string()));
        let client = if let Some(proxy) = spec.proxy.as_deref() {
            crate::crawler::http_client::build_source_client(context.timeout_secs, Some(proxy), context.session.cookies.clone()).map_err(AppError::Internal)?
        } else { context.client.clone() };
        let holds_slot = self.wait_for_rate(source).await;
        let result = fetch_with_client(&client, spec).await;
        if holds_slot { self.finish_rate(source).await; }
        let res = result.map_err(AppError::Internal)?;
        if !res.is_successful || res.status == 202 || res.body.contains("var buid") {
            return Err(AppError::BadRequest(format!("站点未接受验证请求（HTTP {}），请检查网页登录状态", res.status)));
        }
        if source.login_check_js.as_deref().unwrap_or("").trim().is_empty() { return Ok(false); }
        let source_copy = source.clone();
        tokio::task::spawn_blocking(move || crate::parser::js::with_http_context(context, || apply_login_check_js(&source_copy, res)))
            .await.map_err(|err| AppError::Internal(err.into()))??;
        Ok(true)
    }

    async fn fetch_source_url(
        &self,
        user_ns: &str,
        source: &BookSource,
        url_rule: &str,
        base_url: &str,
        key: &str,
    ) -> Result<FetchResponse, AppError> {
        let spec = self.analyze_request(user_ns, source, url_rule, key, 1, base_url).await?;
        let res = self.fetch_with_rate(user_ns, source, spec).await?;
        self.check_source_response(user_ns, source, res).await
    }

    async fn fetch_with_rate(
        &self,
        user_ns: &str,
        source: &BookSource,
        spec: RequestSpec,
    ) -> anyhow::Result<FetchResponse> {
        self.fetch_with_rate_cancellable(user_ns, source, spec, None)
            .await
    }

    /// 与 `fetch_with_rate` 相同, 但可被取消信号中断在途请求。
    ///
    /// 关键点: 用 `select!` 包住 `fetch_with_client` 的 future, 取消时该 future 被
    /// **drop** —— reqwest 的连接随之关闭, 这才是"真断", 而不是等它跑到 30s 超时。
    /// 取消返回 `Err(CANCELLED_ERR)`, 调用方据此把结果标为"已取消"而不是"失效",
    /// 否则把用户主动中止当成站点不可用, 会把好源误标失效。
    async fn fetch_with_rate_cancellable(
        &self,
        user_ns: &str,
        source: &BookSource,
        spec: RequestSpec,
        cancel: Option<&CancelSignal>,
    ) -> anyhow::Result<FetchResponse> {
        if is_cancelled(cancel) {
            return Err(anyhow::anyhow!(CANCELLED_ERR));
        }
        // 限速等待也必须可取消: 配了 `concurrentRate: 2000` 这类较大间隔时,
        // 傻等限速窗口会让"中止"延迟数秒才生效, 与 100ms 响应上限的意图不符。
        //
        // `wait_for_rate` 返回**是否占用了 rate slot**, 据此精确决定要不要归还 ——
        // 只对"自己占到的 slot"调 finish_rate, 避免把别的在途请求的 slot 误释放
        // (那会让限速形同虚设)。
        let owns_serial_slot;
        if let Some(flag) = cancel {
            let waited = self.wait_for_rate(source);
            tokio::pin!(waited);
            loop {
                tokio::select! {
                    acquired = &mut waited => {
                        owns_serial_slot = acquired;
                        break;
                    }
                    _ = sleep(Duration::from_millis(100)) => {
                        if flag.load(std::sync::atomic::Ordering::Relaxed) {
                            // 提前 drop: 未占用时不归还(别人的 slot 不能动)。
                            return Err(anyhow::anyhow!(CANCELLED_ERR));
                        }
                    }
                }
            }
        } else {
            owns_serial_slot = self.wait_for_rate(source).await;
        }
        if is_cancelled(cancel) {
            if owns_serial_slot {
                self.finish_rate(source).await;
            }
            return Err(anyhow::anyhow!(CANCELLED_ERR));
        }
        // JoinSet::abort_all can drop this future while it owns a serial
        // source slot. Return the slot from Drop as well, otherwise the source
        // remains in_flight forever and later catalogs wait indefinitely.
        let _rate_release = owns_serial_slot.then(|| RateSlotRelease {
            service: self.clone(),
            source: source.clone(),
        });
        let request_context = match self.http
            .source_context(user_ns, &source.book_source_url, spec.proxy.as_deref())
        {
            Ok(context) => context,
            Err(err) => {
                return Err(anyhow::anyhow!(err.to_string()));
            }
        };

        // 是否经代理发出: 直连重试只在这种情况下才有意义。
        let used_proxy = request_context.proxy.is_some();
        let result = match cancel {
            Some(flag) => {
                // 100ms 轮询而不是"监听一次": 取消标志是 AtomicBool, 没有可等待的
                // 唤醒机制, 用间隔轮询即可 —— 取消响应延迟上限就是这 100ms。
                let fetch = request_context.session.request(&request_context.client, spec.clone(), source.concurrent_rate.as_deref());
                tokio::pin!(fetch);
                loop {
                    tokio::select! {
                        result = &mut fetch => break result,
                        _ = sleep(Duration::from_millis(100)) => {
                            if flag.load(std::sync::atomic::Ordering::Relaxed) {
                                // fetch 在此处被 drop → 连接关闭。
                                return Err(anyhow::anyhow!(CANCELLED_ERR));
                            }
                        }
                    }
                }
            }
            None => request_context.session.request(&request_context.client, spec.clone(), source.concurrent_rate.as_deref()).await,
        };
        let result = match result {
            Ok(response) => Ok(self.retry_direct_when_proxy_blocked(user_ns, source, used_proxy, spec.clone(), response, cancel).await),
            Err(err) => Err(err),
        };
        result
    }

    /// 代理出口被站点 WAF 拒(403/407 等小体积拒绝页)时, 用直连客户端重试一次。
    ///
    /// 代理 IP 是共享的, 被整段拉黑的情况很常见: 同一个 URL 经代理是 403 拒绝页,
    /// 直连却是完整正文(某书源的探索页就是这样, 手机端因此能看、桌面端空白)。
    /// 只在「确实走了代理」且响应可疑时触发, 直连结果替换原响应交由调用方照常解析。
    async fn retry_direct_when_proxy_blocked(
        &self,
        user_ns: &str,
        source: &BookSource,
        used_proxy: bool,
        spec: RequestSpec,
        response: FetchResponse,
        cancel: Option<&CancelSignal>,
    ) -> FetchResponse {
        if !used_proxy || !response_looks_proxy_blocked(&response) {
            return response;
        }
        if is_cancelled(cancel) {
            return response;
        }
        let Ok(context) = self.http.source_context_direct(user_ns, &source.book_source_url) else {
            return response;
        };
        match context.session.request(&context.client, spec, source.concurrent_rate.as_deref()).await {
            Ok(direct) => {
                tracing::info!(
                    url = %direct.url,
                    status = direct.status,
                    "proxy response looked blocked; used direct retry"
                );
                direct
            }
            Err(err) => {
                tracing::warn!(url = %response.url, error = %err, "direct retry after proxy block failed");
                response
            }
        }
    }

    /// 等待限速窗口并**占用 rate slot**(返回时 `in_flight == true` 表示已占)。
    ///
    /// 返回 `true` 表示本次调用占用了 slot, 调用方结束请求时必须 `finish_rate` 归还;
    /// 返回 `false` 表示未占用(无需归还)。
    async fn wait_for_rate(&self, source: &BookSource) -> bool {
        let Some(rate) = source.concurrent_rate.as_deref().map(str::trim) else {
            return false;
        };
        if rate.is_empty() || rate == "0" {
            return false;
        }
        if let Some((limit, window_ms)) = parse_window_rate(rate) {
            self.wait_for_window_rate(&source.book_source_url, limit, window_ms)
                .await;
            return false;
        }
        let Ok(delay_ms) = rate.parse::<u64>() else {
            return false;
        };
        self.wait_for_serial_rate(&source.book_source_url, delay_ms)
            .await
    }

    async fn wait_for_serial_rate(&self, source_key: &str, delay_ms: u64) -> bool {
        let delay = Duration::from_millis(delay_ms);
        loop {
            let wait = {
                let mut states = self.rate_states.write().await;
                let state = states.entry(source_key.to_string()).or_default();
                let now = Instant::now();
                if state.in_flight {
                    delay
                } else if let Some(last_start) = state.last_start {
                    let elapsed = now.saturating_duration_since(last_start);
                    if elapsed < delay {
                        delay - elapsed
                    } else {
                        state.in_flight = true;
                        state.last_start = Some(now);
                        return true;
                    }
                } else {
                    state.in_flight = true;
                    state.last_start = Some(now);
                    return true;
                }
            };
            sleep(wait).await;
        }
    }

    async fn wait_for_window_rate(&self, source_key: &str, limit: usize, window_ms: u64) -> bool {
        if limit == 0 || window_ms == 0 {
            return false;
        }
        let window = Duration::from_millis(window_ms);
        loop {
            let wait = {
                let mut states = self.rate_states.write().await;
                let state = states.entry(source_key.to_string()).or_default();
                let now = Instant::now();
                state
                    .window_starts
                    .retain(|start| now.saturating_duration_since(*start) <= window);
                if state.window_starts.len() >= limit {
                    state
                        .window_starts
                        .first()
                        .map(|start| window.saturating_sub(now.saturating_duration_since(*start)))
                        .unwrap_or(window)
                } else {
                    state.window_starts.push(now);
                    return true;
                }
            };
            sleep(wait).await;
        }
    }

    async fn finish_rate(&self, source: &BookSource) {
        let mut states = self.rate_states.write().await;
        if let Some(state) = states.get_mut(&source.book_source_url) {
            state.in_flight = false;
            // 只在**没有任何残留状态**时回收条目。
            //
            // 串行限速(`concurrentRate: "1000"`)靠 `last_start` 计算下一次允许的
            // 起点, 而它从不写 `window_starts` —— 早先只判 `window_starts.is_empty()`
            // 就把条目删掉, `last_start` 随之丢失, 下一次 `wait_for_serial_rate`
            // 命中 `entry().or_default()` 直接放行, 配置的间隔实际为 0(限速形同虚设)。
            // 窗口模式则相反: 它只用 `window_starts`, `last_start` 恒为 None, 且
            // `wait_for_rate` 对它固定返回"未占用 slot", 因此 `finish_rate` 走不到
            // 这里 —— 那条件只为串行路径保留。
            if state.window_starts.is_empty() && state.last_start.is_none() {
                states.remove(&source.book_source_url);
            }
        }
    }

    pub async fn search_book(
        &self,
        user_ns: &str,
        source: &BookSource,
        key: &str,
        page: i32,
    ) -> Result<Vec<SearchBook>, AppError> {
        self.search_book_cancellable(user_ns, source, key, page, None)
            .await
    }

    /// 可取消的搜索。解析段放到阻塞线程池, 避免 `js:` 规则里的同步 HTTP 独占
    /// tokio worker(那会让取消命令自己排不上队, 表现为"点了没反应")。
    pub async fn search_book_cancellable(
        &self,
        user_ns: &str,
        source: &BookSource,
        key: &str,
        page: i32,
        cancel: Option<&CancelSignal>,
    ) -> Result<Vec<SearchBook>, AppError> {
        if is_cancelled(cancel) {
            return Err(AppError::BadRequest(CANCELLED_ERR.to_string()));
        }
        let search_url = source
            .search_url
            .clone()
            .ok_or_else(|| AppError::BadRequest("missing search_url".to_string()))?;
        tracing::info!(
            "searching book from {}: key={}, page={}, url={}",
            source.book_source_name,
            key,
            page,
            search_url
        );
        let spec = self.analyze_request(user_ns, source, &search_url, key, page, &source.book_source_url).await
            .map_err(|e| {
                tracing::error!("analyze_url failed: {:?}", e);
                e
            })?;


        tracing::debug!("search request prepared");
        let res = self
            .fetch_with_rate_cancellable(user_ns, source, spec, cancel)
            .await
            .map_err(|e| {
                tracing::error!("fetch failed: {:?}", e);
                e
            })?;
        let res = self.check_source_response(user_ns, source, res).await?;
        tracing::debug!("fetch success, body length: {}", res.body.len());
        let books = self
            .parse_search_books_blocking(user_ns, source, &res.body, &res.url, key, page)
            .await?;
        if books.is_empty() && !res.is_successful {
            return Err(AppError::BadRequest(format!(
                "书源搜索请求失败（HTTP {}），请检查站点访问状态或网络设置", res.status
            )));
        }
        tracing::info!("found {} books", books.len());
        Ok(books)
    }

    /// 搜索解析放到阻塞线程池: 规则里可能带 `js:` 且 JS 内会发起同步 HTTP
    /// (reqwest::blocking, 30s 超时), 留在 async worker 上会把它整个占住。
    ///
    /// **解析器 panic 必须继续向上传播**, 不能吞成空 `Vec`: 空结果会被上层当成
    /// "这个源搜不到书" → `search_ok=false` → `valid=false` → 书源被标为失效并写库,
    /// 把一个完全正常的源标坏。原实现里 `parser.search_books` 是同步调用, panic 会
    /// 直接终止该检测任务(不产生 outcome、不写库), 这里用 `resume_unwind` 保持同一
    /// 行为 —— `spawn_blocking` 只是把 panic 包成了 `JoinError`, 并非真的"没有结果"。
    async fn parse_search_books_blocking(&self, user_ns: &str, source: &BookSource, body: &str, url: &str, key: &str, page: i32) -> Result<Vec<SearchBook>, AppError> {
        self.parse_book_list_blocking(user_ns, source, body, url, false, key.to_string(), page).await
    }

    async fn parse_explore_books_blocking(&self, user_ns: &str, source: &BookSource, body: &str, url: &str, page: i32) -> Result<Vec<SearchBook>, AppError> {
        self.parse_book_list_blocking(user_ns, source, body, url, true, String::new(), page).await
    }

    async fn parse_book_list_blocking(&self, user_ns: &str, source: &BookSource, body: &str, url: &str, explore: bool, key: String, page: i32) -> Result<Vec<SearchBook>, AppError> {
        let context = self.http.source_context(user_ns, &source.book_source_url, None).map_err(AppError::Internal)?;
        let parser = self.parser.clone();
        let (source, body, url) = (source.clone(), body.to_string(), url.to_string());
        match tokio::task::spawn_blocking(move || crate::parser::js::with_http_context(context, || {
            take_js_logs();
            let books = crate::parser::js::with_search_context(&key, page, || if explore { parser.explore_books(&source, &body, &url) } else { parser.search_books(&source, &body, &url) });
            (books, take_js_logs())
        })).await {
            Ok((books, logs)) => {
                if books.is_empty() {
                    if let Some(error) = first_js_error(&logs) { return Err(AppError::BadRequest(format!("书源解析失败: {error}"))); }
                }
                Ok(books)
            }
            Err(err) if err.is_cancelled() => Ok(Vec::new()),
            Err(err) => std::panic::resume_unwind(err.into_panic()),
        }
    }

    pub async fn explore_book(
        &self,
        user_ns: &str,
        source: &BookSource,
        rule_find_url: &str,
        page: i32,
    ) -> Result<Vec<SearchBook>, AppError> {
        self.explore_book_cancellable(user_ns, source, rule_find_url, page, None)
            .await
    }

    /// 可取消的发现页搜索(同 `search_book_cancellable`)。
    pub async fn explore_book_cancellable(
        &self,
        user_ns: &str,
        source: &BookSource,
        rule_find_url: &str,
        page: i32,
        cancel: Option<&CancelSignal>,
    ) -> Result<Vec<SearchBook>, AppError> {
        if is_cancelled(cancel) {
            return Err(AppError::BadRequest(CANCELLED_ERR.to_string()));
        }
        if rule_find_url.trim().is_empty() {
            return Err(AppError::BadRequest("ruleFindUrl required".to_string()));
        }
        let spec = self.analyze_request(user_ns, source, rule_find_url, "", page, &source.book_source_url).await?;


        let res = self.fetch_with_rate_cancellable(user_ns, source, spec, cancel).await?;
        let res = self.check_source_response(user_ns, source, res).await?;
        self.parse_explore_books_blocking(user_ns, source, &res.body, &res.url, page).await
    }

    /// 求值书源的发现页分类（exploreUrl）。
    ///
    /// `info_map` 是发现页控件的当前取值：JS 型 exploreUrl 会读它来决定生成哪些
    /// 分类（如"发现页来源=源甲"），也会写回默认值。缺省用空 map。
    pub fn explore_kinds_with_info(
        &self,
        source: &BookSource,
        info_map: &HashMap<String, String>,
    ) -> Result<Vec<ExploreKind>, AppError> {
        self.source_scope("default", source, || {
            // 与 explore_action 一样先清空线程局部的副作用槽位: exploreUrl 的
            // JS 里也可能出现 java.refreshExplore() / java.open(...)，不清理会
            // 让它们残留到下一次调用（例如被无辜的 action 当成自己的意图）。
            crate::parser::js::take_login_effects();
            crate::parser::js::take_explore_intents();
            parse_explore_kinds(source, info_map)
        })?
    }

    pub fn explore_kinds(&self, source: &BookSource) -> Result<Vec<ExploreKind>, AppError> {
        self.explore_kinds_with_info(source, &HashMap::new())
    }

    /// 求值发现页控件的 `action`（按钮/下拉/输入框回调）。
    ///
    /// 书源脚本里的副作用无法在 WebView 侧产生，统一随返回值交给调用方：
    /// - `java.refreshExplore()` / `java.open` / `java.searchBook` → 意图标志
    /// - `java.startBrowser` / `java.showBrowser` → `show_browser` 面板请求
    /// - `java.toast` / `java.longToast` → `messages` 提示文案
    ///
    /// `show_browser` 必须在这里登记成可渲染的面板：某书源的「更新书源」「书源设置」
    /// 都靠它弹出 HTML，早先只回传意图而丢掉面板，按钮点了毫无反应。
    pub fn explore_action(
        &self,
        source: &BookSource,
        action: &str,
        info_map: &HashMap<String, String>,
    ) -> Result<ExploreActionOutcome, AppError> {
        let action = action.to_string();
        let source_key = source.book_source_url.clone();
        let info_map = info_map.clone();
        let source_for_panel = source.clone();
        self.source_scope("default", source, || {
            take_js_logs();
            crate::parser::js::take_login_effects();
            crate::parser::js::take_explore_intents();
            // 面板是"本次动作"的产物：先清掉上一轮残留，避免把旧面板当成本次结果。
            clear_show_browser_request();
            let value = crate::parser::js::eval_explore_action(&action, &source_key, &info_map);
            let effects = crate::parser::js::take_login_effects();
            let intents = crate::parser::js::take_explore_intents();
            let browser = take_show_browser_request();
            let logs = take_js_logs();

            let messages = logs
                .iter()
                .filter_map(|line| line.strip_prefix("[toast] "))
                .map(str::to_string)
                .collect::<Vec<_>>();

            let show_browser = match browser {
                Some(request) => {
                    let entry = crate::service::source_browser::source_panel_entry(
                        &source_for_panel.book_source_url,
                    )?;
                    Some(crate::service::source_browser::register_browser_request(
                        request, entry,
                    )?)
                }
                None => None,
            };

            match value {
                Ok(value) => Ok(ExploreActionOutcome {
                    value,
                    explore_refresh: effects.explore_refresh,
                    login_refresh: effects.refresh,
                    login: effects.login,
                    open_urls: effects
                        .values
                        .get("__open")
                        .and_then(|v| v.as_str())
                        .map(str::to_string),
                    intents: intents
                        .into_iter()
                        .map(|(kind, payload)| {
                            json!({
                                "kind": kind,
                                "payload": serde_json::from_str::<serde_json::Value>(&payload)
                                    .unwrap_or(serde_json::Value::String(payload)),
                            })
                        })
                        .collect(),
                    show_browser,
                    values: effects.values,
                    messages,
                    logs,
                }),
                Err(err) => Err(AppError::BadRequest(format!("书源动作执行失败: {err}"))),
            }
        })?
    }

    pub async fn test_book_source_availability(
        &self,
        user_ns: &str,
        source: &BookSource,
        keyword: Option<&str>,
    ) -> BookSourceAvailability {
        self.test_book_source_availability_cancellable(user_ns, source, keyword, None)
            .await
    }

    /// 可取消的书源可用性检测。
    ///
    /// 与不可取消版本的区别: 单次请求可被中断(连接真断), 且取消时返回的可用性对象
    /// 带 `cancelled = true`。调用方必须据此**跳过写库** —— 否则把"用户中止"当成
    /// "站点不可用", 会把好源误标成失效。
    pub async fn test_book_source_availability_cancellable(
        &self,
        user_ns: &str,
        source: &BookSource,
        keyword: Option<&str>,
        cancel: Option<&CancelSignal>,
    ) -> BookSourceAvailability {
        let keyword = keyword
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
            // 不给关键词时用源自己的 checkKeyWord, 否则回退默认值(见 check_keyword
            // 的注释: 默认值是"我的"而不是"测试", 避免把好源误判失效)。
            .unwrap_or_else(|| source.check_keyword());

        // 取消时立即返回一个"已取消"的占位结果: 不写库, 也不计入有效/失效统计。
        if is_cancelled(cancel) {
            return cancelled_availability(source, &keyword);
        }

        let (search_ok, search_error) = if source
            .search_url
            .as_deref()
            .is_some_and(|value| !value.trim().is_empty())
            && source.rule_search.is_some()
        {
            match self
                .search_book_cancellable(user_ns, source, &keyword, 1, cancel)
                .await
            {
                Ok(books) => (!books.is_empty(), None),
                Err(err) => (false, Some(format!("{err:?}"))),
            }
        } else {
            (false, Some("missing searchUrl or ruleSearch".to_string()))
        };
        // 搜索阶段被取消: 不再发发现页请求, 直接返回取消结果。
        if is_cancelled(cancel) {
            return cancelled_availability(source, &keyword);
        }

        let explore_url = self.explore_kinds(source).ok().and_then(|kinds| {
            kinds
                .into_iter()
                .filter_map(|kind| kind.url)
                .map(|url| url.trim().to_string())
                .find(|url| !url.is_empty())
        });
        let (explore_ok, explore_error) = if let Some(url) = explore_url.as_deref() {
            match self
                .explore_book_cancellable(user_ns, source, url, 1, cancel)
                .await
            {
                Ok(books) => (!books.is_empty(), None),
                Err(err) => (false, Some(format!("{err:?}"))),
            }
        } else {
            (false, Some("missing explore category url".to_string()))
        };
        if is_cancelled(cancel) {
            return cancelled_availability(source, &keyword);
        }

        BookSourceAvailability {
            book_source_url: source.book_source_url.clone(),
            book_source_name: source.book_source_name.clone(),
            valid: search_ok || explore_ok,
            search_ok,
            explore_ok,
            keyword,
            explore_url,
            search_error,
            explore_error,
            cancelled: false,
        }
    }

    /// Run a single book-source parsing step for the source debugger.
    ///
    /// Unlike the normal crawl path this always fetches fresh (no cache) and
    /// returns the request URL, the raw response body and the parsed result so
    /// the UI can show what the source actually produced.
    pub async fn debug_source_step(
        &self,
        user_ns: &str,
        source: &BookSource,
        step: &str,
        keyword: &str,
        book_url: &str,
        chapter_url: &str,
    ) -> Result<DebugTrace, AppError> {
        match step {
            "search" => {
                let search_url = source
                    .search_url
                    .clone()
                    .ok_or_else(|| AppError::BadRequest("missing searchUrl".to_string()))?;
                let kw = if keyword.trim().is_empty() {
                    source
                        .rule_search
                        .as_ref()
                        .and_then(|r| r.check_key_word.clone())
                        .unwrap_or_default()
                } else {
                    keyword.to_string()
                };
                if kw.trim().is_empty() {
                    return Err(AppError::BadRequest("请输入搜索关键词".to_string()));
                }
                let res = self
                    .fetch_source_url(user_ns, source, &search_url, &source.book_source_url, &kw)
                    .await?;
                let books = self.parse_search_books_blocking(user_ns, source, &res.body, &res.url, &kw, 1).await?;
                Ok(debug_trace_from(
                    &res,
                    serde_json::to_value(books).unwrap_or_default(),
                ))
            }
            "bookInfo" => {
                let url = if book_url.trim().is_empty() {
                    source
                        .rule_book_info
                        .as_ref()
                        .and_then(|r| r.toc_url.clone())
                        .unwrap_or_default()
                } else {
                    book_url.to_string()
                };
                if url.trim().is_empty() {
                    return Err(AppError::BadRequest("请输入书籍链接".to_string()));
                }
                let res = self
                    .fetch_source_url(user_ns, source, &url, &source.book_source_url, "")
                    .await?;
                let info = self.parse_info_page(user_ns, source, &res, &url).await?;
                Ok(debug_trace_from(
                    &res,
                    serde_json::to_value(info).unwrap_or_default(),
                ))
            }
            "toc" => {
                let toc_url = book_url.trim();
                if toc_url.is_empty() {
                    return Err(AppError::BadRequest("请输入目录链接".to_string()));
                }
                let res = self
                    .fetch_source_url(user_ns, source, toc_url, &source.book_source_url, "")
                    .await?;
                let (chapters, _next_urls) =
                    self.parse_toc_page(user_ns, source, &res, toc_url).await?;
                let mut index = 0i32;
                let chapters: Vec<BookChapter> = chapters
                    .into_iter()
                    .map(|mut ch| {
                        ch.index = index;
                        index += 1;
                        ch
                    })
                    .collect();
                Ok(debug_trace_from(
                    &res,
                    serde_json::to_value(chapters).unwrap_or_default(),
                ))
            }
            "content" => {
                let url = chapter_url.trim();
                if url.is_empty() {
                    return Err(AppError::BadRequest("请输入章节链接".to_string()));
                }
                let res = self
                    .fetch_source_url(user_ns, source, url, &source.book_source_url, "")
                    .await?;
                let (content, js_logs) =
                    self.parse_content_blocking(user_ns, source, &res, url, url, None).await?;
                let next = self.source_scope(user_ns, source, || self.parser.next_content_url(source, &res.body, &res.url))?;
                Ok(debug_trace_from(
                    &res,
                    serde_json::json!({
                        "content": content,
                        "nextContentUrl": next,
                        "jsLogs": js_logs,
                    }),
                ))
            }
            other => Err(AppError::BadRequest(format!("未知调试步骤: {other}"))),
        }
    }

    pub async fn login_book_source(&self, source: &BookSource) -> Result<serde_json::Value, AppError> {
        self.run_source_login(source, None, None).await
    }

    pub async fn run_source_login(&self, source: &BookSource, action: Option<String>, values: Option<serde_json::Map<String, serde_json::Value>>) -> Result<serde_json::Value, AppError> {
        self.run_source_login_with_browser(source, action, values, None).await
    }

    pub async fn run_source_login_with_browser(&self, source: &BookSource, action: Option<String>, values: Option<serde_json::Map<String, serde_json::Value>>, handler: Option<crate::service::source_browser::BrowserHandler>) -> Result<serde_json::Value, AppError> {
        self.run_source_task("default", source, move |source| {
            crate::service::source_browser::with_browser_handler(source, handler, || {
                crate::service::source_login::run(source, action.as_deref(), values)
            })?
        }).await?.map_err(AppError::Internal)
    }

    pub async fn get_book_info(
        &self,
        user_ns: &str,
        source: &BookSource,
        book_url: &str,
    ) -> Result<Book, AppError> {
        let res = self
            .fetch_source_url(user_ns, source, book_url, &source.book_source_url, "")
            .await?;
        self.parse_info_page(user_ns, source, &res, book_url).await
    }

    pub async fn get_chapter_list(
        &self,
        user_ns: &str,
        source: &BookSource,
        toc_url: &str,
    ) -> Result<Vec<BookChapter>, AppError> {
        self.get_chapter_list_with_cache(user_ns, source, toc_url, false)
            .await
    }

    pub async fn get_chapter_list_with_cache(
        &self,
        user_ns: &str,
        source: &BookSource,
        toc_url: &str,
        force_refresh: bool,
    ) -> Result<Vec<BookChapter>, AppError> {
        // Check cache first (unless force refresh)
        if !force_refresh {
            if let Ok(Some(cached)) = self.load_chapter_list_cache(user_ns, toc_url).await {
                if !cached.is_empty() {
                    return Ok(cached);
                }
            }
        }
        let (chapters, _) = self
            .get_chapter_list_with_pagination(user_ns, source, toc_url)
            .await?;
        // Save to cache
        let _ = self
            .save_chapter_list_cache(user_ns, toc_url, &chapters)
            .await;
        Ok(chapters)
    }

    /// Get first page of chapters and pagination info for background fetching
    pub async fn get_chapter_list_first_page(
        &self,
        user_ns: &str,
        source: &BookSource,
        toc_url: &str,
    ) -> Result<(Vec<BookChapter>, ChapterPagination), AppError> {
        let res = self
            .fetch_source_url(user_ns, source, toc_url, &source.book_source_url, "")
            .await?;
        let (chapters, next_urls) =
            self.parse_toc_page(user_ns, source, &res, toc_url).await?;
        tracing::info!(
            "get_chapter_list_first_page: toc_url={} final_url={} body_len={} chapters={} next_urls={}",
            toc_url,
            res.url,
            res.body.len(),
            chapters.len(),
            next_urls.len()
        );

        let mut chapter_index = 0i32;
        let mut result = Vec::new();
        for mut ch in chapters {
            ch.index = chapter_index;
            chapter_index += 1;
            result.push(ch);
        }

        // The actual URL we fetched (after redirects) should be considered visited
        let actual_visited_url = res.url.clone();

        // Get chapter URLs from first page for deduplication
        let first_page_chapter_urls: std::collections::HashSet<String> =
            result.iter().map(|c| c.url.clone()).collect();

        // Filter out already visited URLs and the current page URL from pending_urls
        // Also filter out URLs that point to the same page (same path but different domain)
        let pending_urls: Vec<String> = next_urls
            .into_iter()
            .filter(|u| {
                // Filter out exact matches
                if u == &actual_visited_url || u == toc_url {
                    return false;
                }
                // Filter out URLs with the same path but different domain
                // This handles cases like m.novelx.com vs m.novelx.net
                if let (Ok(parsed_u), Ok(parsed_visited)) =
                    (url::Url::parse(u), url::Url::parse(&actual_visited_url))
                {
                    if parsed_u.path() == parsed_visited.path() {
                        return false;
                    }
                }
                true
            })
            .collect();

        let pagination = ChapterPagination {
            user_ns: user_ns.to_string(),
            source: source.clone(),
            toc_url: toc_url.to_string(),
            visited_urls: vec![toc_url.to_string(), actual_visited_url],
            pending_urls,
            seen_chapter_urls: first_page_chapter_urls.iter().cloned().collect(),
            next_index: chapter_index,
        };

        Ok((result, pagination))
    }

    /// Continue fetching remaining chapters from pagination state
    pub async fn fetch_remaining_chapters(
        &self,
        pagination: ChapterPagination,
    ) -> Result<Vec<BookChapter>, AppError> {
        let mut all_chapters = Vec::new();
        let mut visited_page_urls: std::collections::HashSet<String> =
            pagination.visited_urls.iter().cloned().collect();
        let mut seen_chapter_urls: std::collections::HashSet<String> =
            pagination.seen_chapter_urls.iter().cloned().collect();
        let mut chapter_index = pagination.next_index;

        let pending_urls: Vec<String> = pagination
            .pending_urls
            .into_iter()
            .filter(|u| !visited_page_urls.contains(u))
            .collect();

        if pending_urls.len() > 1 {
            // Multiple URLs from option dropdown - fetch all pages
            for url in pending_urls {
                if visited_page_urls.contains(&url) {
                    continue;
                }
                visited_page_urls.insert(url.clone());

                let res = self
                    .fetch_source_url(
                        &pagination.user_ns,
                        &pagination.source,
                        &url,
                        &pagination.source.book_source_url,
                        "",
                    )
                    .await?;
                let (chapters, _) = self.parse_toc_page(&pagination.user_ns, &pagination.source, &res, &pagination.toc_url).await?;

                // Check if this page is a duplicate (all chapters already seen)
                // This handles cases where the first page URL differs from toc_url (e.g., different domain)
                let all_seen = chapters
                    .iter()
                    .all(|ch| seen_chapter_urls.contains(&ch.url));
                if all_seen && !chapters.is_empty() {
                    tracing::debug!("Skipping duplicate page: {}", url);
                    continue;
                }

                for ch in chapters {
                    if seen_chapter_urls.contains(&ch.url) {
                        continue;
                    }
                    seen_chapter_urls.insert(ch.url.clone());

                    all_chapters.push(BookChapter {
                        title: ch.title,
                        url: ch.url,
                        index: chapter_index,
                        ..Default::default()
                    });
                    chapter_index += 1;
                }
            }
        } else if pending_urls.len() == 1 {
            // Single next page link - follow sequentially
            let mut current_url = pending_urls[0].clone();
            loop {
                if visited_page_urls.contains(&current_url) {
                    break;
                }
                visited_page_urls.insert(current_url.clone());

                let res = self
                    .fetch_source_url(
                        &pagination.user_ns,
                        &pagination.source,
                        &current_url,
                        &pagination.source.book_source_url,
                        "",
                    )
                    .await?;
                let (chapters, next_urls) = self.parse_toc_page(&pagination.user_ns, &pagination.source, &res, &pagination.toc_url).await?;

                // Check if this page is a duplicate
                let all_seen = chapters
                    .iter()
                    .all(|ch| seen_chapter_urls.contains(&ch.url));
                if all_seen && !chapters.is_empty() {
                    tracing::debug!("Skipping duplicate page: {}", current_url);
                    break; // Stop following pagination if we hit a duplicate page
                }

                for ch in chapters {
                    if seen_chapter_urls.contains(&ch.url) {
                        continue;
                    }
                    seen_chapter_urls.insert(ch.url.clone());

                    all_chapters.push(BookChapter {
                        title: ch.title,
                        url: ch.url,
                        index: chapter_index,
                        ..Default::default()
                    });
                    chapter_index += 1;
                }

                // Get next page
                let next = next_urls
                    .into_iter()
                    .find(|u| !visited_page_urls.contains(u));
                match next {
                    Some(url) if !url.is_empty() => current_url = url,
                    _ => break,
                }
            }
        }

        Ok(all_chapters)
    }

    async fn get_chapter_list_with_pagination(
        &self,
        user_ns: &str,
        source: &BookSource,
        toc_url: &str,
    ) -> Result<(Vec<BookChapter>, Vec<String>), AppError> {
        let mut all_chapters = Vec::new();
        let mut visited_page_urls = std::collections::HashSet::new();
        let mut seen_chapter_urls = std::collections::HashSet::new();
        let mut chapter_index = 0i32;

        // Fetch first page
        let res = self
            .fetch_source_url(user_ns, source, toc_url, &source.book_source_url, "")
            .await?;
        let (chapters, next_urls) =
            self.parse_toc_page(user_ns, source, &res, toc_url).await?;

        visited_page_urls.insert(toc_url.to_string());

        // Add first page chapters with deduplication
        for ch in chapters {
            if seen_chapter_urls.contains(&ch.url) {
                continue;
            }
            seen_chapter_urls.insert(ch.url.clone());
            all_chapters.push(BookChapter {
                title: ch.title,
                url: ch.url,
                index: chapter_index,
                ..Default::default()
            });
            chapter_index += 1;
        }

        // Determine how to handle pagination
        // Filter out already visited URLs
        let pending_urls: Vec<String> = next_urls
            .into_iter()
            .filter(|u| !visited_page_urls.contains(u))
            .collect();

        if pending_urls.len() > 1 {
            // Multiple URLs from option dropdown - fetch all pages
            for url in pending_urls {
                if visited_page_urls.contains(&url) {
                    continue;
                }
                visited_page_urls.insert(url.clone());

                let res = self
                    .fetch_source_url(user_ns, source, &url, &source.book_source_url, "")
                    .await?;
                let (chapters, _) = self.parse_toc_page(user_ns, source, &res, toc_url).await?;

                for ch in chapters {
                    if seen_chapter_urls.contains(&ch.url) {
                        continue;
                    }
                    seen_chapter_urls.insert(ch.url.clone());
                    all_chapters.push(BookChapter {
                        title: ch.title,
                        url: ch.url,
                        index: chapter_index,
                        ..Default::default()
                    });
                    chapter_index += 1;
                }
            }
        } else if pending_urls.len() == 1 {
            // Single next page link - follow sequentially
            let mut current_url = pending_urls[0].clone();
            loop {
                if visited_page_urls.contains(&current_url) {
                    break;
                }
                visited_page_urls.insert(current_url.clone());

                let res = self
                    .fetch_source_url(user_ns, source, &current_url, &source.book_source_url, "")
                    .await?;
                let (chapters, next_urls) = self.parse_toc_page(user_ns, source, &res, toc_url).await?;

                for ch in chapters {
                    if seen_chapter_urls.contains(&ch.url) {
                        continue;
                    }
                    seen_chapter_urls.insert(ch.url.clone());
                    all_chapters.push(BookChapter {
                        title: ch.title,
                        url: ch.url,
                        index: chapter_index,
                        ..Default::default()
                    });
                    chapter_index += 1;
                }

                // Get next page
                let next = next_urls
                    .into_iter()
                    .find(|u| !visited_page_urls.contains(u));
                match next {
                    Some(url) if !url.is_empty() => current_url = url,
                    _ => break,
                }
            }
        }

        Ok((all_chapters, visited_page_urls.into_iter().collect()))
    }

    /// 正文 JS 的 `book` / `chapter` 上下文: 取书架书籍与缓存目录中的对应章节。
    async fn js_book_context(
        &self,
        user_ns: &str,
        book_url: &str,
        chapter_url: &str,
    ) -> Option<JsBookContext> {
        let book = self.read_bookshelf(user_ns).await.ok().and_then(|books| {
            books.into_iter().find(|book| {
                book.book_url == book_url || book.toc_url.as_deref() == Some(book_url)
            })
        });
        let toc_url = book
            .as_ref()
            .and_then(|b| b.toc_url.clone())
            .filter(|u| !u.is_empty())
            .unwrap_or_else(|| book_url.to_string());
        let chapter = if chapter_url.is_empty() {
            None
        } else {
            self.load_chapter_list_cache(user_ns, &toc_url)
                .await
                .ok()
                .flatten()
                .and_then(|list| list.into_iter().find(|ch| ch.url == chapter_url))
        };
        let book_json = match book {
            Some(book) => serde_json::to_value(book).unwrap_or_else(|_| json!({ "bookUrl": book_url })),
            None => json!({ "bookUrl": book_url }),
        };
        let chapter_json = match chapter {
            Some(ch) => serde_json::to_value(ch).unwrap_or_else(|_| json!({})),
            None if chapter_url.is_empty() => json!({}),
            None => json!({ "url": chapter_url }),
        };
        Some(JsBookContext {
            book: book_json,
            chapter: chapter_json,
        })
    }

    /// 正文图片 `click` 脚本(legado `ReadBookActivity.clickImg`): 在书源 jsLib 作用域里执行,
    /// 因此插件定义的 `showCommentPanel(...)` 等函数可用。
    /// 返回 (脚本返回值, 本次产生的 JS 日志, 面板信息)。
    pub async fn eval_source_click(
        &self,
        user_ns: &str,
        book_url: &str,
        source: &BookSource,
        chapter_url: &str,
        script: &str,
        result: &str,
    ) -> Result<(String, Vec<String>, Option<ShowBrowserRequest>), AppError> {
        let context = self.http.source_context(user_ns, &source.book_source_url, None).map_err(AppError::Internal)?;
        let book_ctx = self.js_book_context(user_ns, book_url, chapter_url).await;
        // 面板登记需要原始 JSON(面板脚本 `writeConfig(chapter, ...)` 依赖这两个绑定)
        let (book_json, chapter_json) = book_ctx
            .as_ref()
            .map(|ctx| (ctx.book.to_string(), ctx.chapter.to_string()))
            .unwrap_or_else(|| ("{}".to_string(), "{}".to_string()));
        let source = source.clone();
        let script = script.to_string();
        let result_text = result.to_string();
        let source_key = book_url.to_string();
        let chapter_url = chapter_url.to_string();

        tokio::task::spawn_blocking(move || {
            take_js_logs();
            clear_show_browser_request();
            let base_url = chapter_url.clone();
            let output = crate::parser::js::with_http_context(context, || with_book_context(book_ctx, || {
                with_source(&source, || {
                    with_book_url(Some(&source_key), || {
                        crate::parser::js::eval_js(&script, &result_text, &base_url)
                    })
                })
            }));
            let logs = take_js_logs();
            // 面板 HTML 由后端托管: 前端按 panelId 加载, 面板内的桥才能同源回调宿主
            let browser = take_show_browser_request().map(|req| {
                // 独立窗口请求不需要面板条目（也不该被清掉 html/换 panelId），
                // 原样交给前端 `openExternalWindow` 即可。
                if req.external_window {
                    return Ok(req);
                }
                crate::service::source_browser::register_browser_request(req, SourcePanelEntry {
                    book_source_url: source.book_source_url.clone(),
                    book_url: source_key.clone(),
                    chapter_url: base_url.clone(),
                    book_json: book_json.clone(),
                    chapter_json: chapter_json.clone(),
                    http: crate::parser::js::source_http_context(&source.book_source_url).ok(),
                    ..Default::default()
                })
            }).transpose().map_err(AppError::Internal)?;
            output
                .map(|value| (value, logs, browser))
                .map_err(|e| AppError::Internal(anyhow::anyhow!("执行 click 脚本失败: {e}")))
        })
        .await
        .map_err(|e| AppError::Internal(anyhow::anyhow!("click 脚本任务失败: {e}")))?
    }

    /// 正文规则可能在 JS 里发起同步网络请求, 放到阻塞线程池执行, 不占用异步 worker。
    /// 返回 (正文, 本次解析产生的 JS 日志)。
    async fn parse_content_blocking(
        &self,
        user_ns: &str,
        source: &BookSource,
        res: &FetchResponse,
        base_url: &str,
        source_key: &str,
        book_ctx: Option<JsBookContext>,
    ) -> Result<(String, Vec<String>), AppError> {
        let context = self.http.source_context(user_ns, &source.book_source_url, None).map_err(AppError::Internal)?;
        let parser = self.parser.clone();
        let source = source.clone();
        let body = res.body.clone();
        // legado 里 `baseUrl` 是**章节的绝对 URL**(含 `#item_id` 这类片段), 请求
        // 之后的真实地址是 `redirectUrl`。把 `res.url` 当 baseUrl 会丢掉片段,
        // 纯 JS 正文规则(如用 `baseUrl.match(/#(\d+)$/)` 取章节标识)因此拿不到
        // 章节标识。这里优先用调用方给的章节 URL。
        let url = if base_url.trim().is_empty() { res.url.clone() } else { base_url.to_string() };
        let source_key = source_key.to_string();
        tokio::task::spawn_blocking(move || {
            take_js_logs();
            let content = crate::parser::js::with_http_context(context, || with_book_context(book_ctx, || {
                parser.content(&source, &body, &url, Some(&source_key))
            }));
            let logs = take_js_logs();
            if content.is_empty() {
                // 优先展示书源脚本自己的提示(如"本书源为VIP专属内容，开通VIP后可阅读"),
                // 它比"正文解析为空"更能告诉用户发生了什么。
                if let Some(message) = first_js_message(&logs) {
                    return Err(AppError::BadRequest(message));
                }
                if let Some(error) = first_js_error(&logs) {
                    return Err(AppError::BadRequest(format!("正文解析失败: {error}")));
                }
                return Err(AppError::BadRequest("书源正文解析为空，请检查正文规则".to_string()));
            }
            Ok((content, logs))
        })
        .await
        .map_err(|e| AppError::Internal(anyhow::anyhow!("正文解析任务失败: {e}")))?
    }

    pub async fn get_content(
        &self,
        user_ns: &str,
        book_url: &str,
        source: &BookSource,
        chapter_url: &str,
    ) -> Result<String, AppError> {
        self.get_content_with_stage(user_ns, book_url, source, chapter_url, None)
            .await
    }

    /// 与 `get_content` 相同, 但把加载阶段回调出去, 供前端显示"卡在哪一步"。
    ///
    /// 读正文统一走"本地优先": 缓存里有内容就直接返回(见下方缓存命中分支), 只有
    /// 未缓存时才联网抓取并写回缓存 —— 与 legado 的 `BookHelp.getContent` 一致。
    ///
    /// 想让某一章重新抓取, 由调用方在进来之前删掉它的缓存(见 `cache_chapter` 的
    /// `refresh` 与 `get_book_content`)。
    pub async fn get_content_with_stage(
        &self,
        user_ns: &str,
        book_url: &str,
        source: &BookSource,
        chapter_url: &str,
        on_stage: Option<&(dyn Fn(ContentLoadStage) + Send + Sync)>,
    ) -> Result<String, AppError> {
        let notify = |stage: ContentLoadStage| {
            if let Some(callback) = on_stage {
                callback(stage);
            }
        };
        let book_key = md5_hex(book_url);
        tracing::debug!(
            "get_content called, chapter_url={}, book_key={}",
            chapter_url,
            book_key
        );
        // 本地已有正文就直接返回, 与 legado 的 `BookHelp.getContent` 一致: 缓存命中即
        // 意味着这一章读过了, 不必再发一次请求等网络往返。联网只发生在未缓存、或用户
        // 显式刷新(`refresh`)时 —— 后者会先删缓存再走到这里。
        let cached = self
            .cache
            .get(user_ns, &book_key, chapter_url)
            .await
            .ok()
            .flatten()
            .filter(|hit| !hit.trim().is_empty());
        if let Some(hit) = cached {
            tracing::debug!("get_content returning cached content, len={}", hit.len());
            return Ok(strip_leading_content_title(&hit));
        }
        tracing::debug!("get_content cache miss, fetching from network");

        let mut all_content = String::new();
        let mut visited_urls = std::collections::HashSet::new();
        let mut current_url = chapter_url.to_string();
        let book_ctx = self.js_book_context(user_ns, book_url, chapter_url).await;

        // Follow pagination to get all content pages
        loop {
            if visited_urls.contains(&current_url) {
                tracing::debug!("get_content detected loop, breaking");
                break;
            }
            visited_urls.insert(current_url.clone());

            notify(ContentLoadStage::Fetching);
            tracing::debug!("get_content fetching: {}", current_url);
            let res = match self
                .fetch_source_url(user_ns, source, &current_url, &source.book_source_url, "")
                .await
            {
                Ok(res) => res,
                // 走到这里说明本地没有这一章的正文(有的话上面已经直接返回了),
                // 请求失败就是失败, 没有可兜底的内容。
                Err(e) => return Err(e),
            };
            tracing::debug!("get_content fetch done, body len={}", res.body.len());
            // 正文规则里往往会触发书源脚本与段评请求, 这是最耗时的一步
            notify(ContentLoadStage::Parsing);
            let (content, _logs) = self
                .parse_content_blocking(user_ns, source, &res, &current_url, book_url, book_ctx.clone())
                .await?;
            tracing::debug!("get_content parsed content len={}", content.len());

            if !content.is_empty() {
                if !all_content.is_empty() {
                    all_content.push('\n');
                }
                all_content.push_str(&content);
            }

            // Check for next page
            if let Some(next_url) = self.source_scope(user_ns, source, || self.parser.next_content_url(source, &res.body, &res.url))? {
                tracing::debug!("get_content found next_url: {}", next_url);
                if should_follow_content_page(chapter_url, &current_url, &next_url) {
                    current_url = next_url;
                } else {
                    tracing::debug!("get_content next_url appears to be next chapter, stopping");
                    break;
                }
            } else {
                tracing::debug!("get_content no more pages");
                break;
            }
        }

        tracing::debug!("get_content final content len={}", all_content.len());
        // 阅读页会统一渲染章节标题；书源正文若自带开头标题会重复显示。
        // 与 Legado 的“去除重复标题”对应，只剥离开头第一个标题块。
        all_content = strip_leading_content_title(&all_content);
        if !all_content.is_empty() {
            notify(ContentLoadStage::Caching);
            let _ = self
                .cache
                .put(user_ns, &book_key, chapter_url, &all_content)
                .await;
        }
        Ok(all_content)
    }

    /// Delete all chapter content cache for a book
    pub async fn delete_book_cache(&self, user_ns: &str, book_url: &str) -> Result<bool, AppError> {
        let book_key = md5_hex(book_url);
        self.cache
            .remove_book(user_ns, &book_key)
            .await
            .map_err(|e| AppError::Internal(e.into()))
    }

    /// Check if a specific chapter is cached
    pub async fn is_chapter_cached(
        &self,
        user_ns: &str,
        book_url: &str,
        chapter_url: &str,
    ) -> bool {
        let book_key = md5_hex(book_url);
        self.cache.exists(user_ns, &book_key, chapter_url).await
    }

    /// 删除**单章**正文缓存, 其余章节不受影响。
    ///
    /// 阅读页的"刷新本章"依赖它: 缓存命中即返回之后, 想让这一章重新抓取就只能
    /// 先删掉它的缓存文件。
    pub async fn delete_chapter_cache(
        &self,
        user_ns: &str,
        book_url: &str,
        chapter_url: &str,
    ) -> Result<(), AppError> {
        let book_key = md5_hex(book_url);
        self.cache
            .remove(user_ns, &book_key, chapter_url)
            .await
            .map_err(|e| AppError::Internal(e.into()))
    }

    /// 回收**不在书架上**的书的章节正文缓存。
    ///
    /// 为什么需要它：`delete_book` / `delete_books` 只在书**还在书架里**时才
    /// 匹配得到，并据此调 `clear_book_related_cache`。而从搜索结果/发现页直接
    /// 点开阅读、又没有「加入书架」的书，其章节缓存写在
    /// `cache/<ns>/<md5(bookUrl)>/` 下，之后就再没有任何路径会删除它 ——
    /// 实测某开发数据目录里 69 份章节缓存有 62 份属于这种孤儿，占了绝大部分
    /// 体积。与封面缓存不同，章节正文**没有任何自动淘汰**（`FileCache` 里没有
    /// TTL，也没有全局容量上限），不主动回收就会一直涨。
    ///
    /// 判定规则：
    /// - 书架上的书（任意命名空间里读到的）一律豁免，无论多旧；
    /// - 其余目录只有**闲置超过 `max_age`** 才删。刚读完但还没决定加不加书架
    ///   的书不会在下次启动时被清掉，符合「读完一本书、隔一会儿再回来看」的
    ///   常见用法。
    ///
    /// 与 `clean_stale_search_covers` 一样是崩溃/强退的兜底：正常的删除书籍
    /// 路径已经会清缓存，这里只负责收那些没被任何人认领的目录。
    pub async fn clean_orphan_chapter_caches(&self, max_age: Duration) -> Result<usize, AppError> {
        // 读书架失败时**整体跳过**：宁可多留，也不能因为读到空列表就误删全部缓存。
        let Ok(books) = self.read_bookshelf("default").await else {
            return Ok(0);
        };
        let mut pinned: HashSet<String> = HashSet::new();
        // 目录缓存（`data/<ns>/chapters/<md5(tocUrl)>.vN.json`）用同一批书架书
        // 反查：它按 tocUrl 命名，与正文缓存按 bookUrl 命名不是一套键。
        let mut pinned_toc: HashSet<String> = HashSet::new();
        for book in &books {
            if !book.book_url.is_empty() {
                pinned.insert(md5_hex(&book.book_url));
            }
            if let Some(toc_url) = book.toc_url.as_deref().filter(|u| !u.is_empty()) {
                let name = md5_hex(toc_url);
                // 两代修订都要钉住，否则 v2/v3 里没被认出的那一份会被当成孤儿。
                pinned_toc.insert(format!("{name}.v2.json"));
                pinned_toc.insert(format!("{name}.v3.json"));
            }
        }

        let dirs = self
            .cache
            .list_book_dirs("default")
            .await
            .map_err(|e| AppError::Internal(e.into()))?;

        let now = std::time::SystemTime::now();
        let mut removed = 0usize;
        for (book_key, modified) in dirs {
            if pinned.contains(&book_key) {
                continue;
            }
            // 刚写过的目录不动：用户可能刚读完、正要加书架。
            let idle = now.duration_since(modified).unwrap_or_default();
            if idle < max_age {
                continue;
            }
            match self.cache.remove_book("default", &book_key).await {
                Ok(true) => removed += 1,
                Ok(false) => {}
                Err(err) => tracing::warn!(
                    book_key = %book_key,
                    "回收孤儿章节缓存失败: {err}"
                ),
            }
        }

        removed += self
            .remove_orphan_chapter_lists(&pinned_toc, max_age)
            .await;
        Ok(removed)
    }

    /// 回收不在书架上的书的**目录缓存**（`data/<ns>/chapters/*.json`）。
    ///
    /// 与正文缓存同样的成因：`clear_book_related_cache` 只在删书时触发，而从
    /// 搜索/发现页点开读过、没加书架的书，其 toc 缓存没有任何删除事件。
    /// 这类文件往往几百 KB 到 1 MB（整份章节目录），实测比正文缓存还占地方。
    async fn remove_orphan_chapter_lists(
        &self,
        pinned: &HashSet<String>,
        max_age: Duration,
    ) -> usize {
        let dir = self.storage_dir.join("data").join("default").join("chapters");
        let mut entries = match tokio::fs::read_dir(&dir).await {
            Ok(entries) => entries,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return 0,
            Err(err) => {
                tracing::warn!(dir = %dir.display(), "读取章节目录缓存目录失败: {err}");
                return 0;
            }
        };
        let now = std::time::SystemTime::now();
        let mut removed = 0usize;
        while let Ok(Some(entry)) = entries.next_entry().await {
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
                continue;
            };
            if pinned.contains(name) {
                continue;
            }
            let modified = entry
                .metadata()
                .await
                .ok()
                .and_then(|meta| meta.modified().ok())
                .unwrap_or(now);
            if now.duration_since(modified).unwrap_or_default() < max_age {
                continue;
            }
            match tokio::fs::remove_file(&path).await {
                Ok(()) => removed += 1,
                Err(err) => tracing::warn!(path = %path.display(), "删除孤儿目录缓存失败: {err}"),
            }
        }
        removed
    }

    pub async fn chapter_list_cache_exists(&self, user_ns: &str, toc_url: &str) -> bool {
        let path = self.chapter_list_cache_path(user_ns, toc_url);
        path.exists()
    }

    pub async fn get_bookshelf(&self, user_ns: &str) -> Result<Vec<Book>, AppError> {
        self.read_bookshelf(user_ns).await
    }

    pub async fn get_shelf_book(
        &self,
        user_ns: &str,
        book_url: &str,
    ) -> Result<Option<Book>, AppError> {
        let list = self.read_bookshelf(user_ns).await?;
        Ok(list.into_iter().find(|b| b.book_url == book_url))
    }

    /// Find book by chapter URL (chapter URL typically shares domain with book URL)
    pub async fn get_shelf_book_by_chapter(
        &self,
        user_ns: &str,
        chapter_url: &str,
    ) -> Result<Option<Book>, AppError> {
        let list = self.read_bookshelf(user_ns).await?;

        // Extract domain from chapter_url
        let chapter_domain = url::Url::parse(chapter_url)
            .ok()
            .and_then(|u| u.host_str().map(|h| h.to_string()));

        for book in list {
            // Check if chapter URL starts with book URL (common pattern)
            if chapter_url.starts_with(&book.book_url) {
                return Ok(Some(book));
            }

            // Check if they share the same domain
            if let (Some(ref ch_domain), Ok(book_url_parsed)) =
                (&chapter_domain, url::Url::parse(&book.book_url))
            {
                if let Some(book_domain) = book_url_parsed.host_str() {
                    if ch_domain == book_domain {
                        // Check if chapter URL path contains book URL path prefix
                        if let (Ok(ch_parsed), Ok(b_parsed)) = (
                            url::Url::parse(chapter_url),
                            url::Url::parse(&book.book_url),
                        ) {
                            let ch_path = ch_parsed.path();
                            let b_path = b_parsed.path();
                            // Check if paths share a common prefix (e.g., /book104/)
                            if ch_path.starts_with(b_path.trim_end_matches('/'))
                                || b_path
                                    .trim_end_matches('/')
                                    .starts_with(ch_path.trim_end_matches('/'))
                            {
                                return Ok(Some(book));
                            }
                        }
                    }
                }
            }
        }
        Ok(None)
    }

    /// Find book by name and author (for cases where book_url might differ)
    pub async fn find_shelf_book_by_name_author(
        &self,
        user_ns: &str,
        name: &str,
        author: &str,
    ) -> Result<Option<Book>, AppError> {
        let list = self.read_bookshelf(user_ns).await?;
        Ok(list.into_iter().find(|b| {
            same_remote_book_identity(&b.name, &b.author, name, author)
        }))
    }

    pub async fn save_book(&self, user_ns: &str, mut book: Book) -> Result<Book, AppError> {
        sanitize_book_urls(&mut book);
        if book.origin.trim().is_empty() {
            return Err(AppError::BadRequest("missing origin".to_string()));
        }
        if book.book_url.trim().is_empty() {
            return Err(AppError::BadRequest("bookUrl required".to_string()));
        }
        // 空简介归一化回 None。
        //
        // `intro` 的 `None` 在这套数据里有个具体含义: 「还没有简介」。
        // `merge_book` 正是用 `target.intro.is_none()` 判断「书源带回了简介就填进去」,
        // 所以 `None` 与 `Some("")` **不能**混为一谈。
        //
        // 前端「编辑书籍信息 → 还原」会把原始值(可能是空)整份提交回来, 于是原本
        // `None` 的简介会被写成 `Some("")` —— 那条回填判据从此永久失效, 换源/刷新
        // 再也补不进新书源的简介。在这里统一收口, 任何调用方都逃不掉。
        //
        // 注意只动 `intro`, 不碰 `original_intro`: 后者的 `''` 表示「记录过且原始为空」,
        // 是「还原」功能的依据, 归一化它会让还原目标丢失。
        if book.intro.as_deref() == Some("") {
            book.intro = None;
        }

        apply_book_read_config_override(&mut book);
        let _write_guard = self.bookshelf_write_lock.lock().await;
        let mut list = self.read_bookshelf(user_ns).await?;
        let mut exist_idx: Option<usize> = None;
        for (i, b) in list.iter().enumerate() {
            if books_match_for_save(b, &book) {
                exist_idx = Some(i);
                break;
            }
        }

        if let Some(i) = exist_idx {
            let exist = list[i].clone();
            merge_book_read_config(&exist, &mut book);
            merge_book_source_candidates(&exist, &mut book);
            if book.dur_chapter_index.is_none() {
                book.dur_chapter_index = exist.dur_chapter_index;
            }
            if book.dur_chapter_title.is_none() {
                book.dur_chapter_title = exist.dur_chapter_title;
            }
            if book.dur_chapter_time.is_none() {
                book.dur_chapter_time = exist.dur_chapter_time;
            }
            if book.dur_chapter_pos.is_none() {
                book.dur_chapter_pos = exist.dur_chapter_pos;
            }
            if book.total_chapter_num.is_none() {
                book.total_chapter_num = exist.total_chapter_num;
            }
            if book.last_check_time.is_none() {
                book.last_check_time = exist.last_check_time;
            }
            if book.group.is_none() {
                book.group = exist.group;
            }
            // `order` 是手动排序序号, 由书架拖拽写入; 只回传部分字段的调用方
            // (编辑书籍信息、切换书源等)不传它时不能把已有序号冲成 None ——
            // 一旦丢失, 下次「手动排序」会退回数组下标, 用户看到的顺序就变了。
            if book.order.is_none() {
                book.order = exist.order;
            }
            // 原始值一旦记下就不再被覆盖: 它代表「首次自定义之前的样子」。
            // 前端会整份回传, 这里兜底是为了任何只传部分字段的调用方(以及
            // 旧版本客户端)都不会把已记录的原始值冲掉。
            if book.original_name.is_none() {
                book.original_name = exist.original_name;
            }
            if book.original_author.is_none() {
                book.original_author = exist.original_author;
            }
            if book.original_intro.is_none() {
                book.original_intro = exist.original_intro;
            }
            list[i] = book.clone();
        } else {
            ensure_current_source_candidate(&mut book);
            list.push(book.clone());
        }

        self.write_bookshelf(user_ns, &list).await?;
        Ok(book)
    }

    pub async fn save_books(&self, user_ns: &str, books: Vec<Book>) -> Result<Vec<Book>, AppError> {
        let _write_guard = self.bookshelf_write_lock.lock().await;
        let mut normalized = Vec::with_capacity(books.len());
        for mut book in books {
            sanitize_book_urls(&mut book);
            if book.origin.trim().is_empty() {
                return Err(AppError::BadRequest("missing origin".to_string()));
            }
            if book.book_url.trim().is_empty() {
                return Err(AppError::BadRequest("bookUrl required".to_string()));
            }
            apply_book_read_config_override(&mut book);
            normalized.push(book);
        }
        self.write_bookshelf(user_ns, &normalized).await?;
        Ok(normalized)
    }

    pub async fn remove_source_candidates(
        &self,
        user_ns: &str,
        removed_source_urls: &HashSet<String>,
    ) -> Result<(), AppError> {
        if removed_source_urls.is_empty() {
            return Ok(());
        }
        let removed = removed_source_urls
            .iter()
            .map(|url| normalize_source_url(url))
            .collect::<HashSet<_>>();

        // 廉价短路: 先看原始 JSON 里有没有出现这些源地址, 没有就直接返回。
        //
        // 删失效书源时这步几乎总是命中 —— 失效源通常从没被任何书引用过。
        // 完整走一遍是「反序列化整本书架 + 遍历所有候选 + 整表重写」, 书架有几百
        // 上千本时是几百毫秒级; 一次子串扫描则几乎无成本。
        //
        // 只做**保守**判断: 未命中则一定没有引用, 可以安全跳过; 命中可能只是
        // 恰好出现同名字段, 仍走完整流程(那样也不会误删任何东西)。
        //
        // 转义形式要一并比较: JSON 里 `"`/`\` 等会被写成 `\"`/`\\`, 只比对原始
        // URL 会在含这类字符时误判成"无引用"而错误跳过。
        let candidates = removed
            .iter()
            .flat_map(|url| {
                let escaped = serde_json::to_string(url).unwrap_or_else(|_| format!("\"{url}\""));
                // to_string 会带外层引号, 去掉它才是字段值内部的样子。
                let inner = escaped
                    .strip_prefix('"')
                    .and_then(|s| s.strip_suffix('"'))
                    .unwrap_or(url.as_str())
                    .to_string();
                [url.clone(), inner]
            })
            .collect::<Vec<_>>();
        let path = self.bookshelf_path(user_ns);
        if !path.exists() {
            return Ok(());
        }
        // 短路探测用锁外读: 它只做一次子串扫描, 不参与写入决策。
        // 真正的读-改-写必须在锁内重新读, 见下方 `_write_guard` 之后 —— 用这里的
        // `probe` 去反序列化会让写锁形同虚设 (与并发 save_books/delete_book 交错时
        // 用陈旧内容整表覆盖, 丢失并发写入)。
        let probe = fs::read_to_string(&path)
            .await
            .map_err(|e| AppError::Internal(e.into()))?;
        if !candidates.iter().any(|url| probe.contains(url.as_str())) {
            return Ok(());
        }
        drop(probe);

        let _write_guard = self.bookshelf_write_lock.lock().await;
        // 锁内重新读: 上面的探测与这里之间可能有并发写入, 必须基于最新内容修改。
        let raw = fs::read_to_string(&path)
            .await
            .map_err(|e| AppError::Internal(e.into()))?;
        let mut list: Vec<Book> = match serde_json::from_str(&raw) {
            Ok(list) => list,
            Err(primary_err) => {
                let recovered = recover_bookshelf_entries(&raw)
                    .ok_or_else(|| AppError::BadRequest(primary_err.to_string()))?;
                tracing::warn!(
                    "recovered malformed bookshelf for user_ns={}, path={}, entries={}",
                    user_ns,
                    path.display(),
                    recovered.len()
                );
                self.write_bookshelf(user_ns, &recovered).await?;
                recovered
            }
        };
        for book in &mut list {
            sanitize_book_urls(book);
        }
        let mut changed = false;
        for book in &mut list {
            if let Some(candidates) = book.source_candidates.as_mut() {
                let before = candidates.len();
                candidates.retain(|candidate| {
                    !removed.contains(&normalize_source_url(&candidate.origin))
                });
                changed |= before != candidates.len();
                if candidates.is_empty() {
                    book.source_candidates = None;
                }
            }
        }
        if changed {
            self.write_bookshelf(user_ns, &list).await?;
        }
        Ok(())
    }

    pub async fn delete_book(&self, user_ns: &str, book: &Book) -> Result<bool, AppError> {
        let _write_guard = self.bookshelf_write_lock.lock().await;
        let mut list = self.read_bookshelf(user_ns).await?;
        let orig_len = list.len();
        let removed: Vec<Book> = list
            .iter()
            .filter(|b| books_match_for_delete(b, book))
            .cloned()
            .collect();
        list.retain(|b| !books_match_for_delete(b, book));
        let deleted = list.len() != orig_len;
        if deleted {
            self.write_bookshelf(user_ns, &list).await?;
            for removed_book in &removed {
                let _ = self.clear_book_related_cache(user_ns, removed_book).await;
            }
        }
        Ok(deleted)
    }

    pub async fn delete_books(&self, user_ns: &str, books: Vec<Book>) -> Result<usize, AppError> {
        let _write_guard = self.bookshelf_write_lock.lock().await;
        let mut list = self.read_bookshelf(user_ns).await?;
        let mut deleted = 0usize;
        let mut removed_books: Vec<Book> = Vec::new();
        for book in books {
            let matched: Vec<Book> = list
                .iter()
                .filter(|b| books_match_for_delete(b, &book))
                .cloned()
                .collect();
            removed_books.extend(matched);
            let before = list.len();
            list.retain(|b| !books_match_for_delete(b, &book));
            if list.len() != before {
                deleted += 1;
            }
        }
        if deleted > 0 {
            self.write_bookshelf(user_ns, &list).await?;
            for removed_book in &removed_books {
                let _ = self.clear_book_related_cache(user_ns, removed_book).await;
            }
        }
        Ok(deleted)
    }

    pub async fn cached_chapter_count(
        &self,
        user_ns: &str,
        book_url: &str,
        chapter_urls: &[String],
    ) -> Result<usize, AppError> {
        let book_key = md5_hex(book_url);
        // 旧实现对每个章节 URL 单独 path.exists(), 一本书几千章就几千次系统调用,
        // 且在 async 上下文里做同步阻塞 IO。改为一次 read_dir 取回所有缓存文件名,
        // 用 HashSet 匹配, O(1) per chapter。
        let cached_files = self.cache.list_chapter_files(user_ns, &book_key).await?;
        if cached_files.is_empty() {
            return Ok(0);
        }
        let cached_set: std::collections::HashSet<&str> =
            cached_files.iter().map(|s| s.as_str()).collect();
        let count = chapter_urls
            .iter()
            .filter(|url| {
                let name = md5_hex(url);
                cached_set.contains(name.as_str())
            })
            .count();
        Ok(count)
    }

    pub async fn cached_chapter_urls(
        &self,
        user_ns: &str,
        book_url: &str,
        chapter_urls: &[String],
    ) -> Result<Vec<String>, AppError> {
        let book_key = md5_hex(book_url);
        let cached_files = self.cache.list_chapter_files(user_ns, &book_key).await?;
        if cached_files.is_empty() {
            return Ok(Vec::new());
        }
        let cached_set: std::collections::HashSet<&str> =
            cached_files.iter().map(|s| s.as_str()).collect();
        Ok(chapter_urls
            .iter()
            .filter(|url| cached_set.contains(md5_hex(url).as_str()))
            .cloned()
            .collect())
    }

    /// 批量取回已缓存章节的"读者可见字数"。
    ///
    /// 字数在**写入缓存时**就存进 sidecar(`<md5>.count`), 这里只读那个小文件, 不再为
    /// 目录打开去解析整篇正文。只返回本地已有正文的章节, 未缓存的章节不出现。
    ///
    /// 旧缓存没有 sidecar: 命中时现算一次并回填, 下次就不必再解析。
    ///
    /// 与 legado 的差别: legado 用 `content.length`(含 HTML 标签), 这里去掉标签后再数,
    /// 数字更接近读者感知的字数。
    pub async fn chapter_word_counts(
        &self,
        user_ns: &str,
        book_url: &str,
        chapter_urls: &[String],
    ) -> Result<Vec<(String, usize)>, AppError> {
        let book_key = md5_hex(book_url);
        let cached_files = self.cache.list_chapter_files(user_ns, &book_key).await?;
        if cached_files.is_empty() {
            return Ok(Vec::new());
        }
        let cached_set: std::collections::HashSet<&str> =
            cached_files.iter().map(|s| s.as_str()).collect();

        let mut counts = Vec::new();
        for url in chapter_urls {
            if !cached_set.contains(md5_hex(url).as_str()) {
                continue;
            }
            // 快路径: sidecar 命中, 不读正文。
            if let Ok(Some(count)) = self.cache.get_word_count(user_ns, &book_key, url).await {
                counts.push((url.clone(), count));
                continue;
            }
            // 慢路径: 旧缓存没有 sidecar。现算并回填, 只在这一章上付一次代价。
            let Ok(Some(content)) = self.cache.get(user_ns, &book_key, url).await else {
                continue;
            };
            let count = visible_char_count(&content);
            let _ = self
                .cache
                .set_word_count(user_ns, &book_key, url, count)
                .await;
            counts.push((url.clone(), count));
        }
        Ok(counts)
    }

    /// Remove persistent JS variables owned by a book and its known chapters.
    /// Chapter keys can only be addressed by URL, so callers pass whatever
    /// chapter list they already have instead of triggering a network fetch
    /// during deletion.
    pub fn clear_book_variables(&self, book_url: &str, chapter_urls: &[String]) {
        crate::parser::js::remove_book_variables(book_url, chapter_urls);
    }

    pub async fn cache_chapter(
        &self,
        user_ns: &str,
        book_url: &str,
        source: &BookSource,
        chapter_url: &str,
        refresh: bool,
    ) -> Result<(), AppError> {
        let book_key = md5_hex(book_url);
        if refresh {
            let _ = self.cache.remove(user_ns, &book_key, chapter_url).await;
        }
        // 缓存走**完整解析**, 与阅读时未命中的那一趟完全一致(含 jsLib 与段评插件)。
        // 缓存命中后阅读不再联网, 所以这里存的必须就是"之后要读到的那份正文";
        // 为省时间跳过书源脚本会让段评标记缺失, 而且这份纯正文会被当成最终结果。
        let content = self
            .get_content_with_stage(user_ns, book_url, source, chapter_url, None)
            .await?;
        // 以「缓存是否真的落盘」为准：空正文不会被写入缓存（见 get_content_with_stage
        // 里的非空判断）。若仍未落盘就报错计入失败数，否则进度会把没缓存的
        // 章节算作成功，且下次缓存时这些章又被判为未缓存。
        if content.trim().is_empty()
            || !self.is_chapter_cached(user_ns, book_url, chapter_url).await
        {
            return Err(AppError::Internal(anyhow::anyhow!(
                "正文解析为空, 未写入缓存"
            )));
        }
        Ok(())
    }

    pub async fn get_cover(&self, user_ns: &str, url: &str) -> Result<(Vec<u8>, String), AppError> {
        self.get_cover_for_source(user_ns, url, None).await
    }

    /// 取封面，并按书源的 `coverDecodeJs` 解密。
    ///
    /// 很多站点（如 18mh）的封面图是 AES 加密的：下载到的字节不是图片，直接
    /// 交给 `<img>` 只能是花图/空白。legado 在 Glide 拿到流之后调用
    /// `ImageUtils.decode(..., isCover = true)` 执行书源的 `coverDecodeJs`
    /// （见 `app/src/main/java/io/legado/app/utils/ImageUtils.kt`），本端此前
    /// 只把该字段解析进模型、封面流水线从未执行 —— 于是同一书源在手机上正常、
    /// 在桌面端花屏（issue #23）。
    ///
    /// 解密必须发生在**嗅探 Content-Type 与写缓存之前**：密文的魔数不是任何
    /// 图片格式，先嗅探只会得到 `application/octet-stream`，落盘扩展名也就错了。
    ///
    /// `source` 由 `/cover` 路由反查得到（前端把书籍的 `origin` 带在查询串里）。
    /// 拿不到书源时保持原行为，只是不解密。
    pub async fn get_cover_for_source(
        &self,
        user_ns: &str,
        url: &str,
        source: Option<&BookSource>,
    ) -> Result<(Vec<u8>, String), AppError> {
        let cover_dir = self
            .storage_dir
            .join("cache")
            .join(user_ns)
            .join("cover");
        let mapping_path = cover_dir.join("urls").join(format!("{}.json", md5_hex(url)));
        if let Ok(mapped) = fs::read_to_string(&mapping_path).await {
            let entry: serde_json::Value =
                serde_json::from_str(&mapped).unwrap_or(serde_json::Value::Null);
            if let (Some(file), Some(content_type)) = (
                entry.get("file").and_then(serde_json::Value::as_str),
                entry
                    .get("contentType")
                    .and_then(serde_json::Value::as_str),
            ) {
                if let Ok(data) = fs::read(cover_dir.join(file)).await {
                    return self
                        .repair_cached_cover(url, data, content_type, source, &cover_dir)
                        .await;
                }
            }
        }

        // 历史遗留: 早先 `sanitize_book_urls` 会把封面地址里的 `%3D` 反转义成
        // `=`, 于是书架记下的 URL 与搜索页缓存那份对不上。读缓存时按原始变体
        // 再找一次, 命中就把映射搬到当前 URL 下 —— 用户不必重新搜一遍。
        //
        // 搬的是映射本身而不是重新落一份内容文件: 内容文件是内容寻址的, 重写
        // 只会因为扩展名规则变化而多出一份等价的孤儿。
        //
        // 目标映射已存在时只读不搬: 两条只差 `=` 的 URL 可能都还在用, 把旧映射
        // 删掉会让另一条下次白白重新回源。
        if let Some(legacy_url) = reencode_cover_signature(url) {
            let legacy_mapping = cover_dir
                .join("urls")
                .join(format!("{}.json", md5_hex(&legacy_url)));
            if let Ok(mapped) = fs::read_to_string(&legacy_mapping).await {
                let entry: serde_json::Value =
                    serde_json::from_str(&mapped).unwrap_or(serde_json::Value::Null);
                let file = entry.get("file").and_then(serde_json::Value::as_str);
                let content_type = entry
                    .get("contentType")
                    .and_then(serde_json::Value::as_str);
                if let (Some(file), Some(content_type)) = (file, content_type) {
                    if let Ok(data) = fs::read(cover_dir.join(file)).await {
                        if !mapping_path.exists() {
                            let _ = fs::write(&mapping_path, &mapped).await;
                            let _ = fs::remove_file(&legacy_mapping).await;
                        }
                        return self
                            .repair_cached_cover(url, data, content_type, source, &cover_dir)
                            .await;
                    }
                }
            }
        }

        // Compatibility with the previous URL-addressed layout. A hit is
        // promoted into the content-addressed layout before returning.
        let ext = file_ext_from_url(url).unwrap_or_else(|| "png".to_string());
        let legacy_path = cover_dir.join(format!("{}.{}", md5_hex(url), ext));
        if legacy_path.exists() {
            let data = fs::read(&legacy_path)
                .await
                .map_err(|e| AppError::Internal(e.into()))?;
            let content_type = content_type_from_ext(&ext);
            if self
                .store_cover_mapping(url, &data, &content_type, &cover_dir)
                .await
                .is_ok()
            {
                let _ = fs::remove_file(&legacy_path).await;
            }
            return self
                .repair_cached_cover(url, data, &content_type, source, &cover_dir)
                .await;
        }
        if let Some(parent) = mapping_path.parent() {
            fs::create_dir_all(parent)
                .await
                .map_err(|e| AppError::Internal(e.into()))?;
        }

        // Extract referer from URL for anti-hotlinking bypass
        let referer = url::Url::parse(url).ok().and_then(|u| {
            let scheme = u.scheme();
            let host = u.host_str()?;
            Some(format!("{}://{}", scheme, host))
        });

        let build_request = |client: &reqwest::Client, target: &str| {
            let mut req = client.get(target);
            // Add necessary headers to bypass anti-hotlinking
            req = req
                .header("User-Agent", "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36")
                .header("Accept", "image/avif,image/webp,image/apng,image/svg+xml,image/*,*/*;q=0.8");
            if let Some(ref referer) = referer {
                req = req.header("Referer", referer);
            }
            req
        };

        let mut res = build_request(&self.http.client(), url).send().await;
        // 封面请求此前只走系统代理。共享代理 IP 被站点 WAF 整段拦下时(该源
        // 某书源的排行榜页就是代理 403 / 直连 200), 正文链路有直连回退
        // 而封面没有, 于是列表能出来、封面全部 404。与 `retry_direct_when_proxy_blocked`
        // 同理: 发送失败(超时/连接被拒)或返回 403/407/451 这类 WAF 拒绝码时,
        // 用直连客户端重发一次, 成功才替换原响应。
        let retry_status = match &res {
            Ok(response) => Some(response.status().as_u16()),
            Err(_) => None,
        };
        if cover_needs_direct_retry(self.http.active_proxy().is_some(), retry_status) {
            if let Ok(direct) = self.http.client_direct() {
                if let Ok(direct_res) = build_request(&direct, url).send().await {
                    if direct_res.status().is_success() {
                        tracing::info!(url = %url, "proxy cover request blocked; used direct retry");
                        res = Ok(direct_res);
                    }
                }
            }
        }
        // 书架里可能还留着旧版本写坏的地址(`%3D` 已被反转义成 `=`)。这类地址
        // 回源必然被站点拒掉, 但把签名还原成 `%3D` 后站点就认了 —— 重试一次,
        // 让历史条目不必等用户重新搜索。缓存仍按传入的 URL 存, 保持调用方看到的一致。
        //
        // 只在**被拒**(403/407/451) 时重试: 超时或连接失败时换条地址重发, 只会
        // 让一次卡住的封面等成两次。重试同样先走代理客户端, 失败再走直连, 与上面
        // 那条回退保持同一套判据。
        let rejected_status = match &res {
            Ok(response) => Some(response.status().as_u16()),
            Err(_) => None,
        };
        if cover_needs_direct_retry(self.http.active_proxy().is_some(), rejected_status) {
            if let Some(repaired_url) = reencode_cover_signature(url) {
                let mut repaired_res = build_request(&self.http.client(), &repaired_url)
                    .send()
                    .await
                    .ok()
                    .filter(|r| r.status().is_success());
                if repaired_res.is_none() {
                    if let Ok(direct) = self.http.client_direct() {
                        repaired_res = build_request(&direct, &repaired_url)
                            .send()
                            .await
                            .ok()
                            .filter(|r| r.status().is_success());
                    }
                }
                if let Some(repaired_res) = repaired_res {
                    tracing::info!(url = %url, "cover signature was unescaped; retried with %3D");
                    res = Ok(repaired_res);
                }
            }
        }
        let res = res.map_err(|e| AppError::Internal(e.into()))?;
        if !res.status().is_success() {
            return Err(AppError::NotFound("cover not found".to_string()));
        }
        let content_type = res
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_string())
            .unwrap_or_else(|| content_type_from_ext(&ext));
        let bytes = res
            .bytes()
            .await
            .map_err(|e| AppError::Internal(e.into()))?
            .to_vec();
        // 书源带 coverDecodeJs → 解密后再当图片处理（legado 的
        // `ImageUtils.decode(src, bytes, isCover = true, source)`，手机端是
        // 无条件执行规则）。
        //
        // 两条兜底，都是为了"坏封面不等于 404"：
        // 1. 脚本报错 → 用原始字节；
        // 2. 解出来的不是图片、但原始字节本来就是图片 → 用原始字节。
        //    少数书源只对部分封面加密（或站点换了未加密的图床），无条件解密
        //    会把本来能看的图画坏。
        let original_is_image = sniff_image_content_type(&bytes).is_some();
        let (bytes, decoded) = match self.decode_cover_bytes(source, url, &bytes).await {
            Ok(Some(decoded)) => {
                if sniff_image_content_type(&decoded).is_some() || !original_is_image {
                    (decoded, true)
                } else {
                    tracing::debug!(url = %url, "封面解密结果不是图片，保留原图");
                    (bytes, false)
                }
            }
            Ok(None) => (bytes, false),
            Err(err) => {
                tracing::warn!(url = %url, "封面解密失败，按原始字节返回: {err:?}");
                (bytes, false)
            }
        };
        // 服务器没给 Content-Type 时, 上面的兜底会拿 URL 后缀猜, 而带尺寸参数的
        // `…:1200:0.image` 猜不出来。图就在手里, 按魔数认格式最可靠。
        //
        // 解密过的字节要**无条件**重认: 服务端给的 content-type 描述的是密文
        // (18mh 回 `binary/octet-stream`, 也有站点照旧回 `image/jpeg`), 而真正
        // 落盘的扩展名必须按解密后明文的魔数来定。
        let content_type = if decoded || is_unknown_image_type(&content_type) {
            sniff_image_content_type(&bytes).unwrap_or(content_type)
        } else {
            content_type
        };
        if let Err(err) = self
            .store_cover_mapping(url, &bytes, &content_type, &cover_dir)
            .await
        {
            tracing::warn!("写入封面缓存失败: {err:?}");
        }
        Ok((bytes, content_type))
    }

    /// 缓存命中时的解密补救。
    ///
    /// 修复前落盘的封面是**密文**（解密逻辑此前不存在），映射里记的
    /// `contentType` 也是按 URL 后缀或 `application/octet-stream` 猜的。
    /// 直接返回这份字节，用户会一直看到花图，而且「清了缓存再来」也一样
    /// —— 因为每次都会命中同一个坏文件。这里识别出"缓存里不是图片"的情况，
    /// 当场解密并把正确的字节与 content-type 写回缓存。
    async fn repair_cached_cover(
        &self,
        url: &str,
        data: Vec<u8>,
        content_type: &str,
        source: Option<&BookSource>,
        cover_dir: &Path,
    ) -> Result<(Vec<u8>, String), AppError> {
        // 已经是图片：正常返回，不做任何多余工作（绝大多数字节走这条）。
        if sniff_image_content_type(&data).is_some() {
            return Ok((data, content_type.to_string()));
        }
        // 不是图片、又没有解密规则 —— 原始站点就是这样，原样返回。
        if source.and_then(|s| s.cover_decode_js.as_deref()).map(str::trim).unwrap_or("").is_empty() {
            return Ok((data, content_type.to_string()));
        }
        let decoded = match self.decode_cover_bytes(source, url, &data).await {
            Ok(Some(decoded)) if !decoded.is_empty() => decoded,
            Ok(_) => return Ok((data, content_type.to_string())),
            Err(err) => {
                tracing::warn!(url = %url, "缓存封面解密失败，保留原字节: {err:?}");
                return Ok((data, content_type.to_string()));
            }
        };
        // 解密结果仍认不出是图片（规则写错/密钥不对）：不要覆盖缓存，
        // 让下次还有机会在规则修正后自愈。
        let Some(new_type) = sniff_image_content_type(&decoded) else {
            tracing::warn!(url = %url, "封面解密结果不是已知图片格式，保留原缓存");
            return Ok((data, content_type.to_string()));
        };
        // 记下旧的密文文件名：重写映射后它就没人引用了。**必须当场删掉**，
        // 不能指望 `clean_stale_search_covers` —— 那条清理只在启动时跑一次，
        // 且要求文件闲置 30 分钟，否则这份密文会一直占着磁盘到下次重启。
        let previous_file = cover_mapping_file(cover_dir, url).await;
        if let Err(err) = self
            .store_cover_mapping(url, &decoded, &new_type, cover_dir)
            .await
        {
            tracing::warn!("重写封面缓存失败: {err:?}");
        } else if let Some(previous) = previous_file {
            // 只有确认没有别的 URL 还引用它时才删（内容寻址，可能被共享）。
            let referenced = collect_referenced_cover_files(cover_dir)
                .await
                .map(|set| set.contains(&previous))
                .unwrap_or(true);
            if !referenced {
                let _ = fs::remove_file(cover_dir.join(&previous)).await;
            }
        }
        tracing::info!(url = %url, "已解密历史的密文封面缓存");
        Ok((decoded, new_type))
    }

    /// 执行书源的 `coverDecodeJs`，把下载到的封面字节解密成图片字节。
    ///
    /// 语义对齐 legado `ImageUtils.decode(src, bytes, isCover = true, source)`：
    /// 规则为空就是"不处理"（返回 `None`），脚本抛错由调用方决定怎么兜底。
    ///
    /// 脚本里 `result` / `src` / `sj` 的约定与手机端一致：`result` 是**字节序列**
    /// （不能是 UTF-8 文本，否则密文里 ≥0x80 的字节已经被改写），`src` 是图片地址。
    /// 返回值同样按字节取回 —— 书源普遍写 `.decrypt(result)`，而 legado 的
    /// `SymmetricCrypto.decrypt` 返回 `ByteArray`。
    async fn decode_cover_bytes(
        &self,
        source: Option<&BookSource>,
        src: &str,
        raw: &[u8],
    ) -> Result<Option<Vec<u8>>, AppError> {
        let Some(source) = source else { return Ok(None) };
        let Some(script) = source
            .cover_decode_js
            .as_deref()
            .map(str::trim)
            .filter(|script| !script.is_empty())
        else {
            return Ok(None);
        };
        let script = script.to_string();
        let source = source.clone();
        let (src, raw) = (src.to_string(), raw.to_vec());
        // JS 求值是同步的（且脚本内部可能发同步 HTTP），放阻塞线程池，
        // 与其它规则解析保持一致，避免占住 tokio worker。
        self.run_source_task("public", &source, move |_source| {
            crate::parser::js::eval_js_cover_decode(&script, &src, &raw)
                .map_err(|err| AppError::BadRequest(format!("封面解密失败: {err}")))
        })
        .await?
        .map(Some)
    }

    async fn store_cover_mapping(
        &self,
        url: &str,
        bytes: &[u8],
        content_type: &str,
        cover_dir: &Path,
    ) -> std::io::Result<()> {
        let digest = hex::encode(Sha256::digest(bytes));
        let extension = cover_file_extension(url, content_type);
        let file_name = format!("{digest}.{extension}");
        fs::create_dir_all(cover_dir.join("urls")).await?;
        let content_path = cover_dir.join(&file_name);
        if !content_path.exists() {
            fs::write(&content_path, bytes).await?;
        }
        let mapping = serde_json::json!({
            "file": file_name,
            "contentType": content_type,
        });
        let mapping_path = cover_dir.join("urls").join(format!("{}.json", md5_hex(url)));
        fs::write(
            mapping_path,
            serde_json::to_vec(&mapping).unwrap_or_default(),
        )
        .await
    }

    /// Remove several cover URL mappings in one pass.
    ///
    /// The mappings are removed first, then one scan of the remaining mappings
    /// determines which deduplicated content files are still shared. This avoids
    /// rescanning `urls/` once per URL when a search session releases many covers.
    pub async fn delete_cover_caches(
        &self,
        user_ns: &str,
        urls: &[String],
    ) -> Result<(), AppError> {
        let mut seen = HashSet::new();
        let urls: Vec<&str> = urls
            .iter()
            .map(|url| url.trim())
            .filter(|url| !url.is_empty() && seen.insert(*url))
            .collect();
        if urls.is_empty() {
            return Ok(());
        }

        let cover_dir = self
            .storage_dir
            .join("cache")
            .join(user_ns)
            .join("cover");

        let mut candidate_files = HashSet::new();
        for url in urls {
            let mapping_path = cover_dir
                .join("urls")
                .join(format!("{}.json", md5_hex(url)));
            if let Ok(mapped) = fs::read_to_string(&mapping_path).await {
                let entry: serde_json::Value =
                    serde_json::from_str(&mapped).unwrap_or(serde_json::Value::Null);
                if let Some(file) = entry.get("file").and_then(serde_json::Value::as_str) {
                    candidate_files.insert(file.to_string());
                }
            }
            let _ = fs::remove_file(&mapping_path).await;

            // Remove the old URL-addressed file as well during migration. 旧写入用
            // `file_ext_from_url(..).unwrap_or("image")` 兜底, 所以 `.image` 也要清。
            let ext = file_ext_from_url(url).unwrap_or_else(|| "png".to_string());
            let _ = fs::remove_file(cover_dir.join(format!("{}.{}", md5_hex(url), ext))).await;
            if ext != "image" {
                let _ = fs::remove_file(cover_dir.join(format!("{}.image", md5_hex(url)))).await;
            }
        }

        match collect_referenced_cover_files(&cover_dir).await {
            Ok(referenced) => {
                for file_name in candidate_files.difference(&referenced) {
                    let _ = fs::remove_file(cover_dir.join(file_name)).await;
                }
            }
            Err(err) => {
                // 引用扫描失败时只保留内容文件，不冒险误删共享图片。
                tracing::warn!("扫描封面内容引用失败，跳过内容文件清理: {err:?}");
            }
        }
        Ok(())
    }

    /// Remove a cover's URL mapping and the content file when no other URL
    /// still references the same bytes.
    pub async fn delete_cover_cache(&self, user_ns: &str, url: &str) -> Result<(), AppError> {
        self.delete_cover_caches(user_ns, &[url.to_string()]).await
    }

    /// Safety net for crashes or forced exits that bypass the search-session
    /// cleanup.
    ///
    /// 崩溃场景下「映射 + 内容文件」都会留下, 只按内容文件找孤儿是收不到的
    /// —— 映射还在, 内容文件就被判定为「仍被引用」。所以这里先按年龄回收
    /// `urls/` 里过期的映射本身, 再回收因此变成孤儿的内容文件。
    ///
    /// 内容文件是内容寻址且可能被多个 URL 共享, 只有在映射扫描确认无人引用
    /// 之后才删除。
    ///
    /// **书架上的封面必须豁免**: 前端只会在会话结束时释放搜索/发现页的封面,
    /// 但那条路径可能整体失效（进程被强杀、IPC 失败、`release` 没被调用）。
    /// 单靠"文件闲置 30 分钟"会把**书架书的封面**也一起回收 —— 用户隔天打开
    /// 书架, 三本书的封面全没了。所以先收集书架引用的封面 URL, 再把它们
    /// 对应的映射与内容文件钉住。
    pub async fn clean_stale_search_covers(&self, max_age: Duration) -> Result<usize, AppError> {
        let cover_dir = self
            .storage_dir
            .join("cache")
            .join("public")
            .join("cover");
        if !cover_dir.exists() {
            return Ok(0);
        }
        let pinned = self.shelf_cover_mapping_files().await;
        let mut removed = 0usize;
        removed += prune_stale_cover_mappings(&cover_dir, max_age, &pinned).await?;
        removed += remove_orphan_cover_files(&cover_dir, max_age).await?;
        Ok(removed)
    }

    /// 书架上仍然引用的封面，其映射文件名（`<md5(url)>.json`）。
    ///
    /// 这些映射**不能**被按年龄回收：它们的 URL 还在书架里，回收等于把用户
    /// 书架上的封面删掉。读书架失败时返回空集合（宁可多留，也不误删）。
    async fn shelf_cover_mapping_files(&self) -> HashSet<String> {
        // 书架在 `default` 命名空间；封面缓存统一在 `public`（未登录也可读）。
        let Ok(books) = self.read_bookshelf("default").await else {
            return HashSet::new();
        };
        let mut pinned = HashSet::new();
        for book in books {
            for url in [book.cover_url.as_deref(), book.custom_cover_url.as_deref()]
                .into_iter()
                .flatten()
                .map(str::trim)
                .filter(|url| !url.is_empty())
            {
                pinned.insert(format!("{}.json", md5_hex(url)));
            }
        }
        pinned
    }

    pub async fn load_book_sources_cache(
        &self,
        user_ns: &str,
        book_url: &str,
    ) -> Result<Option<Vec<SearchBook>>, AppError> {
        let path = self.book_source_cache_path(user_ns, book_url);
        if !path.exists() {
            return Ok(None);
        }
        let data = fs::read_to_string(&path)
            .await
            .map_err(|e| AppError::Internal(e.into()))?;
        let list: Vec<SearchBook> =
            serde_json::from_str(&data).map_err(|e| AppError::BadRequest(e.to_string()))?;
        Ok(Some(list))
    }

    pub async fn save_book_sources_cache(
        &self,
        user_ns: &str,
        book_url: &str,
        list: &Vec<SearchBook>,
    ) -> Result<(), AppError> {
        let path = self.book_source_cache_path(user_ns, book_url);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .await
                .map_err(|e| AppError::Internal(e.into()))?;
        }
        let data = serde_json::to_string(list).map_err(|e| AppError::BadRequest(e.to_string()))?;
        fs::write(&path, data)
            .await
            .map_err(|e| AppError::Internal(e.into()))?;
        Ok(())
    }

    pub async fn delete_book_sources_cache(
        &self,
        user_ns: &str,
        book_url: &str,
    ) -> Result<(), AppError> {
        let path = self.book_source_cache_path(user_ns, book_url);
        if path.exists() {
            fs::remove_file(&path)
                .await
                .map_err(|e| AppError::Internal(e.into()))?;
        }
        Ok(())
    }

    fn book_source_cache_path(&self, user_ns: &str, book_url: &str) -> PathBuf {
        let name = md5_hex(book_url);
        self.storage_dir
            .join("data")
            .join(user_ns)
            .join("book_sources")
            // Earlier caches merged aggregate stations by origin and could omit
            // completed results. Rebuild once under the complete-list format.
            .join(format!("{}.v2.json", name))
    }

    fn bookshelf_path(&self, user_ns: &str) -> PathBuf {
        self.storage_dir
            .join("data")
            .join(user_ns)
            .join("bookshelf.json")
    }

    async fn read_bookshelf(&self, user_ns: &str) -> Result<Vec<Book>, AppError> {
        let path = self.bookshelf_path(user_ns);
        if !path.exists() {
            return Ok(Vec::new());
        }
        let data = fs::read_to_string(&path)
            .await
            .map_err(|e| AppError::Internal(e.into()))?;
        let mut list: Vec<Book> = match serde_json::from_str(&data) {
            Ok(list) => list,
            Err(primary_err) => {
                let recovered = recover_bookshelf_entries(&data)
                    .ok_or_else(|| AppError::BadRequest(primary_err.to_string()))?;
                tracing::warn!(
                    "recovered malformed bookshelf for user_ns={}, path={}, entries={}",
                    user_ns,
                    path.display(),
                    recovered.len()
                );
                self.write_bookshelf(user_ns, &recovered).await?;
                recovered
            }
        };
        for book in &mut list {
            sanitize_book_urls(book);
            apply_book_read_config_override(book);
        }
        Ok(list)
    }

    /// 书架落盘: 先写临时文件再原子替换, 读者永远看不到写了一半的 JSON。
    ///
    /// 必须原子替换的原因: 书架是多个命令共读共写的同一份文件。离开阅读页时的
    /// 进度写入(save_book_progress)与书架页的读取(get_shelf_book_with_cache_info)
    /// 会并发发生 —— 旧的 `fs::write` 直接截断目标文件再写, 夹在中间的读取会拿到
    /// 空串或半截 JSON, 于是要么报「解析失败」, 要么走 `recover_bookshelf_entries`
    /// 的恢复分支(它还会顺手把恢复结果回写, 与正在写的那次互相覆盖)。
    async fn write_bookshelf(&self, user_ns: &str, list: &Vec<Book>) -> Result<(), AppError> {
        let path = self.bookshelf_path(user_ns);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .await
                .map_err(|e| AppError::Internal(e.into()))?;
        }
        let data = serde_json::to_string(list).map_err(|e| AppError::BadRequest(e.to_string()))?;
        // 临时名带唯一后缀: 极端并发下(进度写入与书架编辑同时落盘)两次写不会
        // 争用同一个中间文件, 各自 rename 一次, 最终内容仍是某一次的完整快照。
        let temp_path = path.with_extension(format!("json.{}.tmp", uuid::Uuid::new_v4().simple()));
        fs::write(&temp_path, data)
            .await
            .map_err(|e| AppError::Internal(e.into()))?;
        // Windows 上替换失败只有一种常见原因: 目标文件正被某个读者打开。读者只
        // 做一次短读, 短暂重试即可; 重试仍失败才把错误上报, 不静默吞掉。
        let mut last_error = None;
        for attempt in 0..5u32 {
            match fs::rename(&temp_path, &path).await {
                Ok(()) => return Ok(()),
                Err(error) => {
                    last_error = Some(error);
                    if attempt < 4 {
                        tokio::time::sleep(std::time::Duration::from_millis(15)).await;
                    }
                }
            }
        }
        let _ = fs::remove_file(&temp_path).await;
        Err(AppError::Internal(
            last_error.expect("rename 至少失败过一次").into(),
        ))
    }

    // Chapter list cache methods
    fn chapter_list_cache_path(&self, user_ns: &str, toc_url: &str) -> PathBuf {
        let name = md5_hex(toc_url);
        // The aggregate catalog embeds chapter payloads and review scripts in
        // data URLs. Older java.put/get and getCookie implementations could
        // cache incomplete URLs, so rebuild only this protocol's catalogs once.
        // Content files stay available as offline fallbacks.
        let (url, options) = split_url_options(toc_url);
        let aggregate_catalog = url.starts_with("data:")
            && options
                .and_then(|options| serde_json::from_str::<serde_json::Value>(options).ok())
                .is_some_and(|options| {
                    options.get("type").and_then(|value| value.as_str()) == Some("gycatalog")
                });
        let revision = if aggregate_catalog { "v3" } else { "v2" };
        self.storage_dir
            .join("data")
            .join(user_ns)
            .join("chapters")
            .join(format!("{name}.{revision}.json"))
    }

    pub async fn load_chapter_list_cache(
        &self,
        user_ns: &str,
        toc_url: &str,
    ) -> Result<Option<Vec<BookChapter>>, AppError> {
        let path = self.chapter_list_cache_path(user_ns, toc_url);
        if !path.exists() {
            return Ok(None);
        }
        let data = fs::read_to_string(&path)
            .await
            .map_err(|e| AppError::Internal(e.into()))?;
        let list: Vec<BookChapter> =
            serde_json::from_str(&data).map_err(|e| AppError::BadRequest(e.to_string()))?;
        Ok(Some(list))
    }

    pub async fn save_chapter_list_cache(
        &self,
        user_ns: &str,
        toc_url: &str,
        chapters: &Vec<BookChapter>,
    ) -> Result<(), AppError> {
        let path = self.chapter_list_cache_path(user_ns, toc_url);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .await
                .map_err(|e| AppError::Internal(e.into()))?;
        }
        let data =
            serde_json::to_string(chapters).map_err(|e| AppError::BadRequest(e.to_string()))?;
        fs::write(&path, data)
            .await
            .map_err(|e| AppError::Internal(e.into()))?;
        Ok(())
    }

    pub async fn append_chapter_list_cache(
        &self,
        user_ns: &str,
        toc_url: &str,
        new_chapters: &Vec<BookChapter>,
    ) -> Result<Vec<BookChapter>, AppError> {
        let mut existing = self
            .load_chapter_list_cache(user_ns, toc_url)
            .await?
            .unwrap_or_default();
        let start_index = existing.len() as i32;
        for (i, ch) in new_chapters.iter().enumerate() {
            let mut ch = ch.clone();
            ch.index = start_index + i as i32;
            existing.push(ch);
        }
        self.save_chapter_list_cache(user_ns, toc_url, &existing)
            .await?;
        Ok(existing)
    }

    pub async fn delete_chapter_list_cache(
        &self,
        user_ns: &str,
        toc_url: &str,
    ) -> Result<(), AppError> {
        let path = self.chapter_list_cache_path(user_ns, toc_url);
        if path.exists() {
            fs::remove_file(&path)
                .await
                .map_err(|e| AppError::Internal(e.into()))?;
        }
        Ok(())
    }

    async fn clear_book_related_cache(&self, user_ns: &str, book: &Book) -> Result<(), AppError> {
        let mut chapter_urls = Vec::new();
        if let Some(toc_url) = book.toc_url.as_deref().filter(|url| !url.is_empty()) {
            if let Ok(Some(chapters)) = self.load_chapter_list_cache(user_ns, toc_url).await {
                chapter_urls.extend(chapters.into_iter().map(|chapter| chapter.url));
            }
        }
        if !book.book_url.is_empty() {
            let _ = self.delete_book_cache(user_ns, &book.book_url).await;
            let _ = self
                .delete_book_sources_cache(user_ns, &book.book_url)
                .await;
            let _ = self
                .delete_chapter_list_cache(user_ns, &book.book_url)
                .await;
        }
        if let Some(cover_url) = book.cover_url.as_deref() {
            let _ = self.delete_cover_cache("public", cover_url).await;
        }
        if let Some(cover_url) = book.custom_cover_url.as_deref() {
            if book.cover_url.as_deref() != Some(cover_url) {
                let _ = self.delete_cover_cache("public", cover_url).await;
            }
        }
        if let Some(toc_url) = &book.toc_url {
            if !toc_url.is_empty() {
                let _ = self.delete_chapter_list_cache(user_ns, toc_url).await;
            }
        }
        self.clear_book_variables(&book.book_url, &chapter_urls);
        Ok(())
    }
}

/// 取日志里第一条可用于诊断的脚本错误。
///
/// `[正文 JS]` / `[目录 JS]` 这类**规则级**前缀必须包含在内: 书源脚本抛出的
/// 业务提示(如"本书源为VIP专属内容")正是通过它们传出来的, 漏掉会让正文
/// 解析失败退化成静默的空串。
fn first_js_error(logs: &[String]) -> Option<&str> {
    logs.iter()
        .find(|line| {
            ["[脚本]", "[jsLib]", "[http]", "[正文 JS]", "[目录 JS]", "[详情 JS]"]
                .iter()
                .any(|prefix| line.starts_with(prefix))
        })
        .map(String::as_str)
}

/// 从日志里提取脚本抛出的**用户可读提示**（去掉 `[xxx JS] JS 异常:` 一类的
/// 内部前缀），供界面直接展示。
fn first_js_message(logs: &[String]) -> Option<String> {
    let raw = first_js_error(logs)?;
    let text = raw
        .split_once("JS 异常:")
        .map(|(_, rest)| rest)
        .unwrap_or(raw)
        .trim();
    if text.is_empty() {
        None
    } else {
        Some(text.to_string())
    }
}

/// 执行 `loginCheckJs`。语义与 legado 一致: 脚本的返回值即"处理后的响应"
/// (返回 `result` 表示放行, 返回新响应对象表示替换)。脚本里的
/// `source.login()` 会被记录, 由调用方用已存登录信息重跑登录。
/// 返回 `(响应, 是否请求了重新登录)`。
fn apply_login_check_js(source: &BookSource, res: FetchResponse) -> Result<(FetchResponse, bool), AppError> {
    let Some(script) = source
        .login_check_js
        .as_deref()
        .filter(|script| !script.trim().is_empty())
    else {
        return Ok((res, false));
    };

    with_source(source, || {
        crate::parser::js::take_login_effects();
        let str_response = StrResponse::from(res.clone());
        let response_json = serde_json::to_value(&str_response).unwrap_or_else(|_| json!({}));
        // legado: loginCheckJs 里 `java.getStrResponse()` 返回当前响应。
        crate::parser::js::set_last_response(&response_json.to_string());
        let mut bindings = HashMap::new();
        // legado 的 `result` 是 StrResponse 对象: 脚本按方法调用
        // (`result.code()`/`result.body()`), 必须用 Resp() 包装。
        bindings.insert("result".to_string(), crate::parser::js::resp_binding(&response_json));
        let login_requested = |()| crate::parser::js::take_login_effects().login;
        match eval_js_with_bindings(script, &res.body, &res.url, &bindings) {
            Ok(output) if !output.trim().is_empty() => {
                // 识别"替换响应"：带宿主信封标记，或含可辨识的响应字段
                // (code/url/headers/isSuccessful)。纯书源数据(如详情 JSON)
                // 不能被当信封 —— 否则会把正常正文替换成空响应(HTTP 0)，
                // 表现为"目录没有了但也不报错"。
                if let Some(next) = parse_resp_envelope(&output) {
                    Ok((FetchResponse::from(next), login_requested(())))
                } else {
                    Ok((
                        FetchResponse {
                            body: output,
                            ..res
                        },
                        login_requested(()),
                    ))
                }
            }
            Ok(_) => Ok((res, login_requested(()))),
            Err(err) => {
                // 校验脚本本身出错时不能把整个搜索/详情链路打断:
                // 绝大多数源在没有会话时只是想让脚本原样放行。
                tracing::warn!(
                    "loginCheckJs failed for {}: {:?}",
                    source.book_source_name,
                    err
                );
                Ok((res, false))
            }
        }
    })
}

/// 识别 loginCheckJs 返回的"替换响应"信封。
///
/// 判定条件(满足其一):
/// - 宿主 `Resp` 对象序列化出的信封(带 `__respEnvelope` 标记)
/// - 显式含 `code` 且是有效 HTTP 状态码(>=100)，同时带 `body`/`url` 之一
///
/// 纯书源数据(如详情 JSON `{name,author,bookUrl,...}`)两条都不满足，
/// 因此不会被误当信封把正常正文替换成空响应。
fn parse_resp_envelope(output: &str) -> Option<StrResponse> {
    let value: serde_json::Value = serde_json::from_str(output).ok()?;
    let object = value.as_object()?;
    let marked = object.get("__respEnvelope").and_then(serde_json::Value::as_bool).unwrap_or(false);
    let has_code = object
        .get("code")
        .and_then(serde_json::Value::as_u64)
        .is_some_and(|code| code >= 100);
    if !marked && !(has_code && (object.contains_key("body") || object.contains_key("url"))) {
        return None;
    }
    serde_json::from_value(value).ok()
}

fn parse_explore_kinds(
    source: &BookSource,
    info_map: &HashMap<String, String>,
) -> Result<Vec<ExploreKind>, AppError> {
    let Some(raw) = source
        .explore_url
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        return Ok(Vec::new());
    };

    let text = if let Some(script) = raw.strip_prefix("@js:") {
        crate::parser::js::eval_explore_kinds(script, &source.book_source_url, info_map)
            .map_err(|err| AppError::BadRequest(format!("发现规则执行失败: {err}")))?
    } else if let Some(script) = raw
        .strip_prefix("<js>")
        .and_then(|value| value.strip_suffix("</js>"))
    {
        crate::parser::js::eval_explore_kinds(script, &source.book_source_url, info_map)
            .map_err(|err| AppError::BadRequest(format!("发现规则执行失败: {err}")))?
    } else {
        raw.to_string()
    };

    for json_text in [&text, &normalize_relaxed_explore_json(&text)] {
        if let Ok(kinds) = serde_json::from_str::<Vec<ExploreKind>>(json_text) {
            let mut kinds: Vec<ExploreKind> = kinds
                .into_iter()
                .filter(|kind| !kind.title.trim().is_empty())
                .collect();
            resolve_explore_view_names(source, &mut kinds, info_map);
            return Ok(kinds);
        }
    }

    // 无前缀的裸 JS 对象字面量（这一类写法）：
    //   `[{'title':'我的书架','url':'...','style':{layout_flexGrow: 1}}, ...]`
    // 单引号 + 无引号键名，不是合法 JSON，`serde_json` 直接拒收。
    //
    // legado 用 GSON 解析，而 GSON 的 JsonReader 默认宽松（接受单引号与无引号
    // 键名），所以手机端能显示。这里改为交给 rquickjs 求值 —— JS 引擎天生就懂
    // JS 字面量，比拼字符串规则稳。
    //
    // 只在严格解析失败后尝试：正常书源走不到这里。求值失败也不报错，继续往下
    // 走纯文本切分，保持原有兜底行为。
    if looks_like_js_array_literal(&text) {
        if let Ok(escaped) = crate::parser::js::eval_explore_kinds(
            &text,
            &source.book_source_url,
            info_map,
        ) {
            if let Ok(kinds) = serde_json::from_str::<Vec<ExploreKind>>(escaped.trim()) {
                let mut kinds: Vec<ExploreKind> = kinds
                    .into_iter()
                    .filter(|kind| !kind.title.trim().is_empty())
                    .collect();
                if !kinds.is_empty() {
                    resolve_explore_view_names(source, &mut kinds, info_map);
                    return Ok(kinds);
                }
            }
        }
    }

    let splitter = regex::Regex::new(r"(&&|\n)+").unwrap();
    Ok(splitter
        .split(&text)
        .filter_map(|item| {
            let item = item.trim();
            if item.is_empty() {
                return None;
            }
            let mut parts = item.splitn(2, "::");
            let title = parts.next().unwrap_or_default().trim();
            if title.is_empty() {
                return None;
            }
            let url = parts
                .next()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string);
            Some(ExploreKind {
                title: title.to_string(),
                url,
                ..ExploreKind::default()
            })
        })
        .collect())
}

/// 填充交互控件的显示名（legado `ExploreKind.viewName`）。
///
/// 只处理非 url 型控件：url 型就是普通分类按钮，标题即文字。求值失败时
/// 保持 `view_name` 为 None，前端回退用 `title` —— 显示名不该成为整页失败的
/// 原因。
fn resolve_explore_view_names(
    source: &BookSource,
    kinds: &mut [ExploreKind],
    info_map: &HashMap<String, String>,
) {
    for kind in kinds.iter_mut() {
        if kind.kind_type() == crate::model::book_source::ExploreKindType::Url {
            continue;
        }
        let Some(raw) = kind.view_name.as_deref().filter(|v| !v.trim().is_empty()) else {
            continue;
        };
        kind.view_name = crate::parser::js::eval_explore_view_name(
            raw,
            &source.book_source_url,
            info_map,
        );
    }
}

/// 把"宽松 JSON"（GSON 能读、`serde_json` 读不了）归一化到严格 JSON。
///
/// 目前处理两件事：
///
/// 1. `<` / `>` 在字符串外当作 `{` / `}`（部分书源用尖括号代替花括号）。
/// 2. **字符串内的裸控制字符按转义序列改写**。这一条是刚需：书源的
///    `exploreUrl` 常把多行 JS 内嵌进 `{{ ... }}`，像这样
///
///    ```text
///    "url":"/popular/{{\n\tvar pjstr='index.html';\n\tpjstr}}"
///    ```
///
///    JSON 规范不允许字符串里出现裸换行/制表符，`serde_json` 会直接报
///    「control character found while parsing a string」；而 GSON 的宽松
///    reader 接受，所以手机端能正常显示分类。改写后字符串内容不变
///    （`\n` 仍是换行），只是变成合法 JSON —— 后续 URL 分析照常处理多行 `{{ }}`。
fn normalize_relaxed_explore_json(text: &str) -> String {
    let mut normalized = String::with_capacity(text.len());
    let mut in_string = false;
    let mut quote = '\0';
    let mut escaped = false;

    for ch in text.chars() {
        if in_string {
            if escaped {
                escaped = false;
                normalized.push(ch);
                continue;
            }
            match ch {
                '\\' => {
                    escaped = true;
                    normalized.push(ch);
                }
                // 裸控制字符：改写成 JSON 转义序列（内容等价，语法合法）。
                '\n' => normalized.push_str("\\n"),
                '\r' => normalized.push_str("\\r"),
                '\t' => normalized.push_str("\\t"),
                c if (c as u32) < 0x20 => {
                    normalized.push_str(&format!("\\u{:04x}", c as u32));
                }
                _ => {
                    if ch == quote {
                        in_string = false;
                    }
                    normalized.push(ch);
                }
            }
            continue;
        }

        match ch {
            '"' | '\'' => {
                in_string = true;
                quote = ch;
                normalized.push(ch);
            }
            '<' => normalized.push('{'),
            '>' => normalized.push('}'),
            _ => normalized.push(ch),
        }
    }

    normalized
}

/// 判断文本是否"看起来像 JS 数组里的对象字面量"。
///
/// 用来决定要不要动用 JS 引擎去宽松解析。判据刻意保守，只认这一种形态：
/// 整体是 `[...]`，且开头就出现 `{` 与一个带引号的键 —— 即
/// `[{'title':...}]`。普通文本分类规则（`标题::url&&标题2::url2`）不会命中。
///
/// 不命中时保持原有行为（严格 JSON → 宽松 JSON → 纯文本切分），所以这里漏判
/// 只是少了个优化，不会引入回归。
fn looks_like_js_array_literal(text: &str) -> bool {
    let trimmed = text.trim();
    if !(trimmed.starts_with('[') && trimmed.ends_with(']')) {
        return false;
    }
    let head = &trimmed[1..trimmed.len().min(64)];
    let Some(open) = head.find('{') else {
        return false;
    };
    // `{` 之后应当紧跟（可有空白）一个被引号包起来的键名。
    let after = head[open + 1..].trim_start();
    let Some(quote) = after.chars().next() else {
        return false;
    };
    if quote != '\'' && quote != '"' {
        return false;
    }
    after[quote.len_utf8()..].contains(quote)
}

fn parse_window_rate(rate: &str) -> Option<(usize, u64)> {
    let (limit, window) = rate.split_once('/')?;
    let limit = limit.trim().parse().ok()?;
    let window = window.trim().parse().ok()?;
    if limit == 0 || window == 0 {
        return None;
    }
    Some((limit, window))
}

fn should_follow_content_page(chapter_url: &str, current_url: &str, next_url: &str) -> bool {
    let next_url = strip_fragment(next_url);
    let current_url = strip_fragment(current_url);
    let chapter_url = strip_fragment(chapter_url);

    if next_url == current_url || next_url == chapter_url {
        return false;
    }

    match (
        url::Url::parse(chapter_url),
        url::Url::parse(current_url),
        url::Url::parse(next_url),
    ) {
        (Ok(chapter), Ok(current), Ok(next)) => {
            if chapter.scheme() != next.scheme()
                || chapter.host_str() != next.host_str()
                || chapter.port_or_known_default() != next.port_or_known_default()
            {
                return false;
            }

            let chapter_exact = content_path_exact_base(chapter.path());
            let current_exact = content_path_exact_base(current.path());
            let next_exact = content_path_exact_base(next.path());
            let next_page_base = content_path_page_base(next.path());

            next_exact == chapter_exact
                || next_exact == current_exact
                || next_page_base == chapter_exact
                || next_page_base == current_exact
        }
        _ => {
            let chapter_exact = content_path_exact_base(chapter_url);
            let current_exact = content_path_exact_base(current_url);
            let next_exact = content_path_exact_base(next_url);
            let next_page_base = content_path_page_base(next_url);

            next_exact == chapter_exact
                || next_exact == current_exact
                || next_page_base == chapter_exact
                || next_page_base == current_exact
        }
    }
}

fn strip_fragment(url: &str) -> &str {
    url.split_once('#').map(|(head, _)| head).unwrap_or(url)
}

fn content_path_exact_base(path: &str) -> String {
    content_path_base(path, false)
}

fn content_path_page_base(path: &str) -> String {
    content_path_base(path, true)
}

fn content_path_base(path: &str, strip_page_suffix: bool) -> String {
    let (dir, file) = path.rsplit_once('/').unwrap_or(("", path));
    let (stem, _ext) = file.rsplit_once('.').unwrap_or((file, ""));
    let stem = if strip_page_suffix {
        strip_page_suffix_from_stem(stem)
    } else {
        stem
    };
    if dir.is_empty() {
        stem.to_string()
    } else {
        format!("{dir}/{stem}")
    }
}

fn strip_page_suffix_from_stem(stem: &str) -> &str {
    for sep in ['-', '_'] {
        if let Some(idx) = stem.rfind(sep) {
            let suffix = &stem[idx + sep.len_utf8()..];
            if !suffix.is_empty()
                && suffix.chars().all(|ch| ch.is_ascii_digit())
                && suffix
                    .parse::<usize>()
                    .map(|page| page >= 2)
                    .unwrap_or(false)
            {
                return &stem[..idx];
            }
        }
    }
    stem
}

fn is_local_book(book: &Book) -> bool {
    matches!(book.origin.trim(), "local-txt" | "local-epub" | "local-pdf")
        || book.book_url.trim().starts_with("local-txt:")
        || book.book_url.trim().starts_with("local-epub:")
        || is_local_pdf_origin(&book.origin)
        || is_local_pdf_url(&book.book_url)
}

fn books_match_for_save(existing: &Book, incoming: &Book) -> bool {
    if existing.book_url == incoming.book_url {
        return true;
    }
    if is_local_book(existing) || is_local_book(incoming) {
        return false;
    }
    !existing.name.is_empty()
        && same_remote_book_identity(
            &existing.name,
            &existing.author,
            &incoming.name,
            &incoming.author,
        )
}

fn same_remote_book_identity(
    left_name: &str,
    left_author: &str,
    right_name: &str,
    right_author: &str,
) -> bool {
    normalize_book_name(left_name) == normalize_book_name(right_name)
        && normalize_book_author(left_author) == normalize_book_author(right_author)
}

fn normalize_book_name(value: &str) -> String {
    value
        .chars()
        .filter(|ch| !ch.is_whitespace())
        .flat_map(char::to_lowercase)
        .collect()
}

fn normalize_book_author(value: &str) -> String {
    let compact = normalize_book_name(value);
    compact
        .strip_prefix("作者：")
        .or_else(|| compact.strip_prefix("作者:"))
        .or_else(|| compact.strip_prefix("作者"))
        .unwrap_or(&compact)
        .trim_start_matches(['：', ':'])
        .to_string()
}

fn ensure_current_source_candidate(book: &mut Book) {
    if is_local_book(book) || book.origin.trim().is_empty() || book.book_url.trim().is_empty() {
        return;
    }
    let current = search_candidate_from_book(book);
    let candidates = book.source_candidates.get_or_insert_with(Vec::new);
    merge_source_candidate(candidates, current);
}

fn merge_book_source_candidates(existing: &Book, incoming: &mut Book) {
    if is_local_book(existing) || is_local_book(incoming) {
        return;
    }

    let mut candidates = Vec::new();
    if !existing.origin.trim().is_empty() && !existing.book_url.trim().is_empty() {
        merge_source_candidate(&mut candidates, search_candidate_from_book(existing));
    }
    for candidate in existing.source_candidates.clone().unwrap_or_default() {
        merge_source_candidate(&mut candidates, candidate);
    }
    for candidate in incoming.source_candidates.clone().unwrap_or_default() {
        merge_source_candidate(&mut candidates, candidate);
    }
    if !incoming.origin.trim().is_empty() && !incoming.book_url.trim().is_empty() {
        merge_source_candidate(&mut candidates, search_candidate_from_book(incoming));
    }
    incoming.source_candidates = (!candidates.is_empty()).then_some(candidates);
}

fn merge_source_candidate(candidates: &mut Vec<SearchBook>, candidate: SearchBook) {
    if candidate.origin.trim().is_empty() || candidate.book_url.trim().is_empty() {
        return;
    }
    let origin = normalize_source_url(&candidate.origin);
    if let Some(index) = candidates
        .iter()
        .position(|item| normalize_source_url(&item.origin) == origin && item.book_url == candidate.book_url)
    {
        candidates[index] = candidate;
    } else {
        candidates.push(candidate);
    }
}

fn search_candidate_from_book(book: &Book) -> SearchBook {
    SearchBook {
        name: book.name.clone(),
        author: book.author.clone(),
        book_url: book.book_url.clone(),
        origin: book.origin.clone(),
        cover_url: book.cover_url.clone(),
        intro: book.intro.clone(),
        kind: book.kind.clone(),
        last_chapter: book.latest_chapter_title.clone(),
        update_time: book.update_time.clone(),
        word_count: book.word_count.clone(),
        book_source_urls: None,
    }
}

fn books_match_for_delete(existing: &Book, target: &Book) -> bool {
    if !target.book_url.is_empty() && existing.book_url == target.book_url {
        return true;
    }
    if is_local_book(existing) || is_local_book(target) {
        return false;
    }
    !target.name.is_empty()
        && !target.author.is_empty()
        && existing.name == target.name
        && existing.author == target.author
}

fn apply_book_read_config_override(book: &mut Book) {
    if let Some(use_replace_rule) = crate::parser::js::book_read_config_override(&book.origin, &book.book_url)
        .and_then(|config| config.get("useReplaceRule").and_then(serde_json::Value::as_bool))
    {
        book.read_config.get_or_insert_with(Default::default).use_replace_rule = Some(use_replace_rule);
    }
}

fn merge_book_read_config(existing: &Book, incoming: &mut Book) {
    let Some(existing_config) = &existing.read_config else { return; };
    let config = incoming.read_config.get_or_insert_with(Default::default);
    if config.use_replace_rule.is_none() {
        config.use_replace_rule = existing_config.use_replace_rule;
    }
    for (key, value) in &existing_config.extra {
        config.extra.entry(key.clone()).or_insert_with(|| value.clone());
    }
}

fn sanitize_book_urls(book: &mut Book) {
    book.book_url = repair_encoded_url(&book.book_url);
    book.origin = normalize_source_url(&book.origin);
    // 空串目录地址归一成 None。
    //
    // Legado 的 `Book.tocUrl` 是 Kotlin 非空 `String`，从未取过详情的书在备份里
    // 就是 `"tocUrl": ""`。这边若原样存成 `Some("")`，前端 `b.tocUrl` 是 Truthy
    // 之外的假值但**字段存在**，各类"缺 tocUrl 就去取详情"的判断(`is_none()` /
    // `!tocUrl`)会分叉：有的地方判 `is_none()`（为假，于是不补），有的判空串
    // （为真，于是去补）。表现为同一本书有时能打开、有时一直转圈。
    // 统一收口成 None，让"没有目录地址"只有一种表示。
    if let Some(toc_url) = &book.toc_url {
        let repaired = repair_encoded_url(toc_url);
        book.toc_url = if repaired.trim().is_empty() { None } else { Some(repaired) };
    }
    // 封面地址只 trim, 不做百分号反转义。
    //
    // `repair_encoded_url` 会把 `%3D` 换成 `=`。页面地址上这没问题(站点能容忍),
    // 但封面地址里的 `x-signature=…%3D` 是被站点按原始串校验的签名: 解码后回源
    // 直接 403。而封面缓存是**按 URL** 造键的, 一旦地址被改写, 搜索页缓存的那份
    // 就再也命中不了 —— 表现正是「搜索时有封面, 加入书架后没封面」。
    if let Some(cover_url) = &book.cover_url {
        book.cover_url = Some(normalize_source_url(cover_url));
    }
}

fn recover_bookshelf_entries(data: &str) -> Option<Vec<Book>> {
    let mut recovered = Vec::new();
    let mut seen = HashSet::new();
    let stream = serde_json::Deserializer::from_str(data).into_iter::<serde_json::Value>();

    for item in stream {
        let value = match item {
            Ok(value) => value,
            Err(err) => {
                tracing::warn!("bookshelf recovery stream stopped: {}", err);
                break;
            }
        };
        match value {
            serde_json::Value::Array(items) => {
                for entry in items {
                    if let Ok(book) = serde_json::from_value::<Book>(entry) {
                        push_recovered_book(&mut recovered, &mut seen, book);
                    }
                }
            }
            serde_json::Value::Object(_) => {
                if let Ok(book) = serde_json::from_value::<Book>(value) {
                    push_recovered_book(&mut recovered, &mut seen, book);
                }
            }
            _ => {}
        }
    }

    if recovered.is_empty() {
        None
    } else {
        Some(recovered)
    }
}

fn push_recovered_book(recovered: &mut Vec<Book>, seen: &mut HashSet<String>, mut book: Book) {
    sanitize_book_urls(&mut book);
    let key = format!("{}::{}", book.book_url, book.origin);
    if seen.insert(key) {
        recovered.push(book);
    }
}

fn file_ext_from_url(url: &str) -> Option<String> {
    let url = url.split('?').next().unwrap_or(url);
    let url = url.split('#').next().unwrap_or(url);
    let pos = url.rfind('.')?;
    let ext = &url[pos + 1..];
    if ext.len() > 0 && ext.len() <= 8 {
        Some(ext.to_ascii_lowercase())
    } else {
        None
    }
}

/// 一个「没认出是哪种图片」的 Content-Type: 空、octet-stream, 或根本不是图片。
fn is_unknown_image_type(content_type: &str) -> bool {
    let mime = content_type
        .split(';')
        .next()
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase();
    !matches!(
        mime.as_str(),
        "image/png"
            | "image/jpeg"
            | "image/jpg"
            | "image/webp"
            | "image/gif"
            | "image/bmp"
            | "image/avif"
            | "image/svg+xml"
    )
}

/// 按文件头认图片格式。只覆盖封面实际会用到的几种。
fn sniff_image_content_type(bytes: &[u8]) -> Option<String> {
    let mime = if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        "image/jpeg"
    } else if bytes.starts_with(&[0x89, b'P', b'N', b'G']) {
        "image/png"
    } else if bytes.starts_with(b"GIF8") {
        "image/gif"
    } else if bytes.len() > 12 && bytes.starts_with(b"RIFF") && &bytes[8..12] == b"WEBP" {
        "image/webp"
    } else if bytes.starts_with(b"BM") {
        "image/bmp"
    } else if bytes.len() > 12 && &bytes[4..8] == b"ftyp" && &bytes[8..12] == b"avif" {
        "image/avif"
    } else {
        return None;
    };
    Some(mime.to_string())
}

/// 封面落盘用的扩展名。
///
/// 以服务端给的 `Content-Type` 为准: 一类图床的地址形如 `…:1200:0.image`,
/// 按 URL 取后缀会落下 `.image` —— 文件本身是正常 JPEG, 只是系统没有这个后缀的
/// 缩略图/打开方式, 在资源管理器里看着像「空白文件」。`Content-Type` 认不出时
/// 才回退到 URL 后缀, 再兜底 `img`(不写 `image`, 免得继续冒充未知类型)。
fn cover_file_extension(url: &str, content_type: &str) -> String {
    let mime = content_type
        .split(';')
        .next()
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase();
    match mime.as_str() {
        "image/png" => "png".to_string(),
        "image/jpeg" | "image/jpg" => "jpg".to_string(),
        "image/webp" => "webp".to_string(),
        "image/gif" => "gif".to_string(),
        "image/bmp" => "bmp".to_string(),
        "image/avif" => "avif".to_string(),
        "image/svg+xml" => "svg".to_string(),
        _ => file_ext_from_url(url).unwrap_or_else(|| "img".to_string()),
    }
}

/// 把封面地址里裸的 `=` 重新转义成 `%3D`, 用于找回历史上被反转义过的缓存。
///
/// 只处理**查询串里**的 `=`, 路径、fragment 和参数名中间的 `=` 一概不动: 前者
/// 才是签名载荷, 后者一旦改动整条地址就废了。找不到可回退的写法时返回 `None`。
fn reencode_cover_signature(url: &str) -> Option<String> {
    let (before_fragment, fragment) = match url.split_once('#') {
        Some((head, tail)) => (head, Some(tail)),
        None => (url, None),
    };
    let (path, query) = before_fragment.split_once('?')?;
    let rebuilt = query
        .split('&')
        .map(|pair| match pair.split_once('=') {
            // 只转义值里的 `=`(签名载荷), 键与键之间的分隔符不动。
            Some((key, value)) if value.contains('=') => {
                format!("{key}={}", value.replace('=', "%3D"))
            }
            _ => pair.to_string(),
        })
        .collect::<Vec<_>>()
        .join("&");
    if rebuilt == query {
        return None;
    }
    let mut out = format!("{path}?{rebuilt}");
    if let Some(fragment) = fragment {
        out.push('#');
        out.push_str(fragment);
    }
    Some(out)
}

fn content_type_from_ext(ext: &str) -> String {
    match ext {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "webp" => "image/webp",
        "gif" => "image/gif",
        "bmp" => "image/bmp",
        "svg" => "image/svg+xml",
        _ => "application/octet-stream",
    }
    .to_string()
}

/// 读某条封面 URL 映射里记的内容文件名（读不到就 `None`）。
///
/// 用于"重写映射后清掉旧内容文件"：内容文件是内容寻址的，重写后旧文件
/// 不再被这条 URL 引用，但可能仍被别的 URL 共享，所以调用方还要再查一次引用。
async fn cover_mapping_file(cover_dir: &Path, url: &str) -> Option<String> {
    let path = cover_dir.join("urls").join(format!("{}.json", md5_hex(url)));
    let raw = fs::read_to_string(path).await.ok()?;
    serde_json::from_str::<serde_json::Value>(&raw)
        .ok()?
        .get("file")
        .and_then(serde_json::Value::as_str)
        .map(str::to_string)
}

/// 按 mtime 回收 `urls/` 下过期的封面映射文件，`pinned` 里的文件名永不回收。
async fn prune_stale_cover_mappings(
    cover_dir: &Path,
    max_age: Duration,
    pinned: &HashSet<String>,
) -> Result<usize, AppError> {
    let urls_dir = cover_dir.join("urls");
    let mut entries = match fs::read_dir(&urls_dir).await {
        Ok(entries) => entries,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(0),
        Err(err) => return Err(AppError::Internal(err.into())),
    };
    let now = SystemTime::now();
    let mut removed = 0usize;
    while let Some(entry) = entries.next_entry().await.map_err(|e| AppError::Internal(e.into()))? {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        // 书架引用的映射：无论多旧都留着。
        if path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| pinned.contains(name))
        {
            continue;
        }
        let stale = entry
            .metadata()
            .await
            .ok()
            .and_then(|metadata| metadata.modified().ok())
            .and_then(|modified| now.duration_since(modified).ok())
            .is_some_and(|age| age >= max_age);
        if stale && fs::remove_file(path).await.is_ok() {
            removed += 1;
        }
    }
    Ok(removed)
}

/// 回收没有任何映射引用的内容文件; 同样只看超过 `max_age` 未更新的文件。
async fn remove_orphan_cover_files(cover_dir: &Path, max_age: Duration) -> Result<usize, AppError> {
    let referenced = collect_referenced_cover_files(cover_dir).await?;
    let mut entries = match fs::read_dir(cover_dir).await {
        Ok(entries) => entries,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(0),
        Err(err) => return Err(AppError::Internal(err.into())),
    };
    let now = SystemTime::now();
    let mut removed = 0usize;
    while let Some(entry) = entries.next_entry().await.map_err(|e| AppError::Internal(e.into()))? {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let stale = entry
            .metadata()
            .await
            .ok()
            .and_then(|metadata| metadata.modified().ok())
            .and_then(|modified| now.duration_since(modified).ok())
            .is_some_and(|age| age >= max_age);
        if !stale {
            continue;
        }
        let Some(file_name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        if referenced.contains(file_name) {
            continue;
        }
        if fs::remove_file(path).await.is_ok() {
            removed += 1;
        }
    }
    Ok(removed)
}

async fn collect_referenced_cover_files(
    cover_dir: &Path,
) -> Result<HashSet<String>, AppError> {
    let urls_dir = cover_dir.join("urls");
    let mut entries = match fs::read_dir(&urls_dir).await {
        Ok(entries) => entries,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(HashSet::new()),
        Err(err) => return Err(AppError::Internal(err.into())),
    };
    let mut referenced = HashSet::new();
    while let Some(entry) = entries.next_entry().await.map_err(|e| AppError::Internal(e.into()))? {
        if entry
            .path()
            .extension()
            .and_then(|extension| extension.to_str())
            != Some("json")
        {
            continue;
        }
        if let Ok(mapped) = fs::read_to_string(entry.path()).await {
            if let Some(file) = serde_json::from_str::<serde_json::Value>(&mapped)
                .ok()
                .and_then(|value| {
                    value
                        .get("file")
                        .and_then(serde_json::Value::as_str)
                        .map(str::to_string)
                })
            {
                referenced.insert(file);
            }
        }
    }
    Ok(referenced)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 阶段枚举的序列化结果就是前端 `loadStage` 收到的字符串,
    /// 改动这里会直接破坏阅读页的阶段文案, 所以固定住。
    #[test]
    fn content_load_stage_serializes_as_expected_strings() {
        let to_json = |stage: ContentLoadStage| serde_json::to_string(&stage).unwrap();
        assert_eq!(to_json(ContentLoadStage::Fetching), "\"fetching\"");
        assert_eq!(to_json(ContentLoadStage::Parsing), "\"parsing\"");
        assert_eq!(to_json(ContentLoadStage::Caching), "\"caching\"");
    }

    /// 书架落盘必须原子替换: 并发读者永远看不到半截 JSON。
    ///
    /// issue #19 的退出路径改成"发起写入即放行导航"之后, `save_book_progress`
    /// 会与书架页的读取真正并发。旧的 `fs::write` 先截断再写, 夹在中间的读取
    /// 会拿到空串, 触发 `recover_bookshelf_entries` 恢复分支, 甚至把恢复结果
    /// 回写覆盖掉正在写的那一次。
    #[tokio::test]
    async fn bookshelf_write_never_exposes_a_partial_file() {
        let storage_dir = std::env::temp_dir().join(format!(
            "reader-atomic-bookshelf-{}",
            uuid::Uuid::new_v4().simple()
        ));
        let service = BookService::new(
            HttpClient::new(15, None).unwrap(),
            RuleEngine::new().unwrap(),
            FileCache::new(storage_dir.join("cache")),
            storage_dir.to_str().unwrap(),
        );
        let book = Book {
            name: "原子写入".to_string(),
            author: "测试".to_string(),
            origin: "https://example.com".to_string(),
            book_url: "https://example.com/book/1".to_string(),
            ..Default::default()
        };
        service
            .save_book("default", book.clone())
            .await
            .unwrap();

        let path = storage_dir
            .join("data")
            .join("default")
            .join("bookshelf.json");

        // 写入与读取交错跑: 任何一次读取都必须拿到完整 JSON。
        let reader = {
            let path = path.clone();
            tokio::spawn(async move {
                for _ in 0..200 {
                    if let Ok(data) = tokio::fs::read_to_string(&path).await {
                        if data.is_empty() {
                            return Err("读到了空文件");
                        }
                        serde_json::from_str::<Vec<Book>>(&data)
                            .map_err(|_| "读到了半截 JSON")?;
                    }
                    tokio::task::yield_now().await;
                }
                Ok(())
            })
        };
        for _ in 0..20 {
            service.save_book("default", book.clone()).await.unwrap();
        }
        reader.await.unwrap().unwrap();

        // 最终内容是一次完整快照: 能反序列化, 且仍是这本书(save_book 会补齐
        // 自己的字段, 所以不逐字节比对)。
        let persisted: Vec<Book> =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(persisted.len(), 1);
        assert_eq!(persisted[0].book_url, "https://example.com/book/1");
        // 中间文件不能残留。
        let leftovers = std::fs::read_dir(path.parent().unwrap())
            .unwrap()
            .filter_map(Result::ok)
            .filter(|entry| entry.file_name().to_string_lossy().contains(".tmp"))
            .count();
        assert_eq!(leftovers, 0);
        let _ = std::fs::remove_dir_all(storage_dir);
    }

    #[test]
    fn detect_anti_crawler_flags_pc_verification_page() {
        // 某小说站 PC 站反爬: 202 + var buid
        let warnings = detect_anti_crawler(202, "var buid = xxx", &[]);
        assert!(
            warnings.iter().any(|w| w.contains("反爬")),
            "应提示反爬, 实际: {warnings:?}"
        );
    }

    #[tokio::test]
    async fn same_cover_bytes_share_one_content_file() {
        let storage_dir = std::env::temp_dir().join(format!(
            "reader-cover-dedup-{}",
            uuid::Uuid::new_v4().simple()
        ));
        let service = BookService::new(
            HttpClient::new(15, None).unwrap(),
            RuleEngine::new().unwrap(),
            FileCache::new(storage_dir.join("cache")),
            storage_dir.to_str().unwrap(),
        );
        let cover_dir = storage_dir.join("cache").join("public").join("cover");
        let bytes = b"same-image-bytes";

        service
            .store_cover_mapping(
                "https://img.example/a.png",
                bytes,
                "image/png",
                &cover_dir,
            )
            .await
            .unwrap();
        service
            .store_cover_mapping(
                "https://img.example/b.png",
                bytes,
                "image/png",
                &cover_dir,
            )
            .await
            .unwrap();

        let content_files = std::fs::read_dir(&cover_dir)
            .unwrap()
            .filter_map(Result::ok)
            .filter(|entry| entry.path().is_file())
            .count();
        assert_eq!(content_files, 1);
        service
            .delete_cover_cache("public", "https://img.example/a.png")
            .await
            .unwrap();
        assert_eq!(std::fs::read_dir(&cover_dir).unwrap().count(), 2);
        service
            .delete_cover_cache("public", "https://img.example/b.png")
            .await
            .unwrap();
        assert_eq!(std::fs::read_dir(&cover_dir).unwrap().count(), 1);
        let _ = std::fs::remove_dir_all(storage_dir);
    }

    #[tokio::test]
    async fn batch_cover_delete_preserves_shared_content_still_referenced() {
        let storage_dir = std::env::temp_dir().join(format!(
            "reader-cover-batch-delete-{}",
            uuid::Uuid::new_v4().simple()
        ));
        let service = BookService::new(
            HttpClient::new(15, None).unwrap(),
            RuleEngine::new().unwrap(),
            FileCache::new(storage_dir.join("cache")),
            storage_dir.to_str().unwrap(),
        );
        let cover_dir = storage_dir.join("cache").join("public").join("cover");
        let shared = b"shared-cover-bytes";

        for url in [
            "https://img.example/a.png",
            "https://img.example/b.png",
            "https://img.example/keep.png",
        ] {
            service
                .store_cover_mapping(url, shared, "image/png", &cover_dir)
                .await
                .unwrap();
        }
        service
            .store_cover_mapping(
                "https://img.example/unique.png",
                b"unique-cover-bytes",
                "image/png",
                &cover_dir,
            )
            .await
            .unwrap();

        service
            .delete_cover_caches(
                "public",
                &[
                    "https://img.example/a.png".to_string(),
                    "https://img.example/b.png".to_string(),
                    "https://img.example/unique.png".to_string(),
                ],
            )
            .await
            .unwrap();

        assert_eq!(std::fs::read_dir(cover_dir.join("urls")).unwrap().count(), 1);
        let content_files: Vec<_> = std::fs::read_dir(&cover_dir)
            .unwrap()
            .filter_map(Result::ok)
            .filter(|entry| entry.path().is_file())
            .collect();
        assert_eq!(content_files.len(), 1, "仅保留仍被 keep.png 引用的共享内容");

        let (data, content_type) = service
            .get_cover("public", "https://img.example/keep.png")
            .await
            .unwrap();
        assert_eq!(data, shared);
        assert_eq!(content_type, "image/png");

        service
            .delete_cover_cache("public", "https://img.example/keep.png")
            .await
            .unwrap();
        assert!(!content_files[0].path().exists());
        let _ = std::fs::remove_dir_all(storage_dir);
    }

    #[test]
    fn sniffs_image_type_when_the_server_omits_it() {
        // 服务器不回 Content-Type 时, 图床的 `.image` 地址只能靠魔数认。
        assert!(is_unknown_image_type("application/octet-stream"));
        assert!(is_unknown_image_type(""));
        assert!(!is_unknown_image_type("image/jpeg"));
        assert_eq!(
            sniff_image_content_type(&[0xFF, 0xD8, 0xFF, 0xE0]).as_deref(),
            Some("image/jpeg")
        );
        assert_eq!(
            sniff_image_content_type(b"\x89PNG\r\n\x1a\n").as_deref(),
            Some("image/png")
        );
        let mut webp = b"RIFF".to_vec();
        webp.extend_from_slice(&[0, 0, 0, 0]);
        webp.extend_from_slice(b"WEBPVP8 ");
        assert_eq!(
            sniff_image_content_type(&webp).as_deref(),
            Some("image/webp")
        );
        assert_eq!(sniff_image_content_type(b"<html>"), None);
    }

    #[test]
    fn cover_extension_follows_content_type_not_url_suffix() {
        // 图床: 地址以 `…:1200:0.image?…%3D` 结尾, 实际是 JPEG。
        let url = "https://img.example/novel-pic-r/abc~tplv:1200:0.image?x-signature=a%3D";
        assert_eq!(cover_file_extension(url, "image/jpeg"), "jpg");
        assert_eq!(cover_file_extension(url, "image/webp"), "webp");
        assert_eq!(
            cover_file_extension("https://img.example/a.PNG?x=1", "image/png"),
            "png"
        );
        // 认不出的 Content-Type 才回退 URL 后缀, 再兜底 img。
        assert_eq!(
            cover_file_extension("https://img.example/a.gif", "application/octet-stream"),
            "gif"
        );
        assert_eq!(
            cover_file_extension("https://img.example/no-ext", "application/octet-stream"),
            "img"
        );
    }

    #[test]
    fn reencodes_only_the_signature_payload() {
        // 历史上被反转义过的地址: 签名末尾的 `=` 需要还原成 `%3D`。
        assert_eq!(
            reencode_cover_signature(
                "https://img.example/c.jpg?lk=ae4&x-expires=1797656168&x-signature=eizdm5i4RB58HkAj3MkxtkLWJp4="
            )
            .as_deref(),
            Some(
                "https://img.example/c.jpg?lk=ae4&x-expires=1797656168&x-signature=eizdm5i4RB58HkAj3MkxtkLWJp4%3D"
            )
        );
        // 参数名里的 `=` 与不含签名的地址都不改。
        assert_eq!(
            reencode_cover_signature("https://img.example/c.jpg?a=1&b=2"),
            None
        );
        assert_eq!(reencode_cover_signature("https://img.example/c.jpg"), None);
        // fragment 不是查询串, 不参与转义, 且原样保留。
        assert_eq!(
            reencode_cover_signature("https://img.example/c.jpg?sig=ab=#frag=1").as_deref(),
            Some("https://img.example/c.jpg?sig=ab%3D#frag=1")
        );
    }

    #[test]
    fn sanitizing_a_book_keeps_the_cover_signature_intact() {
        // 封面地址里的 `%3D` 是站点按原始串校验的签名, 反转义后回源 403;
        // 而且封面缓存按 URL 造键, 改写会让搜索页已缓存的那份彻底失配。
        let mut book = Book {
            book_url: "https://books.example/1.html".to_string(),
            origin: "https://books.example".to_string(),
            cover_url: Some(
                "https://img.example/c.jpg?x-expires=1&x-signature=eiz%2Bdm5%3D".to_string(),
            ),
            ..Default::default()
        };
        sanitize_book_urls(&mut book);
        assert_eq!(
            book.cover_url.as_deref(),
            Some("https://img.example/c.jpg?x-expires=1&x-signature=eiz%2Bdm5%3D")
        );
    }

    #[tokio::test]
    async fn legacy_signature_mapping_migrates_to_the_shelf_url() {
        let storage_dir = std::env::temp_dir().join(format!(
            "reader-cover-migrate-{}",
            uuid::Uuid::new_v4().simple()
        ));
        let service = BookService::new(
            HttpClient::new(15, None).unwrap(),
            RuleEngine::new().unwrap(),
            FileCache::new(storage_dir.join("cache")),
            storage_dir.to_str().unwrap(),
        );
        let cover_dir = storage_dir.join("cache").join("public").join("cover");
        let original = "https://img.example/c.jpg?x-signature=eiz%3D";
        let escaped = "https://img.example/c.jpg?x-signature=eiz=";
        // 模拟旧版本: 缓存按原始 URL 存, 书架记的是被反转义的地址。
        service
            .store_cover_mapping(original, b"cover-bytes", "image/jpeg", &cover_dir)
            .await
            .unwrap();

        let (data, content_type) = service.get_cover("public", escaped).await.unwrap();
        assert_eq!(data, b"cover-bytes");
        assert_eq!(content_type, "image/jpeg");
        // 映射被搬到书架用的地址下, 旧映射已释放。
        assert!(cover_dir
            .join("urls")
            .join(format!("{}.json", md5_hex(escaped)))
            .exists());
        assert!(!cover_dir
            .join("urls")
            .join(format!("{}.json", md5_hex(original)))
            .exists());
        let _ = std::fs::remove_dir_all(storage_dir);
    }

    #[tokio::test]
    async fn stale_cover_cleanup_keeps_referenced_content() {
        let storage_dir = std::env::temp_dir().join(format!(
            "reader-cover-stale-{}",
            uuid::Uuid::new_v4().simple()
        ));
        let service = BookService::new(
            HttpClient::new(15, None).unwrap(),
            RuleEngine::new().unwrap(),
            FileCache::new(storage_dir.join("cache")),
            storage_dir.to_str().unwrap(),
        );
        let cover_dir = storage_dir.join("cache").join("public").join("cover");
        service
            .store_cover_mapping(
                "https://img.example/keep.png",
                b"keep-bytes",
                "image/png",
                &cover_dir,
            )
            .await
            .unwrap();
        let content_file = std::fs::read_dir(&cover_dir)
            .unwrap()
            .filter_map(Result::ok)
            .find(|entry| entry.path().is_file())
            .unwrap()
            .path();
        let removed = service
            .clean_stale_search_covers(Duration::from_secs(30 * 60))
            .await
            .unwrap();
        assert_eq!(removed, 0);
        assert!(content_file.exists());
        let _ = std::fs::remove_dir_all(storage_dir);
    }

    #[test]
    fn detect_anti_crawler_flags_degraded_405_response() {
        let warnings = detect_anti_crawler(403, "<html></html>", &[]);
        assert!(warnings.iter().any(|w| w.contains("403")));
    }

    #[test]
    fn detect_anti_crawler_ignores_normal_html_200() {
        let warnings = detect_anti_crawler(
            200,
            "<html><body>正常页面内容足够长</body></html>",
            &[("Content-Type".to_string(), "text/html".to_string())],
        );
        assert!(warnings.is_empty(), "正常响应不应有告警: {warnings:?}");
    }

    #[test]
    fn source_cookie_keys_do_not_share_same_domain_sources() {
        let http = HttpClient::new(5, None).unwrap();
        let first = http.source_context("default", "https://a.example.com", None).unwrap();
        let second = http.source_context("default", "https://b.example.com", None).unwrap();
        assert!(!Arc::ptr_eq(&first.session.cookies, &second.session.cookies));
    }

    #[test]
    fn local_pdf_books_are_treated_as_local_books() {
        let book = Book {
            origin: "local-pdf".to_string(),
            book_url: "local-pdf:0123456789abcdef0123456789abcdef".to_string(),
            ..Book::default()
        };
        assert!(is_local_book(&book));
    }

    /// `remove_source_candidates` 的廉价短路不能因 JSON 转义而漏判。
    ///
    /// JSON 里 `"` / `\` 会被写成 `\"` / `\\`, 若只用原始 URL 做子串比较, 含这类
    /// 字符的源地址会被误判成"书架里没有引用"而**错误跳过清理**。
    #[tokio::test]
    async fn escaped_source_url_is_detected_by_removal_shortcut() {
        let storage_dir = std::env::temp_dir().join(format!(
            "reader-rust-remove-candidates-{}",
            uuid::Uuid::new_v4()
        ));
        let service = BookService::new(
            HttpClient::new(5, None).unwrap(),
            RuleEngine::new().unwrap(),
            FileCache::new(storage_dir.join("cache")),
            storage_dir.to_str().unwrap(),
        );

        // 含引号的源地址: 写进 JSON 后会出现 `\"` 转义。
        let awkward_origin = "https://a.example.com/\"quoted\"";
        let book = Book {
            book_url: "https://a.example.com/book/1".to_string(),
            name: "书".to_string(),
            author: "作者".to_string(),
            source_candidates: Some(vec![crate::model::search::SearchBook {
                origin: awkward_origin.to_string(),
                book_url: "https://a.example.com/book/1".to_string(),
                name: "书".to_string(),
                author: "作者".to_string(),
                ..Default::default()
            }]),
            ..Book::default()
        };
        service
            .write_bookshelf("default", &vec![book])
            .await
            .unwrap();

        service
            .remove_source_candidates(
                "default",
                &std::collections::HashSet::from([awkward_origin.to_string()]),
            )
            .await
            .unwrap();

        let list = service.read_bookshelf("default").await.unwrap();
        assert!(
            list[0].source_candidates.is_none(),
            "含转义字符的源地址也必须被清理, 不能被短路跳过"
        );
        let _ = tokio::fs::remove_dir_all(&storage_dir).await;
    }

    /// 书架里完全没有引用时, 短路应跳过清理(并且不破坏书架内容)。
    #[tokio::test]
    async fn removal_shortcut_keeps_bookshelf_intact_when_unreferenced() {
        let storage_dir = std::env::temp_dir().join(format!(
            "reader-rust-remove-shortcut-{}",
            uuid::Uuid::new_v4()
        ));
        let service = BookService::new(
            HttpClient::new(5, None).unwrap(),
            RuleEngine::new().unwrap(),
            FileCache::new(storage_dir.join("cache")),
            storage_dir.to_str().unwrap(),
        );

        let book = Book {
            book_url: "https://b.example.com/book/1".to_string(),
            name: "书".to_string(),
            author: "作者".to_string(),
            source_candidates: Some(vec![crate::model::search::SearchBook {
                origin: "https://b.example.com".to_string(),
                book_url: "https://b.example.com/book/1".to_string(),
                name: "书".to_string(),
                author: "作者".to_string(),
                ..Default::default()
            }]),
            ..Book::default()
        };
        service
            .write_bookshelf("default", &vec![book])
            .await
            .unwrap();

        service
            .remove_source_candidates(
                "default",
                &std::collections::HashSet::from(["https://unrelated.test".to_string()]),
            )
            .await
            .unwrap();

        let list = service.read_bookshelf("default").await.unwrap();
        assert_eq!(list.len(), 1);
        assert!(
            list[0].source_candidates.is_some(),
            "无关源不应被清理"
        );
        let _ = tokio::fs::remove_dir_all(&storage_dir).await;
    }

    /// 造一个只用于简介归一化测试的 service。
    fn cover_test_service(tag: &str) -> (BookService, PathBuf) {
        let storage_dir = std::env::temp_dir().join(format!(
            "reader-rust-intro-{tag}-{}",
            uuid::Uuid::new_v4()
        ));
        let service = BookService::new(
            HttpClient::new(5, None).unwrap(),
            RuleEngine::new().unwrap(),
            FileCache::new(storage_dir.join("cache")),
            storage_dir.to_str().unwrap(),
        );
        (service, storage_dir)
    }

    #[tokio::test]
    async fn empty_intro_is_normalized_back_to_none_on_save() {
        // 回归测试: 前端「编辑信息 → 还原」会把原始简介整份提交回来, 原始为空时
        // 提交的是 Some("")。若原样落库, `merge_book` 的 `intro.is_none()` 判据
        // 就永久失效 —— 换源后再也补不进新书源的简介。save_book 必须把它收回 None。
        let (service, storage_dir) = cover_test_service("normalize");
        let book = Book {
            book_url: "https://i.example.com/book/1".to_string(),
            name: "书".to_string(),
            author: "作者".to_string(),
            origin: "https://i.example.com".to_string(),
            intro: None,
            ..Book::default()
        };
        service.save_book("default", book.clone()).await.unwrap();

        // 带空简介保存(模拟还原)
        let mut with_empty = book.clone();
        with_empty.intro = Some(String::new());
        service.save_book("default", with_empty).await.unwrap();

        let list = service.read_bookshelf("default").await.unwrap();
        assert_eq!(list.len(), 1);
        assert!(
            list[0].intro.is_none(),
            "空简介应被归一化回 None, 实际: {:?}",
            list[0].intro
        );

        // 非空的简介当然要保留
        let mut with_text = book.clone();
        with_text.intro = Some("真实简介".to_string());
        service.save_book("default", with_text).await.unwrap();
        let list = service.read_bookshelf("default").await.unwrap();
        assert_eq!(list[0].intro.as_deref(), Some("真实简介"));

        let _ = tokio::fs::remove_dir_all(&storage_dir).await;
    }

    #[tokio::test]
    async fn normalized_intro_still_allows_source_backfill() {
        // 归一化的意义所在: 简介保持 None 才允许「书源带回了简介就填进去」。
        // `merge_book`(在 api/commands/book.rs) 的判据就是 `target.intro.is_none()`,
        // 这里不复刻那个函数, 只守住它依赖的不变量 —— 简介落库后必须是 None。
        let (service, storage_dir) = cover_test_service("backfill");
        let book = Book {
            book_url: "https://j.example.com/book/1".to_string(),
            name: "书".to_string(),
            author: "作者".to_string(),
            origin: "https://j.example.com".to_string(),
            intro: Some(String::new()),
            ..Book::default()
        };
        service.save_book("default", book).await.unwrap();

        let saved = service.read_bookshelf("default").await.unwrap();
        assert_eq!(saved.len(), 1);
        // 这条断言就是 merge_book 能回填的前提: 空简介入库后仍是 None。
        // 若哪天归一化被删掉, 这里会变成 Some(""), 回填能力静默失效。
        assert!(
            saved[0].intro.is_none(),
            "空简介入库后必须是 None, 否则「书源简介回填」永久失效; 实际: {:?}",
            saved[0].intro
        );

        let _ = tokio::fs::remove_dir_all(&storage_dir).await;
    }

    #[tokio::test]
    async fn login_sessions_are_random_and_bound_to_one_source() {
        let storage_dir = std::env::temp_dir().join(format!(
            "reader-rust-login-session-{}",
            std::process::id()
        ));
        let service = BookService::new(
            HttpClient::new(5, None).unwrap(),
            RuleEngine::new().unwrap(),
            FileCache::new(storage_dir.join("cache")),
            storage_dir.to_str().unwrap(),
        );
        let first = service
            .create_source_login_session("https://a.example.com")
            .await;
        let second = service
            .create_source_login_session("https://b.example.com")
            .await;

        assert_ne!(first, second);
        assert_eq!(
            service.source_for_login_session(&first).await.as_deref(),
            Some("https://a.example.com")
        );
        assert_eq!(
            service.source_for_login_session(&second).await.as_deref(),
            Some("https://b.example.com")
        );
        assert!(service.source_for_login_session("forged").await.is_none());
    }

    #[test]
    fn same_remote_book_matches_across_sources() {
        let existing = Book {
            // 书名带空格 + 作者带「作者：」前缀: 双重验证归一化匹配
            name: "测 试书目".to_string(),
            author: "作者：测试作者".to_string(),
            book_url: "https://source-a.test/book/1".to_string(),
            origin: "https://source-a.test".to_string(),
            ..Book::default()
        };
        let incoming = Book {
            name: "测试书目".to_string(),
            author: "测试作者".to_string(),
            book_url: "https://source-b.test/book/2".to_string(),
            origin: "https://source-b.test".to_string(),
            ..Book::default()
        };

        assert!(books_match_for_save(&existing, &incoming));
    }

    #[test]
    fn merging_same_book_retains_each_source_candidate() {
        let existing = Book {
            name: "测试书目".to_string(),
            author: "测试作者".to_string(),
            book_url: "https://source-a.test/book/1".to_string(),
            origin: "https://source-a.test".to_string(),
            ..Book::default()
        };
        let mut incoming = Book {
            name: "测试书目".to_string(),
            author: "测试作者".to_string(),
            book_url: "https://source-b.test/book/2".to_string(),
            origin: "https://source-b.test".to_string(),
            ..Book::default()
        };

        merge_book_source_candidates(&existing, &mut incoming);

        let candidates = incoming.source_candidates.expect("source candidates");
        assert_eq!(candidates.len(), 2);
        assert!(candidates.iter().any(|item| item.origin == "https://source-a.test"));
        assert!(candidates.iter().any(|item| item.origin == "https://source-b.test"));
    }

    #[test]
    fn merging_books_preserves_multiple_addresses_with_the_same_origin() {
        let existing = Book {
            name: "测试书目".to_string(), author: "测试作者".to_string(),
            book_url: "data:;base64,station-a".to_string(), origin: "聚合测试".to_string(),
            ..Book::default()
        };
        let mut incoming = Book { book_url: "data:;base64,station-b".to_string(), ..existing.clone() };
        merge_book_source_candidates(&existing, &mut incoming);
        ensure_current_source_candidate(&mut incoming);
        let candidates = incoming.source_candidates.unwrap();
        assert_eq!(candidates.len(), 2);
        assert_eq!(candidates[0].book_url, existing.book_url);
        assert_eq!(candidates[1].book_url, incoming.book_url);
    }

    #[tokio::test]
    async fn concurrent_source_saves_keep_one_book_and_both_candidates() {
        let storage_dir = std::env::temp_dir().join(format!(
            "reader-rust-bookshelf-source-merge-{}",
            std::process::id()
        ));
        let service = BookService::new(
            HttpClient::new(5, None).unwrap(),
            RuleEngine::new().unwrap(),
            FileCache::new(storage_dir.join("cache")),
            storage_dir.to_str().unwrap(),
        );
        let first_service = service.clone();
        let second_service = service.clone();
        let first = Book {
            name: "测试书目".to_string(),
            author: "测试作者".to_string(),
            book_url: "https://source-a.test/book/1".to_string(),
            origin: "https://source-a.test".to_string(),
            ..Book::default()
        };
        let second = Book {
            name: "测试书目".to_string(),
            author: "测试作者".to_string(),
            book_url: "https://source-b.test/book/2".to_string(),
            origin: "https://source-b.test".to_string(),
            ..Book::default()
        };

        let (first_result, second_result) = tokio::join!(
            first_service.save_book("default", first),
            second_service.save_book("default", second),
        );
        first_result.unwrap();
        second_result.unwrap();

        let books = service.get_bookshelf("default").await.unwrap();
        assert_eq!(books.len(), 1);
        let candidates = books[0]
            .source_candidates
            .as_ref()
            .expect("source candidates");
        assert_eq!(candidates.len(), 2);
        assert!(candidates.iter().any(|item| item.origin == "https://source-a.test"));
        assert!(candidates.iter().any(|item| item.origin == "https://source-b.test"));

        let _ = tokio::fs::remove_dir_all(&storage_dir).await;
    }

    #[tokio::test]
    async fn window_rate_waits_when_existing_starts_reach_limit() {
        let storage_dir =
            std::env::temp_dir().join(format!("reader-rust-window-rate-{}", std::process::id()));
        let service = BookService::new(
            HttpClient::new(5, None).unwrap(),
            RuleEngine::new().unwrap(),
            FileCache::new(storage_dir.join("cache")),
            storage_dir.to_str().unwrap(),
        );
        let now = Instant::now();
        service.rate_states.write().await.insert(
            "source".to_string(),
            RateState {
                window_starts: vec![now, now],
                ..Default::default()
            },
        );

        let result = tokio::time::timeout(
            Duration::from_millis(20),
            service.wait_for_window_rate("source", 2, 200),
        )
        .await;

        let _ = tokio::fs::remove_dir_all(&storage_dir).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn serial_rate_notice_returns_slot() {
        let storage_dir =
            std::env::temp_dir().join(format!("reader-rust-rate-notice-{}", std::process::id()));
        let service = BookService::new(
            HttpClient::new(5, None).unwrap(),
            RuleEngine::new().unwrap(),
            FileCache::new(storage_dir.join("cache")),
            storage_dir.to_str().unwrap(),
        );
        let source = BookSource {
            book_source_url: "https://source.example/".to_string(),
            ..Default::default()
        };

        let notice = RateSlotRelease {
            service: service.clone(),
            source: source.clone(),
        };
        drop(notice);
        tokio::task::yield_now().await;

        let states = service.rate_states.read().await;
        assert!(!states
            .get(&source.book_source_url)
            .is_some_and(|state| state.in_flight));
        drop(states);
        let _ = tokio::fs::remove_dir_all(&storage_dir).await;
    }

    /// 串行限速(`concurrentRate: "1000"`)必须在请求结束后**保留** `last_start`,
    /// 否则下一章请求会立刻放行, 配置的间隔形同虚设(反爬风险)。
    #[tokio::test]
    async fn serial_rate_survives_finish_rate() {
        let storage_dir =
            std::env::temp_dir().join(format!("reader-rust-serial-rate-{}", std::process::id()));
        let service = BookService::new(
            HttpClient::new(5, None).unwrap(),
            RuleEngine::new().unwrap(),
            FileCache::new(storage_dir.join("cache")),
            storage_dir.to_str().unwrap(),
        );
        let source = BookSource {
            book_source_url: "https://source.example/serial".to_string(),
            concurrent_rate: Some("60000".to_string()),
            ..Default::default()
        };

        // 第一次占用 slot 后归还: last_start 必须留下。
        assert!(service.wait_for_rate(&source).await, "首次请求应直接获得 slot");
        service.finish_rate(&source).await;

        let states = service.rate_states.read().await;
        let state = states
            .get(&source.book_source_url)
            .expect("串行限速状态必须保留, 否则限速间隔丢失");
        assert!(!state.in_flight, "归还后不应仍占着 slot");
        assert!(state.last_start.is_some(), "last_start 必须保留");
        drop(states);

        // 第二次请求在 60s 间隔内不许放行(20ms 超时即视为仍在等待)。
        let blocked = tokio::time::timeout(
            Duration::from_millis(20),
            service.wait_for_rate(&source),
        )
        .await;
        assert!(blocked.is_err(), "间隔未到就必须等待, 不能立刻放行");

        let _ = tokio::fs::remove_dir_all(&storage_dir).await;
    }
    /// 封面直连回退只在"确实走代理"且"WAF 拒绝/请求失败"时触发。
    /// 其它状态码(404/500/200)绝不能重发, 否则每次封面加载都多花一倍时间。
    #[test]
    fn cover_direct_retry_only_triggers_for_proxy_blocked_or_failed_requests() {
        // 走了代理 + 失败/拒绝码 → 重试
        assert!(cover_needs_direct_retry(true, None));
        assert!(cover_needs_direct_retry(true, Some(403)));
        assert!(cover_needs_direct_retry(true, Some(407)));
        assert!(cover_needs_direct_retry(true, Some(451)));

        // 走代理但成功/其它错误码 → 不重试
        assert!(!cover_needs_direct_retry(true, Some(200)));
        assert!(!cover_needs_direct_retry(true, Some(404)));
        assert!(!cover_needs_direct_retry(true, Some(500)));

        // 没有代理 → 永不重试(直连重试没有意义)
        assert!(!cover_needs_direct_retry(false, None));
        assert!(!cover_needs_direct_retry(false, Some(403)));
        assert!(!cover_needs_direct_retry(false, Some(200)));
    }
    /// 代理出口被站点 WAF 拦掉时返回的是几百字节的 403/407/451 拒绝页;
    /// 真实页面(以及其它状态码)不能触发直连重试, 否则会给每个正常请求加一次翻倍开销。
    #[test]
    fn proxy_block_detection_only_matches_small_rejection_pages() {
        let response = |status: u16, len: usize| FetchResponse {
            url: "https://example.com/rank.html".to_string(),
            status,
            body: "x".repeat(len),
            raw_body: Vec::new(),
            content_type: Some("text/html".to_string()),
            headers: Vec::new(),
            is_successful: false,
        };

        // 本机实测: 某书源经代理返回 403 + 298 字节拒绝页, 直连是 41KB 正文。
        assert!(response_looks_proxy_blocked(&response(403, 298)));
        assert!(response_looks_proxy_blocked(&response(407, 200)));
        assert!(response_looks_proxy_blocked(&response(451, 300)));

        // 大体积 403(带完整错误页/正文)不重试; 其它状态码一律不重试。
        assert!(!response_looks_proxy_blocked(&response(403, 8192)));
        assert!(!response_looks_proxy_blocked(&response(200, 298)));
        assert!(!response_looks_proxy_blocked(&response(404, 298)));
        assert!(!response_looks_proxy_blocked(&response(500, 128)));
    }

    /// 崩溃/强退场景: 映射与内容文件都还在, 前端没机会清理。
    /// 只按「内容文件是否被引用」判断是收不到的(映射还在), 必须同时按年龄
    /// 回收 urls/ 下的过期映射, 再把因此变孤儿的内容文件删掉。
    #[tokio::test]
    async fn stale_cover_cleanup_removes_abandoned_mapping_and_content() {
        let storage_dir = std::env::temp_dir().join(format!(
            "reader-cover-abandoned-{}",
            uuid::Uuid::new_v4().simple()
        ));
        let service = BookService::new(
            HttpClient::new(15, None).unwrap(),
            RuleEngine::new().unwrap(),
            FileCache::new(storage_dir.join("cache")),
            storage_dir.to_str().unwrap(),
        );
        let cover_dir = storage_dir.join("cache").join("public").join("cover");
        service
            .store_cover_mapping(
                "https://img.example/abandoned.png",
                b"abandoned-bytes",
                "image/png",
                &cover_dir,
            )
            .await
            .unwrap();
        let content_file = std::fs::read_dir(&cover_dir)
            .unwrap()
            .filter_map(Result::ok)
            .find(|entry| entry.path().is_file())
            .unwrap()
            .path();
        let mapping_file = std::fs::read_dir(cover_dir.join("urls"))
            .unwrap()
            .filter_map(Result::ok)
            .next()
            .unwrap()
            .path();

        // 零阈值等价于「超过任意保留时间」，用于验证映射和孤儿内容
        // 都会被这条清理路径回收。
        let removed = service
            .clean_stale_search_covers(Duration::from_secs(0))
            .await
            .unwrap();
        assert_eq!(removed, 2, "过期映射与孤儿内容都要被回收");
        assert!(!mapping_file.exists(), "过期映射必须被回收");
        assert!(!content_file.exists(), "孤儿内容文件必须被回收");
        let _ = std::fs::remove_dir_all(storage_dir);
    }

    #[test]
    fn strips_a_leading_heading_from_source_content() {
        assert_eq!(
            strip_leading_content_title("<h1>第二十七章 荒古禁</h1><p>正文</p>"),
            "<p>正文</p>",
        );
        assert_eq!(
            strip_leading_content_title("<p>第二十七章 荒古禁</p><p>正文</p>"),
            "<p>正文</p>",
        );
        assert_eq!(
            strip_leading_content_title("&nbsp;<p>第七十二章心中的希望</p><p>正文</p>"),
            "<p>正文</p>",
        );
        assert_eq!(
            strip_leading_content_title(
                "<div class=\"con\"><p>第七十二章心中的希望</p><p>正文</p></div>",
            ),
            "<div class=\"con\"><p>正文</p></div>",
        );
    }

    #[test]
    fn keeps_ordinary_source_content() {
        let content = "<p>他抬头看向远处的星空。</p><p>正文</p>";
        assert_eq!(strip_leading_content_title(content), content);
    }

    /// 缓存命中必须直接返回本地正文, 且**一次网络请求都不发**。
    ///
    /// 只断言正文内容是不够的: 旧实现"优先在线、失败回退缓存"在站点连不上时也会返回
    /// 同一份缓存正文, 用例照样通过, 只是白等一次超时。这里用本地 mock 数请求次数,
    /// 一旦又走回优先在线, `requests` 就会大于 0。
    #[tokio::test]
    async fn cached_chapter_is_returned_without_touching_the_network() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        use std::sync::Arc;

        let requests = Arc::new(AtomicUsize::new(0));
        let counter = requests.clone();
        let router = axum::Router::new().route(
            "/2.html",
            axum::routing::get(move || {
                counter.fetch_add(1, Ordering::SeqCst);
                async { "<div class='con'>在线正文</div>" }
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let server = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });

        let storage_dir = std::env::temp_dir().join(format!(
            "reader-cache-first-{}",
            uuid::Uuid::new_v4().simple()
        ));
        let service = BookService::new(
            HttpClient::new(15, None).unwrap(),
            RuleEngine::new().unwrap(),
            FileCache::new(storage_dir.join("cache")),
            storage_dir.to_str().unwrap(),
        );
        let book_url = format!("{base}/book");
        let chapter_url = format!("{base}/2.html");
        let cached = "<p>本地已有的正文</p>";
        service
            .cache
            .put("default", &md5_hex(&book_url), &chapter_url, cached)
            .await
            .unwrap();

        let source: BookSource = serde_json::from_value(serde_json::json!({
            "bookSourceUrl": base,
            "bookSourceName": "测试源",
            "bookSourceType": 0,
            "ruleContent": { "content": "class.con@text" },
        }))
        .unwrap();

        let content = service
            .get_content_with_stage("default", &book_url, &source, &chapter_url, None)
            .await
            .unwrap();

        assert_eq!(content, cached);
        assert_eq!(
            requests.load(Ordering::SeqCst),
            0,
            "缓存命中的章节不应该再发网络请求"
        );
        server.abort();
        let _ = std::fs::remove_dir_all(storage_dir);
    }

    /// 字数只从**已缓存**的正文现算: 未缓存的章节不出现在结果里, 调用方据此决定
    /// 是否显示。这也保证字数与缓存文件始终一致, 不会出现两套真相。
    #[tokio::test]
    async fn chapter_word_counts_only_cover_cached_chapters() {
        let storage_dir = std::env::temp_dir().join(format!(
            "reader-word-count-{}",
            uuid::Uuid::new_v4().simple()
        ));
        let service = BookService::new(
            HttpClient::new(15, None).unwrap(),
            RuleEngine::new().unwrap(),
            FileCache::new(storage_dir.join("cache")),
            storage_dir.to_str().unwrap(),
        );
        let book_url = "https://book.example/wc";
        let cached_url = "https://book.example/wc/1.html";
        let uncached_url = "https://book.example/wc/2.html";
        service
            .cache
            .put("default", &md5_hex(book_url), cached_url, "<p>你好世界</p>")
            .await
            .unwrap();

        let counts = service
            .chapter_word_counts(
                "default",
                book_url,
                &[cached_url.to_string(), uncached_url.to_string()],
            )
            .await
            .unwrap();

        // 标签被去掉: `<p>你好世界</p>` 是 4 个字, 不是 14。
        assert_eq!(counts, vec![(cached_url.to_string(), 4)]);
        let _ = std::fs::remove_dir_all(storage_dir);
    }

    /// 旧缓存(只有正文、没有 sidecar)必须仍然能算出字数, 并把结果回填下来 ——
    /// 否则升级后每次打开目录都要把所有旧章节重新解析一遍。
    #[tokio::test]
    async fn legacy_cache_without_sidecar_is_backfilled() {
        let storage_dir = std::env::temp_dir().join(format!(
            "reader-word-count-legacy-{}",
            uuid::Uuid::new_v4().simple()
        ));
        let cache = FileCache::new(storage_dir.join("cache"));
        let service = BookService::new(
            HttpClient::new(15, None).unwrap(),
            RuleEngine::new().unwrap(),
            cache.clone(),
            storage_dir.to_str().unwrap(),
        );
        let book_url = "https://book.example/legacy";
        let chapter_url = "https://book.example/legacy/1.html";
        let book_key = md5_hex(book_url);

        // 直接写正文文件, 模拟升级前留下的缓存(没有 .count)。
        cache
            .put("default", &book_key, chapter_url, "<p>旧正文五个</p>")
            .await
            .unwrap();
        tokio::fs::remove_file(
            storage_dir
                .join("cache")
                .join("default")
                .join(&book_key)
                .join(format!("{}.count", md5_hex(chapter_url))),
        )
        .await
        .unwrap();
        assert_eq!(
            cache.get_word_count("default", &book_key, chapter_url).await.unwrap(),
            None
        );

        let counts = service
            .chapter_word_counts("default", book_url, &[chapter_url.to_string()])
            .await
            .unwrap();
        assert_eq!(counts, vec![(chapter_url.to_string(), 5)]);

        // 回填之后 sidecar 就该存在, 下次不再走解析。
        assert_eq!(
            cache.get_word_count("default", &book_key, chapter_url).await.unwrap(),
            Some(5)
        );
        let _ = std::fs::remove_dir_all(storage_dir);
    }

    /// issue #23 端到端：书源的 `coverDecodeJs` 必须在封面流水线里执行。
    ///
    /// 用一个本地 HTTP 服务返回**加密的封面字节**（Content-Type 还是
    /// `binary/octet-stream`，和 18mh 的真实响应一致），断言 `get_cover_for_source`
    /// 返回的是解密后的 JPEG。
    #[tokio::test]
    async fn cover_is_decoded_with_source_cover_decode_js() {
        use std::sync::Arc;

        // AES-128/CBC/NoPadding，key/iv 与 18mh 书源相同；明文以 JPEG 魔数开头。
        let cipher: Vec<u8> = hex::decode(
            "093de3b1fc4af46ff223ac4c8bdb076a2cd8265cb34aa1f5b2108d081a019956\
             d014684a3ea5f29b05c20175c3d50f1e78835c48eef745af103df74d525e9010080\
             cbb77195f73e94364a9dd5237613c674c340a17dc360501cb9b9ba5835b8b",
        )
        .unwrap();
        let payload = Arc::new(cipher);
        let router = axum::Router::new().route(
            "/c.jpg",
            axum::routing::get(move || {
                let payload = payload.clone();
                async move {
                    (
                        [(axum::http::header::CONTENT_TYPE, "binary/octet-stream")],
                        (*payload).clone(),
                    )
                }
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let server = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });

        let storage_dir = std::env::temp_dir().join(format!(
            "reader-cover-decode-{}",
            uuid::Uuid::new_v4().simple()
        ));
        let service = BookService::new(
            HttpClient::new(15, None).unwrap(),
            RuleEngine::new().unwrap(),
            FileCache::new(storage_dir.join("cache")),
            storage_dir.to_str().unwrap(),
        );
        let source: BookSource = serde_json::from_value(serde_json::json!({
            "bookSourceUrl": base,
            "bookSourceName": "加密封面",
            "coverDecodeJs": "java.createSymmetricCrypto(\"AES/CBC/NoPadding\", \"f5d965df75336270\", \"97b60394abc2fbe1\").decrypt(result)",
        }))
        .unwrap();

        let cover_url = format!("{base}/c.jpg");
        let (data, content_type) = service
            .get_cover_for_source("public", &cover_url, Some(&source))
            .await
            .unwrap();

        assert_eq!(&data[..4], &[0xFF, 0xD8, 0xFF, 0xE0], "应解出 JPEG");
        assert_eq!(data.len(), 96, "NoPadding 不应改变长度");
        assert_eq!(content_type, "image/jpeg", "解密后按魔数认格式");
        // 缓存里也必须是解密后的字节, 否则下次命中又会花图。
        let (cached, cached_type) = service.get_cover_for_source("public", &cover_url, Some(&source)).await.unwrap();
        assert_eq!(cached, data);
        assert_eq!(cached_type, "image/jpeg");

        server.abort();
        let _ = std::fs::remove_dir_all(storage_dir);
    }

    /// 没有 `coverDecodeJs` 的书源不能被解密逻辑影响: 原样返回服务端字节。
    #[tokio::test]
    async fn cover_passthrough_without_cover_decode_js() {
        use std::sync::Arc;

        let payload = Arc::new(vec![0xFFu8, 0xD8, 0xFF, 0xE0, 1, 2, 3, 4]);
        let router = axum::Router::new().route(
            "/plain.jpg",
            axum::routing::get(move || {
                let payload = payload.clone();
                async move { (*payload).clone() }
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let server = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });

        let storage_dir = std::env::temp_dir().join(format!(
            "reader-cover-plain-{}",
            uuid::Uuid::new_v4().simple()
        ));
        let service = BookService::new(
            HttpClient::new(15, None).unwrap(),
            RuleEngine::new().unwrap(),
            FileCache::new(storage_dir.join("cache")),
            storage_dir.to_str().unwrap(),
        );
        let source: BookSource = serde_json::from_value(serde_json::json!({
            "bookSourceUrl": base,
            "bookSourceName": "普通封面",
        }))
        .unwrap();

        let (data, content_type) = service
            .get_cover_for_source("public", &format!("{base}/plain.jpg"), Some(&source))
            .await
            .unwrap();
        assert_eq!(data, vec![0xFFu8, 0xD8, 0xFF, 0xE0, 1, 2, 3, 4]);
        assert_eq!(content_type, "image/jpeg");

        server.abort();
        let _ = std::fs::remove_dir_all(storage_dir);
    }

    /// 只对**部分**封面加密的书源：明文封面不能被解密规则弄坏。
    ///
    /// 一些书源的封面图床混了两种情况（老图未加密、新图加密）。无条件解密
    /// 会把本来正常的 JPEG 变成一堆乱码 —— 此时应保留原图。
    #[tokio::test]
    async fn plain_image_survives_a_broken_cover_decode_rule() {
        use std::sync::Arc;

        // 一段合法 JPEG 头 + 若干字节，长度不是 16 的倍数。
        let payload = Arc::new(vec![0xFFu8, 0xD8, 0xFF, 0xE0, 9, 8, 7, 6, 5, 4, 3]);
        let router = axum::Router::new().route(
            "/plain.jpg",
            axum::routing::get(move || {
                let payload = payload.clone();
                async move { (*payload).clone() }
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let server = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });

        let storage_dir = std::env::temp_dir().join(format!(
            "reader-cover-mixed-{}",
            uuid::Uuid::new_v4().simple()
        ));
        let service = BookService::new(
            HttpClient::new(15, None).unwrap(),
            RuleEngine::new().unwrap(),
            FileCache::new(storage_dir.join("cache")),
            storage_dir.to_str().unwrap(),
        );
        let source: BookSource = serde_json::from_value(serde_json::json!({
            "bookSourceUrl": base,
            "bookSourceName": "部分加密",
            "coverDecodeJs": "java.createSymmetricCrypto(\"AES/CBC/NoPadding\", \"f5d965df75336270\", \"97b60394abc2fbe1\").decrypt(result)",
        }))
        .unwrap();

        let (data, content_type) = service
            .get_cover_for_source("public", &format!("{base}/plain.jpg"), Some(&source))
            .await
            .unwrap();
        assert_eq!(data, vec![0xFFu8, 0xD8, 0xFF, 0xE0, 9, 8, 7, 6, 5, 4, 3]);
        assert_eq!(content_type, "image/jpeg");

        server.abort();
        let _ = std::fs::remove_dir_all(storage_dir);
    }

    /// 搜索/发现页会话结束时释放封面：解密过的封面也必须能删干净。
    ///
    /// `release()` 走的是 `clearSearchCoverCache` → `delete_cover_caches("public")`。
    /// 解密只发生在下载那一步，落盘后与普通封面完全同构，所以这里验证的是
    /// "解密路径不会把文件写到别的目录/换成别的键"。
    #[tokio::test]
    async fn decoded_cover_is_released_with_the_search_session() {
        let storage_dir = std::env::temp_dir().join(format!(
            "reader-cover-release-{}",
            uuid::Uuid::new_v4().simple()
        ));
        let service = BookService::new(
            HttpClient::new(15, None).unwrap(),
            RuleEngine::new().unwrap(),
            FileCache::new(storage_dir.join("cache")),
            storage_dir.to_str().unwrap(),
        );
        let cover_dir = storage_dir.join("cache").join("public").join("cover");

        // 直接写解密后的字节（等价于 get_cover 成功解密后的落盘结果）。
        let plain = vec![0xFFu8, 0xD8, 0xFF, 0xE0, 1, 2, 3, 4];
        let url = "https://img.example/search-result.jpg";
        service
            .store_cover_mapping(url, &plain, "image/jpeg", &cover_dir)
            .await
            .unwrap();
        assert!(cover_dir.join("urls").exists());

        // 会话结束时的释放。
        service
            .delete_cover_caches("public", &[url.to_string()])
            .await
            .unwrap();

        assert!(
            !cover_dir
                .join("urls")
                .join(format!("{}.json", md5_hex(url)))
                .exists(),
            "映射必须被删除"
        );
        let left: Vec<String> = std::fs::read_dir(&cover_dir)
            .unwrap()
            .filter_map(Result::ok)
            .filter(|entry| entry.path().is_file())
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .collect();
        assert!(left.is_empty(), "会话释放后不应留下内容文件: {left:?}");

        let _ = std::fs::remove_dir_all(storage_dir);
    }

    /// 启动清理**不能**把书架书的封面当垃圾删掉（issue #23 后续）。
    ///
    /// 前端只在搜索页/发现页卸载时释放封面。进程被强杀、直接关窗、
    /// IPC 失败都会漏，于是书架封面也会变成"超过 30 分钟未更新"的候选。
    /// 旧实现只按 mtime 回收 → 隔天启动一次，书架封面全被删光。
    #[tokio::test]
    async fn stale_cleanup_never_evicts_a_shelf_cover() {
        let storage_dir = std::env::temp_dir().join(format!(
            "reader-cover-shelf-pin-{}",
            uuid::Uuid::new_v4().simple()
        ));
        let service = BookService::new(
            HttpClient::new(15, None).unwrap(),
            RuleEngine::new().unwrap(),
            FileCache::new(storage_dir.join("cache")),
            storage_dir.to_str().unwrap(),
        );
        let cover_dir = storage_dir.join("cache").join("public").join("cover");

        // 书架上一本书 + 一张发现页残留的封面。
        let shelf_url = "https://img.example/on-shelf.jpg";
        let search_url = "https://img.example/from-explore.jpg";
        service
            .store_cover_mapping(shelf_url, b"shelf-cover-bytes", "image/jpeg", &cover_dir)
            .await
            .unwrap();
        service
            .store_cover_mapping(search_url, b"explore-cover-bytes", "image/jpeg", &cover_dir)
            .await
            .unwrap();
        service
            .save_book(
                "default",
                crate::model::book::Book {
                    name: "书架书".to_string(),
                    author: "作者".to_string(),
                    book_url: "https://s.example/book/1".to_string(),
                    origin: "https://s.example".to_string(),
                    cover_url: Some(shelf_url.to_string()),
                    ..Default::default()
                },
            )
            .await
            .unwrap();

        // 阈值 0 = 全部视为过期。
        service
            .clean_stale_search_covers(Duration::from_secs(0))
            .await
            .unwrap();

        assert!(
            cover_dir
                .join("urls")
                .join(format!("{}.json", md5_hex(shelf_url)))
                .exists(),
            "书架书的封面映射必须被保住"
        );
        assert!(
            !cover_dir
                .join("urls")
                .join(format!("{}.json", md5_hex(search_url)))
                .exists(),
            "发现页残留的映射应被回收"
        );
        // 书架的图还在，发现页那张（连内容）已被清掉。
        let (data, _) = service.get_cover("public", shelf_url).await.unwrap();
        assert_eq!(data, b"shelf-cover-bytes");

        let _ = std::fs::remove_dir_all(storage_dir);
    }

    /// 从搜索/发现页读过、但**没加入书架**的书，章节缓存原本永远没人删：
    /// `delete_book` 只在书架里匹配得到，孤儿目录会一直堆着。
    #[tokio::test]
    async fn orphan_chapter_cache_is_cleaned_but_shelf_book_is_kept() {
        let storage_dir = std::env::temp_dir().join(format!(
            "reader-orphan-chapter-{}",
            uuid::Uuid::new_v4().simple()
        ));
        let service = BookService::new(
            HttpClient::new(15, None).unwrap(),
            RuleEngine::new().unwrap(),
            FileCache::new(storage_dir.join("cache")),
            storage_dir.to_str().unwrap(),
        );

        let shelf_url = "https://s.example/book/on-shelf";
        let orphan_url = "https://s.example/book/read-but-not-added";
        service
            .save_book(
                "default",
                crate::model::book::Book {
                    name: "书架书".to_string(),
                    author: "作者".to_string(),
                    book_url: shelf_url.to_string(),
                    origin: "https://s.example".to_string(),
                    ..Default::default()
                },
            )
            .await
            .unwrap();

        // 两边都写了章节正文缓存。
        service
            .cache
            .put("default", &md5_hex(shelf_url), "ch-1", "书架书的正文")
            .await
            .unwrap();
        service
            .cache
            .put("default", &md5_hex(orphan_url), "ch-1", "看完就走的正文")
            .await
            .unwrap();

        // 阈值 0 = 全部视为闲置够久。
        let removed = service
            .clean_orphan_chapter_caches(Duration::from_secs(0))
            .await
            .unwrap();

        assert_eq!(removed, 1, "只该清掉孤儿目录");
        assert!(
            service
                .cache
                .exists("default", &md5_hex(shelf_url), "ch-1")
                .await,
            "书架书的章节缓存必须保住"
        );
        assert!(
            !service
                .cache
                .exists("default", &md5_hex(orphan_url), "ch-1")
                .await,
            "未加入书架且闲置的章节缓存应被回收"
        );

        let _ = std::fs::remove_dir_all(storage_dir);
    }

    /// 刚读完还没决定加不加书架的书，不能在下一次启动时被清掉。
    #[tokio::test]
    async fn recent_orphan_chapter_cache_survives_the_age_threshold() {

        let storage_dir = std::env::temp_dir().join(format!(
            "reader-orphan-chapter-fresh-{}",
            uuid::Uuid::new_v4().simple()
        ));
        let service = BookService::new(
            HttpClient::new(15, None).unwrap(),
            RuleEngine::new().unwrap(),
            FileCache::new(storage_dir.join("cache")),
            storage_dir.to_str().unwrap(),
        );
        let url = "https://s.example/book/just-read";
        service
            .cache
            .put("default", &md5_hex(url), "ch-1", "刚读完的正文")
            .await
            .unwrap();

        // 24 小时的阈值：刚写下的目录远没到。
        let removed = service
            .clean_orphan_chapter_caches(Duration::from_secs(24 * 60 * 60))
            .await
            .unwrap();

        assert_eq!(removed, 0);
        assert!(service.cache.exists("default", &md5_hex(url), "ch-1").await);

        let _ = std::fs::remove_dir_all(storage_dir);
    }

    /// 目录缓存（`data/<ns>/chapters/*.json`）同样是孤儿高发区：
    /// 从搜索页读过的书没加书架，toc 缓存就没人清。这类文件单体最大。
    #[tokio::test]
    async fn orphan_chapter_list_cache_is_cleaned_but_shelf_book_is_kept() {
        let storage_dir = std::env::temp_dir().join(format!(
            "reader-orphan-toc-{}",
            uuid::Uuid::new_v4().simple()
        ));
        let service = BookService::new(
            HttpClient::new(15, None).unwrap(),
            RuleEngine::new().unwrap(),
            FileCache::new(storage_dir.join("cache")),
            storage_dir.to_str().unwrap(),
        );
        let toc_dir = storage_dir.join("data").join("default").join("chapters");
        std::fs::create_dir_all(&toc_dir).unwrap();

        let shelf_toc = "https://s.example/toc/on-shelf";
        let orphan_toc = "https://s.example/toc/read-but-not-added";
        std::fs::write(toc_dir.join(format!("{}.v2.json", md5_hex(shelf_toc))), "[]").unwrap();
        std::fs::write(toc_dir.join(format!("{}.v2.json", md5_hex(orphan_toc))), "[]").unwrap();

        service
            .save_book(
                "default",
                crate::model::book::Book {
                    name: "书架书".to_string(),
                    author: "作者".to_string(),
                    book_url: "https://s.example/book/on-shelf".to_string(),
                    origin: "https://s.example".to_string(),
                    toc_url: Some(shelf_toc.to_string()),
                    ..Default::default()
                },
            )
            .await
            .unwrap();

        service
            .clean_orphan_chapter_caches(Duration::from_secs(0))
            .await
            .unwrap();

        assert!(
            toc_dir.join(format!("{}.v2.json", md5_hex(shelf_toc))).exists(),
            "书架书的目录缓存必须保住"
        );
        assert!(
            !toc_dir.join(format!("{}.v2.json", md5_hex(orphan_toc))).exists(),
            "未加入书架的目录缓存应被回收"
        );

        let _ = std::fs::remove_dir_all(storage_dir);
    }

    /// 空壳书源会话目录要回收，有登录态的必须保住。
    #[tokio::test]
    async fn empty_source_state_dirs_are_cleaned_but_logged_in_ones_are_kept() {
        let storage_dir = std::env::temp_dir().join(format!(
            "reader-source-state-clean-{}",
            uuid::Uuid::new_v4().simple()
        ));
        let service = BookService::new(
            HttpClient::new(15, None).unwrap(),
            RuleEngine::new().unwrap(),
            FileCache::new(storage_dir.join("cache")),
            storage_dir.to_str().unwrap(),
        );
        let root = storage_dir.join("source-state");

        let empty = root.join("empty-shell");
        std::fs::create_dir_all(&empty).unwrap();
        std::fs::write(empty.join("cookies.json"), "{}").unwrap();
        std::fs::write(empty.join("state.json"), "{}").unwrap();

        let logged_in = root.join("has-cookie");
        std::fs::create_dir_all(&logged_in).unwrap();
        std::fs::write(logged_in.join("cookies.json"), r#"{"sid":"abc"}"#).unwrap();
        std::fs::write(logged_in.join("state.json"), "{}").unwrap();

        let removed = service
            .clean_empty_source_state_dirs(Duration::from_secs(0))
            .await;

        assert_eq!(removed, 1, "只该删空壳目录");
        assert!(!empty.exists(), "空壳目录应被回收");
        assert!(logged_in.exists(), "有 cookie 的目录是登录态，必须保住");

        let _ = std::fs::remove_dir_all(storage_dir);
    }

    /// 修复前落盘的「密文封面」缓存必须能自愈。
    /// 用户已经踩过一次坑: 旧版本把密文按 URL 后缀存成了 `.jpg`，
    /// 映射里 content-type 也是 `image/jpeg`。若只在新下载路径解密，这些
    /// 用户永远看到花图（清缓存也一样，因为每次都会命中同一个坏文件）。
    #[tokio::test]
    async fn legacy_encrypted_cover_cache_is_repaired() {
        let storage_dir = std::env::temp_dir().join(format!(
            "reader-cover-repair-{}",
            uuid::Uuid::new_v4().simple()
        ));
        let service = BookService::new(
            HttpClient::new(15, None).unwrap(),
            RuleEngine::new().unwrap(),
            FileCache::new(storage_dir.join("cache")),
            storage_dir.to_str().unwrap(),
        );
        let cover_dir = storage_dir.join("cache").join("public").join("cover");
        let cipher = hex::decode(
            "093de3b1fc4af46ff223ac4c8bdb076a2cd8265cb34aa1f5b2108d081a019956\
             d014684a3ea5f29b05c20175c3d50f1e78835c48eef745af103df74d525e9010080\
             cbb77195f73e94364a9dd5237613c674c340a17dc360501cb9b9ba5835b8b",
        )
        .unwrap();
        let url = "https://img.example/legacy.jpg";
        service
            .store_cover_mapping(url, &cipher, "image/jpeg", &cover_dir)
            .await
            .unwrap();

        let source: BookSource = serde_json::from_value(serde_json::json!({
            "bookSourceUrl": "https://18mh.example",
            "bookSourceName": "加密封面",
            "coverDecodeJs": "java.createSymmetricCrypto(\"AES/CBC/NoPadding\", \"f5d965df75336270\", \"97b60394abc2fbe1\").decrypt(result)",
        }))
        .unwrap();

        let (data, content_type) = service
            .get_cover_for_source("public", url, Some(&source))
            .await
            .unwrap();
        assert_eq!(&data[..4], &[0xFF, 0xD8, 0xFF, 0xE0], "缓存里的密文应被就地修好");
        assert_eq!(content_type, "image/jpeg");

        // 重写映射后旧的密文文件必须当场删掉：清理任务只在启动时跑，
        // 留着它要等到下次重启、再闲置 30 分钟才会被回收。
        let content_files: Vec<String> = std::fs::read_dir(&cover_dir)
            .unwrap()
            .filter_map(Result::ok)
            .filter(|entry| entry.path().is_file())
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(
            content_files.len(),
            1,
            "只应留下解密后的图，密文孤儿要被就地清掉: {content_files:?}"
        );
        assert_eq!(
            std::fs::read(cover_dir.join(&content_files[0])).unwrap(),
            data
        );

        let _ = std::fs::remove_dir_all(storage_dir);
    }
}
