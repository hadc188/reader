use serde_json::Value;

use crate::api::AppState;
use crate::error::error::{ApiResponse, AppError};
use crate::model::ai_model::AiModelConfig;

#[tauri::command]
pub async fn get_ai_model_config(
    state: tauri::State<'_, AppState>,
) -> Result<ApiResponse<Value>, AppError> {
    // Single-user desktop: always admin, server model always allowed.
    let config = state.ai_model_service.get().await?;
    Ok(ApiResponse::ok(serde_json::json!({
        "config": config,
        "canUseServerModel": true,
        "isAdmin": true,
    })))
}

#[tauri::command]
pub async fn save_ai_model_config(
    state: tauri::State<'_, AppState>,
    req: AiModelConfig,
) -> Result<ApiResponse<Value>, AppError> {
    let saved = state.ai_model_service.save(req).await?;
    Ok(ApiResponse::ok(serde_json::json!({
        "config": saved,
        "canUseServerModel": true,
        "isAdmin": true,
    })))
}