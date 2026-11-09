use crate::api::AppState;
use serde_json::Value;
use tauri::Manager;
use tauri_plugin_dialog::DialogExt;

use crate::error::error::{ApiResponse, AppError};
use crate::model::bookmark::Bookmark;

/// `get_bookmarks` 的共享实现 (Tauri command 与 Web HTTP handler 共用)。
pub async fn get_bookmarks_core(state: &AppState) -> Result<ApiResponse<Value>, AppError> {
    let user_ns = "default";
    let list = read_list::<Bookmark>(state, &user_ns, "bookmark.json").await?;
    Ok(ApiResponse::ok(
        serde_json::to_value(list).unwrap_or_default(),
    ))
}

/// `get_bookmarks` 的 Tauri 入口: 只是 core 的薄壳。
#[tauri::command]
pub async fn get_bookmarks(
    state: tauri::State<'_, AppState>,
) -> Result<ApiResponse<Value>, AppError> {
    get_bookmarks_core(&state).await
}

/// `save_bookmark` 的共享实现。
///
/// 书签是阅读页的日常操作(划词/工具栏都要用), Web 端必须有对应路由 ——
/// 否则浏览器里点「加入书签」只会拿到一个 404。
pub async fn save_bookmark_core(
    state: &AppState,
    req: Bookmark,
) -> Result<ApiResponse<Value>, AppError> {
    let user_ns = "default";
    if req.book_name.is_empty() && req.book_author.is_empty() {
        return Err(AppError::BadRequest("书籍信息错误".to_string()));
    }
    let mut list = read_list::<Bookmark>(state, &user_ns, "bookmark.json").await?;
    upsert_by_key(&mut list, req, bookmark_key);
    write_list(state, &user_ns, "bookmark.json", &list).await?;
    Ok(ApiResponse::ok(Value::String("".to_string())))
}

#[tauri::command]
pub async fn save_bookmark(
    state: tauri::State<'_, AppState>,
    req: Bookmark,
) -> Result<ApiResponse<Value>, AppError> {
    save_bookmark_core(&state, req).await
}

/// `save_bookmarks` 的共享实现 (批量 upsert)。
pub async fn save_bookmarks_core(
    state: &AppState,
    mut req: Vec<Bookmark>,
) -> Result<ApiResponse<Value>, AppError> {
    let user_ns = "default";
    let mut list = read_list::<Bookmark>(state, &user_ns, "bookmark.json").await?;
    req.retain(|b| !(b.book_name.is_empty() && b.book_author.is_empty()));
    for b in req {
        upsert_by_key(&mut list, b, bookmark_key);
    }
    write_list(state, &user_ns, "bookmark.json", &list).await?;
    Ok(ApiResponse::ok(Value::String("".to_string())))
}

#[tauri::command]
pub async fn save_bookmarks(
    state: tauri::State<'_, AppState>,
    req: Vec<Bookmark>,
) -> Result<ApiResponse<Value>, AppError> {
    save_bookmarks_core(&state, req).await
}

/// `delete_bookmark` 的共享实现。
pub async fn delete_bookmark_core(
    state: &AppState,
    req: Bookmark,
) -> Result<ApiResponse<Value>, AppError> {
    let user_ns = "default";
    let mut list = read_list::<Bookmark>(state, &user_ns, "bookmark.json").await?;
    list.retain(|b| bookmark_key(b) != bookmark_key(&req));
    write_list(state, &user_ns, "bookmark.json", &list).await?;
    Ok(ApiResponse::ok(Value::String("".to_string())))
}

#[tauri::command]
pub async fn delete_bookmark(
    state: tauri::State<'_, AppState>,
    req: Bookmark,
) -> Result<ApiResponse<Value>, AppError> {
    delete_bookmark_core(&state, req).await
}

/// `delete_bookmarks` 的共享实现 (批量)。
pub async fn delete_bookmarks_core(
    state: &AppState,
    req: Vec<Bookmark>,
) -> Result<ApiResponse<Value>, AppError> {
    let user_ns = "default";
    let mut list = read_list::<Bookmark>(state, &user_ns, "bookmark.json").await?;
    for b in req {
        list.retain(|v| bookmark_key(v) != bookmark_key(&b));
    }
    write_list(state, &user_ns, "bookmark.json", &list).await?;
    Ok(ApiResponse::ok(Value::String("".to_string())))
}

#[tauri::command]
pub async fn delete_bookmarks(
    state: tauri::State<'_, AppState>,
    req: Vec<Bookmark>,
) -> Result<ApiResponse<Value>, AppError> {
    delete_bookmarks_core(&state, req).await
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveBookmarkExportRequest {
    pub file_name: String,
    pub content: String,
    pub format: String,
}

/// 弹出系统保存对话框，把书签导出内容写到用户选定的位置。
///
/// 不能复用 WebDAV 的 `save_*_as`：那条命令的源文件必须已经在应用数据目录里。
/// 书签导出在 WebView 内生成文本，桌面端需要直接接收内容，浏览器端则继续使用
/// 原生下载，不经过这条 IPC。
#[tauri::command]
pub async fn save_bookmark_export_as(
    app: tauri::AppHandle,
    req: SaveBookmarkExportRequest,
) -> Result<ApiResponse<Value>, AppError> {
    let file_name = req.file_name.trim();
    if file_name.is_empty() {
        return Err(AppError::BadRequest("导出文件名不能为空".to_string()));
    }
    if file_name.contains(['/', '\\']) {
        return Err(AppError::BadRequest("导出文件名不能包含路径".to_string()));
    }
    let (label, extensions) = match req.format.as_str() {
        "json" => ("JSON 文件", ["json"]),
        "md" => ("Markdown 文件", ["md"]),
        _ => {
            return Err(AppError::BadRequest(
                "只支持导出 JSON 或 Markdown".to_string(),
            ))
        }
    };
    let mut dialog = app
        .dialog()
        .file()
        .set_title("保存书签")
        .set_file_name(file_name)
        .add_filter(label, &extensions);
    // Windows 上不指定 parent 时，rfd 会新建一个没有应用归属的对话框；
    // WebView 窗口仍在前台时，保存框可能跑到应用后面。绑定主窗口后由系统
    // 保证模态层级与任务栏激活顺序。
    if let Some(window) = app.get_webview_window("main") {
        dialog = dialog.set_parent(&window);
    }
    let Some(file) = dialog.blocking_save_file() else {
        return Ok(ApiResponse::ok(serde_json::json!({
            "saved": false,
            "cancelled": true
        })));
    };
    let target = file
        .into_path()
        .map_err(|error| AppError::BadRequest(format!("无法访问保存路径：{error}")))?;
    tokio::fs::write(&target, req.content.as_bytes())
        .await
        .map_err(|error| AppError::BadRequest(format!("保存书签失败：{error}")))?;
    Ok(ApiResponse::ok(serde_json::json!({
        "saved": true,
        "path": target.to_string_lossy()
    })))
}

async fn read_list<T: for<'de> serde::Deserialize<'de>>(
    state: &AppState,
    user_ns: &str,
    name: &str,
) -> Result<Vec<T>, AppError> {
    state.json_document_service.read_list(user_ns, name).await
}

async fn write_list<T: serde::Serialize>(
    state: &AppState,
    user_ns: &str,
    name: &str,
    list: &Vec<T>,
) -> Result<(), AppError> {
    state
        .json_document_service
        .write_list(user_ns, name, list)
        .await
}

fn upsert_by_key<T, F>(list: &mut Vec<T>, item: T, key_fn: F)
where
    F: Fn(&T) -> String,
{
    let key = key_fn(&item);
    if let Some(pos) = list.iter().position(|v| key_fn(v) == key) {
        list[pos] = item;
    } else {
        list.push(item);
    }
}

/// 书签是章节内的一条标记，不是整本书的一行记录。
///
/// 旧实现只按书名/作者 upsert 和删除，导致：
/// - 同一本书的第二个书签会把第一个覆盖掉；
/// - 管理页删一条书签会删掉这本书的全部书签。
///
/// 用「书名/作者 + 章节 + 章内位置 + 时间」做键，既保住同一章的多个书签，
/// 也能让批量删除做到精确命中。
fn bookmark_key(bookmark: &Bookmark) -> String {
    format!(
        "{}\u{0}{}\u{0}{}\u{0}{}\u{0}{}",
        bookmark.book_name,
        bookmark.book_author,
        bookmark.chapter_index,
        bookmark.chapter_pos,
        bookmark.time,
    )
}

#[cfg(test)]
mod tests {
    use super::bookmark_key;
    use crate::model::bookmark::Bookmark;

    fn sample() -> Bookmark {
        Bookmark {
            time: 1711886400000,
            book_name: "测试书".to_string(),
            book_author: "作者".to_string(),
            chapter_index: 3,
            chapter_pos: 5000,
            chapter_name: "第三章".to_string(),
            book_text: "摘录".to_string(),
            content: "想法".to_string(),
        }
    }

    #[test]
    fn bookmark_key_separates_markers_inside_one_book() {
        let first = sample();
        let mut second = sample();
        second.time += 1;
        second.chapter_pos += 1;
        assert_ne!(bookmark_key(&first), bookmark_key(&second));
    }

    #[test]
    fn bookmark_key_is_stable_for_the_same_marker() {
        assert_eq!(bookmark_key(&sample()), bookmark_key(&sample()));
    }
}
