use serde::Serialize;

use crate::api::AppState;
use crate::crawler::http_client::ProxyMode;
use crate::error::error::AppError;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NetworkProxyStatus {
    pub mode: String,
    pub active: bool,
    pub address: Option<String>,
}

#[tauri::command]
pub async fn configure_network_proxy(
    state: tauri::State<'_, AppState>,
    mode: String,
    proxy_url: Option<String>,
) -> Result<NetworkProxyStatus, AppError> {
    let proxy_mode = match mode.trim().to_ascii_lowercase().as_str() {
        "system" => ProxyMode::System,
        "manual" => ProxyMode::Manual,
        _ => return Err(AppError::BadRequest("未知的代理模式".to_string())),
    };
    let status = state
        .book_service
        .configure_network_proxy(proxy_mode, proxy_url.as_deref())
        .await
        .map_err(|error| AppError::BadRequest(error.to_string()))?;
    Ok(NetworkProxyStatus {
        mode: mode.trim().to_ascii_lowercase(),
        active: status.active,
        address: status.address,
    })
}
