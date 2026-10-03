use crate::crawler::{
    fetcher::{fetch_with_client, FetchResponse, RequestSpec, StrResponse},
    http_client::{HttpClient, ProxyMode, ProxyStatus},
    url_analyzer::analyze_url,
};
use crate::error::error::AppError;
use crate::model::{
    book::Book,
    book_chapter::BookChapter,
    book_source::{BookSource, ExploreKind},
    search::SearchBook,
};
use crate::parser::js::{
    eval_js, eval_js_with_bindings, take_js_logs, with_book_context, with_source, with_source_key,
    JsBookContext,
};
use crate::parser::rule_engine::RuleEngine;
use crate::storage::cache::file_cache::FileCache;
use crate::service::local_pdf_book::{is_local_pdf_origin, is_local_pdf_url};
use crate::util::hash::md5_hex;
use crate::util::text::{normalize_source_url, repair_encoded_url};
use serde_json::json;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::fs;
use tokio::sync::{Mutex, RwLock};
use tokio::time::{sleep, Duration, Instant};

/// 书源 JS 调用 `java.showBrowser(url, html)` 时捕获的请求, 供前端弹出面板。
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ShowBrowserRequest {
    pub url: String,
    pub html: Option<String>,
    pub preload_js: Option<String>,
    /// 后端托管的面板 id(前端按 `${readerOrigin}/sourcePanel?panelId=...` 加载)。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub panel_id: Option<String>,
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
#[derive(Clone, Debug)]
pub struct SourcePanelEntry {
    pub url: String,
    pub html: String,
    /// 归属书源, 面板内 `window.run` 需要在其 jsLib 作用域执行。
    pub book_source_url: String,
    pub book_url: String,
    pub chapter_url: String,
    /// `book` / `chapter` 绑定(JSON), 面板脚本 `writeConfig(chapter, ...)` 需要。
    pub book_json: String,
    pub chapter_json: String,
}

static SOURCE_PANELS: std::sync::LazyLock<std::sync::Mutex<Vec<(String, SourcePanelEntry)>>> =
    std::sync::LazyLock::new(|| std::sync::Mutex::new(Vec::new()));

/// 同时保留的面板数上限(插件通常只有 1~2 层弹层)。
const MAX_SOURCE_PANELS: usize = 8;

/// 登记一个面板并返回其 id。
pub fn register_source_panel(entry: SourcePanelEntry) -> String {
    let id = format!(
        "p{}_{}",
        md5_hex(&format!("{}:{}", entry.book_source_url, entry.url)),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0)
    );
    let mut panels = SOURCE_PANELS.lock().unwrap_or_else(|e| e.into_inner());
    // 按插入顺序淘汰最旧的(用 Vec 而非 HashMap: 后者无序, 淘汰会变成随机的,
    // 可能剔掉正在显示的面板导致其后续交互 404)
    if panels.len() >= MAX_SOURCE_PANELS {
        panels.remove(0);
    }
    panels.push((id.clone(), entry));
    id
}

pub fn get_source_panel(panel_id: &str) -> Option<SourcePanelEntry> {
    SOURCE_PANELS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .iter()
        .find(|(id, _)| id == panel_id)
        .map(|(_, entry)| entry.clone())
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
        "close" => Ok(serde_json::Value::Null),
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
                crate::parser::js::with_book_context(Some(book_ctx), || {
                    crate::parser::js::with_source(&source, || {
                        crate::parser::js::with_source_key(Some(&entry.book_url), || {
                            crate::parser::js::eval_js(&script, "", &entry.chapter_url)
                        })
                    })
                })
            })
            .await
            .map_err(|e| e.to_string())?;
            output
                .map(serde_json::Value::String)
                .map_err(|e| format!("脚本执行失败: {e}"))
        }
        "ajax" | "connect" | "http" => {
            let source = book_source_service
                .get("default", &entry.book_source_url)
                .await
                .map_err(|e| e.to_string())?
                .ok_or_else(|| "bookSource not found".to_string())?;
            let action = action.to_string();
            let payload = payload.clone();
            let entry = entry.clone();
            let result = tokio::task::spawn_blocking(move || {
                crate::parser::js::run_panel_request(&source, &entry.chapter_url, &action, &payload)
            })
            .await
            .map_err(|e| e.to_string())?;
            result
                .map(serde_json::Value::String)
                .map_err(|e| e.to_string())
        }
        // 面板内再次调 `java.showBrowser`: 登记为新面板并回传 panelId,
        // 由桥通过 postMessage 通知宿主换成新面板(嵌套弹层)。
        "showBrowser" => {
            let panel_id = register_source_panel(SourcePanelEntry {
                url: text_of("url"),
                html: text_of("html"),
                book_source_url: entry.book_source_url.clone(),
                book_url: entry.book_url.clone(),
                chapter_url: entry.chapter_url.clone(),
                book_json: entry.book_json.clone(),
                chapter_json: entry.chapter_json.clone(),
            });
            Ok(serde_json::Value::String(panel_id))
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

#[derive(Clone)]
pub struct BookService {
    http: HttpClient,
    parser: RuleEngine,
    cache: FileCache,
    storage_dir: PathBuf,
    source_cookies: Arc<RwLock<HashMap<String, String>>>,
    source_clients: Arc<RwLock<HashMap<String, reqwest::Client>>>,
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
/// apart from「规则写错」. Factors collected from the Qidian debugging session:
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
        Self {
            http,
            parser,
            cache,
            storage_dir,
            source_cookies: Arc::new(RwLock::new(HashMap::new())),
            source_clients: Arc::new(RwLock::new(HashMap::new())),
            login_sessions: Arc::new(RwLock::new(HashMap::new())),
            rate_states: Arc::new(RwLock::new(HashMap::new())),
            bookshelf_write_lock: Arc::new(Mutex::new(())),
        }
    }

    pub fn http_client(&self) -> reqwest::Client {
        self.http.client()
    }

    /// Return a client isolated to one book source. Its cookie jar is never
    /// shared with another source or with the app's generic HTTP client.
    pub async fn source_http_client(
        &self,
        user_ns: &str,
        source_url: &str,
        proxy: Option<&str>,
    ) -> Result<reqwest::Client, AppError> {
        let key = Self::source_cookie_key(user_ns, source_url);
        let proxy_key = proxy.map(str::trim).unwrap_or_default();
        let client_key = format!("{key}::proxy={proxy_key}");
        if let Some(client) = self.source_clients.read().await.get(&client_key).cloned() {
            return Ok(client);
        }

        let mut clients = self.source_clients.write().await;
        if let Some(client) = clients.get(&client_key).cloned() {
            return Ok(client);
        }
        let client = self
            .http
            .new_client_with_proxy(proxy)
            .map_err(AppError::Internal)?;
        clients.insert(client_key, client.clone());
        Ok(client)
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
        self.source_clients.write().await.clear();
        // A proxy switch invalidates any in-flight login preview tokens. This
        // prevents a session created under one network path from being reused
        // after the request routing has changed.
        self.login_sessions.write().await.clear();
        Ok(status)
    }

    fn source_cookie_key(user_ns: &str, source_url: &str) -> String {
        format!("{}::{}", user_ns, normalize_source_url(source_url))
    }

    async fn apply_source_cookie(
        &self,
        user_ns: &str,
        source: &BookSource,
        headers: &mut Vec<(String, String)>,
    ) {
        let key = Self::source_cookie_key(user_ns, &source.book_source_url);
        if let Some(cookie) = self.source_cookies.read().await.get(&key).cloned() {
            if !headers
                .iter()
                .any(|(name, _)| name.eq_ignore_ascii_case("cookie"))
            {
                headers.push(("Cookie".to_string(), cookie));
            }
        }
    }

    pub async fn set_source_cookie(&self, user_ns: &str, source_url: &str, cookie: &str) {
        let cookie = cookie.trim();
        if cookie.is_empty() {
            return;
        }
        let key = Self::source_cookie_key(user_ns, source_url);
        let prefix = format!("{key}::proxy=");
        self.source_cookies
            .write()
            .await
            .insert(key.clone(), cookie.to_string());
        self.source_clients
            .write()
            .await
            .retain(|client_key, _| client_key != &key && !client_key.starts_with(&prefix));
    }

    pub async fn clear_source_cookie(&self, user_ns: &str, source_url: &str) {
        let key = Self::source_cookie_key(user_ns, source_url);
        self.source_cookies.write().await.remove(&key);
        let prefix = format!("{key}::proxy=");
        self.source_clients
            .write()
            .await
            .retain(|client_key, _| client_key != &key && !client_key.starts_with(&prefix));
        let source_url = normalize_source_url(source_url);
        self.login_sessions
            .write()
            .await
            .retain(|_, session| session.source_url != source_url);
    }

    /// Validate a candidate login cookie before storing it: run one search
    /// request with the cookie attached and make sure the site still returns
    /// a normal page (Qidian returns a degraded/anti-crawler page — e.g. the
    /// `var buid` verification shell — when a stale or wrong cookie is sent,
    /// which would otherwise poison every later search of this source).
    pub async fn validate_source_cookie(
        &self,
        // 校验是一次性动作, 校验请求自己带 cookie, 不需要按用户取存量值
        _user_ns: &str,
        source: &BookSource,
        cookie: &str,
        keyword: &str,
    ) -> Result<(), AppError> {
        let search_url = source
            .search_url
            .clone()
            .ok_or_else(|| AppError::BadRequest("书源未配置 searchUrl，无法校验 Cookie".to_string()))?;
        let mut spec = analyze_url(&search_url, keyword, 1, &source.book_source_url, source)
            .map_err(|e| AppError::BadRequest(e.to_string()))?;
        // 单次校验请求只携带待验证的 cookie, 不带旧 cookie, 避免旧值干扰判断
        spec.headers.retain(|(name, _)| !name.eq_ignore_ascii_case("cookie"));
        spec.headers.push(("Cookie".to_string(), cookie.trim().to_string()));

        // Cookie validation must not reuse the source's persistent reqwest
        // client: its cookie jar may contain an older session and make an
        // invalid candidate appear valid. Use a fresh client for this request.
        self.wait_for_rate(source).await;
        let client = self
            .http
            .new_client_with_proxy(spec.proxy.as_deref())
            .map_err(AppError::Internal)?;
        let result = fetch_with_client(&client, spec).await;
        self.finish_rate(source).await;
        let res = result.map_err(AppError::Internal)?;
        let body = res.body;

        // 反爬/降级页面在站内跳转前后都不可靠, 直接按内容特征判断
        let body_lower = body.to_lowercase();
        if res.status == 202
            || body_lower.contains("var buid")
            || body_lower.contains("验证")
            || body_lower.contains("安全校验")
            || body_lower.contains("<title>403")
            || body_lower.contains("access denied")
        {
            return Err(AppError::BadRequest(format!(
                "Cookie 无效或已过期(站点返回 {}，疑似被反爬拦截)。请更换抓包 Cookie 后重试",
                res.status
            )));
        }

        // 正常搜索页: 用规则解析一次, 拿不到书就说明页面结构已变, 同样拒绝
        let books = self.parser.search_books(source, &body, &res.url);
        if books.is_empty() {
            return Err(AppError::BadRequest(
                "Cookie 校验失败: 搜索页未返回任何结果, 请确认 Cookie 有效".to_string(),
            ));
        }

        Ok(())
    }

    async fn fetch_source_url(
        &self,
        user_ns: &str,
        source: &BookSource,
        url_rule: &str,
        base_url: &str,
        key: &str,
    ) -> Result<FetchResponse, AppError> {
        let mut spec = analyze_url(url_rule, key, 1, base_url, source)?;
        self.apply_source_cookie(user_ns, source, &mut spec.headers)
            .await;
        let res = self.fetch_with_rate(user_ns, source, spec).await?;
        Ok(apply_login_check_js(source, res))
    }

    async fn fetch_with_rate(
        &self,
        user_ns: &str,
        source: &BookSource,
        spec: RequestSpec,
    ) -> anyhow::Result<FetchResponse> {
        self.wait_for_rate(source).await;
        let request_client = self
            .source_http_client(user_ns, &source.book_source_url, spec.proxy.as_deref())
            .await
            .map_err(|err| anyhow::anyhow!(err.to_string()))?;
        let result = fetch_with_client(&request_client, spec).await;
        self.finish_rate(source).await;
        result
    }

    async fn wait_for_rate(&self, source: &BookSource) {
        let Some(rate) = source.concurrent_rate.as_deref().map(str::trim) else {
            return;
        };
        if rate.is_empty() || rate == "0" {
            return;
        }
        if let Some((limit, window_ms)) = parse_window_rate(rate) {
            self.wait_for_window_rate(&source.book_source_url, limit, window_ms)
                .await;
            return;
        }
        let Ok(delay_ms) = rate.parse::<u64>() else {
            return;
        };
        self.wait_for_serial_rate(&source.book_source_url, delay_ms)
            .await;
    }

    async fn wait_for_serial_rate(&self, source_key: &str, delay_ms: u64) {
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
                        return;
                    }
                } else {
                    state.in_flight = true;
                    state.last_start = Some(now);
                    return;
                }
            };
            sleep(wait).await;
        }
    }

    async fn wait_for_window_rate(&self, source_key: &str, limit: usize, window_ms: u64) {
        if limit == 0 || window_ms == 0 {
            return;
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
                    return;
                }
            };
            sleep(wait).await;
        }
    }

    async fn finish_rate(&self, source: &BookSource) {
        let mut states = self.rate_states.write().await;
        if let Some(state) = states.get_mut(&source.book_source_url) {
            state.in_flight = false;
        }
    }

    pub async fn search_book(
        &self,
        user_ns: &str,
        source: &BookSource,
        key: &str,
        page: i32,
    ) -> Result<Vec<SearchBook>, AppError> {
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
        let mut spec = analyze_url(&search_url, key, page, &source.book_source_url, source)
            .map_err(|e| {
                tracing::error!("analyze_url failed: {:?}", e);
                e
            })?;

        self.apply_source_cookie(user_ns, source, &mut spec.headers)
            .await;

        tracing::debug!("search_book fetched spec: {:?}", spec);
        let res = self.fetch_with_rate(user_ns, source, spec).await.map_err(|e| {
            tracing::error!("fetch failed: {:?}", e);
            e
        })?;
        let res = apply_login_check_js(source, res);
        tracing::debug!("fetch success, body length: {}", res.body.len());
        let books = self.parser.search_books(source, &res.body, &res.url);
        tracing::info!("found {} books", books.len());
        Ok(books)
    }

    pub async fn explore_book(
        &self,
        user_ns: &str,
        source: &BookSource,
        rule_find_url: &str,
        page: i32,
    ) -> Result<Vec<SearchBook>, AppError> {
        if rule_find_url.trim().is_empty() {
            return Err(AppError::BadRequest("ruleFindUrl required".to_string()));
        }
        let mut spec = analyze_url(rule_find_url, "", page, &source.book_source_url, source)?;

        self.apply_source_cookie(user_ns, source, &mut spec.headers)
            .await;

        let res = apply_login_check_js(source, self.fetch_with_rate(user_ns, source, spec).await?);
        Ok(self.parser.explore_books(source, &res.body, &res.url))
    }

    pub fn explore_kinds(&self, source: &BookSource) -> Result<Vec<ExploreKind>, AppError> {
        parse_explore_kinds(source)
    }

    pub async fn test_book_source_availability(
        &self,
        user_ns: &str,
        source: &BookSource,
        keyword: Option<&str>,
    ) -> BookSourceAvailability {
        let keyword = keyword
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .or_else(|| {
                source
                    .rule_search
                    .as_ref()
                    .and_then(|rule| rule.check_key_word.as_deref())
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
            })
            .unwrap_or("测试")
            .to_string();

        let (search_ok, search_error) = if source
            .search_url
            .as_deref()
            .is_some_and(|value| !value.trim().is_empty())
            && source.rule_search.is_some()
        {
            match self.search_book(user_ns, source, &keyword, 1).await {
                Ok(books) => (!books.is_empty(), None),
                Err(err) => (false, Some(format!("{err:?}"))),
            }
        } else {
            (false, Some("missing searchUrl or ruleSearch".to_string()))
        };

        let explore_url = self.explore_kinds(source).ok().and_then(|kinds| {
            kinds
                .into_iter()
                .filter_map(|kind| kind.url)
                .map(|url| url.trim().to_string())
                .find(|url| !url.is_empty())
        });
        let (explore_ok, explore_error) = if let Some(url) = explore_url.as_deref() {
            match self.explore_book(user_ns, source, url, 1).await {
                Ok(books) => (!books.is_empty(), None),
                Err(err) => (false, Some(format!("{err:?}"))),
            }
        } else {
            (false, Some("missing explore category url".to_string()))
        };

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
                let books = self.parser.search_books(source, &res.body, &res.url);
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
                let info = self.parser.book_info(source, &res.body, &res.url, &url);
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
                    self.parser.chapter_list(source, &res.body, &res.url, Some(toc_url));
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
                    self.parse_content_blocking(source, &res, url, None).await?;
                let next = self.parser.next_content_url(source, &res.body, &res.url);
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

    pub async fn login_book_source(
        &self,
        source: &BookSource,
    ) -> Result<serde_json::Value, AppError> {
        let login_url = source
            .login_url
            .clone()
            .filter(|v| !v.trim().is_empty())
            .ok_or_else(|| AppError::BadRequest("missing loginUrl".to_string()))?;

        let spec = analyze_url(&login_url, "", 1, &source.book_source_url, source)?;

        let res = self.fetch_with_rate("default", source, spec).await?;
        let check_result = if let Some(login_check_js) = source
            .login_check_js
            .as_deref()
            .filter(|s| !s.trim().is_empty())
        {
            Some(with_source(source,|| {
                eval_js(login_check_js, &res.body, &res.url).unwrap_or_default()
            }))
        } else {
            None
        };

        Ok(serde_json::json!({
            "success": true,
            "status": res.status,
            "url": res.url,
            "checkResult": check_result,
            "bodyPreview": res.body.chars().take(500).collect::<String>(),
            "bodyHtml": res.body
        }))
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
        Ok(self.parser.book_info(source, &res.body, &res.url, book_url))
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
            self.parser.chapter_list(source, &res.body, &res.url, Some(toc_url));
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
                // This handles cases like m.22biqu.com vs m.22biqu.net
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
                let (chapters, _) = self.parser.chapter_list(
                    &pagination.source,
                    &res.body,
                    &res.url,
                    Some(&pagination.toc_url),
                );

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
                let (chapters, next_urls) = self.parser.chapter_list(
                    &pagination.source,
                    &res.body,
                    &res.url,
                    Some(&pagination.toc_url),
                );

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
            self.parser.chapter_list(source, &res.body, &res.url, Some(toc_url));

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
                let (chapters, _) = self
                    .parser
                    .chapter_list(source, &res.body, &res.url, Some(toc_url));

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
                let (chapters, next_urls) = self
                    .parser
                    .chapter_list(source, &res.body, &res.url, Some(toc_url));

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
        let book = self.get_shelf_book(user_ns, book_url).await.ok().flatten();
        let toc_url = book
            .as_ref()
            .and_then(|b| b.toc_url.clone())
            .filter(|u| !u.is_empty())
            .unwrap_or_else(|| book_url.to_string());
        let chapter = self
            .load_chapter_list_cache(user_ns, &toc_url)
            .await
            .ok()
            .flatten()
            .and_then(|list| list.into_iter().find(|ch| ch.url == chapter_url));
        let book_json = match book {
            Some(b) => json!({
                "name": b.name,
                "author": b.author,
                "bookUrl": b.book_url,
                "tocUrl": b.toc_url,
                "origin": b.origin,
                "originName": b.origin_name,
                "coverUrl": b.cover_url,
                "intro": b.intro,
                "kind": b.kind,
                "wordCount": b.word_count,
                "durChapterIndex": b.dur_chapter_index,
                "durChapterTitle": b.dur_chapter_title,
                "totalChapterNum": b.total_chapter_num,
            }),
            None => json!({ "bookUrl": book_url }),
        };
        let chapter_json = match chapter {
            Some(ch) => serde_json::to_value(ch).unwrap_or_else(|_| json!({})),
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
            let output = with_book_context(book_ctx, || {
                with_source(&source, || {
                    with_source_key(Some(&source_key), || {
                        crate::parser::js::eval_js(&script, &result_text, &base_url)
                    })
                })
            });
            let logs = take_js_logs();
            // 面板 HTML 由后端托管: 前端按 panelId 加载, 面板内的桥才能同源回调宿主
            let browser = take_show_browser_request().map(|mut req| {
                req.panel_id = Some(register_source_panel(SourcePanelEntry {
                    url: req.url.clone(),
                    html: req.html.clone().unwrap_or_default(),
                    book_source_url: source.book_source_url.clone(),
                    book_url: source_key.clone(),
                    chapter_url: base_url.clone(),
                    book_json: book_json.clone(),
                    chapter_json: chapter_json.clone(),
                }));
                req
            });
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
        source: &BookSource,
        res: &FetchResponse,
        source_key: &str,
        book_ctx: Option<JsBookContext>,
    ) -> Result<(String, Vec<String>), AppError> {
        let parser = self.parser.clone();
        let source = source.clone();
        let body = res.body.clone();
        let url = res.url.clone();
        let source_key = source_key.to_string();
        tokio::task::spawn_blocking(move || {
            take_js_logs();
            let content = with_book_context(book_ctx, || {
                parser.content(&source, &body, &url, Some(&source_key))
            });
            (content, take_js_logs())
        })
        .await
        .map_err(|e| AppError::Internal(anyhow::anyhow!("正文解析任务失败: {e}")))
    }

    pub async fn get_content(
        &self,
        user_ns: &str,
        book_url: &str,
        source: &BookSource,
        chapter_url: &str,
    ) -> Result<String, AppError> {
        self.get_content_with_stage(user_ns, book_url, source, chapter_url, None, false)
            .await
    }

    /// 与 `get_content` 相同, 但把加载阶段回调出去, 供前端显示"卡在哪一步"。
    ///
    /// `skip_js_lib` 为 true 时**不加载书源脚本**(jsLib): 正文规则里的插件调用
    /// (如 `addComment(result)`) 会因函数不存在而失败, 从而回退到选择器提取结果
    /// —— 也就是纯正文。离线缓存只需要正文, 没必要为每章付出脚本解析与段评请求
    /// 的代价。
    pub async fn get_content_with_stage(
        &self,
        user_ns: &str,
        book_url: &str,
        source: &BookSource,
        chapter_url: &str,
        on_stage: Option<&(dyn Fn(ContentLoadStage) + Send + Sync)>,
        skip_js_lib: bool,
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
        let cached = self
            .cache
            .get(user_ns, &book_key, chapter_url)
            .await
            .ok()
            .flatten();
        // 离线缓存路径命中即用; 阅读路径则**优先在线解析**(保证段评是最新的),
        // 缓存只作为请求失败时的兜底 —— 离线缓存里只有纯正文, 直接用它就看不到段评。
        if skip_js_lib {
            if let Some(hit) = cached.clone() {
                tracing::debug!("get_content returning cached content, len={}", hit.len());
                return Ok(hit);
            }
        }
        tracing::debug!("get_content cache miss, fetching from network");

        let mut all_content = String::new();
        let mut visited_urls = std::collections::HashSet::new();
        let mut current_url = chapter_url.to_string();
        let book_ctx = self.js_book_context(user_ns, book_url, chapter_url).await;
        // 只缓存正文时用一份不带 jsLib 的书源副本(见 `skip_js_lib` 的说明)
        let stripped_source: Option<BookSource> = skip_js_lib
            .then(|| BookSource { js_lib: None, ..source.clone() });
        let parse_source = stripped_source.as_ref().unwrap_or(source);

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
                Err(e) => {
                    // 网络失败(离线或书源故障)时回退到已缓存正文, 保证离线仍可阅读
                    if let Some(hit) = cached.clone() {
                        tracing::debug!("get_content fetch failed, falling back to cache: {e}");
                        return Ok(hit);
                    }
                    return Err(e);
                }
            };
            tracing::debug!("get_content fetch done, body len={}", res.body.len());
            // 正文规则里往往会触发书源脚本与段评请求, 这是最耗时的一步
            notify(ContentLoadStage::Parsing);
            let (content, _logs) = self
                .parse_content_blocking(parse_source, &res, book_url, book_ctx.clone())
                .await?;
            tracing::debug!("get_content parsed content len={}", content.len());

            if !content.is_empty() {
                if !all_content.is_empty() {
                    all_content.push('\n');
                }
                all_content.push_str(&content);
            }

            // Check for next page
            if let Some(next_url) = self.parser.next_content_url(source, &res.body, &res.url) {
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
                            // Check if paths share a common prefix (e.g., /biqu104/)
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
        let _write_guard = self.bookshelf_write_lock.lock().await;
        let mut list = self.read_bookshelf(user_ns).await?;
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
        let content = self
            .get_content_with_stage(user_ns, book_url, source, chapter_url, None, true)
            .await?;
        // 正文规则若是**纯 JS**（没有选择器部分，如 `@js:clean(result)`），跳过书源
        // 脚本会让整条规则失败并得到空内容。这类书源回退到完整解析（含 jsLib）再试。
        if content.trim().is_empty() {
            tracing::debug!("cache_chapter: 跳过书源脚本后正文为空, 回退完整解析");
            if let Err(e) = self
                .get_content_with_stage(user_ns, book_url, source, chapter_url, None, false)
                .await
            {
                tracing::warn!("cache_chapter 回退完整解析失败: {e}");
            }
        }
        // 以「缓存是否真的落盘」为准：空正文不会被写入缓存（见 get_content_with_stage
        // 里的 is_empty 判断）。若仍未落盘就报错计入失败数，否则进度会把没缓存的
        // 章节算作成功，且下次缓存时这些章又被判为未缓存。
        if !self.is_chapter_cached(user_ns, book_url, chapter_url).await {
            return Err(AppError::Internal(anyhow::anyhow!(
                "正文解析为空, 未写入缓存"
            )));
        }
        Ok(())
    }

    pub async fn get_cover(&self, user_ns: &str, url: &str) -> Result<(Vec<u8>, String), AppError> {
        let ext = file_ext_from_url(url).unwrap_or_else(|| "png".to_string());
        let name = md5_hex(url);
        let path = self
            .storage_dir
            .join("cache")
            .join(user_ns)
            .join("cover")
            .join(format!("{}.{}", name, ext));
        if path.exists() {
            let data = fs::read(&path)
                .await
                .map_err(|e| AppError::Internal(e.into()))?;
            let content_type = content_type_from_ext(&ext);
            return Ok((data, content_type));
        }
        if let Some(parent) = path.parent() {
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

        let mut req = self.http.client().get(url);

        // Add necessary headers to bypass anti-hotlinking
        req = req
            .header("User-Agent", "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36")
            .header("Accept", "image/avif,image/webp,image/apng,image/svg+xml,image/*,*/*;q=0.8");

        if let Some(ref referer) = referer {
            req = req.header("Referer", referer);
        }

        let res = req.send().await.map_err(|e| AppError::Internal(e.into()))?;
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
        let _ = fs::write(&path, &bytes).await;
        Ok((bytes, content_type))
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
            .join(format!("{}.json", name))
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
        }
        Ok(list)
    }

    async fn write_bookshelf(&self, user_ns: &str, list: &Vec<Book>) -> Result<(), AppError> {
        let path = self.bookshelf_path(user_ns);
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

    // Chapter list cache methods
    fn chapter_list_cache_path(&self, user_ns: &str, toc_url: &str) -> PathBuf {
        let name = md5_hex(toc_url);
        self.storage_dir
            .join("data")
            .join(user_ns)
            .join("chapters")
            .join(format!("{}.json", name))
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
        if !book.book_url.is_empty() {
            let _ = self.delete_book_cache(user_ns, &book.book_url).await;
            let _ = self
                .delete_book_sources_cache(user_ns, &book.book_url)
                .await;
            let _ = self
                .delete_chapter_list_cache(user_ns, &book.book_url)
                .await;
        }
        if let Some(toc_url) = &book.toc_url {
            if !toc_url.is_empty() {
                let _ = self.delete_chapter_list_cache(user_ns, toc_url).await;
            }
        }
        Ok(())
    }
}

fn apply_login_check_js(source: &BookSource, res: FetchResponse) -> FetchResponse {
    let Some(script) = source
        .login_check_js
        .as_deref()
        .filter(|script| !script.trim().is_empty())
    else {
        return res;
    };

    with_source(source,|| {
        let str_response = StrResponse::from(res.clone());
        let mut bindings = HashMap::new();
        bindings.insert(
            "result".to_string(),
            serde_json::to_value(&str_response).unwrap_or_else(|_| json!({})),
        );
        match eval_js_with_bindings(script, &res.body, &res.url, &bindings) {
            Ok(output) if !output.trim().is_empty() => {
                if let Ok(next) = serde_json::from_str::<StrResponse>(&output) {
                    FetchResponse::from(next)
                } else {
                    FetchResponse {
                        body: output,
                        ..res
                    }
                }
            }
            Ok(_) => res,
            Err(err) => {
                tracing::warn!(
                    "loginCheckJs failed for {}: {:?}",
                    source.book_source_name,
                    err
                );
                res
            }
        }
    })
}

fn parse_explore_kinds(source: &BookSource) -> Result<Vec<ExploreKind>, AppError> {
    let Some(raw) = source
        .explore_url
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        return Ok(Vec::new());
    };

    let text = with_source(source,|| {
        if let Some(script) = raw.strip_prefix("@js:") {
            eval_js(script, "", &source.book_source_url).map_err(AppError::Internal)
        } else if let Some(script) = raw
            .strip_prefix("<js>")
            .and_then(|value| value.strip_suffix("</js>"))
        {
            eval_js(script, "", &source.book_source_url).map_err(AppError::Internal)
        } else {
            Ok(raw.to_string())
        }
    })?;

    for json_text in [&text, &normalize_relaxed_explore_json(&text)] {
        if let Ok(kinds) = serde_json::from_str::<Vec<ExploreKind>>(json_text) {
            return Ok(kinds
                .into_iter()
                .filter(|kind| !kind.title.trim().is_empty())
                .collect());
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
                style: None,
            })
        })
        .collect())
}

fn normalize_relaxed_explore_json(text: &str) -> String {
    let mut normalized = String::with_capacity(text.len());
    let mut in_string = false;
    let mut quote = '\0';
    let mut escaped = false;

    for ch in text.chars() {
        if in_string {
            normalized.push(ch);
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == quote {
                in_string = false;
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

fn parse_window_rate(rate: &str) -> Option<(usize, u64)> {
    let (limit, window) = rate.split_once('/')?;
    let limit = limit.trim().parse().ok()?;
    let window = window.trim().parse().ok()?;
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
        .position(|item| normalize_source_url(&item.origin) == origin)
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

fn sanitize_book_urls(book: &mut Book) {
    book.book_url = repair_encoded_url(&book.book_url);
    book.origin = normalize_source_url(&book.origin);
    if let Some(toc_url) = &book.toc_url {
        book.toc_url = Some(repair_encoded_url(toc_url));
    }
    if let Some(cover_url) = &book.cover_url {
        book.cover_url = Some(repair_encoded_url(cover_url));
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

    #[test]
    fn detect_anti_crawler_flags_pc_verification_page() {
        // 某小说站 PC 站反爬: 202 + var buid
        let warnings = detect_anti_crawler(202, "var buid = xxx", &[]);
        assert!(
            warnings.iter().any(|w| w.contains("反爬")),
            "应提示反爬, 实际: {warnings:?}"
        );
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
        let first = BookService::source_cookie_key("default", "https://a.example.com");
        let second = BookService::source_cookie_key("default", "https://b.example.com");
        assert_ne!(first, second);
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
}
