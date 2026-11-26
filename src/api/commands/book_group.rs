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

#[derive(Debug, Deserialize)]
pub struct GroupIdsParam {
    #[serde(rename = "groupIds", default)]
    group_ids: Vec<i64>,
}

/// 批量删除分组。一次读改写, 避免逐个删除时的 N 次全量重写。
#[tauri::command]
pub async fn delete_book_groups(
    state: tauri::State<'_, AppState>,
    req: GroupIdsParam,
) -> Result<ApiResponse<Value>, AppError> {
    let user_ns = "default";
    // 去重, 免得同一个 id 传两次把计数算错。
    let mut ids = req.group_ids;
    ids.sort_unstable();
    ids.dedup();
    let removed = state
        .book_group_service
        .delete_groups(user_ns, &ids)
        .await?;
    Ok(ApiResponse::ok(serde_json::json!({ "removed": removed })))
}

/// `get_book_groups` 的共享实现 (Tauri command 与 Web HTTP handler 共用)。
pub async fn get_book_groups_core(
    state: &AppState,
) -> Result<ApiResponse<Value>, AppError> {
    let user_ns = "default";
    let groups = state.book_group_service.get_groups(&user_ns).await?;
    Ok(ApiResponse::ok(
        serde_json::to_value(groups).unwrap_or_default(),
    ))
}

/// `get_book_groups` 的 Tauri 入口: 只是 core 的薄壳。
#[tauri::command]
pub async fn get_book_groups(
    state: tauri::State<'_, AppState>,
) -> Result<ApiResponse<Value>, AppError> {
    get_book_groups_core(&state).await
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

/// 修复非 2 幂的分组 id(与启动迁移同一逻辑)。
///
/// WebDAV 恢复旧版备份会经 `saveBookGroupOrder` 整表覆盖, 把非 2 幂的脏 id
/// 原样写回 —— 迁移只在启动时跑, 恢复后不重启的话书架继续串扰, 用户会以为
/// 「修复无效」。恢复流程末尾调用本命令立即修复, 免去重启。
#[tauri::command]
pub async fn repair_book_groups(
    state: tauri::State<'_, AppState>,
) -> Result<ApiResponse<Value>, AppError> {
    let user_ns = "default";
    let reader = state.book_service.clone();
    let writer = state.book_service.clone();
    let migrated = state
        .book_group_service
        .migrate_non_power_of_two_group_ids(
            user_ns,
            move || {
                let service = reader.clone();
                async move { service.get_bookshelf(user_ns).await }
            },
            move |books| {
                let service = writer.clone();
                async move {
                    service
                        .save_books(user_ns, books)
                        .await
                        .map(|_| ())
                }
            },
        )
        .await?;
    Ok(ApiResponse::ok(
        serde_json::json!({ "migrated": migrated }),
    ))
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

#[derive(Debug, Deserialize)]
pub struct SetBooksGroupMaskParam {
    #[serde(rename = "bookUrls")]
    book_urls: Option<Vec<String>>,
    #[serde(rename = "groupMask")]
    group_mask: Option<i64>,
}

/// 批量设置导入书籍的分组位域。
///
/// 不用前端整表 `saveBooks`: 那个接口会覆盖整个书架，导入期间另一个窗口
/// 的进度、排序或新增书籍都可能被旧快照盖掉。这里在写锁内只改目标书。
#[tauri::command]
pub async fn set_books_group_mask(
    state: tauri::State<'_, AppState>,
    req: SetBooksGroupMaskParam,
) -> Result<ApiResponse<Value>, AppError> {
    let user_ns = "default";
    let urls = req.book_urls.unwrap_or_default();
    let mask = req.group_mask.unwrap_or(0);
    if urls.is_empty() {
        return Ok(ApiResponse::ok(serde_json::json!({
            "updated": 0,
            "changed": false,
            "groupMask": mask,
        })));
    }
    let targets: std::collections::HashSet<String> = urls.into_iter().collect();
    let mut changed = false;
    let mut updated = 0;
    state
        .book_service
        .update_books_in_place(user_ns, |list| {
            for book in list.iter_mut() {
                if targets.contains(&book.book_url) {
                    updated += 1;
                    if book.group.unwrap_or(0) != mask {
                        book.group = Some(mask);
                        changed = true;
                    }
                }
            }
            Ok(changed)
        })
        .await?;
    Ok(ApiResponse::ok(serde_json::json!({
        "updated": updated,
        "changed": changed,
        "groupMask": mask,
    })))
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
