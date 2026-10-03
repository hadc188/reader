use crate::crawler::url_analyzer::parse_source_headers;
use crate::model::book_source::BookSource;
use crate::util::hash::md5_hex;
use crate::util::text::{apply_regex_replace, strip_whitespace};
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
use reqwest::Method;
use rquickjs::context::EvalOptions;
use rquickjs::function::Func;
use rquickjs::{Context, Object, Runtime, Value};
use serde_json::{json, Value as JsonValue};
use std::cell::RefCell;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};
use uuid::Uuid;

/// 进程内存 KV: `cache.putMemory` / `java.putCache` / `kv_put` 等, 重启即失。
static JS_KV: Lazy<Mutex<HashMap<String, String>>> = Lazy::new(|| Mutex::new(HashMap::new()));
/// 持久 KV: `cache.put`、`source/book/chapter.putVariable` 等, 落盘到 `{storage}/js_store.json`。
static JS_STORE: Lazy<Mutex<HashMap<String, String>>> = Lazy::new(|| Mutex::new(load_js_store()));
static JS_STORAGE_DIR: OnceLock<PathBuf> = OnceLock::new();
/// 组装好的 jsLib 源码, 按 jsLib 文本 md5 缓存; `Some(Instant)` 表示含下载失败项, 到期后重试。
static JS_LIB_CACHE: Lazy<Mutex<HashMap<String, (String, Option<Instant>)>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));
const JS_LIB_RETRY_AFTER: Duration = Duration::from_secs(60);
const DEFAULT_JS_USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36";
/// blocking client 只能在无 tokio runtime 的线程上构建/使用, 所有访问都走 `run_blocking_http`。
static JS_HTTP_CLIENT: Lazy<Client> = Lazy::new(|| {
    Client::builder()
        .cookie_store(true)
        .gzip(true)
        .brotli(true)
        .deflate(true)
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
    headers: Vec<(String, String)>,
}

/// 正文/目录解析时的书籍与章节上下文, 对应 legado JS 里的 `book` / `chapter`。
#[derive(Clone, Debug, Default)]
pub struct JsBookContext {
    pub book: JsonValue,
    pub chapter: JsonValue,
}

thread_local! {
    static ACTIVE_JS_LIB: RefCell<Option<String>> = const { RefCell::new(None) };
    static ACTIVE_SOURCE_KEY: RefCell<Option<String>> = const { RefCell::new(None) };
    static ACTIVE_SOURCE: RefCell<Option<JsSourceInfo>> = const { RefCell::new(None) };
    static ACTIVE_BOOK_CTX: RefCell<Option<JsBookContext>> = const { RefCell::new(None) };
    static JS_LOGS: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
}

const MAX_JS_LOGS: usize = 200;

/// 设置 JS 持久数据目录(jsLib 磁盘缓存、js_store.json)。启动时调用一次。
pub fn set_js_storage_dir(dir: impl Into<PathBuf>) {
    let _ = JS_STORAGE_DIR.set(dir.into());
}

fn with_thread_local<V: 'static, T>(
    key: &'static std::thread::LocalKey<RefCell<Option<V>>>,
    value: Option<V>,
    f: impl FnOnce() -> T,
) -> T {
    let previous = key.with(|cell| cell.replace(value));
    let result = f();
    key.with(|cell| cell.replace(previous));
    result
}

pub fn with_js_lib<T>(js_lib: Option<&str>, f: impl FnOnce() -> T) -> T {
    with_thread_local(&ACTIVE_JS_LIB, js_lib.map(|value| value.to_string()), f)
}

/// 设置当前书源(jsLib + `source` 信息 + 默认请求头)后执行 f。
pub fn with_source<T>(source: &BookSource, f: impl FnOnce() -> T) -> T {
    let headers = source
        .header
        .as_deref()
        .map(parse_source_headers)
        .unwrap_or_default();
    let info = JsSourceInfo {
        url: source.book_source_url.clone(),
        name: source.book_source_name.clone(),
        headers,
    };
    with_thread_local(&ACTIVE_SOURCE, Some(info), || {
        with_js_lib(source.js_lib.as_deref(), f)
    })
}

/// 在解析目录/正文期间设置当前书籍 URL, 供 JS `source.getKey()` 读取。
/// 例如规则 `source.getKey().match(/\d+/)` 需要书籍 bookUrl 中的 id。
pub fn with_source_key<T>(source_key: Option<&str>, f: impl FnOnce() -> T) -> T {
    with_thread_local(&ACTIVE_SOURCE_KEY, source_key.map(|value| value.to_string()), f)
}

/// 设置 JS `book` / `chapter` / `title` 上下文后执行 f。
pub fn with_book_context<T>(ctx: Option<JsBookContext>, f: impl FnOnce() -> T) -> T {
    with_thread_local(&ACTIVE_BOOK_CTX, ctx, f)
}

fn active_source_key() -> Option<String> {
    ACTIVE_SOURCE_KEY.with(|cell| cell.borrow().clone())
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
    JS_KV
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get(key)
        .cloned()
}

fn memory_put(key: String, value: String) {
    JS_KV
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .insert(key, value);
}

pub fn eval_js(script: &str, input: &str, base_url: &str) -> anyhow::Result<String> {
    eval_js_inner(script, Some(input), Some(base_url), None, None, None)
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
    source_key: &str,
) -> anyhow::Result<String> {
    eval_js_inner_with_source(
        script,
        None,
        None,
        Some(key),
        Some(page),
        Some(source_key),
        None,
    )
}

pub fn eval_js_url(
    script: &str,
    result: &str,
    key: &str,
    page: i32,
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
    eval_js_inner_with_source(script, input, base_url, key, page, None, bindings)
}

/// legado 兼容层: 在原生 `__xxx` 原语之上组装 `java` / `source` / `book` / `chapter` / `cache`
/// 以及 Rhino 风格的 `java.util.HashMap` / `java.lang.String` 等。
/// 原生函数参数个数与类型必须精确匹配, 所以这里统一做字符串强转和可变参数适配。
const JS_PRELUDE: &str = r#"(function (g) {
  var S = function (v) { return v === undefined || v === null ? '' : String(v); };
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
   'decodeURIComponent', 'encodeURI', 'decodeURI', 'hexEncodeToString', 'hexDecodeToString',
   'htmlFormat', 'toNumChapter', 'getCache'].forEach(function (n) { wrap(n, 1); });
  ['digestHex', 'digestBase64Str', 'ensureGlobalVariable', 'putCache'].forEach(function (n) { wrap(n, 2); });
  ['HMacHex', 'HMacBase64'].forEach(function (n) { wrap(n, 3); });
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
  function Resp(raw) {
    var o;
    try { o = JSON.parse(raw); } catch (e) { o = { body: '', code: 0, url: '', headers: {} }; }
    return {
      __isResp: true,
      body: function () { return o.body; },
      statusCode: function () { return o.code; },
      code: function () { return o.code; },
      url: function () { return o.url; },
      headers: function () { return o.headers; },
      header: function (k) { var v = o.headers[S(k).toLowerCase()]; return v === undefined ? null : v; },
      toString: function () { return o.body; }
    };
  }
  J.ajax = function (u) { return Resp(g.__connect(S(u), '{}')).body(); };
  J.connect = function (u, h) { return Resp(g.__connect(S(u), headerJson(h))); };
  J.get = function (u, h) { return Resp(g.__http('GET', S(u), '', headerJson(h))); };
  J.head = function (u, h) { return Resp(g.__http('HEAD', S(u), '', headerJson(h))); };
  J.post = function (u, b, h) { return Resp(g.__http('POST', S(u), S(b), headerJson(h))); };
  J.put = function (u, b, h) { return Resp(g.__http('PUT', S(u), S(b), headerJson(h))); };

  function symmetric(t, k, iv) {
    t = S(t); k = S(k); iv = S(iv);
    var run = function (op, d, fmt) { return g.__sym(op, t, k, iv, S(d), fmt); };
    return {
      encrypt: function (d) { return run('enc', d, 'base64'); },
      encryptBase64: function (d) { return run('enc', d, 'base64'); },
      encryptHex: function (d) { return run('enc', d, 'hex'); },
      decrypt: function (d) { return run('dec', d, 'auto'); },
      decryptStr: function (d) { return run('dec', d, 'auto'); },
      setIv: function (v) { iv = S(v); return this; }
    };
  }
  J.createSymmetricCrypto = symmetric;
  J.aesEncodeToBase64String = function (d, k, t, iv) { return symmetric(t, k, iv).encryptBase64(d); };
  J.aesEncodeToString = function (d, k, t, iv) { return symmetric(t, k, iv).encryptHex(d); };
  J.aesBase64DecodeToString = function (d, k, t, iv) { return g.__sym('dec', S(t), S(k), S(iv), S(d), 'base64'); };
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
  J.showBrowser = function (u, html, preloadJs) {
    g.__show_browser(S(u), html === undefined || html === null ? '' : S(html),
                     preloadJs === undefined || preloadJs === null ? '' : S(preloadJs));
    return true;
  };
  J.startBrowser = function (u, title) {
    g.__show_browser(S(u), '', '');
    return true;
  };
  // legado 的 startBrowserAwait 会等面板关闭; 桌面端不阻塞, 返回空响应对象
  J.startBrowserAwait = function (u, title) {
    g.__show_browser(S(u), '', '');
    return Resp('{}');
  };

  // ── legado `AnalyzeRule`: 用规则解析当前页面内容 ──
  // 注意: legado 的 `isUrl` 参数(把结果转成绝对地址)暂未实现, 结果原样返回。
  function wrapElement(snapshot) {
    var s = snapshot || {};
    var attrs = s.attrs || {};
    return {
      text: function () { return S(s.text); },
      ownText: function () { return S(s.ownText); },
      html: function () { return S(s.innerHtml); },
      outerHtml: function () { return S(s.outerHtml); },
      attr: function (name) { var v = attrs[S(name)]; return v === undefined ? '' : S(v); },
      hasAttr: function (name) { return Object.prototype.hasOwnProperty.call(attrs, S(name)); },
      tagName: function () { return S(s.tag); },
      // 在元素内部继续查询: 用 innerHtml 作为内容, 只匹配**后代**
      // (JSoup 的 element.select() 不含元素自身; 用 outerHtml 会把自己也选进来)
      select: function (rule) {
        var inner = S(s.innerHtml);
        if (!inner) return [];
        return wrapElements(g.__rule_get(S(rule), inner, 'elements'));
      },
      toString: function () { return S(s.outerHtml); }
    };
  }
  function wrapElements(json) {
    var list;
    try { list = JSON.parse(S(json)); } catch (e) { return []; }
    if (!list || typeof list.length !== 'number') return [];
    var out = [];
    for (var i = 0; i < list.length; i++) out.push(wrapElement(list[i]));
    return out;
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
    return wrapElements(g.__rule_get(S(rule), '', 'elements'));
  };
  J.getElement = function (rule) {
    var list = wrapElements(g.__rule_get(S(rule), '', 'elements'));
    return list.length ? list[0] : null;
  };

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
  J.lang = {
    String: function (s) { return new String(S(s)); },
    System: { currentTimeMillis: function () { return Date.now(); } },
    Integer: { parseInt: function (s) { return parseInt(s, 10); }, valueOf: function (s) { return parseInt(s, 10); } }
  };
  g.Packages = { java: J };

  g.cache = {
    get: function (k) { return g.__store_get(S(k)); },
    put: function (k, v) { g.__store_put(S(k), S(v)); return v; },
    'delete': function (k) { g.__store_del(S(k)); },
    getFile: function (k) { return g.__store_get('file:' + S(k)); },
    putFile: function (k, v) { g.__store_put('file:' + S(k), S(v)); return v; },
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
  src.bookSourceUrl = si.url;
  src.bookSourceName = si.name;
  src.getVariable = function () { return g.__store_get('sv:' + si.url) || ''; };
  src.setVariable = function (v) { g.__store_put('sv:' + si.url, S(v)); };
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
    // legado 的 Book/BookChapter 只实现 RuleDataInterface(没有 getKey), 只有
    // BookSource 有。但插件常做 `book.getKey ? book.getKey() : book.bookUrl` 式探测,
    // 因此按"主键即 bookUrl/章节 url"补上, 与 `source.getKey()` 语义保持一致。
    o.getKey = function () { return S(o[keyField]); };
    return o;
  }
  var bc = JSON.parse(g.__book_ctx());
  g.book = withVariables(bc.book || {}, 'bv:', 'bookUrl');
  g.chapter = withVariables(bc.chapter || {}, 'cv:', 'url');
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
    shared_js: &str,
    f: impl FnOnce(rquickjs::Ctx<'_>) -> anyhow::Result<T>,
) -> anyhow::Result<T> {
    let nested = JS_EVAL_DEPTH.with(|depth| depth.get() > 0);
    JS_EVAL_DEPTH.with(|depth| depth.set(depth.get() + 1));
    let _guard = EvalDepthGuard;

    // 嵌套调用无法借用外层正在使用的 Context, 退化为一次性上下文(仍需加载 jsLib,
    // 否则嵌套脚本里的 jsLib 函数不可用)。
    if nested {
        let runtime = Runtime::new()?;
        let context = Context::full(&runtime)?;
        if !shared_js.trim().is_empty() {
            context.with(|ctx| {
                if let Err(e) = eval_script(ctx.clone(), shared_js) {
                    push_js_log(format!("[jsLib] 执行失败: {e}"));
                }
            });
        }
        return context.with(f);
    }

    let cached = JS_CONTEXT.with(|cell| {
        let slot = cell.borrow();
        match slot.as_ref() {
            Some(entry) if entry.lib_key == lib_key => Some(entry.context.clone()),
            _ => None,
        }
    });

    let context = match cached {
        Some(context) => context,
        None => {
            let runtime = Runtime::new()?;
            let context = Context::full(&runtime)?;
            // jsLib 出错不拖垮规则本身: 记日志后继续(与 legado 共享作用域加载失败一致)
            if !shared_js.trim().is_empty() {
                context.with(|ctx| {
                    if let Err(e) = eval_script(ctx.clone(), shared_js) {
                        push_js_log(format!("[jsLib] 执行失败: {e}"));
                    }
                });
            }
            JS_CONTEXT.with(|cell| {
                *cell.borrow_mut() = Some(CachedJsContext {
                    lib_key: lib_key.to_string(),
                    context: context.clone(),
                });
            });
            context
        }
    };

    context.with(f)
}

fn eval_js_inner_with_source(
    script: &str,
    input: Option<&str>,
    base_url: Option<&str>,
    key: Option<&str>,
    page: Option<i32>,
    source_key: Option<&str>,
    bindings: Option<&HashMap<String, JsonValue>>,
) -> anyhow::Result<String> {
    let shared_js = active_js_lib_script();
    let source_info = active_source();
    let book_ctx = ACTIVE_BOOK_CTX.with(|cell| cell.borrow().clone()).unwrap_or_default();

    // 复用键必须带上书源: 不同书源(尤其都没有 jsLib 时)不能共用同一个上下文,
    // 否则规则脚本声明的全局变量会互相可见。同一书源的连续章节仍能命中复用。
    let lib_key = format!("{}\u{1}{}", source_info.url, md5_hex(&shared_js));
    with_reusable_context(&lib_key, &shared_js, |ctx| {
        let globals = ctx.globals();
        let input_value = input.unwrap_or("");
        let base_url_value = base_url.unwrap_or("");

        globals.set("input", input_value)?;
        globals.set("result", input_value)?;
        globals.set("src", input_value)?;
        globals.set("base_url", base_url_value)?;
        globals.set("baseUrl", base_url_value)?;
        if let Some(key) = key {
            globals.set("key", key)?;
        }
        if let Some(page) = page {
            globals.set("page", page)?;
        }

        // Default url variable for Legado compatibility
        globals.set("url", base_url_value)?;

        let source_key_val = source_key.unwrap_or("").to_string();
        let source_obj = Object::new(ctx.clone())?;
        let key_clone = source_key_val.clone();
        source_obj.set("key", source_key_val)?;
        // getKey: 优先返回显式传入的 source_key; 否则回退到解析期 thread-local 的书籍 URL
        source_obj.set(
            "getKey",
            Func::new(move || {
                if !key_clone.is_empty() {
                    key_clone.clone()
                } else {
                    active_source_key().unwrap_or_default()
                }
            }),
        )?;
        globals.set("source", source_obj)?;

        let cookie_obj = Object::new(ctx.clone())?;
        cookie_obj.set(
            "getCookie",
            Func::new(|_key: String| -> String { String::new() }),
        )?;
        cookie_obj.set(
            "removeCookie",
            Func::new(|_key: String| -> String { "".to_string() }),
        )?;
        globals.set("cookie", cookie_obj)?;

        // 原生原语, 由 JS_PRELUDE 组装成 legado API
        let default_headers = source_info.headers.clone();
        globals.set(
            "__http",
            Func::new(
                move |method: String, url: String, body: String, headers: String| -> String {
                    js_http(&method, &url, &body, &headers, &default_headers).to_string()
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
        globals.set(
            "__sym",
            Func::new(
                |op: String,
                 transformation: String,
                 key: String,
                 iv: String,
                 data: String,
                 format: String|
                 -> String {
                    match symmetric_codec(&op, &transformation, &key, &iv, &data, &format) {
                        Ok(value) => value,
                        Err(e) => {
                            push_js_log(format!("[crypto] {transformation}: {e}"));
                            String::new()
                        }
                    }
                },
            ),
        )?;
        globals.set(
            "__log",
            Func::new(|kind: String, message: String| {
                push_js_log(format!("[{kind}] {message}"));
            }),
        )?;
        // java.showBrowser(url, html, preloadJs): 捕获请求交给前端弹面板(legado 的 BottomWebViewDialog)
        globals.set(
            "__show_browser",
            Func::new(|url: String, html: String, preload_js: String| {
                let html = (!html.trim().is_empty()).then(|| html.clone());
                let preload_js = (!preload_js.trim().is_empty()).then(|| preload_js.clone());
                push_js_log(format!("[browser] showBrowser: {url}"));
                crate::service::book_service::set_show_browser_request(
                    crate::service::book_service::ShowBrowserRequest {
                        url,
                        html,
                        preload_js,
                        panel_id: None,
                    },
                );
            }),
        )?;
        // 规则解析原语: 供 `java.getString` / `getStringList` / `getElement(s)` 使用
        // (legado `AnalyzeRule` 是 `bindings["java"]`, 这些方法用规则解析当前页面内容)
        let rule_input = input_value.to_string();
        let rule_base_url = base_url_value.to_string();
        globals.set(
            "__rule_get",
            Func::new(move |rule: String, override_input: String, mode: String| -> String {
                // override_input 为空时用当前页面内容(元素内部查询会显式传入片段)
                let content = if override_input.is_empty() {
                    rule_input.clone()
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
                    _ => crate::parser::rule_engine::RuleEngine::eval_rule_on(
                        &rule,
                        &content,
                        &rule_base_url,
                        false,
                    )
                    .into_iter()
                    .next()
                    .unwrap_or_default(),
                }
            }),
        )?;
        globals.set(
            "__store_get",
            Func::new(|key: String| -> Option<String> { store_get(&key) }),
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
        let source_json = json!({ "url": source_info.url, "name": source_info.name }).to_string();
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
        java_obj.set("deviceID", Func::new(|| -> String { JS_DEVICE_ID.clone() }))?;
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
                base64::engine::general_purpose::STANDARD
                    .decode(input)
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
        java_obj.set(
            "encodeURI",
            Func::new(|input: String| -> String { urlencoding::encode(&input).into_owned() }),
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
            Func::new(|input: String, algorithm: String| -> String {
                java_digest_hex(&input, &algorithm)
            }),
        )?;
        java_obj.set(
            "digestBase64Str",
            Func::new(|input: String, algorithm: String| -> String {
                java_digest_base64(&input, &algorithm)
            }),
        )?;
        java_obj.set(
            "HMacHex",
            Func::new(|input: String, algorithm: String, key: String| -> String {
                java_hmac_hex(&input, &algorithm, &key)
            }),
        )?;
        java_obj.set(
            "HMacBase64",
            Func::new(|input: String, algorithm: String, key: String| -> String {
                java_hmac_base64(&input, &algorithm, &key)
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
        java_obj.set(
            "htmlFormat",
            Func::new(|input: String| -> String { java_html_format(&input) }),
        )?;
        java_obj.set(
            "toNumChapter",
            Func::new(|input: String| -> String { java_to_num_chapter(&input) }),
        )?;
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

        // prelude 每次都要跑: 它包装的是本次新建的 java 对象与本次的 source/book/chapter
        eval_script(ctx.clone(), JS_PRELUDE)?;
        // 注意: jsLib 已在 with_reusable_context 建立上下文时执行过一次, 这里不重复执行。
        // 调用方显式绑定放在 jsLib 之后, 避免被 jsLib 顶层同名变量覆盖
        if let Some(bindings) = bindings {
            for (key, value) in bindings {
                let js_value = ctx.json_parse(value.to_string())?;
                globals.set(key.as_str(), js_value)?;
            }
        }

        let v = eval_script(ctx.clone(), script)?;
        globals.set("__result", v)?;
        // String 包装对象 / java.get 响应对象按 legado 语义转成字符串
        let v: Value = eval_script(
            ctx.clone(),
            "(function (v) { if (v instanceof String) return v.valueOf(); if (v && v.__isResp) return v.body(); return v; })(__result)",
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
    })
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
fn symmetric_crypt(
    encrypt: bool,
    transformation: &str,
    key: &[u8],
    iv: &[u8],
    data: &[u8],
) -> anyhow::Result<Vec<u8>> {
    let mut parts = transformation.split('/').map(|p| p.trim().to_ascii_uppercase());
    let algorithm = parts.next().unwrap_or_default();
    let mode = match parts.next().as_deref() {
        Some("CBC") => CipherMode::Cbc,
        Some("ECB") | Some("") | None => CipherMode::Ecb,
        Some(other) => anyhow::bail!("不支持的模式 {other}"),
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
            n => anyhow::bail!("AES 密钥长度无效: {n}"),
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
        other => anyhow::bail!("不支持的算法 {other}"),
    }
}

/// `__sym` 原语: 加密时 data 为明文, 按 format(base64/hex) 输出;
/// 解密时按 format(base64/hex/auto) 解码输入, 输出 UTF-8 文本。
fn symmetric_codec(
    op: &str,
    transformation: &str,
    key: &str,
    iv: &str,
    data: &str,
    format: &str,
) -> anyhow::Result<String> {
    let engine = &base64::engine::general_purpose::STANDARD;
    if op == "enc" {
        let out = symmetric_crypt(true, transformation, key.as_bytes(), iv.as_bytes(), data.as_bytes())?;
        return Ok(if format == "hex" {
            hex::encode(out)
        } else {
            engine.encode(out)
        });
    }
    let trimmed = data.trim();
    let is_hex = !trimmed.is_empty()
        && trimmed.len() % 2 == 0
        && trimmed.chars().all(|c| c.is_ascii_hexdigit());
    let bytes = match format {
        "hex" => hex::decode(trimmed)?,
        "auto" if is_hex => hex::decode(trimmed)?,
        _ => engine.decode(trimmed)?,
    };
    let out = symmetric_crypt(false, transformation, key.as_bytes(), iv.as_bytes(), &bytes)?;
    Ok(String::from_utf8_lossy(&out).into_owned())
}

fn eval_script<'js>(ctx: rquickjs::Ctx<'js>, script: &str) -> anyhow::Result<Value<'js>> {
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
                return Err(anyhow::anyhow!("JS 异常: {caught:?}"));
            }
            Err(e.into())
        }
    }
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
fn java_digest_hex(input: &str, algorithm: &str) -> String {
    let bytes = input.as_bytes();
    let out = match normalize_digest_name(algorithm).as_str() {
        "MD5" => hex::encode(Md5::digest(bytes)),
        "SHA-1" => hex::encode(Sha1::digest(bytes)),
        "SHA-224" => hex::encode(Sha224::digest(bytes)),
        "SHA-256" => hex::encode(Sha256::digest(bytes)),
        "SHA-384" => hex::encode(Sha384::digest(bytes)),
        "SHA-512" => hex::encode(Sha512::digest(bytes)),
        _ => return String::new(),
    };
    out
}

/// legado `java.digestBase64Str(data, algorithm)`: 同上但 base64 输出
fn java_digest_base64(input: &str, algorithm: &str) -> String {
    let bytes = input.as_bytes();
    let out = match normalize_digest_name(algorithm).as_str() {
        "MD5" => base64::engine::general_purpose::STANDARD.encode(Md5::digest(bytes)),
        "SHA-1" => base64::engine::general_purpose::STANDARD.encode(Sha1::digest(bytes)),
        "SHA-224" => base64::engine::general_purpose::STANDARD.encode(Sha224::digest(bytes)),
        "SHA-256" => base64::engine::general_purpose::STANDARD.encode(Sha256::digest(bytes)),
        "SHA-384" => base64::engine::general_purpose::STANDARD.encode(Sha384::digest(bytes)),
        "SHA-512" => base64::engine::general_purpose::STANDARD.encode(Sha512::digest(bytes)),
        _ => return String::new(),
    };
    out
}

/// legado `java.HMacHex(data, algorithm, key)`: HMAC 摘要(hex)
fn java_hmac_hex(input: &str, algorithm: &str, key: &str) -> String {
    java_hmac(input, algorithm, key, true)
}

/// legado `java.HMacBase64(data, algorithm, key)`: HMAC 摘要(base64)
fn java_hmac_base64(input: &str, algorithm: &str, key: &str) -> String {
    java_hmac(input, algorithm, key, false)
}

/// 通用 HMAC 计算, 按算法名分发到对应摘要类型
fn java_hmac(input: &str, algorithm: &str, key: &str, hex_out: bool) -> String {
    let key_bytes = key.as_bytes();
    let encoded: Option<String> = match normalize_digest_name(algorithm).as_str() {
        "MD5" => hmac_md5(input, key_bytes, hex_out),
        "SHA-1" => hmac_sha1(input, key_bytes, hex_out),
        "SHA-224" => hmac_sha224(input, key_bytes, hex_out),
        "SHA-256" => hmac_sha256(input, key_bytes, hex_out),
        "SHA-384" => hmac_sha384(input, key_bytes, hex_out),
        "SHA-512" => hmac_sha512(input, key_bytes, hex_out),
        _ => None,
    };
    encoded.unwrap_or_default()
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

/// 归一化摘要算法名, 兼容各种大小写/连字符写法
fn normalize_digest_name(algorithm: &str) -> String {
    let a = algorithm.trim().to_uppercase();
    match a.as_str() {
        "MD5" => "MD5".to_string(),
        "SHA" | "SHA1" | "SHA-1" => "SHA-1".to_string(),
        "SHA224" | "SHA-224" => "SHA-224".to_string(),
        "SHA256" | "SHA-256" => "SHA-256".to_string(),
        "SHA384" | "SHA-384" => "SHA-384".to_string(),
        "SHA512" | "SHA-512" => "SHA-512".to_string(),
        other => other.to_string(),
    }
}

/// legado `java.htmlFormat(str)`: 解码 HTML 实体为纯文本
fn java_html_format(input: &str) -> String {
    input
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
) -> JsonValue {
    let url = url.trim().to_string();
    if url.is_empty() {
        return json!({ "body": "", "code": 0, "url": "", "headers": {} });
    }
    let method = Method::from_bytes(method.to_ascii_uppercase().as_bytes()).unwrap_or(Method::GET);
    let mut merged: Vec<(String, String)> = default_headers.to_vec();
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
    let request_url = url.clone();
    let outcome = run_blocking_http(move || -> anyhow::Result<JsonValue> {
        let mut req = JS_HTTP_CLIENT.request(method, &request_url);
        for (key, value) in merged {
            req = req.header(key, value);
        }
        if let Some(body) = body {
            req = req.body(body);
        }
        let response = req.send()?;
        let code = response.status().as_u16();
        let final_url = response.url().to_string();
        let headers: serde_json::Map<String, JsonValue> = response
            .headers()
            .iter()
            .map(|(k, v)| {
                (
                    k.as_str().to_ascii_lowercase(),
                    JsonValue::String(v.to_str().unwrap_or_default().to_string()),
                )
            })
            .collect();
        let text = response.text().unwrap_or_default();
        Ok(json!({ "body": text, "code": code, "url": final_url, "headers": headers }))
    });
    outcome.unwrap_or_else(|e| {
        push_js_log(format!("[http] {url}: {e}"));
        json!({ "body": "", "code": 0, "url": url, "headers": {} })
    })
}

fn js_http(
    method: &str,
    url: &str,
    body: &str,
    headers: &str,
    default_headers: &[(String, String)],
) -> JsonValue {
    let headers = serde_json::from_str::<JsonValue>(headers).unwrap_or(JsonValue::Null);
    let body = (!body.is_empty() || method.eq_ignore_ascii_case("POST")).then(|| body.to_string());
    js_request(method, url, body, &headers, default_headers)
}

/// legado `java.ajax(url)` / `java.connect(url, header)`: url 可带 `,{method, body, headers}` 选项。
fn js_connect(spec: &str, headers: &str, default_headers: &[(String, String)]) -> JsonValue {
    let (url, options) = split_ajax_spec(spec);
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
    js_request(&method, url, body, &JsonValue::Object(merged), default_headers)
}

fn split_ajax_spec(spec: &str) -> (&str, Option<&str>) {
    let mut depth = 0i32;
    let mut in_string = false;
    let mut quote = '\0';
    let mut escaped = false;

    for (idx, ch) in spec.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }

        match ch {
            '\\' if in_string => {
                escaped = true;
            }
            '"' | '\'' if in_string && ch == quote => {
                in_string = false;
                quote = '\0';
            }
            '"' | '\'' if !in_string => {
                in_string = true;
                quote = ch;
            }
            '{' | '[' if !in_string => depth += 1,
            '}' | ']' if !in_string => depth -= 1,
            ',' if !in_string && depth == 0 => {
                let left = &spec[..idx];
                let right = &spec[idx + ch.len_utf8()..];
                return (left, Some(right.trim()));
            }
            _ => {}
        }
    }

    (spec, None)
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
    let headers = source
        .header
        .as_deref()
        .map(parse_source_headers)
        .unwrap_or_default();
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
            let url = text_of("url");
            let body = text_of("body");
            let extra = headers_text("headers");
            let mut merged = headers;
            if !extra.trim().is_empty() {
                merged.extend(parse_source_headers(&extra));
            }
            let _ = base_url;
            js_http(&method, &url, &body, "{}", &merged)
        }
        other => anyhow::bail!("未知的请求类型 {other}"),
    };

    Ok(envelope
        .get("body")
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string())
}

/// 面板 `java.createSymmetricCrypto` 的桥实现, 复用书源规则的同一套密码学。
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
            let bytes = base64::engine::general_purpose::STANDARD
                .decode(input.trim())
                .map_err(|e| anyhow::anyhow!("base64 解码失败: {e}"))?;
            Ok(String::from_utf8_lossy(&bytes).into_owned())
        }
        other => anyhow::bail!("未知的摘要类型 {other}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
    /// 因此按主键提供: book → bookUrl, chapter → 章节 url, 与 `source.getKey()` 一致。
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

        assert_eq!(
            eval_js("java.getElement('.item').attr('data-id')", html, "").unwrap(),
            "7"
        );
        // 没有匹配时返回 null
        assert_eq!(
            eval_js("String(java.getElement('.nope'))", html, "").unwrap(),
            "null"
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
        // XPath 取单个元素时也要能取到内容
        assert_eq!(
            eval_js("java.getElement('@xpath://p').text()", html, "").unwrap(),
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
                with_source_key(Some("https://panel-bind.example/1"), || {
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
}
