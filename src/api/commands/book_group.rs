use crate::api::AppState;
use crate::error::error::{ApiResponse, AppError};
use crate::model::book_group::BookGroup;
use serde::Deserialize;
use serde_json::Value;

#[derive(Debug, Deserialize)]
pub struct GroupIdParam {
    #[serde(rename = "groupId")]
    group_id: Option<i64>,
}

#[tauri::command]
pub async fn get_book_groups(
    state: tauri::State<'_, AppState>,
) -> Result<ApiResponse<Value>, AppError> {
    let user_ns = "default";
    let groups = state.book_group_service.get_groups(&user_ns).await?;
    Ok(ApiResponse::ok(
        serde_json::to_value(groups).unwrap_or_default(),
    ))
}

#[tauri::command]
pub async fn save_book_group(
    state: tauri::State<'_, AppState>,
    req: BookGroup,
) -> Result<ApiResponse<Value>, AppError> {
    let user_ns = "default";
    state.book_group_service.save_group(&user_ns, req).await?;
    Ok(ApiResponse::ok(serde_json::json!("success")))
}

#[tauri::command]
pub async fn delete_book_group(
    state: tauri::State<'_, AppState>,
    req: GroupIdParam,
) -> Result<ApiResponse<Value>, AppError> {
    let user_ns = "default";
    let gid = req
        .group_id
        .ok_or_else(|| AppError::BadRequest("groupId required".to_string()))?;
    state.book_group_service.delete_group(&user_ns, gid).await?;
    Ok(ApiResponse::ok(serde_json::json!("success")))
}

#[tauri::command]
pub async fn save_book_group_order(
    state: tauri::State<'_, AppState>,
    req: Vec<BookGroup>,
) -> Result<ApiResponse<Value>, AppError> {
    let user_ns = "default";
    state
        .book_group_service
        .save_groups(&user_ns, &req)
        .await?;
    Ok(ApiResponse::ok(serde_json::json!("success")))
}

#[derive(Debug, Deserialize)]
pub struct SaveBookGroupIdParam {
    #[serde(rename = "bookUrl")]
    book_url: Option<String>,
    #[serde(rename = "groupId")]
    group_id: Option<i64>,
}

#[tauri::command]
pub async fn save_book_group_id(
    state: tauri::State<'_, AppState>,
    req: SaveBookGroupIdParam,
) -> Result<ApiResponse<Value>, AppError> {
    let user_ns = "default";
    let url = req
        .book_url
        .ok_or_else(|| AppError::BadRequest("bookUrl required".to_string()))?;
    let gid = req.group_id.unwrap_or(0);
    let mut book = state
        .book_service
        .get_shelf_book(&user_ns, &url)
        .await?
        .ok_or_else(|| AppError::NotFound("Book not found".to_string()))?;
    book.group = Some(gid);
    state.book_service.save_book(&user_ns, book).await?;
    Ok(ApiResponse::ok(serde_json::json!("success")))
}

#[derive(Debug, Deserialize)]
pub struct MultiBookGroupParam {
    #[serde(rename = "bookUrls")]
    book_urls: Option<Vec<String>>,
    #[serde(rename = "groupId")]
    group_id: Option<i64>,
}

#[tauri::command]
pub async fn add_book_group_multi(
    state: tauri::State<'_, AppState>,
    req: MultiBookGroupParam,
) -> Result<ApiResponse<Value>, AppError> {
    let user_ns = "default";
    let urls = req.book_urls.unwrap_or_default();
    let gid = req.group_id.unwrap_or(0);
    // bitwise OR the group_id into each book's bitfield group
    for url in urls {
        if let Some(mut book) = state.book_service.get_shelf_book(&user_ns, &url).await? {
            let cur = book.group.unwrap_or(0);
            book.group = Some(cur | gid);
            let _ = state.book_service.save_book(&user_ns, book).await;
        }
    }
    Ok(ApiResponse::ok(serde_json::json!("success")))
}

#[tauri::command]
pub async fn remove_book_group_multi(
    state: tauri::State<'_, AppState>,
    req: MultiBookGroupParam,
) -> Result<ApiResponse<Value>, AppError> {
    let user_ns = "default";
    let urls = req.book_urls.unwrap_or_default();
    let gid = req.group_id.unwrap_or(0);
    // remove: book.group = book.group & ~groupId
    for url in urls {
        if let Some(mut book) = state.book_service.get_shelf_book(&user_ns, &url).await? {
            let cur = book.group.unwrap_or(0);
            book.group = Some(cur & !gid);
            let _ = state.book_service.save_book(&user_ns, book).await;
        }
    }
    Ok(ApiResponse::ok(serde_json::json!("success")))
}