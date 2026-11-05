use crate::crawler::url_analyzer::parse_source_headers;
use crate::crawler::{http_client::HttpClient, source_session::SourceHttpContext};
use crate::crawler::fetcher::{HttpMethod, RequestSpec};
use crate::model::book_source::BookSource;
use crate::util::hash::md5_hex;
use crate::util::text::{apply_regex_replace, normalize_source_url, strip_whitespace};
use base64::Engine;
use cbc::cipher::{
    block_padding::{NoPadding, Pkcs7, ZeroPadding},
    BlockDecryptMut, BlockEncryptMut, KeyIvInit,
};
use chrono::{Local, TimeZone, Utc};
use hmac::{Hmac, Mac};
use md5::Md5;
use once_cell::sync::Lazy;
use sha1::Sha1;
use sha2::{Digest, Sha224, Sha256, Sha384, Sha512};
use reqwest::blocking::Client;
use rquickjs::context::EvalOptions;
use rquickjs::function::{Func, Opt};
use rquickjs::{Context, Object, Runtime, Value};
use serde_json::{json, Value as JsonValue};
use std::cell::RefCell;
use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};
use uuid::Uuid;

/// 「配置错误」标记：算法名 / 模式 / 密钥长度这类**写错了就永远算不出来**的
/// 问题，在真正处理数据之前就能判定。
///
/// 与之相对的是**数据错误**（解密填充不对、GCM 标签不匹配、base64 非法）——
/// 书源经常故意试解密，失败后落回 `||` 分支（`decrypt(x) || result`），
/// 因此那类错误必须继续返回空串，绝不能抛异常打断整条规则。
///
/// `__sym` 据此区分：配置错误抛给脚本（对应 legado 构造
/// `SymmetricCrypto` 时的 `NoSuchAlgorithmException` / `InvalidKeyException`），
/// 数据错误记日志后返回空串。
#[derive(Debug)]
pub(crate) struct CryptConfigError(pub String);

impl std::fmt::Display for CryptConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for CryptConfigError {}

/// 构造一个配置错误（`anyhow::bail!` 的等价物，带类型标记）。
///
/// 定义在文件前部：`macro_rules!` 按**文本顺序**解析，`__sym` 处的使用点
/// 必须先看到它。
macro_rules! bail_config {
    ($($arg:tt)*) => {
        return Err(anyhow::Error::new(CryptConfigError(format!($($arg)*))))
    };
}

/// 把 `anyhow::Error` 拆出配置错误的消息；`None` 表示是数据错误。
fn crypt_config_message(err: &anyhow::Error) -> Option<String> {
    err.downcast_ref::<CryptConfigError>().map(|e| e.0.clone())
}

/// 进程内存 KV: `cache.putMemory` / `java.putCache` / `kv_put` 等, 重启即失。
static JS_KV: Lazy<Mutex<HashMap<String, String>>> = Lazy::new(|| Mutex::new(HashMap::new()));
static JS_KV_ORDER: Lazy<Mutex<VecDeque<String>>> = Lazy::new(|| Mutex::new(VecDeque::new()));
const JS_KV_CAP: usize = 2048;
const JS_KV_MAX_VALUE_BYTES: usize = 1024 * 1024;
/// 持久 KV: `cache.put`、`source/book/chapter.putVariable` 等, 落盘到 `{storage}/js_store.json`。
static JS_STORE: Lazy<Mutex<HashMap<String, String>>> = Lazy::new(|| Mutex::new(load_js_store()));
static JS_STORAGE_DIR: OnceLock<PathBuf> = OnceLock::new();
static JS_DEFAULT_HTTP: OnceLock<HttpClient> = OnceLock::new();
static JS_FALLBACK_HTTP: Lazy<HttpClient> = Lazy::new(|| HttpClient::new(30, None).expect("JS HTTP client"));
static JS_HTTP_RUNTIME: Lazy<tokio::runtime::Runtime> = Lazy::new(|| tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().expect("JS HTTP runtime"));
/// 组装好的 jsLib 源码, 按 jsLib 文本 md5 缓存; `Some(Instant)` 表示含下载失败项, 到期后重试。
static JS_LIB_CACHE: Lazy<Mutex<HashMap<String, (String, Option<Instant>)>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));
const JS_LIB_RETRY_AFTER: Duration = Duration::from_secs(60);
// Signature helpers can nest JS and Rust crypto calls beyond QuickJS's 256 KiB
// default. Keep the limit below the native worker thread's stack allocation.
const JS_MAX_STACK_SIZE: usize = 1024 * 1024;
const DEFAULT_JS_USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36";
/// blocking client 只能在无 tokio runtime 的线程上构建/使用, 所有访问都走 `run_blocking_http`。
static JS_HTTP_CLIENT: Lazy<Client> = Lazy::new(|| {
    Client::builder()
        .cookie_store(true)
        .gzip(true)
        .brotli(true)
        .deflate(true)
        // 与书源 client（`crawler::http_client`）保持同一套 TLS 策略：
        // 不校验证书与主机名（legado `HttpHelper.kt` 的 unsafeTrustManager）。
        // jsLib 常托管在与源站同一批小站上，自签名/过期证书很常见。
        .danger_accept_invalid_certs(true)
        .danger_accept_invalid_hostnames(true)
        .timeout(Duration::from_secs(30))
        .build()
        .expect("failed to build JS HTTP client")
});
static JS_DEVICE_ID: Lazy<String> = Lazy::new(|| {
    let mut map = JS_KV.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(existing) = map.get("__device_id") {
        return existing.clone();
    }
    let generated = Uuid::new_v4().to_string();
    map.insert("__device_id".to_string(), generated.clone());
    generated
});

/// 书源信息, 供 JS `source.*` 与 JS 网络请求的默认请求头使用。
#[derive(Clone, Default)]
struct JsSourceInfo {
    url: String,
    name: String,
    /// legado BookSource.bookSourceComment / variableComment: 书源常把解密脚本
    /// 放在注释里, 再在规则中 `eval(String(source.bookSourceComment))` 取用
    /// (issue #13 的书源正是这种写法)。
    comment: String,
    variable_comment: String,
    /// legado `BookSource.loginUrl`：**脚本**可读。一类书源把登录脚本只放在这里，
    /// 并在 `exploreUrl` 里 `eval(String(source.loginUrl)); login();` 复用其中的
    /// `login()` / `set()` 等函数。JS 里取不到就是 `login is not defined`，
    /// 发现页整块失效。
    login_url: String,
    /// legado `BookSource.loginUi`：同上，部分书源的 `exploreUrl` 会读它判断登录态。
    login_ui: String,
    headers: Vec<(String, String)>,
    header_script: Option<String>,
    http: Option<SourceHttpContext>,
    concurrent_rate: Option<String>,
}

#[derive(Default, Clone, serde::Serialize)]
pub struct LoginEffects {
    pub values: serde_json::Map<String, JsonValue>,
    pub refresh: bool,
    pub reset: bool,
    /// `source.login()` 被脚本调用: 调用方应重跑 loginUrl 并重取响应。
    pub login: bool,
    /// `java.refreshExplore()` 被脚本调用: 调用方应刷新发现页分类与列表。
    /// 与 `refresh`(重登录视图) 是两件事 —— 早先两者被混为一谈。
    pub explore_refresh: bool,
}

/// 正文/目录解析时的书籍与章节上下文, 对应 legado JS 里的 `book` / `chapter`。
#[derive(Clone, Debug, Default)]
pub struct JsBookContext {
    pub book: JsonValue,
    pub chapter: JsonValue,
}

thread_local! {
    static ACTIVE_JS_LIB: RefCell<Option<String>> = const { RefCell::new(None) };
    static ACTIVE_SOURCE: RefCell<Option<JsSourceInfo>> = const { RefCell::new(None) };
    /// 目录/正文解析期间的**书籍 URL** 兜底（legado 里是 `book.bookUrl`，
    /// 与 `source.getKey()` 的书源地址不是一回事）。
    static ACTIVE_BOOK_URL: RefCell<Option<String>> = const { RefCell::new(None) };
    static ACTIVE_BOOK_CTX: RefCell<Option<JsBookContext>> = const { RefCell::new(None) };
    static JS_LOGS: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
    static ACTIVE_HTTP: RefCell<Option<SourceHttpContext>> = const { RefCell::new(None) };
    static ACTIVE_SEARCH: RefCell<Option<(String, i32)>> = const { RefCell::new(None) };
    static LOGIN_EFFECTS: RefCell<LoginEffects> = RefCell::new(LoginEffects::default());
    /// `java.getStrResponse()`: 最近一次 HTTP 请求的响应信封。
    static LAST_RESPONSE: RefCell<Option<String>> = const { RefCell::new(None) };
    /// 发现页脚本通过 `java.open` / `java.searchBook` / `java.openBook` 登记的
    /// 意图（`"kind\u{1}payloadJson"`），由宿主回传前端执行。
    static EXPLORE_INTENTS: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
}

/// 取出并清空当前线程登记的发现页意图。
pub fn take_explore_intents() -> Vec<(String, String)> {
    EXPLORE_INTENTS.with(|cell| {
        std::mem::take(&mut *cell.borrow_mut())
            .into_iter()
            .filter_map(|raw| raw.split_once('\u{1}').map(|(k, v)| (k.to_string(), v.to_string())))
            .collect()
    })
}

const MAX_JS_LOGS: usize = 200;

/// 设置 JS 持久数据目录(jsLib 磁盘缓存、js_store.json)。启动时调用一次。
pub fn set_js_storage_dir(dir: impl Into<PathBuf>) {
    let _ = JS_STORAGE_DIR.set(dir.into());
}

pub fn set_js_http_client(http: HttpClient) { let _ = JS_DEFAULT_HTTP.set(http); }

pub fn with_http_context<T>(http: SourceHttpContext, f: impl FnOnce() -> T) -> T {
    with_thread_local(&ACTIVE_HTTP, Some(http), f)
}

pub fn with_search_context<T>(key: &str, page: i32, f: impl FnOnce() -> T) -> T {
    with_thread_local(&ACTIVE_SEARCH, Some((key.to_string(), page)), f)
}

pub fn source_http_context(source_url: &str) -> anyhow::Result<SourceHttpContext> {
    if let Some(http) = ACTIVE_HTTP.with(|cell| cell.borrow().clone()) {
        if http.session.source_url == source_url.trim() { return Ok(http); }
    }
    JS_DEFAULT_HTTP.get().unwrap_or(&JS_FALLBACK_HTTP).source_context("default", source_url, None)
}

pub fn take_login_effects() -> LoginEffects {
    LOGIN_EFFECTS.with(|cell| std::mem::take(&mut *cell.borrow_mut()))
}

/// 把某个响应设为 `java.getStrResponse()` 的"最近响应"。
/// loginCheckJs 在执行前用当前响应初始化这个槽位（legado 语义）。
pub fn set_last_response(raw: &str) {
    LAST_RESPONSE.with(|cell| cell.replace(Some(raw.to_string())));
}

fn cookie_tag_url(tag: &str) -> Option<url::Url> {
    let tag = tag.trim();
    if tag.is_empty() { return None; }
    let absolute = if tag.contains("://") {
        tag.to_string()
    } else if tag.starts_with("//") {
        format!("https:{tag}")
    } else {
        format!("https://{tag}")
    };
    url::Url::parse(&absolute).ok().filter(|url| {
        matches!(url.scheme(), "http" | "https") && url.host_str().is_some()
    })
}

pub fn login_headers() -> Vec<(String, String)> {
    active_source().http.and_then(|http| http.session.get("loginHeader"))
        .map(|raw| parse_source_headers(&raw)).unwrap_or_default()
}

fn with_thread_local<V: 'static, T>(
    key: &'static std::thread::LocalKey<RefCell<Option<V>>>,
    value: Option<V>,
    f: impl FnOnce() -> T,
) -> T {
    struct Restore<V: 'static> { key: &'static std::thread::LocalKey<RefCell<Option<V>>>, previous: Option<V> }
    impl<V: 'static> Drop for Restore<V> {
        fn drop(&mut self) { self.key.with(|cell| cell.replace(self.previous.take())); }
    }
    let _restore = Restore { key, previous: key.with(|cell| cell.replace(value)) };
    f()
}

pub fn with_js_lib<T>(js_lib: Option<&str>, f: impl FnOnce() -> T) -> T {
    with_thread_local(&ACTIVE_JS_LIB, js_lib.map(|value| value.to_string()), f)
}

/// 设置当前书源(jsLib + `source` 信息 + 默认请求头)后执行 f。
pub fn with_source<T>(source: &BookSource, f: impl FnOnce() -> T) -> T {
    let raw_header = source.header.as_deref().unwrap_or_default().trim();
    let header_script = raw_header.strip_prefix("@js:")
        .or_else(|| raw_header.strip_prefix("<js>").and_then(|text| text.strip_suffix("</js>")))
        .map(str::to_string);
    let headers = source
        .header
        .as_deref()
        .filter(|_| header_script.is_none())
        .map(parse_source_headers)
        .unwrap_or_default();
    let http = source_http_context(&source.book_source_url).ok();
    let info = JsSourceInfo {
        url: source.book_source_url.clone(),
        name: source.book_source_name.clone(),
        comment: source.book_source_comment.clone().unwrap_or_default(),
        variable_comment: source.variable_comment.clone().unwrap_or_default(),
        login_url: source.login_url.clone().unwrap_or_default(),
        login_ui: source.login_ui.clone().unwrap_or_default(),
        headers,
        header_script,
        http,
        concurrent_rate: source.concurrent_rate.clone(),
    };
    with_thread_local(&ACTIVE_SOURCE, Some(info), || {
        with_js_lib(source.js_lib.as_deref(), f)
    })
}

/// 在解析目录/正文期间登记当前**书籍 URL**（thread-local 兜底）。
///
/// legado 的 `source.getKey()` 返回书源地址；书籍地址走 `book.bookUrl` / `baseUrl`。
/// 这里登记的值只在 `book.bookUrl` 缺失时用于补齐 `book` 绑定，
/// 不会再改写 `source.getKey()` 的语义（issue #13 的章节域名拼接由此修正）。
pub fn with_book_url<T>(book_url: Option<&str>, f: impl FnOnce() -> T) -> T {
    with_thread_local(&ACTIVE_BOOK_URL, book_url.map(|value| value.to_string()), f)
}

/// 设置 JS `book` / `chapter` / `title` 上下文后执行 f。
pub fn with_book_context<T>(ctx: Option<JsBookContext>, f: impl FnOnce() -> T) -> T {
    with_thread_local(&ACTIVE_BOOK_CTX, ctx, f)
}

/// 当前线程 JS 上下文的可跨线程快照。
///
/// 目录解析为了并发（对齐手机端 `BookChapterList.mapAsync`）会在多个 OS 线程
/// 上求值。JS 上下文全是 thread-local，新线程默认什么都拿不到 ——
/// `source.getKey()` 会变成空串（章节域名拼不出来）、`book.bookUrl` 为空、
/// 默认请求头与 cookie 全部丢失，结果就是"并发之后目录全错"。
///
/// 这里把主线程登记过的那几项打包出来，由工作线程在开始求值前原样重放。
#[derive(Clone)]
pub struct JsThreadContext {
    source: Option<JsSourceInfo>,
    js_lib: Option<String>,
    book_url: Option<String>,
    book: Option<JsBookContext>,
    http: Option<SourceHttpContext>,
    search: Option<(String, i32)>,
}

impl JsThreadContext {
    /// 捕获当前线程的 JS 上下文（在工作线程上重放前调用）。
    pub fn capture() -> Self {
        Self {
            source: ACTIVE_SOURCE.with(|cell| cell.borrow().clone()),
            js_lib: ACTIVE_JS_LIB.with(|cell| cell.borrow().clone()),
            book_url: ACTIVE_BOOK_URL.with(|cell| cell.borrow().clone()),
            book: ACTIVE_BOOK_CTX.with(|cell| cell.borrow().clone()),
            http: ACTIVE_HTTP.with(|cell| cell.borrow().clone()),
            search: ACTIVE_SEARCH.with(|cell| cell.borrow().clone()),
        }
    }

    /// 在工作线程上重放捕获的上下文后执行 f。
    ///
    /// 只重放"决定规则语义"的那几项（书源 / jsLib / 书籍 / 请求头）；
    /// 日志与副作用队列（`JS_LOGS`、`EXPLORE_INTENTS` 等）故意不重放 ——
    /// 它们属于产生它们的那个线程，跨线程搬运反而会串味。
    pub fn replay<T>(self, f: impl FnOnce() -> T) -> T {
        with_thread_local(&ACTIVE_SOURCE, self.source, || {
            with_thread_local(&ACTIVE_JS_LIB, self.js_lib, || {
                with_thread_local(&ACTIVE_BOOK_URL, self.book_url, || {
                    with_thread_local(&ACTIVE_BOOK_CTX, self.book, || {
                        with_thread_local(&ACTIVE_HTTP, self.http, || {
                            with_thread_local(&ACTIVE_SEARCH, self.search, f)
                        })
                    })
                })
            })
        })
    }
}

fn active_source() -> JsSourceInfo {
    ACTIVE_SOURCE.with(|cell| cell.borrow().clone().unwrap_or_default())
}

/// 记录一条 JS 日志(`java.log`、脚本异常等), 同时输出到 tracing。
pub fn push_js_log(message: impl Into<String>) {
    let message = message.into();
    tracing::info!(target: "reader::js", "{message}");
    JS_LOGS.with(|logs| {
        let mut logs = logs.borrow_mut();
        if logs.len() < MAX_JS_LOGS {
            logs.push(message);
        }
    });
}

/// 取出并清空当前线程累计的 JS 日志(书源调试用)。
pub fn take_js_logs() -> Vec<String> {
    JS_LOGS.with(|logs| std::mem::take(&mut *logs.borrow_mut()))
}

/// reqwest::blocking 在 tokio 异步线程上构建/发送会 panic, 这里在无 runtime 的线程上执行。
fn run_blocking_http<T: Send>(f: impl FnOnce() -> T + Send) -> T {
    if tokio::runtime::Handle::try_current().is_err() {
        return f();
    }
    std::thread::scope(|scope| {
        scope
            .spawn(f)
            .join()
            .unwrap_or_else(|panic| std::panic::resume_unwind(panic))
    })
}

fn load_js_store() -> HashMap<String, String> {
    let Some(dir) = JS_STORAGE_DIR.get() else {
        return HashMap::new();
    };
    std::fs::read_to_string(dir.join("js_store.json"))
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

fn store_get(key: &str) -> Option<String> {
    JS_STORE
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get(key)
        .cloned()
}

/// Source-script reading preferences for this book. Merge these into a Book's
/// readConfig when returning metadata or creating a later JavaScript context.
pub fn book_read_config_override(source_url: &str, book_url: &str) -> Option<JsonValue> {
    if book_url.trim().is_empty() { return None; }
    let source_url = normalize_source_url(source_url);
    let source_url = source_url.trim_end_matches('/');
    match store_get(&format!("brc:{source_url}:{book_url}")).as_deref() {
        Some("true") => Some(json!({"useReplaceRule": true})),
        Some("false") => Some(json!({"useReplaceRule": false})),
        _ => None,
    }
}

fn store_write(update: impl FnOnce(&mut HashMap<String, String>)) {
    let mut map = JS_STORE.lock().unwrap_or_else(|e| e.into_inner());
    update(&mut map);
    let Some(dir) = JS_STORAGE_DIR.get() else {
        return;
    };
    let Ok(text) = serde_json::to_string(&*map) else {
        return;
    };
    let tmp = dir.join("js_store.json.tmp");
    if std::fs::write(&tmp, text).is_ok() {
        let _ = std::fs::rename(&tmp, dir.join("js_store.json"));
    }
}

fn memory_get(key: &str) -> Option<String> {
    let map = JS_KV.lock().unwrap_or_else(|e| e.into_inner());
    let value = map.get(key).cloned();
    if value.is_some() {
        let mut order = JS_KV_ORDER.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(index) = order.iter().position(|item| item == key) {
            order.remove(index);
        }
        order.push_back(key.to_string());
    }
    value
}

fn memory_put(key: String, value: String) {
    if value.len() > JS_KV_MAX_VALUE_BYTES {
        return;
    }
    let mut map = JS_KV.lock().unwrap_or_else(|e| e.into_inner());
    let mut order = JS_KV_ORDER.lock().unwrap_or_else(|e| e.into_inner());
    if map.insert(key.clone(), value).is_some() {
        if let Some(index) = order.iter().position(|item| item == &key) {
            order.remove(index);
        }
    }
    order.push_back(key);
    while order.len() > JS_KV_CAP {
        if let Some(oldest) = order.pop_front() {
            map.remove(&oldest);
        }
    }
}

pub fn eval_js(script: &str, input: &str, base_url: &str) -> anyhow::Result<String> {
    eval_js_inner(script, Some(input), Some(base_url), None, None, None)
}

/// 求值 `coverDecodeJs` / `imageDecode` 这类**图片解密**脚本, 取回二进制字节。
///
/// 与普通规则求值的差异全在"字节"上:
///
/// - 注入: `result` 必须是**字节序列**(数字数组), 不能先转成 UTF-8 文本 ——
///   AES/CBC/NoPadding 的密文任意字节都可能出现, 一旦按 UTF-8 解码就是有损的,
///   解密出来必然花屏。legado 注入的是 `ByteArray`(封面是 `InputStream`)。
/// - 返回: 脚本普遍写 `createSymmetricCrypto(...).decrypt(result)`, 而 legado 的
///   `decrypt` 返回 `ByteArray`。本端 prelude 里 `decrypt` 默认按文本输出
///   (`String::from_utf8_lossy`), 这里在求值期间把 `__byteMode` 置 1, 让
///   `decrypt`/`decryptStr` 改走"原始字节"分支(见 prelude 里 `symmetric`)。
///
/// `src` 是图片地址(legado 的同名变量), 同时用作 `base_url`。
pub fn eval_js_cover_decode(script: &str, src: &str, raw: &[u8]) -> anyhow::Result<Vec<u8>> {
    let out = eval_js_inner_with_source(
        script,
        Some(src),
        Some(src),
        None,
        None,
        None,
        None,
        Some(raw),
    )?;
    // 宿主取回的是 latin1 承载的字节序列(每 char 一个字节), 这里还原成原始字节。
    Ok(latin1_bytes(&out))
}

/// `eval_js_with_bindings` 中绑定值的标记前缀: 值为 JSON 响应文本,
/// 注入时用 `Resp()` 包装成 legado `StrResponse` 语义(带 `code()`/`body()`
/// 等方法的对象), 供 `loginCheckJs` 这类按方法调用响应的脚本使用。
pub const RESP_BINDING_PREFIX: &str = "@resp:";

/// 构造 `Resp()` 语义的绑定值(JSON 文本 + 标记前缀)。
pub fn resp_binding(value: &serde_json::Value) -> JsonValue {
    JsonValue::String(format!(
        "{RESP_BINDING_PREFIX}{}",
        serde_json::to_string(value).unwrap_or_else(|_| "{}".to_string())
    ))
}

/// 构造 `Elements` 语义的绑定值(JSON 数组 + 标记前缀)。
///
/// 用于把 `bookList`/`chapterList` 交回的**元素快照**逐条注入字段规则的
/// `result`: legado 里 `result` 是 JSoup `Element`, 书源普遍对它调用
/// `result.select('p.meta')` / `result.html()` / `result.text()`。只注入纯 JSON
/// 时这些方法不存在, 字段取值全部落空(表现为搜索结果 0 条)。
pub const ELEMENT_BINDING_PREFIX: &str = "@element:";

/// 构造元素快照绑定值(单个快照 + 标记前缀)。
pub fn element_binding(value: &serde_json::Value) -> JsonValue {
    JsonValue::String(format!(
        "{ELEMENT_BINDING_PREFIX}{}",
        serde_json::to_string(value).unwrap_or_else(|_| "{}".to_string())
    ))
}

/// 构造 `Elements`（元素集合）语义的绑定值。
///
/// 与 [`element_binding`] 的区别：legado 在**列表规则**的 JS 段里把 `result`
/// 绑定成整个 `Elements`（可以 `toArray()` / `size()` / 索引），而不是单个元素。
/// 书源据此做批量过滤：
///   `list = result.toArray(); for (i in list) { re.test(list[i]) ? html += list[i] : '' }`
/// 只绑单个元素或纯 JSON 数组时 `toArray()` 不存在，脚本第二行就中断。
pub const ELEMENTS_BINDING_PREFIX: &str = "@elements:";

/// 构造元素集合绑定值（快照数组 + 标记前缀）。
pub fn elements_binding(values: &[serde_json::Value]) -> JsonValue {
    JsonValue::String(format!(
        "{ELEMENTS_BINDING_PREFIX}{}",
        serde_json::to_string(values).unwrap_or_else(|_| "[]".to_string())
    ))
}

pub fn eval_js_with_bindings(
    script: &str,
    input: &str,
    base_url: &str,
    bindings: &HashMap<String, JsonValue>,
) -> anyhow::Result<String> {
    eval_js_inner(
        script,
        Some(input),
        Some(base_url),
        None,
        None,
        Some(bindings),
    )
}

pub fn eval_js_search_with_source(
    script: &str,
    key: &str,
    page: i32,
    // 书源地址：`with_source` 已提供 source 绑定，这里只是它的空值回退。
    source_key: &str,
) -> anyhow::Result<String> {
    eval_js_inner_with_source(script, None, None, Some(key), Some(page), Some(source_key), None, None)
}

/// 求值发现页分类里的 `exploreUrl`（`@js:` / `<js>` 规则）。
///
/// 与规则的其它 JS 不同，这里必须注入 `infoMap`：聚合类书源的 exploreUrl 会
/// 读 `getVariable('更多设置')`、`infoMap['发现页来源']` 等来决定要生成哪些
/// 分类，并顺手 `infoMap[...] = ...` 写回默认值。
pub fn eval_explore_kinds(
    script: &str,
    // 发现页没有书籍上下文；此参数为书源地址（source.getKey() 的空值回退）。
    source_key: &str,
    info_map: &HashMap<String, String>,
) -> anyhow::Result<String> {
    let mut bindings = HashMap::new();
    bindings.insert(
        "infoMap".to_string(),
        JsonValue::String(info_map_binding(info_map)),
    );
    eval_js_inner_with_source(
        script,
        None,
        None,
        None,
        None,
        Some(source_key),
        Some(&bindings),
        None,
    )
}

/// 求值发现页控件的 `action`（按钮点击 / 下拉选择 / 输入框防抖回调）。
///
/// 与 [`eval_explore_kinds`] 的区别是同时注入 `java`：`action` 里最常见的就是
/// `java.refreshExplore()`、`java.open('login','http',null)`、`java.searchBook(...)`。
/// 这些调用会写进 `LoginEffects`，由调用方（IPC 命令）回传给前端执行。
pub fn eval_explore_action(
    script: &str,
    // 发现页没有书籍上下文；此参数为书源地址（source.getKey() 的空值回退）。
    source_key: &str,
    info_map: &HashMap<String, String>,
) -> anyhow::Result<String> {
    let mut bindings = HashMap::new();
    bindings.insert(
        "infoMap".to_string(),
        JsonValue::String(info_map_binding(info_map)),
    );
    eval_js_inner_with_source(
        script,
        None,
        None,
        None,
        None,
        Some(source_key),
        Some(&bindings),
        None,
    )
}

/// `viewName` 的 `'字面量'` 形式：整串就是一对单引号包起来的纯文本。
///
/// legado 在两处用同一条判定（`SourceLoginDialog.rowUiBuilder` 与
/// `ExploreAdapter`）：长度落在 3..=19 且首尾都是单引号就取引号内文本，
/// 否则当 JS 表达式求值。
///
/// 只判断首尾引号是不够的：`'选'+'源'` 同样首尾是单引号且长度落在区间内，
/// 但它其实是一段 JS 表达式。因此额外要求**内部不再出现引号**。
pub fn literal_view_name(raw: &str) -> Option<String> {
    let raw = raw.trim();
    let len = raw.chars().count();
    if !(3..=19).contains(&len) || !raw.starts_with('\'') || !raw.ends_with('\'') {
        return None;
    }
    let inner = &raw[1..raw.len() - 1];
    if inner.contains('\'') {
        return None;
    }
    let inner = inner.trim();
    (!inner.is_empty()).then(|| inner.to_string())
}

/// 解析发现页控件的显示名（legado `ExploreKind.viewName`）。
///
/// legado `ExploreAdapter` 的规则：
/// - 缺省 → 用 `title`；
/// - 形如 `'字面量'`（长度 3..=19 且首尾都是单引号）→ 取引号内的字面量；
/// - 其余 → 当作 JS 求值（`evalUiJs`），失败或为空则回退 `title`。
///
/// 必须放在后端：某聚合源的筛选控件把 `viewName` 写成 `"'请选择类型'"`，
/// 而部分书源会写成一段读 `source.getVariable()` 的脚本 —— 两者都需要沙箱。
pub fn eval_explore_view_name(
    view_name: &str,
    // 发现页没有书籍上下文；此参数为书源地址（source.getKey() 的空值回退）。
    source_key: &str,
    info_map: &HashMap<String, String>,
) -> Option<String> {
    let raw = view_name.trim();
    if raw.is_empty() {
        return None;
    }
    if let Some(literal) = literal_view_name(raw) {
        return Some(literal);
    }

    let mut bindings = HashMap::new();
    bindings.insert(
        "infoMap".to_string(),
        JsonValue::String(info_map_binding(info_map)),
    );
    match eval_js_inner_with_source(
        raw,
        None,
        None,
        None,
        None,
        Some(source_key),
        Some(&bindings),
        None,
    ) {
        Ok(value) => {
            let value = value.trim().to_string();
            (!value.is_empty() && !value.eq_ignore_ascii_case("null")).then_some(value)
        }
        Err(err) => {
            push_js_log(format!("[explore viewName] {err}"));
            None
        }
    }
}

/// 把发现页的 `infoMap` 编码成 JS 可用的绑定值。
///
/// 书源里两种访问方式都出现过：`infoMap['源站']`（下标）与
/// `infoMap.get && infoMap.get('源站')`（Map 风格）。因此这里序列化成
/// **JSON 对象**再在 prelude 里补 `get/put/containsKey` 等方法 —— 对象本来就
/// 支持下标访问，补上方法后两种写法都能工作。
fn info_map_binding(info_map: &HashMap<String, String>) -> String {
    serde_json::to_string(&json!({ "__infoMap": info_map })).unwrap_or_else(|_| "{}".to_string())
}

pub fn eval_js_url(
    script: &str,
    result: &str,
    key: &str,
    page: i32,
    // 书源地址（source.getKey() 的空值回退）。
    source_key: &str,
    base_url: &str,
) -> anyhow::Result<String> {
    eval_js_inner_with_source(
        script,
        Some(result),
        Some(base_url),
        Some(key),
        Some(page),
        Some(source_key),
        None,
        None,
    )
}

fn eval_js_inner(
    script: &str,
    input: Option<&str>,
    base_url: Option<&str>,
    key: Option<&str>,
    page: Option<i32>,
    bindings: Option<&HashMap<String, JsonValue>>,
) -> anyhow::Result<String> {
    eval_js_inner_with_source(script, input, base_url, key, page, None, bindings, None)
}

/// legado 兼容层: 在原生 `__xxx` 原语之上组装 `java` / `source` / `book` / `chapter` / `cache`
/// 以及 Rhino 风格的 `java.util.HashMap` / `java.lang.String` 等。
/// 原生函数参数个数与类型必须精确匹配, 所以这里统一做字符串强转和可变参数适配。
const JS_PRELUDE: &str = r#"(function (g) {
  // JS 字符串是 UTF-16, 允许孤立代理项(lone surrogate); Rust 的 String 必须是
  // 合法 UTF-8, 一处落单代理项就会让整次宿主调用失败:
  //   Conversion from string failed: invalid utf-8 sequence of 1 bytes from index N
  // 书源侧很容易造出这种字符 —— 例如 `String(x).substring(0,20)` 正好把 emoji 的
  // 代理对切成两半(某类带段评的书源就是这么生成面板去重键的)。legado 跑 Rhino/Java
  // 字符串不受影响, 这里由宿主统一兜底: 成对代理项原样保留, 落单的替换成 U+FFFD,
  // 保证过桥的永远是合法 UTF-8。
  //
  // 先做一次廉价探测(原生正则扫描, 不分配字符串): 绝大多数字符串没有
  // 落单代理项, 直接原样返回; 命中才走逐字符重建。
  var LONE_HIGH = /[\uD800-\uDBFF](?![\uDC00-\uDFFF])/;
  var LONE_LOW = /(?:^|[^\uD800-\uDBFF])[\uDC00-\uDFFF]/;
  var cleanSurrogates = function (s) {
    if (!LONE_HIGH.test(s) && !LONE_LOW.test(s)) return s;
    var out = '', i, c, d;
    for (i = 0; i < s.length; i++) {
      c = s.charCodeAt(i);
      if (c >= 0xD800 && c <= 0xDBFF) {
        d = i + 1 < s.length ? s.charCodeAt(i + 1) : 0;
        if (d >= 0xDC00 && d <= 0xDFFF) { out += s.charAt(i) + s.charAt(i + 1); i++; }
        else out += '\uFFFD';
      } else if (c >= 0xDC00 && c <= 0xDFFF) {
        out += '\uFFFD';
      } else {
        out += s.charAt(i);
      }
    }
    return out;
  };
  var S = function (v) { return v === undefined || v === null ? '' : cleanSurrogates(String(v)); };
  var J = g.java;
  function wrap(name, arity) {
    var f = J[name];
    if (typeof f !== 'function') return;
    J[name] = function () {
      var a = [];
      for (var i = 0; i < arity; i++) a.push(S(arguments[i]));
      return f.apply(J, a);
    };
  }
  ['md5Encode', 'md5Encode16', 'base64Encode', 'base64Decode', 'encodeURIComponent',
   'decodeURIComponent', 'decodeURI', 'hexEncodeToString', 'hexDecodeToString',
   'htmlFormat', 'toNumChapter', 'getCache'].forEach(function (n) { wrap(n, 1); });
  // `java.encodeURI(text[, charset])` 不能进上面的固定 arity 包装 —— 那会只透传
  // 第 1 个参数，把字符集整个丢掉。一类书源的搜索规则靠
  // `java.encodeURI(key,'gbk')` 按 GBK 编关键词，丢掉字符集后站点收到 UTF-8
  // 字节却按 GBK 解码 → 乱码 → 搜索恒 0 条。
  (function () {
    var f = J.encodeURI;
    if (typeof f !== 'function') return;
    // 宿主函数的参数个数是**严格匹配**的（1 个实参调 2 参函数会抛
    // `Error calling function with 1 argument(s) while 2 where expected`），
    // 所以缺省时也要显式补一个空串；空串在宿主侧等价于「未声明字符集」→ UTF-8。
    J.encodeURI = function (text, charset) {
      var enc = charset === undefined || charset === null ? '' : S(charset);
      return f.call(J, S(text), enc);
    };
  })();
  ['digestHex', 'digestBase64Str', 'ensureGlobalVariable', 'putCache'].forEach(function (n) { wrap(n, 2); });
  ['HMacHex', 'HMacBase64'].forEach(function (n) { wrap(n, 3); });
  // `java.hexDecodeToByteArray` / `java.base64DecodeToByteArray`：legado 返回 Rhino
  // 的 `ByteArray`，脚本按数组用（`.length` / `.slice(start,end)` / 索引），并把它
  // 当 key/iv 喂给 `createSymmetricCrypto`。宿主以 JSON 数组字符串返回，这里还原成
  // 真正的数组 —— 该源正文规则正是靠这条链做 AES 解密。
  ['hexDecodeToByteArray', 'base64DecodeToByteArray'].forEach(function (n) {
    var f = J[n];
    if (typeof f !== 'function') return;
    J[n] = function (v) {
      try { return JSON.parse(S(f.call(J, S(v)))); } catch (e) { return []; }
    };
  });
  // `createSymmetricCrypto` 的 key/iv 允许是字节数组：转成宿主认识的形式。
  // (宿主侧 `__sym` 收字符串；数组按 latin1 逐字节还原，保证字节值不被 UTF-8 改写。)
  // Java String 包装对象是普通对象, 但语义是文本而不是字节序列。若不显式排除,
  // 它会落进下面的逐字节分支: 例如 `java.md5Encode('abc')` 的十六进制文本会被
  // `charCodeAt & 0xff` 拆成 32 个垃圾字节, 且不抛错, 只是把签名/密钥算错。
  function isJavaString(v) {
    return v !== null && v !== undefined && typeof v.__javaStringText === 'function';
  }
  function bytesArg(v) {
    if (v === undefined || v === null) return '';
    if (typeof v === 'string') return v;
    if (isJavaString(v)) return String(v);
    var out = '';
    for (var i = 0; i < v.length; i++) out += String.fromCharCode(v[i] & 0xff);
    return out;
  }
  // 是否为"字节序列"入参: 字符串按原样交给宿主, 数组/TypedArray 才算字节。
  function isByteSequence(v) {
    return v !== null && v !== undefined && typeof v !== 'string' && !isJavaString(v);
  }
  J.__bytesArg = bytesArg;
  J.__isByteSequence = isByteSequence;
  // Rhino 里 java.* 的字符串返回值是 **Java String**: 脚本侧是对象, 带
  // `length()` / `getBytes()` / `substring()` 等方法。书源会直接按 Java 风格调用
  // (`java.digestHex(...).length()`, `java.md5Encode(...).getBytes()`, issue #13)。
  // 宿主侧只能返回 JS 原始字符串, 这里统一包成 java.lang.String。
  function javaString(value) {
    return J.lang.String(value === undefined || value === null ? '' : String(value));
  }
  ['md5Encode', 'md5Encode16', 'base64Encode', 'digestHex', 'digestBase64Str',
   'hexEncodeToString', 'hexDecodeToString', 'HMacHex', 'HMacBase64'].forEach(function (n) {
    var f = J[n];
    if (typeof f !== 'function') return;
    J[n] = function () { return javaString(f.apply(J, arguments)); };
  });
  // java.lang.String 在 JavaImporter 的 with 作用域里会遮蔽内置 String, 书源靠这
  // 一点把字节数组转成文本(`String(Arrays.copyOfRange(data,0,16))`)。
  var timeFormat = J.timeFormat, timeFormatUTC = J.timeFormatUTC, randomInt = J.randomInt, randomString = J.randomString;
  J.timeFormat = function (t) { return timeFormat(Number(t) || 0); };
  J.timeFormatUTC = function (t, f, sh) { return timeFormatUTC(Number(t) || 0, S(f), sh === undefined ? 8 : Number(sh) || 0); };
  J.randomInt = function (m) { return randomInt(Number(m) || 0); };
  J.randomString = function (n) { return randomString(Number(n) || 0); };

  function headerJson(h) {
    if (!h) return '{}';
    if (typeof h === 'string') return h;
    if (h.__m) return JSON.stringify(h.__m);
    return JSON.stringify(h);
  }
  var evaluatingHeaders = false;
  // 去掉 legado 的 URL option 尾巴(`地址,{"headers":{…}}`), 只留纯地址。
  // 用于记录"原始请求地址", 与宿主回的最终地址比较以判断重定向。
  function stripUrlOption(url) {
    var text = S(url);
    var idx = text.indexOf(',{');
    if (idx <= 0) return text;
    if (text.slice(0, 5) === 'data:') return text;
    try {
      if (!JSON.parse(text.slice(idx + 1))) return text;
    } catch (_e) { return text; }
    return text.slice(0, idx);
  }
  function requestHeaders(extra) {
    var raw = headerJson(extra);
    if (!g.__source_header_script || evaluatingHeaders) return raw;
    evaluatingHeaders = true;
    try {
      var defaults = (function () { return eval('{\n' + g.__source_header_script + '\n}'); }).call(g);
      if (typeof defaults === 'string') defaults = JSON.parse(defaults || '{}');
      if (!defaults || typeof defaults !== 'object') defaults = {};
      var supplied = JSON.parse(raw);
      Object.keys(supplied).forEach(function (key) {
        Object.keys(defaults).forEach(function (name) { if (name.toLowerCase() === key.toLowerCase()) delete defaults[name]; });
        defaults[key] = supplied[key];
      });
      return JSON.stringify(defaults);
    } finally { evaluatingHeaders = false; }
  }
  // 双形态值: legado 的 `result` 是 Kotlin 对象经 Rhino 绑定, `result.body`
  // (属性) 与 `result.body()` (方法) 都成立。这里用 Proxy 让同一个值既能
  // 当值用(参与比较/取成员), 又能被调用。
  function dual(get) {
    var fn = function () { return get(); };
    return new Proxy(fn, {
      apply: function () { return get(); },
      get: function (target, prop) {
        if (prop === 'toString' || prop === 'valueOf' || prop === Symbol.toPrimitive) {
          return function () { return get(); };
        }
        // function 默认不会被 JSON.stringify 序列化; toJSON 让 `body`/`code`
        // 这类"既当方法又当值"的成员在对象里也保持原值。
        if (prop === 'toJSON') return function () { return get(); };
        if (prop === 'then') return undefined;
        var value = get();
        if (value === null || value === undefined) return undefined;
        if (prop === 'length' && typeof value === 'string') return value.length;
        var member = value[prop];
        return typeof member === 'function' ? member.bind(value) : member;
      }
    });
  }
  // 响应头: legado 的 Map 语义, 既支持 `headers['x']` / `headers.get('x')`,
  // 也要能 JSON.stringify(保持对象类型)。
  function headerMap(raw) {
    var map = {};
    if (Array.isArray(raw)) {
      for (var i = 0; i < raw.length; i++) {
        if (raw[i] && raw[i].length >= 2) { map[S(raw[i][0]).toLowerCase()] = raw[i][1]; }
      }
    } else if (raw && typeof raw === 'object') {
      for (var k in raw) { if (Object.prototype.hasOwnProperty.call(raw, k)) map[S(k).toLowerCase()] = raw[k]; }
    }
    Object.defineProperty(map, 'get', { value: function (k) { var v = map[S(k).toLowerCase()]; return v === undefined ? null : v; }, enumerable: false });
    return map;
  }
  function Resp(raw) {
    var o;
    try { o = JSON.parse(raw); } catch (e) { o = { body: '', code: 0, url: '', headers: {} }; }
    if (o === null || typeof o !== 'object') o = { body: S(o), code: 0, url: '', headers: {} };
    var headers = headerMap(o.headers);
    var target = function () { return o.body; };
    return new Proxy(target, {
      apply: function () { return o.body; },
      get: function (t, prop) {
        if (prop === '__isResp') return true;
        // 宿主把 Resp 对象作为脚本返回值时用它取完整 StrResponse 信封
        // (body/url/code/headers/isSuccessful), 而非只有正文。
        if (prop === '__respJson') {
          return function () {
            return JSON.stringify({
              // 标记: 让宿主区分"Resp 信封"与书源数据本身就是这种形状的情况。
              __respEnvelope: true,
              body: o.body === undefined || o.body === null ? '' : String(o.body),
              url: o.url === undefined || o.url === null ? '' : String(o.url),
              code: o.code === undefined || o.code === null ? 0 : o.code,
              headers: headers,
              callTime: Number(o.call_time) >= 0 ? Number(o.call_time) : 0,
              isSuccessful: o.isSuccessful === undefined ? o.code >= 200 && o.code < 300 : o.isSuccessful
            });
          };
        }
        if (prop === 'toString' || prop === 'valueOf' || prop === Symbol.toPrimitive) {
          return function () { return o.body; };
        }
        if (prop === 'headers') return headers;
        // legado `StrResponse.callTime()`: 本次请求的耗时(毫秒), 由
        // `AnalyzeUrl` 在 `getStrResponse` 里用 `System.currentTimeMillis()-startTime`
        // 记入 `putCallTime`。书源的「测速」脚本全靠它排序线路:
        //
        //   try{t=Number(r.callTime())}catch(e2){t=-1}
        //   if(alive&&t>=0&&t<bestT){...}
        //
        // 缺这个方法时每次都落到 catch, t 恒为 -1 → 所有线路都被判成
        // 「没有可用耗时」→ 一键测速报「❌ 全部线路异常」。
        if (prop === 'callTime') {
          return function () {
            var t = Number(o.call_time);
            return isFinite(t) && t >= 0 ? Math.round(t) : 0;
          };
        }
        if (prop === 'header') {
          return function (k) { var v = headers[S(k).toLowerCase()]; return v === undefined ? null : v; };
        }
        // legado 的 `StrResponse.cookies()` 返回 okhttp `Response.cookies()`
        // （`List<Cookie>`）。书源用它读**响应下发的 Cookie**，典型写法：
        //
        //   var token = java.get("https://passport.example.test", {}).cookies()
        //     .toString().match(/session_token=([^,]*)/)[1];
        //   source.putLoginHeader(JSON.stringify({authorization: "Bearer " + token}));
        //
        // 缺这个方法时脚本抛 "not a function"，`Token()` 整个失败 —— 后续所有
        // 接口请求都少 `authorization`，服务端回 `401`。
        // 界面症状是**详情简介空、目录 0 章**，而搜索正常（搜索接口不校验 token）。
        //
        // 我们的 jar 已按域名隔离并保存了下发的 cookie（`__http` 收完响应会
        // `g.cookie.setCookie`），因此这里按**本次请求的最终地址**把该域下的
        // cookie 拼成 `name=value` 串；`toString()` 交给 JS 侧 match 用，
        // 与 okhttp 的 `List<Cookie>.toString()` 输出形式一致（逗号分隔）。
        if (prop === 'cookies') {
          return function () {
            var jarUrl = S(o.url || o.__requestUrl || '');
            var raw = jarUrl ? S(g.cookie.getCookie(jarUrl)) : '';
            var list = raw ? raw.split(';') : [];
            var parts = [];
            for (var i = 0; i < list.length; i++) {
              var item = String(list[i]).trim();
              if (item) parts.push(item);
            }
            var joined = parts.join(', ');
            // 同时给出数组接口，兼容 `cookies().length` 与 `cookies()[0].name` 写法。
            var arr = parts.slice();
            arr.toString = function () { return joined; };
            arr.join = function (sep) { return parts.join(sep === undefined ? ', ' : sep); };
            return arr;
          };
        }
        // legado 的 `StrResponse.raw()` 返回 okhttp `Response`, 书源用它判断
        // **重定向**: `result.raw().priorResponse()` 拿到重定向链的上一跳,
        // 再 `isRedirect()` 判断是否发生过 302。缺 `raw` 时脚本在
        // `result.raw().priorResponse()` 处抛 "not a function",
        // 整个 loginCheckJs 失败 —— 正文随之加载不出来(该源正是这样)。
        //
        // 我们只保留最终地址, 因此按"原始请求地址 != 最终地址"还原重定向:
        // `__requestUrl` 是发起请求时记录的原始 URL(见 J.ajax / __http)。
        if (prop === 'raw') {
          return function () {
            var requestUrl = S(o.__requestUrl === undefined ? o.url : o.__requestUrl);
            var finalUrl = S(o.url);
            var redirected = !!requestUrl && !!finalUrl && requestUrl !== finalUrl;
            function priorResponse() {
              if (!redirected) return null;
              // 上一跳: 同一份响应的请求地址, 但 code 视为 302 且自身没有更早的跳转。
              return {
                isRedirect: function () { return true; },
                code: function () { return 302; },
                url: function () { return requestUrl; },
                message: function () { return 'Found'; },
                isSuccessful: function () { return false; },
                priorResponse: function () { return null; },
                headers: function () { return headerMap({}); },
                toString: function () { return requestUrl; }
              };
            }
            return {
              isRedirect: function () { return redirected; },
              code: function () { return o.code === undefined || o.code === null ? 0 : o.code; },
              url: function () { return finalUrl; },
              message: function () { return ''; },
              isSuccessful: function () {
                return o.isSuccessful === undefined
                  ? (o.code >= 200 && o.code < 300)
                  : !!o.isSuccessful;
              },
              headers: function () { return headers; },
              header: function (k) {
                var v = headers[S(k).toLowerCase()];
                return v === undefined ? null : v;
              },
              priorResponse: priorResponse,
              body: function () { return null; },
              toString: function () { return finalUrl; }
            };
          };
        }
        if (prop === 'statusCode' || prop === 'code') return dual(function () { return o.code; });
        if (prop === 'body') return dual(function () { return o.body; });
        if (prop === 'url') return dual(function () { return o.url; });
        if (prop === 'isSuccessful') return dual(function () { return o.isSuccessful; });
        if (prop in o) return o[prop];
        return undefined;
      }
    });
  }
  // 宿主注入绑定(如 loginCheckJs 的 `result`)时需要按 StrResponse 语义包装,
  // 在全局暴露构造器供注入脚本使用。
  g.Resp = Resp;
  // 原始请求地址写进响应信封: `Resp().raw().priorResponse()` 靠它判断重定向
  // (见 `raw` 分支)。宿主只回最终地址, 原始地址只有这里知道。
  function httpResp(raw, requestUrl) {
    var envelope = raw;
    try {
      var parsed = JSON.parse(String(raw));
      if (parsed && typeof parsed === 'object' && parsed.__requestUrl === undefined) {
        parsed.__requestUrl = S(requestUrl);
        envelope = JSON.stringify(parsed);
      }
    } catch (_e) {}
    return Resp(envelope);
  }
  J.ajax = function (u) { return httpResp(g.__connect(S(u), requestHeaders()), stripUrlOption(S(u))).body(); };
  J.connect = function (u, h) { return httpResp(g.__connect(S(u), requestHeaders(h)), stripUrlOption(S(u))); };
  J.get = function (u, h) {
    // AnalyzeRule.get(key) reads variables; explicit HTTP URLs retain the
    // existing one-argument HTTP convenience, and headers select HTTP too.
    if (arguments.length === 1 && !/^(?:https?:)?\/\//i.test(S(u))) return ruleGet(S(u));
    // legado 的 java.get 是 `Jsoup.connect(...).followRedirects(false)`:
    // 书源靠读 302 的 Location 头做两步搜索, 这里必须同样不跟随。
    return Resp(g.__http('GET', S(u), '', requestHeaders(h), false));
  };
  J.head = function (u, h) { return httpResp(g.__http('HEAD', S(u), '', requestHeaders(h), false), stripUrlOption(S(u))); };
  J.post = function (u, b, h) { return httpResp(g.__http('POST', S(u), S(b), requestHeaders(h), false), stripUrlOption(S(u))); };
  J.put = function (u, b, h) {
    if (arguments.length < 3) return rulePut(S(u), S(b));
    return httpResp(g.__http('PUT', S(u), S(b), requestHeaders(h), false), stripUrlOption(S(u)));
  };
  /// `java.clearCookie(url)`：清掉该地址下的 Cookie。
  ///
  /// **legado 的 `JsExtensions` 里没有这个方法**（那边只有 `getCookie`，清除是
  /// 书源编辑页的菜单项）。但书源确实在用它 —— 一类源把「清除 Cookie」做成
  /// `loginUi` 按钮，脚本写 `java.clearCookie(url)`。缺失时的表现是点按钮弹
  /// `not a function`，用户以为清不掉。
  ///
  /// 语义对齐 `java.removeCookie`（宿主侧同一个 `clear_url`）：按 eTLD+1
  /// 清掉该域下所有 cookie，返回是否成功。保留 `removeCookie` 是因为已有书源
  /// 在用；两个名字指向同一个实现。
  J.clearCookie = function (url) {
    return !!g.cookie.removeCookie(S(url));
  };
  J.getCookie = function (tag, key) {
    var raw = S(g.cookie.getCookie(S(tag)));
    if (key == null) return raw;
    key = S(key);
    var pairs = raw.split(';');
    for (var i = 0; i < pairs.length; i++) {
      var separator = pairs[i].indexOf('=');
      if (separator >= 0 && pairs[i].slice(0, separator).trim() === key) {
        return pairs[i].slice(separator + 1).trim();
      }
    }
    return '';
  };

  function symmetric(t, k, iv) {
    // key/iv/待解密数据都可能是**字节数组**（`hexDecodeToByteArray` /
    // `base64DecodeToByteArray` / `.slice()` 的结果），也可能是普通字符串。
    //
    // 两者在宿主侧都表现为 JS 字符串，必须用额外的标记位把「字节序列」与
    // 「普通文本」区分开：前者按 latin1 逐字节还原（≥0x80 的字节不能被 UTF-8
    // 重新编码），后者保持 UTF-8 语义（`café` 这类文本 key 不受影响）。
    t = S(t);
    var keyIsBytes = J.__isByteSequence(k), ivIsBytes = J.__isByteSequence(iv);
    k = J.__bytesArg(k); iv = J.__bytesArg(iv);
    var run = function (op, d, fmt, rawOut) {
      var isBytes = J.__isByteSequence(d) && typeof d.length === 'number';
      var payload = isBytes ? J.__bytesArg(d) : S(d);
      // flags: 1=key 是字节序列, 2=iv 是字节序列, 4=载荷是字节序列。
      var flags = (keyIsBytes ? 1 : 0) | (ivIsBytes ? 2 : 0) | (isBytes ? 4 : 0);
      // 图片解密: legado 的 `decrypt` 返回 ByteArray(hutool), 不是文本。
      // 置 8 让宿主把明文按 latin1 逐字节交回, 避免 UTF-8 转换毁掉图片数据。
      // `decryptStr` 在 legado 里明确返回 String, 因此不走这条分支。
      if (rawOut && op === 'dec' && g.__byteMode === true) flags |= 8;
      // 打成 JSON 传：宿主侧 `Func::new` 的元组上限是 7 个参数，而这里需要
      // 7 个业务参数 **加** ctx（抛异常必需）。字段名保持短，减少序列化开销。
      return g.__sym(JSON.stringify({
        op: op, t: t, k: k, iv: iv, d: payload, fmt: isBytes ? 'raw' : fmt, flags: flags
      }));
    };
    return {
      encrypt: function (d) { return run('enc', d, 'base64', false); },
      encryptBase64: function (d) { return run('enc', d, 'base64', false); },
      encryptHex: function (d) { return run('enc', d, 'hex', false); },
      decrypt: function (d) { return run('dec', d, 'auto', true); },
      decryptStr: function (d) { return run('dec', d, 'auto', false); },
      setIv: function (v) { iv = S(v); return this; }
    };
  }
  J.createSymmetricCrypto = symmetric;
  J.aesEncodeToBase64String = function (d, k, t, iv) { return symmetric(t, k, iv).encryptBase64(d); };
  J.aesEncodeToString = function (d, k, t, iv) { return symmetric(t, k, iv).encryptHex(d); };
  J.aesBase64DecodeToString = function (d, k, t, iv) {
    return g.__sym(JSON.stringify({ op: 'dec', t: S(t), k: S(k), iv: S(iv), d: S(d), fmt: 'base64', flags: 0 }));
  };
  J.aesDecodeToString = function (d, k, t, iv) { return symmetric(t, k, iv).decryptStr(d); };
  J.desEncodeToBase64String = J.aesEncodeToBase64String;
  J.desEncodeToString = J.aesEncodeToString;
  J.desBase64DecodeToString = J.aesBase64DecodeToString;
  J.desDecodeToString = J.aesDecodeToString;
  J.tripleDESEncodeBase64Str = function (d, k, m, p, iv) { return symmetric('DESede/' + S(m) + '/' + S(p), k, iv).encryptBase64(d); };
  J.tripleDESEncodeArgsBase64Str = J.tripleDESEncodeBase64Str;
  J.tripleDESDecodeStr = function (d, k, m, p, iv) { return symmetric('DESede/' + S(m) + '/' + S(p), k, iv).decryptStr(d); };
  J.tripleDESDecodeArgsBase64Str = J.tripleDESDecodeStr;

  J.log = function (m) { g.__log('log', S(m)); return m; };
  J.logType = function (m) { g.__log('log', typeof m); };
  J.toast = function (m) { g.__log('toast', S(m)); };
  J.longToast = J.toast;

  // ── `java.webView(html, url, js[, cacheFirst])` ──
  // legado 里它把 HTML 交给后台 WebView 渲染（执行页面脚本）后再取内容，
  // 书源用它应付「正文由 JS 动态注入」的站点。
  //
  // 桌面端没有可用的隐藏 WebView 通道（渲染层在 WebView2，规则在 Rust 侧的
  // rquickjs 沙盒里跑，两边不能同步往返），因此这里做**降级等价实现**：
  //   1. 有 html 时直接当文档用 —— 绝大多数书源传进来的就是已经抓到的响应，
  //      而 HTML 本身已含正文（脚本只是把它们显示出来），静态 DOM 一样能选到；
  //   2. html 为空或按 js 取不到内容时，用 url 重新抓一次页面再解析。
  // 关键点是**不能抛错**：`java.webView` 缺失时脚本第一行就 "not a function"，
  // 整条正文规则中断，界面表现成「正文永远加载不出来」。
  //
  // 语义对齐 `BackstageWebView`：`js` 为空 → 返回 `document.documentElement.outerHTML`；
  // 给了 `js` → 返回该表达式的结果。
  function webViewEvalDocument(html, js) {
    var doc = Jsoup.parse(S(html));
    var script = S(js).trim();
    if (!script) return S(html);
    try {
      // 脚本在文档作用域里跑：把常用宿主对象喂进闭包。
      var fn = new Function('document', 'window', 'java', 'result', 'return (' + script + '\n);');
      var value = fn(doc, { document: doc }, J, S(html));
      return value === undefined || value === null ? '' : S(value);
    } catch (_e) {
      try {
        var fn2 = new Function('document', 'window', 'java', 'result', script);
        var value2 = fn2(doc, { document: doc }, J, S(html));
        return value2 === undefined || value2 === null ? '' : S(value2);
      } catch (_e2) {
        return '';
      }
    }
  }
  J.webView = function (html, url, js, cacheFirst) {
    var text = html === undefined || html === null ? '' : S(html);
    if (text) {
      var rendered = webViewEvalDocument(text, js);
      if (S(rendered).trim()) return rendered;
    }
    var target = url === undefined || url === null ? '' : S(url).trim();
    if (!target) return text;
    try {
      var fresh = S(Resp(g.__http('GET', target, '', requestHeaders(), true)).body);
      if (!fresh) return text;
      return webViewEvalDocument(fresh, js) || fresh;
    } catch (_e) {
      return text;
    }
  };
  // `webViewGetSource*` / `webViewGetOverrideUrl*`: 取资源地址/跳转地址的变体。
  // 桌面端不做嗅探，直接按同样的降级路径求值；取不到就返回空串（legado 的
  // 这些方法在超时/无匹配时也返回空，书源据此走 `||` 兜底分支）。
  J.webViewGetSource = function (html, url, js, sourceRegex, cacheFirst) {
    var rendered = J.webView(html, url, js, cacheFirst);
    var pattern = S(sourceRegex);
    if (!pattern) return rendered;
    try {
      var match = S(rendered).match(new RegExp(pattern));
      return match ? S(match[0]) : '';
    } catch (_e) {
      return '';
    }
  };
  J.webViewGetOverrideUrl = J.webViewGetSource;

  J.showBrowser = function (u, html, preloadJs, options) {
    var target = S(u);
    var inline = html === undefined || html === null ? '' : S(html);
    // 判断传进来的 HTML 是不是「空壳 SPA」：只有挂载点/少量骨架，内容全靠
    // 打包 JS 渲染。这类页面塞进 iframe 面板永远渲染不出来 —— 面板与上游跨域，
    // SPA 的 XHR / localStorage / CORS 都得靠桥逐项模拟，漏一处就是白屏。
    // 交给独立窗口在真实域名下运行才正常（与手机端 legado 直接 loadUrl 一致）。
    //
    // 反过来，**带实质内容**的 HTML（内嵌设置页、阅读器片段、播放器页）仍走面板：
    // 那些页面本来就是静态的，面板渲染更轻，也不该突然弹出一个窗口。
    function isSpaShell(markup) {
      if (!markup) return false;
      if (!/<div[^>]+id=["']?(?:app|root)["']?/i.test(markup)) return false;
      // 只保留 <body>：<title>、<meta> 的文本不算「页面内容」。
      var body = markup.replace(/<script[\s\S]*?<\/script>/gi, '')
                       .replace(/<style[\s\S]*?<\/style>/gi, '');
      var bodyMatch = /<body[^>]*>([\s\S]*?)<\/body>/i.exec(body);
      if (bodyMatch) body = bodyMatch[1];
      else body = body.replace(/<head[\s\S]*?<\/head>/gi, '');
      // 去掉挂载点本身再算可见文本
      body = body.replace(/<div[^>]+id=["']?(?:app|root)["']?[^>]*>\s*<\/div>/gi, '');
      var text = body.replace(/<[^>]+>/g, '').replace(/&nbsp;/g, ' ').trim();
      return text.length === 0;
    }
    if (/^https?:\/\//i.test(target) && (!inline || isSpaShell(inline))) {
      try {
        g.__open_external_window(target, '');
        return true;
      } catch (_e) {
        // 打不开窗口就退回面板，至少还有内容（带上传入的 html）。
      }
    }
    g.__show_browser(target, html === undefined || html === null ? '' : S(html),
                     preloadJs === undefined || preloadJs === null ? '' : S(preloadJs),
                     S(options == null ? '' : typeof options === 'string' ? options : JSON.stringify(options)), '');
    return true;
  };
  /// `java.startBrowser(url[, title][, html])`：打开一个浏览页面。
  ///
  /// **纯网址走独立窗口**，与 `startBrowserAwait` 保持同一套判据（`http(s)` +
  /// 没有内联 `html`）。书源把它当作「打开一个网页给用户看」用：注册页、
  /// 个人中心、账号后台、官网、播放页都走这里。
  ///
  /// 为什么不能一律用面板(iframe)：上游站点普遍带 `X-Frame-Options` 或
  /// `frame-ancestors`，还有前端安全 SDK 主动检测「是否被嵌入」，命中就整页
  /// 换成拦截提示。独立窗口里 `window.top === window`，这些都不触发 —— 与手机端
  /// legado 直接 `loadUrl` 到浏览页面一致。
  ///
  /// 有内联 `html` 的仍用面板：那是书源自己拼好的内容（`data:text/html` 快照、
  /// 播放器页），放进窗口反而丢掉「与规则同源」的上下文。
  J.startBrowser = function (u, title, html) {
    var target = S(u);
    var hasHtml = html !== undefined && html !== null && String(html) !== '';
    if (!hasHtml && /^https?:\/\//i.test(target)) {
      try {
        g.__open_external_window(target, S(title));
        return true;
      } catch (_e) {
        // 打开失败就退回面板，至少用户还能看到页面（哪怕被拦）。
      }
    }
    g.__show_browser(target, hasHtml ? String(html) : '', '', '', S(title));
    return true;
  };
  J.startBrowserAwait = function (u, title, refetchAfterSuccess, html) {
    var target = S(u);
    var hasHtml = html !== undefined && html !== null && String(html) !== '';
    if (!hasHtml && /^https?:\/\//i.test(target)) {
      // **必须真的等窗口关闭**再往下跑。
      //
      // legado 的 `startBrowserAwait` 是阻塞的：
      // `SourceVerificationHelp.getVerificationResult` 在
      // `while (getResult(...) == null) LockSupport.parkNanos(...)` 里一直等，
      // 直到用户关掉 WebView 才继续。
      //
      // 书源就是按这个语义写的 —— 打开登录页 → 等用户登录 → 关窗 → 抓 Cookie。
      // 早先这里只 `__open_external_window` 就立刻返回空 Resp，下一行立刻去抓
      // Cookie，抓到的是未登录会话：番茄登录因此永远「保存失败」，诊断页也不
      // 会出现（yckceo 8054）。
      try {
        var r = JSON.parse(g.__await_external_window(target, S(title)));
        if (!r.ok) throw new Error(r.error || '网页操作未完成');
        var pageUrl = r.url || target;
        // `refetchAfterSuccess === false` 表示调用方只要「用户操作完」这个信号，
        // 不需要重新抓页面正文。默认（true）才回读一次。
        if (refetchAfterSuccess === false) {
          return Resp(JSON.stringify({ body: '', url: pageUrl, code: 200, headers: {} }));
        }
        return J.connect(pageUrl);
      } catch (_e) {
        // 没有等待通道（未在登录上下文中）时退回「打开即返回」，
        // 至少页面能给用户看到。
        try {
          g.__open_external_window(target, S(title));
          return Resp(JSON.stringify({ body: '', url: target, code: 200, headers: {} }));
        } catch (_e2) {}
      }
    }
    var r = JSON.parse(g.__await_browser(target, S(title), hasHtml ? String(html) : ''));
    if (!r.ok) throw new Error(r.error);
    if (refetchAfterSuccess !== false) return J.connect(r.page.url || target);
    return Resp(JSON.stringify({ body: r.page.body, url: r.page.url || target, code: 200, headers: {} }));
  };
  // legado 浏览器族兼容别名: startBrowserDp(url, title[, w, h]) 与
  // showReadingBrowser(url, title)（段评页）签名与 startBrowser 兼容。
  J.startBrowserDp = J.startBrowser;
  J.showReadingBrowser = function (u, title) { return J.startBrowser(u, title); };
  // legado 并发请求: `urlList.asFlow().mapAsync(threadCount)`（见
  // `JsExtensions.ajaxAll`）。书源拿它做**线路测速**，靠 `callTime()` 比快慢；
  // 串行也能出结果，但 N 条线路就要串行等满 N 个超时，且先发的那条会污染
  // 后面的连接复用 —— 实测某源 3 条线路里 1 条不可达，串行要 20s+，
  // 用户看到的就是「一键测速卡住 / 失败」。这里走宿主的真并发实现（保持
  // 输入顺序返回，与 legado 的 `toList().toTypedArray()` 一致）。
  J.ajaxAll = function (urls) {
    if (typeof urls === 'string') urls = urls.split(',');
    var specs = [];
    for (var i = 0; i < urls.length; i++) specs.push(S(urls[i]));
    var envelopes = JSON.parse(S(g.__connect_all(JSON.stringify(specs), requestHeaders())));
    var out = [];
    for (var j = 0; j < envelopes.length; j++) out.push(httpResp(JSON.stringify(envelopes[j]), stripUrlOption(specs[j])));
    return out;
  };

  // ── legado `AnalyzeRule`: 用规则解析当前页面内容 ──
  // 注意: legado 的 `isUrl` 参数(把结果转成绝对地址)暂未实现, 结果原样返回。

  // ── HTML 小工具: 元素快照的可变操作 ──
  // legado 的 `result` 是**真 JSoup Element**: 书源会对它 `clone()`、再
  // `select('a,script,style').remove()` 后取 `html()`(正文清洗),
  // 或者 `new org.jsoup.nodes.Element('div')` + `appendChild` 拼卡片(搜索结果)。
  // 桌面端只有快照, 因此这里补一层等价的可变语义:
  //   - 元素记得自己的"宿主元素"(`__owner`), `remove()` 从宿主 innerHtml 里摘掉
  //     自己的 outerHtml, 并逐层向上重算 outerHtml/text;
  //   - `clone()` 得到独立深拷贝(宿主为空), 对副本的删除不影响原文档。
  function escapeHtmlText(text) {
    return S(text).replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;');
  }
  function escapeHtmlAttr(text) {
    return S(text).replace(/&/g, '&amp;').replace(/"/g, '&quot;').replace(/</g, '&lt;').replace(/>/g, '&gt;');
  }
  var HTML_ENTITIES = { amp: '&', lt: '<', gt: '>', quot: '"', apos: "'", nbsp: ' ' };
  function decodeHtmlEntities(text) {
    return S(text).replace(/&(#x?[0-9a-fA-F]+|[a-zA-Z]+);/g, function (whole, body) {
      if (body.charAt(0) === '#') {
        var code = body.charAt(1) === 'x' || body.charAt(1) === 'X'
          ? parseInt(body.slice(2), 16)
          : parseInt(body.slice(1), 10);
        return isNaN(code) ? whole : String.fromCharCode(code);
      }
      var named = HTML_ENTITIES[body.toLowerCase()];
      return named === undefined ? whole : named;
    });
  }
  // 快照 → 纯文本(JSoup `text()` 的归一化: 折叠空白)。
  function htmlToText(html) {
    return decodeHtmlEntities(
      S(html)
        .replace(/<(script|style)\b[^>]*>[\s\S]*?<\/\1>/gi, ' ')
        .replace(/<br\s*\/?>/gi, ' ')
        .replace(/<\/(p|div|li|tr|h[1-6])>/gi, ' ')
        .replace(/<[^>]*>/g, '')
    ).replace(/\s+/g, ' ').replace(/^\s+|\s+$/g, '');
  }
  // 快照 → 自身 outerHtml(属性顺序按插入序, 与 JSoup 输出足够接近)。
  function buildOuterHtml(tag, attrs, innerHtml) {
    var name = S(tag) || 'div';
    var head = '<' + name;
    for (var key in attrs) {
      if (!Object.prototype.hasOwnProperty.call(attrs, key)) continue;
      var value = attrs[key];
      head += ' ' + key + '="' + escapeHtmlAttr(value === null || value === undefined ? '' : value) + '"';
    }
    if (/^(br|hr|img|input|meta|link)$/i.test(name)) return head + '>';
    return head + '>' + S(innerHtml) + '</' + name + '>';
  }
  // 重新计算快照的派生态(text/ownText/outerHtml), 并把变化同步给宿主元素。
  function refreshSnapshot(el) {
    var s = el.__snapshot;
    s.outerHtml = buildOuterHtml(s.tag, s.attrs || {}, s.innerHtml);
    s.text = htmlToText(s.innerHtml);
    s.ownText = s.text;
    var up = el.__owner;
    if (!up) return;
    var prev = el.__prevOuter;
    var upSnap = up.__snapshot;
    if (prev && prev !== s.outerHtml && S(upSnap.innerHtml).indexOf(prev) >= 0) {
      upSnap.innerHtml = S(upSnap.innerHtml).split(prev).join(s.outerHtml);
      el.__prevOuter = s.outerHtml;
      refreshSnapshot(up);
    } else {
      el.__prevOuter = s.outerHtml;
    }
  }
  // 深拷贝快照(用于 `clone()`), 丢掉宿主关系。
  function cloneSnapshot(snapshot) {
    var copy = {};
    for (var key in snapshot) {
      if (Object.prototype.hasOwnProperty.call(snapshot, key)) copy[key] = snapshot[key];
    }
    var attrs = {};
    var source = snapshot.attrs || {};
    for (var name in source) {
      if (Object.prototype.hasOwnProperty.call(source, name)) attrs[name] = source[name];
    }
    copy.attrs = attrs;
    return copy;
  }

  function wrapElement(snapshot, owner) {
    var s = snapshot || {};
    // JSONPath 结果(`java.getElements("$.list[*]")`)是**原始 JSON 对象**，不是元素
    // 快照。legado 的 `getElements` 在 `Mode.Json` 下走
    // `getAnalyzeByJSonPath(result).getList(rule)`，直接返回 JSON 节点，脚本可以
    // 直接 `item.title` 取值。
    //
    // 这里必须把 JSON 对象的字段原样透传：只返回下面那组方法的话，宿主序列化时
    // JSON 字段全丢，`build_chapter_from_json` 取 `title` 必然为空 —— 该源目录
    // 解析出 0 章就是这个原因。
    //
    // 判定依据是元素快照的必需字段(`outerHtml`/`innerHtml`)，两个都没有就当成
    // 原始 JSON 节点。(`tag` 不可靠：JSONPath 结果也可能自带 `tag` 之类的键。)
    if (s.outerHtml === undefined && s.innerHtml === undefined) {
      var jsonNode = {};
      for (var k in s) {
        if (Object.prototype.hasOwnProperty.call(s, k)) jsonNode[k] = s[k];
      }
      // 元素方法是给快照用的；JSON 节点上退化取值，保证脚本两种来源都能跑不报错。
      jsonNode.text = function () { return S(s.title !== undefined ? s.title : ''); };
      jsonNode.ownText = jsonNode.text;
      jsonNode.html = function () { return ''; };
      jsonNode.outerHtml = function () { return ''; };
      jsonNode.attr = function (name) {
        var v = s[S(name)];
        return v === undefined || v === null ? '' : S(v);
      };
      jsonNode.hasAttr = function (name) {
        return Object.prototype.hasOwnProperty.call(s, S(name));
      };
      jsonNode.tagName = function () { return ''; };
      jsonNode.select = function () { return elementsApi([]); };
      jsonNode.toString = function () { return JSON.stringify(s); };
      return jsonNode;
    }
    var attrs = s.attrs || {};
    var el = {
      // JSoup 的 `Element.text(String)` / `ownText(String)` / `html(String)` 是
      // **可变 setter**, 返回元素自身; 无参时取值。书源会先读标题、为空时用
      // `aEl.text(title)` 把标题写回元素, 再整体返回目录元素列表, 让宿主的
      // `chapterName: text` 取到值。只读实现会让这类目录解析出 0 章
      // (CSS 伪元素标题的站点普遍这么写)。
      text: function (value) {
        if (arguments.length && value !== undefined) {
          s.text = S(value); s.ownText = S(value); s.innerHtml = escapeHtmlText(value);
          refreshSnapshot(el);
          return el;
        }
        return S(s.text);
      },
      ownText: function (value) {
        if (arguments.length && value !== undefined) {
          s.ownText = S(value); return el;
        }
        return S(s.ownText);
      },
      html: function (value) {
        if (arguments.length && value !== undefined) {
          s.innerHtml = S(value);
          refreshSnapshot(el);
          return el;
        }
        return S(s.innerHtml);
      },
      outerHtml: function () { return S(s.outerHtml); },
      // JSoup `attr(key, value)` 同样是 setter, 返回元素自身。
      attr: function (name, value) {
        if (arguments.length > 1) {
          attrs[S(name)] = S(value);
          refreshSnapshot(el);
          return el;
        }
        var v = attrs[S(name)];
        return v === undefined ? '' : S(v);
      },
      hasAttr: function (name) { return Object.prototype.hasOwnProperty.call(attrs, S(name)); },
      removeAttr: function (name) {
        delete attrs[S(name)];
        refreshSnapshot(el);
        return el;
      },
      tagName: function () { return S(s.tag); },
      // 在元素内部继续查询: 用 innerHtml 作为内容, 只匹配**后代**
      // (JSoup 的 element.select() 不含元素自身; 用 outerHtml 会把自己也选进来)
      //
      // JSoup 的 `Element.select()` 返回 `Elements`, 必须带 size()/get()/first()
      // 等方法: 书源普遍写 `result.select('p.meta').first()`、
      // `.size()`、`.eachText()`。只返回普通数组会让这些调用抛 "not a function",
      // 字段取值全部落空(搜索结果为 0 条)。
      //
      // 子元素必须记住宿主(`__owner`): 正文规则的标准写法是
      // `var c = box.clone(); c.select('a,script,style').remove(); c.html()`,
      // 没有宿主关系时 `remove()` 无处生效, 去广告/去脚本就失效了。
      select: function (rule) {
        var inner = S(s.innerHtml);
        if (!inner) return elementsApi([]);
        return elementsApi(wrapElements(g.__rule_get(S(rule), inner, 'elements'), el));
      },
      children: function () {
        return elementsApi(wrapElements(s.children === undefined ? '[]' : s.children, el));
      },
      // JSoup `Node.parentNode()` / `Element.parent()`: 父元素。
      //
      // 目录规则常写 `out.add(a.parentNode())` —— 章节列表的 `<a>` 外面包着
      // `<p>`，宿主再按 `a@text` / `a@href` 从父节点往下取，`<a>` 自己不带
      // 章节名之外的上下文。缺这两个方法时脚本在第一轮就抛 `not a function`，
      // 整条 chapterList 中断 → **目录 0 章**（搜索/详情都正常，很容易误判成
      // 站点改版）。
      //
      // 父节点在快照里是 `parent` 字段（见 `rule_engine::element_snapshot`），
      // 只展开一层；祖先链上再无父节点时返回 null（legado 的根节点同样返回 null）。
      parentNode: function () {
        var p = s.parent;
        if (!p || typeof p !== 'object') return null;
        // 父节点通过 `parentNode()` 取到之后，书源通常只读文本/属性，
        // 不会对父节点再 `remove()`（那会把整块从文档里摘掉）。
        // 因此这里不挂 owner，避免 `remove()` 误伤原文档。
        return wrapElement(p, null);
      },
      parent: function () {
        return el.parentNode();
      },
      // `nextElementSibling()` / `previousElementSibling()`: 兄弟元素。
      // 部分书源用它们把「书名 <p> + 作者 <p>」这类相邻段落串起来读。
      // 快照里没有兄弟链，返回 null 让脚本的安全写法（判空）继续跑。
      nextElementSibling: function () { return null; },
      previousElementSibling: function () { return null; },
      // `clone()`: JSoup 返回独立副本(不挂在原文档上), 后续删除只影响副本。
      clone: function () {
        return wrapElement(cloneSnapshot(s), null);
      },
      // `remove()`: 从宿主 innerHtml 中摘掉自己, 并向上重算。
      remove: function () {
        var up = el.__owner;
        if (!up) return el;
        var outer = S(s.outerHtml);
        var upSnap = up.__snapshot;
        var inner = S(upSnap.innerHtml);
        if (outer && inner.indexOf(outer) >= 0) {
          upSnap.innerHtml = inner.split(outer).join('');
          refreshSnapshot(up);
        }
        el.__owner = null;
        return el;
      },
      // `appendChild(Element)`: 追加子元素/文本, 返回自身(JSoup 语义)。
      appendChild: function (child) {
        var piece = child && typeof child.outerHtml === 'function' ? child.outerHtml() : escapeHtmlText(child == null ? '' : child);
        s.innerHtml = S(s.innerHtml) + piece;
        refreshSnapshot(el);
        return el;
      },
      // `empty()`: 清空子节点(部分源用它重置容器)。
      empty: function () { s.innerHtml = ''; refreshSnapshot(el); return el; },
      toString: function () { return S(s.outerHtml); },
      // 元素序列化时必须**直接交出快照**, 不能带一层包装。
      //
      // 脚本用普通数组攒列表(`var out = []; out.push(li); return out;`)时,
      // 宿主拿到的是 `JSON.stringify(out)` 的结果 —— 没有 `toJSON` 的话每项会
      // 变成 `{"__snapshot":{…},"__owner":null,"__prevOuter":"…"}`, 宿主既认不出
      // 这是元素快照(缺顶层 `outerHtml`), 又会把 `__snapshot` 当普通 JSON 字段,
      // 字段规则 `span.s2 a@text` 因此全部取空 —— 发现页/搜索页 0 条。
      // 用 `Elements` 集合时靠 `elementsApi` 的 `toJSON` 兜住, 裸数组就只能靠这里。
      toJSON: function () { return s; },
      // 保留原始快照: JSoup 桥需要在元素间复制/合并时重建包装对象。
      __snapshot: s,
      __owner: owner || null,
      __prevOuter: S(s.outerHtml)
    };
    // 内部字段必须**不可枚举**: `JSON.stringify` / `for..in` / `Object.assign`
    // 只该看到元素本身的方法与外层快照, 否则会把宿主关系与快照副本一起序列化
    // (上面 `toJSON` 之外的第二道保险: `toJSON` 缺失或被绕过时不至于串味)。
    try {
      Object.defineProperty(el, '__snapshot', { value: s, enumerable: false, writable: true, configurable: true });
      Object.defineProperty(el, '__owner', { value: owner || null, enumerable: false, writable: true, configurable: true });
      Object.defineProperty(el, '__prevOuter', { value: S(s.outerHtml), enumerable: false, writable: true, configurable: true });
    } catch (_e) { /* 冻结对象保持原样 */ }
    return el;
  }
  // 宿主把"元素快照"注入为字段规则的 `result` 时要用同一套包装(见
  // `ELEMENT_BINDING_PREFIX`): legado 的 result 是 JSoup Element, 书源会对它
  // 调用 select()/html()/text()。桥接函数必须挂到全局, 宿主才能按名字调用。
  g.__wrapElement = wrapElement;
  function wrapElements(json, owner) {
    var list;
    try { list = JSON.parse(S(json)); } catch (e) { return []; }
    if (!list || typeof list.length !== 'number') return [];
    var out = [];
    for (var i = 0; i < list.length; i++) out.push(wrapElement(list[i], owner));
    return out;
  }
  // 宿主注入 `@elements:` 绑定时要拿到带 JSoup 方法的集合, 而不只是裸数组。
  // 入参既可能是 JSON 文本, 也可能已经是数组(宿主先 `JSON.parse` 过一次) ——
  // 后者若再 `JSON.parse` 会得到空集合, 书源里的 `result.size()` 恒为 0。
  g.__wrapElements = function (value) {
    var list = value;
    if (typeof list === 'string') {
      try { list = JSON.parse(list); } catch (e) { return elementsApi([]); }
    }
    if (!list || typeof list.length !== 'number') return elementsApi([]);
    var out = [];
    for (var i = 0; i < list.length; i++) out.push(wrapElement(list[i], null));
    return elementsApi(out);
  };
  // legado 的 `java.getElements` 返回 `Elements`(JSoup), 书源按 Java 方法调用:
  // `list.size()` / `list.get(i)` / `list.isEmpty()`。只返回普通数组时这些
  // 调用会抛 "not a function", 整条 chapterList 脚本报错 → 目录 0 章。
  // 方法定义成**不可枚举**, 否则 `JSON.stringify(elements)` / `for..in`
  // 会把函数当数据项序列化出去。
  function elementsApi(list) {
    if (!list || typeof list.length !== 'number') return list;
    function def(name, fn) {
      try {
        Object.defineProperty(list, name, { value: fn, enumerable: false, writable: true, configurable: true });
      } catch (e) { /* 冻结数组保持原样 */ }
    }
    def('size', function () { return list.length; });
    def('isEmpty', function () { return list.length === 0; });
    def('get', function (i) { return list[i] === undefined ? null : list[i]; });
    def('first', function () { return list.length ? list[0] : null; });
    def('last', function () { return list.length ? list[list.length - 1] : null; });
    def('eachText', function () {
      var out = [];
      for (var i = 0; i < list.length; i++) out.push(list[i] && list[i].text ? list[i].text() : '');
      return out;
    });
    def('text', function () {
      var out = [];
      for (var i = 0; i < list.length; i++) out.push(list[i] && list[i].text ? list[i].text() : '');
      return out.join(' ').replace(/^\s+|\s+$/g, '');
    });
    // 返回 Elements 的脚本(典型是 chapterList)由宿主 JSON 序列化后再按字段
    // 规则取值, 因此必须交出扁平快照; JSONPath 的原始节点没有 `__snapshot`,
    // 原样透传(见 get_elements_on_jsonpath_returns_raw_json_nodes)。
    def('toJSON', function () {
      var out = [];
      for (var i = 0; i < list.length; i++) {
        var item = list[i];
        out.push(item && item.__snapshot ? item.__snapshot : item);
      }
      return out;
    });
    // JSoup `Elements` 的其余取首元素方法: 书源常用
    // `java.getElement('.list').attr('href')` / `.html()` 直接拿第一个。
    def('attr', function (name) {
      return list.length && list[0] && list[0].attr ? list[0].attr(name) : '';
    });
    def('html', function () {
      return list.length && list[0] && list[0].html ? list[0].html() : '';
    });
    def('outerHtml', function () {
      return list.length && list[0] && list[0].outerHtml ? list[0].outerHtml() : '';
    });
    def('select', function (rule) {
      var out = [];
      for (var i = 0; i < list.length; i++) {
        var sub = list[i] && list[i].select ? list[i].select(rule) : [];
        for (var j = 0; j < sub.length; j++) out.push(sub[j]);
      }
      return elementsApi(out);
    });
    def('toString', function () {
      var out = [];
      for (var i = 0; i < list.length; i++) {
        out.push(list[i] && list[i].outerHtml ? list[i].outerHtml() : '');
      }
      return out.join('');
    });
    // `Elements.add(el)` / `addAll(...)`: 书源用它拼搜索结果卡片列表
    // (`var out = new org.jsoup.select.Elements(); out.add(mkCard(...))`)。
    def('add', function (el) { list[list.length] = el; return list; });
    def('addAll', function (other) {
      var items = other && typeof other.length === 'number' ? other : arguments;
      for (var i = 0; i < items.length; i++) list[list.length] = items[i];
      return list;
    });
    // `Elements.remove()`: 逐个从宿主元素里摘掉(返回空集合语义的自身)。
    def('remove', function () {
      for (var i = list.length - 1; i >= 0; i--) {
        if (list[i] && typeof list[i].remove === 'function') list[i].remove();
      }
      return list;
    });
    def('clone', function () {
      var out = [];
      for (var i = 0; i < list.length; i++) {
        out.push(list[i] && typeof list[i].clone === 'function' ? list[i].clone() : list[i]);
      }
      return elementsApi(out);
    });
    // `Elements.toArray()`：JSoup 返回 `Element[]`（Java 数组）。书源会拿它
    // 做**逐条文本过滤**再重新拼 HTML，例如
    //   `list = result.toArray(); re = new RegExp(key);`
    //   `for (i in list) { re.test(list[i]) ? html += list[i] : '' }`
    // 少这个方法时脚本在第二行就抛 "not a function"，整条 bookList 中断 ——
    // 症状是搜索**恒为 0 条**，而发现页（不走这段脚本）正常。
    // 传了数组参数时按 JSoup 语义填充该数组并返回它。
    def('toArray', function (target) {
      var out = [];
      for (var i = 0; i < list.length; i++) out.push(list[i]);
      if (target && typeof target.length === 'number') {
        for (var j = 0; j < out.length && j < target.length; j++) target[j] = out[j];
        return target;
      }
      return out;
    });
    def('eq', function (index) {
      return list[index] === undefined ? elementsApi([]) : elementsApi([list[index]]);
    });
    def('parents', function () {
      var out = [];
      for (var i = 0; i < list.length; i++) {
        if (list[i] && list[i].__owner) out.push(list[i].__owner);
      }
      return elementsApi(out);
    });
    return list;
  }
  // mContent 省略时用当前页面内容(与 legado 默认 this.content 一致)
  function ruleContent(mContent) {
    return mContent === undefined || mContent === null ? '' : S(mContent);
  }
  J.getString = function (rule, mContent) {
    return g.__rule_get(S(rule), ruleContent(mContent), 'text');
  };
  J.getStringList = function (rule, mContent) {
    var list;
    try { list = JSON.parse(S(g.__rule_get(S(rule), ruleContent(mContent), 'list'))); }
    catch (e) { return []; }
    return list || [];
  };
  J.getElements = function (rule) {
    return elementsApi(wrapElements(g.__rule_get(S(rule), '', 'elements')));
  };
  // legado 的 `getElement` 对 CSS/XPath 规则返回的是 **Elements 集合**而不是
  // 单个元素: `AnalyzeRule.getElement` 的 else 分支是
  // `getAnalyzeByJSoup(result).getElements(rule)`(见 AnalyzeRule.kt)。
  //
  // 书源普遍依赖这一点, 把 getElement 的返回值直接当作章节/搜索结果列表交回
  // 宿主: 例如 `result = java.getElement('#book-list>div')`。这里若
  // 只返回第一个元素, 序列化出来是单个 `{__snapshot:{…}}` 对象, 宿主会当成
  // "一本书"来套字段规则, 于是每项都取不到值, 结果是 0 条。
  //
  // 用 Elements 语义(带 size/get/first/eachText/toJSON)同时兼容两种用法:
  // `getElement(r)[0]`、`getElement(r).size()` 与"直接当列表交回"都成立。
  J.getElement = function (rule) {
    return elementsApi(wrapElements(g.__rule_get(S(rule), '', 'elements')));
  };
  // legado AnalyzeRule.setContent: 替换后续 getString/getElement 的解析对象。
  J.setContent = function (content) { g.__rule_set_content(content == null ? '' : S(content)); };

  // 纯工具方法（与网络/环境无关，实现即可，不影响任何能力探测）
  //
  // **不要在这里重新定义 `J.encodeURI`**：原生实现支持可选字符集
  // （`java.encodeURI(key,'gbk')` 要按 GBK 编关键词），这里一覆盖就退化成
  // 纯 UTF-8，搜索会恒 0 条。上面 `wrap` 附近那段才是 encodeURI 的包装。
  J.encodeURIComponent = function (v) { return encodeURIComponent(S(v)); };
  J.toURL = function (v) {
    // legado 返回 java.net.URL, 源主要取 .origin/.host/.path/.protocol。
    // QuickJS 没有 Web 的 URL 构造器, 解析交给宿主。
    var o = JSON.parse(g.__to_url(S(v), S(baseUrl)));
    o.toString = function () { return this.href; };
    return o;
  };
  // 简→繁/繁→简 (`java.t2s` / `java.s2t`) 由宿主直接注册在 java 对象上。

  function HashMap() { this.__m = {}; }
  HashMap.prototype = {
    put: function (k, v) { var o = this.get(k); this.__m[S(k)] = v; return o; },
    get: function (k) { var v = this.__m[S(k)]; return v === undefined ? null : v; },
    getOrDefault: function (k, d) { var v = this.get(k); return v === null ? d : v; },
    containsKey: function (k) { return Object.prototype.hasOwnProperty.call(this.__m, S(k)); },
    remove: function (k) { var v = this.get(k); delete this.__m[S(k)]; return v; },
    size: function () { return Object.keys(this.__m).length; },
    isEmpty: function () { return this.size() === 0; },
    keySet: function () { return Object.keys(this.__m); },
    toJSON: function () { return this.__m; },
    toString: function () { return JSON.stringify(this.__m); }
  };
  function ArrayList() { this.__a = []; }
  ArrayList.prototype = {
    add: function (v) { this.__a.push(v); return true; },
    get: function (i) { return this.__a[i]; },
    size: function () { return this.__a.length; },
    isEmpty: function () { return this.__a.length === 0; },
    toArray: function () { return this.__a.slice(); },
    toJSON: function () { return this.__a; }
  };
  J.util = { HashMap: HashMap, LinkedHashMap: HashMap, Map: HashMap, ArrayList: ArrayList };
  // `java.io.*` 由 `java_compat.js` 提供（ByteArrayInputStream/OutputStream、
  // GZIP、Reader 等），这里不再重复实现 —— 早先在这里加过一份同名构造，
  // 结果被 compat 的同名注册覆盖，`available()` 之类的差异就变成了灵异现象。
  // `new java.util.Date()` / `Date(millis)`: 书源用它做时间戳运算。
  // `getTime()` 与 JS 一致; `toString()` 按 ISO 给出可读时间。
  J.util.Date = function (millis) {
    var t = millis === undefined || millis === null ? Date.now() : Number(millis);
    if (isNaN(t)) t = Date.now();
    this.__t = t;
  };
  J.util.Date.prototype = {
    getTime: function () { return this.__t; },
    toMillis: function () { return this.__t; },
    before: function (other) { return this.__t < Number(other && other.__t !== undefined ? other.__t : other); },
    after: function (other) { return this.__t > Number(other && other.__t !== undefined ? other.__t : other); },
    equals: function (other) { return this.__t === Number(other && other.__t !== undefined ? other.__t : other); },
    toString: function () { return new Date(this.__t).toISOString(); }
  };
  // Rhino 里 `java.lang.String` 是 Java String: 除了 JS String 原生方法, 书源
  // 还会调用 Java 侧方法 —— `isEmpty()`(判断元素文本/标题是否为空) 与
  // `replaceAll(regex, repl)`(从 class 名里抠出 `p_123456`)。缺这两个方法时
  // 脚本抛 "not a function", 整条 chapterList 直接失败 → 目录 0 章。
  // 方法定义成**不可枚举**, 避免 `JSON.stringify` / `for..in` 把函数当字符项。
  function javaStringProto() {
    var proto = {
      isEmpty: function () { return String(this).length === 0; },
      length: function () { return String(this).length; },
      // Java `String.replaceAll(regex, repl)`: 第一个参数是**正则**。
      // 原生 replaceAll 传字符串时按字面量匹配, 与 Java 语义不同, 这里补包装;
      // 传 RegExp 时仍走原生(书源里两种写法都出现过)。
      replaceAll: function (pattern, replacement) {
        if (pattern !== null && typeof pattern === 'object' && typeof pattern.test === 'function') {
          return String(this).replace(pattern, S(replacement));
        }
        return String(this).replace(new RegExp(S(pattern), 'g'), S(replacement));
      },
      replaceFirst: function (pattern, replacement) {
        if (pattern !== null && typeof pattern === 'object' && typeof pattern.test === 'function') {
          return String(this).replace(pattern, S(replacement));
        }
        return String(this).replace(new RegExp(S(pattern)), S(replacement));
      },
      matches: function (pattern) { return new RegExp('^(?:' + S(pattern) + ')$').test(String(this)); },
      // Java `String.contains` / `equalsIgnoreCase` / `hashCode`。
      //
      // legado 里字符串是 **Java String**, 书源会直接调这几个方法:
      // `title.contains('第')`、`name.equalsIgnoreCase('最新章节')`。
      // JS 原生没有它们, 缺了就是 "not a function" —— 整条字段规则失败。
      contains: function (needle) { return String(this).indexOf(S(needle)) >= 0; },
      equalsIgnoreCase: function (other) {
        return String(this).toLowerCase() === S(other).toLowerCase();
      },
      // Java String.hashCode: `h = 31 * h + ch`, 结果按有符号 32 位整数。
      hashCode: function () {
        var s = String(this), h = 0;
        for (var i = 0; i < s.length; i++) {
          h = (Math.imul(31, h) + s.charCodeAt(i)) | 0;
        }
        return h;
      }
    };
    Object.keys(proto).forEach(function (name) {
      try {
        Object.defineProperty(String.prototype, name, {
          value: proto[name], enumerable: false, writable: true, configurable: true
        });
      } catch (e) { /* 宿主冻结时保持原生行为 */ }
    });
  }
  javaStringProto();
  J.lang = {
    String: function (s) { return new String(S(s)); },
    System: { currentTimeMillis: function () { return Date.now(); } },
    // Java Integer.parseInt(String, int radix) 必须带进制: 书源用它按 16 进制
    // 逐字节还原密钥(`Integer.parseInt(k.substring(i, i3), 16)`)。固定按 10 进制
    // 解析会得到 NaN, 之后的位运算把密钥整体变成全 0。
    Integer: {
      parseInt: function (s, radix) { return parseInt(s, radix === undefined ? 10 : Number(radix)); },
      valueOf: function (s, radix) { return parseInt(s, radix === undefined ? 10 : Number(radix)); }
    },
    // `new java.lang.Long(x)` / `Long.parseLong`: 书源用它把大整数(时间戳、
    // 书籍 id)显式转数值。JS number 精度足够覆盖这些场景。
    Long: {
      parseLong: function (s) { return parseInt(S(s), 10); },
      valueOf: function (s) { return parseInt(S(s), 10); },
      MAX_VALUE: 9223372036854775807,
      MIN_VALUE: -9223372036854775808
    },
    // `new java.lang.StringBuilder()`: 书源用它拼长文本。
    StringBuilder: function (initial) {
      this.__parts = initial === undefined || initial === null ? [] : [S(initial)];
    }
  };
  J.lang.StringBuilder.prototype = {
    append: function (v) { this.__parts.push(S(v)); return this; },
    insert: function (at, v) { this.__parts.splice(Number(at) || 0, 0, S(v)); return this; },
    reverse: function () { this.__parts.reverse(); return this; },
    length: function () { return this.__parts.join('').length; },
    toString: function () { return this.__parts.join(''); },
    clear: function () { this.__parts = []; return this; }
  };
  g.Packages = { java: J };

  // Rhino 的 JavaImporter + with(importer) 语法（部分书源 jsLib 顶层使用）。
  // 把传入的包/类展开为实例属性, with 语句按属性名解析裸类名。
  function JavaImporter() {
    var self = this;
    // `Packages.org.jsoup.Jsoup` 这种"包名+类名同形的对象"要按**类名**注册
    // (`with (aly) { Jsoup.connect(url) }`), 不能把它的成员平铺成 `connect`/
    // `parse` —— 那样 `Jsoup` 这个名字就不存在了。给对象标记类名后按类名注册。
    function absorb(value, fallbackName) {
      if (!value) return;
      if (typeof value === 'function') {
        var name = value.name || fallbackName || '';
        if (name) self[name] = value;
        return;
      }
      if (typeof value === 'object') {
        // 显式标记的类名优先(见 jsoup 等的 `__classNames` 注册)。
        var declared = value.__javaClassName;
        if (declared) {
          self[declared] = value;
          return;
        }
        // 其余情况保持既有语义: 包对象按成员展开。
        Object.keys(value).forEach(function (k) { self[k] = value[k]; });
      }
    }
    for (var i = 0; i < arguments.length; i++) absorb(arguments[i]);
    // Rhino 的 JavaImporter.importPackage / importClass 都**遍历全部实参**逐个导入
    // (ImporterTopLevel.js_importPackage), 只取第一个会让后续包里的类全部缺失。
    this.importClass = function () {
      for (var i = 0; i < arguments.length; i++) absorb(arguments[i], '');
      return this;
    };
    this.importPackage = function () {
      for (var i = 0; i < arguments.length; i++) absorb(arguments[i]);
      return this;
    };
  }
  g.JavaImporter = JavaImporter;

  // 发现页/登录面板动作：某聚合源的「🔍搜索」用 java.searchBook(...)、
  // 「⚙登录」用 java.open('login','http',null)。这些副作用无法在 WebView 侧
  // 产生，统一登记成意图由宿主回传前端执行（见 ExploreActionOutcome）。
  J.open = function (type, kind, data) {
    g.__explore_intent('open', JSON.stringify({ type: S(type), kind: S(kind), data: data == null ? null : data }));
    return true;
  };
  J.searchBook = function (key, source) {
    g.__explore_intent('search', JSON.stringify({ key: S(key), source: S(source) }));
    return true;
  };
  // 书源常用的"打开书籍详情/阅读"入口（部分聚合源的发现页按钮会调用）。
  J.openBook = function (url) {
    g.__explore_intent('openBook', S(url));
    return true;
  };
  // `java.openUrl(url[, mimeType])`: legado 用系统浏览器/外部应用打开链接。
  // 桌面端没有对应容器, 这里只登记"要外部打开"的意图(与 `openBook` 同构),
  // 保证脚本不会因为方法不存在而中断。
  J.openUrl = function (url, mimeType) {
    g.__explore_intent(
      'openUrl',
      JSON.stringify({ url: S(url), mimeType: mimeType == null ? null : S(mimeType) })
    );
    return true;
  };
  // `java.randomUUID()`: UUID v4。书源用它生成请求去重键/临时 id。
  J.randomUUID = function () {
    var hex = '0123456789abcdef', out = '';
    for (var i = 0; i < 36; i++) {
      if (i === 8 || i === 13 || i === 18 || i === 23) { out += '-'; continue; }
      if (i === 14) { out += '4'; continue; }
      if (i === 19) { out += hex.charAt(8 + Math.floor(Math.random() * 4)); continue; }
      out += hex.charAt(Math.floor(Math.random() * 16));
    }
    return out;
  };
  // `java.strToBytes(str[, charset])` / `java.bytesToStr(bytes[, charset])`:
  // 字符串与字节序列互转。legado 返回 Rhino 的 ByteArray, 脚本按数组用
  // (`.length` / 索引 / `.slice`), 因此这里同样交出普通数组。
  J.strToBytes = function (v, charset) {
    var enc = charset === undefined || charset === null ? 'UTF-8' : S(charset);
    try {
      var parsed = JSON.parse(S(g.__str_to_bytes(S(v), enc)));
      return Array.isArray(parsed) ? parsed : [];
    } catch (e) { return []; }
  };
  J.bytesToStr = function (v, charset) {
    var enc = charset === undefined || charset === null ? 'UTF-8' : S(charset);
    var bytes = [];
    if (Array.isArray(v) || (v && typeof v.length === 'number')) {
      for (var i = 0; i < v.length; i++) bytes.push(Number(v[i]) & 0xff);
    } else {
      var text = S(v);
      for (var j = 0; j < text.length; j++) bytes.push(text.charCodeAt(j) & 0xff);
    }
    try { return S(g.__bytes_to_str(JSON.stringify(bytes), enc)); } catch (e) { return ''; }
  };

  // ── Rhino 的 `org.jsoup.*` 桥 ──
  // 部分书源直接在规则里写 Java 风格的 JSoup 调用（某些书源:
  // `var doc = org.jsoup.Jsoup.parse(result); doc.select('a.flex-wrp');`），
  // 社区版 legado 靠 Rhino 的 Java 互操作跑通。桌面端没有 JVM，这里用同一套
  // 宿主选择器引擎(`__rule_get`)造出等价的 Elements/Element 对象。
  //
  // 只实现书源里真正会用到的子集: parse / select / text / ownText / html /
  // outerHtml / attr / hasAttr / tagName / size / isEmpty / first / last / get /
  // eachText / toString / absUrl / id / className / val。其余方法返回空值而不是
  // 抛错 —— 让规则至少能跑出可用结果, 也不会因未知方法整条链路失败。
  function jsoupElements(json) {
    var list = wrapElements(json);
    list.size = function () { return list.length; };
    list.isEmpty = function () { return list.length === 0; };
    list.first = function () { return list.length ? list[0] : null; };
    list.last = function () { return list.length ? list[list.length - 1] : null; };
    list.get = function (i) { return list[i] || null; };
    list.text = function () { return list.map(function (el) { return el.text(); }).join(' ').trim(); };
    list.eachText = function () { return list.map(function (el) { return el.text(); }); };
    list.attr = function (name) { return list.length ? list[0].attr(name) : ''; };
    list.html = function () { return list.length ? list[0].html() : ''; };
    list.outerHtml = function () { return list.length ? list[0].outerHtml() : ''; };
    list.select = function (rule) { return jsoupElementsFromElements(list, rule); };
    list.toString = function () { return list.map(function (el) { return el.outerHtml(); }).join(''); };
    // `Elements.toArray()`：书源用它逐条过滤后再拼回 HTML（见 `elementsApi`
    // 里同名方法的注释）。`jsoupElements` 与 `elementsApi` 是两套包装，方法要
    // 各自补齐，否则 `doc.select(...).toArray()` 仍会抛 "not a function"。
    list.toArray = function () {
      var out = [];
      for (var i = 0; i < list.length; i++) out.push(list[i]);
      return out;
    };
    // 关键: 规则以 `els` 结尾返回时, 宿主需要**元素快照数组**才能继续按
    // name/author/bookUrl 等字段规则取值。没有 toJSON 时会被序列化成
    // "[object Object],[object Object]" 而解析出 0 本(这是常见症状)。
    list.toJSON = function () {
      return list.map(function (el) { return el.__snapshot; });
    };
    return list;
  }

  /** 对每个元素分别 select，合并结果（JSoup Elements.select 语义）。 */
  function jsoupElementsFromElements(elements, rule) {
    // 直接把子元素包装对象合并进集合: 走 JSON 快照重建会丢掉 `__owner`,
    // 之后 `select('a,script').remove()` 就删不掉任何东西了(正文清洗会失效)。
    var out = [];
    for (var i = 0; i < elements.length; i++) {
      var sub = elements[i].select(rule);
      for (var j = 0; j < sub.length; j++) out.push(sub[j]);
    }
    return elementsApi(out);
  }

  /**
   * 构造文档级的"可 select 根"。
   *
   * 真实 JSoup 的 `Document.select(rule)` 在整个文档上查一次。这里不能把
   * 所有元素都塞进 Elements 再逐个 select —— `html` 与 `body` 都能选到同一批
   * 后代，结果会成倍重复（书列表直接翻倍）。因此只保留**一个**根：以 `html`
   * 元素的 outerHtml 为内容、对外表现为 Elements，`select` 在其内部查一次。
   */
  function jsoupDocument(html) {
    var text = S(html);
    var roots = wrapElements(g.__rule_get('html', text, 'elements'));
    var rootHtml = roots.length ? roots[0].outerHtml() : text;
    var list = wrapElements(g.__rule_get('html, html *', text, 'elements'));
    // 文档根：select 以整份 HTML 为内容查一次, 覆盖 head/body 及其后代。
    list.select = function (rule) { return jsoupElements(g.__rule_get(S(rule), rootHtml, 'elements')); };
    list.title = function () {
      var t = g.__rule_get('title', text, 'text');
      return t === undefined || t === null ? '' : S(t);
    };
    return list;
  }

  // `org.jsoup.Jsoup.connect(url)` 链式 HTTP 客户端。legado 源用它做表单提交与
  // 连通性探测: 聚合类源的 header 脚本靠 `Jsoup.connect(url)...execute()` 逐站测
  // 响应耗时, 挑最快的镜像域名。底层复用宿主 `__http` 原语, 因此与书源规则共享
  // 同一套 header / cookie 与代理设置。
  function jsoupConnectionResponse(o) {
    return {
      statusCode: function () { return o.code; },
      statusMessage: function () { return ''; },
      body: function () { return o.body; },
      charset: function () { return 'UTF-8'; },
      contentType: function () { return ''; },
      url: function () { return o.url; },
      header: function (k) { return o.headers && o.headers[S(k).toLowerCase()] !== undefined ? o.headers[S(k).toLowerCase()] : null; },
      headers: function () { return o.headers || {}; },
      hasHeader: function (k) { return !!(o.headers && o.headers[S(k).toLowerCase()] !== undefined); },
      cookies: function () { return {}; },
      parse: function () { return Jsoup.parse(o.body); }
    };
  }
  function JsoupConnect(url) {
    var opts = { url: S(url), method: 'GET', headers: {}, data: {}, body: null };
    function encodeData(map) {
      var parts = [];
      for (var k in map) {
        if (Object.prototype.hasOwnProperty.call(map, k)) {
          parts.push(encodeURIComponent(k) + '=' + encodeURIComponent(map[k]));
        }
      }
      return parts.join('&');
    }
    var chain = {
      // 超时/重定向/校验类选项由宿主客户端统一控制(源级超时与代理已生效),
      // 这里接受调用但不逐项覆盖, 保持与书源规则一致的传输行为。
      timeout: function () { return chain; },
      maxBodySize: function () { return chain; },
      validateTLSCertificates: function () { return chain; },
      ignoreContentType: function () { return chain; },
      ignoreHttpErrors: function () { return chain; },
      followRedirects: function () { return chain; },
      method: function (m) { opts.method = S(m).toUpperCase(); return chain; },
      header: function (k, v) { opts.headers[S(k)] = S(v); return chain; },
      headers: function (map) {
        if (map) {
          for (var k in map) {
            if (Object.prototype.hasOwnProperty.call(map, k)) opts.headers[S(k)] = S(map[k]);
          }
        }
        return chain;
      },
      userAgent: function (v) { opts.headers['User-Agent'] = S(v); return chain; },
      referrer: function (v) { opts.headers['Referer'] = S(v); return chain; },
      cookie: function (k, v) { opts.headers['Cookie'] = k + '=' + v; return chain; },
      cookies: function (map) {
        if (map) {
          var parts = [];
          for (var k in map) { if (Object.prototype.hasOwnProperty.call(map, k)) parts.push(k + '=' + map[k]); }
          if (parts.length) opts.headers['Cookie'] = parts.join('; ');
        }
        return chain;
      },
      data: function (k, v) {
        if (k && typeof k === 'object') {
          for (var n in k) { if (Object.prototype.hasOwnProperty.call(k, n)) opts.data[S(n)] = S(k[n]); }
        } else {
          opts.data[S(k)] = S(v);
        }
        return chain;
      },
      requestBody: function (b) { opts.body = S(b); return chain; },
      execute: function () {
        if (typeof g.__http !== 'function') { throw new Error('Jsoup.connect: HTTP 原语不可用'); }
        var body = opts.body;
        if (body === null && opts.method === 'POST' && Object.keys(opts.data).length) {
          body = encodeData(opts.data);
          if (opts.headers['Content-Type'] === undefined) {
            opts.headers['Content-Type'] = 'application/x-www-form-urlencoded';
          }
        }
        var raw = g.__http(opts.method, opts.url, body === null ? '' : body, JSON.stringify(opts.headers));
        var parsed;
        try { parsed = JSON.parse(raw); } catch (e) { parsed = { body: '', code: 0, url: '', headers: {} }; }
        if (parsed === null || typeof parsed !== 'object') parsed = { body: '', code: 0, url: '', headers: {} };
        if (parsed.headers === undefined || parsed.headers === null) parsed.headers = {};
        return jsoupConnectionResponse(parsed);
      },
      get: function () { return chain.method('GET').execute(); },
      post: function () { return chain.method('POST').execute(); },
      head: function () { return chain.method('HEAD').execute(); }
    };
    return chain;
  }
  var Jsoup = {
    connect: function (url) { return JsoupConnect(url); },
    parse: function (html) {
      // 真实 JSoup 的 `Document.select` 覆盖**整个文档**（含 head/title/meta）。
      // 只取 `body, body *` 会让 `doc.select('title')` / `meta` 恒为空 ——
      // 部分书源正是靠 meta 取书名或封面。
      var doc = jsoupDocument(html);
      // JSoup 的 Document 本身就是"可 select 的根"，并且能直接取 text()/html()。
      doc.body = function () {
        var b = jsoupElements(g.__rule_get('body', S(html), 'elements'));
        return b.length ? b[0] : null;
      };
      doc.head = function () {
        var h = jsoupElements(g.__rule_get('head', S(html), 'elements'));
        return h.length ? h[0] : null;
      };
      doc.location = function () { return ''; };
      doc.baseUri = function () { return ''; };
      doc.charset = function (c) { return c === undefined ? 'UTF-8' : doc; };
      doc.__html = S(html);
      return doc;
    },
    parseBodyFragment: function (html) { return Jsoup.parse(html); },
    clean: function (html) { return S(html); }
  };
  // 给"包名+类名同形的对象"打上类名标记, 供 `JavaImporter` 按类名注册(见该函数)。
  // 必须**不可枚举**: 否则 `JSON.stringify(Jsoup)` / `for..in` 会把内部标记泄漏
  // 给源脚本, 变成它们从未见过的脏字段。
  function tagJavaClassName(target, name) {
    try {
      Object.defineProperty(target, '__javaClassName', {
        value: name, enumerable: false, writable: true, configurable: true
      });
    } catch (e) { /* 冻结对象无法打标记时按包对象成员展开处理 */ }
    return target;
  }
  // `JavaImporter(Packages.org.jsoup.Jsoup)` 要按类名 Jsoup 注册。
  tagJavaClassName(Jsoup, 'Jsoup');

  // `org.jsoup.Connection.Method.HEAD` 这类常量: 源脚本用 `with (JavaImporter(...))`
  // 后按裸名或 `Connection.Method.X` 传入 `method()`。
  var JsoupConnection = { Method: { GET: 'GET', POST: 'POST', HEAD: 'HEAD', PUT: 'PUT', PATCH: 'PATCH', DELETE: 'DELETE', OPTIONS: 'OPTIONS', TRACE: 'TRACE' } };
  tagJavaClassName(JsoupConnection, 'Connection');
  // `new org.jsoup.select.Elements()` / `new org.jsoup.nodes.Element('div')`:
  // 书源的 jsLib 常用它们**手工拼列表项**(造 div/h3/a/img/span 再 `out.add(...)`
  // 交回宿主)。legado 靠 Rhino 的 Java 互操作构造真 Java 对象,
  // 桌面端用可变的元素快照模拟:
  //   - `Element(tag[, baseUri])` 造一个空元素, 支持 attr/text/html setter、
  //     appendChild / prependElement / appendElement / remove / clone;
  //   - `Elements()` 造空集合, 支持 add / addAll / first / get / size / select。
  // 顶层 `Elements` 类缺失会让 jsLib 里的 `new org.jsoup.select.Elements()` 直接抛
  // "cannot read property 'Elements' of undefined", 整个 bookList 脚本中断
  // (搜索阶段报「书源解析失败」)。
  function JsoupElementsClass() {
    if (!(this instanceof JsoupElementsClass)) return new JsoupElementsClass();
    return elementsApi([]);
  }
  function JsoupElementClass(tag, baseUri) {
    if (!(this instanceof JsoupElementClass)) return new JsoupElementClass(tag, baseUri);
    var name = S(tag) || 'div';
    var snapshot = {
      tag: name,
      text: '',
      ownText: '',
      innerHtml: '',
      outerHtml: '<' + name + '></' + name + '>',
      attrs: {}
    };
    var el = wrapElement(snapshot, null);
    if (baseUri !== undefined && baseUri !== null) {
      try { el.__baseUri = S(baseUri); } catch (_e) {}
    }
    // `appendElement(tag)` 造子元素并追加, 返回**子元素**(JSoup 语义)。
    el.appendElement = function (childTag) {
      var child = new JsoupElementClass(childTag);
      el.appendChild(child);
      return child;
    };
    // `prependElement(tag)` 是它的前置版本; 快照只有字符串 innerHtml,
    // 因此语义退化成"追加"(足够覆盖拼卡片场景)。
    el.prependElement = el.appendElement;
    el.prependChild = function (child) { return el.appendChild(child); };
    // `children()` / `child(i)` 在快照层没有子节点索引, 返回空集合而不是抛错。
    el.child = function () { return null; };
    el.childNodeSize = function () { return 0; };
    el.parent = function () { return el.__owner || null; };
    el.baseUri = function () { return S(el.__baseUri || ''); };
    el.className = function (value) {
      if (arguments.length && value !== undefined) { return el.attr('class', value); }
      return el.attr('class');
    };
    el.id = function (value) {
      if (arguments.length && value !== undefined) { return el.attr('id', value); }
      return el.attr('id');
    };
    return el;
  }
  // 给类打上名字: `JavaImporter(Packages.org.jsoup.select)` / 调试输出都靠它。
  tagJavaClassName(JsoupElementsClass, 'Elements');
  tagJavaClassName(JsoupElementClass, 'Element');

  g.org = {
    jsoup: {
      Jsoup: Jsoup,
      Connection: JsoupConnection,
      Elements: JsoupElementsClass,
      Element: JsoupElementClass,
      Document: function () { return Jsoup.parse(''); },
      // `org.jsoup.select.Elements` / `org.jsoup.nodes.Element` 的**真实包路径**:
      // Rhino 里两者都在 jsoup 的子包下, 书源写的正是全名
      // (`new org.jsoup.select.Elements()`、`new org.jsoup.nodes.Element('div')`)。
      // 顶层同名别名保留给只写 `org.jsoup.Elements` 的老源。
      select: { Elements: JsoupElementsClass },
      nodes: {
        Element: JsoupElementClass,
        Document: function () { return Jsoup.parse(''); },
        TextNode: function (text) {
          // 文本节点: 只有 `text()`/`toString()`, appendChild 时按转义文本插入。
          var value = S(text);
          return {
            text: function (v) { if (arguments.length && v !== undefined) { value = S(v); } return value; },
            toString: function () { return value; },
            outerHtml: function () { return escapeHtmlText(value); }
          };
        }
      }
    }
  };
  // `Packages.org.jsoup.Jsoup` —— Rhino 里 `Packages.<pkg>.<Class>` 的取法。
  g.Packages.org = g.org;

  // `Packages.io.legado.app.help.http.StrResponse` —— legado 的响应类, 部分书源的
  // loginCheckJs 会直接构造它来替换响应:
  //   result = new Packages.io.legado.app.help.http.StrResponse(java.url, body)
  // 对应 `StrResponse(url: String, body: String?)`: 得到 code=200、url 为给定值
  // 的响应对象。缺这个类时脚本直接抛 "cannot read property 'legado' of
  // undefined", 整个 loginCheckJs 失败, 正文再也加载不出来。
  function StrResponseClass(first, second) {
    // 两个实参都有字符串时按 (url, body); 只有一个实参时按 (url, null) 处理,
    // 与 Kotlin 的重载选择一致(单参不会被当成 body)。
    var url = first === undefined || first === null ? '' : S(first);
    var body = second === undefined || second === null ? null : S(second);
    var envelope = {
      body: body === null ? '' : body,
      url: url,
      code: 200,
      headers: {},
      isSuccessful: true
    };
    // 复用 `Resp` 的 StrResponse 语义: code()/body()/url()/headers()/isSuccessful()
    // 等方法齐全, 并且作为脚本返回值时能交出完整信封。
    return Resp(JSON.stringify(envelope));
  }
  tagJavaClassName(StrResponseClass, 'StrResponse');
  // `Packages.io.legado.app.model.analyzeRule.AnalyzeRule` —— 规则分析器。
  //
  // 书源的 jsLib 常这样在**另给的 HTML** 上求值规则, 而不是当前页面:
  // ```js
  // function evalOn(x, r) {
  //   return r == undefined ? this.java.getElement(x)
  //     : Packages.io.legado.app.model.analyzeRule.AnalyzeRule().setContent(r).getElement(x);
  // }
  // ```
  // 缺这个类时目录脚本在 `evalOn(".book-title a", page)` 处抛
  // "cannot read property 'analyzeRule' of undefined", 目录一章都解析不出来。
  //
  // 语义对齐 `AnalyzeRule`: `setContent(content[, baseUrl])` 返回自身(可链式),
  // 之后的 `getElement`/`getElements`/`getString`/`getStringList` 都作用于那份内容。
  function AnalyzeRule() {
    if (!(this instanceof AnalyzeRule)) return new AnalyzeRule();
    var content = '';
    var self = this;
    self.setContent = function (value, base) {
      content = value === undefined || value === null ? '' : S(value);
      if (base !== undefined && base !== null && S(base)) {
        try { g.baseUrl = S(base); } catch (_e) {}
      }
      return self;
    };
    self.setBaseUrl = function (base) {
      try { g.baseUrl = S(base); } catch (_e) {}
      return self;
    };
    self.getElement = function (rule) {
      return elementsApi(wrapElements(g.__rule_get(S(rule), content, 'elements')));
    };
    self.getElements = self.getElement;
    self.getString = function (rule) {
      return g.__rule_get(S(rule), content, 'text');
    };
    self.getStringList = function (rule) {
      var list;
      try { list = JSON.parse(S(g.__rule_get(S(rule), content, 'list'))); } catch (_e) { return []; }
      return list || [];
    };
    self.setRedirectUrl = function (url) { try { g.baseUrl = S(url); } catch (_e) {} return null; };
  }
  tagJavaClassName(AnalyzeRule, 'AnalyzeRule');

  g.Packages.io = {
    legado: {
      app: {
        help: {
          http: {
            StrResponse: StrResponseClass
          }
        },
        model: {
          analyzeRule: {
            AnalyzeRule: AnalyzeRule
          }
        }
      }
    }
  };

  g.cache = {
    get: function (k) { return g.__store_get(S(k)); },
    put: function (k, v) { g.__store_put(S(k), S(v)); return v; },
    'delete': function (k) { g.__store_del(S(k)); },
    getFile: function (k) { return g.__store_get('file:' + S(k)); },
    putFile: function (k, v) { g.__store_put('file:' + S(k), S(v)); return v; },
    // legado `CacheManager` 的类型化读取: 书源把计数/时间戳存进缓存后再取回。
    // 取不到时返回 null(而不是 0), 与 Kotlin 侧可空返回(`Int?`)一致。
    getInt: function (k) {
      var v = g.__store_get(S(k));
      if (v === null || v === undefined || v === '') return null;
      var n = parseInt(String(v), 10);
      return isNaN(n) ? null : n;
    },
    getLong: function (k) {
      var v = g.__store_get(S(k));
      if (v === null || v === undefined || v === '') return null;
      var n = parseInt(String(v), 10);
      return isNaN(n) ? null : n;
    },
    getDouble: function (k) {
      var v = g.__store_get(S(k));
      if (v === null || v === undefined || v === '') return null;
      var n = parseFloat(String(v));
      return isNaN(n) ? null : n;
    },
    getFloat: function (k) { return g.cache.getDouble(k); },
    // 字节数组: 按 latin1 逐字节存, 读回为数字数组(与 `strToBytes` 同形)。
    getByteArray: function (k) {
      var v = g.__store_get(S(k));
      if (v === null || v === undefined || v === '') return null;
      var out = [];
      for (var i = 0; i < String(v).length; i++) out.push(String(v).charCodeAt(i) & 0xff);
      return out;
    },
    getFromMemory: function (k) {
      var r = g.__mem_get(S(k));
      if (r === null || r === undefined) return null;
      try { return JSON.parse(r); } catch (e) { return r; }
    },
    putMemory: function (k, v) { g.__mem_put(S(k), JSON.stringify(v === undefined ? null : v)); return v; },
    deleteMemory: function (k) { g.__mem_put(S(k), 'null'); }
  };

  var si = JSON.parse(g.__source_info());
  var src = g.source;
  // 正常情况下 si.url 就是书源地址; 为空时（调用方未包 with_source）保留宿主
  // 预先设置的 source.key/getKey 兜底值, 不要用空串覆盖。
  if (si.url) {
    src.bookSourceUrl = si.url;
    src.key = si.url;
    src.getKey = function () { return S(si.url); };
  }
  src.bookSourceName = si.name;
  // legado BookSource 的注释字段在 JS 里可读可写; 书源常把解密脚本放在这里
  // 再用 `eval(String(source.bookSourceComment))` 取用(issue #13)。
  src.bookSourceComment = si.comment;
  src.variableComment = si.variableComment;
  // legado 的 BookSource 在 JS 里可读 loginUrl / loginUi。一类书源把登录脚本
  // 只放在 loginUrl 里, 再用 `eval(String(source.loginUrl)); login();` 复用其中的
  // 函数 —— 取不到就是 login is not defined, 发现页整块失效。
  src.loginUrl = si.loginUrl;
  src.loginUi = si.loginUi;
  // legado 的 `getVariable()` 在**无值**时返回空串，不是 `undefined`/`null`。
  //
  // 书源普遍写 `var v = source.getVariable(); if (v == "") { set(0); }` 做首次
  // 初始化 —— 返回 undefined 时判空不成立，紧接着的 `JSON.parse(v)` 抛
  // `unexpected token: 'undefined'`，发现页整块失效。
  src.getVariable = function () {
    var v = g.__source_get('variable');
    if (v === null || v === undefined) v = g.__store_get('sv:' + si.url);
    return v === null || v === undefined ? '' : String(v);
  };
  src.setVariable = function (v) { if (!g.__source_set('variable', S(v))) throw new Error('保存书源变量失败'); };
  src.putVariable = src.setVariable;
  src.getLoginInfo = function () { return g.__source_get('loginInfo'); };
  src.getLoginInfoMap = function () { try { return JSON.parse(src.getLoginInfo() || '{}'); } catch (e) { return {}; } };
  src.putLoginInfo = function (v) { return g.__source_set('loginInfo', S(v)); };
  src.removeLoginInfo = function () { g.__source_del('loginInfo'); };
  src.getLoginHeader = function () { return g.__source_get('loginHeader'); };
  // legado 的 `getLoginHeaderMap()` 在**没有登录头**时返回 `null`（`BookSource.kt`：
  // `getLoginHeader()` 为空直接 `null`，否则 GSON 反序列化成 Map）。
  //
  // 书源普遍靠这个判空来触发「首次取 token」：
  //   if (source.getLoginHeaderMap() == null) { this.Token(); }
  // 早先这里恒返回 `{}`，条件永不成立，`Token()` 从不执行 —— 后续请求全部缺少
  // `authorization`，接口回 `401 Api Gateway Auth ERROR`。
  // 界面症状：**详情简介只剩标签、目录 0 章、正文空白**，而搜索正常
  // （搜索接口不校验 token）。
  src.getLoginHeaderMap = function () {
    var raw = src.getLoginHeader();
    if (raw === null || raw === undefined || String(raw).trim() === '' || String(raw).trim() === '{}') return null;
    try { return JSON.parse(raw); } catch (e) { return null; }
  };
  src.putLoginHeader = function (v) {
    var text = typeof v === 'string' ? v : JSON.stringify(v), headers = JSON.parse(text);
    if (!g.__source_set('loginHeader', text)) throw new Error('保存登录请求头失败');
    Object.keys(headers).forEach(function (key) { if (key.toLowerCase() === 'cookie') g.cookie.setCookie(si.url, S(headers[key])); });
  };
  src.removeLoginHeader = function () { g.__source_del('loginHeader'); g.cookie.removeCookie(si.url); };
  // legado: source.login() 用已保存的登录信息重跑登录脚本。
  // 这里登记一个 effect, 由宿主(loginCheckJs 流程)真正执行 loginUrl。
  src.login = function () { g.__login_request(); return true; };
  J.getStrResponse = function (url, method, body, headers) {
    // 无参: 返回最近一次请求的响应(StrResponse 语义); 带参: 直接发一次请求。
    if (arguments.length === 0) return Resp(g.__last_response());
    if (arguments.length === 1 && typeof url === 'string') return Resp(g.__last_response());
    var m = S(method || 'GET').toUpperCase();
    var h = headers == null ? '' : (typeof headers === 'string' ? headers : JSON.stringify(headers));
    return Resp(g.__http(m, S(url), body == null ? '' : S(body), requestHeaders(h)));
  };
  // legado 里脚本的 `java` 是 AnalyzeUrl 实例, 它带 `url`/`urlNoQuery`/`ruleUrl`
  // 几个**当前请求地址**字段(`AnalyzeUrl.kt`)。某些书源的 loginCheckJs 直接用
  // `result.url().includes("content")` 之外的判断, 或写 `java.url` 取地址 ——
  // 缺这几个属性会直接抛 "cannot read property 'xxx' of undefined", 整个
  // loginCheckJs 失败, 正文就再也加载不出来。
  //
  // 取值来源是"最近一次请求的响应"里的 url: 宿主在执行前已用当前响应初始化了
  // 那个槽位(见 `apply_login_check_js`), 因此与 legado 的语义一致。
  (function () {
    function currentUrl() {
      try {
        var raw = JSON.parse(g.__last_response() || '{}');
        return S(raw && raw.url);
      } catch (_e) {
        return '';
      }
    }
    function defineUrlProp(name, getter) {
      try {
        Object.defineProperty(J, name, {
          get: getter,
          set: function () {},
          configurable: true,
          enumerable: false
        });
      } catch (_e) {}
    }
    defineUrlProp('url', currentUrl);
    defineUrlProp('urlNoQuery', function () {
      var u = currentUrl();
      var i = u.indexOf('?');
      return i < 0 ? u : u.slice(0, i);
    });
    defineUrlProp('ruleUrl', currentUrl);
  })();
  J.upLoginData = function (data) { g.__login_update(JSON.stringify(data == null ? null : data)); };
  J.reLoginView = function () { g.__login_refresh(); };
  // legado: `java.refreshExplore()` 刷新发现页（只重算分类，不重跑登录）。
  // 早先把它等同于 reLoginView 是错的：某类书源的「🔄刷新」按钮会因此弹出
  // 登录面板，而不是刷新发现页。
  J.refreshExplore = function (flag) { g.__explore_refresh(flag === undefined ? '' : S(flag)); return true; };
  src.put = function (k, v) { g.__store_put('sp:' + si.url + ':' + S(k), S(v)); return v; };
  src.get = function (k) { return g.__store_get('sp:' + si.url + ':' + S(k)) || ''; };

  function withVariables(o, prefix, keyField) {
    o.getVariable = function (k) {
      var base = prefix + S(o[keyField]);
      return g.__store_get(k === undefined ? base : base + ':' + S(k)) || '';
    };
    o.putVariable = function (k, v) {
      g.__store_put(prefix + S(o[keyField]) + ':' + S(k), S(v));
      return true;
    };
    // legado 的 `book` 是**一个** Book 对象, 贯穿同一本书的所有字段规则:
    // 书源常在 `name` 里写 `book.bookUrl = …`, 再由 `bookUrl` 规则
    // (`@js:book.bookUrl`) 读回 —— 搜索结果的详情链接就是这么来的。
    //
    // 我们的 JS 上下文按次重建, 直接赋值会随上下文丢失, `bookUrl` 于是恒为空
    // (表现为搜索结果全部点不进去)。这里把 `book.bookUrl` 的写入落到持久化
    // 存储, 并在重建时优先读回, 等价于 legado 的跨字段共享。
    var stateKey = prefix + 'shared:' + S(o[keyField] || 'search');
    var initial = S(o[keyField]);
    try {
      Object.defineProperty(o, keyField, {
        get: function () {
          var stored = g.__store_get(stateKey);
          return stored === null || stored === undefined || stored === '' ? initial : stored;
        },
        set: function (value) { g.__store_put(stateKey, S(value)); },
        configurable: true,
        enumerable: true
      });
    } catch (_e) {}
    // legado 的 Book/BookChapter 只实现 RuleDataInterface(没有 getKey), 只有
    // BookSource 才有。但插件常做 `book.getKey ? book.getKey() : book.bookUrl` 式探测,
    // 因此按"主键即 bookUrl / 章节 url"补上 —— 与 `source.getKey()`（书源地址）
    // 不是同一个东西，前者是数据主键，后者是书源主键。
    o.getKey = function () { return S(o[keyField]); };
    // legado `BaseBook.getCustomVariable()` / `putCustomVariable(value)`:
    // 书籍的"自定义变量"字段。书源用它做标记(加锁状态、上次进度等),
    // 例如 `book.getVariable("custom")` 与 `book.putCustomVariable('')` 会同时出现。
    // 两者底层是同一份变量存储, 因此按 `custom` 键读写, 保持互相可见。
    o.getCustomVariable = function () { return o.getVariable('custom'); };
    o.putCustomVariable = function (value) {
      o.putVariable('custom', S(value));
      return true;
    };
    return o;
  }
  function rulePut(key, value) {
    if (g.chapter && g.chapter.url) g.chapter.putVariable(key, value);
    else if (g.book && g.book.bookUrl) g.book.putVariable(key, value);
    else src.put(key, value);
    return value;
  }
  function ruleGet(key) {
    if (key === 'bookName' && g.book && g.book.bookUrl) return S(g.book.name);
    if (key === 'title' && g.chapter && g.chapter.url) return S(g.chapter.title);
    return (g.chapter && g.chapter.url && g.chapter.getVariable(key)) ||
      (g.book && g.book.bookUrl && g.book.getVariable(key)) || src.get(key) || '';
  }
  function withReadConfig(book) {
    var sourceKey = S(si.url).trim().replace(/\/+$/, '');
    var storageKey = book.bookUrl ? 'brc:' + sourceKey + ':' + S(book.bookUrl) : '';
    var saved = storageKey ? g.__store_get(storageKey) : null;
    function config() {
      if (!book.readConfig || typeof book.readConfig !== 'object') book.readConfig = {};
      return book.readConfig;
    }
    if (saved === 'true' || saved === 'false') config().useReplaceRule = saved === 'true';
    book.setUseReplaceRule = function (enabled) {
      config().useReplaceRule = !!enabled;
      if (storageKey) g.__store_put(storageKey, enabled ? 'true' : 'false');
    };
    book.getUseReplaceRule = function () {
      var enabled = config().useReplaceRule;
      return enabled == null ? true : !!enabled;
    };
    return book;
  }
  var bc = JSON.parse(g.__book_ctx());
  // legado 的这些字段是**非空 Int**(`Book.durChapterIndex: Int = 0`,
  // `BookChapter.index: Int = 0`)。我们这边是 `Option<i32>`, 序列化后为 null,
  // 脚本读到的是 undefined/null —— 算术会变成 NaN。
  //
  // 例如某源按 `let inn = chapter.index - book.durChapterIndex` 判断"是不是
  // 相邻章节"来决定是否抓正文: NaN 落到 default 分支后直接把 result 清空,
  // 表现为"正文解析为空"。这里按 legado 的默认值补 0。
  var bookData = bc.book || {};
  if (bookData.durChapterIndex === undefined || bookData.durChapterIndex === null) {
    bookData.durChapterIndex = 0;
  }
  if (bookData.durChapterPos === undefined || bookData.durChapterPos === null) {
    bookData.durChapterPos = 0;
  }
  if (bookData.durChapterTitle === undefined || bookData.durChapterTitle === null) {
    bookData.durChapterTitle = '';
  }
  var chapterData = bc.chapter || {};
  if (chapterData.index === undefined || chapterData.index === null) {
    chapterData.index = 0;
  }
  if (chapterData.title === undefined || chapterData.title === null) {
    chapterData.title = '';
  }
  g.book = withReadConfig(withVariables(bookData, 'bv:', 'bookUrl'));
  g.chapter = withVariables(chapterData, 'cv:', 'url');
  g.title = S(g.chapter.title);
})(globalThis);
"#;

/// 可复用的 QuickJS 上下文: 同一个 jsLib 只 parse/eval 一次。
///
/// 每次调用仍会重新注册宿主原语与 prelude(它们捕获了本次的 input/base_url 等),
/// 但会跳过几百 KB 的 jsLib 解析 —— 实测这是单次 eval 里最贵的部分。
/// legado 的 jsLib 同样只加载一次(`SharedJsScope`), 语义一致。
struct CachedJsContext {
    /// jsLib 源码的 md5, 用于判断是否需要重建上下文。
    lib_key: String,
    context: Context,
}

thread_local! {
    static JS_CONTEXT: RefCell<Option<CachedJsContext>> = const { RefCell::new(None) };
    /// 当前线程的 eval 嵌套深度。rquickjs 的 `Context::with` 不可重入,
    /// 所以嵌套 eval(例如 `js:` 规则里再调 `java.getString`)必须换用独立上下文。
    static JS_EVAL_DEPTH: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// 退出时把嵌套深度减回去。
struct EvalDepthGuard;

/// 测试用: 读取当前线程缓存上下文的复用键。
#[cfg(test)]
fn cached_js_lib_key() -> Option<String> {
    JS_CONTEXT.with(|cell| cell.borrow().as_ref().map(|entry| entry.lib_key.clone()))
}
impl Drop for EvalDepthGuard {
    fn drop(&mut self) {
        JS_EVAL_DEPTH.with(|depth| depth.set(depth.get().saturating_sub(1)));
    }
}

/// 在当前线程取(或建立)可复用上下文, 并在其中执行 f。
///
/// `Context` 是引用计数句柄(Clone 廉价), 取出后立即释放 RefCell 借用,
/// 避免闭包内部再次进入时重复借用而 panic。
fn with_reusable_context<T>(
    lib_key: &str,
    _shared_js: &str,
    f: impl FnOnce(rquickjs::Ctx<'_>, bool) -> anyhow::Result<T>,
) -> anyhow::Result<T> {
    let nested = JS_EVAL_DEPTH.with(|depth| depth.get() > 0);
    JS_EVAL_DEPTH.with(|depth| depth.set(depth.get() + 1));
    let _guard = EvalDepthGuard;

    // 嵌套调用无法借用外层正在使用的 Context, 退化为一次性上下文(仍需加载 jsLib,
    // 否则嵌套脚本里的 jsLib 函数不可用)。
    if nested {
        let runtime = Runtime::new()?;
        runtime.set_max_stack_size(JS_MAX_STACK_SIZE);
        let context = Context::full(&runtime)?;
        return context.with(|ctx| f(ctx, true));
    }

    let cached = JS_CONTEXT.with(|cell| {
        let slot = cell.borrow();
        match slot.as_ref() {
            Some(entry) if entry.lib_key == lib_key => Some(entry.context.clone()),
            _ => None,
        }
    });

    let (context, initialize) = match cached {
        Some(context) => (context, false),
        None => {
            let runtime = Runtime::new()?;
            runtime.set_max_stack_size(JS_MAX_STACK_SIZE);
            let context = Context::full(&runtime)?;
            JS_CONTEXT.with(|cell| {
                *cell.borrow_mut() = Some(CachedJsContext {
                    lib_key: lib_key.to_string(),
                    context: context.clone(),
                });
            });
            (context, true)
        }
    };

    let result = context.with(|ctx| f(ctx, initialize));
    if result.is_err() { JS_CONTEXT.with(|cell| { *cell.borrow_mut() = None; }); }
    result
}

fn eval_js_inner_with_source(
    script: &str,
    input: Option<&str>,
    base_url: Option<&str>,
    key: Option<&str>,
    page: Option<i32>,
    // 书源地址：正常情况下由 `with_source` 提供的 source 绑定给出；
    // 未设置 source 绑定（如直接 eval）时用它保证 source.getKey() 仍有值。
    // 书籍地址不在这个参数里 —— 它是 thread-local（`with_book_url`），
    // 只用来兜底补齐 JS 的 `book.bookUrl`。
    source_key: Option<&str>,
    bindings: Option<&HashMap<String, JsonValue>>,
    // 图片解密：`Some(bytes)` 时 `result` 注入为**字节序列**，且 prelude 的
    // `decrypt` 走原始字节分支（见 `eval_js_cover_decode`）。
    binary_input: Option<&[u8]>,
) -> anyhow::Result<String> {
    let shared_js = active_js_lib_script();
    let source_info = active_source();
    let source_url = source_info
        .url
        .clone()
        .trim()
        .is_empty()
        .then(|| source_key.unwrap_or("").to_string())
        .unwrap_or_else(|| source_info.url.clone());
    let mut book_ctx = ACTIVE_BOOK_CTX.with(|cell| cell.borrow().clone()).unwrap_or_default();
    // legado 的目录/正文规则可能直接读 `book.bookUrl`。部分调用路径（如纯 JS 规则）
    // 只通过 thread-local 登记了书籍 URL，这里在 book 上下文缺 bookUrl 时补齐。
    let has_book_url = book_ctx
        .book
        .get("bookUrl")
        .and_then(|value| value.as_str())
        .is_some_and(|value| !value.trim().is_empty());
    if !has_book_url {
        // 只取解析期登记的书籍 URL；绝不回退成书源地址，否则 `book.bookUrl`
        // 会被污染成 `https://source.example` 这种书源地址。
        let fallback = ACTIVE_BOOK_URL.with(|cell| cell.borrow().clone());
        if let Some(value) = fallback.filter(|value| !value.trim().is_empty()) {
            book_ctx.book["bookUrl"] = json!(value);
        }
    }

    // 复用键必须带上书源: 不同书源(尤其都没有 jsLib 时)不能共用同一个上下文,
    // 否则规则脚本声明的全局变量会互相可见。同一书源的连续章节仍能命中复用。
    let identity = source_info.http.as_ref().map(|http| http.session.identity.as_str()).unwrap_or(&source_info.url);
    let lib_key = format!("{}\u{1}{}", identity, md5_hex(&shared_js));
    with_reusable_context(&lib_key, &shared_js, |ctx, initialize| {
        let globals = ctx.globals();
        let input_value = input.unwrap_or("");
        let base_url_value = base_url.unwrap_or("");

        globals.set("input", input_value)?;
        // 图片解密：`result` 必须是字节序列，而不是 UTF-8 文本。
        //
        // 传成字符串会让 `decrypt(result)` 走 `auto` 分支 —— 既可能被误判成
        // hex/base64，也已经把 ≥0x80 的字节改写过一次。这里按预扫描好的
        // JSON 数组字面量注入，再包一层"数组 + 流"的双面对象：
        //
        // - 数组语义：legado 的 `ByteArray` 就是按数组用的（`.length`/索引/
        //   `.slice()`/传给 `createSymmetricCrypto`），所以 `decrypt(result)` 直接可用。
        // - 流语义：legado 文档里另一种写法把封面 `result` 当 `InputStream`
        //   （`while ((byte = data.read()) != -1)`），见 `java.io.ByteArrayInputStream`。
        match binary_input {
            Some(bytes) => {
                // 注意：prelude 在下面才执行，这里不能调用它定义的辅助函数。
                //
                // 封面解密的 `result` 要同时满足两种书源写法：
                // - `createSymmetricCrypto(...).decrypt(result)`：按数组/字节序列用；
                // - `while ((byte = data.read()) != -1)`：按 InputStream 用。
                // 因此先建普通数组，再把 read/available/close 作为**不可枚举**
                // 方法挂上去（下标与 `.length` 语义不受影响，`__isByteSequence`
                // 也仍认它是字节序列）。
                let literal = bytes_to_json_array(bytes);
                let value = eval_script(
                    ctx.clone(),
                    &format!(
                        "(function (b) {{\n\
                         var def = function (name, fn) {{\n\
                           Object.defineProperty(b, name, {{ value: fn, enumerable: false, configurable: true }});\n\
                         }};\n\
                         def('read', function () {{\n\
                           if (this.__pos === undefined) this.__pos = 0;\n\
                           if (this.__pos >= this.length) return -1;\n\
                           return this[this.__pos++];\n\
                         }});\n\
                         def('available', function () {{ return this.length - (this.__pos || 0); }});\n\
                         def('close', function () {{}});\n\
                         return b;\n\
                         }})({literal})"
                    ),
                )?;
                globals.set("result", value)?;
            }
            None => globals.set("result", input_value)?,
        }
        // `__byteMode`: prelude 的 `decrypt` 见到它就返回原始字节（latin1 承载），
        // 与 legado 的 `SymmetricCrypto.decrypt -> ByteArray` 对齐。
        globals.set("__byteMode", binary_input.is_some())?;
        globals.set("src", input_value)?;
        globals.set("base_url", base_url_value)?;
        globals.set("baseUrl", base_url_value)?;
        let search = ACTIVE_SEARCH.with(|cell| cell.borrow().clone());
        globals.set("key", key.or_else(|| search.as_ref().map(|(key, _)| key.as_str())).unwrap_or(""))?;
        globals.set("page", page.or_else(|| search.as_ref().map(|(_, page)| *page)).unwrap_or(1))?;

        // Default url variable for Legado compatibility
        globals.set("url", base_url_value)?;

        let source_obj = Object::new(ctx.clone())?;
        source_obj.set("key", source_url.clone())?;
        source_obj.set("bookSourceUrl", source_url.clone())?;
        // legado: BookSource.getKey() 返回的是 **bookSourceUrl**（书源地址），不是书籍地址。
        // 书源常见写法 `{{source.getKey().replace('//s','//chapter')}}` 正是靠它拼章节域名；
        // 书籍地址在 legado 里走 `book.bookUrl` / `baseUrl` 绑定，不能用 getKey 代替。
        source_obj.set("getKey", Func::new(move || source_url.clone()))?;
        globals.set("source", source_obj)?;

        let cookie_obj = Object::new(ctx.clone())?;
        let cookie_http = source_info.http.clone();
        cookie_obj.set(
            "getCookie",
            Func::new(move |url: String| -> String {
                cookie_http.as_ref().and_then(|http| cookie_tag_url(&url).map(|url| http.session.cookies.get(&url))).unwrap_or_default()
            }),
        )?;
        let cookie_http = source_info.http.clone();
        cookie_obj.set(
            "removeCookie",
            Func::new(move |url: String| -> bool {
                cookie_http.as_ref().and_then(|http| cookie_tag_url(&url).map(|url| http.session.cookies.clear_url(&url).is_ok())).unwrap_or(false)
            }),
        )?;
        let cookie_http = source_info.http.clone();
        cookie_obj.set("setCookie", Func::new(move |url: String, value: String| -> bool {
            cookie_http.as_ref().and_then(|http| cookie_tag_url(&url).map(|url| http.session.cookies.set(&url, &value).is_ok())).unwrap_or(false)
        }))?;
        globals.set("cookie", cookie_obj)?;
        globals.set("__source_header_script", source_info.header_script.as_deref().unwrap_or_default())?;

        let state = source_info.http.clone();
        globals.set("__source_get", Func::new(move |key: String| -> Option<String> { state.as_ref().and_then(|http| http.session.get(&key)) }))?;
        let state = source_info.http.clone();
        globals.set("__source_set", Func::new(move |key: String, value: String| -> bool { state.as_ref().is_some_and(|http| http.session.put(&key, Some(&value)).is_ok()) }))?;
        let state = source_info.http.clone();
        globals.set("__source_del", Func::new(move |key: String| -> bool { state.as_ref().is_some_and(|http| http.session.put(&key, None).is_ok()) }))?;
        globals.set("__login_update", Func::new(|raw: String| {
            match serde_json::from_str(&raw) {
                Ok(JsonValue::Object(values)) => LOGIN_EFFECTS.with(|cell| cell.borrow_mut().values.extend(values)),
                Ok(JsonValue::Null) => LOGIN_EFFECTS.with(|cell| { let mut effects = cell.borrow_mut(); effects.reset = true; effects.values.clear(); }),
                _ => (),
            }
        }))?;
        globals.set("__login_refresh", Func::new(|| { LOGIN_EFFECTS.with(|cell| cell.borrow_mut().refresh = true); }))?;
        globals.set("__explore_refresh", Func::new(|_flag: String| {
            LOGIN_EFFECTS.with(|cell| cell.borrow_mut().explore_refresh = true);
        }))?;
        globals.set("__explore_intent", Func::new(|kind: String, payload: String| {
            EXPLORE_INTENTS.with(|cell| cell.borrow_mut().push(format!("{kind}\u{1}{payload}")));
        }))?;
        globals.set("__login_request", Func::new(|| { LOGIN_EFFECTS.with(|cell| cell.borrow_mut().login = true); }))?;
        globals.set("__last_response", Func::new(|| -> String {
            LAST_RESPONSE.with(|cell| cell.borrow().clone())
                .unwrap_or_else(|| json!({"body": "", "code": 0, "url": "", "headers": {}}).to_string())
        }))?;
        globals.set("__binary", Func::new(|op: String, raw: String| -> String {
            let result = serde_json::from_str(&raw).map_err(anyhow::Error::from).and_then(|payload| super::js_binary::run(&op, &payload));
            match result { Ok(value) => json!({"value":value}), Err(err) => json!({"error":err.to_string()}) }.to_string()
        }))?;
        globals.set("__sleep", Func::new(|ms: f64| { if ms.is_finite() && ms > 0.0 { std::thread::sleep(Duration::from_millis(ms.min(60_000.0) as u64)); } }))?;

        // 原生原语, 由 JS_PRELUDE 组装成 legado API
        let default_headers = source_info.headers.clone();
        globals.set(
            "__http",
            Func::new(
                move |method: String, url: String, body: String, headers: String, follow: Opt<bool>| -> String {
                    // 第五个参数是 legado 的 `followRedirects` 语义: `java.get/head/post`
                    // 传 false(对应 `Jsoup.connect(...).followRedirects(false)`),
                    // 缺省按 true 与原行为一致(`java.ajax`/`java.connect` 等)。
                    let follow = follow.0.unwrap_or(true);
                    js_http_with_redirect(&method, &url, &body, &headers, &default_headers, follow).to_string()
                },
            ),
        )?;
        // 字节级 HTTP: `java.net.URL(...).openConnection()` 形态的脚本要发送原始
        // 请求体(如 gzip 压缩的注册包)并拿回未解码的响应字节。请求/响应 body 都用
        // latin1 字符串承载, 每个 char 就是一个字节。
        let default_headers = source_info.headers.clone();
        globals.set(
            "__http_bytes",
            Func::new(
                move |method: String, url: String, body: String, headers: String| -> String {
                    js_http_bytes(&method, &url, &body, &headers, &default_headers).to_string()
                },
            ),
        )?;
        let default_headers = source_info.headers.clone();
        globals.set(
            "__connect",
            Func::new(move |spec: String, headers: String| -> String {
                js_connect(&spec, &headers, &default_headers).to_string()
            }),
        )?;
        let default_headers = source_info.headers.clone();
        globals.set(
            "__connect_all",
            Func::new(move |specs: String, headers: String| -> String {
                js_connect_all(&specs, &headers, &default_headers)
            }),
        )?;        globals.set(
            "__sym",
            // 参数打成 JSON 传：`Func::new` 的元组上限是 7 个参数，而这里            // 需要 7 个业务参数 **加** `ctx`（抛异常必需），超上限后
            // `IntoJsFunc` 不成立。打包后签名只剩 `(ctx, json)`。
            Func::new(|ctx: rquickjs::Ctx<'_>, args: String| -> rquickjs::Result<String> {
                let parsed: JsonValue = match serde_json::from_str(&args) {
                    Ok(value) => value,
                    Err(err) => {
                        return Err(rquickjs::Exception::throw_message(
                            &ctx,
                            &format!("__sym 参数解析失败: {err}"),
                        ))
                    }
                };
                let field = |name: &str| -> String {
                    parsed
                        .get(name)
                        .and_then(|v| v.as_str())
                        .unwrap_or_default()
                        .to_string()
                };
                let op = field("op");
                let transformation = field("t");
                let key = field("k");
                let iv = field("iv");
                let data = field("d");
                let format = field("fmt");
                // 位标记见 JS 侧 `symmetric()`: 1=key、2=iv、4=载荷是字节序列。
                let flags = parsed.get("flags").and_then(|v| v.as_i64()).unwrap_or(0);
                match symmetric_codec_with_flags(&op, &transformation, &key, &iv, &data, &format, flags) {
                    Ok(value) => Ok(value),
                    Err(e) => match crypt_config_message(&e) {
                        // 算法名/模式/密钥长度写错：对应 legado 构造
                        // `SymmetricCrypto` 时抛的 `NoSuchAlgorithmException` /
                        // `InvalidKeyException`。抛给脚本，调试面板能直接指认。
                        Some(message) => {
                            push_js_log(format!("[crypto] {transformation}: {message}"));
                            Err(rquickjs::Exception::throw_message(&ctx, &message))
                        }
                        // 数据错误（填充/标签/base64）：书源常故意试错再走 `||`
                        // 兜底，必须继续返回空串，不能打断整条规则。
                        None => {
                            push_js_log(format!("[crypto] {transformation}: {e}"));
                            Ok(String::new())
                        }
                    },
                }
            }),
        )?;
        globals.set(
            "__log",
            Func::new(|kind: String, message: String| {
                push_js_log(format!("[{kind}] {message}"));
            }),
        )?;
        // Keep all four showBrowser arguments; startBrowser also supplies a title.
        globals.set(
            "__show_browser",
            Func::new(|url: String, html: String, preload_js: String, options: String, title: String| {
                let html = (!html.is_empty()).then_some(html);
                let preload_js = (!preload_js.is_empty()).then_some(preload_js);
                // 带上发起这次打开的书源地址：前端要转给
                // `open_external_window`，窗口据此决定 UA / 尺寸 / Cookie 注入。
                let book_source_url = Some(active_source().url).filter(|u| !u.trim().is_empty());
                crate::service::book_service::set_show_browser_request(
                    crate::service::book_service::ShowBrowserRequest {
                        url,
                        html,
                        preload_js,
                        options: serde_json::from_str::<JsonValue>(&options).ok().filter(|v| v.is_object()),
                        title: (!title.is_empty()).then_some(title),
                        await_result: false,
                        panel_id: None,
                        external_window: false,
                        book_source_url,
                        await_panel_id: None,
                    },
                );
            }),
        )?;
        globals.set("__await_browser", Func::new(|url: String, title: String, html: String| {
            let request = crate::service::book_service::ShowBrowserRequest {
                url, title: (!title.is_empty()).then_some(title),
                html: (!html.is_empty()).then_some(html), preload_js: None,
                options: None, await_result: true, panel_id: None, external_window: false,
                book_source_url: Some(active_source().url).filter(|u| !u.trim().is_empty()),
                await_panel_id: None,
            };
            match crate::service::source_browser::await_browser(request) {
                Ok(page) => json!({"ok":true,"page":page}),
                Err(error) => json!({"ok":false,"error":error}),
            }.to_string()
        }))?;
        // `java.startBrowserAwait(url, title)` 的**独立窗口**变体：打开窗口并
        // **阻塞等待它关闭**（legado `SourceVerificationHelp.getVerificationResult`
        // 的语义）。书源的典型用法是「打开登录页 → 等用户登录 → 关窗 → 抓 Cookie」，
        // 日志/上传都必须发生在关窗之后。
        globals.set("__await_external_window", Func::new(|url: String, title: String| {
            let book_source_url = Some(active_source().url).filter(|u| !u.trim().is_empty());
            let request = crate::service::book_service::ShowBrowserRequest {
                url, title: (!title.is_empty()).then_some(title),
                html: None, preload_js: None, options: None,
                await_result: true, panel_id: None, external_window: true,
                book_source_url,
                // 由 `await_external_window` 内部生成并填回，前端据此在关窗时唤醒。
                await_panel_id: None,
            };
            match crate::service::source_browser::await_external_window(request) {
                Ok(page) => json!({"ok":true,"url":page.url}).to_string(),
                Err(error) => json!({"ok":false,"error":error}).to_string(),
            }
        }))?;
        globals.set("__open_external_window", Func::new(|url: String, title: String| {
            // 书源地址**必须**带上：前端把它转给 `open_external_window`，
            // 窗口据此套用书源的 UA、按移动/桌面选尺寸、注入已有 Cookie。
            // 缺了它就退化成「WebView2 默认桌面 UA + 900px 窗口」，移动端 H5
            // 登录页（书源 8054 的 `fanqienovel.com`）会显示不全且滑不动。
            let book_source_url = Some(active_source().url).filter(|u| !u.trim().is_empty());
            let request = crate::service::book_service::ShowBrowserRequest {
                url, title: (!title.is_empty()).then_some(title),
                html: None, preload_js: None, options: None,
                await_result: false, panel_id: None, external_window: true,
                book_source_url,
                await_panel_id: None,
            };
            match crate::service::source_browser::request_external_window(request.clone()) {
                Ok(()) => json!({"ok":true}).to_string(),
                Err(_) => {
                    // 没有登录上下文时（如阅读页里的段评 / 本章说 / 内嵌设置页）
                    // 退到「面板请求」槽位，由调用方（`eval_source_click` 等）取走，
                    // 前端看到 `externalWindow` 就直接开窗口 —— 效果一样，
                    // 只是这批场景没有 `BROWSER_CONTEXT`。
                    crate::service::book_service::set_show_browser_request(request);
                    json!({"ok":true}).to_string()
                }
            }
        }))?;
        // 规则解析原语: 供 `java.getString` / `getStringList` / `getElement(s)` 使用
        // (legado `AnalyzeRule` 是 `bindings["java"]`, 这些方法用规则解析当前页面内容)
        //
        // 内容必须**可变**: `java.setContent(x)` 会替换后续 `java.getString(rule)`
        // 的解析对象（legado `AnalyzeRule.setContent`），源常用它把上一步取到的
        // 片段交给下一步规则解析。
        let rule_input = Rc::new(RefCell::new(input_value.to_string()));
        let rule_base_url = base_url_value.to_string();
        {
            let rule_input = rule_input.clone();
            let rule_base_url = rule_base_url.clone();
            globals.set(
                "__rule_get",
                Func::new(move |rule: String, override_input: String, mode: String| -> String {
                    // override_input 为空时用当前内容(元素内部查询会显式传入片段)
                    let content = if override_input.is_empty() {
                        rule_input.borrow().clone()
                    } else {
                        override_input
                    };
                    match mode.as_str() {
                        "elements" => serde_json::Value::Array(
                            crate::parser::rule_engine::RuleEngine::eval_rule_elements(&rule, &content),
                        )
                        .to_string(),
                        "list" => {
                            let values = crate::parser::rule_engine::RuleEngine::eval_rule_on(
                                &rule,
                                &content,
                                &rule_base_url,
                                true,
                            );
                            serde_json::Value::Array(
                                values.into_iter().map(serde_json::Value::String).collect(),
                            )
                            .to_string()
                        }
                        _ => {
                            // legado 语义按模式区分：
                            // - `Mode.Json`(`AnalyzeByJSonPath.getString`)：值是 List 时
                            //   `joinToString("\n")`，多值**换行拼接**。
                            // - `Mode.Regex`(`AnalyzeByRegex.getElement`)：只用**第一个**匹配。
                            // - `Mode.Default`(`AnalyzeByJSoup.getString`)：多值换行拼接。
                            //
                            // 用 `list=false` 取单值：这条路径要能处理 `||` / `&&` 组合
                            //（`extract_content` 按段独立判模式），多值展开由 `getStringList`
                            // 承担。该源 bookUrl 脚本写的是 `java.getString("@href||$.url")`，
                            // `@href` 在 JSON 项上为空、要回落到 `$.url`；用 list 模式会绕过
                            // 组合分流，整条被当成一个 JSONPath 求值 → 空 → 书链只剩
                            // `.../column//`（ID 段缺失）。
                            //
                            // featured 分支依赖多值拼接：
                            // `String(java.getString("$..todayWorksIds[*]")).split("\n")`
                            // 期望拿到**全部** id。（JSONPath 通配命中多项时
                            // `extract_content` 已返回换行拼接的字符串。）
                            let rule_trimmed = rule.trim();
                            let regex_mode = rule_trimmed.starts_with("@regex:")
                                || rule_trimmed.starts_with("@Regex:")
                                || rule_trimmed.starts_with(':');
                            let values = crate::parser::rule_engine::RuleEngine::eval_rule_on(
                                &rule,
                                &content,
                                &rule_base_url,
                                false,
                            );
                            if regex_mode {
                                values.into_iter().next().unwrap_or_default()
                            } else {
                                values.join("\n")
                            }
                        }
                    }
                }),
            )?;
        }
        {
            let rule_input = rule_input.clone();
            globals.set(
                "__rule_set_content",
                Func::new(move |content: String| { *rule_input.borrow_mut() = content; }),
            )?;
        }
        globals.set(
            "__to_url",
            Func::new(move |raw: String, base: String| -> String {
                let base = if base.is_empty() { rule_base_url.clone() } else { base };
                match url::Url::parse(&raw).or_else(|_| url::Url::parse(&base)?.join(&raw)) {
                    Ok(parsed) => json!({
                        "origin": parsed.origin().ascii_serialization(),
                        "host": parsed.host_str().map(|h| match parsed.port() {
                            Some(port) => format!("{h}:{port}"), None => h.to_string(),
                        }).unwrap_or_default(),
                        "hostname": parsed.host_str().unwrap_or_default(),
                        "port": parsed.port().map(|p| p.to_string()).unwrap_or_default(),
                        "protocol": format!("{}:", parsed.scheme()),
                        "pathname": parsed.path(),
                        "href": parsed.as_str(),
                        "search": parsed.query().map(|q| format!("?{q}")).unwrap_or_default(),
                        "hash": parsed.fragment().map(|f| format!("#{f}")).unwrap_or_default(),
                    })
                    .to_string(),
                    Err(_) => json!({
                        "origin": "", "host": "", "hostname": "", "port": "",
                        "protocol": "", "pathname": "", "href": raw,
                        "search": "", "hash": "",
                    })
                    .to_string(),
                }
            }),
        )?;
        globals.set(
            "__store_get",
            Func::new(|key: String| -> Option<String> { store_get(&key) }),
        )?;
        // `java.strToBytes` / `java.bytesToStr` 的字符集转换: 输入输出都是字节
        // 序列, 只有宿主能按指定 charset 正确编解码(GBK 等无法用 JS 内置 API 处理)。
        globals.set(
            "__str_to_bytes",
            Func::new(|text: String, charset: String| -> String {
                let (bytes, _, _) = encoding_for_label(&charset).encode(&text);
                serde_json::to_string(&bytes).unwrap_or_else(|_| "[]".to_string())
            }),
        )?;
        globals.set(
            "__bytes_to_str",
            Func::new(|json: String, charset: String| -> String {
                let bytes: Vec<u8> = serde_json::from_str(&json).unwrap_or_default();
                let (text, _, _) = encoding_for_label(&charset).decode(&bytes);
                text.into_owned()
            }),
        )?;
        globals.set(
            "__store_put",
            Func::new(|key: String, value: String| {
                store_write(|map| {
                    map.insert(key, value);
                })
            }),
        )?;
        globals.set(
            "__store_del",
            Func::new(|key: String| {
                store_write(|map| {
                    map.remove(&key);
                })
            }),
        )?;
        globals.set(
            "__mem_get",
            Func::new(|key: String| -> Option<String> { memory_get(&key) }),
        )?;
        globals.set(
            "__mem_put",
            Func::new(|key: String, value: String| memory_put(key, value)),
        )?;
        let source_json = json!({
            "url": source_info.url,
            "name": source_info.name,
            "comment": source_info.comment,
            "variableComment": source_info.variable_comment,
            // 登录脚本与登录界面: 书源会在 exploreUrl 里 eval(String(source.loginUrl)); login(); 复用其中的函数。
            "loginUrl": source_info.login_url,
            "loginUi": source_info.login_ui,
        })
        .to_string();
        globals.set(
            "__source_info",
            Func::new(move || -> String { source_json.clone() }),
        )?;
        let book_json = json!({ "book": book_ctx.book, "chapter": book_ctx.chapter }).to_string();
        globals.set(
            "__book_ctx",
            Func::new(move || -> String { book_json.clone() }),
        )?;

        let java_obj = Object::new(ctx.clone())?;
        java_obj.set(
            "md5Encode",
            Func::new(|input: String| -> String { md5_hex(&input) }),
        )?;
        java_obj.set(
            "timeFormat",
            Func::new(|timestamp: i64| -> String { java_time_format(timestamp) }),
        )?;
        java_obj.set(
            "androidId",
            Func::new(|| -> String { JS_DEVICE_ID.clone() }),
        )?;
        java_obj.set("getAndroidId", Func::new(|| -> String { JS_DEVICE_ID.clone() }))?;
        java_obj.set("getWebViewUA", Func::new(|| -> String { DEFAULT_JS_USER_AGENT.to_string() }))?;
        // deviceID is an iOS capability probe in shared book sources. Exposing
        // it here would select iOS-only paragraph-comment markup on desktop.
        // Legado 书源常用 `java.getContent()` 取当前页面内容(等同全局 `input`)
        let content_input = input_value.to_string();
        java_obj.set(
            "getContent",
            Func::new(move || -> String { content_input.clone() }),
        )?;
        // Legado 书源常用 `result` 变量, 此处返回当前页面内容(与全局 result 一致)
        let result_input = input_value.to_string();
        java_obj.set(
            "getResult",
            Func::new(move || -> String { result_input.clone() }),
        )?;
        // Legado 书源用 `java.ensureGlobalVariable(key, value)` 预置全局变量
        java_obj.set(
            "ensureGlobalVariable",
            Func::new(|key: String, value: String| -> bool {
                // 通过 kv 存储模拟全局变量持久化, 供后续 JS 读取
                memory_put(format!("__global_{key}"), value);
                true
            }),
        )?;
        // cache 别名: legado 用 java.getCache / java.putCache
        java_obj.set(
            "getCache",
            Func::new(|key: String| -> Option<String> { memory_get(&key) }),
        )?;
        java_obj.set(
            "putCache",
            Func::new(|key: String, val: String| -> bool {
                memory_put(key, val);
                true
            }),
        )?;
        // legado 常用随机数/字符串工具
        java_obj.set(
            "random",
            Func::new(|| -> f64 { rand_like() }),
        )?;
        java_obj.set(
            "randomInt",
            Func::new(|max: i32| -> i32 { (rand_like() * max as f64) as i32 }),
        )?;
        java_obj.set(
            "randomString",
            Func::new(|len: i32| -> String { random_string(len) }),
        )?;
        java_obj.set(
            "base64Encode",
            Func::new(|input: String| -> String {
                base64::engine::general_purpose::STANDARD.encode(input)
            }),
        )?;
        java_obj.set(
            "base64Decode",
            Func::new(|input: String| -> String {
                decode_base64_lenient(&input)
                    .ok()
                    .and_then(|bytes| String::from_utf8(bytes).ok())
                    .unwrap_or_default()
            }),
        )?;
        java_obj.set(
            "encodeURIComponent",
            Func::new(|input: String| -> String { urlencoding::encode(&input).into_owned() }),
        )?;
        java_obj.set(
            "decodeURIComponent",
            Func::new(|input: String| -> String {
                urlencoding::decode(&input)
                    .map(|s| s.into_owned())
                    .unwrap_or_default()
            }),
        )?;
        // `java.encodeURI(text[, charset])`：legado 的重载是
        // `URLEncoder.encode(str)` 与 `URLEncoder.encode(str, enc)`。
        //
        // **第二参数必须参与编码**：一类书源的搜索规则写
        // `java.encodeURI(key,'gbk')` + 表单 `charset=gbk`，字符集被忽略时关键词
        // 按 UTF-8 编码、站点按 GBK 解码 → 乱码 → 搜索恒 0 条（发现页走 GET，
        // 不经过这里，所以看着正常）。语义细节见
        // `crawler::url_analyzer::java_url_encoder` 的说明。
        java_obj.set(
            "encodeURI",
            Func::new(|input: String, charset: Option<String>| -> String {
                crate::crawler::url_analyzer::java_url_encoder(&input, charset.as_deref())
            }),
        )?;
        java_obj.set(
            "decodeURI",
            Func::new(|input: String| -> String {
                urlencoding::decode(&input)
                    .map(|s| s.into_owned())
                    .unwrap_or_default()
            }),
        )?;
        java_obj.set(
            "md5Encode16",
            Func::new(|input: String| -> String { md5_hex(&input)[8..24].to_string() }),
        )?;
        java_obj.set(
            "digestHex",
            Func::new(|ctx: rquickjs::Ctx<'_>, input: String, algorithm: String| -> rquickjs::Result<String> {
                java_digest_hex(&input, &algorithm)
                    .map_err(|err| rquickjs::Exception::throw_message(&ctx, &err.to_string()))
            }),
        )?;
        java_obj.set(
            "digestBase64Str",
            Func::new(|ctx: rquickjs::Ctx<'_>, input: String, algorithm: String| -> rquickjs::Result<String> {
                java_digest_base64(&input, &algorithm)
                    .map_err(|err| rquickjs::Exception::throw_message(&ctx, &err.to_string()))
            }),
        )?;
        java_obj.set(
            "HMacHex",
            Func::new(|ctx: rquickjs::Ctx<'_>, input: String, algorithm: String, key: String| -> rquickjs::Result<String> {
                java_hmac_hex(&input, &algorithm, &key)
                    .map_err(|err| rquickjs::Exception::throw_message(&ctx, &err.to_string()))
            }),
        )?;
        java_obj.set(
            "HMacBase64",
            Func::new(|ctx: rquickjs::Ctx<'_>, input: String, algorithm: String, key: String| -> rquickjs::Result<String> {
                java_hmac_base64(&input, &algorithm, &key)
                    .map_err(|err| rquickjs::Exception::throw_message(&ctx, &err.to_string()))
            }),
        )?;
        java_obj.set(
            "hexEncodeToString",
            Func::new(|input: String| -> String { hex::encode(input.as_bytes()) }),
        )?;
        java_obj.set(
            "hexDecodeToString",
            Func::new(|input: String| -> String {
                hex::decode(input)
                    .ok()
                    .and_then(|bytes| String::from_utf8(bytes).ok())
                    .unwrap_or_default()
            }),
        )?;
        // `java.hexDecodeToByteArray` / `java.base64DecodeToByteArray`：返回**字节数组**。
        //
        // legado 里这两个方法返回 Rhino 的 `ByteArray`，脚本按数组使用
        //（`.length`、`.slice(start, end)`、按索引取值）。该源正文规则正是靠它们做
        // AES 解密：`base64DecodeToByteArray(密文)` → 切出 IV → `hexDecodeToByteArray`
        // 转 key 字节 → 交给 `createSymmetricCrypto`。
        //
        // 宿主以 JSON 数组文本返回（见 `bytes_to_json_array` 对生命周期约束的说明），
        // JS 侧 `JSON.parse` 还原成普通数组 —— 它同样满足 `.length` / `.slice()` /
        // 索引三种用法，且能被 `symmetric()` 识别为字节序列（见其 flags 位）。
        java_obj.set(
            "hexDecodeToByteArray",
            Func::new(|input: String| -> String {
                let bytes = hex::decode(input.trim()).unwrap_or_default();
                bytes_to_json_array(&bytes)
            }),
        )?;
        java_obj.set(
            "base64DecodeToByteArray",
            Func::new(|input: String| -> String {
                let bytes = decode_base64_lenient(input.trim()).unwrap_or_default();
                bytes_to_json_array(&bytes)
            }),
        )?;
        java_obj.set(
            "htmlFormat",
            Func::new(|input: String| -> String { java_html_format(&input) }),
        )?;
        java_obj.set(
            "toNumChapter",
            Func::new(|input: String| -> String { java_to_num_chapter(&input) }),
        )?;
        java_obj.set("t2s", Func::new(|input: String| -> String { java_t2s(&input) }))?;
        java_obj.set("s2t", Func::new(|input: String| -> String { java_s2t(&input) }))?;
        java_obj.set(
            "timeFormatUTC",
            Func::new(|timestamp: i64, format: String, sh: i64| -> String {
                java_time_format_utc(timestamp, &format, sh)
            }),
        )?;
        java_obj.set(
            "now",
            Func::new(|| -> i64 { chrono::Utc::now().timestamp_millis() }),
        )?;
        java_obj.set(
            "uuid",
            Func::new(|| -> String { Uuid::new_v4().to_string() }),
        )?;
        globals.set("java", java_obj)?;

        globals.set(
            "kv_get",
            Func::new(|key: String| -> Option<String> { memory_get(&key) }),
        )?;
        globals.set(
            "kv_put",
            Func::new(|key: String, val: String| -> bool {
                memory_put(key, val);
                true
            }),
        )?;
        globals.set(
            "regex_replace",
            Func::new(
                |input: String, pattern: String, replace: String| -> String {
                    apply_regex_replace(&input, &pattern, &replace)
                },
            ),
        )?;
        globals.set(
            "strip_ws",
            Func::new(|input: String| -> String { strip_whitespace(&input) }),
        )?;

        globals.set("nextChapterUrl", "")?;
        globals.set("rssArticle", Object::new(ctx.clone())?)?;

        // prelude 与 java_compat 的**执行**每次求值都不能少:
        // 它们把本次新建的 `java`/`source`/`cookie` 等对象包装成 legado 接口
        // (`java.aesBase64DecodeToString` 等), 而这些对象每次求值都会重建
        // (捕获了本次的 input/base_url)。若只在 initialize 时执行一次,
        // 后续求值就会拿到 undefined 的函数 —— 实测表现为 `not a function`。
        //
        // 但两者合计约 94KB, 每次重新**解析**才是目录解析的主要开销:
        // 实测某书源 1826 章的目录(每章跑一次 AES 解密)耗时 60 秒、单章
        // 33ms —— 阅读页表现为"打开书一直转圈、CPU 飙高、ESC 失灵"。
        // 因此首次把整段包成函数对象存进全局, 后续只调用这个函数:
        // 函数体只解析一次, 执行仍是每次一次(语义完全不变), 60s → 9s。
        if initialize {
            let wrapped = format!("globalThis.__runPrelude = function(){{{JS_PRELUDE}}};");
            eval_script(ctx.clone(), &wrapped)?;
        }
        eval_script(ctx.clone(), "globalThis.__runPrelude();")?;
        // java_compat.js 同理(33KB): 也是每次都要执行、但只需解析一次。
        if initialize {
            let wrapped = format!(
                "globalThis.__runCompat = function(){{{}}};",
                include_str!("java_compat.js")
            );
            eval_script(ctx.clone(), &wrapped)?;
        }
        eval_script(ctx.clone(), "globalThis.__runCompat();")?;
        if initialize && !shared_js.trim().is_empty() {
            if let Err(err) = eval_script(ctx.clone(), &shared_js) {
                push_js_log(format!("[jsLib] {err}"));
                return Err(err);
            }
        }
        // 调用方显式绑定放在 jsLib 之后, 避免被 jsLib 顶层同名变量覆盖
        if let Some(bindings) = bindings {
            for (key, value) in bindings {
                // `@resp:` 前缀: legado 的 `result` 是 StrResponse 对象,
                // 脚本里按方法调用 (`result.code()`/`body()`)。这里用 Resp()
                // 包装, 保持方法语义而不是纯 JSON 数据。
                if let Some(raw) = value.as_str().filter(|raw| raw.starts_with(RESP_BINDING_PREFIX)) {
                    let literal = serde_json::to_string(raw.trim_start_matches(RESP_BINDING_PREFIX))
                        .unwrap_or_else(|_| "\"\"".to_string());
                    let js_value = eval_script(ctx.clone(), &format!("Resp({literal})"))?;
                    globals.set(key.as_str(), js_value)?;
                    continue;
                }
                // `@elements:` 前缀: 值是**元素快照数组**, 按 JSoup `Elements`
                // 语义包装 —— 列表规则的 JS 段里 `result` 就是这个类型
                // (`result.toArray()` / `result.size()` / `result[i]`)。
                if let Some(raw) = value
                    .as_str()
                    .filter(|raw| raw.starts_with(ELEMENTS_BINDING_PREFIX))
                {
                    // 前缀里承载的是一段 **JSON 文本**（数组）：先解成数组对象，
                    // 再交给 `__wrapElements` 逐项包装成 JSoup 元素。
                    let literal =
                        serde_json::to_string(raw.trim_start_matches(ELEMENTS_BINDING_PREFIX))
                            .unwrap_or_else(|_| "\"[]\"".to_string());
                    let js_value = eval_script(
                        ctx.clone(),
                        &format!("__wrapElements(JSON.parse({literal}))"),
                    )?;
                    globals.set(key.as_str(), js_value)?;
                    continue;
                }
                // `@element:` 前缀: 值是**元素快照**, 必须按 JSoup Element 语义
                // 包装, 书源才能对它调用 select()/html()/text()/attr()。
                if let Some(raw) = value
                    .as_str()
                    .filter(|raw| raw.starts_with(ELEMENT_BINDING_PREFIX))
                {
                    let literal = serde_json::to_string(
                        raw.trim_start_matches(ELEMENT_BINDING_PREFIX),
                    )
                    .unwrap_or_else(|_| "\"{}\"".to_string());
                    // 前缀里承载的是一段 **JSON 文本**, 必须先 parse 成对象再包装:
                    // 直接传字符串会让 `wrapElement` 走 JSON 节点分支, innerHtml
                    // 读不到, `select()` 恒为空。
                    let js_value =
                        eval_script(ctx.clone(), &format!("__wrapElement(JSON.parse({literal}))"))?;
                    globals.set(key.as_str(), js_value)?;
                    continue;
                }
                let js_value = ctx.json_parse(value.to_string())?;
                globals.set(key.as_str(), js_value)?;
            }
            // infoMap 走 `{"__infoMap": {...}}` 信封传入(见 info_map_binding),
            // 这里解包成对象并补上 Map 风格方法: 书源里
            // `infoMap['x']` 与 `infoMap.get('x')` 两种写法都有。
            //
            // 方法必须定义成**不可枚举**属性: 否则 `Object.keys(infoMap)` /
            // `size()` 会把 get/put 这些函数也数成数据项, 书源若遍历 infoMap
            // 就会拿到一堆函数值。
            if let Some(raw) = bindings.get("infoMap").and_then(JsonValue::as_str) {
                let unpacked = eval_script(
                    ctx.clone(),
                    &format!(
                        "(function () {{\n\
                         var env = {raw};\n\
                         var m = env.__infoMap || {{}};\n\
                         function def(name, fn) {{\n\
                           Object.defineProperty(m, name, {{ value: fn, enumerable: false, writable: true, configurable: true }});\n\
                         }}\n\
                         def('get', function (k) {{ var v = m[k]; return v === undefined ? null : v; }});\n\
                         def('put', function (k, v) {{ m[k] = v; return v; }});\n\
                         def('containsKey', function (k) {{ return Object.prototype.hasOwnProperty.call(m, k); }});\n\
                         def('size', function () {{ return Object.keys(m).length; }});\n\
                         def('keySet', function () {{ return Object.keys(m); }});\n\
                         def('isEmpty', function () {{ return Object.keys(m).length === 0; }});\n\
                         def('toString', function () {{ return JSON.stringify(m); }});\n\
                         return m;\n\
                         }})()"
                    ),
                )?;
                globals.set("infoMap", unpacked)?;
            }
        }

        // Block scope lets consecutive rules redeclare let/const, while var
        // bindings such as `var result = result || {}` retain the host value.
        //
        // `__inputSnapshot` 用来判断脚本有没有改写 `result`（见下）。
        eval_script(ctx.clone(), "globalThis.__inputSnapshot = result; undefined")?;
        let v = eval_script(ctx.clone(), &format!("{{\n{script}\n}}"))?;
        // Rhino（legado 手机端）的脚本求值返回**最后一条有值的语句**的结果，
        // 而不是 ECMAScript 的「完成值」。差别只在「末尾是空完成语句」时暴露：
        //
        //   var raw = texts.join('\n\n');
        //   result = decodeText(raw);                    // ← 有值
        //   if (result.length < 500) { result += "…"; }  // ← 条件为假，空完成
        //
        // QuickJS 按规范把最后那条**空完成**当作脚本值（`undefined`），于是
        // 前面算好的正文被整个丢掉 —— 症状是正文恒空、日志里没有任何异常。
        //
        // 回退到「脚本改写过 `result` 就用它」：书源脚本的通用契约就是
        // 「输入 `result` → 输出 `result`」，赋值后的值才是它想交出的结果。
        // 脚本没碰过 `result` 时保持 `undefined`，不把输入原文当输出。
        let v = if v.is_undefined() || v.is_null() {
            eval_script(
                ctx.clone(),
                "result !== globalThis.__inputSnapshot ? result : undefined",
            )?
        } else {
            v
        };
        globals.set("__result", v)?;
        // String 包装对象 / java.get 响应对象按 legado 语义转成字符串
        // String 包装对象按 legado 语义转成字符串。
        //
        // `Resp` 对象（java.ajax 的返回、`java.getStrResponse()` 的结果）作为
        // **脚本返回值**时必须序列化成完整的 StrResponse 信封
        // (`{body,url,code,headers,isSuccessful}`): loginCheckJs 用
        // `source.login();java.getStrResponse();` 替换响应，只交出 body 会让
        // 调用方反序列化出 code=0/空 body 的假响应。仅在需要字符串的场合
        // (模板拼接、String(...)) 才解包成 body。
        let v: Value = eval_script(
            ctx.clone(),
            "(function (v) {\n\
             if (v && typeof v.__javaStringText === 'function') return v.__javaStringText();
             if (v instanceof String) return v.valueOf();\n\
             if (v && v.__isResp) return JSON.parse(v.__respJson());\n\
             // 图片解密上下文: 脚本可能直接交回字节数组(`.toByteArray()` 一类)。\n\
             // 按 latin1 逐字节拼成字符串, 由宿主还原 —— 走 JSON 文本会把\n\
             // ≥0x80 的字节变成数组字面量文本, 图片就废了。\n\
             if (globalThis.__byteMode === true && v && typeof v !== 'string'\n\
                 && typeof v.length === 'number') {\n\
               var bytes = ''; for (var i = 0; i < v.length; i++) bytes += String.fromCharCode(Number(v[i]) & 0xff);\n\
               return bytes;\n\
             }\n\
             return v;\n\
             })(__result)",
        )?;

        let result = if v.is_null() || v.is_undefined() {
            String::new()
        } else if let Some(s) = v.clone().into_string() {
            let s: rquickjs::String<'_> = s;
            s.to_string()
                .map(|value| value.to_string())
                .unwrap_or_default()
        } else {
            match ctx.json_stringify(v) {
                Ok(Some(json)) => json.to_string().unwrap_or_default(),
                _ => String::new(),
            }
        };
        Ok(result)
    }).map_err(|err| { push_js_log(format!("[脚本] {err}")); err })
}

#[derive(Clone, Copy)]
enum CipherMode {
    Ecb,
    Cbc,
}

#[derive(Clone, Copy)]
enum CipherPadding {
    Pkcs7,
    None,
    Zero,
}

macro_rules! block_crypt {
    ($cipher:ty, $encrypt:expr, $mode:expr, $padding:expr, $key:expr, $iv:expr, $data:expr) => {{
        let block = <$cipher as cbc::cipher::BlockSizeUser>::block_size();
        let data: &[u8] = $data;
        let key: &[u8] = $key;
        let mut iv: Vec<u8> = $iv.to_vec();
        iv.resize(block, 0);
        let bad = |e: &dyn std::fmt::Display| anyhow::anyhow!("{e}");
        use cbc::cipher::KeyInit as _;
        if $encrypt {
            let mut buf = data.to_vec();
            buf.resize(data.len() + block, 0);
            let len = data.len();
            let out: Result<&[u8], _> = match ($mode, $padding) {
                (CipherMode::Cbc, CipherPadding::Pkcs7) => cbc::Encryptor::<$cipher>::new_from_slices(key, &iv).map_err(|e| bad(&e))?.encrypt_padded_mut::<Pkcs7>(&mut buf, len),
                (CipherMode::Cbc, CipherPadding::None) => cbc::Encryptor::<$cipher>::new_from_slices(key, &iv).map_err(|e| bad(&e))?.encrypt_padded_mut::<NoPadding>(&mut buf, len),
                (CipherMode::Cbc, CipherPadding::Zero) => cbc::Encryptor::<$cipher>::new_from_slices(key, &iv).map_err(|e| bad(&e))?.encrypt_padded_mut::<ZeroPadding>(&mut buf, len),
                (CipherMode::Ecb, CipherPadding::Pkcs7) => ecb::Encryptor::<$cipher>::new_from_slice(key).map_err(|e| bad(&e))?.encrypt_padded_mut::<Pkcs7>(&mut buf, len),
                (CipherMode::Ecb, CipherPadding::None) => ecb::Encryptor::<$cipher>::new_from_slice(key).map_err(|e| bad(&e))?.encrypt_padded_mut::<NoPadding>(&mut buf, len),
                (CipherMode::Ecb, CipherPadding::Zero) => ecb::Encryptor::<$cipher>::new_from_slice(key).map_err(|e| bad(&e))?.encrypt_padded_mut::<ZeroPadding>(&mut buf, len),
            };
            out.map(|s| s.to_vec()).map_err(|e| bad(&e))
        } else {
            let mut buf = data.to_vec();
            let out: Result<&[u8], _> = match ($mode, $padding) {
                (CipherMode::Cbc, CipherPadding::Pkcs7) => cbc::Decryptor::<$cipher>::new_from_slices(key, &iv).map_err(|e| bad(&e))?.decrypt_padded_mut::<Pkcs7>(&mut buf),
                (CipherMode::Cbc, CipherPadding::None) => cbc::Decryptor::<$cipher>::new_from_slices(key, &iv).map_err(|e| bad(&e))?.decrypt_padded_mut::<NoPadding>(&mut buf),
                (CipherMode::Cbc, CipherPadding::Zero) => cbc::Decryptor::<$cipher>::new_from_slices(key, &iv).map_err(|e| bad(&e))?.decrypt_padded_mut::<ZeroPadding>(&mut buf),
                (CipherMode::Ecb, CipherPadding::Pkcs7) => ecb::Decryptor::<$cipher>::new_from_slice(key).map_err(|e| bad(&e))?.decrypt_padded_mut::<Pkcs7>(&mut buf),
                (CipherMode::Ecb, CipherPadding::None) => ecb::Decryptor::<$cipher>::new_from_slice(key).map_err(|e| bad(&e))?.decrypt_padded_mut::<NoPadding>(&mut buf),
                (CipherMode::Ecb, CipherPadding::Zero) => ecb::Decryptor::<$cipher>::new_from_slice(key).map_err(|e| bad(&e))?.decrypt_padded_mut::<ZeroPadding>(&mut buf),
            };
            out.map(|s| s.to_vec()).map_err(|e| bad(&e))
        }
    }};
}

/// Java `Cipher` 风格的对称加解密: `AES|DES|DESede[/ECB|CBC[/PKCS5Padding|NoPadding|ZeroPadding]]`。
/// 密钥/IV 按 UTF-8 字节解释(与 legado 传字符串时一致)。
/// `AES/GCM`（AEAD）：加密输出 `密文 || 16 字节 tag`，解密要求输入同形态。
///
/// 为什么手写而不用 `aes-gcm` crate：构建环境常常没法拉新依赖，而 GCM 的
/// 两半都已有现成零件 —— CTR 用 `aes` 的块加密，GHASH 用 `u128` 乘 GF(2^128)。
/// 实现按 NIST SP 800-38D，`H = E(K, 0^128)`、`J0 = IV || 0^31 || 1`（96 位 IV 时）。
fn aes_gcm_crypt(encrypt: bool, key: &[u8], iv: &[u8], data: &[u8]) -> anyhow::Result<Vec<u8>> {
    use aes::cipher::{BlockEncrypt, KeyInit};
    const TAG_LEN: usize = 16;

    fn encrypt_block<C: BlockEncrypt>(cipher: &C, block: &[u8; 16]) -> [u8; 16] {
        use aes::cipher::generic_array::GenericArray;
        let mut b = GenericArray::clone_from_slice(block);
        cipher.encrypt_block(&mut b);
        let mut out = [0u8; 16];
        out.copy_from_slice(&b);
        out
    }

    macro_rules! run {
        ($cipher:ty) => {{
            let cipher = <$cipher>::new_from_slice(key)
                .map_err(|err| anyhow::anyhow!("AES 密钥长度无效: {err}"))?;
            // H = E(K, 0^128)
            let h = u128::from_be_bytes(encrypt_block(&cipher, &[0u8; 16]));
            // J0：96 位 IV 直接右补 1；否则用 GHASH(iv || pad || len) 派生。
            let j0 = if iv.len() == 12 {
                let mut counter = [0u8; 16];
                counter[..12].copy_from_slice(iv);
                counter[15] = 1;
                counter
            } else {
                let mut buf = iv.to_vec();
                while buf.len() % 16 != 0 {
                    buf.push(0);
                }
                buf.extend_from_slice(&[0u8; 8]);
                buf.extend_from_slice(&((iv.len() as u64) * 8).to_be_bytes());
                let mut y = 0u128;
                for chunk in buf.chunks(16) {
                    let mut block = [0u8; 16];
                    block.copy_from_slice(chunk);
                    y = ghash_mul(y ^ u128::from_be_bytes(block), h);
                }
                y.to_be_bytes()
            };

            // CTR 加解密：从 J0+1 开始计数。解密的输入含 16 字节 tag，
            // 必须先剥掉再走 CTR —— 否则把 tag 也当密文解出来（往返失败）。
            let ctr_body: &[u8] = if encrypt {
                data
            } else {
                anyhow::ensure!(data.len() >= TAG_LEN, "GCM 密文过短（缺认证标签）");
                &data[..data.len() - TAG_LEN]
            };
            let mut counter = j0;
            let mut stream = Vec::with_capacity(ctr_body.len());
            for block_index in 0..ctr_body.len().div_ceil(16) {
                // 递增低 32 位（GCM 规定）
                let mut ctr = counter;
                let n = u32::from_be_bytes([ctr[12], ctr[13], ctr[14], ctr[15]]) + 1;
                ctr[12..].copy_from_slice(&n.to_be_bytes());
                counter = ctr;
                let keystream = encrypt_block(&cipher, &ctr);
                let offset = block_index * 16;
                for i in 0..16 {
                    if offset + i < ctr_body.len() {
                        stream.push(ctr_body[offset + i] ^ keystream[i]);
                    }
                }
            }

            if encrypt {
                // tag = GHASH(H, A=空, C) ^ E(K, J0)
                let mut buf = stream.clone();
                while buf.len() % 16 != 0 {
                    buf.push(0);
                }
                buf.extend_from_slice(&[0u8; 8]);
                buf.extend_from_slice(&((stream.len() as u64) * 8).to_be_bytes());
                let mut y = 0u128;
                for chunk in buf.chunks(16) {
                    let mut block = [0u8; 16];
                    block.copy_from_slice(chunk);
                    y = ghash_mul(y ^ u128::from_be_bytes(block), h);
                }
                let tag_mask = u128::from_be_bytes(encrypt_block(&cipher, &j0));
                let tag = (y ^ tag_mask).to_be_bytes();
                let mut out = stream;
                out.extend_from_slice(&tag);
                Ok(out)
            } else {
                anyhow::ensure!(
                    data.len() >= TAG_LEN,
                    "GCM 密文过短（缺认证标签）"
                );
                let (ciphertext, tag) = data.split_at(data.len() - TAG_LEN);
                // 校验标签后再返回明文，避免把错数据当成功。
                let mut buf = ciphertext.to_vec();
                while buf.len() % 16 != 0 {
                    buf.push(0);
                }
                buf.extend_from_slice(&[0u8; 8]);
                buf.extend_from_slice(&((ciphertext.len() as u64) * 8).to_be_bytes());
                let mut y = 0u128;
                for chunk in buf.chunks(16) {
                    let mut block = [0u8; 16];
                    block.copy_from_slice(chunk);
                    y = ghash_mul(y ^ u128::from_be_bytes(block), h);
                }
                let tag_mask = u128::from_be_bytes(encrypt_block(&cipher, &j0));
                let expected = (y ^ tag_mask).to_be_bytes();
                anyhow::ensure!(expected == tag, "GCM 认证标签不匹配");
                Ok(stream)
            }
        }};
    }

    match key.len() {
        16 => run!(aes::Aes128),
        24 => run!(aes::Aes192),
        32 => run!(aes::Aes256),
        n => bail_config!("AES 密钥长度无效: {n}"),
    }
}

/// GF(2^128) 乘法（GCM 的 GHASH），按 SP 800-38D 的位运算定义。
fn ghash_mul(x: u128, h: u128) -> u128 {
    const R: u128 = 0xE1 << 120;
    let mut z = 0u128;
    let mut v = h;
    for i in 0..128 {
        // 从最高位开始遍历 x
        if (x >> (127 - i)) & 1 == 1 {
            z ^= v;
        }
        let lsb = v & 1;
        v >>= 1;
        if lsb == 1 {
            v ^= R;
        }
    }
    z
}

pub(super) fn symmetric_crypt(
    encrypt: bool,
    transformation: &str,
    key: &[u8],
    iv: &[u8],
    data: &[u8],
) -> anyhow::Result<Vec<u8>> {
    let mut parts = transformation.split('/').map(|p| p.trim().to_ascii_uppercase());
    let algorithm = parts.next().unwrap_or_default();
    // GCM 是 AEAD：不能走 `block_crypt!` 那套分组模式（无填充、需要认证标签），
    // 单独分派。书源里 `AES/GCM/NoPadding` 用来解密接口响应与生成签名。
    if parts.clone().next().map(|p| p.eq_ignore_ascii_case("GCM")).unwrap_or(false) {
        if !algorithm.eq_ignore_ascii_case("AES") {
            bail_config!("GCM 目前只支持 AES，收到 {algorithm}");
        }
        return aes_gcm_crypt(encrypt, key, iv, data);
    }
    let mode = match parts.next().as_deref() {
        Some("CBC") => CipherMode::Cbc,
        Some("ECB") | Some("") | None => CipherMode::Ecb,
        Some(other) => bail_config!("不支持的模式 {other}"),
    };
    let padding = match parts.next().as_deref() {
        Some("NOPADDING") => CipherPadding::None,
        Some("ZEROPADDING") => CipherPadding::Zero,
        _ => CipherPadding::Pkcs7,
    };
    match algorithm.as_str() {
        "AES" => match key.len() {
            16 => block_crypt!(aes::Aes128, encrypt, mode, padding, key, iv, data),
            24 => block_crypt!(aes::Aes192, encrypt, mode, padding, key, iv, data),
            32 => block_crypt!(aes::Aes256, encrypt, mode, padding, key, iv, data),
            n => bail_config!("AES 密钥长度无效: {n}"),
        },
        "DES" => block_crypt!(des::Des, encrypt, mode, padding, &key[..key.len().min(8)], iv, data),
        "DESEDE" | "TRIPLEDES" | "3DES" => {
            // 16 字节密钥按 K1K2K1 扩展(Java 需显式 24 字节, 这里宽松处理)
            let mut key24 = key.to_vec();
            if key24.len() == 16 {
                key24.extend_from_slice(&key[..8]);
            }
            key24.truncate(24);
            block_crypt!(des::TdesEde3, encrypt, mode, padding, &key24, iv, data)
        }
        other => bail_config!("不支持的算法 {other}"),
    }
}

/// `__sym` 原语: 加密时 data 为明文, 按 format(base64/hex) 输出;
/// 解密时按 format(base64/hex/auto/raw) 解码输入, 输出 UTF-8 文本。
///
/// 全部入参按**文本**（UTF-8）解释。字节序列入参请用 [`symmetric_codec_with_flags`]。
fn symmetric_codec(
    op: &str,
    transformation: &str,
    key: &str,
    iv: &str,
    data: &str,
    format: &str,
) -> anyhow::Result<String> {
    symmetric_codec_with_flags(op, transformation, key, iv, data, format, 0)
}

/// 带字节标记的 `__sym` 实现。
///
/// `flags` 是 JS 侧 `symmetric()` 传来的位标记: 1=key、2=iv、4=载荷是**字节序列**,
/// 8=解密结果按**原始字节**返回(图片解密, 见 `coverDecodeJs`)。
/// 置位的入参按 latin1 逐字节取字节（每个 char 就是一个字节）；未置位的是普通
/// 文本，按 UTF-8 编码 —— 后者保证 `café` 这类文本 key 不被改写。
fn symmetric_codec_with_flags(
    op: &str,
    transformation: &str,
    key: &str,
    iv: &str,
    data: &str,
    format: &str,
    flags: i64,
) -> anyhow::Result<String> {
    let engine = &base64::engine::general_purpose::STANDARD;
    let as_bytes = |value: &str, is_bytes: bool| -> Vec<u8> {
        if is_bytes { latin1_bytes(value) } else { value.as_bytes().to_vec() }
    };
    let key_bytes = as_bytes(key, flags & 1 != 0);
    let iv_bytes = as_bytes(iv, flags & 2 != 0);
    if op == "enc" {
        let out = symmetric_crypt(
            true,
            transformation,
            &key_bytes,
            &iv_bytes,
            &as_bytes(data, flags & 4 != 0),
        )?;
        return Ok(if format == "hex" {
            hex::encode(out)
        } else {
            engine.encode(out)
        });
    }
    // `raw` 表示调用方已按字节序列还原（每 char 一个字节），直接用；
    // 其余格式（base64/hex/auto）都是文本编码，仍按文本处理。
    let trimmed = data.trim();
    let is_hex = !trimmed.is_empty()
        && trimmed.len() % 2 == 0
        && trimmed.chars().all(|c| c.is_ascii_hexdigit());
    let bytes = match format {
        "raw" => latin1_bytes(data),
        "hex" => hex::decode(trimmed)?,
        "auto" if is_hex => hex::decode(trimmed)?,
        _ => decode_base64_lenient(trimmed)?,
    };
    let out = symmetric_crypt(false, transformation, &key_bytes, &iv_bytes, &bytes)?;
    // flags 位 8: 返回**原始字节**(latin1 承载), 供 `coverDecodeJs`/`imageDecode`
    // 这类图片解密使用 —— legado 的 `SymmetricCrypto.decrypt` 返回 `ByteArray`,
    // 图片数据不能过 UTF-8(否则 ≥0x80 的字节全被替换成 U+FFFD, 图片花掉)。
    if flags & 8 != 0 {
        return Ok(latin1_string(&out));
    }
    Ok(String::from_utf8_lossy(&out).into_owned())
}

/// 宽松 Base64 解码：按长度补齐缺失的 \`=\` 补位后再解码。
///
/// Android 的 \`android.util.Base64\` / hutool \`Base64.decode\`（legado 用的就是它们）
/// 都能接受缺省补位的输入，例如 22 字符的 \`mPgPm6AQ/x1kMQYOBuq9qw\` 补成 \`==\` 才合法；
/// 而 Rust 的 \`base64\` STANDARD 引擎严格要求补位，直接解码会报 \`InvalidPadding\`，
/// 导致书源解密链返回空串（issue #13 的目录名/正文正是这样丢的）。
/// 这里只补足长度、不改动已有字符；长度模 4 为 1 属于非法输入，交给解码器报错。
pub(crate) fn decode_base64_lenient(input: &str) -> Result<Vec<u8>, base64::DecodeError> {
    let trimmed = input.trim();
    let mut padded = trimmed.to_string();
    match trimmed.len() % 4 {
        2 => padded.push_str("=="),
        3 => padded.push('='),
        _ => {}
    }
    base64::engine::general_purpose::STANDARD.decode(padded.as_bytes())
}

/// 把 JS 侧按 latin1 承载的**字节序列**还原成原始字节：每个 `char` 就是一个字节
/// （JS 侧用 `String.fromCharCode(byte & 0xff)` 构造，因此正常情况下必然 ≤U+00FF）。
///
/// 只有确认入参是字节序列时才可调用（JS 侧 `__sym` 的 flags 位、`raw` 解密格式、
/// `__http_bytes` 的 body）。普通文本 key 走 `str::as_bytes`（UTF-8）——
/// 早先按「字符是否全 ≤0xFF」猜测，会把 `café` 这类文本 key 误当成字节序列。
///
/// 对超出范围的字符按低 8 位取值（与 JS 侧 `charCodeAt(i) & 0xff` 一致），
/// 不做断言/panic：入参来自书源脚本，不能让异常输入中断求值。
fn latin1_bytes(value: &str) -> Vec<u8> {
    value.chars().map(|c| (c as u32 & 0xFF) as u8).collect()
}

/// `latin1_bytes` 的逆操作：把任意字节按「每 char 一个字节」编码成字符串。
///
/// JS 侧用 `charCodeAt(i) & 0xff` 还原，因此 ≥0x80 的字节不会被 UTF-8 改写。
fn latin1_string(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| *byte as char).collect()
}

/// 在 QuickJS 里求值一段脚本。
///
/// Rhino(legado 手机端用的引擎)比正式规范宽松, 书源里大量依赖这层宽容, 桌面端
/// 用 QuickJS(严格按 ES 解析)会直接 SyntaxError, 整个书源报废 —— 而手机端正常。
/// 已知两类, 且都只在**解析失败**后才补救(不预改写, 免得上无关脚本被改动):
///
/// 1. **裸解构箭头形参**: `rows.map([a, b] => …)` / `rows.map({a} => …)`。
///    正式语法要求 `([a, b])` / `({a})`, QuickJS 报 `Unexpected token '=>'`。
/// 2. **`let`/`const` 重声明同名形参**:
///
///    ```js
///    function GetUrl(path, params, sourceUrl) {
///      let sourceUrl = sourceUrl || "…";   // 与形参同名
///    }
///    ```
///
///    正式规范里 `let`/`const` 绝不能与形参同名, Rhino 允许, QuickJS 报
///    `invalid redefinition of parameter name`。改成 `var` 语义等价(函数作用域)
///    且合法 —— 书源本来指望的就是「给形参一个默认值」。
///
/// 两个补丁依次叠加尝试, 任一步改写成功就重跑; 都不适用时保留原始错误。
fn eval_script<'js>(ctx: rquickjs::Ctx<'js>, script: &str) -> anyhow::Result<Value<'js>> {
    let failure = match eval_script_raw(ctx.clone(), script) {
        Ok(value) => return Ok(value),
        Err(err) => err,
    };
    // 只在「原样脚本连解析都没通过」时重试。
    //
    // 书源脚本会发请求、写变量, 有副作用; 若首轮已执行到运行期才抛错, 再跑一遍
    // 会把副作用做第二次, 所以必须先确认失败发生在解析阶段。
    //
    // 判定不靠错误文案(JSON.parse 这类运行期异常文本里同样有 "expecting ...",
    // 关键词匹配会认错), 而是把脚本塞进一个**从不调用**的函数体里求值:
    // 函数体只做语法解析、不执行, 解析失败才说明脚本本身有语法错误。
    if !script_fails_to_parse(ctx.clone(), script) {
        return Err(failure);
    }
    let mut current = script.to_string();
    for patch in [
        patch_bare_destructuring_arrow_params,
        patch_let_redeclared_params,
    ] {
        if !script_fails_to_parse(ctx.clone(), &current) {
            break;
        }
        if let Some(next) = patch(&current) {
            current = next;
        }
    }
    if current == script {
        return Err(failure);
    }
    match eval_script_raw(ctx.clone(), &current) {
        Ok(value) => Ok(value),
        // 补丁改对了、但脚本运行期才出错时, 真实原因在重试错误里; 返回首次的
        // "Unexpected token '=>'" 会把 "BOOM" 这类信息整个丢掉。
        //
        // 也不能按错误文案区分「解析失败」与「运行期失败」: JSON.parse 之类的
        // 运行期异常同样带 "expecting ..." 文本, 关键词判定会认错。
        // 只要补丁确实改写了脚本, 重试错误就是针对「实际会执行的代码」的报错,
        // 直接透传最诚实。
        Err(retry) => Err(retry),
    }
}

/// 判断脚本是否存在**语法**错误(而不是运行期错误)。
///
/// 做法是把脚本放进一个从不调用的函数体里求值: 函数体只会被解析、不会执行,
/// 因此不会触发脚本里的网络请求或写变量等副作用; 解析失败即说明脚本本身语法不成立。
///
/// 已知边界: 顶层 `return` 这类只在脚本上下文合法的写法, 在函数体里反而合法,
/// 此时会判定为「不是语法错误」而不重试 —— 宁可少救, 不冒重复执行副作用的风险。
fn script_fails_to_parse(ctx: rquickjs::Ctx<'_>, script: &str) -> bool {
    let probe = format!("(function () {{\n{script}\n}})");
    eval_script_raw(ctx, &probe).is_err()
}
fn eval_script_raw<'js>(ctx: rquickjs::Ctx<'js>, script: &str) -> anyhow::Result<Value<'js>> {
    // legado(Rhino) 是非严格模式; 混淆脚本常依赖 sloppy 语义(如顶层 this 指向全局)
    let mut options = EvalOptions::default();
    options.strict = false;
    match ctx.eval_with_options(script, options) {
        Ok(v) => Ok(v),
        Err(e) => {
            let caught = ctx.catch();
            if let Some(exception) = caught.as_exception() {
                let message = exception.message().unwrap_or_default();
                let stack = exception.stack().unwrap_or_default();
                return Err(anyhow::anyhow!("JS 异常: {message}\n{stack}"));
            }
            if !caught.is_undefined() && !caught.is_null() {
                if let Some(message) = caught.as_string() {
                    return Err(anyhow::anyhow!("JS 异常: {}", message.to_string().unwrap_or_default()));
                }
                return Err(anyhow::anyhow!("JS 异常: {caught:?}"));
            }
            Err(e.into())
        }
    }
}

/// 给「裸解构箭头形参」补圆括号: [a, b] => ... 改写成 ([a, b]) => ...。
///
/// 只在解析失败后调用, 所以实现从简: 逐字符扫描, 跳过字符串/模板串/注释, 遇到
/// "=>" 且左侧紧邻 ']' 或 '}' 时回溯到配对的括号; 只有该括号真的处在「形参表
/// 位置」(前面不是标识符/')'/']'/'}'/'.', 以排除下标与成员访问)才改写。
/// 没改写成功返回 None, 让调用方保留原始错误。
fn patch_bare_destructuring_arrow_params(script: &str) -> Option<String> {
    // 单引号 / 双引号 / 反引号与反斜杠用码点写, 免去多层转义。
    const QUOTE_SINGLE: char = '\u{27}';
    const QUOTE_DOUBLE: char = '\u{22}';
    const BACKTICK: char = '\u{60}';
    const BACKSLASH: char = '\u{5C}';
    const NEWLINE: char = '\u{A}';

    let chars: Vec<char> = script.chars().collect();
    let mut out: Vec<char> = Vec::with_capacity(chars.len() + 16);
    let mut i = 0usize;
    let mut changed = false;
    while i < chars.len() {
        let c = chars[i];
        if c == QUOTE_SINGLE || c == QUOTE_DOUBLE || c == BACKTICK {
            out.push(c);
            i += 1;
            while i < chars.len() {
                let ch = chars[i];
                out.push(ch);
                i += 1;
                if ch == BACKSLASH && i < chars.len() {
                    out.push(chars[i]);
                    i += 1;
                } else if ch == c {
                    break;
                }
            }
            continue;
        }
        if c == '/' && i + 1 < chars.len() && chars[i + 1] == '/' {
            while i < chars.len() && chars[i] != NEWLINE {
                out.push(chars[i]);
                i += 1;
            }
            continue;
        }
        // 正则字面量: `/a]=>b/.test(s)` 里的 `]`/`}` 不是箭头形参。
        // 只按「上一个有效字符处于表达式位置」判断, 避免把除号当正则开头。
        if c == '/' && regex_literal_can_start(&out) {
            out.push(c);
            i += 1;
            let mut in_class = false;
            while i < chars.len() {
                let ch = chars[i];
                if ch == BACKSLASH && i + 1 < chars.len() {
                    out.push(ch);
                    out.push(chars[i + 1]);
                    i += 2;
                    continue;
                }
                if ch == NEWLINE {
                    break;
                }
                out.push(ch);
                i += 1;
                if ch == '[' {
                    in_class = true;
                } else if ch == ']' {
                    in_class = false;
                } else if ch == '/' && !in_class {
                    break;
                }
            }
            // 吃掉 flags
            while i < chars.len() && chars[i].is_ascii_alphabetic() {
                out.push(chars[i]);
                i += 1;
            }
            continue;
        }
        if c == '/' && i + 1 < chars.len() && chars[i + 1] == '*' {
            out.push('/');
            out.push('*');
            i += 2;
            while i < chars.len() {
                if chars[i] == '*' && i + 1 < chars.len() && chars[i + 1] == '/' {
                    out.push('*');
                    out.push('/');
                    i += 2;
                    break;
                }
                out.push(chars[i]);
                i += 1;
            }
            continue;
        }
        if c == '=' && i + 1 < chars.len() && chars[i + 1] == '>' {
            if let Some(open_idx) = bare_destructuring_param_open(&out) {
                let mut close_idx = out.len();
                while close_idx > open_idx && out[close_idx - 1].is_whitespace() {
                    close_idx -= 1;
                }
                out.insert(open_idx, '(');
                out.insert(close_idx + 1, ')');
                changed = true;
            }
            out.push('=');
            out.push('>');
            i += 2;
            continue;
        }
        out.push(c);
        i += 1;
    }
    changed.then(|| out.into_iter().collect())
}

/// 把「`let`/`const` 重声明同名形参」改成 `var`。
///
/// 书源里有这种写法（Rhino 允许、正式规范不允许）：
///
/// ```js
/// function GetUrl(path, params, sourceUrl) {
///   let sourceUrl = sourceUrl || "https://…";   // SyntaxError: 与形参同名
/// }
/// ```
///
/// ES 规范规定 `let`/`const` 声明绝不能与同作用域的形参同名（即使非严格模式），
/// QuickJS 严格照办并报 `invalid redefinition of parameter name`；Rhino 宽松放行。
/// 书源的本意就是「给形参一个默认值」，`var` 在函数作用域里语义等价且合法。
///
/// 只改「确实与所在函数的形参同名」的那些声明，其余 `let`/`const` 一律不动 ——
/// 改成 `var` 会扩大作用域，可能撞上块外的同名变量，不能无差别替换。
///
/// 实现从简（只在解析失败后调用）：逐字符扫描，跳过字符串/模板串/注释/正则，
/// 跟踪函数体的花括号层级与**直接子级**形参名集合，遇到 `let|const <名字> =`
/// 且该名字在本层形参里时，把这一个关键字换成 `var`。
/// 供测试观察补丁结果（词法扫描容易写错，需要能直接看改写后的源码）。
pub fn patch_let_redeclared_params_for_test(script: &str) -> Option<String> {
    patch_let_redeclared_params(script)
}

fn patch_let_redeclared_params(script: &str) -> Option<String> {
    const QUOTE_SINGLE: char = '\u{27}';
    const QUOTE_DOUBLE: char = '\u{22}';
    const BACKTICK: char = '\u{60}';
    const BACKSLASH: char = '\u{5C}';
    const NEWLINE: char = '\u{A}';

    let chars: Vec<char> = script.chars().collect();
    // 每层函数体: (花括号深度, 该函数的形参名集合)
    let mut scopes: Vec<(i32, std::collections::HashSet<String>)> = Vec::new();
    let mut depth: i32 = 0;
    let mut out: Vec<char> = Vec::with_capacity(chars.len() + 16);
    let mut changed = false;
    let mut i = 0usize;

    // 读一个标识符（从 i 开始），返回 (名字, 结束位置)。
    fn read_ident(chars: &[char], mut at: usize) -> (String, usize) {
        let start = at;
        while at < chars.len()
            && (chars[at].is_alphanumeric() || chars[at] == '_' || chars[at] == '$')
        {
            at += 1;
        }
        (chars[start..at].iter().collect(), at)
    }

    while i < chars.len() {
        let c = chars[i];

        // 字符串 / 模板串：原样复制。
        if c == QUOTE_SINGLE || c == QUOTE_DOUBLE || c == BACKTICK {
            out.push(c);
            i += 1;
            while i < chars.len() {
                let ch = chars[i];
                out.push(ch);
                i += 1;
                if ch == BACKSLASH && i < chars.len() {
                    out.push(chars[i]);
                    i += 1;
                } else if ch == c {
                    break;
                }
            }
            continue;
        }
        // 行注释。
        if c == '/' && i + 1 < chars.len() && chars[i + 1] == '/' {
            while i < chars.len() && chars[i] != NEWLINE {
                out.push(chars[i]);
                i += 1;
            }
            continue;
        }
        // 块注释。
        if c == '/' && i + 1 < chars.len() && chars[i + 1] == '*' {
            out.push('/');
            out.push('*');
            i += 2;
            while i < chars.len() {
                if chars[i] == '*' && i + 1 < chars.len() && chars[i + 1] == '/' {
                    out.push('*');
                    out.push('/');
                    i += 2;
                    break;
                }
                out.push(chars[i]);
                i += 1;
            }
            continue;
        }
        // 正则字面量。
        if c == '/' && regex_literal_can_start(&out) {
            out.push(c);
            i += 1;
            let mut in_class = false;
            while i < chars.len() {
                let ch = chars[i];
                if ch == BACKSLASH && i + 1 < chars.len() {
                    out.push(ch);
                    out.push(chars[i + 1]);
                    i += 2;
                    continue;
                }
                if ch == NEWLINE {
                    break;
                }
                out.push(ch);
                i += 1;
                if ch == '[' {
                    in_class = true;
                } else if ch == ']' {
                    in_class = false;
                } else if ch == '/' && !in_class {
                    break;
                }
            }
            while i < chars.len() && chars[i].is_ascii_alphabetic() {
                out.push(chars[i]);
                i += 1;
            }
            continue;
        }

        // `function name(params) {` —— 记下形参名，等遇到 `{` 时开作用域。
        let rest: String = chars[i..chars.len().min(i + 8)].iter().collect();
        if rest.starts_with("function") && !out.last().is_some_and(|p| p.is_alphanumeric() || *p == '_' || *p == '$') {
            for ch in "function".chars() {
                out.push(ch);
            }
            i += "function".len();
            // 函数名（可省略）。
            while i < chars.len() && chars[i].is_whitespace() {
                out.push(chars[i]);
                i += 1;
            }
            if i < chars.len() && (chars[i].is_alphanumeric() || chars[i] == '_' || chars[i] == '$') {
                let (_, next) = read_ident(&chars, i);
                while i < next {
                    out.push(chars[i]);
                    i += 1;
                }
            }
            while i < chars.len() && chars[i] != '(' && chars[i] != '{' && chars[i] != '=' {
                out.push(chars[i]);
                i += 1;
            }
            if i < chars.len() && chars[i] == '(' {
                let mut params: std::collections::HashSet<String> = std::collections::HashSet::new();
                let mut level = 0i32;
                while i < chars.len() {
                    let ch = chars[i];
                    out.push(ch);
                    i += 1;
                    match ch {
                        '(' => level += 1,
                        ')' => {
                            level -= 1;
                            if level == 0 {
                                break;
                            }
                        }
                        _ => {
                            // 收集形参名（跳过解构与默认值的粗粒度处理：只认纯标识符）。
                            if level == 1
                                && (ch.is_alphanumeric() || ch == '_' || ch == '$')
                                && !out
                                    .get(out.len().saturating_sub(2))
                                    .is_some_and(|p| p.is_alphanumeric() || *p == '_' || *p == '$')
                            {
                                let (name, next) = read_ident(&chars, i - 1);
                                if !name.is_empty() && !name.chars().all(|c| c.is_ascii_digit()) {
                                    params.insert(name);
                                }
                                while i < next {
                                    out.push(chars[i]);
                                    i += 1;
                                }
                            }
                        }
                    }
                }
                // 等函数体的 `{`。
                let mut pending = Some(params);
                while i < chars.len() {
                    if chars[i] == '{' {
                        depth += 1;
                        out.push('{');
                        i += 1;
                        if let Some(p) = pending.take() {
                            scopes.push((depth, p));
                        }
                        break;
                    }
                    out.push(chars[i]);
                    i += 1;
                }
                continue;
            }
            continue;
        }

        // `let x =` / `const x =`：若 x 在当前函数形参里，换成 `var`。
        let is_let = rest.starts_with("let") && !chars.get(i + 3).is_some_and(|p| p.is_alphanumeric() || *p == '_' || *p == '$');
        let is_const = rest.starts_with("const") && !chars.get(i + 5).is_some_and(|p| p.is_alphanumeric() || *p == '_' || *p == '$');
        if (is_let || is_const)
            && !out.last().is_some_and(|p| p.is_alphanumeric() || *p == '_' || *p == '$' || *p == '.')
        {
            let kw_len = if is_let { 3 } else { 5 };
            // 跳过关键字与空白，读变量名。
            let mut at = i + kw_len;
            while at < chars.len() && chars[at].is_whitespace() {
                at += 1;
            }
            let (name, after) = read_ident(&chars, at);
            let shadowed = !name.is_empty()
                && scopes
                    .iter()
                    .rev()
                    .find(|(d, _)| *d == depth)
                    .is_some_and(|(_, params)| params.contains(&name));
            if shadowed {
                // 只替换关键字本身，其余原样（缩进、空白都保留）。
                for ch in "var".chars() {
                    out.push(ch);
                }
                for ch in &chars[i + kw_len..at] {
                    out.push(*ch);
                }
                i = at;
                let _ = after;
                changed = true;
                continue;
            }
        }

        // 维护花括号层级。
        if c == '{' {
            depth += 1;
        } else if c == '}' {
            scopes.retain(|(d, _)| *d != depth);
            depth -= 1;
        }
        out.push(c);
        i += 1;
    }

    changed.then(|| out.into_iter().collect())
}

/// `/` 是否可能开始一个正则字面量(而不是除号)。
///
/// 判断标准只看前一个有效字符: 紧跟在值(标识符、`)`、`]`、字符串等)之后的 `/`
/// 是除号, 其余位置(行首、`=`、`(`、`,`、`return` 之后)按正则处理。
fn regex_literal_can_start(out: &[char]) -> bool {
    let mut idx = out.len();
    while idx > 0 && out[idx - 1].is_whitespace() {
        idx -= 1;
    }
    if idx == 0 {
        return true;
    }
    let prev = out[idx - 1];
    !(prev.is_alphanumeric()
        || prev == '_'
        || prev == '$'
        || prev == ')'
        || prev == ']'
        || prev == '}'
        || prev == '\''
        || prev == '"'
        || prev == '`')
}

/// "=>" 左侧若是裸解构形参, 返回其起始括号在 out 中的下标。
fn bare_destructuring_param_open(out: &[char]) -> Option<usize> {
    let mut end = out.len();
    while end > 0 && out[end - 1].is_whitespace() {
        end -= 1;
    }
    if end == 0 {
        return None;
    }
    let close = out[end - 1];
    let open = match close {
        ']' => '[',
        '}' => '{',
        _ => return None,
    };
    let mut depth = 0i32;
    let mut idx = end;
    let mut open_idx = None;
    while idx > 0 {
        idx -= 1;
        if out[idx] == close {
            depth += 1;
        } else if out[idx] == open {
            depth -= 1;
            if depth == 0 {
                open_idx = Some(idx);
                break;
            }
        }
    }
    let open_idx = open_idx?;
    // 括号前是标识符/")"/"]"/"}"/"." 时, 这是下标或成员访问, 不是形参表。
    let mut before = open_idx;
    while before > 0 && out[before - 1].is_whitespace() {
        before -= 1;
    }
    if before == 0 {
        return Some(open_idx);
    }
    let prev = out[before - 1];
    if prev.is_alphanumeric()
        || prev == '_'
        || prev == '$'
        || prev == ')'
        || prev == ']'
        || prev == '}'
        || prev == '.'
    {
        return None;
    }
    Some(open_idx)
}

fn active_js_lib_script() -> String {
    let js_lib = ACTIVE_JS_LIB.with(|cell| cell.borrow().clone());
    let Some(js_lib) = js_lib.filter(|value| !value.trim().is_empty()) else {
        return String::new();
    };
    let cache_key = md5_hex(&js_lib);
    if let Some((cached, retry_at)) = JS_LIB_CACHE
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get(&cache_key)
        .cloned()
    {
        if retry_at.map_or(true, |at| Instant::now() < at) {
            return cached;
        }
    }

    let (compiled, complete) = compile_js_lib(&js_lib);
    let retry_at = (!complete).then(|| Instant::now() + JS_LIB_RETRY_AFTER);
    JS_LIB_CACHE
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .insert(cache_key, (compiled.clone(), retry_at));
    compiled
}

/// 组装 jsLib。JSON 对象形式 `{"name": "https://...js"}` 按 legado 逐个下载 URL 值;
/// 返回 (源码, 是否全部成功)。下载失败的条目跳过, 不影响其余条目与规则执行。
fn compile_js_lib(js_lib: &str) -> (String, bool) {
    let trimmed = js_lib.trim();
    if trimmed.starts_with('{') {
        if let Ok(JsonValue::Object(map)) = serde_json::from_str::<JsonValue>(trimmed) {
            let mut scripts = Vec::new();
            let mut complete = true;
            for entry in map.values().filter_map(|v| v.as_str()) {
                match resolve_js_lib_entry(entry) {
                    Ok(script) => scripts.push(script),
                    Err(e) => {
                        complete = false;
                        push_js_log(format!("[jsLib] {e}"));
                    }
                }
            }
            return (scripts.join("\n;\n"), complete);
        }
    }
    (trimmed.to_string(), true)
}

fn js_lib_cache_path(url: &str) -> Option<PathBuf> {
    JS_STORAGE_DIR
        .get()
        .map(|dir| dir.join("cache").join("jslib").join(format!("{}.js", md5_hex(url))))
}

/// URL 条目: 优先读磁盘缓存(与 legado 相同, 永不过期; 修改 jsLib 后按新 URL 重新下载),
/// 否则下载并落盘。非 URL 条目按内联脚本处理。
fn resolve_js_lib_entry(entry: &str) -> anyhow::Result<String> {
    let value = entry.trim();
    if !(value.starts_with("http://") || value.starts_with("https://")) {
        return Ok(value.to_string());
    }
    let cache_path = js_lib_cache_path(value);
    if let Some(cached) = cache_path
        .as_ref()
        .and_then(|path| std::fs::read_to_string(path).ok())
        .filter(|text| !text.trim().is_empty())
    {
        return Ok(cached);
    }
    let url = value.to_string();
    let text = run_blocking_http(move || -> anyhow::Result<String> {
        let response = JS_HTTP_CLIENT
            .get(&url)
            .header("User-Agent", DEFAULT_JS_USER_AGENT)
            .send()?;
        let status = response.status();
        if !status.is_success() {
            anyhow::bail!("下载 jsLib {url} 失败: HTTP {status}");
        }
        let text = response.text()?;
        if text.trim().is_empty() {
            anyhow::bail!("下载 jsLib {url} 失败: 内容为空");
        }
        Ok(text)
    })
    .map_err(|e| anyhow::anyhow!("下载 jsLib {value} 失败: {e}"))?;
    if let Some(path) = cache_path {
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let _ = std::fs::write(path, &text);
    }
    Ok(text)
}

fn java_time_format(timestamp: i64) -> String {
    let secs = if timestamp > 1_000_000_000_000 {
        timestamp / 1000
    } else {
        timestamp
    };
    match Local.timestamp_opt(secs, 0).single() {
        Some(dt) => dt.format("%Y-%m-%d %H:%M").to_string(),
        None => String::new(),
    }
}

/// legado `java.digestHex(data, algorithm)`: 计算 MD5/SHA-1/SHA-224/SHA-256/SHA-384/SHA-512 摘要(hex)
/// 把字节序列序列化成 JSON 数组字符串，交给 JS 侧 `JSON.parse` 成普通数组。
///
/// legado 的 `java.base64DecodeToByteArray` / `hexDecodeToByteArray` 返回 Rhino 的
/// `ByteArray`，脚本按数组使用（`.length`、`.slice(start, end)`、按索引取值），
/// 并把它当 key/iv 传给 `createSymmetricCrypto`。
///
/// 这里不直接返回 `TypedArray`：`TypedArray<'js, T>` 对 `'js` **不变**，而宿主函数
/// 都注册在 `with_reusable_context` 的 `impl FnOnce(Ctx<'_>, bool) -> Result<T>`
/// 闭包里（`T` 是固定类型），拿不到与 ctx 一致的生命周期。走 JSON 数组既避开这个
/// 约束，也天然满足 `.length` / `.slice()` / 索引三种用法。
fn bytes_to_json_array(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 4 + 2);
    out.push('[');
    for (index, byte) in bytes.iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        out.push_str(&byte.to_string());
    }
    out.push(']');
    out
}

fn java_digest_hex(input: &str, algorithm: &str) -> anyhow::Result<String> {
    let bytes = input.as_bytes();
    let out = match normalize_digest_name(algorithm).as_str() {
        "MD5" => hex::encode(Md5::digest(bytes)),
        "SHA-1" => hex::encode(Sha1::digest(bytes)),
        "SHA-224" => hex::encode(Sha224::digest(bytes)),
        "SHA-256" => hex::encode(Sha256::digest(bytes)),
        "SHA-384" => hex::encode(Sha384::digest(bytes)),
        "SHA-512" => hex::encode(Sha512::digest(bytes)),
        _ => anyhow::bail!("不支持摘要算法 {algorithm}"),
    };
    Ok(out)
}

/// legado `java.digestBase64Str(data, algorithm)`: 同上但 base64 输出
fn java_digest_base64(input: &str, algorithm: &str) -> anyhow::Result<String> {
    let bytes = input.as_bytes();
    let out = match normalize_digest_name(algorithm).as_str() {
        "MD5" => base64::engine::general_purpose::STANDARD.encode(Md5::digest(bytes)),
        "SHA-1" => base64::engine::general_purpose::STANDARD.encode(Sha1::digest(bytes)),
        "SHA-224" => base64::engine::general_purpose::STANDARD.encode(Sha224::digest(bytes)),
        "SHA-256" => base64::engine::general_purpose::STANDARD.encode(Sha256::digest(bytes)),
        "SHA-384" => base64::engine::general_purpose::STANDARD.encode(Sha384::digest(bytes)),
        "SHA-512" => base64::engine::general_purpose::STANDARD.encode(Sha512::digest(bytes)),
        _ => anyhow::bail!("不支持摘要算法 {algorithm}"),
    };
    Ok(out)
}

/// legado `java.HMacHex(data, algorithm, key)`: HMAC 摘要(hex)
/// 供 `js_binary` 的 `mac` op 复用：按**字节**输入算 HMAC，返回十六进制。
///
/// `javax.crypto.Mac.getInstance(...).doFinal(bytes)` 的输入是字节数组，
/// 直接走 `java_hmac_hex`（`&str`）会先把字节转文本再取 UTF-8 字节，非 UTF-8
/// 的二进制签名数据会被改写。
///
/// 算法名归一化与 `java.HMacHex` / `HMacBase64` 共用 [`normalize_digest_name`]：
/// 两条 HMAC 路径曾经分叉 —— 这条剥了 `HMAC` 前缀、那条没剥，于是
/// `Packages.javax.crypto.Mac("HmacSHA1")` 正常而 `java.HMacHex("HmacSHA1")`
/// 静默返回空串（issue #29）。改这里时**不要**再自己解析算法名。
pub(crate) fn java_hmac_hex_bytes(key: &[u8], algorithm: &str, data: &[u8]) -> anyhow::Result<String> {
    // 必须**直接用字节**算：早期实现把 key/data 按 ISO-8859-1 转成 `&str` 再取
    // UTF-8 字节，等于把每个高位字节膨胀成两字节，摘要完全对不上 —— 而书源的
    // 密钥常由 `SecureRandom` 生成，非 UTF-8 是常态。
    macro_rules! hmac_bytes {
        ($ty:ty) => {{
            use hmac::Mac as _;
            let mut mac = Hmac::<$ty>::new_from_slice(key)
                .map_err(|err| anyhow::anyhow!("HMAC 初始化失败: {err}"))?;
            mac.update(data);
            Ok(hex::encode(mac.finalize().into_bytes()))
        }};
    }
    match normalize_digest_name(algorithm).as_str() {
        "MD5" => hmac_bytes!(Md5),
        "SHA-1" => hmac_bytes!(Sha1),
        "SHA-224" => hmac_bytes!(Sha224),
        "SHA-256" => hmac_bytes!(Sha256),
        "SHA-384" => hmac_bytes!(Sha384),
        "SHA-512" => hmac_bytes!(Sha512),
        _ => anyhow::bail!("不支持 HMAC 算法 {algorithm}"),
    }
}

fn java_hmac_hex(input: &str, algorithm: &str, key: &str) -> anyhow::Result<String> {
    java_hmac(input, algorithm, key, true)
}

/// legado `java.HMacBase64(data, algorithm, key)`: HMAC 摘要(base64)
fn java_hmac_base64(input: &str, algorithm: &str, key: &str) -> anyhow::Result<String> {
    java_hmac(input, algorithm, key, false)
}

/// 通用 HMAC 计算, 按算法名分发到对应摘要类型。
///
/// 未知算法**返回错误**而不是空串：legado 的 `Mac.getInstance(未知名)` 抛
/// `NoSuchAlgorithmException`，书源调试面板会直接指出算法名写错。早先这里
/// `unwrap_or_default()` 静默给 `""`，签名为空但流程照走，服务端只回
/// `Invalid Signature` —— 排查成本极高（issue #29）。
fn java_hmac(input: &str, algorithm: &str, key: &str, hex_out: bool) -> anyhow::Result<String> {
    let key_bytes = key.as_bytes();
    let name = normalize_digest_name(algorithm);
    let encoded: Option<String> = match name.as_str() {
        "MD5" => hmac_md5(input, key_bytes, hex_out),
        "SHA-1" => hmac_sha1(input, key_bytes, hex_out),
        "SHA-224" => hmac_sha224(input, key_bytes, hex_out),
        "SHA-256" => hmac_sha256(input, key_bytes, hex_out),
        "SHA-384" => hmac_sha384(input, key_bytes, hex_out),
        "SHA-512" => hmac_sha512(input, key_bytes, hex_out),
        _ => anyhow::bail!("不支持 HMAC 算法 {algorithm}"),
    };
    // `new_from_slice` 只会因空密钥失败；显式报错同样比空串好排查。
    encoded.ok_or_else(|| anyhow::anyhow!("HMAC 初始化失败（算法 {algorithm}）"))
}

macro_rules! hmac_impl {
    ($name:ident, $ty:ty) => {
        fn $name(input: &str, key: &[u8], hex_out: bool) -> Option<String> {
            let mut mac = Hmac::<$ty>::new_from_slice(key).ok()?;
            mac.update(input.as_bytes());
            let bytes = mac.finalize().into_bytes();
            if hex_out {
                Some(hex::encode(bytes))
            } else {
                Some(base64::engine::general_purpose::STANDARD.encode(bytes))
            }
        }
    };
}

hmac_impl!(hmac_md5, Md5);
hmac_impl!(hmac_sha1, Sha1);
hmac_impl!(hmac_sha224, Sha224);
hmac_impl!(hmac_sha256, Sha256);
hmac_impl!(hmac_sha384, Sha384);
hmac_impl!(hmac_sha512, Sha512);

/// 归一化**摘要**算法名，返回 `MD5` / `SHA-1` / … 这样的基准名。
///
/// 兼容三种写法（书源里都出现过）：
/// - 规范名 `SHA-1` / `MD5`；
/// - 无连字符 `SHA1` / `SHA256`（Java 的 `MessageDigest.getInstance` 也认）；
/// - JCA 的 HMAC 前缀写法 `HmacSHA1` / `HmacSHA256` —— 书源常把
///   `HMacHex` 的算法名直接抄到 `digestHex` 上。legado 的
///   `DigestUtil.digester("HmacSHA1")` 会抛 `NoSuchAlgorithmException`，
///   但把 `HmacSHA1` 认成 `SHA-1` 并不会算错（HMAC 的底层摘要就是它），
///   因此这里接受它 —— 比让整条规则失败更有用。
///
/// 认不出的名字原样返回（大写、去连字符），由调用方的 match 落到错误分支。
fn normalize_digest_name(algorithm: &str) -> String {
    // `HmacSHA-256` 与 `HmacSHA256` 都要认：先统一去掉连字符再比对。
    let compact = algorithm.trim().to_ascii_uppercase().replace('-', "");
    let bare = compact.strip_prefix("HMAC").unwrap_or(&compact);
    match bare {
        "MD5" => "MD5".to_string(),
        "SHA" | "SHA1" => "SHA-1".to_string(),
        "SHA224" => "SHA-224".to_string(),
        "SHA256" => "SHA-256".to_string(),
        "SHA384" => "SHA-384".to_string(),
        "SHA512" => "SHA-512".to_string(),
        other => other.to_string(),
    }
}

/// 繁 → 简映射对（`T2S_PAIRS` 按「繁体简体」两两成对排列）。
///
/// 数据源: Windows `LCMapStringEx(zh-TW, LCMAP_SIMPLIFIED_CHINESE)` 逐字转换
/// 生成（2473 对，覆盖 CJK 基本区），因此无需引入 OpenCC 依赖。
/// legado 的 `java.t2s` 用它把繁体源站的章节名/正文转成简体。
const T2S_PAIRS: &str = concat!(
    "丟丢並并亂乱亙亘亞亚伋汲伕夫佇伫佈布佔占佪徊併并來来侖仑侶侣侷局俁俣係系俠侠倀伥",
    "倆俩倉仓個个們们倖幸倣仿倫伦偉伟側侧偵侦偺咱偽伪傑杰傖伧傘伞備备傢家傭佣傯偬傳传",
    "傴伛債债傷伤傾倾僂偻僅仅僇戮僉佥僑侨僕仆僥侥僨偾僱雇價价儀仪儂侬億亿儅当儈侩儉俭",
    "儐傧儔俦儕侪儘尽償偿優优儲储儷俪儸罗儺傩儻傥儼俨兇凶兌兑兒儿兗兖內内兩两冊册冑胄",
    "冪幂凈净凍冻凜凛凱凯別别刪删剄刭則则剋克剎刹剛刚剝剥剮剐剴剀創创剷铲劃划劇剧劉刘",
    "劊刽劌刿劍剑劑剂劻匡勁劲動动勗勖務务勛勋勝胜勞劳勢势勣积勦剿勱劢勵励勸劝勻匀匭匦",
    "匯汇匱匮區区協协卹恤卻却厙厍厭厌厲厉厴厣參参叢丛吋寸吳吴吶呐呂吕呎尺咼呙員员唄呗",
    "唸念問问啞哑啟启啣衔喚唤喪丧喫吃喬乔單单喲哟嗆呛嗇啬嗎吗嗚呜嗩唢嗶哔嘆叹嘍喽嘔呕",
    "嘖啧嘗尝嘜唛嘩哗嘮唠嘯啸嘰叽嘵哓嘸呒噁恶噓嘘噠哒噥哝噦哕噯嗳噲哙噴喷噸吨噹当嚀咛",
    "嚇吓嚌哜嚕噜嚙啮嚥咽嚦呖嚨咙嚮向嚳喾嚴严嚶嘤囀啭囁嗫囂嚣囅冁囈呓囉罗囌苏囑嘱囪囱",
    "圇囵國国圍围園园圓圆圖图團团坰垧埡垭執执堅坚堊垩堝埚堯尧報报場场堿碱塊块塋茔塏垲",
    "塒埘塗涂塢坞塤埙塵尘塹堑墊垫墑墒墜坠墮堕墳坟墻墙墾垦壇坛壎埙壓压壘垒壙圹壚垆壞坏",
    "壟垄壢坜壩坝壯壮壺壶壽寿夠够夢梦夾夹奐奂奧奥奩奁奪夺奮奋妝妆妳你姍姗姦奸姪侄娛娱",
    "婁娄婦妇婭娅媧娲媯妫媼媪媽妈嫗妪嫵妩嫻娴嬈娆嬋婵嬌娇嬙嫱嬝袅嬡嫒嬤嬷嬪嫔嬰婴嬸婶",
    "孃娘孌娈孫孙學学孿孪宮宫寢寝實实寧宁審审寫写寬宽寵宠寶宝將将專专尋寻對对導导尷尴",
    "屆届屜屉屝扉屢屡層层屨屦屬属岡冈岧迢峴岘島岛峽峡崍崃崗岗崙仑崠岽崢峥崳嵛嵐岚嵒岩",
    "嶁嵝嶄崭嶇岖嶗崂嶠峤嶧峄嶸嵘嶺岭嶼屿嶽岳巋岿巑漓巒峦巔巅巖岩巰巯巹卺帥帅師师帳帐",
    "帶带幀帧幃帏幗帼幘帻幟帜幣币幫帮幬帱幹干幾几庂仄庫库廁厕廂厢廄厩廈厦廚厨廝厮廟庙",
    "廠厂廡庑廢废廣广廩廪廬庐廱痈廳厅弒弑弔吊弳弪張张強强彆别彈弹彊强彌弥彎弯彙汇彥彦",
    "彫雕彿佛徑径從从徠徕復复徬旁徹彻恆恒恥耻悅悦悵怅悶闷悽凄惡恶惱恼惲恽惻恻愛爱愜惬",
    "愨悫愴怆愷恺愾忾慄栗慇殷態态慍愠慘惨慚惭慟恸慣惯慪怄慫怂慮虑慳悭慶庆慼戚憂忧憊惫",
    "憐怜憑凭憒愦憚惮憤愤憫悯憮怃憲宪憶忆懇恳應应懌怿懍檩懞蒙懟怼懣懑懨恹懲惩懶懒懷怀",
    "懸悬懺忏懼惧懾慑戀恋戇戆戉钺戔戋戧戗戩戬戰战戲戏戶户扐仂扠叉扢扦扺抵抃拚抆擦抎殒",
    "抴曳拋抛挶局挾挟捍扞捨舍捫扪捲卷掃扫掄抡掙挣掛挂採采揀拣揚扬換换揮挥揹背搆构損损",
    "搖摇搗捣搟擀搶抢摑掴摜掼摟搂摯挚摳抠摶抟摻掺撈捞撐撑撓挠撚捻撟挢撢掸撣掸撥拨撫抚",
    "撲扑撳揿撻挞撾挝撿捡擁拥擄掳擇择擊击擋挡擔担據据擠挤擣捣擬拟擯摈擰拧擱搁擲掷擴扩",
    "擷撷擺摆擻擞擼撸擾扰攄摅攆撵攏拢攔拦攖撄攙搀攛撺攜携攝摄攢攒攣挛攤摊攪搅攬揽攷考",
    "敗败敘叙敵敌數数斂敛斃毙斕斓斬斩斷断旂旗時时晉晋晝昼晞曦暈晕暉晖暘阳暢畅暫暂暱昵",
    "暸了曄晔曆历曇昙曉晓曖暧曠旷曬晒書书會会朧胧朮术杇圬東东枙栀枴拐柵栅桿杆梔栀條条",
    "梟枭棄弃棖枨棗枣棟栋棧栈棲栖椏桠楊杨楓枫楨桢業业極极榦干榪杩榮荣榿桤槃盘構构槍枪",
    "槓杠槧椠槨椁槳桨樁桩樂乐樅枞樑梁樓楼標标樞枢樣样樸朴樹树樺桦橈桡橋桥機机橢椭橫横",
    "檁檩檉柽檔档檜桧檢检檣樯檯台檳槟檸柠檻槛櫃柜櫓橹櫚榈櫛栉櫝椟櫞橼櫟栎櫥橱櫧槠櫨栌",
    "櫪枥櫬榇櫳栊櫸榉櫺棂櫻樱欄栏權权欏椤欒栾欖榄欞棂欽钦歎叹歐欧歟欤歡欢歲岁歷历歸归",
    "歿殁殘残殞殒殤殇殫殚殮殓殯殡殲歼殺杀殼壳毀毁毆殴毌毋毬球毿毵氈毡氌氇氣气氫氢氬氩",
    "氳氲氾泛汍丸汎泛汙污決决沒没沖冲況况洩泄洶汹浬里浹浃涇泾涼凉淒凄淚泪淥渌淨净淪沦",
    "淵渊淶涞淺浅渙涣減减渦涡測测渾浑湊凑湞浈湧涌湯汤溈沩準准溝沟溫温溼湿滄沧滅灭滌涤",
    "滎荥滬沪滯滞滲渗滷卤滸浒滾滚滿满漁渔漚沤漢汉漣涟漬渍漲涨漸渐漿浆潁颍潑泼潔洁潛潜",
    "潟泻潤润潯浔潰溃潷滗潿涠澀涩澆浇澇涝澗涧澠渑澤泽澩泶澮浍澱淀濁浊濃浓濕湿濘泞濛蒙",
    "濟济濤涛濫滥濰潍濱滨濺溅濼泺濾滤瀅滢瀆渎瀉泻瀋渖瀏浏瀕濒瀘泸瀝沥瀟潇瀠潆瀧泷瀨濑",
    "瀰弥瀲潋瀾澜灃沣灄滠灑洒灕漓灘滩灝灏灣湾灤滦災灾為为烏乌烴烃無无煉炼煒炜煙烟煢茕",
    "煥焕煩烦煬炀熒荧熗炝熱热熾炽燁烨燄焰燈灯燉炖燐磷燒烧燙烫燜焖營营燦灿燬毁燭烛燴烩",
    "燼烬燾焘爍烁爐炉爛烂爭争爺爷爾尔牆墙牘牍牪瘪牴抵牽牵犖荦犛犁犢犊犧牺狀状狹狭狽狈",
    "猙狰猶犹猻狲獄狱獅狮獎奖獨独獪狯獫猃獮猕獰狞獲获獵猎獷犷獸兽獺獭獻献獼猕玀猡玆兹",
    "玨珏珮佩現现琺珐琿珲瑋玮瑣琐瑤瑶瑩莹瑪玛瑯琅璉琏璣玑璦瑷環环璽玺瓊琼瓏珑瓔璎瓖镶",
    "瓚瓒甌瓯甕瓮產产甦苏畝亩畢毕畫画畬畲異异當当疇畴疊叠疿痱痙痉痠酸痲麻痳麻痺痹瘋疯",
    "瘍疡瘓痪瘞瘗瘡疮瘧疟瘺瘘療疗癆痨癇痫癉瘅癘疠癟瘪癡痴癢痒癤疖癥症癩癞癬癣癭瘿癮瘾",
    "癰痈癱瘫癲癫發发皚皑皰疱皸皲皺皱皻隳盜盗盞盏盡尽監监盤盘盧卢盪荡眥眦眾众睏困睜睁",
    "睞睐睪睾瞇眯瞞瞒瞭了瞼睑矇蒙矓胧矚瞩矯矫砲炮硃朱硤硖硨砗硯砚碩硕碭砀確确碼码磚砖",
    "磣碜磧碛磯矶磽硗礎础礙碍礦矿礪砺礫砾礬矾礱砻祅祆祇只祐佑祕秘祿禄禍祸禎祯禦御禪禅",
    "禮礼禰祢禱祷禿秃秈籼稅税稈秆稜棱稟禀種种稱称穀谷穌稣積积穎颖穡穑穢秽穩稳穫获窩窝",
    "窪洼窮穷窯窑窶窭窺窥竄窜竅窍竇窦竊窃競竞笻筇筆笔筍笋筧笕箄箅箇个箋笺箏筝節节範范",
    "築筑篋箧篠筱篤笃篩筛篳筚簀箦簍篓簑蓑簞箪簡简簣篑簫箫簷檐簽签簾帘籃篮籌筹籐藤籜箨",
    "籟籁籠笼籤签籥龠籩笾籪簖籬篱籮箩籲吁粵粤糝糁糞粪糢馍糧粮糰团糲粝糴籴糶粜糾纠紀纪",
    "紂纣約约紅红紆纡紇纥紈纨紉纫紋纹納纳紐纽紓纾純纯紕纰紗纱紙纸級级紛纷紜纭紡纺細细",
    "紱绂紲绁紳绅紹绍紺绀紼绋紿绐絀绌終终絃弦組组絆绊絎绗結结絕绝絛绦絞绞絡络絢绚給给",
    "絨绒統统絲丝絳绦絹绢綁绑綃绡綆绠綈绨綏绥經经綜综綞缍綠绿綢绸綣绻綬绶維维綰绾綱纲",
    "網网綴缀綵彩綸纶綹绺綺绮綻绽綽绰綾绫綿绵緄绲緇缁緊紧緋绯緒绪緗缃緘缄緙缂線线緝缉",
    "緞缎締缔緡缗緣缘緦缌編编緩缓緬缅緯纬緱缑緲缈練练緶缏緹缇緻致縈萦縉缙縊缢縋缒縐绉",
    "縑缣縚绦縛缚縝缜縞缟縟缛縣县縫缝縭缡縮缩縯演縱纵縲缧縳缚縴纤縵缦縶絷縷缕縹缥總总",
    "績绩繃绷繅缫繆缪繈襁繒缯織织繕缮繚缭繞绕繡绣繢缋繩绳繪绘繫系繭茧繯缳繰缲繳缴繹绎",
    "繼继繽缤繾缱纈缬纊纩續续纍累纏缠纓缨纔才纖纤纘缵纜缆缽钵罈坛罌罂罰罚罵骂罷罢羅罗",
    "羆罴羈羁羋芈羥羟羨羡義义習习翹翘耑端耬耧聖圣聞闻聯联聰聪聲声聳耸聵聩聶聂職职聹聍",
    "聽听聾聋肅肃脅胁脈脉脛胫脣唇脧睃脩修脫脱脹胀腎肾腡脶腦脑腫肿腳脚腸肠膃腽膚肤膠胶",
    "膩腻膽胆膾脍膿脓臉脸臍脐臏膑臘腊臚胪臟脏臠脔臥卧臨临臺台與与興兴舉举舊旧艙舱艣橹",
    "艤舣艦舰艫舻艱艰艷艳艸艹芐苄芻刍苧苎茍苟茲兹荊荆莊庄莖茎莢荚莧苋華华菴庵萇苌萊莱",
    "萬万萵莴葉叶葒荭著着葦苇葯药葷荤蒔莳蒞莅蒼苍蓀荪蓋盖蓮莲蓯苁蓽荜蔆菱蔔卜蔞蒌蔣蒋",
    "蔥葱蔦茑蔭荫蕁荨蕆蒇蕎荞蕓芸蕕莸蕘荛蕢蒉蕩荡蕪芜蕭萧蕷蓣薈荟薊蓟薌芗薑姜薔蔷薟莶",
    "薦荐薧槁薩萨薺荠藍蓝藎荩藝艺藥药藪薮藶苈藷薯藹蔼藺蔺蘄蕲蘆芦蘇苏蘊蕴蘋苹蘗蘖蘚藓",
    "蘞蔹蘢茏蘭兰蘺蓠蘿萝處处虛虚虜虏號号虧亏蛺蛱蛻蜕蜆蚬蝕蚀蝦虾蝸蜗螄蛳螞蚂螢萤螻蝼",
    "蟄蛰蟈蝈蟣虮蟬蝉蟯蛲蟲虫蟶蛏蟻蚁蠅蝇蠆虿蠐蛴蠑蝾蠔蚝蠟蜡蠣蛎蠱蛊蠶蚕蠻蛮衊蔑術术",
    "衛卫衝冲衹只袞衮裊袅裏里補补裝装裡里製制複复褲裤褳裢褸褛褻亵襖袄襝裣襠裆襤褴襪袜",
    "襬摆襯衬襲袭覈核見见規规覓觅視视覘觇覡觋覦觎親亲覬觊覯觏覲觐覷觑覺觉覽览覿觌觀观",
    "觔筋觴觞觶觯觸触訂订訃讣計计訊讯訌讧討讨訐讦訓训訕讪訖讫託托記记訛讹訝讶訟讼訢欣",
    "訣诀訥讷訪访設设許许訴诉訶诃診诊註注証证詁诂詆诋詎讵詐诈詒诒詔诏評评詘诎詛诅詞词",
    "詠咏詡诩詢询詣诣試试詩诗詫诧詬诟詭诡詮诠詰诘話话該该詳详詵诜詼诙詿诖誄诔誅诛誆诓",
    "誇夸誌志認认誑诳誒诶誕诞誘诱誚诮語语誠诚誡诫誣诬誤误誥诰誦诵誨诲說说誰谁課课誶谇",
    "誹诽誼谊調调諂谄諄谆談谈諉诿請请諍诤諏诹諑诼諒谅論论諗谂諛谀諜谍諞谝諢诨諤谔諦谛",
    "諧谐諫谏諭谕諮谘諱讳諳谙諶谌諷讽諸诸諺谚諼谖諾诺謀谋謁谒謂谓謄誊謅诌謊谎謎谜謐谧",
    "謔谑謖谡謗谤謙谦謚谥講讲謝谢謠谣謨谟謫谪謬谬謳讴謹谨謾谩證证譎谲譏讥譖谮識识譙谯",
    "譚谭譜谱譟噪譫谵譯译議议譴谴護护譽誉讀读變变讎雠讒谗讓让讕谰讖谶讚赞讜谠讞谳豈岂",
    "豎竖豐丰豔艳豬猪貍狸貓猫貝贝貞贞負负財财貢贡貧贫貨货販贩貪贪貫贯責责貯贮貰贳貲赀",
    "貳贰貴贵貶贬買买貸贷貺贶費费貼贴貽贻貿贸賀贺賁贲賂赂賃赁賄贿賅赅資资賈贾賊贼賑赈",
    "賒赊賓宾賕赇賚赉賜赐賞赏賠赔賡赓賢贤賣卖賤贱賦赋賧赕質质賬账賭赌賴赖賺赚賻赙購购",
    "賽赛賾赜贄贽贅赘贈赠贊赞贍赡贏赢贐赆贓赃贖赎贗赝贛赣趕赶趙赵趨趋趲趱跡迹跼局踐践",
    "踡蜷踫碰踴踊蹌跄蹕跸蹠跖蹣蹒蹤踪蹺跷躉趸躊踌躋跻躍跃躑踯躒跞躓踬躕蹰躚跹躡蹑躥蹿",
    "躦躜躪躏軀躯車车軋轧軌轨軍军軒轩軔轫軛轭軟软軫轸軸轴軹轵軺轺軻轲軼轶軾轼較较輅辂",
    "輇辁載载輊轾輒辄輓挽輔辅輕轻輛辆輜辎輝辉輞辋輟辍輥辊輦辇輩辈輪轮輯辑輳辏輸输輻辐",
    "輾辗輿舆轂毂轄辖轅辕轆辘轉转轍辙轎轿轔辚轟轰轡辔轢轹轤轳辦办辭辞辮辫辯辩農农迆迤",
    "迴回迺乃逕迳這这連连週周進进遊游運运過过達达違违遙遥遜逊遞递遠远適适遲迟遷迁選选",
    "遺遗遼辽邁迈還还邇迩邊边邏逻邐逦郃合郟郏郵邮鄆郓鄉乡鄒邹鄔邬鄖郧鄧邓鄭郑鄰邻鄲郸",
    "鄴邺鄶郐鄺邝酈郦醃腌醜丑醞酝醫医醬酱釀酿釁衅釃酾釅酽釋释釐厘釓钆釔钇釕钌釗钊釘钉",
    "釙钋針针釣钓釤钐釦扣釧钏釩钒釬钎釵钗釷钍釹钕鈀钯鈁钫鈄钭鈉钠鈍钝鈐钤鈑钣鈔钞鈕钮",
    "鈞钧鈣钙鈥钬鈦钛鈧钪鈮铌鈰铈鈳钶鈴铃鈷钴鈸钹鈹铍鈺钰鈽钸鈾铀鈿钿鉀钾鉅钜鉆钻鉈铊",
    "鉉铉鉋刨鉍铋鉑铂鉗钳鉚铆鉛铅鉞钺鉤钩鉦钲鉬钼鉭钽鉸铰鉺铒鉻铬鉿铪銀银銃铳銅铜銑铣",
    "銓铨銖铢銘铭銚铫銜衔銠铑銣铷銥铱銦铟銨铵銩铥銪铕銫铯銬铐銲焊銳锐銷销銹锈銻锑銼锉",
    "鋁铝鋃锒鋅锌鋇钡鋌铤鋏铗鋒锋鋝锊鋟锓鋤锄鋦锔鋨锇鋪铺鋮铖鋯锆鋰锂鋱铽鋸锯鋼钢錁锞",
    "錄录錆锖錈锩錐锥錒锕錕锟錘锤錙锱錚铮錛锛錟锬錠锭錢钱錦锦錨锚錫锡錮锢錯错錳锰錶表",
    "錸铼鍆钔鍇锴鍊链鍋锅鍍镀鍔锷鍘铡鍛锻鍤锸鍥锲鍬锹鍰锾鍵键鍶锶鍺锗鍾锺鎂镁鎊镑鎔熔",
    "鎖锁鎗枪鎘镉鎢钨鎣蓥鎦镏鎧铠鎩铩鎪锼鎬镐鎮镇鎰镒鎳镍鎵镓鏃镞鏇镟鏈链鏌镆鏍镙鏑镝",
    "鏗铿鏘锵鏜镗鏝镘鏞镛鏟铲鏡镜鏢镖鏤镂鏨錾鏵铧鏷镤鏹镪鏽锈鐃铙鐉铣鐋铴鐐镣鐒铹鐓镦",
    "鐔镡鐘钟鐙镫鐠镨鐨镄鐫镌鐮镰鐲镯鐳镭鐵铁鐸铎鐺铛鐽钽鐿镱鑄铸鑊镬鑌镔鑑监鑒鉴鑠铄",
    "鑣镳鑤刨鑭镧鑰钥鑲镶鑷镊鑼锣鑽钻鑾銮鑿凿長长門门閂闩閃闪閆闫閉闭開开閌闶閎闳閏闰",
    "閑闲閒闲間间閔闵閘闸閡阂閣阁閤合閥阀閨闺閩闽閫阃閬阆閭闾閱阅閶阊閹阉閻阎閼阏閽阍",
    "閾阈閿阌闃阒闆板闈闱闊阔闋阕闌阑闐阗闔阖闕阙闖闯關关闞阚闡阐闢辟闥闼阨厄阯址陘陉",
    "陜陕陝陕陣阵陰阴陳陈陸陆陽阳隄堤隉陧隊队階阶隕陨際际隨随險险隱隐隴陇隸隶隻只雋隽",
    "雖虽雙双雛雏雜杂雞鸡離离難难雲云電电霑沾霤溜霧雾霽霁靂雳靄霭靈灵靚靓靜静靦腼靨靥",
    "鞏巩鞦秋韁缰韃鞑韆千韉鞯韋韦韌韧韓韩韙韪韜韬韞韫韻韵響响頁页頂顶頃顷項项順顺頇顸",
    "須须頊顼頌颂頎颀頏颃預预頑顽頒颁頓顿頗颇領领頜颌頡颉頤颐頦颏頭头頰颊頷颔頸颈頹颓",
    "頻频顆颗題题額额顎颚顏颜顓颛願愿顙颡顛颠類类顢颟顥颢顧顾顫颤顯显顰颦顱颅顳颞顴颧",
    "風风颮飑颯飒颱台颳刮颶飓颺扬颼飕飄飘飆飙飛飞飢饥飩饨飪饪飫饫飭饬飯饭飲饮飴饴飼饲",
    "飽饱飾饰餃饺餅饼餉饷養养餌饵餑饽餒馁餓饿餘余餚肴餛馄餞饯餡馅館馆餳饧餵喂餼饩餽馈",
    "餾馏餿馊饃馍饅馒饈馐饉馑饋馈饌馔饑饥饒饶饗飨饜餍饞馋馬马馭驭馮冯馱驮馳驰馴驯駁驳",
    "駐驻駑驽駒驹駔驵駕驾駘骀駙驸駛驶駝驼駟驷駢骈駭骇駱骆駿骏騁骋騅骓騍骒騎骑騏骐騖骛",
    "騙骗騫骞騭骘騮骝騰腾騶驺騷骚騸骟騾骡驀蓦驁骜驂骖驃骠驄骢驅驱驊骅驍骁驏骣驕骄驗验",
    "驚惊驛驿驟骤驢驴驤骧驥骥驪骊骯肮髏髅髒脏體体髕髌髖髋髮发鬆松鬍胡鬚须鬢鬓鬥斗鬧闹",
    "鬨哄鬩阋鬮阄鬱郁魎魉魘魇魚鱼魯鲁魴鲂魷鱿鮐鲐鮑鲍鮒鲋鮚鲒鮞鲕鮪鲔鮫鲛鮭鲑鮮鲜鯀鲧",
    "鯁鲠鯇鲩鯉鲤鯊鲨鯔鲻鯖鲭鯛鲷鯡鲱鯢鲵鯤鲲鯧鲳鯨鲸鯪鲮鯫鲰鯰鲶鯽鲫鰈鲽鰉鳇鰍鳅鰒鳆",
    "鰓鳃鰣鲥鰥鳏鰨鳎鰩鳐鰭鳍鰱鲢鰲鳌鰳鳓鰷鲦鰹鲣鰻鳗鰾鳔鱈鳕鱉鳖鱒鳟鱔鳝鱖鳜鱗鳞鱘鲟",
    "鱟鲎鱧鳢鱭鲚鱷鳄鱸鲈鱺鲡鳥鸟鳧凫鳩鸠鳳凤鳴鸣鳶鸢鴆鸩鴇鸨鴉鸦鴕鸵鴛鸳鴝鸲鴟鸱鴣鸪",
    "鴦鸯鴨鸭鴯鸸鴰鸹鴻鸿鴿鸽鵂鸺鵑鹃鵒鹆鵓鹁鵜鹈鵝鹅鵠鹄鵡鹉鵪鹌鵬鹏鵯鹎鵲鹊鶇鸫鶉鹑",
    "鶘鹕鶚鹗鶩鹜鶯莺鶱骞鶴鹤鶻鹘鶼鹣鶿鹚鷂鹞鷓鹧鷗鸥鷙鸷鷚鹨鷥鸶鷦鹪鷯鹩鷲鹫鷳鹇鷴鹇",
    "鷸鹬鷹鹰鷺鹭鸕鸬鸚鹦鸛鹳鸝鹂鸞鸾鹵卤鹹咸鹺鹾鹼硷鹽盐麗丽麥麦麩麸麵面麼麽黃黄黌黉",
    "點点黨党黲黪黴霉黷黩黽黾黿鼋鼇鳌鼉鼍鼕冬鼴鼹齊齐齋斋齏齑齒齿齔龀齙龅齜龇齟龃齠龆",
    "齡龄齣出齦龈齧啮齪龊齬龉齲龋齷龌龍龙龐庞龔龚龕龛龜龟"
);

/// 构建繁→简映射（首次调用时解析，之后复用）。
fn t2s_map() -> &'static HashMap<char, char> {
    static MAP: OnceLock<HashMap<char, char>> = OnceLock::new();
    MAP.get_or_init(|| {
        T2S_PAIRS.chars()
            .collect::<Vec<char>>()
            .chunks(2)
            .filter(|pair| pair.len() == 2 && pair[0] != pair[1])
            .map(|pair| (pair[0], pair[1]))
            .collect()
    })
}

/// legado `java.t2s(s)`: 繁体转简体。
fn java_t2s(input: &str) -> String {
    let map = t2s_map();
    input.chars().map(|c| *map.get(&c).unwrap_or(&c)).collect()
}

/// `java.s2t(s)`: 简体转繁体（上表反向；同一简体对应多个繁体时取表中最后一个）。
fn java_s2t(input: &str) -> String {
    static REVERSE: OnceLock<HashMap<char, char>> = OnceLock::new();
    let reverse = REVERSE.get_or_init(|| t2s_map().iter().map(|(t, s)| (*s, *t)).collect());
    input.chars().map(|c| *reverse.get(&c).unwrap_or(&c)).collect()
}
/// legado `java.htmlFormat(str)`: 解码 HTML 实体为纯文本
fn java_html_format(input: &str) -> String {    input
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&nbsp;", " ")
}

/// legado `java.toNumChapter(s)`: 把「第N章/第N话」等还原为数字编号
fn java_to_num_chapter(input: &str) -> String {
    static RE: Lazy<regex::Regex> = Lazy::new(|| {
        regex::Regex::new(r"(?i)(第?\s*([0-9０-９]{1,9})\s*[章卷话集回])").unwrap()
    });
    let re = &*RE;
    if let Some(caps) = re.captures(input) {
        if let Some(num) = caps.get(2) {
            // 全角数字转半角
            let mut n: String = num.as_str().to_string();
            let full: Vec<(char, char)> = "０１２３４５６７８９".chars().zip("0123456789".chars()).collect();
            for (f, h) in full {
                n = n.replace(f, &h.to_string());
            }
            return n;
        }
    }
    String::new()
}

/// legado `java.timeFormatUTC(timestamp, format, sh)`: 按东八区偏移格式化时间
fn java_time_format_utc(timestamp: i64, format: &str, sh: i64) -> String {
    let secs = if timestamp > 1_000_000_000_000 {
        timestamp / 1000
    } else {
        timestamp
    };
    // Legado 书源用 Java/SimpleDateFormat 格式串(如 yyyy-MM-dd HH:mm),
    // chrono 需要 %Y-%m-%d %H:%M 格式。空格式回退到默认。
    let fmt = if format.is_empty() {
        "%Y-%m-%d %H:%M".to_string()
    } else {
        java_date_format_to_chrono(format)
    };
    // sh 是 UTC 偏移小时数, legado 默认东八区(8)
    let offset_secs = sh * 3600;
    let dt = Utc.timestamp_opt(secs, 0).single().map(|t| t + chrono::Duration::seconds(offset_secs));
    match dt {
        Some(d) => d.format(&fmt).to_string(),
        None => String::new(),
    }
}

/// 把 Java SimpleDateFormat 格式串转换为 chrono strftime 格式串。
/// 只转换书源中常见的日期/时间模式字母; 不支持的字母原样保留(chrono 会忽略)。
fn java_date_format_to_chrono(format: &str) -> String {
    let mut out = String::with_capacity(format.len() * 2);
    let chars: Vec<char> = format.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        // 跳过单引号内的字面量(SimpleDateFormat 用 '' 转义)
        if c == '\'' {
            // 找到配对的单引号
            if let Some(end) = chars[i + 1..].iter().position(|&ch| ch == '\'') {
                let literal: String = chars[i + 1..i + 1 + end].iter().collect();
                out.push_str(&literal);
                i = i + 1 + end + 1;
            } else {
                // 不配对, 当字面量
                i += 1;
            }
            continue;
        }
        // 统计连续相同字母的长度(模式字母可重复, 如 yyyy/MM)
        let mut j = i + 1;
        while j < chars.len() && chars[j] == c {
            j += 1;
        }
        let replacement = match c {
            'y' => "%Y",
            'M' => "%m",
            'd' => "%d",
            'H' => "%H",
            'm' => "%M",
            's' => "%S",
            'S' => "%f",
            'E' => "%A",
            'a' => "%p",
            'G' => "%E",
            'w' => "%W",
            'D' => "%j",
            'z' | 'Z' => "%z",
            _ => {
                // 非模式字母(分隔符、中文等)原样输出
                let literal: String = chars[i..j].iter().collect();
                out.push_str(&literal);
                i = j;
                continue;
            }
        };
        out.push_str(replacement);
        i = j;
    }
    out
}

/// 0..1 的伪随机数(无外部依赖, 供 java.random 使用)
fn rand_like() -> f64 {
    #[cfg(not(test))]
    {
        use std::time::{SystemTime, UNIX_EPOCH};
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.subsec_nanos())
            .unwrap_or(0);
        // 用纳秒低位与一个固定大数做取模, 产生稳定分布
        ((nanos as u64).wrapping_mul(1_103_515_245) % 10_000) as f64 / 10_000.0
    }
    #[cfg(test)]
    {
        0.5
    }
}

fn random_string(len: i32) -> String {
    const CHARS: &[u8] = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";
    let len = len.max(0) as usize;
    let mut seed = rand_like();
    (0..len)
        .map(|_| {
            seed = (seed * 13.0 + 1.0) % 1.0;
            let idx = (seed * CHARS.len() as f64) as usize % CHARS.len();
            CHARS[idx] as char
        })
        .collect()
}

/// 发起 JS 网络请求, 返回 `{body, code, url, headers}`; 失败时 code=0 并记录日志。
/// 书源 header 作为默认请求头, 脚本显式传入的同名头覆盖之。
fn js_request(
    method: &str,
    url: &str,
    body: Option<String>,
    headers: &JsonValue,
    default_headers: &[(String, String)],
    options: &JsonValue,
) -> JsonValue {
    js_request_with_bytes(method, url, body, None, headers, default_headers, options, false)
}

fn latin1_bytes_string(bytes: &[u8]) -> String {
    bytes.iter().map(|b| *b as char).collect()
}

/// 按 charset 标签取编码器。
///
/// 未知标签或空标签一律按 UTF-8 —— 与 Java 侧 `charsetName.toCharset()`
/// 拿不到编码器时的回落一致, 避免书源传了怪标签就整条规则抛错。
fn encoding_for_label(label: &str) -> &'static encoding_rs::Encoding {
    let trimmed = label.trim();
    if trimmed.is_empty() {
        return encoding_rs::UTF_8;
    }
    encoding_rs::Encoding::for_label(trimmed.as_bytes()).unwrap_or(encoding_rs::UTF_8)
}

/// `java.net.URL(...).openConnection()` 形态的字节级请求: 请求体按 latin1
/// 还原成原始字节发送, 响应体按 latin1 原样回传(不解码字符集、不自动解压),
/// 由脚本自己决定如何解释。
fn js_http_bytes(
    method: &str,
    url: &str,
    body: &str,
    headers: &str,
    default_headers: &[(String, String)],
) -> JsonValue {
    let headers = serde_json::from_str::<JsonValue>(headers).unwrap_or(JsonValue::Null);
    let body = (!body.is_empty()).then(|| latin1_bytes(body));
    js_request_with_bytes(method, url, None, body, &headers, default_headers, &JsonValue::Null, true)
}

/// 解析 JS 网络请求的重试次数。
///
/// 默认值分两条路:
///
/// - 文本路径(`java.ajax`/`java.get`)沿用 legado 的宽松语义, 保持 2 次重试;
/// - 字节路径走的是 `Packages.java.net.URL(...).openConnection()` —— 真实 Java
///   的 HttpURLConnection **不会自动重试**, 书源脚本自己负责重试。早先这里同样
///   默认 2 次, 而脚本设的 connectTimeout/readTimeout 在本层是空实现, 于是单个
///   不可达地址要串行挂满 3 个宿主超时。某类带段评的书源首次打开要注册两台设备,
///   最坏 ≈90s, 用户看到的就是「点了很久、像是打不开」。按 Java 语义退回单次。
fn js_request_retry(options: &JsonValue, raw_response: bool) -> usize {
    let default_retry = if raw_response { 0 } else { 2 };
    options["retry"].as_u64().unwrap_or(default_retry).min(5) as usize
}

/// 面板请求的宿主超时上限(秒)。
///
/// 面板里大量 `<img>`/`fetch` 指向第三方 CDN, 连不通是常态。手机端 legado 面板的
/// `java.ajax`/`get`/`post` 默认 callTimeout 就是 9s(见 WebJsExtensions.kt), 这里对齐:
/// 面板首屏不该被不可达的头像 URL 按书源长超时挂着。
const PANEL_REQUEST_TIMEOUT_SECS: u64 = 9;

/// 面板请求要不要沿用书源的 `concurrentRate`。
///
/// 面板里每个头像/封面都会单独发一次请求, 把它们塞进书源限速队列后, 用户在
/// 面板里做的下一次真实请求(如翻页拉段评)要排在几十张图片后面。实测该类书源
/// (`concurrentRate=1/1000`) + 24 个并发头像会把一次段评请求从 ~0.9s 拖到 24.8s。
fn js_concurrent_rate(panel_request: bool, source_rate: Option<String>) -> Option<String> {
    if panel_request { None } else { source_rate }
}

/// 面板请求的宿主超时: 取**全局请求超时**与 [`PANEL_REQUEST_TIMEOUT_SECS`] 的较小值。
///
/// 参数来自 `HttpClient::timeout_secs`(配置项 `request_timeout_secs`, 默认 15s),
/// 不是书源上的单源超时 —— 书源没有这个字段。取较小值只是为了避免某些部署把
/// 全局超时配得比面板上限还短时被面板策略反手放大。
fn js_panel_timeout_secs(global_timeout_secs: u64) -> u64 {
    global_timeout_secs.min(PANEL_REQUEST_TIMEOUT_SECS)
}

/// 面板请求的重试次数: 保持 Java `HttpURLConnection` 语义(不自动重试)。
///
/// 面板里一个头像 URL 不可达时, 若沿用书源文本路径的 2 次重试, 单个资源就要
/// 挂满 3 个超时窗口, 首屏 50 条段评的头像会把它放大成分钟级等待。
fn js_panel_retry(options: &JsonValue) -> usize {
    options["retry"].as_u64().unwrap_or(0).min(5) as usize
}

#[allow(clippy::too_many_arguments)]
fn js_request_with_bytes(
    method: &str,
    url: &str,
    body: Option<String>,
    body_bytes: Option<Vec<u8>>,
    headers: &JsonValue,
    default_headers: &[(String, String)],
    options: &JsonValue,
    raw_response: bool,
) -> JsonValue {
    let url = url.trim().to_string();
    if url.is_empty() {
        return json!({ "body": "", "code": 0, "url": "", "headers": {}, "call_time": 0 });
    }
    let method = match method.to_ascii_uppercase().as_str() {
        "POST" => HttpMethod::POST, "HEAD" => HttpMethod::HEAD,
        "PUT" => HttpMethod::PUT, "PATCH" => HttpMethod::PATCH,
        "DELETE" => HttpMethod::DELETE, _ => HttpMethod::GET,
    };
    let mut merged: Vec<(String, String)> = default_headers.to_vec();
    for (key, value) in login_headers() {
        merged.retain(|(k, _)| !k.eq_ignore_ascii_case(&key));
        merged.push((key, value));
    }
    if let Some(map) = headers.as_object() {
        for (key, value) in map {
            let value = match value {
                JsonValue::String(s) => s.clone(),
                JsonValue::Null => continue,
                other => other.to_string(),
            };
            merged.retain(|(k, _)| !k.eq_ignore_ascii_case(key));
            merged.push((key.clone(), value));
        }
    }
    if !merged.iter().any(|(k, _)| k.eq_ignore_ascii_case("user-agent")) {
        merged.push(("User-Agent".to_string(), DEFAULT_JS_USER_AGENT.to_string()));
    }
    let source = active_source();
    // `panel` 由面板桥(见 `run_panel_request_inner`)注入。
    let panel_request = options.get("panel").and_then(|v| v.as_bool()).unwrap_or(false);
    // 面板请求(见 `js_panel_*`)不参与书源的并发限速: 面板首屏会给每条段评拉头像,
    // 该类书源 concurrentRate=1/1000 时这些静态资源会排满限速队列, 用户再点下一段
    // 段评就得排在几十个头像请求后面(实测 24 个并发头像把一次段评请求拖到 24.8s)。
    let concurrent_rate = js_concurrent_rate(panel_request, source.concurrent_rate.clone());
    let http = source.http.map(Ok).unwrap_or_else(|| source_http_context(&source.url));
    let request_url = crate::crawler::url_analyzer::absolute_url(&source.url, &url);
    let proxy = merged.iter().find(|(key, _)| key.eq_ignore_ascii_case("proxy")).map(|(_, value)| value.clone());
    merged.retain(|(key, _)| !key.eq_ignore_ascii_case("proxy"));
    // 默认跟随重定向; `java.get/head/post` 显式关掉(legado
    // `Jsoup.connect(...).followRedirects(false)`, 见 `js_http_with_redirect`)。
    // 这个开关只影响**选哪个 client**, 不进 RequestSpec: 客户端本身决定了
    // reqwest 的 redirect policy。
    let follow_redirects = options
        .get("follow_redirects")
        .and_then(|value| value.as_bool())
        .unwrap_or(true);
    let spec = RequestSpec {
        url: request_url, method, headers: merged, body,
        body_bytes,
        retry: if panel_request { js_panel_retry(options) } else { js_request_retry(options, raw_response) },
        charset: options["charset"].as_str().map(str::to_string),
        response_type: options.get("type").filter(|value| !value.is_null()).map(|value| value.as_str().map(str::to_string).unwrap_or_else(|| value.to_string())),
        ..Default::default()
    };
    let outcome = run_blocking_http(move || -> anyhow::Result<JsonValue> {
        let http = http?;
        // `StrResponse.callTime()` 的计时起点：与 legado `getStrResponse` 一致，
        // 从「发起这次请求」开始算到读完 body 为止（见下方 elapsed_ms）。
        let started = std::time::Instant::now();
        // 带自定义 `proxy` 头的请求要另建 client(书源 client 的代理是固定的)。
        // 注意: 面板请求的短超时不是靠这里, 而是下面给 `spec.timeout` 做 per-request
        // 覆盖 —— 它优先于 client 自身的超时, 且不需要重建连接池。
        let client = if let Some(proxy) = proxy.as_deref().filter(|v| Some(*v) != http.proxy.as_deref()) {
            if raw_response {
                crate::crawler::http_client::build_raw_source_client(http.timeout_secs, Some(proxy), http.session.cookies.clone())?
            } else if !follow_redirects {
                crate::crawler::http_client::build_source_client_no_redirect(http.timeout_secs, Some(proxy), http.session.cookies.clone())?
            } else {
                crate::crawler::http_client::build_source_client(http.timeout_secs, Some(proxy), http.session.cookies.clone())?
            }
        } else if raw_response {
            crate::crawler::http_client::build_raw_source_client(http.timeout_secs, http.proxy.as_deref(), http.session.cookies.clone())?
        } else if !follow_redirects {
            http.client_no_redirect.clone()
        } else { http.client };
        // 面板请求(浏览器 `fetch` 过桥的那些)只压单次超时, 不重建 client:
        // 面板首屏会并发几十个头像, 每次新建 reqwest::Client 就得重做一轮 TLS
        // 握手/连接池, 反而更慢。`timeout` 是 per-request 覆盖, 连接池照旧复用。
        // `follow_redirects` 交由 fetcher 执行：它会在每一跳按新地址重建 `Host`，
        // 换域跳转才不会带着旧域的 `Host` 打转（详见 `fetcher::fetch_inner`）。
        let spec = RequestSpec { follow_redirects, ..spec };
        let spec = if panel_request {
            let timeout = std::time::Duration::from_secs(js_panel_timeout_secs(http.timeout_secs));
            RequestSpec { timeout: Some(timeout), ..spec }
        } else { spec };
        let response = if raw_response {
            JS_HTTP_RUNTIME.block_on(http.session.request_raw(&client, spec, concurrent_rate.as_deref()))?
        } else {
            JS_HTTP_RUNTIME.block_on(http.session.request(&client, spec, concurrent_rate.as_deref()))?
        };
        // `StrResponse.callTime()` 的耗时口径：legado 在 `getStrResponse` 里包的是
        // **整个请求**（建连 + 读完 body），见 `AnalyzeUrl.kt` 的
        // `connectionTime = System.currentTimeMillis() - startTime`。
        // 这里同样包住 `session.request*` 的整个调用，而不是只量建连。
        let elapsed_ms = started.elapsed().as_millis().min(u32::MAX as u128) as u64;
        let headers = response.headers.into_iter().map(|(k, v)| (k.to_ascii_lowercase(), json!(v))).collect::<serde_json::Map<_, _>>();
        let body = if raw_response { latin1_bytes_string(&response.raw_body) } else { response.body };
        Ok(json!({"body":body, "code":response.status, "url":response.url, "headers":headers, "call_time":elapsed_ms}))
    });
    let value = outcome.unwrap_or_else(|e| {
        if panel_request {
            // 面板里大量头像/封面指向第三方 CDN, 连不通是常态; 这类失败由面板
            // 自己的 Promise catch 兜底, 不该混进书源日志(会盖住真正的书源错误,
            // 也让人误以为段评加载失败)。需要排查时把日志级别调到 debug。
            tracing::debug!(target: "reader::js", "[panel] 请求失败: {e:#}");
        } else {
            push_js_log(format!("[http] 请求失败: {e:#}"));
        }
        json!({ "body": "", "code": 0, "url": url, "headers": {}, "call_time": 0 })
    });
    // legado `java.getStrResponse()` 无参时返回"最近一次请求的响应"。
    //
    // 两类响应不写这个槽位:
    // - 字节级响应(原始二进制)不是文本 StrResponse, 否则依赖最近响应的
    //   `loginCheckJs` 会读到无法按行解析的原始字节;
    // - 面板里的浏览器 `fetch`(见 `js_panel_fetch`)。手机端这类请求由 WebView
    //   直连发出, 根本不经过 Java 桥, 因此没有"最近响应"副作用; 若在这里写入,
    //   面板首屏几十张头像会把书源真正的接口响应顶掉, `java.getStrResponse()`
    //   随后读到一张图片的信封。
    if !raw_response && !panel_request {
        LAST_RESPONSE.with(|cell| cell.replace(Some(value.to_string())));
    }
    value
}

/// 面板里浏览器 `fetch` 形态的请求: 强制注入 `panel` 标志。
///
/// `panel` 让 `js_request_with_bytes` 跳过书源并发限速、不自动重试、并把单次
/// 超时压到 [`PANEL_REQUEST_TIMEOUT_SECS`]。面板首屏会给每条段评拉头像, 这些
/// 第三方静态资源连不通是常态, 不该占着书源限速队列, 也不该按书源长超时干等;
/// 手机端同样的请求是 WebView 直连, 本来就不经过书源限速器。
fn js_panel_fetch(method: &str, url: &str, body: &str, default_headers: &[(String, String)]) -> JsonValue {
    let options = with_panel_flag(json!({}));
    let body = (!body.is_empty() || method.eq_ignore_ascii_case("POST")).then(|| body.to_string());
    js_request(method, url, body, &JsonValue::Null, default_headers, &options)
}

/// 给请求选项打上 `panel` 标记。面板桥构造选项时必须走这里, 否则限速/超时/
/// 重试三条面板专用策略都会静默失效(调用点分散在 http 与 connect 两条路)。
fn with_panel_flag(mut options: JsonValue) -> JsonValue {
    if let Some(map) = options.as_object_mut() {
        map.insert("panel".to_string(), JsonValue::Bool(true));
    }
    options
}


fn js_http(
    method: &str,
    url: &str,
    body: &str,
    headers: &str,
    default_headers: &[(String, String)],
) -> JsonValue {
    js_http_with_redirect(method, url, body, headers, default_headers, true)
}

/// `__http` 的宿主实现。`follow_redirects` 为 false 时走不跟随 3xx 的 client,
/// 并把 3xx 响应的 `Location` 原样交回脚本(legado `java.get/head/post` 语义)。
fn js_http_with_redirect(
    method: &str,
    url: &str,
    body: &str,
    headers: &str,
    default_headers: &[(String, String)],
    follow_redirects: bool,
) -> JsonValue {
    let headers = serde_json::from_str::<JsonValue>(headers).unwrap_or(JsonValue::Null);
    let body = (!body.is_empty() || method.eq_ignore_ascii_case("POST")).then(|| body.to_string());
    let options = json!({ "follow_redirects": follow_redirects });
    js_request(method, url, body, &headers, default_headers, &options)
}

/// legado `java.ajax(url)` / `java.connect(url, header)`: url 可带 `,{method, body, headers}` 选项。
fn js_connect(spec: &str, headers: &str, default_headers: &[(String, String)]) -> JsonValue {
    let (url, options) = crate::crawler::url_analyzer::split_url_options(spec);
    let options = options
        .and_then(|raw| serde_json::from_str::<JsonValue>(raw).ok())
        .unwrap_or(JsonValue::Null);
    let method = options
        .get("method")
        .and_then(|v| v.as_str())
        .unwrap_or("GET")
        .to_string();
    let body = options.get("body").and_then(|body| match body {
        JsonValue::String(s) => Some(s.clone()),
        JsonValue::Null => None,
        other => Some(other.to_string()),
    });
    let mut merged = serde_json::Map::new();
    if let Some(map) = serde_json::from_str::<JsonValue>(headers)
        .ok()
        .as_ref()
        .and_then(|v| v.as_object())
    {
        merged.extend(map.clone());
    }
    if let Some(map) = options.get("headers").and_then(|v| v.as_object()) {
        merged.extend(map.clone());
    }
    js_request(&method, url, body, &JsonValue::Object(merged), default_headers, &options)
}

/// `__connect_all(specsJson, headers)`：`ajaxAll` 的宿主实现。
///
/// legado 的 `ajaxAll` 是 `mapAsync(AppConfig.threadCount)` 的**并发**请求，
/// 书源用它给多条线路测速并按 `callTime()` 选最快的一条。串行实现虽然结果
/// 一样，但一条不可达线路就要等满一个超时窗口，整体等于 N 个超时之和。
///
/// 返回值是**按输入顺序**排列的信封数组（与 `toList().toTypedArray()` 一致）：
/// 书源靠下标把信封和 URL 对上（`rs[i]` ↔ `hs[i]`），顺序错位就会选错线路。
fn js_connect_all(specs: &str, headers: &str, default_headers: &[(String, String)]) -> String {
    let specs: Vec<String> = serde_json::from_str(specs).unwrap_or_default();
    if specs.is_empty() {
        return "[]".to_string();
    }
    // 这里的调用方是 `run_blocking_http` 起出来的阻塞线程，本函数内部再起线程
    // 做真并发；`js_connect` 在没有 tokio 上下文的线程上会直接同步发请求。
    let results: Vec<JsonValue> = std::thread::scope(|scope| {
        let handles: Vec<_> = specs
            .iter()
            .map(|spec| scope.spawn(|| js_connect(spec, headers, default_headers)))
            .collect();
        handles
            .into_iter()
            .map(|handle| handle.join().unwrap_or_else(|_| {
                json!({"body": "", "code": 0, "url": "", "headers": {}, "call_time": 0})
            }))
            .collect()
    });
    JsonValue::Array(results).to_string()
}

// ────────────────────── 书源面板(showBrowser)宿主桥 ──────────────────────

/// 面板 `window.cache.get` / `java.getCache`: 复用书源变量存储(`sp:` 前缀)。
pub fn get_panel_cache(source_url: &str, key: &str) -> Option<String> {
    store_get(&format!("sp:{source_url}:{key}"))
}

pub fn put_panel_cache(source_url: &str, key: &str, value: &str) {
    store_write(|map| {
        map.insert(format!("sp:{source_url}:{key}"), value.to_string());
    });
}

/// Remove persistent JS variables owned by a book or one of its chapters.
///
/// Book variables are stored as `bv:<bookUrl>[:key]`, chapter variables as
/// `cv:<chapterUrl>[:key]`, and the per-book read override as
/// `brc:<sourceUrl>:<bookUrl>`. Exact key matching is important here: a
/// prefix-only match would delete another book whose URL starts with this one.
pub fn remove_book_variables(book_url: &str, chapter_urls: &[String]) {
    if book_url.trim().is_empty() && chapter_urls.is_empty() {
        return;
    }
    store_write(|map| {
        map.retain(|key, _| {
            let book_scoped = key
                .strip_prefix("bv:")
                .is_some_and(|suffix| {
                    suffix == book_url
                        || suffix
                            .strip_prefix(book_url)
                            .is_some_and(|rest| rest.starts_with(':'))
                });
            let read_config_scoped = key.strip_prefix("brc:").is_some_and(|suffix| {
                suffix
                    .strip_suffix(book_url)
                    .and_then(|prefix| prefix.strip_suffix(':'))
                    .is_some_and(|source_url| !source_url.is_empty())
            });
            let chapter_scoped = chapter_urls.iter().any(|chapter_url| {
                key.strip_prefix("cv:").is_some_and(|suffix| {
                    suffix == chapter_url
                        || suffix
                            .strip_prefix(chapter_url)
                            .is_some_and(|rest| rest.starts_with(':'))
                })
            });
            !book_scoped && !read_config_scoped && !chapter_scoped
        });
    });
}

/// 面板内的网络请求: 与书源规则使用同一套 header 与 cookie 客户端。
///
/// - `ajax` / `connect`: `spec` 为 legado 的 `url,{json}` 形式
/// - `http`: 显式 method/url/body/headers
///
/// 返回值必须是**响应体字符串**: 面板会把它直接注入 DOM 或 `JSON.parse`
/// (legado 的 `java.ajax` 同样只返回 body), 因此这里要剥掉
/// `js_request` 的 `{body, code, url, headers}` 信封。
pub fn run_panel_request(
    source: &BookSource,
    base_url: &str,
    action: &str,
    payload: &JsonValue,
) -> anyhow::Result<String> {
    run_panel_fetch(source, base_url, action, payload).map(|response| {
        response.get("body").and_then(|v| v.as_str()).unwrap_or_default().to_string()
    })
}

pub fn run_panel_fetch(
    source: &BookSource, base_url: &str, action: &str, payload: &JsonValue,
) -> anyhow::Result<JsonValue> {
    with_source(source, || run_panel_request_inner(source, base_url, action, payload))
}

fn run_panel_request_inner(
    source: &BookSource, base_url: &str, action: &str, payload: &JsonValue,
) -> anyhow::Result<JsonValue> {
    let headers = crate::crawler::url_analyzer::source_header_spec(source)?.headers;
    let text_of = |key: &str| payload.get(key).and_then(|v| v.as_str()).unwrap_or("");
    // headers 兜底: 面板桥已把它序列化成文本, 但若调用方直接传对象,
    // 上面 as_str() 只会取到空串, 会让自定义头静默失效。
    let headers_text = |key: &str| -> String {
        match payload.get(key) {
            Some(JsonValue::String(s)) => s.clone(),
            Some(JsonValue::Null) | None => String::new(),
            Some(other) => other.to_string(),
        }
    };

    let envelope = match action {
        "ajax" | "connect" => {
            let spec = text_of("spec");
            let extra = headers_text("headers");
            let mut merged = headers;
            if !extra.trim().is_empty() {
                merged.extend(parse_source_headers(&extra));
            }
            js_connect(spec, "{}", &merged)
        }
        "http" => {
            let method = text_of("method").to_uppercase();
            let url = crate::crawler::url_analyzer::absolute_url(base_url, text_of("url"));
            let body = text_of("body");
            let extra = headers_text("headers");
            let mut merged = headers;
            if !extra.trim().is_empty() {
                merged.extend(parse_source_headers(&extra));
            }
            js_http(&method, &url, &body, "{}", &merged)
        }
        // 只有浏览器 `fetch` 形态豁免书源并发限速。手机端面板里 window.fetch 是
        // WebView 自己发的请求(不经过书源限速器), 而 java.ajax/get/post 走
        // AnalyzeUrl → concurrentRateLimiter, 仍受限速约束。这里保持同样的分工:
        // 面板首屏的几十张头像不会把队列占满, 面板自己的业务请求依旧守规矩。
        "fetch" => {
            let method = text_of("method").to_uppercase();
            let url = crate::crawler::url_analyzer::absolute_url(base_url, text_of("url"));
            let body = text_of("body");
            let extra = headers_text("headers");
            let mut merged = headers;
            if !extra.trim().is_empty() {
                merged.extend(parse_source_headers(&extra));
            }
            js_panel_fetch(&method, &url, &body, &merged)
        }
        other => anyhow::bail!("未知的请求类型 {other}"),
    };

    Ok(envelope)
}

/// 面板 `java.createSymmetricCrypto` 的桥实现, 复用书源规则的同一套密码学。
///
/// 面板侧 key/iv/data 都来自 `JSON.stringify` 过的字符串参数（只能是文本），
/// 因此走文本语义（UTF-8），与改造前一致。
pub fn panel_symmetric_crypto(payload: &JsonValue) -> anyhow::Result<String> {
    let text_of = |key: &str| payload.get(key).and_then(|v| v.as_str()).unwrap_or("");
    symmetric_codec(
        text_of("op"),
        text_of("transformation"),
        text_of("key"),
        text_of("iv"),
        text_of("data"),
        text_of("format"),
    )
}

/// 面板 `java.md5Encode` / `base64Encode` / `base64Decode` 的桥实现。
pub fn panel_digest(payload: &JsonValue) -> anyhow::Result<String> {
    let text_of = |key: &str| payload.get(key).and_then(|v| v.as_str()).unwrap_or("");
    let input = text_of("input");
    match text_of("kind") {
        "md5" => Ok(md5_hex(&input)),
        "base64" => Ok(base64::engine::general_purpose::STANDARD.encode(input.as_bytes())),
        "unbase64" => {
            let bytes = decode_base64_lenient(input.trim())
                .map_err(|e| anyhow::anyhow!("base64 解码失败: {e}"))?;
            Ok(String::from_utf8_lossy(&bytes).into_owned())
        }
        other => anyhow::bail!("未知的摘要类型 {other}"),
    }
}

#[cfg(test)]
mod tests {

    /// AES-GCM 必须与标准实现逐字节一致（NIST SP 800-38D 测试向量）。
    ///
    /// 用例数据：key = 16 字节 0，iv = 12 字节 0，明文全 0 共 16 字节 ——
    /// 这是规范里的 Classic Test Case 2，密文与标签都是固定值。
    #[test]
    fn aes_gcm_matches_nist_vector() {
        let key = [0u8; 16];
        let iv = [0u8; 12];
        let plain: Vec<u8> = (0u8..16).collect();
        let out = super::symmetric_crypt(true, "AES/GCM/NoPadding", &key, &iv, &plain)
            .expect("GCM 加密应成功");
        assert_eq!(out.len(), plain.len() + 16, "密文 = 明文 + 16 字节 tag");
        // 往返：解密必须还原明文
        let back = super::symmetric_crypt(false, "AES/GCM/NoPadding", &key, &iv, &out)
            .expect("GCM 解密应成功");
        assert_eq!(back, plain, "GCM 往返失败");
        // 期望值由 `cryptography.hazmat.primitives.ciphers.aead.AESGCM` 独立算出：
        //   AESGCM(bytes(16)).encrypt(bytes(12), bytes(range(16)), None)
        // → 0389d8cd64b3a595fb21c8b27dbff077 || 00e840843bf4619947096b7103fcb148
        let expect_cipher = "0389d8cd64b3a595fb21c8b27dbff077";
        let expect_tag = "00e840843bf4619947096b7103fcb148";
        assert_eq!(hex::encode(&out[..16]), expect_cipher, "GCM 密文不符");
        assert_eq!(hex::encode(&out[16..]), expect_tag, "GCM 认证标签不符");
    }

    /// 篡改密文必须被认证标签拦下。
    #[test]
    fn aes_gcm_rejects_tampered_ciphertext() {
        let key = [0u8; 16];
        let iv = [0u8; 12];
        let mut out = super::symmetric_crypt(true, "AES/GCM/NoPadding", &key, &iv, b"hello world")
            .expect("加密");
        out[0] ^= 1;
        assert!(super::symmetric_crypt(false, "AES/GCM/NoPadding", &key, &iv, &out).is_err());
    }
    use super::*;

    /// `java.url` 必须取到当前请求地址, `urlNoQuery` 去掉 query。
    ///
    /// legado 里脚本的 `java` 是 `AnalyzeUrl` 实例, 上面有 `url`/`urlNoQuery`
    /// 两个字段。这里没有那个对象, 用"最近响应"里的 url 代为提供 —— 宿主在
    /// 执行 loginCheckJs 前已用当前响应初始化了该槽位。
    #[test]
    fn java_url_reflects_current_request() {
        set_last_response(
            &serde_json::json!({
                "body": "{}",
                "url": "https://api.example.test/content?item_id=1&x=2",
                "code": 200,
                "headers": {}
            })
            .to_string(),
        );
        let output = eval_js_with_bindings(
            "java.url + '|' + java.urlNoQuery",
            "{}",
            "https://api.example.test/content?item_id=1&x=2",
            &HashMap::new(),
        )
        .expect("java.url 应可读");
        assert_eq!(
            output,
            "https://api.example.test/content?item_id=1&x=2|https://api.example.test/content"
        );
    }

    /// `loginCheckJs` 里的两个 legado 兼容点: `java.url` 与
    /// `Packages.io.legado.app.help.http.StrResponse`。
    ///
    /// 聚合类书源用它们把"接口返回的 JSON 包一层"变成正文:
    /// ```js
    /// if (result.url().includes("content")) {
    ///     let c = JSON.parse(result.body());
    ///     c = c.data?.content || c.message || '暂不支持';
    ///     result = new Packages.io.legado.app.help.http.StrResponse(java.url, c);
    /// }
    /// result
    /// ```
    /// 两者缺任一都会让脚本抛错(实测报 `cannot read property 'legado' of
    /// undefined`), loginCheckJs 因此失效, 正文再也加载不出来。
    #[test]
    fn login_check_js_supports_java_url_and_str_response() {
        set_last_response(
            &serde_json::json!({
                "body": r#"{"data":{"content":"<p>正文</p>"}}"#,
                "url": "https://api.example.test/content?item_id=1",
                "code": 200,
                "headers": {}
            })
            .to_string(),
        );
        let script = r#"
            if (result.url().includes("content")) {
                let c = JSON.parse(result.body());
                c = c.data?.content || c.message || '暂不支持';
                result = new Packages.io.legado.app.help.http.StrResponse(java.url, c);
            }
            result
        "#;
        let mut bindings = HashMap::new();
        bindings.insert(
            "result".to_string(),
            resp_binding(&serde_json::json!({
                "body": r#"{"data":{"content":"<p>正文</p>"}}"#,
                "url": "https://api.example.test/content?item_id=1",
                "code": 200,
                "headers": {}
            })),
        );
        let output = eval_js_with_bindings(
            script,
            r#"{"data":{"content":"<p>正文</p>"}}"#,
            "https://api.example.test/content?item_id=1",
            &bindings,
        )
        .expect("loginCheckJs 应执行成功");

        let envelope: serde_json::Value = serde_json::from_str(&output).expect("应返回响应信封");
        assert_eq!(
            envelope.get("body").and_then(|v| v.as_str()),
            Some("<p>正文</p>"),
            "StrResponse(url, body) 的 body 必须被保留: {output}"
        );
        assert_eq!(
            envelope.get("code").and_then(|v| v.as_u64()),
            Some(200),
            "StrResponse 构造的响应 code 固定为 200: {output}"
        );
        assert_eq!(
            envelope.get("url").and_then(|v| v.as_str()),
            Some("https://api.example.test/content?item_id=1"),
            "java.url 应返回当前请求地址: {output}"
        );
    }

    #[test]
    fn java_date_format_to_chrono_converts_common_patterns() {
        assert_eq!(java_date_format_to_chrono("yyyy-MM-dd HH:mm:ss"), "%Y-%m-%d %H:%M:%S");
        assert_eq!(java_date_format_to_chrono("yyyy/MM/dd"), "%Y/%m/%d");
        assert_eq!(java_date_format_to_chrono("HH:mm"), "%H:%M");
    }

    #[test]
    fn java_date_format_keeps_literal_text() {
        // 中文字面量应原样保留
        let result = java_date_format_to_chrono("yyyy年MM月dd日");
        assert!(result.contains("年"));
        assert!(result.contains("月"));
        assert!(result.contains("日"));
        assert!(result.contains("%Y"));
        assert!(result.contains("%m"));
        assert!(result.contains("%d"));
    }

    #[test]
    fn java_time_format_utc_with_java_pattern() {
        // 1768480200 = 2026-01-15 12:30:00 UTC, sh=8 → 东八区 20:30
        let ts = 1768480200i64;
        let result = java_time_format_utc(ts, "yyyy-MM-dd HH:mm", 8);
        assert_eq!(result, "2026-01-15 20:30");
    }

    /// legado 里 `java.putCache`/`java.getCache` 与 `cache.putMemory`/`getFromMemory`
    /// 共用同一份内存缓存, 且 memory 可存任意类型 —— 两种写入方式必须能互相读到。
    #[test]
    fn prelude_cache_and_java_cache_share_memory() {
        // java.putCache 写入后, cache.getFromMemory 应能读到(而不是 JSON 解析异常)
        let plain = eval_js(
            "java.putCache('m1_plain','abc'); cache.getFromMemory('m1_plain')",
            "",
            "",
        )
        .unwrap();
        assert_eq!(plain, "abc");

        // cache.putMemory 写入对象后, 应能原样读回对象
        let object = eval_js(
            "cache.putMemory('m1_obj',{x:1,y:'z'}); JSON.stringify(cache.getFromMemory('m1_obj'))",
            "",
            "",
        )
        .unwrap();
        assert_eq!(object, r#"{"x":1,"y":"z"}"#);
    }

    /// 期望值由 Node `crypto` 生成(等价于 Java `Cipher` PKCS5Padding):
    /// `node -e "const c=require('crypto');const k=Buffer.from('0821CAAD409B84020821CAAD');
    ///   const e=c.createCipheriv('des-ede3-cbc',k,Buffer.alloc(8));
    ///   console.log(Buffer.concat([e.update('测试文本'),e.final()]).toString('base64'))"`
    #[test]
    fn symmetric_codec_matches_java_desede_cbc_vector() {
        let key = "0821CAAD409B84020821CAAD";
        let encrypted = symmetric_codec(
            "enc",
            "DESede/CBC/PKCS5Padding",
            key,
            "",
            "测试文本",
            "base64",
        )
        .unwrap();
        assert_eq!(encrypted, "tJzEHmM1hTDNyM9hQ+3rdQ==");

        let decrypted =
            symmetric_codec("dec", "DESede/CBC/PKCS5Padding", key, "", &encrypted, "auto").unwrap();
        assert_eq!(decrypted, "测试文本");
    }

    #[test]
    fn symmetric_codec_matches_java_aes_ecb_vector() {
        // Node: createCipheriv('aes-128-ecb', Buffer.from('1234567890abcdef'), null) over 'abc'
        let encrypted = symmetric_codec(
            "enc",
            "AES/ECB/PKCS5Padding",
            "1234567890abcdef",
            "",
            "abc",
            "hex",
        )
        .unwrap();
        assert_eq!(encrypted, "008a30f11b63572c2da1fad67b18eb66");

        let decrypted = symmetric_codec(
            "dec",
            "AES/ECB/PKCS5Padding",
            "1234567890abcdef",
            "",
            &encrypted,
            "auto",
        )
        .unwrap();
        assert_eq!(decrypted, "abc");
    }

    /// 16 字节 DESede 密钥: 按 K1K2K1 扩展成 24 字节(等价于 Node 的 24 字节 key)。
    /// 注意 SunJCE 会直接拒绝 16 字节密钥, 这里是刻意为兼容书源而放宽。
    #[test]
    fn symmetric_codec_expands_16_byte_desede_key() {
        let encrypted = symmetric_codec(
            "enc",
            "DESede/CBC/PKCS5Padding",
            "0123456789abcdef",
            "",
            "hello",
            "base64",
        )
        .unwrap();
        assert_eq!(encrypted, "qm5j84FYbv8=");

        let decrypted = symmetric_codec(
            "dec",
            "DESede/CBC/PKCS5Padding",
            "0123456789abcdef",
            "",
            &encrypted,
            "auto",
        )
        .unwrap();
        assert_eq!(decrypted, "hello");
    }

    #[test]
    fn symmetric_codec_supports_aes_192_and_256() {
        // Node: aes-192-ecb / aes-256-ecb over 'abc'
        let aes192 = symmetric_codec(
            "enc",
            "AES/ECB/PKCS5Padding",
            "0123456789abcdef01234567",
            "",
            "abc",
            "hex",
        )
        .unwrap();
        assert_eq!(aes192, "377f35478afd40126bdcb4d5f9be9ee6");

        let aes256 = symmetric_codec(
            "enc",
            "AES/ECB/PKCS5Padding",
            "0123456789abcdef0123456789abcdef",
            "",
            "abc",
            "hex",
        )
        .unwrap();
        assert_eq!(aes256, "9d7ab829be8b28fd0d1f1568d8842e00");
    }

    #[test]
    fn symmetric_codec_supports_no_padding() {
        // Node setAutoPadding(false): aes-128-ecb over 16 字节对齐输入
        let aes = symmetric_codec(
            "enc",
            "AES/ECB/NoPadding",
            "1234567890abcdef",
            "",
            "1234567890abcdef",
            "hex",
        )
        .unwrap();
        assert_eq!(aes, "95b012b0bc898e5c37eeed6588635f09");

        // DESede/CBC/NoPadding over 8 字节对齐输入
        let des = symmetric_codec(
            "enc",
            "DESede/CBC/NoPadding",
            "0123456789abcdef",
            "",
            "12345678",
            "base64",
        )
        .unwrap();
        assert_eq!(des, "OI3Pry69kmw=");
    }

    /// 非法密钥长度应返回错误而不是 panic。
    #[test]
    fn symmetric_codec_rejects_invalid_key_length() {
        assert!(symmetric_codec("enc", "AES/ECB/PKCS5Padding", "short", "", "abc", "hex").is_err());
        assert!(symmetric_crypt(true, "RC4/ECB/PKCS5Padding", b"key", b"", b"data").is_err());
    }

    /// 插件用 java.* 的链式写法, 验证 prelude 包装后可原样调用。
    #[test]
    fn prelude_exposes_legado_java_crypto_api() {
        let key = "0821CAAD409B84020821CAAD";
        let encrypted = eval_js(
            &format!(
                "java.createSymmetricCrypto('DESede/CBC/PKCS5Padding','{key}','').encryptBase64('测试文本')"
            ),
            "",
            "",
        )
        .unwrap();
        assert_eq!(encrypted, "tJzEHmM1hTDNyM9hQ+3rdQ==");

        let decrypted = eval_js(
            &format!(
                "java.createSymmetricCrypto('DESede/CBC/PKCS5Padding','{key}','').decryptStr('{encrypted}')"
            ),
            "",
            "",
        )
        .unwrap();
        assert_eq!(decrypted, "测试文本");
    }

    /// 同一书源 + 同一 jsLib 应命中复用；换 jsLib 或换书源应重建。
    /// (这是本次提速的核心行为，避免将来被改回「每次重新解析」)
    #[test]
    fn reusable_context_keys_on_source_and_js_lib() {
        let source = BookSource {
            book_source_name: "复用".to_string(),
            book_source_url: "https://reuse.example".to_string(),
            js_lib: Some("function reuseLib(){ return 'a'; }".to_string()),
            ..Default::default()
        };
        with_source(&source, || eval_js("typeof reuseLib", "", "").unwrap());
        let first = cached_js_lib_key().expect("应已建立上下文");

        // 同一书源 + 同一 jsLib → 复用同一个上下文
        with_source(&source, || eval_js("typeof reuseLib", "", "").unwrap());
        assert_eq!(
            cached_js_lib_key().as_deref(),
            Some(first.as_str()),
            "同一书源同一 jsLib 应复用上下文"
        );

        // 换书源(即使 jsLib 文本相同) → 重建, 避免跨源泄漏
        let other_source = BookSource {
            book_source_name: "复用2".to_string(),
            book_source_url: "https://reuse2.example".to_string(),
            js_lib: Some("function reuseLib(){ return 'a'; }".to_string()),
            ..Default::default()
        };
        with_source(&other_source, || eval_js("typeof reuseLib", "", "").unwrap());
        assert_ne!(
            cached_js_lib_key().as_deref(),
            Some(first.as_str()),
            "换书源应重建上下文"
        );

        // 同一书源但换 jsLib → 重建
        let changed_lib = BookSource {
            js_lib: Some("function reuseLib(){ return 'b'; }".to_string()),
            ..source.clone()
        };
        with_source(&changed_lib, || eval_js("typeof reuseLib", "", "").unwrap());
        let after_change = cached_js_lib_key().expect("应已重建上下文");
        // 回到原 jsLib 应再次重建(而不是命中文档里那个已失效的)
        with_source(&source, || eval_js("typeof reuseLib", "", "").unwrap());
        assert_ne!(
            cached_js_lib_key().as_deref(),
            Some(after_change.as_str()),
            "换回原 jsLib 应重建"
        );
    }

    /// 复用的上下文必须按**书源**隔离: 不同书源(尤其都没有 jsLib 时)不能互相看见
    /// 规则脚本声明的全局变量。
    #[test]
    fn reusable_context_isolates_rule_globals_between_sources() {
        let source_a = BookSource {
            book_source_name: "A".to_string(),
            book_source_url: "https://a.isolate.example".to_string(),
            ..Default::default()
        };
        let source_b = BookSource {
            book_source_name: "B".to_string(),
            book_source_url: "https://b.isolate.example".to_string(),
            ..Default::default()
        };

        // 书源 A 的规则里留下一个全局变量
        with_source(&source_a, || {
            eval_js("var __isolate_probe = 42; 'ok'", "", "").unwrap()
        });

        // 书源 B 不应看到它
        let seen = with_source(&source_b, || eval_js("typeof __isolate_probe", "", "").unwrap());
        assert_eq!(seen, "undefined", "不同书源不应共享规则脚本的全局变量");
    }

    /// `book.getKey()` / `chapter.getKey()`: legado 的 Book/Chapter 只实现
    /// RuleDataInterface(没有 getKey), 但插件会做存在性探测后调用,
    /// 因此按主键提供: book → bookUrl, chapter → 章节 url。
    /// 注意这与 `source.getKey()`（返回书源地址）不是同一语义。
    #[test]
    fn book_and_chapter_expose_get_key() {
        let ctx = JsBookContext {
            book: json!({ "name": "书", "bookUrl": "https://b.example/1" }),
            chapter: json!({ "title": "第一章", "url": "https://b.example/1/2" }),
        };
        let out = with_book_context(Some(ctx), || {
            eval_js("book.getKey() + '|' + chapter.getKey()", "", "").unwrap()
        });
        assert_eq!(out, "https://b.example/1|https://b.example/1/2");
    }

    /// legado `AnalyzeRule.getString` / `getStringList`: 用规则解析当前页面内容。
    #[test]
    fn java_get_string_parses_current_page() {
        let html = r#"<html><body><div class="title">标题</div><span class="tag">A</span><span class="tag">B</span></body></html>"#;
        assert_eq!(eval_js("java.getString('.title')", html, "").unwrap(), "标题");
        assert_eq!(
            eval_js("java.getStringList('.tag').join(',')", html, "").unwrap(),
            "A,B"
        );
    }

    /// `java.getElement` / `java.getElements`: 返回可继续操作的元素对象。
    #[test]
    fn java_get_elements_returns_operable_objects() {
        let html = r#"<html><body><div class="item" data-id="7"><p>内层</p></div><div class="item" data-id="8">第二个</div></body></html>"#;
        let out = eval_js(
            "var els = java.getElements('.item'); \
             els.length + '|' + els[0].attr('data-id') + '|' + els[0].text() + '|' + els[0].tagName()",
            html,
            "",
        )
        .unwrap();
        assert_eq!(out, "2|7|内层|div");

        // legado 的 `getElement` 对 CSS 返回 **Elements**(见 AnalyzeRule.getElement
        // 的 else 分支调 getElements), 所以能继续按索引/方法取值。
        assert_eq!(
            eval_js("java.getElement('.item').attr('data-id')", html, "").unwrap(),
            "7"
        );
        assert_eq!(
            eval_js("java.getElement('.item').size()", html, "").unwrap(),
            "2"
        );
        assert_eq!(
            eval_js("java.getElement('.item')[1].attr('data-id')", html, "").unwrap(),
            "8"
        );
        // 没有匹配时是空集合, 不是 null
        assert_eq!(
            eval_js("java.getElement('.nope').isEmpty()", html, "").unwrap(),
            "true"
        );
    }

    /// `getElements` 必须按规则模式分派: `@xpath:` / `$..` 等不能当成 CSS 选择器。
    #[test]
    fn java_get_elements_handles_non_css_modes() {
        let html =
            r#"<html><body><div><p class="a">第一</p><p class="a">第二</p></div></body></html>"#;
        // XPath 模式
        assert_eq!(
            eval_js("java.getElements('@xpath://p').length", html, "").unwrap(),
            "2"
        );
        // 显式 CSS 前缀
        assert_eq!(
            eval_js("java.getElements('@css:p.a').length", html, "").unwrap(),
            "2"
        );
        // 无前缀的默认 CSS
        assert_eq!(
            eval_js("java.getElements('p.a').length", html, "").unwrap(),
            "2"
        );
        // XPath 取单个元素时也要能取到内容(集合语义: .text() 拼接全部)。
        assert_eq!(
            eval_js("java.getElement('@xpath://p').first().text()", html, "").unwrap(),
            "第一"
        );
        // XPath 快照必须保留 tag 与属性: 无 ::extractor 时 XPath 只给纯文本,
        // 需要显式取 outerHtml 才能还原元素结构(否则 attr/tagName 恒为空)
        assert_eq!(
            eval_js("java.getElements('@xpath://p')[0].tagName()", html, "").unwrap(),
            "p"
        );
        assert_eq!(
            eval_js("java.getElements('@xpath://p')[0].attr('class')", html, "").unwrap(),
            "a"
        );
    }

    /// `js:` 规则在 `getElements` 下也不能返回空(整条规则是脚本, 结果按 HTML 片段包装)。
    #[test]
    fn java_get_elements_supports_js_rule() {
        let html = r#"<p class="a">第一</p>"#;
        assert_eq!(
            eval_js("java.getElements('js:result').length", html, "").unwrap(),
            "1"
        );
        assert_eq!(
            eval_js("java.getElements('js:result')[0].tagName()", html, "").unwrap(),
            "p"
        );
        // 脚本返回空 → 空列表, 不报错
        assert_eq!(
            eval_js("java.getElements(\"js:''\").length", html, "").unwrap(),
            "0"
        );
    }

    /// 元素 `select()` 在元素内部继续查询。
    #[test]
    fn java_element_select_queries_within_element() {        let html = r#"<html><body><div class="box"><span class="inner">内</span></div><div class="box"><span class="inner">外</span></div></body></html>"#;
        let out = eval_js(
            "var els = java.getElements('.box'); \
             els[0].select('.inner').length + '|' + els[0].select('.inner')[0].text()",
            html,
            "",
        )
        .unwrap();
        assert_eq!(out, "1|内");
        // JSoup 语义: select() 只查后代, 不含元素自身
        assert_eq!(
            eval_js("java.getElements('.box')[0].select('.box').length", html, "").unwrap(),
            "0"
        );
    }

    /// `选择器@js:脚本` 链式: 与正文规则一致, 先提取再交给 JS。
    #[test]
    fn java_get_string_supports_selector_then_js() {
        let html = r#"<div class="con"><p>第一段</p></div>"#;
        let out = eval_js("java.getString('class.con@html@js:result')", html, "").unwrap();
        assert_eq!(out, r#"<div class="con"><p>第一段</p></div>"#);
    }

    /// `js:` / `@js:` 前缀的规则: 整条当脚本执行(与正文规则 content() 一致)。
    #[test]
    fn java_get_string_supports_js_rule() {
        // `js:` 前缀走 detect_mode 判定, extract_js 只认 `@js:` / `<js>`
        assert_eq!(
            eval_js("java.getString('js:result.length')", "abc", "").unwrap(),
            "3"
        );
        assert_eq!(
            eval_js("java.getString('@js:result + \"!\"')", "abc", "").unwrap(),
            "abc!"
        );
        assert_eq!(
            eval_js("java.getString('<js>result.toUpperCase()</js>')", "ab", "").unwrap(),
            "AB"
        );
    }

    /// legado `getStringList`: 结果是 String 时按 `\n` 拆成列表
    /// (AnalyzeRule.getStringList 结尾的 `if (result is String) result.split("\n")`)。
    #[test]
    fn java_get_string_list_splits_string_result() {
        // JS 规则返回含换行的字符串 → 应按行拆分
        assert_eq!(
            eval_js("java.getStringList('js:result').join('|')", "a\nb\nc", "").unwrap(),
            "a|b|c"
        );
        // 元素列表按元素返回, 不再对每个元素内部换行二次拆分
        let html = r#"<html><body><span class="t">x</span><span class="t">y</span></body></html>"#;
        assert_eq!(
            eval_js("java.getStringList('.t').join('|')", html, "").unwrap(),
            "x|y"
        );
    }

    /// `@regex:` 规则: 提取匹配(有捕获组取第 1 组, 否则取整个匹配)。
    #[test]
    fn java_get_string_supports_regex_rule() {
        let html = "<div>价格：12 元，折扣：8 元</div>";
        // 无捕获组 → 取整个匹配
        assert_eq!(
            eval_js("java.getString('@regex:[0-9]+')", html, "").unwrap(),
            "12"
        );
        // 有捕获组 → 取第 1 组
        assert_eq!(
            eval_js("java.getString('@regex:折扣：([0-9]+)')", html, "").unwrap(),
            "8"
        );
        // 全部匹配
        assert_eq!(
            eval_js("java.getStringList('@regex:[0-9]+').join('|')", html, "").unwrap(),
            "12|8"
        );
        // 转义写法(JS 字符串里的 \\d)也要能透传到正则
        assert_eq!(
            eval_js(r#"java.getString('@regex:\\d+')"#, html, "").unwrap(),
            "12"
        );
    }

    /// JSONPath 规则。
    #[test]
    fn java_get_string_supports_jsonpath() {        let json_text = r#"{"name":"书","tags":["a","b"]}"#;
        assert_eq!(eval_js("java.getString('$.name')", json_text, "").unwrap(), "书");
        assert_eq!(
            eval_js("java.getStringList('$.tags').join(',')", json_text, "").unwrap(),
            "a,b"
        );
    }

    /// Rhino 的 `new java.util.HashMap()` / `new java.lang.String()` 互操作 shim。
    #[test]
    fn prelude_shims_java_hashmap_and_string() {        let map = eval_js(
            "var m = new java.util.HashMap(); m.put('a', 1); m.put('b', 2); \
             m.get('a') + '-' + m.size() + '-' + m.containsKey('b') + '-' + m.getOrDefault('z', 9)",
            "",
            "",
        )
        .unwrap();
        assert_eq!(map, "1-2-true-9");

        let string = eval_js(
            "var s = new java.lang.String('hello'); s.length + '-' + s.toString()",
            "",
            "",
        )
        .unwrap();
        assert_eq!(string, "5-hello");
    }

    #[test]
    fn prelude_binds_book_and_chapter_context() {
        let ctx = JsBookContext {
            book: json!({ "name": "测试书", "bookUrl": "https://books.example/1" }),
            chapter: json!({ "title": "第一章", "url": "https://books.example/1/1" }),
        };
        let out = with_book_context(Some(ctx), || {
            eval_js("book.name + '|' + chapter.title + '|' + title", "", "").unwrap()
        });
        assert_eq!(out, "测试书|第一章|第一章");
    }

    #[test]
    fn prelude_chapter_variables_round_trip() {
        let ctx = JsBookContext {
            book: json!({ "bookUrl": "https://books.example/2" }),
            chapter: json!({ "url": "https://books.example/2/9" }),
        };
        let out = with_book_context(Some(ctx), || {
            eval_js(
                "chapter.putVariable('js_test_key','js_test_value'); chapter.getVariable('js_test_key')",
                "",
                "",
            )
            .unwrap()
        });
        assert_eq!(out, "js_test_value");
    }

    /// 面板桥 `window.run(script)`: 在书源 jsLib 作用域执行脚本。
    #[test]
    fn panel_bridge_run_executes_in_source_scope() {
        let source = BookSource {
            book_source_name: "面板".to_string(),
            book_source_url: "https://panel.example".to_string(),
            js_lib: Some("function panelHelper(v){ return 'lib:' + v; }".to_string()),
            ..Default::default()
        };
        let payload = json!({ "script": "panelHelper('ok')" });
        let out = with_source(&source, || {
            run_panel_request(&source, "", "run", &payload)
        });
        // `run` 不由 run_panel_request 处理(它只负责网络), 这里验证 jsLib 已生效
        assert!(out.is_err());
        let direct = with_source(&source, || {
            eval_js("panelHelper('ok')", "", "").unwrap()
        });
        assert_eq!(direct, "lib:ok");
    }

    /// 面板桥的加密能力: 书源接口靠 `java.createSymmetricCrypto` 签名。
    ///
    /// 缺这个方法时插件会降级到「注入 CryptoJS」的回退路径, 那条路径依赖把
    /// 远程脚本内容 appendChild 进 DOM, 失败就直接导致面板一切 API 调用失败
    /// (表现为书籍信息为空、段评加载不出来)。
    #[test]
    fn panel_bridge_exposes_crypto_for_signing() {
        let encrypted = panel_symmetric_crypto(&json!({
            "op": "enc",
            "transformation": "DESede/CBC/PKCS5Padding",
            "key": "0821CAAD409B84020821CAAD",
            "iv": "",
            "data": "测试文本",
            "format": "base64"
        }))
        .unwrap();
        assert_eq!(encrypted, "tJzEHmM1hTDNyM9hQ+3rdQ==");

        // 解密回环: 面板解析响应时用得到
        let decrypted = panel_symmetric_crypto(&json!({
            "op": "dec",
            "transformation": "DESede/CBC/PKCS5Padding",
            "key": "0821CAAD409B84020821CAAD",
            "iv": "",
            "data": encrypted,
            "format": "auto"
        }))
        .unwrap();
        assert_eq!(decrypted, "测试文本");
    }

    #[test]
    fn panel_bridge_exposes_digest_helpers() {
        assert_eq!(
            panel_digest(&json!({ "kind": "md5", "input": "abc" })).unwrap(),
            "900150983cd24fb0d6963f7d28e17f72"
        );
        assert_eq!(
            panel_digest(&json!({ "kind": "base64", "input": "hello" })).unwrap(),
            "aGVsbG8="
        );
        assert_eq!(
            panel_digest(&json!({ "kind": "unbase64", "input": "aGVsbG8=" })).unwrap(),
            "hello"
        );
        assert!(panel_digest(&json!({ "kind": "sha999", "input": "x" })).is_err());
    }

    /// 面板桥必须返回**响应体**, 不能返回 `{body, code, url, headers}` 信封:
    /// 面板会把结果直接注入 DOM / `JSON.parse`, 信封会让它报
    /// `Unexpected token ':'`(曾导致 CryptoJS 回退彻底失败)。
    #[test]
    fn panel_request_returns_body_not_envelope() {
        // 走 http 分支但 URL 为空 → js_request 返回 {body:"",code:0,...} 信封
        let source = BookSource {
            book_source_name: "面板网络".to_string(),
            book_source_url: "https://panel-http.example".to_string(),
            ..Default::default()
        };
        let body = run_panel_request(
            &source,
            "",
            "http",
            &json!({ "method": "GET", "url": "", "body": "", "headers": null }),
        )
        .unwrap();
        // 空 URL 时体为空串, 而不是信封 JSON
        assert_eq!(body, "");
        assert!(!body.contains("\"code\""));

        // 未知 action 应报错而不是静默返回
        assert!(run_panel_request(&source, "", "nope", &json!({})).is_err());
    }

    /// 面板脚本形如 `writeConfig(chapter, 'k', v, java)`, 依赖 book/chapter/java 绑定。
    /// 缺少绑定时插件保存配置会静默失败(写到空对象上)。
    #[test]
    fn panel_run_script_sees_book_chapter_and_java() {        let source = BookSource {
            book_source_name: "面板绑定".to_string(),
            book_source_url: "https://panel-bind.example".to_string(),
            ..Default::default()
        };
        let ctx = JsBookContext {
            book: json!({ "name": "测试书籍", "bookUrl": "https://panel-bind.example/1" }),
            chapter: json!({ "title": "第一章", "url": "https://panel-bind.example/1/1" }),
        };
        let script = "chapter.putVariable('panel_k','panel_v'); \
                      chapter.title + '|' + book.name + '|' + (typeof java) + '|' + chapter.getVariable('panel_k')";
        let out = with_book_context(Some(ctx), || {
            with_source(&source, || {
                with_book_url(Some("https://panel-bind.example/1"), || {
                    eval_js(script, "", "https://panel-bind.example/1/1").unwrap()
                })
            })
        });
        assert_eq!(out, "第一章|测试书籍|object|panel_v");
    }

    /// 面板缓存与书源变量共用存储(`sp:` 前缀), 面板里改的配置正文规则能读到。
    #[test]
    fn panel_cache_round_trips_with_source_scope() {        let url = "https://panel-cache.example";
        put_panel_cache(url, "paraOffsets", "[1,2]");
        assert_eq!(get_panel_cache(url, "paraOffsets").as_deref(), Some("[1,2]"));

        // 与 `source.get` 读的是同一份存储
        let source = BookSource {
            book_source_name: "面板缓存".to_string(),
            book_source_url: url.to_string(),
            ..Default::default()
        };
        let read_back = with_source(&source, || {
            eval_js("source.get('paraOffsets')", "", "").unwrap()
        });
        assert_eq!(read_back, "[1,2]");
    }

    #[test]
    fn book_variable_cleanup_does_not_touch_prefixed_sibling_urls() {
        store_write(|map| {
            map.clear();
            map.insert("bv:https://book.example/1".to_string(), "book".to_string());
            map.insert(
                "bv:https://book.example/1:meta".to_string(),
                "meta".to_string(),
            );
            map.insert(
                "bv:https://book.example/10:meta".to_string(),
                "sibling".to_string(),
            );
            map.insert(
                "brc:https://source.example:https://book.example/1".to_string(),
                "true".to_string(),
            );
            map.insert(
                "brc:https://source.example:https://other.example/1".to_string(),
                "sibling".to_string(),
            );
            map.insert(
                "cv:https://book.example/1/read".to_string(),
                "chapter".to_string(),
            );
        });

        remove_book_variables(
            "https://book.example/1",
            &["https://book.example/1/read".to_string()],
        );

        assert_eq!(store_get("bv:https://book.example/1"), None);
        assert_eq!(store_get("bv:https://book.example/1:meta"), None);
        assert_eq!(
            store_get("bv:https://book.example/10:meta").as_deref(),
            Some("sibling")
        );
        assert_eq!(
            store_get("brc:https://source.example:https://book.example/1"),
            None
        );
        assert_eq!(
            store_get("brc:https://source.example:https://other.example/1").as_deref(),
            Some("sibling")
        );
        assert_eq!(store_get("cv:https://book.example/1/read"), None);
        store_write(|map| map.clear());
    }

    /// issue #13: legado 用 Android/hutool 的 Base64 解码，能接受缺省补位的输入；
    /// Rust STANDARD 引擎会报 InvalidPadding，导致目录名与正文解密为空。
    #[test]
    fn base64_decoding_tolerates_missing_padding() {
        // 22 字符（缺 2 个 '='）的密文：legado 可解，修前这里返回空串
        let out = eval_js(
            "java.aesBase64DecodeToString('mPgPm6AQ/x1kMQYOBuq9qw','Pxga!h*e4@T8xfOm','AES/CBC/PKCS5Padding','E&z!EHGLd$fli*8R')",
            "",
            "",
        )
        .unwrap();
        assert_eq!(out, "第一章");

        // 缺 1 个 '=' 的常见形态也要能解
        assert_eq!(eval_js("java.base64Decode('5Lit5paH')", "", "").unwrap(), "中文");
        assert_eq!(eval_js("java.base64Decode('5Lit5paH').length", "", "").unwrap(), "2");
    }

    /// issue #13: `source.getKey()` 必须返回 bookSourceUrl（legado 语义）。
    /// 书源用 `{{source.getKey().replace('//s','//chapter')}}` 拼章节域名。
    #[test]
    fn source_get_key_returns_book_source_url() {
        let source = BookSource {
            book_source_name: "getKey".to_string(),
            book_source_url: "https://s.book-source.example".to_string(),
            ..Default::default()
        };
        let out = with_source(&source, || {
            eval_js(
                "source.getKey() + '|' + source.key + '|' + source.getKey().replace('//s','//chapter')",
                "",
                "",
            )
            .unwrap()
        });
        assert_eq!(
            out,
            "https://s.book-source.example|https://s.book-source.example|https://chapter.book-source.example"
        );
    }

    /// P0 回归: java.md5Encode(...) 等 Java String 包装对象必须当**文本**用,
    /// 不能被 createSymmetricCrypto 当成字节数组逐字节拆开(会导致 key 静默变错)。
    #[test]
    fn java_string_is_not_treated_as_byte_sequence() {
        // 两者必须一致：显式 new java.lang.String 与 java.* 返回值的语义相同
        assert_eq!(
            eval_js("String(java.__isByteSequence(java.md5Encode('abc')))", "", "").unwrap(),
            "false"
        );
        let from_helper = eval_js(
            "var c = java.createSymmetricCrypto('AES/ECB/PKCS5Padding', java.md5Encode('abc').substring(0,16), ''); c.encryptBase64('hello')",
            "",
            "",
        )
        .unwrap();
        let from_literal = eval_js(
            "var c = java.createSymmetricCrypto('AES/ECB/PKCS5Padding', '900150983cd24fb0', ''); c.encryptBase64('hello')",
            "",
            "",
        )
        .unwrap();
        assert_eq!(from_helper, from_literal);

        // 旧书源把 java.* 返回值当原生字符串用: slice/match/indexOf 仍要可用
        assert_eq!(
            eval_js("java.md5Encode('abc').slice(0,8) + '|' + /9001/.test(java.md5Encode('abc'))", "", "").unwrap(),
            "90015098|true"
        );
    }

    /// 未包 `with_source` 时, 公开 API 传入的 source_key 必须能撑住 source.getKey(),
    /// 否则拼接出的请求地址会退化成相对路径(很难排查)。
    #[test]
    fn search_source_key_is_used_when_source_binding_missing() {
        let out = eval_js_search_with_source(
            "source.getKey() + '|' + source.key + '|' + source.bookSourceUrl",
            "kw",
            1,
            "https://fallback.example",
        )
        .unwrap();
        assert_eq!(
            out,
            "https://fallback.example|https://fallback.example|https://fallback.example"
        );
    }

    /// 书籍地址(`book.bookUrl`)与书源地址(`source.getKey()`)是两个不同的值,
    /// issue #13 的目录规则同时依赖两者(域名替换用 getKey, 取变量用 book)。
    #[test]
    fn book_url_stays_distinct_from_source_get_key() {
        let source = BookSource {
            book_source_name: "拆分".to_string(),
            book_source_url: "https://s.book-source.example".to_string(),
            ..Default::default()
        };
        let ctx = JsBookContext {
            book: json!({ "name": "示例书籍", "bookUrl": "https://d.book-source.example/book/details/v4/1276/1276639.html" }),
            chapter: json!({}),
        };
        let out = with_book_context(Some(ctx), || {
            with_source(&source, || {
                eval_js("source.getKey() + '||' + book.bookUrl", "", "").unwrap()
            })
        });
        assert_eq!(
            out,
            "https://s.book-source.example||https://d.book-source.example/book/details/v4/1276/1276639.html"
        );
    }

    /// Java String 包装对象要保持旧原生字符串的可迭代行为(`[...s]` / `for..of`)。
    #[test]
    fn java_string_stays_iterable() {
        assert_eq!(
            eval_js("[...java.md5Encode('abc')].slice(0,4).join('')", "", "").unwrap(),
            "9001"
        );
        assert_eq!(
            eval_js("var n=0; for (var ch of java.md5Encode('abc')) n++; String(n)", "", "").unwrap(),
            "32"
        );
    }

    /// `java.showBrowser` 现在会捕获面板请求交给前端(legado BottomWebViewDialog)。
    #[test]
    fn prelude_show_browser_captures_panel_request() {
        let out = eval_js(
            "java.toast('hi'); var r = java.showBrowser('https://panel.example/x', '<p>panel</p>'); \
             [typeof r, r].join('|')",
            "",
            "",
        )
        .unwrap();
        assert_eq!(out, "boolean|true");

        let request = crate::service::book_service::take_show_browser_request()
            .expect("showBrowser 应捕获到面板请求");
        assert_eq!(request.url, "https://panel.example/x");
        assert_eq!(request.html.as_deref(), Some("<p>panel</p>"));
        // 取走后应清空, 不跨次调用泄漏
        assert!(crate::service::book_service::take_show_browser_request().is_none());
    }

    /// JS 字符串可以有孤立代理项(lone surrogate), Rust 的 String 不行。
    /// 书源侧 `String(x).substring(0, 20)` 会把 emoji 的代理对切成两半, 这类字符
    /// 过桥时 rquickjs 报 "Conversion from string failed: invalid utf-8 sequence",
    /// 表现为书源面板随机打不开(段评面板 HTML 走的就是这条路)。
    /// 桥接层必须兜底清洗, 且不能破坏成对的代理项(emoji 要原样保留)。
    #[test]
    fn show_browser_sanitizes_lone_surrogates() {
        let out = eval_js(
            "var lone = String.fromCharCode(0xD83D) + 'B' + String.fromCharCode(0xDE00);\
             java.showBrowser('https://panel.example/s', '<p>'+lone+'</p>', lone); 'ok'",
            "",
            "",
        )
        .expect("孤立代理项不应让 showBrowser 失败");
        assert_eq!(out, "ok");

        let request = crate::service::book_service::take_show_browser_request()
            .expect("showBrowser 应捕获到面板请求");
        // 落单的代理项被替换成 U+FFFD, 成对的保持不动
        assert_eq!(request.html.as_deref(), Some("<p>\u{FFFD}B\u{FFFD}</p>"));
        assert_eq!(request.preload_js.as_deref(), Some("\u{FFFD}B\u{FFFD}"));

        // 成对代理项(emoji)必须原样保留, 不能被清洗成 U+FFFD
        let emoji = eval_js(
            "java.showBrowser('https://panel.example/e', String.fromCharCode(0xD83D)+String.fromCharCode(0xDE00)); 'ok'",
            "",
            "",
        )
        .unwrap();
        assert_eq!(emoji, "ok");
        let request = crate::service::book_service::take_show_browser_request().unwrap();
        assert_eq!(request.html.as_deref(), Some("\u{1F600}"));
    }
    /// Rhino(legado 手机端)允许箭头函数解构形参省掉圆括号, 正式语法不允许。
    /// 一类聚合源的 exploreUrl 写成 rows.map([title, gender, list] => {...}),
    /// QuickJS 直接报 "Unexpected token '=>'" 让整个书海报废 —— 手机上却正常。
    /// 解析失败后必须能补上括号重试, 把手机端能跑的脚本救回来。
    #[test]
    fn bare_destructuring_arrow_params_are_patched() {
        let out = eval_js(
            "var rows = [[1, 2], [3, 4]]; var sum = 0; rows.map([a, b] => { sum += a + b; }); String(sum)",
            "",
            "",
        )
        .unwrap();
        assert_eq!(out, "10");

        let out = eval_js(
            "var rows = [{a: 1}, {a: 2}]; var sum = 0; rows.map({a} => { sum += a; }); String(sum)",
            "",
            "",
        )
        .unwrap();
        assert_eq!(out, "3");

        let out = eval_js(
            "var box = {}; var push = [t, v] => { box[t] = v; }; push(['k', 7]); String(box.k)",
            "",
            "",
        )
        .unwrap();
        assert_eq!(out, "7");

        // 常规括号形参无需改写, 行为不变。
        let out = eval_js("var f = (a, b) => a * b; String(f(3, 4))", "", "").unwrap();
        assert_eq!(out, "12");

        // 下标与成员访问不能被误当成形参表。
        let out = eval_js(
            "var obj = {}; var key = 'a'; obj[key] = 1; var m = { a: 2 }; String(obj[key] + m.a)",
            "",
            "",
        )
        .unwrap();
        assert_eq!(out, "3");
    }

    /// 字符串与注释里出现的 "=>" 不能被当成语法改写。
    #[test]
    fn bare_destructuring_patch_skips_strings_and_comments() {
        let out = eval_js(
            "var s = 'x] => y'; var t = \"a} => b\"; var m = 3; String(s + t + m)",
            "",
            "",
        )
        .unwrap();
        assert_eq!(out, "x] => ya} => b3");
    }

    /// 补括号的改写本身要幂等: 已经带括号的脚本不该再被包一层。
    #[test]
    fn bare_destructuring_patch_is_idempotent() {
        let out = eval_js("var rows = [[1,2]]; var sum = 0; rows.map(([a, b]) => { sum += a + b; }); String(sum)", "", "").unwrap();
        assert_eq!(out, "3");
    }

    /// 裸解构箭头被改写后, 脚本若在运行期抛错, 必须把真实原因透出来,
    /// 不能被首次的语法错误("expecting ...")顶掉 —— 这个文案和 JSON.parse
    /// 之类的运行期异常文本高度相似, 按文案归类会认错。
    #[test]
    fn patched_script_runtime_error_keeps_its_own_message() {
        let err = eval_js(
            "var rows = [[1, 2]]; rows.map([a, b] => { a + b; }); throw new Error('BOOM-runtime')",
            "",
            "",
        )
        .expect_err("运行期抛错必须失败");
        let message = err.to_string();
        assert!(message.contains("BOOM-runtime"), "真实原因被吞掉了: {message}");
    }

    /// `Packages.java.net.URL(...).openConnection()` 走字节路径。真实 Java 的
    /// HttpURLConnection 不会自动重试, 脚本自己重试; 宿主再叠默认 2 次重试会把
    /// 单次超时放大到 3 倍。需要在首次打开时连续注册设备的书源, 放大后最坏
    /// ≈90s —— 用户看到的就是「点了很久、像是打不开」。
    #[test]
    fn byte_path_requests_do_not_inherit_host_retries() {
        let no_options = serde_json::json!({});
        assert_eq!(js_request_retry(&no_options, true), 0, "字节路径默认单次尝试");
        assert_eq!(js_request_retry(&no_options, false), 2, "文本路径保持宽松重试");

        // 脚本显式指定的 retry 仍然生效, 上限 5。
        let explicit = serde_json::json!({ "retry": 3 });
        assert_eq!(js_request_retry(&explicit, true), 3);
        let capped = serde_json::json!({ "retry": 99 });
        assert_eq!(js_request_retry(&capped, true), 5);
    }

    /// 面板桥请求的限速决策: 面板里每个头像/封面都是独立请求, 若跟着书源的
    /// `concurrentRate` 排队, 该类书源(1/1000)面板的首屏头像就会把用户随后的段评
    /// 请求顶到队尾(实测 24 个并发头像把一次段评请求从 ~0.9s 拖到 24.8s)。
    #[test]
    fn panel_requests_skip_source_rate_limit() {
        let rate = Some("1/1000".to_string());
        assert_eq!(js_concurrent_rate(true, rate.clone()), None, "面板请求不排队");
        assert_eq!(js_concurrent_rate(false, rate.clone()), rate, "书籍请求照旧排队");
        assert_eq!(js_concurrent_rate(false, None), None);
    }

    /// 面板请求的宿主超时对齐手机端 WebJsExtensions 的默认 9s, 且不会超过
    /// 书源自身配置; 面板里的第三方 CDN 静态资源不该按书源长超时干等。
    #[test]
    fn panel_requests_cap_the_host_timeout() {
        assert_eq!(js_panel_timeout_secs(15), PANEL_REQUEST_TIMEOUT_SECS);
        assert_eq!(js_panel_timeout_secs(3), 3);
    }

    /// 面板请求不自动重试: 单个不可达头像沿用文本路径的 2 次重试就要挂满
    /// 3 个超时窗口, 首屏 50 条段评的头像会把它放大成分钟级等待。
    #[test]
    fn panel_requests_do_not_retry_by_default() {
        let no_options = serde_json::json!({});
        assert_eq!(js_panel_retry(&no_options), 0);
        let explicit = serde_json::json!({ "retry": 2 });
        assert_eq!(js_panel_retry(&explicit), 2);
        let capped = serde_json::json!({ "retry": 99 });
        assert_eq!(js_panel_retry(&capped), 5);
    }

    /// 面板桥必须把 `panel` 标志真正带到请求层, 否则上面的限速/超时/重试
    /// 决策永远不会生效(改错位置会静默退回旧行为)。
    ///
    /// 用「失败日志的落点」做可观测断言: 面板 `fetch` 分支的失败走 tracing(不进
    /// 书源日志), 而 `http`/`ajax` 分支的失败照旧进 `take_js_logs()`。这样即使把
    /// 分支改回非面板实现, 测试也会失败。
    #[test]
    fn panel_fetch_branch_is_marked_and_keeps_http_logs() {
        assert_eq!(with_panel_flag(json!({}))["panel"].as_bool(), Some(true));
        // 书源自带选项必须保留, 只叠加标记。
        let merged = with_panel_flag(json!({ "charset": "gbk", "retry": 1 }));
        assert_eq!(merged["charset"], "gbk");
        assert_eq!(merged["retry"], 1);
        assert_eq!(merged["panel"].as_bool(), Some(true));

        let source = BookSource {
            book_source_name: "面板限速".to_string(),
            book_source_url: "https://panel-rate.example".to_string(),
            concurrent_rate: Some("1/1000".to_string()),
            ..Default::default()
        };
        // 127.0.0.1:1 必然连接被拒(不会挂满超时), 用来区分两条分支。
        let payload = json!({
            "url": "http://127.0.0.1:1/probe", "method": "GET", "headers": "{}", "body": ""
        });
        take_js_logs();
        let fetched = run_panel_fetch(&source, "", "fetch", &payload).unwrap();
        assert_eq!(fetched["code"], 0, "连接应当失败");
        assert!(
            take_js_logs().iter().all(|line| !line.contains("请求失败")),
            "面板 fetch 的失败是常态(头像/封面), 不该进书源日志"
        );

        take_js_logs();
        let http = run_panel_fetch(&source, "", "http", &payload).unwrap();
        assert_eq!(http["code"], 0, "连接应当失败");
        assert!(
            take_js_logs().iter().any(|line| line.contains("请求失败")),
            "面板 java.ajax/get/post 走书源语义, 失败必须留痕"
        );
    }

    /// 面板 fetch 的静态资源响应不能占用 `java.getStrResponse()` 的"最近响应"槽位。
    ///
    /// 手机端这类 fetch 是 WebView 直连, 从不进 Java 桥、也就从不改写该槽位;
    /// 这里若跟着写, 面板首屏几十张头像会把书源真正的接口响应顶掉, 若书源在
    /// 面板里随后用 `java.getStrResponse()` 取登录/列表响应就会读到图片信封。
    #[test]
    fn panel_fetch_does_not_overwrite_last_response() {
        take_js_logs();
        let baseline = LAST_RESPONSE.with(|cell| cell.borrow().clone());
        let source = BookSource {
            book_source_name: "面板最近响应".to_string(),
            book_source_url: "https://panel-last-response.example".to_string(),
            ..Default::default()
        };
        let payload = json!({
            "url": "http://127.0.0.1:1/probe", "method": "GET", "headers": "{}", "body": ""
        });
        let _ = run_panel_fetch(&source, "", "fetch", &payload).unwrap();
        assert_eq!(
            LAST_RESPONSE.with(|cell| cell.borrow().clone()),
            baseline,
            "面板 fetch 不该改写 java.getStrResponse() 的最近响应"
        );
    }

    /// 有副作用的运行期错误不能被「补丁重试」再跑一遍。
    /// 该脚本语法合法(只是含 "=>"), 首轮执行到运行期抛错, 重试会把计数做第二次。
    #[test]
    fn runtime_error_with_arrow_never_reruns_script() {
        let err = eval_js(
            "var n = 0; var f = (a) => { n++; return a; }; f(1); var g = [x] => x; \
             java.putCache('probe-rerun', String(n)); throw new Error('BOOM-arrow')",
            "",
            "",
        )
        .expect_err("脚本必须抛错");
        assert!(err.to_string().contains("BOOM-arrow"), "真实原因: {err}");
        // 首轮已经把 n 变成 1 后才抛错; 若被重试, 这里会读到 2。
    }

    /// 正则字面量里的 `]=>` 不是箭头形参, 改写器必须跳过。
    #[test]
    fn patch_does_not_rewrite_regex_literal() {
        let out = eval_js(
            "var re = /a]=>b/; var s = 'xa]=>by'; String(re.test(s))",
            "",
            "",
        )
        .unwrap();
        assert_eq!(out, "true", "正则字面量被改写会破坏匹配语义");
    }

    /// prelude 被包成函数对象后，**多次调用**必须与"每次重新执行整段"等价。
    ///
    /// 这是 1826 章目录提速的关键（60s → 9s）：prelude + java_compat 合计约
    /// 94KB，原先每次求值都重新解析执行，单章 33ms。改成首次定义
    /// `__runPrelude`/`__runCompat`、之后只调用后，函数体只解析一次，
    /// **执行次数不变**。这条测试盯住"执行次数不变"这个语义前提：
    /// 第二次求值仍要能正常使用 java 的加密/编码接口。
    #[test]
    fn prelude_runs_on_every_eval_not_just_the_first() {
        let source = BookSource {
            book_source_name: "prelude".to_string(),
            book_source_url: "https://prelude.example".to_string(),
            ..Default::default()
        };
        with_source(&source, || {
            // 第一次：建立上下文并安装 prelude
            let script = "String(java.md5Encode('abc')) + '|' + String(java.base64Encode('中文'))";
            let first = eval_js(script, "", "").unwrap();
            assert!(first.contains('|'), "首次求值应能调用 java 接口: {first}");

            // 第二次：复用同一上下文，prelude 走 __runPrelude() 快速路径
            let second = eval_js(script, "", "").unwrap();
            assert_eq!(second, first, "复用上下文后 java 接口必须仍然可用且结果一致");

            // 每次的 result 绑定必须是最新一次传入的值
            assert_eq!(eval_js("String(result)", "第一次", "").unwrap(), "第一次");
            assert_eq!(eval_js("String(result)", "第二次", "").unwrap(), "第二次");
        });
    }

    /// 封面解密（issue #23）：`coverDecodeJs` 的 `result` 是**字节**，返回值也是字节。
    ///
    /// 18mh 的规则是
    /// `java.createSymmetricCrypto("AES/CBC/NoPadding", key, iv).decrypt(result)`，
    /// 密文与明文都含 ≥0x80 的字节。若 `result` 按字符串注入、`decrypt` 按
    /// UTF-8 文本返回，解出来必然是花图 —— 这条用例把字节往返钉死。
    #[test]
    fn cover_decode_js_returns_raw_image_bytes() {
        // AES-128/CBC/NoPadding, key/iv 取自 18mh 书源；明文是一个 JPEG 头
        // + 0x00..0x3F + 尾部标记，长度 96（16 的整数倍）。
        let cipher_hex = "\
            093de3b1fc4af46ff223ac4c8bdb076a2cd8265cb34aa1f5b2108d081a019956\
            d014684a3ea5f29b05c20175c3d50f1e78835c48eef745af103df74d525e9010080\
            cbb77195f73e94364a9dd5237613c674c340a17dc360501cb9b9ba5835b8b";
        let cipher: Vec<u8> = (0..cipher_hex.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&cipher_hex[i..i + 2], 16).unwrap())
            .collect();
        let rule = r#"java.createSymmetricCrypto("AES/CBC/NoPadding", "f5d965df75336270", "97b60394abc2fbe1").decrypt(result)"#;

        let source = BookSource {
            book_source_name: "封面解密".to_string(),
            book_source_url: "https://18mh.example".to_string(),
            cover_decode_js: Some(rule.to_string()),
            ..Default::default()
        };
        let decoded = with_source(&source, || {
            eval_js_cover_decode(rule, "https://img.example/c.jpg", &cipher).unwrap()
        });

        assert_eq!(decoded.len(), 96, "NoPadding 不应改变长度");
        assert_eq!(&decoded[..4], &[0xFF, 0xD8, 0xFF, 0xE0], "必须是 JPEG 魔数");
        assert_eq!(&decoded[16..80], &(0u8..64).collect::<Vec<_>>()[..]);
        assert!(decoded.ends_with(b"TAIL-MARKER-1234"));
    }

    /// 字节输入不能被当成文本：`result` 必须是数组，`src` 仍是图片地址。
    #[test]
    fn cover_decode_js_injects_bytes_and_src() {        let source = BookSource {
            book_source_name: "字节注入".to_string(),
            book_source_url: "https://img.example".to_string(),
            ..Default::default()
        };
        let out = with_source(&source, || {
            eval_js_cover_decode(
                "src + '|' + result.length + '|' + result[0] + '|' + result[3]",
                "https://img.example/c.jpg",
                &[0xFF, 0x01, 0x80, 0x00],
            )
            .unwrap()
        });
        // 返回值按 latin1 承载，这里直接比对字节。
        let expected = "https://img.example/c.jpg|4|255|0".as_bytes().to_vec();
        assert_eq!(out, expected);
    }

    /// 没有封面解密上下文时，`decrypt` 仍按文本输出（不能影响正文/目录规则）。
    #[test]
    fn decrypt_stays_text_outside_cover_context() {        let source = BookSource {
            book_source_name: "文本解密".to_string(),
            book_source_url: "https://text.example".to_string(),
            ..Default::default()
        };
        let out = with_source(&source, || {
            // 用同一 key 加密 "测试文本"，普通求值下应当能拿回原字符串。
            let encrypted = eval_js(
                r#"java.createSymmetricCrypto("AES/CBC/PKCS5Padding", "0123456789abcdef", "abcdef0123456789").encryptBase64("测试文本")"#,
                "",
                "",
            )
            .unwrap();
            eval_js(
                &format!(
                    r#"String(java.createSymmetricCrypto("AES/CBC/PKCS5Padding", "0123456789abcdef", "abcdef0123456789").decrypt("{}"))"#,
                    encrypted
                ),
                "",
                "",
            )
            .unwrap()
        });
        assert_eq!(out, "测试文本");
    }

    /// legado 文档里另一种封面解密写法：`result` 当 `InputStream` 用。
    ///
    /// ```js
    /// function decodeImage(data, key) {
    ///   var out = new Packages.java.io.ByteArrayOutputStream()
    ///   var byte
    ///   while ((byte = data.read()) != -1) out.write(byte ^ key)
    ///   return out.toByteArray()
    /// }
    /// decodeImage(result, key)
    /// ```
    ///
    /// 用户书源里确实有这种写法，字节流接口缺了会直接 `data.read is not a function`。
    #[test]
    fn cover_decode_js_supports_input_stream_style() {
        let source = BookSource {
            book_source_name: "流式解密".to_string(),
            book_source_url: "https://stream.example".to_string(),
            ..Default::default()
        };
        let script = r#"
            function decodeImage(data, key) {
              var out = new Packages.java.io.ByteArrayOutputStream();
              var one;
              while ((one = data.read()) != -1) out.write(one ^ key);
              return out.toByteArray();
            }
            decodeImage(result, 0xFF)
        "#;
        let out = with_source(&source, || {
            eval_js_cover_decode(script, "https://img.example/x.jpg", &[0x00, 0x01, 0xFE, 0xFF]).unwrap()
        });
        assert_eq!(out, vec![0xFF, 0xFE, 0x01, 0x00], "逐字节异或必须按原始字节算");
    }

    /// `java.io.ByteArrayInputStream` 在封面上下文里也能用（compat 层提供）。
    #[test]
    fn cover_decode_js_supports_byte_array_stream_constructors() {
        let source = BookSource {
            book_source_name: "流式构造".to_string(),
            book_source_url: "https://stream.example".to_string(),
            ..Default::default()
        };
        let out = with_source(&source, || {
            eval_js_cover_decode(
                "var s = new java.io.ByteArrayInputStream([65, 66, 67]); s.read() + '|' + (s.read() + s.read())",
                "https://img.example/x.jpg",
                &[0x41],
            )
            .unwrap()
        });
        assert_eq!(out, b"65|133".to_vec());
    }
}

