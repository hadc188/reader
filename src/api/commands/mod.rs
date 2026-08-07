pub mod ai_book;
pub mod ai_model;
pub mod ai_proxy;
pub mod book;
pub mod book_group;
pub mod book_source;
pub mod bookmark;
pub mod debug;
pub mod reading_stats;
pub mod replace_rule;
pub mod rss;
pub mod update;
pub mod user;
pub mod webdav;

use crate::error::error::{ApiResponse, AppError};

#[tauri::command]
pub async fn health() -> Result<ApiResponse<&'static str>, AppError> {
    Ok(ApiResponse::ok("ok"))
}