use crate::api::AppState;
use crate::error::error::{ApiResponse, AppError};
use crate::service::book_service::DebugTrace;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DebugSourceStepRequest {
    pub book_source_url: Option<String>,
    pub step: Option<String>,
    pub keyword: Option<String>,
    pub book_url: Option<String>,
    pub chapter_url: Option<String>,
}

/// Run one step of the source debugger (search / bookInfo / toc / content).
/// Returns the fetched URL, raw response and parsed result for the UI.
#[tauri::command]
pub async fn debug_source_step(
    state: tauri::State<'_, AppState>,
    req: DebugSourceStepRequest,
) -> Result<ApiResponse<DebugTrace>, AppError> {
    let user_ns = "default";
    let source_url = req
        .book_source_url
        .ok_or_else(|| AppError::BadRequest("bookSourceUrl required".to_string()))?;
    let step = req
        .step
        .ok_or_else(|| AppError::BadRequest("step required".to_string()))?;
    let source = state
        .book_source_service
        .get(&user_ns, &source_url)
        .await?
        .ok_or_else(|| AppError::NotFound("bookSource not found".to_string()))?;

    let trace = state
        .book_service
        .debug_source_step(
            &user_ns,
            &source,
            &step,
            req.keyword.as_deref().unwrap_or(""),
            req.book_url.as_deref().unwrap_or(""),
            req.chapter_url.as_deref().unwrap_or(""),
        )
        .await?;
    Ok(ApiResponse::ok(trace))
}
