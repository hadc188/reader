use crate::api::AppState;
use serde::Deserialize;
use serde_json::Value;
use std::path::PathBuf;
use tokio::fs;

use crate::error::error::{ApiResponse, AppError};

#[derive(Debug, Deserialize)]
pub struct DeleteFileRequest {
    pub url: Option<String>,
}

#[tauri::command]
pub async fn save_user_config(
    state: tauri::State<'_, AppState>,
    req: Value,
) -> Result<ApiResponse<Value>, AppError> {
    let user_ns = "default";
    state.user_service.save_user_config(&user_ns, req).await?;
    Ok(ApiResponse::ok(Value::String("".to_string())))
}

#[tauri::command]
pub async fn get_user_config(
    state: tauri::State<'_, AppState>,
) -> Result<ApiResponse<Value>, AppError> {
    let user_ns = "default";
    let cfg = state.user_service.get_user_config(&user_ns).await?;
    Ok(ApiResponse::ok(cfg))
}

#[tauri::command]
pub async fn upload_file(
    state: tauri::State<'_, AppState>,
    file: Vec<u8>,
    file_name: String,
    file_type: Option<String>,
) -> Result<ApiResponse<Value>, AppError> {
    let user_ns = "default";
    let file_type = file_type
        .filter(|t| !t.is_empty())
        .unwrap_or_else(|| "images".to_string());
    let dir = PathBuf::from(&state.config.storage_dir)
        .join("assets")
        .join(&user_ns)
        .join(&file_type);
    fs::create_dir_all(&dir)
        .await
        .map_err(|e| AppError::Internal(e.into()))?;
    let path = dir.join(&file_name);
    fs::write(&path, &file)
        .await
        .map_err(|e| AppError::Internal(e.into()))?;
    let url = format!("/assets/{}/{}/{}", user_ns, file_type, file_name);
    Ok(ApiResponse::ok(Value::from(vec![Value::String(url)])))
}

#[tauri::command]
pub async fn delete_file(
    state: tauri::State<'_, AppState>,
    req: DeleteFileRequest,
) -> Result<ApiResponse<Value>, AppError> {
    let user_ns = "default";
    let url = req.url.unwrap_or_default();
    if url.is_empty() {
        return Ok(ApiResponse::err("请输入文件链接"));
    }
    let prefix = format!("/assets/{}/", user_ns);
    if !url.starts_with(&prefix) {
        return Ok(ApiResponse::err("文件链接错误"));
    }
    let full_path = PathBuf::from(&state.config.storage_dir).join(url.trim_start_matches('/'));
    let _ = fs::remove_file(full_path).await;
    Ok(ApiResponse::ok(Value::String("".to_string())))
}