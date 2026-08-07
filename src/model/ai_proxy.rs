use crate::model::ai_model::AiModelKind;
use serde::Deserialize;
use serde_json::Value;
use std::time::Duration;
use url::Url;

const AI_PROXY_TIMEOUT_SECS: u64 = 300;
const ALLOWED_PROXY_PATHS: [&str; 3] = [
    "/v1/chat/completions",
    "/v1/images/generations",
    "/v1/audio/speech",
];

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AiProxyRequest {
    #[serde(default)]
    pub base_url: String,
    pub api_key: Option<String>,
    pub path: String,
    #[serde(default)]
    pub full_url: bool,
    #[serde(default)]
    pub use_server_config: bool,
    pub kind: Option<AiModelKind>,
    pub body: Value,
}

#[derive(Debug, Deserialize)]
pub struct AiProxyImageRequest {
    pub url: String,
}

pub fn build_ai_proxy_url(base_url: &str, path: &str, full_url: bool) -> Result<Url, String> {
    if full_url {
        return parse_http_url(base_url);
    }

    if !ALLOWED_PROXY_PATHS.contains(&path) {
        return Err(format!("unsupported proxy path: {}", path));
    }

    let mut base = parse_http_url(base_url)?;
    let joined_path = format!("{}{}", base.path().trim_end_matches('/'), path,);
    base.set_path(&joined_path);
    base.set_query(None);
    base.set_fragment(None);
    Ok(base)
}

pub fn validate_ai_proxy_image_url(url: &str) -> Result<Url, String> {
    parse_http_url(url)
}

pub fn ai_proxy_timeout() -> Duration {
    Duration::from_secs(AI_PROXY_TIMEOUT_SECS)
}

pub fn format_ai_proxy_upstream_error(status: u16, body: &str) -> String {
    let reason = http::StatusCode::from_u16(status)
        .ok()
        .and_then(|status| status.canonical_reason())
        .unwrap_or("Upstream Error");
    let detail = extract_error_detail(body);
    if detail.is_empty() {
        return format!("模型服务返回 {} {}", status, reason);
    }
    format!("模型服务返回 {} {}：{}", status, reason, detail)
}

fn parse_http_url(raw: &str) -> Result<Url, String> {
    let url = Url::parse(raw.trim()).map_err(|e| e.to_string())?;
    match url.scheme() {
        "http" | "https" => Ok(url),
        _ => Err("only http/https proxy targets are supported".to_string()),
    }
}

fn extract_error_detail(body: &str) -> String {
    if let Ok(value) = serde_json::from_str::<Value>(body) {
        if let Some(message) = value
            .pointer("/error/message")
            .or_else(|| value.get("errorMsg"))
            .or_else(|| value.get("message"))
            .and_then(Value::as_str)
        {
            return truncate_error_detail(message);
        }
    }

    truncate_error_detail(&strip_html_tags(body))
}

fn strip_html_tags(value: &str) -> String {
    let mut output = String::with_capacity(value.len());
    let mut in_tag = false;
    for ch in value.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => {
                in_tag = false;
                output.push(' ');
            }
            _ if !in_tag => output.push(ch),
            _ => {}
        }
    }
    output.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn truncate_error_detail(value: &str) -> String {
    let cleaned = value.trim();
    if cleaned.chars().count() <= 240 {
        return cleaned.to_string();
    }
    let mut result = cleaned.chars().take(240).collect::<String>();
    result.push('…');
    result
}
