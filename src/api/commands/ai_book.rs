use serde::Deserialize;
use serde_json::Value;

use crate::api::AppState;
use crate::error::error::{ApiResponse, AppError};
use crate::model::ai_book::AiBookMemory;
use crate::util::text::repair_encoded_url;

#[derive(Debug, Deserialize, Default)]
pub struct AiBookMemoryRequest {
    #[serde(rename = "bookUrl", alias = "url")]
    pub book_url: Option<String>,
}

#[tauri::command]
pub async fn get_ai_book_memory(
    state: tauri::State<'_, AppState>,
    req: AiBookMemoryRequest,
) -> Result<ApiResponse<Value>, AppError> {
    let user_ns = "default";
    let book_url = required_book_url(req.book_url)?;
    ensure_shelf_book(&state, &user_ns, &book_url).await?;
    let memory = state.ai_book_service.get(&user_ns, &book_url).await?;
    Ok(ApiResponse::ok(
        serde_json::to_value(memory).unwrap_or_default(),
    ))
}

#[tauri::command]
pub async fn save_ai_book_memory(
    state: tauri::State<'_, AppState>,
    mut req: AiBookMemory,
) -> Result<ApiResponse<Value>, AppError> {
    let user_ns = "default";
    let book_url = required_book_url(Some(req.book_url.clone()))?;
    let shelf_book = ensure_shelf_book(&state, &user_ns, &book_url).await?;
    if req.book_name.as_deref().unwrap_or("").trim().is_empty() {
        req.book_name = Some(shelf_book.name);
    }
    if req.author.as_deref().unwrap_or("").trim().is_empty() {
        req.author = Some(shelf_book.author);
    }
    let saved = state
        .ai_book_service
        .save_for_book(&user_ns, &book_url, req)
        .await?;
    Ok(ApiResponse::ok(
        serde_json::to_value(saved).unwrap_or_default(),
    ))
}

#[tauri::command]
pub async fn delete_ai_book_memory(
    state: tauri::State<'_, AppState>,
    req: AiBookMemoryRequest,
) -> Result<ApiResponse<Value>, AppError> {
    let user_ns = "default";
    let book_url = required_book_url(req.book_url)?;
    ensure_shelf_book(&state, &user_ns, &book_url).await?;
    let deleted = state.ai_book_service.delete(&user_ns, &book_url).await?;
    Ok(ApiResponse::ok(
        serde_json::json!({ "deleted": deleted }),
    ))
}

async fn ensure_shelf_book(
    state: &AppState,
    user_ns: &str,
    book_url: &str,
) -> Result<crate::model::book::Book, AppError> {
    state
        .book_service
        .get_shelf_book(user_ns, book_url)
        .await?
        .ok_or_else(|| AppError::BadRequest("书籍未加入书架".to_string()))
}

fn required_book_url(book_url: Option<String>) -> Result<String, AppError> {
    let book_url = book_url
        .filter(|v| !v.trim().is_empty())
        .ok_or_else(|| AppError::BadRequest("bookUrl required".to_string()))?;
    Ok(repair_encoded_url(&book_url))
}