use crate::crawler::fetcher::{HttpMethod, RequestSpec};
use crate::error::error::AppError;
use crate::model::book_source::BookSource;
use crate::parser::js::{eval_js, eval_js_url, with_source, login_headers};
use crate::util::text::normalize_source_url;
use encoding_rs::Encoding;
use regex::Regex;
use serde_json::Value;
use std::collections::HashMap;

pub const DEFAULT_USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36";

#[derive(Debug, Clone, Default)]
pub struct HeaderSpec {
    pub headers: Vec<(String, String)>,
    pub proxy: Option<String>,
}

pub fn analyze_url(
    m_url: &str,
    key: &str,
    page: i32,
    base_url: &str,
    source: &BookSource,
) -> Result<RequestSpec, AppError> {
    with_source(source,|| {
        let base_url = strip_url_options(&normalize_source_url(base_url)).to_string();
        let mut rule_url = m_url.to_string();
        let mut header_spec = source_header_spec(source)?;

        rule_url = eval_url_js_segments(&rule_url, key, page, &source.book_source_url, &base_url)?;
        rule_url = replace_inline_js(&rule_url, key, page, &source.book_source_url, &base_url)?;
        rule_url = replace_legacy_placeholders(&rule_url, key, page);
        rule_url = replace_page_choices(&rule_url, page);

        let (url_part, options) = split_url_options(&rule_url);
        let mut url = absolute_url(&base_url, url_part.trim());
        let mut method = HttpMethod::GET;
        let mut body = None;
        let mut retry = 2usize;
        let mut response_type = None;
        let mut charset = None;
        let mut server_id = None;
        let mut web_view = false;
        let mut web_js = None;
        let mut web_view_delay_time = 0u64;

        if let Some(options) = options {
            let options = parse_url_options(options)?;
            if let Some(raw_method) = options.get("method").and_then(Value::as_str) {
                method = match raw_method.to_ascii_uppercase().as_str() {
                    "POST" => HttpMethod::POST, "HEAD" => HttpMethod::HEAD,
                    "PUT" => HttpMethod::PUT, "PATCH" => HttpMethod::PATCH,
                    "DELETE" => HttpMethod::DELETE, _ => HttpMethod::GET,
                };
            }
            if let Some(value) = options.get("charset").and_then(Value::as_str) {
                if !value.trim().is_empty() {
                    charset = Some(value.to_string());
                }
            }
            if let Some(value) = options.get("headers") {
                let extra = headers_from_value(value)?;
                merge_headers(&mut header_spec, extra);
            }
            if let Some(value) = options.get("body") {
                body = Some(match value {
                    Value::String(value) => value.clone(),
                    other => other.to_string(),
                });
            }
            if let Some(value) = options.get("retry") {
                retry = parse_usize(value).unwrap_or(0);
            }
            if let Some(value) = options.get("type").and_then(Value::as_str) {
                if !value.trim().is_empty() {
                    response_type = Some(value.to_string());
                }
            }
            if let Some(value) = options.get("serverID").or_else(|| options.get("serverId")) {
                server_id = parse_i64(value);
            }
            if let Some(value) = options.get("webView") {
                web_view = is_truthy_webview(value);
            }
            if let Some(value) = options.get("webJs").and_then(Value::as_str) {
                web_js = Some(value.to_string());
            }
            if let Some(value) = options.get("webViewDelayTime") {
                web_view_delay_time = parse_i64(value).unwrap_or(0).max(0) as u64;
            }
            if let Some(script) = options.get("js").and_then(Value::as_str) {
                if !script.trim().is_empty() {
                    // legado `AnalyzeUrl`：
                    //   option.getJs()?.let { evalJS(it, url)?.toString()?.let { url = it } }
                    // 脚本的**返回值**才替换 url；`evalJS` 返回 `null`（Rhino 的
                    // undefined）时 `?.let` 不执行，url 保持不变。
                    //
                    // 这里的 `js` 选项在真实书源里大量用作「请求前副作用」（清限频
                    // Cookie 之类），末尾往往是 `try{cookie.removeCookie(u)}catch(e){}`
                    // 这种语句块 —— 按 ECMAScript 规范它**有完成值**（`try` 块里最后
                    // 一条表达式的值，如 `true`），于是 url 被改成 `"true"`，紧接着
                    // `absolute_url` 拼出一个非法地址，报
                    // `builder error: relative URL without a base`，症状是**搜索恒失败**。
                    //
                    // 所以只接受「看起来确实是地址」的返回值（带 scheme 或站内绝对
                    // 路径），其余一律保留原 url —— 这个源的脚本末尾就是
                    // `try{cookie.removeCookie(u)}catch(e){}`，完成值为 `true`。
                    if let Ok(value) = eval_js_url(script, &url, key, page, &source.book_source_url, &base_url) {
                        let value = value.trim();
                        if looks_like_url(value) {
                            url = value.to_string();
                        }
                    }
                }
            }
        }

        url = encode_get_query(&url, charset.as_deref());
        if matches!(method, HttpMethod::POST) {
            if let Some(raw_body) = body.take() {
                body = Some(encode_post_body(
                    &raw_body,
                    charset.as_deref(),
                    has_header(&header_spec.headers, "content-type"),
                ));
            }
        }

        Ok(RequestSpec {
            url,
            method,
            headers: header_spec.headers,
            body,
            body_bytes: None,
            retry,
            response_type,
            charset,
            proxy: header_spec.proxy,
            timeout: None,
            server_id,
            web_view,
            web_js,
            web_view_delay_time,
            // 规则解析出的请求一律由 fetcher 自己跟随 3xx（换域跳转时按新地址
            // 重建 `Host`，见 `fetcher::fetch_inner`）；书源若要读 302 的
            // `Location`，走的是 `java.get/post/head` 那条显式设 false 的路径。
            follow_redirects: true,
        })
    })
}

/// 解析 URL 的请求选项（legado 的 `url,{...}` 语法）。
///
/// 三段式，逐级放宽：
/// 1. 严格 JSON —— 规范书源走这里。
/// 2. 转义控制字符后重试 —— 选项里含裸换行/Tab 的。
/// 3. **交给 JS 引擎求值** —— 选项写成 JS 对象字面量时（单引号键名/值、无引号
///    键名、尾逗号）。该源的选项就是这一形态：
///    `{'headers':'{"User-Agent": "..."}'}`，`serde_json` 直接拒收
///    （`key must be a string`），于是整个分类加载失败、界面只剩 internal error。
///    legado 用 GSON 的宽松模式能读，所以手机端正常。
///
/// 只在两条 JSON 路径都失败后尝试求值，正常书源不受影响；求值失败仍按原样报错。
fn parse_url_options(options: &str) -> Result<Value, AppError> {
    let trimmed = options.trim();
    let parsed = serde_json::from_str::<Value>(trimmed)
        .or_else(|_| serde_json::from_str::<Value>(&escape_control_chars_in_json_strings(trimmed)));
    if let Ok(value) = parsed {
        return Ok(value);
    }
    if let Some(value) = eval_options_as_js_literal(trimmed) {
        return Ok(value);
    }
    // 三条路都走不通：用严格 JSON 的报错信息，便于定位格式问题。
    serde_json::from_str::<Value>(trimmed)
        .map_err(|e| AppError::BadRequest(format!("invalid url options: {}", e)))
}

/// 把 `{...}` 形式的 JS 对象字面量求值成 JSON。
///
/// 判据保守：整体是大括号包裹才尝试，避免把普通字符串丢进 JS 引擎。
/// 返回值必须是 JSON **对象**，否则视为解析失败（例如字面量求值出个数组）。
fn eval_options_as_js_literal(trimmed: &str) -> Option<Value> {
    if !(trimmed.starts_with('{') && trimmed.ends_with('}')) {
        return None;
    }
    // 括号包裹后求值，避免 `{}` 被当作代码块（那是语句，不产生值）。
    let escaped = crate::parser::js::eval_js(&format!("({trimmed})"), "", "").ok()?;
    let value: Value = serde_json::from_str(escaped.trim()).ok()?;
    value.is_object().then_some(value)
}

fn escape_control_chars_in_json_strings(input: &str) -> String {
    let mut output = String::with_capacity(input.len());
    let mut in_string = false;
    let mut escaped = false;
    for ch in input.chars() {
        if in_string {
            if escaped {
                output.push(ch);
                escaped = false;
                continue;
            }
            match ch {
                '\\' => {
                    output.push(ch);
                    escaped = true;
                }
                '"' => {
                    output.push(ch);
                    in_string = false;
                }
                '\n' => output.push_str("\\n"),
                '\r' => output.push_str("\\r"),
                '\t' => output.push_str("\\t"),
                ch if ch.is_control() => output.push_str(&format!("\\u{:04X}", ch as u32)),
                ch => output.push(ch),
            }
            continue;
        }

        output.push(ch);
        if ch == '"' {
            in_string = true;
        }
    }
    output
}

pub fn source_header_spec(source: &BookSource) -> Result<HeaderSpec, AppError> {
    let mut spec = HeaderSpec::default();
    if let Some(raw) = source
        .header
        .as_deref()
        .filter(|value| !value.trim().is_empty())
    {
        let header_text = eval_header_rule(raw, source)?;
        merge_headers(&mut spec, parse_source_headers(&header_text));
    }
    ensure_default_user_agent(&mut spec.headers);
    merge_headers(&mut spec, login_headers());
    Ok(spec)
}

pub fn parse_source_headers(header_str: &str) -> Vec<(String, String)> {
    let trimmed = header_str.trim();
    if trimmed.is_empty() {
        return vec![];
    }
    if let Ok(headers) = serde_json::from_str::<HashMap<String, String>>(trimmed) {
        return headers
            .into_iter()
            .filter(|(key, _)| !key.trim().is_empty())
            .collect();
    }
    if let Ok(value) = serde_json::from_str::<Value>(trimmed) {
        if let Some(object) = value.as_object() {
            return object
                .iter()
                .filter_map(|(key, value)| {
                    if key.trim().is_empty() {
                        None
                    } else {
                        Some((key.clone(), value_to_header_string(value)))
                    }
                })
                .collect();
        }
    }
    parse_legacy_header_object(trimmed)
}

/// 判断 `js` 选项的返回值是否"确实是个地址"。
///
/// 只用于 [`analyze_url`] 里覆盖 url 的那一步：legado 的语义是「脚本返回非
/// null 就替换 url」，而 ECMAScript 的语句块（`try{...}catch(e){}`、赋值、
/// 自增……）都有完成值，脚本尾部随手写一句副作用就会把 url 改成 `"true"`
/// 这种非地址文本，随后建请求时炸在 `relative URL without a base`。
///
/// 接受这几种形态：
/// - 带 scheme 的绝对地址（`http:` / `https:` / `data:` / 其它 `x:` 协议）；
/// - 协议相对（`//host/path`）；
/// - 站内绝对路径（`/path`）、相对路径（`./`、`../`）、查询串（`?a=b`）。
///
/// 其余（布尔、数字、`undefined`、任意裸词）返回 false，调用方保留原 url。
fn looks_like_url(value: &str) -> bool {
    let value = value.trim();
    if value.is_empty() || value.chars().any(char::is_whitespace) {
        return false;
    }
    if value.starts_with("//") || value.starts_with('/') || value.starts_with("./") || value.starts_with("../") || value.starts_with('?') {
        return true;
    }
    // `scheme:` —— scheme 必须是「字母开头 + 字母/数字/+/-/.」，避免把
    // `true`、`123`、`NaN` 这类求值结果误判成协议名。
    match value.split_once(':') {
        Some((scheme, rest)) => {
            let mut chars = scheme.chars();
            let head_ok = chars.next().is_some_and(|c| c.is_ascii_alphabetic());
            head_ok
                && chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
                && (rest.is_empty() || !rest.starts_with(' '))
        }
        None => false,
    }
}

pub fn absolute_url(base: &str, url: &str) -> String {
    let base = normalize_source_url(strip_url_options(base));
    let url = normalize_source_url(strip_url_options(url));
    if url.trim().is_empty() {
        return base;
    }
    if url.starts_with("javascript") {
        return String::new();
    }
    if url.starts_with("data:") || url.starts_with("http://") || url.starts_with("https://") {
        return url;
    }
    if url.starts_with("//") {
        return format!("https:{}", url);
    }
    if let Ok(mut parsed) = url::Url::parse(&base) {
        parsed.set_query(None);
        parsed.set_fragment(None);
        if let Ok(joined) = parsed.join(&url) {
            return joined.to_string();
        }
    }
    url
}

fn eval_header_rule(rule: &str, source: &BookSource) -> Result<String, AppError> {
    let trimmed = rule.trim();
    if let Some(script) = trimmed.strip_prefix("@js:") {
        return eval_js(script, "", &source.book_source_url).map_err(AppError::Internal);
    }
    if let Some(script) = trimmed
        .strip_prefix("<js>")
        .and_then(|value| value.strip_suffix("</js>"))
    {
        return eval_js(script, "", &source.book_source_url).map_err(AppError::Internal);
    }
    Ok(trimmed.to_string())
}

fn headers_from_value(value: &Value) -> Result<Vec<(String, String)>, AppError> {
    match value {
        Value::String(raw) => Ok(parse_source_headers(raw)),
        Value::Object(object) => Ok(object
            .iter()
            .filter_map(|(key, value)| {
                if key.trim().is_empty() {
                    None
                } else {
                    Some((key.clone(), value_to_header_string(value)))
                }
            })
            .collect()),
        _ => Ok(vec![]),
    }
}

/// 书源作者从网页复制请求头时可能混入 Unicode 连字符变体
/// （如 `User‑Agent` 用 U+2011 非断行连字符），统一还原为 ASCII '-'。
fn sanitize_header_name(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    for ch in name.trim().chars() {
        out.push(match ch {
            '\u{2010}' | '\u{2011}' | '\u{2012}' | '\u{2013}' | '\u{2014}' | '\u{2015}'
            | '\u{2212}' => '-',
            other => other,
        });
    }
    out
}

fn is_valid_header_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&b))
}

fn merge_headers(spec: &mut HeaderSpec, headers: Vec<(String, String)>) {
    for (name, value) in headers {
        let name = sanitize_header_name(&name);
        if !is_valid_header_name(&name) {
            tracing::warn!("忽略非法书源请求头: {name:?}");
            continue;
        }
        if name.eq_ignore_ascii_case("proxy") {
            spec.proxy = Some(value);
            continue;
        }
        if let Some((_, existing)) = spec
            .headers
            .iter_mut()
            .find(|(existing, _)| existing.eq_ignore_ascii_case(&name))
        {
            *existing = value;
        } else {
            spec.headers.push((name, value));
        }
    }
}

fn ensure_default_user_agent(headers: &mut Vec<(String, String)>) {
    if !has_header(headers, "user-agent") {
        headers.push(("User-Agent".to_string(), DEFAULT_USER_AGENT.to_string()));
    }
}

fn has_header(headers: &[(String, String)], name: &str) -> bool {
    headers
        .iter()
        .any(|(header_name, _)| header_name.eq_ignore_ascii_case(name))
}

fn eval_url_js_segments(
    rule: &str,
    key: &str,
    page: i32,
    source_key: &str,
    base_url: &str,
) -> Result<String, AppError> {
    let re = Regex::new(r"(?is)<js>(.*?)</js>|@js:([\w\W]*)").unwrap();
    if !re.is_match(rule) {
        return Ok(rule.to_string());
    }

    let mut result = rule.to_string();
    let mut start = 0usize;
    for captures in re.captures_iter(rule) {
        let Some(matched) = captures.get(0) else {
            continue;
        };
        if matched.start() > start {
            let prefix = rule[start..matched.start()].trim();
            if !prefix.is_empty() {
                result = prefix.replace("@result", &result);
            }
        }
        let script = captures
            .get(2)
            .or_else(|| captures.get(1))
            .map(|m| m.as_str())
            .unwrap_or_default();
        result = eval_js_url(script, &result, key, page, source_key, base_url)
            .map_err(AppError::Internal)?;
        start = matched.end();
        if captures.get(2).is_some() {
            return Ok(result);
        }
    }
    if rule.len() > start {
        let suffix = rule[start..].trim();
        if !suffix.is_empty() {
            result = suffix.replace("@result", &result);
        }
    }
    Ok(result)
}

fn replace_inline_js(
    rule: &str,
    key: &str,
    page: i32,
    source_key: &str,
    base_url: &str,
) -> Result<String, AppError> {
    let re = Regex::new(r"\{\{([\w\W]*?)\}\}").unwrap();
    let mut output = String::with_capacity(rule.len());
    let mut last = 0usize;
    for captures in re.captures_iter(rule) {
        let Some(matched) = captures.get(0) else {
            continue;
        };
        output.push_str(&rule[last..matched.start()]);
        let script = captures.get(1).map(|m| m.as_str()).unwrap_or_default();
        let value =
            eval_js_url(script, "", key, page, source_key, base_url).map_err(AppError::Internal)?;
        output.push_str(&value);
        last = matched.end();
    }
    output.push_str(&rule[last..]);
    Ok(output)
}

fn replace_legacy_placeholders(rule: &str, key: &str, page: i32) -> String {
    rule.replace("{key}", key)
        .replace("{page}", &page.to_string())
        .replace("searchKey", key)
        .replace("searchPage", &page.to_string())
}

fn replace_page_choices(rule: &str, page: i32) -> String {
    let re = Regex::new(r"<(.*?)>").unwrap();
    re.replace_all(rule, |captures: &regex::Captures| {
        let pages = captures
            .get(1)
            .map(|m| m.as_str())
            .unwrap_or_default()
            .split(',')
            .map(str::trim)
            .collect::<Vec<_>>();
        if pages.is_empty() {
            return String::new();
        }
        let idx = page.saturating_sub(1) as usize;
        pages
            .get(idx)
            .or_else(|| pages.last())
            .copied()
            .unwrap_or_default()
            .to_string()
    })
    .into_owned()
}

pub(crate) fn split_url_options(spec: &str) -> (&str, Option<&str>) {
    let mut depth = 0i32;
    let mut in_string = false;
    let mut quote = '\0';
    let mut escaped = false;
    let mut data_separator = spec.starts_with("data:");

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
                if data_separator { data_separator = false; continue; }
                let left = &spec[..idx];
                let right = &spec[idx + ch.len_utf8()..];
                if right.trim_start().starts_with('{') { return (left, Some(right.trim())); }
            }
            _ => {}
        }
    }

    (spec, None)
}

fn strip_url_options(rule: &str) -> &str {
    split_url_options(rule).0
}

fn encode_get_query(url: &str, charset: Option<&str>) -> String {
    let Ok(mut parsed) = url::Url::parse(url) else {
        return url.to_string();
    };
    let Some(query) = parsed.query().map(str::to_string) else {
        return url.to_string();
    };
    let encode_with_declared_charset = uses_non_utf_charset(charset);
    if query.is_empty() || (query.contains('%') && !encode_with_declared_charset) {
        return url.to_string();
    }
    let encoded = query
        .split('&')
        .map(|part| {
            let mut pieces = part.splitn(2, '=');
            let key = pieces.next().unwrap_or_default();
            let value = pieces.next();
            match value {
                Some(value) => format!(
                    "{}={}",
                    encode_query_component(key, charset),
                    encode_query_component(value, charset)
                ),
                None => encode_query_component(key, charset),
            }
        })
        .collect::<Vec<_>>()
        .join("&");
    let fragment = parsed.fragment().map(str::to_string);
    parsed.set_query(None);
    parsed.set_fragment(None);
    let mut encoded_url = parsed.to_string();
    encoded_url.push('?');
    encoded_url.push_str(&encoded);
    if let Some(fragment) = fragment {
        encoded_url.push('#');
        encoded_url.push_str(&fragment);
    }
    encoded_url
}

fn encode_query_component(input: &str, charset: Option<&str>) -> String {
    let Some(charset) = charset.map(str::trim).filter(|value| !value.is_empty()) else {
        return urlencoding::encode(input).into_owned();
    };
    if charset.eq_ignore_ascii_case("utf-8") || charset.eq_ignore_ascii_case("utf8") {
        return urlencoding::encode(input).into_owned();
    }
    let Some(encoding) = Encoding::for_label(charset.as_bytes()) else {
        return urlencoding::encode(input).into_owned();
    };
    let input = if input.contains('%') {
        match urlencoding::decode(input) {
            Ok(decoded) => decoded.into_owned(),
            Err(_) => return input.to_string(),
        }
    } else {
        input.to_string()
    };
    let (bytes, _, _) = encoding.encode(&input);
    percent_encode_query_bytes(&bytes)
}

fn uses_non_utf_charset(charset: Option<&str>) -> bool {
    let Some(charset) = charset.map(str::trim).filter(|value| !value.is_empty()) else {
        return false;
    };
    !charset.eq_ignore_ascii_case("utf-8")
        && !charset.eq_ignore_ascii_case("utf8")
        && Encoding::for_label(charset.as_bytes()).is_some()
}

fn percent_encode_query_bytes(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|byte| {
            if byte.is_ascii_alphanumeric() || matches!(*byte, b'-' | b'_' | b'.' | b'~') {
                (*byte as char).to_string()
            } else {
                format!("%{:02X}", byte)
            }
        })
        .collect()
}

/// 编码表单型 POST 正文（legado 的 `AnalyzeUrl.analyzeFields`）。
///
/// **`charset` 必须参与编码**：legado 的 `encodeParams(fieldsTxt, charset, false)`
/// 会把每个键值按声明字符集 `URLEncoder.encode` —— 书源写 `"charset": "GBK"`
/// 时正文发的是 GBK 百分号编码。此前不论 `charset` 一律按 UTF-8 编码，站点按
/// GBK 解码就成了乱码关键词，搜索稳定返回「无结果」（发现页正常，因为走 GET）。
fn encode_post_body(body: &str, charset: Option<&str>, has_content_type: bool) -> String {
    let trimmed = body.trim_start();
    if has_content_type
        || trimmed.starts_with('{')
        || trimmed.starts_with('[')
        || trimmed.starts_with('<')
    {
        return body.to_string();
    }
    body.split('&')
        .map(|part| encode_form_part(part, charset))
        .collect::<Vec<_>>()
        .join("&")
}

fn encode_form_part(part: &str, charset: Option<&str>) -> String {
    let mut pieces = part.splitn(2, '=');
    let key = pieces.next().unwrap_or_default();
    let value = pieces.next();
    match value {
        Some(value) => format!(
            "{}={}",
            encode_form_value(key, charset),
            encode_form_value(value, charset)
        ),
        None => encode_form_value(key, charset),
    }
}

/// 单个表单键/值的编码。
///
/// 已带 `%` 的片段视为调用方自己编好了，原样保留 —— 与 legado 的
/// `NetworkUtils.encodedForm(value)` 快速路径一致（它只在未声明 charset
/// 时启用；声明了 charset 的源本就该由这里统一编码）。
fn encode_form_value(input: &str, charset: Option<&str>) -> String {
    if input.is_empty() || input.contains('%') {
        return input.to_string();
    }
    let Some(charset) = charset.map(str::trim).filter(|value| !value.is_empty()) else {
        return urlencoding::encode(input).into_owned();
    };
    if charset.eq_ignore_ascii_case("escape") {
        return escape_encode(input);
    }
    if charset.eq_ignore_ascii_case("utf-8") || charset.eq_ignore_ascii_case("utf8") {
        return urlencoding::encode(input).into_owned();
    }
    let Some(encoding) = Encoding::for_label(charset.as_bytes()) else {
        return urlencoding::encode(input).into_owned();
    };
    let (bytes, _, _) = encoding.encode(input);
    percent_encode_query_bytes(&bytes)
}

/// `java.encodeURI(text, charset)` —— 等价于 Java 的
/// `URLEncoder.encode(text, charset)`。
///
/// 与 [`encode_form_value`] 的区别（这不是同一个东西）：
///   1. **不跳过含 `%` 的片段**。`URLEncoder.encode` 会把 `%` 编成 `%25`，
///      调用方若已自己编好再交给它只会被二次编码 —— 这正是 Java 的语义。
///   2. 空格编成 `+`（form 语义），不是 `%20`。
///   3. 安全字符集是 `[A-Za-z0-9._*-]`，`~` 不在其中（会被编成 `%7E`）。
///
/// `charset` 缺失时按 UTF-8，与 legado 的单参数重载一致。
///
/// 书源 `java.encodeURI(key,'gbk')` 依赖这条：关键词按 GBK 编码后交给站点的
/// `charset=gbk` 表单。字符集被忽略时（此前恒按 UTF-8）站点按 GBK 解码得到
/// 乱码关键词，搜索稳定 0 条 —— 而发现页走 GET、不经过这里，所以看着正常。
pub(crate) fn java_url_encoder(input: &str, charset: Option<&str>) -> String {
    let charset = charset.map(str::trim).filter(|value| !value.is_empty());
    let bytes: Vec<u8> = match charset {
        // 未声明 / 显式 UTF-8：直接取 UTF-8 字节。
        None => input.as_bytes().to_vec(),
        Some(label)
            if label.eq_ignore_ascii_case("utf-8") || label.eq_ignore_ascii_case("utf8") =>
        {
            input.as_bytes().to_vec()
        }
        Some(label) => match Encoding::for_label(label.as_bytes()) {
            Some(encoding) => {
                let (encoded, _, _) = encoding.encode(input);
                encoded.into_owned()
            }
            // 未知字符集：Java 侧会抛异常并返回空串，这里退回 UTF-8 更宽容 ——
            // 宁可发出「可能不对但非空」的关键词，也不要让搜索整条链路炸掉。
            None => input.as_bytes().to_vec(),
        },
    };
    let mut out = String::with_capacity(bytes.len() * 3);
    for byte in bytes {
        let ch = byte as char;
        if ch.is_ascii_alphanumeric() || matches!(ch, '.' | '-' | '*' | '_') {
            out.push(ch);
        } else if ch == ' ' {
            out.push('+');
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

fn escape_encode(input: &str) -> String {
    input
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '*' | '@' | '-' | '_' | '+' | '.' | '/') {
                ch.to_string()
            } else {
                format!("%{:02X}", ch as u32)
            }
        })
        .collect()
}

fn parse_usize(value: &Value) -> Option<usize> {
    value
        .as_u64()
        .map(|value| value as usize)
        .or_else(|| value.as_str().and_then(|value| value.parse().ok()))
}

fn parse_i64(value: &Value) -> Option<i64> {
    value
        .as_i64()
        .or_else(|| value.as_str().and_then(|value| value.parse().ok()))
}

fn is_truthy_webview(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(value) => *value,
        Value::String(value) => {
            let value = value.trim();
            !value.is_empty() && !value.eq_ignore_ascii_case("false")
        }
        _ => true,
    }
}

fn value_to_header_string(value: &Value) -> String {
    value
        .as_str()
        .map(str::to_string)
        .unwrap_or_else(|| value.to_string())
}

fn parse_legacy_header_object(raw: &str) -> Vec<(String, String)> {
    let mut text = raw.trim();
    if let Some(inner) = text
        .strip_prefix('{')
        .and_then(|value| value.strip_suffix('}'))
    {
        text = inner;
    }

    let chars = text.chars().collect::<Vec<_>>();
    let mut index = 0usize;
    let mut headers = Vec::new();
    while index < chars.len() {
        skip_separators(&chars, &mut index);
        if index >= chars.len() {
            break;
        }
        let key = parse_header_token(&chars, &mut index, true);
        skip_ws(&chars, &mut index);
        if chars.get(index) != Some(&':') {
            index += 1;
            continue;
        }
        index += 1;
        skip_ws(&chars, &mut index);
        let value = parse_header_token(&chars, &mut index, false);
        if !key.trim().is_empty() {
            headers.push((key, value));
        }
        while index < chars.len() && chars[index] != ',' {
            index += 1;
        }
    }
    headers
}

fn skip_separators(chars: &[char], index: &mut usize) {
    while *index < chars.len() && (chars[*index].is_whitespace() || chars[*index] == ',') {
        *index += 1;
    }
}

fn skip_ws(chars: &[char], index: &mut usize) {
    while *index < chars.len() && chars[*index].is_whitespace() {
        *index += 1;
    }
}

fn parse_header_token(chars: &[char], index: &mut usize, stop_at_colon: bool) -> String {
    if *index >= chars.len() {
        return String::new();
    }
    if matches!(chars[*index], '"' | '\'') {
        let quote = chars[*index];
        *index += 1;
        let start = *index;
        while *index < chars.len() && chars[*index] != quote {
            *index += 1;
        }
        let out = chars[start..*index].iter().collect::<String>();
        if *index < chars.len() {
            *index += 1;
        }
        return out;
    }
    let start = *index;
    while *index < chars.len() && chars[*index] != ',' && (!stop_at_colon || chars[*index] != ':') {
        *index += 1;
    }
    chars[start..*index]
        .iter()
        .collect::<String>()
        .trim()
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `java_url_encoder` 必须按声明字符集编码字节。
    #[test]
    fn java_url_encoder_honours_charset() {
        // 「剑来」GBK = BD A3 C0 B4；UTF-8 = E5 89 91 E6 9D A5
        assert_eq!(java_url_encoder("剑来", Some("gbk")), "%BD%A3%C0%B4");
        assert_eq!(java_url_encoder("剑来", Some("UTF-8")), "%E5%89%91%E6%9D%A5");
        assert_eq!(java_url_encoder("剑来", None), "%E5%89%91%E6%9D%A5");
        // Java URLEncoder 的 form 语义
        assert_eq!(java_url_encoder("a b", None), "a+b");
        assert_eq!(java_url_encoder("100%", None), "100%25");
    }


    /// 规范 JSON 选项走严格解析（最常走的路径）。
    #[test]
    fn parses_standard_json_options() {
        let options = parse_url_options(r#"{"method":"POST","headers":{"A":"B"}}"#).unwrap();
        assert_eq!(options.get("method").and_then(Value::as_str), Some("POST"));
        assert_eq!(
            options.get("headers").and_then(|h| h.get("A")).and_then(Value::as_str),
            Some("B")
        );
    }

    /// 单引号 + 单引号值的 JS 对象字面量（该源的分类 URL 选项）。
    ///
    /// `serde_json` 对这种形态报 `key must be a string`。旧实现只有「严格 JSON →
    /// 转义控制字符」两条路，于是整个分类加载失败，界面只剩 `internal error`。
    /// legado 用 GSON 宽松模式能读，所以手机端正常。
    #[test]
    fn parses_single_quoted_js_object_options() {
        let options = parse_url_options(
            r#"{'headers':'{"User-Agent": "Mozilla/5.0"}'}"#,
        )
        .expect("单引号 JS 字面量应当可解析");

        // 内层 headers 是「JSON 文本」而非对象 —— 与 legado 行为一致，
        // 由 headers_from_value 再做一次解析。
        let raw = options.get("headers").and_then(Value::as_str).unwrap_or_default();
        let inner: Value = serde_json::from_str(raw).expect("内层应为 JSON 文本");
        assert_eq!(inner.get("User-Agent").and_then(Value::as_str), Some("Mozilla/5.0"));
    }

    /// 无引号键名（JS 对象字面量的另一种常见写法）。
    #[test]
    fn parses_unquoted_key_js_object_options() {
        let options = parse_url_options("{method:'POST', retry:3}").expect("无引号键名应当可解析");
        assert_eq!(options.get("method").and_then(Value::as_str), Some("POST"));
        assert_eq!(options.get("retry").and_then(Value::as_i64), Some(3));
    }

    /// 非对象、且非法的输入仍要报错，不能被静默吞掉。
    #[test]
    fn rejects_unparsable_options() {
        let err = parse_url_options("not json at all").unwrap_err();
        assert!(
            format!("{err}").contains("invalid url options"),
            "错误信息应保留原始原因: {err}"
        );
    }

    /// 数组字面量不该被当成合法选项对象。
    #[test]
    fn rejects_js_array_literal_as_options() {
        assert!(eval_options_as_js_literal("[1,2,3]").is_none());
    }

    /// 尾逗号（宽松 JSON 允许、严格 JSON 拒绝）也要能用。
    #[test]
    fn parses_trailing_comma_options() {
        let options = parse_url_options(r#"{"method":"GET","retry":2,}"#);
        // 严格 JSON 拒绝尾逗号，JS 引擎接受 —— 两者之一成功即可。
        assert!(options.is_ok(), "尾逗号选项应当可解析");
    }

    /// POST 表单正文必须按书源声明的 `charset` 编码。
    ///
    /// 书源把 `"charset": "GBK"` 写在搜索 URL 的选项里，站点按 GBK 解码关键词。
    /// 早先正文一律按 UTF-8 编码，站点拿到的是一串乱码，于是「搜索永远无结果，
    /// 但发现页正常」（发现页走 GET，编码路径是另一条）。
    #[test]
    fn post_body_uses_declared_charset() {
        let body = encode_post_body("submit=&type=articlename&s=我的", Some("GBK"), false);
        assert_eq!(body, "submit=&type=articlename&s=%CE%D2%B5%C4");
        // 英文/ASCII 部分不受影响。
        assert!(body.starts_with("submit=&type=articlename&s="));
    }

    /// 声明 UTF-8（或不声明）时仍走 UTF-8 百分号编码。
    #[test]
    fn post_body_defaults_to_utf8() {
        for charset in [None, Some("UTF-8"), Some("utf8")] {
            let body = encode_post_body("s=我的", charset, false);
            assert_eq!(body, "s=%E6%88%91%E7%9A%84", "charset={charset:?}");
        }
    }

    /// 已带 `%xx` 的片段原样保留，避免二次编码。
    #[test]
    fn post_body_keeps_preexisting_percent_escapes() {
        let body = encode_post_body("s=%E6%88%91&x=1", Some("UTF-8"), false);
        assert_eq!(body, "s=%E6%88%91&x=1");
    }

    /// 调用方自带 `Content-Type`（例如 JSON）时不得改动正文。
    #[test]
    fn post_body_untouched_when_content_type_given() {
        let raw = r#"{"key":"我的"}"#;
        assert_eq!(encode_post_body(raw, Some("GBK"), true), raw);
        // 结构体正文（JSON/XML）也不编码。
        assert_eq!(
            encode_post_body(r#"{"key":"我的"}"#, Some("GBK"), false),
            r#"{"key":"我的"}"#
        );
    }
}
