use crate::util::hash::md5_hex;
use crate::util::text::{apply_regex_replace, strip_whitespace};
use aes::Aes128;
use base64::Engine;
use cbc::cipher::{block_padding::Pkcs7, BlockDecryptMut, KeyIvInit};
use chrono::{Local, TimeZone, Utc};
use hmac::{Hmac, Mac};
use md5::Md5;
use once_cell::sync::Lazy;
use sha1::Sha1;
use sha2::{Digest, Sha224, Sha256, Sha384, Sha512};
use reqwest::blocking::Client;
use reqwest::Method;
use rquickjs::function::Func;
use rquickjs::{Context, Object, Runtime, Value};
use serde_json::Value as JsonValue;
use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::Mutex;
use uuid::Uuid;

static JS_KV: Lazy<Mutex<HashMap<String, String>>> = Lazy::new(|| Mutex::new(HashMap::new()));
static JS_LIB_CACHE: Lazy<Mutex<HashMap<String, String>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));
static JS_HTTP_CLIENT: Lazy<Client> = Lazy::new(|| {
    Client::builder()
        .cookie_store(true)
        .gzip(true)
        .brotli(true)
        .deflate(true)
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
type Aes128CbcDecryptor = cbc::Decryptor<Aes128>;
thread_local! {
    static ACTIVE_JS_LIB: RefCell<Option<String>> = const { RefCell::new(None) };
    static ACTIVE_SOURCE_KEY: RefCell<Option<String>> = const { RefCell::new(None) };
}

pub fn with_js_lib<T>(js_lib: Option<&str>, f: impl FnOnce() -> T) -> T {
    ACTIVE_JS_LIB.with(|cell| {
        let previous = cell.replace(js_lib.map(|value| value.to_string()));
        let result = f();
        cell.replace(previous);
        result
    })
}

/// 在解析目录/正文期间设置当前书籍 URL, 供 JS `source.getKey()` 读取。
/// 例如规则 `source.getKey().match(/\d+/)` 需要书籍 bookUrl 中的 id。
pub fn with_source_key<T>(source_key: Option<&str>, f: impl FnOnce() -> T) -> T {
    ACTIVE_SOURCE_KEY.with(|cell| {
        let previous = cell.replace(source_key.map(|value| value.to_string()));
        let result = f();
        cell.replace(previous);
        result
    })
}

fn active_source_key() -> Option<String> {
    ACTIVE_SOURCE_KEY.with(|cell| cell.borrow().clone())
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

fn eval_js_inner_with_source(
    script: &str,
    input: Option<&str>,
    base_url: Option<&str>,
    key: Option<&str>,
    page: Option<i32>,
    source_key: Option<&str>,
    bindings: Option<&HashMap<String, JsonValue>>,
) -> anyhow::Result<String> {
    let rt = Runtime::new()?;
    let ctx = Context::full(&rt)?;
    ctx.with(|ctx| {
        let globals = ctx.globals();
        let input_value = input.unwrap_or("");
        let base_url_value = base_url.unwrap_or("");
        let shared_js = active_js_lib_script()?;

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

        // Stubs for Legado compatibility
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

        let cache_obj = Object::new(ctx.clone())?;
        cache_obj.set(
            "get",
            Func::new(|key: String| -> Option<String> {
                let map = JS_KV.lock().unwrap_or_else(|e| e.into_inner());
                map.get(&key).cloned()
            }),
        )?;
        cache_obj.set(
            "put",
            Func::new(|key: String, val: String| -> bool {
                let mut map = JS_KV.lock().unwrap_or_else(|e| e.into_inner());
                map.insert(key, val);
                true
            }),
        )?;
        globals.set("cache", cache_obj)?;

        let java_obj = Object::new(ctx.clone())?;
        java_obj.set(
            "ajax",
            Func::new(|spec: String| -> String { java_ajax(&spec).unwrap_or_default() }),
        )?;
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
        java_obj.set(
            "get",
            Func::new(|url: String| -> String {
                java_request_simple("GET", &url, None).unwrap_or_default()
            }),
        )?;
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
                let mut map = JS_KV.lock().unwrap_or_else(|e| e.into_inner());
                map.insert(format!("__global_{key}"), value);
                true
            }),
        )?;
        // cache 别名: legado 用 java.getCache / java.putCache
        java_obj.set(
            "getCache",
            Func::new(|key: String| -> Option<String> {
                let map = JS_KV.lock().unwrap_or_else(|e| e.into_inner());
                map.get(&key).cloned()
            }),
        )?;
        java_obj.set(
            "putCache",
            Func::new(|key: String, val: String| -> bool {
                let mut map = JS_KV.lock().unwrap_or_else(|e| e.into_inner());
                map.insert(key, val);
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
            "post",
            Func::new(|url: String, body: String| -> String {
                java_request_simple("POST", &url, Some(body)).unwrap_or_default()
            }),
        )?;
        java_obj.set(
            "put",
            Func::new(|url: String, body: String| -> String {
                java_request_simple("PUT", &url, Some(body)).unwrap_or_default()
            }),
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
            "aesBase64DecodeToString",
            Func::new(
                |input: String, key: String, algorithm: String, iv: String| -> String {
                    java_aes_base64_decode_to_string(&input, &key, &algorithm, &iv)
                },
            ),
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
            Func::new(|key: String| -> Option<String> {
                let map = JS_KV.lock().unwrap_or_else(|e| e.into_inner());
                map.get(&key).cloned()
            }),
        )?;
        globals.set(
            "kv_put",
            Func::new(|key: String, val: String| -> bool {
                let mut map = JS_KV.lock().unwrap_or_else(|e| e.into_inner());
                map.insert(key, val);
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

        globals.set("book", Object::new(ctx.clone())?)?;
        globals.set("chapter", Object::new(ctx.clone())?)?;
        globals.set("title", "")?;
        globals.set("nextChapterUrl", "")?;
        globals.set("rssArticle", Object::new(ctx.clone())?)?;

        if let Some(bindings) = bindings {
            for (key, value) in bindings {
                let js_value = ctx.json_parse(value.to_string())?;
                globals.set(key.as_str(), js_value)?;
            }
        }

        if !shared_js.trim().is_empty() {
            eval_script(ctx.clone(), &shared_js)?;
        }

        let v = eval_script(ctx.clone(), script)?;

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

fn java_aes_base64_decode_to_string(input: &str, key: &str, algorithm: &str, iv: &str) -> String {
    let algorithm = algorithm.to_ascii_uppercase();
    if algorithm != "AES/CBC/PKCS5PADDING" && algorithm != "AES/CBC/PKCS7PADDING" {
        return String::new();
    }

    let Ok(mut encrypted) = base64::engine::general_purpose::STANDARD.decode(input.trim()) else {
        return String::new();
    };

    let Ok(cipher) = Aes128CbcDecryptor::new_from_slices(key.as_bytes(), iv.as_bytes()) else {
        return String::new();
    };

    cipher
        .decrypt_padded_mut::<Pkcs7>(&mut encrypted)
        .ok()
        .and_then(|bytes| String::from_utf8(bytes.to_vec()).ok())
        .unwrap_or_default()
}

fn eval_script<'js>(ctx: rquickjs::Ctx<'js>, script: &str) -> anyhow::Result<Value<'js>> {
    match ctx.eval(script) {
        Ok(v) => Ok(v),
        Err(e) => {
            if let Some(exception) = ctx.catch().into_exception() {
                return Err(anyhow::anyhow!("JS Exception: {:?}", exception));
            }
            Err(e.into())
        }
    }
}

fn active_js_lib_script() -> anyhow::Result<String> {
    let js_lib = ACTIVE_JS_LIB.with(|cell| cell.borrow().clone());
    let Some(js_lib) = js_lib.filter(|value| !value.trim().is_empty()) else {
        return Ok(String::new());
    };
    let cache_key = md5_hex(&js_lib);
    if let Some(cached) = JS_LIB_CACHE
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get(&cache_key)
        .cloned()
    {
        return Ok(cached);
    }

    let compiled = compile_js_lib(&js_lib)?;
    JS_LIB_CACHE
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .insert(cache_key, compiled.clone());
    Ok(compiled)
}

fn compile_js_lib(js_lib: &str) -> anyhow::Result<String> {
    let trimmed = js_lib.trim();
    if trimmed.is_empty() {
        return Ok(String::new());
    }
    if trimmed.starts_with('{') {
        if let Ok(value) = serde_json::from_str::<JsonValue>(trimmed) {
            if let Some(map) = value.as_object() {
                let mut scripts = Vec::new();
                for entry in map.values() {
                    if let Some(raw) = entry.as_str() {
                        scripts.push(resolve_js_lib_entry(raw)?);
                    }
                }
                return Ok(scripts.join("\n"));
            }
        }
    }
    Ok(trimmed.to_string())
}

fn resolve_js_lib_entry(entry: &str) -> anyhow::Result<String> {
    let value = entry.trim();
    if value.starts_with("http://") || value.starts_with("https://") {
        let response = JS_HTTP_CLIENT.get(value).send()?;
        return Ok(response.text().unwrap_or_default());
    }
    Ok(value.to_string())
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

fn java_ajax(spec: &str) -> anyhow::Result<String> {
    let (url, options) = split_ajax_spec(spec);
    if url.trim().is_empty() {
        return Ok(String::new());
    }

    let options_json = options
        .and_then(|raw| serde_json::from_str::<JsonValue>(raw).ok())
        .unwrap_or(JsonValue::Null);

    let method = options_json
        .get("method")
        .and_then(|v| v.as_str())
        .unwrap_or("GET")
        .to_uppercase();
    let method = Method::from_bytes(method.as_bytes()).unwrap_or(Method::GET);

    let mut req = JS_HTTP_CLIENT.request(method, url.trim());

    if let Some(headers) = options_json.get("headers").and_then(|v| v.as_object()) {
        for (key, value) in headers {
            if let Some(value) = value.as_str() {
                req = req.header(key, value);
            } else if !value.is_null() {
                req = req.header(key, value.to_string());
            }
        }
    }

    if let Some(body) = options_json.get("body") {
        if let Some(body) = body.as_str() {
            req = req.body(body.to_string());
        } else if !body.is_null() {
            req = req.body(body.to_string());
        }
    }

    let response = req.send()?;
    Ok(response.text().unwrap_or_default())
}

fn java_request_simple(method: &str, url: &str, body: Option<String>) -> anyhow::Result<String> {
    let method = Method::from_bytes(method.as_bytes()).unwrap_or(Method::GET);
    let mut req = JS_HTTP_CLIENT.request(method, url.trim());
    if let Some(body) = body {
        req = req.body(body);
    }
    let response = req.send()?;
    Ok(response.text().unwrap_or_default())
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn java_date_format_to_chrono_converts_common_patterns() {
        assert_eq!(java_date_format_to_chrono("yyyy-MM-dd HH:mm:ss"), "%Y-%m-%d %H:%M:%S");
        assert_eq!(java_date_format_to_chrono("yyyy/MM/dd"), "%Y/%m/%d");
        assert_eq!(java_date_format_to_chrono("MM-dd"), "%m-%d");
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
}
