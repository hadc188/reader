use crate::api::AppState;
use crate::error::error::{ApiResponse, AppError};
use crate::service::reading_stats_service::{BookReadingStats, DailyReadingStats};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AddReadingStatsRequest {
    pub seconds: Option<i64>,
    pub characters: Option<i64>,
    pub date: Option<String>,
    pub book_url: Option<String>,
    pub book_name: Option<String>,
    pub book_author: Option<String>,
}

/// Accumulate reading time/characters for the current user on a date.
#[tauri::command]
pub async fn add_reading_stats(
    state: tauri::State<'_, AppState>,
    req: AddReadingStatsRequest,
) -> Result<ApiResponse<serde_json::Value>, AppError> {
    let user_ns = "default";
    state
        .reading_stats_service
        .add_reading(
            user_ns,
            req.seconds.unwrap_or(0),
            req.characters.unwrap_or(0),
            req.date.as_deref(),
            req.book_url.as_deref(),
            req.book_name.as_deref(),
            req.book_author.as_deref(),
        )
        .await?;
    Ok(ApiResponse::ok(serde_json::json!({ "saved": true })))
}

/// Per-day stats for a date range (YYYY-MM-DD), oldest first.
#[tauri::command]
pub async fn get_reading_stats_daily(
    state: tauri::State<'_, AppState>,
    start: String,
    end: String,
) -> Result<ApiResponse<Vec<DailyReadingStats>>, AppError> {
    let user_ns = "default";
    let daily = state.reading_stats_service.get_daily(user_ns, &start, &end).await?;
    Ok(ApiResponse::ok(daily))
}

/// Lifetime totals.
#[tauri::command]
pub async fn get_reading_stats_summary(
    state: tauri::State<'_, AppState>,
) -> Result<ApiResponse<serde_json::Value>, AppError> {
    let user_ns = "default";
    let summary = state.reading_stats_service.get_summary(user_ns).await?;
    Ok(ApiResponse::ok(summary))
}

#[tauri::command]
pub async fn get_reading_stats_by_book(
    state: tauri::State<'_, AppState>,
    start: String,
    end: String,
) -> Result<ApiResponse<Vec<BookReadingStats>>, AppError> {
    let user_ns = "default";
    let books = state
        .reading_stats_service
        .get_by_book(user_ns, &start, &end)
        .await?;
    Ok(ApiResponse::ok(books))
}

/// Delete all per-book reading stats for a given book_url (across all dates).
#[tauri::command]
pub async fn delete_reading_stats_by_book(
    state: tauri::State<'_, AppState>,
    book_url: String,
) -> Result<ApiResponse<serde_json::Value>, AppError> {
    let user_ns = "default";
    let deleted = state
        .reading_stats_service
        .delete_book_stats(user_ns, &book_url)
        .await?;
    Ok(ApiResponse::ok(serde_json::json!({ "deleted": deleted })))
}
