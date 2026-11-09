use serde::{Deserialize, Serialize};
use serde_json::Value;
use url::Url;

use crate::api::AppState;
use crate::error::error::{ApiResponse, AppError};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SendXmnoteBooksRequest {
    pub endpoint: String,
    pub books: Vec<Value>,
}

#[derive(Debug, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct SendXmnoteBooksResult {
    pub sent: usize,
    pub failed: usize,
    pub errors: Vec<String>,
}

#[tauri::command]
pub async fn send_xmnote_books(
    state: tauri::State<'_, AppState>,
    req: SendXmnoteBooksRequest,
) -> Result<ApiResponse<SendXmnoteBooksResult>, AppError> {
    let endpoint = normalize_endpoint(&req.endpoint)?;
    if req.books.is_empty() {
        return Err(AppError::BadRequest("没有可发送的书籍".to_string()));
    }

    let client = state.book_service.http_client();
    let mut result = SendXmnoteBooksResult::default();

    for book in req.books {
        let title = book
            .get("title")
            .and_then(Value::as_str)
            .unwrap_or("未命名书籍");
        match send_one(&client, &endpoint, &book).await {
            Ok(()) => result.sent += 1,
            Err(message) => {
                result.failed += 1;
                result.errors.push(format!("{title}：{message}"));
            }
        }
    }

    Ok(ApiResponse::ok(result))
}

async fn send_one(client: &reqwest::Client, endpoint: &str, book: &Value) -> Result<(), String> {
    let response = client
        .post(endpoint)
        .json(book)
        .send()
        .await
        .map_err(|error| request_error(&error))?;
    let status = response.status();
    let body = response
        .text()
        .await
        .map_err(|error| format!("读取接收端响应失败: {error}"))?;

    if !status.is_success() {
        return Err(format!("HTTP {status}: {}", response_message(&body)));
    }

    let payload: Value = serde_json::from_str(&body).map_err(|_| {
        if body.trim().is_empty() {
            "接收端返回了空响应".to_string()
        } else {
            format!("接收端返回的不是 JSON：{}", truncate(&body, 120))
        }
    })?;
    let code = payload.get("code").and_then(Value::as_i64);
    if code != Some(200) {
        let message = payload
            .get("message")
            .and_then(Value::as_str)
            .filter(|value| !value.trim().is_empty())
            .unwrap_or("纸间书摘拒绝接收");
        return Err(message.to_string());
    }
    Ok(())
}

fn normalize_endpoint(raw: &str) -> Result<String, AppError> {
    let mut value = raw.trim().to_string();
    if value.is_empty() {
        return Err(AppError::BadRequest("请填写纸间书摘导入地址".to_string()));
    }
    if !value.contains("://") {
        value = format!("http://{value}");
    }
    let mut url = Url::parse(&value)
        .map_err(|_| AppError::BadRequest("纸间书摘导入地址格式不正确".to_string()))?;
    if !matches!(url.scheme(), "http" | "https") {
        return Err(AppError::BadRequest(
            "纸间书摘导入地址只支持 HTTP 或 HTTPS".to_string(),
        ));
    }
    if url.host_str().is_none() {
        return Err(AppError::BadRequest(
            "纸间书摘导入地址缺少主机名".to_string(),
        ));
    }
    if url.path().is_empty() || url.path() == "/" {
        url.set_path("/send");
    }
    url.set_fragment(None);
    Ok(url.to_string())
}

fn request_error(error: &reqwest::Error) -> String {
    if error.is_timeout() {
        "连接超时，请确认手机与电脑在同一局域网，且纸间书摘的 API 导入页面保持开启".to_string()
    } else if error.is_connect() {
        "无法连接，请检查地址是否填写正确，并确认 API 导入页面保持开启".to_string()
    } else {
        error.to_string()
    }
}

fn response_message(body: &str) -> String {
    serde_json::from_str::<Value>(body)
        .ok()
        .and_then(|payload| {
            payload
                .get("message")
                .and_then(Value::as_str)
                .map(str::to_string)
        })
        .filter(|message| !message.trim().is_empty())
        .unwrap_or_else(|| truncate(body, 160))
}

fn truncate(value: &str, max_chars: usize) -> String {
    let mut result = value.chars().take(max_chars).collect::<String>();
    if value.chars().count() > max_chars {
        result.push('…');
    }
    result
}

#[cfg(test)]
mod tests {
    use super::{normalize_endpoint, truncate};

    #[test]
    fn endpoint_accepts_ip_and_full_send_url() {
        assert_eq!(
            normalize_endpoint("192.168.1.8:8080/#/import").unwrap(),
            "http://192.168.1.8:8080/send"
        );
        assert_eq!(
            normalize_endpoint("http://xmnote.local:8080/send").unwrap(),
            "http://xmnote.local:8080/send"
        );
    }

    #[test]
    fn endpoint_rejects_non_http_scheme() {
        assert!(normalize_endpoint("ftp://xmnote.local/send").is_err());
    }

    #[test]
    fn truncates_without_splitting_unicode() {
        assert_eq!(truncate("纸间书摘导入", 4), "纸间书摘…");
    }
}
