use crate::crawler::fetcher::{HttpMethod, RequestSpec};
use crate::error::error::AppError;
use crate::model::book_source::BookSource;
use crate::parser::js::{eval_js, eval_js_url, with_js_lib};
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
    with_js_lib(source.js_lib.as_deref(), || {
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
                if raw_method.eq_ignore_ascii_case("POST") {
                    method = HttpMethod::POST;
                }
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
                    url = eval_js_url(script, &url, key, page, &source.book_source_url, &base_url)
                        .map_err(AppError::Internal)?;
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
            retry,
            response_type,
            charset,
            proxy: header_spec.proxy,
            server_id,
            web_view,
            web_js,
            web_view_delay_time,
        })
    })
}

fn parse_url_options(options: &str) -> Result<Value, AppError> {
    let trimmed = options.trim();
    serde_json::from_str::<Value>(trimmed)
        .or_else(|_| serde_json::from_str::<Value>(&escape_control_chars_in_json_strings(trimmed)))
        .map_err(|e| AppError::BadRequest(format!("invalid url options: {}", e)))
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

fn merge_headers(spec: &mut HeaderSpec, headers: Vec<(String, String)>) {
    for (name, value) in headers {
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

fn split_url_options(rule: &str) -> (&str, Option<&str>) {
    for (idx, ch) in rule.char_indices() {
        if ch != ',' {
            continue;
        }
        let after = &rule[idx + ch.len_utf8()..];
        let trimmed = after.trim_start();
        if trimmed.starts_with('{') {
            let option_start = rule.len() - trimmed.len();
            return (rule[..idx].trim_end(), Some(&rule[option_start..]));
        }
    }
    (rule, None)
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

fn encode_post_body(body: &str, charset: Option<&str>, has_content_type: bool) -> String {
    let trimmed = body.trim_start();
    if has_content_type
        || trimmed.starts_with('{')
        || trimmed.starts_with('[')
        || trimmed.starts_with('<')
        || body.contains('%')
    {
        return body.to_string();
    }
    if charset
        .map(|value| value.eq_ignore_ascii_case("escape"))
        .unwrap_or(false)
    {
        return body
            .split('&')
            .map(|part| encode_form_part(part, true))
            .collect::<Vec<_>>()
            .join("&");
    }
    body.split('&')
        .map(|part| encode_form_part(part, false))
        .collect::<Vec<_>>()
        .join("&")
}

fn encode_form_part(part: &str, escape: bool) -> String {
    let mut pieces = part.splitn(2, '=');
    let key = pieces.next().unwrap_or_default();
    let value = pieces.next();
    let enc = |input: &str| {
        if escape {
            escape_encode(input)
        } else {
            urlencoding::encode(input).into_owned()
        }
    };
    match value {
        Some(value) => format!("{}={}", enc(key), enc(value)),
        None => enc(key),
    }
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
