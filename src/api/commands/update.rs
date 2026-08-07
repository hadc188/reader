use serde::Deserialize;
use serde_json::Value;

use crate::api::AppState;
use crate::error::error::{ApiResponse, AppError};

#[derive(Debug, Deserialize)]
pub struct VersionUpdateQuery {
    pub force: Option<bool>,
}

#[derive(Debug, Deserialize)]
pub struct DismissVersionUpdateRequest {
    pub version: Option<String>,
}

#[tauri::command]
pub async fn get_version_update(
    state: tauri::State<'_, AppState>,
    req: VersionUpdateQuery,
) -> Result<ApiResponse<Value>, AppError> {
    let info = state
        .update_service
        .check(req.force.unwrap_or(false))
        .await?;
    Ok(ApiResponse::ok(
        serde_json::to_value(info).map_err(|err| AppError::BadRequest(err.to_string()))?,
    ))
}

#[tauri::command]
pub async fn dismiss_version_update(
    state: tauri::State<'_, AppState>,
    req: DismissVersionUpdateRequest,
) -> Result<ApiResponse<Value>, AppError> {
    let version = req.version.unwrap_or_default();
    let info = state.update_service.dismiss(&version).await?;
    Ok(ApiResponse::ok(
        serde_json::to_value(info).map_err(|err| AppError::BadRequest(err.to_string()))?,
    ))
}