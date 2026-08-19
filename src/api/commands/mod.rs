pub mod book;
pub mod book_group;
pub mod book_source;
pub mod bookmark;
pub mod backup;
pub mod debug;
pub mod network;
pub mod reading_stats;
pub mod replace_rule;
pub mod rss;
pub mod speech;
pub mod update;
pub mod user;
pub mod webdav;
pub mod window;

use crate::error::error::{ApiResponse, AppError};

#[tauri::command]
pub async fn health() -> Result<ApiResponse<&'static str>, AppError> {
    Ok(ApiResponse::ok("ok"))
}
