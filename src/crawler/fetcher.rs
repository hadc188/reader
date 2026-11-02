use crate::crawler::http_client::HttpClient;
use encoding_rs::Encoding;
use serde::{Deserialize, Serialize};
use std::time::Duration;
use tokio::time::sleep;
use base64::Engine;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum HttpMethod {
    GET,
    POST,
    HEAD,
    PUT,
    PATCH,
    DELETE,
}

impl Default for HttpMethod {
    fn default() -> Self {
        Self::GET
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RequestSpec {
    pub url: String,
    pub method: HttpMethod,
    pub headers: Vec<(String, String)>,
    pub body: Option<String>,
    /// 原始字节请求体。`java.net.URL(...).openConnection()` 这类 Java 互操作脚本
    /// 会发送未按字符集重编码的二进制包(如 gzip 注册包)。设置后优先于 `body`。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body_bytes: Option<Vec<u8>>,
    pub retry: usize,
    pub response_type: Option<String>,
    pub charset: Option<String>,
    pub proxy: Option<String>,
    /// 单次尝试的覆盖超时。`None` 时沿用 client 自身的超时(书源默认 15s)。
    /// 面板里的浏览器 `fetch`(头像/封面等第三方静态资源)用它压到 9s, 免得
    /// 一个连不通的 CDN 一直挂着; 因为是 per-request 覆盖, 连接池仍然复用。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout: Option<Duration>,
    pub server_id: Option<i64>,
    pub web_view: bool,
    pub web_js: Option<String>,
    pub web_view_delay_time: u64,
    /// 是否跟随 3xx。默认 `true`。
    ///
    /// `java.get` / `java.post` / `java.head` 传 `false`（对应 legado 的
    /// `Jsoup.connect(...).followRedirects(false)`），书源靠读 302 的 `Location`
    /// 做两步搜索（先换 searchid 再拼结果地址）。跟随关闭时 fetcher 不动响应，
    /// `Location` 原样交给脚本。
    #[serde(default = "default_true")]
    pub follow_redirects: bool,
}

fn default_true() -> bool { true }

/// reqwest 的「跳转次数超限」错误。
///
/// 只能靠 `source()` 链判断：这种情况的 `ErrorKind` 是 `Redirect`，但
/// `Display` / `{:#}` 都只打印 "error following redirect for url (...)"，
/// 真正的 "too many redirects" 在最内层的 `source()` 上。
fn is_too_many_redirects(err: &reqwest::Error) -> bool {
    let mut source: Option<&(dyn std::error::Error + 'static)> = std::error::Error::source(err);
    while let Some(inner) = source {
        if inner.to_string().contains("too many redirects") {
            return true;
        }
        source = inner.source();
    }
    false
}

impl Default for RequestSpec {
    fn default() -> Self {
        Self {
            url: String::new(),
            method: HttpMethod::GET,
            headers: Vec::new(),
            body: None,
            body_bytes: None,
            retry: 2,
            response_type: None,
            charset: None,
            proxy: None,
            timeout: None,
            server_id: None,
            web_view: false,
            web_js: None,
            web_view_delay_time: 0,
            follow_redirects: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FetchResponse {
    pub url: String,
    pub status: u16,
    pub body: String,
    /// 未经字符集解码的原始响应字节。字节级互操作脚本(`java.net.URL`)用它按
    /// 原始字节解读响应(gzip / 二进制协议),普通文本请求不使用。
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub raw_body: Vec<u8>,
    pub content_type: Option<String>,
    pub headers: Vec<(String, String)>,
    pub is_successful: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
pub struct StrResponse {
    pub body: String,
    pub url: String,
    pub code: u16,
    /// legado 的 `headers` 是 Map 语义, 书源脚本 `JSON.stringify(result.headers)`
    /// 会产出对象(`{"k":"v"}`), 而 Rust 侧内部表示是 `[[k,v],...]`。两种都收。
    #[serde(deserialize_with = "lenient_headers")]
    pub headers: Vec<(String, String)>,
    pub is_successful: bool,
}

/// 接受 `[[k,v],...]`、`{"k":"v"}` 与 null 三种形态。
fn lenient_headers<'de, D>(deserializer: D) -> Result<Vec<(String, String)>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = Option::<serde_json::Value>::deserialize(deserializer)?;
    Ok(match value {
        Some(serde_json::Value::Array(rows)) => rows
            .into_iter()
            .filter_map(|row| {
                let mut row = row.as_array()?.iter();
                let key = row.next()?.as_str()?.to_string();
                let value = row.next()?.as_str().unwrap_or_default().to_string();
                Some((key, value))
            })
            .collect(),
        Some(serde_json::Value::Object(map)) => map
            .into_iter()
            .map(|(key, value)| {
                let value = value.as_str().map(str::to_string).unwrap_or_else(|| value.to_string());
                (key, value)
            })
            .collect(),
        _ => Vec::new(),
    })
}

pub async fn fetch(client: &HttpClient, req: RequestSpec) -> anyhow::Result<FetchResponse> {
    let request_client = client.client_with_proxy(req.proxy.as_deref())?;
    fetch_with_client(&request_client, req).await
}

/// Fetch with a caller-owned client. Book-source callers use a client scoped
/// to one source, preserving redirects and cookies without sharing them with
/// another source.
pub async fn fetch_with_client(
    request_client: &reqwest::Client,
    req: RequestSpec,
) -> anyhow::Result<FetchResponse> {
    fetch_inner(request_client, req, false).await
}

/// 字节级请求: 请求体按原始字节发送, 响应体保留原始字节。
///
/// 调用方必须使用关闭了自动解压的客户端(见 `build_raw_source_client`),
/// 否则 `Content-Encoding` 对应的字节在到达这里之前就已经被解压。
pub async fn fetch_raw_with_client(
    request_client: &reqwest::Client,
    req: RequestSpec,
) -> anyhow::Result<FetchResponse> {
    fetch_inner(request_client, req, true).await
}

async fn fetch_inner(
    request_client: &reqwest::Client,
    req: RequestSpec,
    raw: bool,
) -> anyhow::Result<FetchResponse> {
    if let Some(response) = data_uri_response(&req)? { return Ok(response); }
    let mut last_err: Option<anyhow::Error> = None;
    let max_retries = req.retry;
    for attempt in 0..=max_retries {
        let req = req.clone();
        let mut builder = match req.method {
            HttpMethod::GET => request_client.get(&req.url),
            HttpMethod::POST => request_client.post(&req.url),
            HttpMethod::HEAD => request_client.head(&req.url),
            HttpMethod::PUT => request_client.put(&req.url),
            HttpMethod::PATCH => request_client.patch(&req.url),
            HttpMethod::DELETE => request_client.delete(&req.url),
        };
        // 是否跟随 3xx 由调用方选用的 client 决定: `java.get/head/post` 走
        // 不跟随的 client, 以便脚本读到 302 的 `Location`
        // (见 `http_client::build_source_client_no_redirect`)。
        if let Some(timeout) = req.timeout {
            builder = builder.timeout(timeout);
        }

        let mut has_content_type = false;
        // 显式把 `Host` 放在第一个首部。
        //
        // Cloudflare 的托管挑战会按首部写法识别客户端: 同一请求把 `Host` 放在
        // 首位、其余首部用 Title-Case 就返回 200, 换成 hyper 默认的「Host 最后 +
        // 全小写」就是 403 `cf-mitigated: challenge`(实测, 逐项二分确认)。
        // hyper 在没有显式 `Host` 时会自己补一个并放在最后, 所以书源请求需显式给出;
        // 地址里带的端口/用户信息由 `host_header_value` 处理。
        if let Some(host) = host_header_value(&req.url) {
            let already = req
                .headers
                .iter()
                .any(|(k, _)| k.eq_ignore_ascii_case("host"));
            if !already {
                builder = builder.header(reqwest::header::HOST, host);
            }
        }
        for (k, v) in &req.headers {
            if k.to_lowercase() == "content-type" {
                has_content_type = true;
            }
            builder = builder.header(k, v);
        }

        if let Some(bytes) = req.body_bytes.clone() {
            builder = builder.body(bytes);
        } else if let Some(body) = req.body.clone() {
            if matches!(req.method, HttpMethod::POST) && !has_content_type {
                builder = builder.header(
                    reqwest::header::CONTENT_TYPE,
                    if serde_json::from_str::<serde_json::Value>(&body).is_ok() {
                        "application/json"
                    } else { "application/x-www-form-urlencoded" },
                );
            }
            builder = builder.body(body);
        }

        let sent = builder.send().await;
        // 换域跳转会让「显式 `Host`」变成毒药：reqwest 跟随 3xx 时原样沿用我们注入
        // 的 `Host`，目标是新域却仍宣称旧域，对端于是再回 301 指回新域，直到
        // `too many redirects`（实测 `m.xxx.net` → `m.xxx.com` 必现）。
        //
        // 这类站点只能放弃「Host 首位」这一个 Cloudflare 优化：不注入 `Host`
        // 重发一次，交给 hyper 按当前地址生成。能走到这里说明首跳已经成功了，
        // 站点本来也没在用它挡我们，重发是安全的。
        let sent = match sent {
            Err(err) if req.follow_redirects && is_too_many_redirects(&err) => {
                let mut retry = match req.method {
                    HttpMethod::GET => request_client.get(&req.url),
                    HttpMethod::POST => request_client.post(&req.url),
                    HttpMethod::HEAD => request_client.head(&req.url),
                    HttpMethod::PUT => request_client.put(&req.url),
                    HttpMethod::PATCH => request_client.patch(&req.url),
                    HttpMethod::DELETE => request_client.delete(&req.url),
                };
                for (k, v) in &req.headers {
                    retry = retry.header(k, v);
                }
                if let Some(timeout) = req.timeout {
                    retry = retry.timeout(timeout);
                }
                if let Some(bytes) = req.body_bytes.clone() {
                    retry = retry.body(bytes);
                } else if let Some(body) = req.body.clone() {
                    retry = retry.body(body);
                }
                retry.send().await.or(Err(err))
            }
            other => other,
        };
        match sent {
            Ok(res) => {
                let status = res.status().as_u16();
                let is_successful = res.status().is_success();
                let url = res.url().to_string();
                let content_type = res
                    .headers()
                    .get(reqwest::header::CONTENT_TYPE)
                    .and_then(|v| v.to_str().ok())
                    .map(|s| s.to_string());
                let headers = res
                    .headers()
                    .iter()
                    .filter_map(|(name, value)| {
                        value
                            .to_str()
                            .ok()
                            .map(|value| (name.to_string(), value.to_string()))
                    })
                    .collect::<Vec<_>>();
                let bytes = res.bytes().await?;
                // 只有字节级请求才需要保留原始字节; 文本路径直接解码,
                // 避免每个响应都多一次整体拷贝。
                let raw_body = if raw { bytes.to_vec() } else { Vec::new() };
                let mut body = if req
                    .response_type
                    .as_deref()
                    .map(|value| !value.trim().is_empty())
                    .unwrap_or(false)
                {
                    hex::encode(&bytes)
                } else {
                    if raw {
                        String::new()
                    } else {
                        decode_body(&bytes, req.charset.as_deref(), content_type.as_deref())
                    }
                };
                if is_xml_response(content_type.as_deref(), &body)
                    && !body.trim_start().starts_with("<?xml")
                {
                    body = format!("<?xml version=\"1.0\"?>{}", body);
                }
                // 非 2xx 一律重试，对齐 legado `OkHttpUtils.newCallResponse`：
                //
                //   for (i in 0..retry) {
                //       response = newCall(...).await()
                //       if (response.isSuccessful) return response
                //   }
                //
                // 早先这里只对 `status >= 500` 重试，服务端 4xx 直接返回。这在
                // 「一次性挑战 Cookie」的站点上是致命的：`xsbook.org`(Tengine)
                // 对未授权的请求回 403 + `Set-Cookie: <随机名>=<值>`，带上这个
                // Cookie 后同一请求就 200。书源脚本恰恰先用
                // `cookie.removeCookie(base)` 清掉 Cookie（为了绕限频），于是
                // **每个** 详情/目录/正文请求都要重新过一次挑战；只重试 5xx
                // 时永远停在那条 403 上，界面症状就是「目录正文加载不出来」。
                //
                // 重试次数仍由书源的 `retry` 选项决定（默认 2），不额外放大。
                if !is_successful && attempt < max_retries {
                    last_err = Some(anyhow::anyhow!("request failed with status {}", status));
                } else {
                    return Ok(FetchResponse {
                        url,
                        status,
                        body,
                        raw_body,
                        content_type,
                        headers,
                        is_successful,
                    });
                }
            }
            Err(e) => {
                last_err = Some(e.into());
            }
        }

        if attempt < max_retries {
            let backoff = 200u64 * (attempt as u64 + 1);
            sleep(Duration::from_millis(backoff)).await;
        }
    }
    Err(last_err.unwrap_or_else(|| anyhow::anyhow!("fetch failed")))
}

pub(crate) fn decode_body(bytes: &[u8], charset: Option<&str>, content_type: Option<&str>) -> String {
    // URL charset controls request encoding. A declared response encoding takes
    // precedence, including HTML pages returned by an UTF-8 search endpoint.
    let label = charset_from_content_type(content_type)
        .or_else(|| charset_from_html_meta(bytes))
        .or_else(|| charset
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string));

    let text = if let Some(label) = label {
        match Encoding::for_label(label.trim().as_bytes()) {
            Some(encoding) => {
                let (text, _, _) = encoding.decode(bytes);
                text.into_owned()
            }
            None => String::from_utf8_lossy(bytes).into_owned(),
        }
    } else {
        String::from_utf8_lossy(bytes).into_owned()
    };
    strip_bom(&text).to_string()
}

/// 去掉响应体开头的 BOM。
///
/// 站点偶尔会在 JSON 前面带一个 `\u{FEFF}`（实测某站的目录接口就是）。
/// `serde_json` / JSONPath 会把 BOM 当非法字符，整段解析失败，
/// 于是目录和搜索都变成 0 条 —— 阅读页表现为「打开书一直转圈、目录空白」，
/// 用户只能反复换源。
///
/// legado 的 HTTP 层在按 charset 解码时就会消费掉 BOM（`String(response.body)`
/// 走 Gson 的 Reader），所以拿到的 JSON 里没有它。这里在解码处统一剥掉，
/// 比让每条 JSON 规则各自容错更彻底。
fn strip_bom(text: &str) -> &str {
    text.strip_prefix('\u{feff}').unwrap_or(text)
}

/// 从 URL 里取出 `Host` 首部的值(含非默认端口, 去掉 userinfo)。
///
/// 显式 `Host` 是为了让首部顺序与真实浏览器一致(见 `fetch_inner` 里的说明)。
/// IPv6 字面量要保留方括号; 解析失败时返回 `None`, 交给 hyper 自行补。
fn host_header_value(url: &str) -> Option<String> {
    let parsed = url::Url::parse(url).ok()?;
    let host = parsed.host_str()?;
    let bracketed = if host.contains(':') && !host.starts_with('[') {
        format!("[{host}]")
    } else {
        host.to_string()
    };
    Some(match parsed.port() {
        Some(port) => format!("{bracketed}:{port}"),
        None => bracketed,
    })
}

pub(crate) fn data_uri_response(req: &RequestSpec) -> anyhow::Result<Option<FetchResponse>> {
    let Some(data) = req.url.strip_prefix("data:") else { return Ok(None) };
    let (meta, payload) = data.split_once(',').ok_or_else(|| anyhow::anyhow!("无效的内嵌数据地址"))?;
    let decoded = urlencoding::decode_binary(payload.as_bytes());
    let bytes = if meta.split(';').any(|part| part.eq_ignore_ascii_case("base64")) {
        base64::engine::general_purpose::STANDARD.decode(decoded.as_ref())?
    } else { decoded.into_owned() };
    let content_type = meta.split(';').filter(|part| !part.eq_ignore_ascii_case("base64")).collect::<Vec<_>>().join(";");
    let body = if req.response_type.as_deref().is_some_and(|value| !value.is_empty()) {
        hex::encode(&bytes)
    } else { decode_body(&bytes, req.charset.as_deref(), Some(&content_type)) };
    Ok(Some(FetchResponse {
        url: req.url.clone(), status: 200, body, raw_body: bytes, content_type: Some(content_type),
        headers: Vec::new(), is_successful: true,
    }))
}

fn charset_from_content_type(content_type: Option<&str>) -> Option<String> {
    content_type.and_then(|content_type| {
        content_type.split(';').find_map(|part| {
            let (key, value) = part.split_once('=')?;
            if key.trim().eq_ignore_ascii_case("charset") {
                let value = value.trim().trim_matches('"').trim_matches('\'');
                (!value.is_empty()).then(|| value.to_string())
            } else {
                None
            }
        })
    })
}

fn charset_from_html_meta(bytes: &[u8]) -> Option<String> {
    let sniff_len = bytes.len().min(4096);
    let head = String::from_utf8_lossy(&bytes[..sniff_len]);
    let lower = head.to_ascii_lowercase();
    let index = lower.find("charset")?;
    let after = &head[index + "charset".len()..];
    let after = after.trim_start();
    let after = after.strip_prefix('=').unwrap_or(after).trim_start();
    let after = after
        .strip_prefix('"')
        .or_else(|| after.strip_prefix('\''))
        .unwrap_or(after);
    let label = after
        .chars()
        .take_while(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_'))
        .collect::<String>();
    (!label.is_empty()).then_some(label)
}

impl From<FetchResponse> for StrResponse {
    fn from(value: FetchResponse) -> Self {
        Self {
            body: value.body,
            url: value.url,
            code: value.status,
            headers: value.headers,
            is_successful: value.is_successful,
        }
    }
}

impl From<StrResponse> for FetchResponse {
    fn from(value: StrResponse) -> Self {
        Self {
            url: value.url,
            status: value.code,
            body: value.body,
            raw_body: Vec::new(),
            content_type: None,
            headers: value.headers,
            is_successful: value.is_successful,
        }
    }
}

fn is_xml_response(content_type: Option<&str>, body: &str) -> bool {
    content_type
        .map(|value| value.to_ascii_lowercase().contains("xml"))
        .unwrap_or(false)
        || body.trim_start().starts_with("<rss")
        || body.trim_start().starts_with("<feed")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decode_body_uses_response_charset() {
        let bytes = b"\xd0\xa1\xcb\xb5\xca\xd5\xb2\xd8\xc5\xc5\xd0\xd0\xb0\xf1";

        let text = decode_body(bytes, None, Some("text/html; charset=gb2312"));

        assert_eq!(text, "小说收藏排行榜");
    }

    #[test]
    fn decode_body_detects_html_meta_charset() {
        let bytes = b"<meta http-equiv=\"content-type\" content=\"text/html;charset=gb2312\"><title>\xb7\xc9\xc2\xac\xd0\xa1\xcb\xb5</title>";

        let text = decode_body(bytes, None, None);

        assert!(text.contains("飞卢小说"));
    }

    /// 带 BOM 的 JSON 必须剥掉，否则 `serde_json` 直接报 "expected value"，
    /// 目录/搜索会被静默解析成 0 条（issue #22 的"打开书一直转圈"）。
    #[test]
    fn decode_body_strips_utf8_bom() {
        let mut bytes = vec![0xEF, 0xBB, 0xBF];
        bytes.extend_from_slice(r#"{"code":1,"data":[{"name":"第一章"}]}"#.as_bytes());

        let text = decode_body(&bytes, None, Some("application/json"));

        assert!(!text.starts_with('\u{feff}'), "BOM 必须被剥掉");
        assert_eq!(text, r#"{"code":1,"data":[{"name":"第一章"}]}"#);
        assert!(serde_json::from_str::<serde_json::Value>(&text).is_ok());
    }

    /// 没有 BOM 时不能改变内容。
    #[test]
    fn decode_body_leaves_plain_json_untouched() {
        let text = decode_body(br#"{"a":1}"#, None, Some("application/json"));
        assert_eq!(text, r#"{"a":1}"#);
    }
}
