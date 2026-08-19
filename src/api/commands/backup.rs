//! 备份增强命令: 本地书籍内容、自定义字体、阅读统计的导出/导入。
//!
//! 本地书(txt/epub/pdf)与字体都是存储目录里的原始文件, 以 base64 进入备份
//! payload; 阅读统计来自 SQLite 的两张统计表。导出带体积上限, 超限条目跳过
//! 并在返回值中列出, 由前端提示用户。

use crate::api::AppState;
use crate::error::error::{ApiResponse, AppError};
use crate::service::reading_stats_service::{BookStatsRow, DailyStatsRow};
use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;
use serde::Deserialize;
use serde_json::Value;
use std::path::{Path, PathBuf};
use tokio::fs;

/// 单本本地书导出上限(原始字节), 与各导入上限同量级。
const MAX_LOCAL_BOOK_EXPORT_BYTES: u64 = 100 * 1024 * 1024;
/// 本地书总导出上限, 防止 payload 撑爆前端内存。
const MAX_LOCAL_BOOKS_TOTAL_BYTES: u64 = 300 * 1024 * 1024;
/// 单个字体文件导出上限(上传上限 32MB, 留余量)。
const MAX_FONT_EXPORT_BYTES: u64 = 64 * 1024 * 1024;
const MAX_FONTS_TOTAL_BYTES: u64 = 128 * 1024 * 1024;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportLocalBooksRequest {
    pub books: Vec<LocalBookFilesInput>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalBookFilesInput {
    pub id: String,
    pub files: Vec<LocalBookFileInput>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalBookFileInput {
    pub path: String,
    pub base64: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportCustomFontsRequest {
    pub fonts: Vec<CustomFontInput>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CustomFontInput {
    pub file_name: String,
    pub base64: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportReadingStatsRequest {
    pub daily: Vec<DailyStatsRow>,
    pub by_book: Vec<BookStatsRow>,
}

fn local_books_root(state: &AppState) -> PathBuf {
    Path::new(&state.config.storage_dir)
        .join("data")
        .join("default")
        .join("local_books")
}

fn custom_fonts_root(state: &AppState) -> PathBuf {
    Path::new(&state.config.storage_dir)
        .join("assets")
        .join("default")
        .join("fonts")
}

/// 本地书目录名是 bookUrl 里的 hash(各服务一致: data/<ns>/local_books/<hash>)。
fn valid_local_book_id(id: &str) -> bool {
    (8..=64).contains(&id.len()) && id.chars().all(|c| c.is_ascii_alphanumeric())
}

/// 字体文件名形如 "<32位hex>__<名称>.<扩展名>"(见 user.rs upload_custom_font)。
fn valid_custom_font_file_name(name: &str) -> bool {
    let Some((id, _rest)) = name.split_once("__") else {
        return false;
    };
    id.len() == 32
        && id.chars().all(|c| c.is_ascii_hexdigit())
        && !name.contains('/')
        && !name.contains('\\')
        && !name.contains("..")
}

/// 相对路径只允许平铺的安全组件, 杜绝目录穿越。
fn safe_rel_path(path: &str) -> bool {
    !path.is_empty()
        && path.len() <= 512
        && !path.contains('\\')
        && !path.contains(':')
        && path
            .split('/')
            .all(|part| !part.is_empty() && part != "." && part != "..")
}

/// 迭代式收集目录下全部文件(相对路径, 字节数, 内容)。
async fn collect_dir_files(dir: &Path, out: &mut Vec<(String, u64, Vec<u8>)>) -> Result<(), AppError> {
    let mut stack = vec![(dir.to_path_buf(), String::new())];
    while let Some((current, prefix)) = stack.pop() {
        let mut entries = fs::read_dir(&current).await.map_err(|e| AppError::Internal(e.into()))?;
        while let Some(entry) = entries
            .next_entry()
            .await
            .map_err(|e| AppError::Internal(e.into()))?
        {
            let file_type = entry
                .file_type()
                .await
                .map_err(|e| AppError::Internal(e.into()))?;
            let name = entry.file_name().to_string_lossy().to_string();
            let rel = if prefix.is_empty() { name.clone() } else { format!("{prefix}/{name}") };
            if file_type.is_dir() {
                stack.push((entry.path(), rel));
            } else if file_type.is_file() {
                if !safe_rel_path(&rel) || !valid_file_name(&name) {
                    continue;
                }
                let bytes = fs::read(entry.path()).await.map_err(|e| AppError::Internal(e.into()))?;
                out.push((rel, bytes.len() as u64, bytes));
            }
        }
    }
    Ok(())
}

fn valid_file_name(name: &str) -> bool {
    !name.is_empty() && name.len() <= 255 && !name.starts_with('.')
}

#[tauri::command]
pub async fn export_local_books(
    state: tauri::State<'_, AppState>,
) -> Result<ApiResponse<Value>, AppError> {
    let root = local_books_root(&state);
    let mut books = Vec::new();
    let mut skipped = Vec::new();
    let mut total: u64 = 0;

    if root.exists() {
        let mut entries = fs::read_dir(&root).await.map_err(|e| AppError::Internal(e.into()))?;
        while let Some(entry) = entries
            .next_entry()
            .await
            .map_err(|e| AppError::Internal(e.into()))?
        {
            if !entry
                .file_type()
                .await
                .map_err(|e| AppError::Internal(e.into()))?
                .is_dir()
            {
                continue;
            }
            let id = entry.file_name().to_string_lossy().to_string();
            if !valid_local_book_id(&id) {
                continue;
            }
            let mut files = Vec::new();
            collect_dir_files(&entry.path(), &mut files).await?;
            let size: u64 = files.iter().map(|(_, len, _)| *len).sum();
            if size > MAX_LOCAL_BOOK_EXPORT_BYTES || total + size > MAX_LOCAL_BOOKS_TOTAL_BYTES {
                skipped.push(serde_json::json!({ "id": id, "sizeBytes": size }));
                continue;
            }
            total += size;
            books.push(serde_json::json!({
                "id": id,
                "sizeBytes": size,
                "files": files
                    .into_iter()
                    .map(|(path, _, bytes)| serde_json::json!({
                        "path": path,
                        "base64": BASE64.encode(bytes),
                    }))
                    .collect::<Vec<_>>(),
            }));
        }
    }

    Ok(ApiResponse::ok(serde_json::json!({
        "books": books,
        "skipped": skipped,
        "totalBytes": total,
    })))
}

#[tauri::command]
pub async fn import_local_books(
    state: tauri::State<'_, AppState>,
    req: ImportLocalBooksRequest,
) -> Result<ApiResponse<Value>, AppError> {
    let root = local_books_root(&state);
    let mut imported = 0usize;
    for book in req.books {
        if !valid_local_book_id(&book.id) || book.files.is_empty() {
            return Err(AppError::BadRequest("备份中的本地书籍数据无效".to_string()));
        }
        let book_dir = root.join(&book.id);
        if book_dir.exists() {
            fs::remove_dir_all(&book_dir)
                .await
                .map_err(|e| AppError::Internal(e.into()))?;
        }
        fs::create_dir_all(&book_dir)
            .await
            .map_err(|e| AppError::Internal(e.into()))?;
        for file in book.files {
            if !safe_rel_path(&file.path) {
                return Err(AppError::BadRequest("备份中的本地书籍文件路径无效".to_string()));
            }
            let bytes = BASE64
                .decode(file.base64.as_bytes())
                .map_err(|_| AppError::BadRequest("备份中的本地书籍文件内容无效".to_string()))?;
            let target = book_dir.join(&file.path);
            if let Some(parent) = target.parent() {
                fs::create_dir_all(parent)
                    .await
                    .map_err(|e| AppError::Internal(e.into()))?;
            }
            fs::write(&target, bytes)
                .await
                .map_err(|e| AppError::Internal(e.into()))?;
        }
        imported += 1;
    }
    Ok(ApiResponse::ok(
        serde_json::json!({ "imported": imported }),
    ))
}

#[tauri::command]
pub async fn export_custom_fonts(
    state: tauri::State<'_, AppState>,
) -> Result<ApiResponse<Value>, AppError> {
    let root = custom_fonts_root(&state);
    let mut fonts = Vec::new();
    let mut skipped = Vec::new();
    let mut total: u64 = 0;

    if root.exists() {
        let mut entries = fs::read_dir(&root).await.map_err(|e| AppError::Internal(e.into()))?;
        while let Some(entry) = entries
            .next_entry()
            .await
            .map_err(|e| AppError::Internal(e.into()))?
        {
            if !entry
                .file_type()
                .await
                .map_err(|e| AppError::Internal(e.into()))?
                .is_file()
            {
                continue;
            }
            let file_name = entry.file_name().to_string_lossy().to_string();
            if !valid_custom_font_file_name(&file_name) {
                continue;
            }
            let bytes = fs::read(entry.path()).await.map_err(|e| AppError::Internal(e.into()))?;
            let size = bytes.len() as u64;
            if size > MAX_FONT_EXPORT_BYTES || total + size > MAX_FONTS_TOTAL_BYTES {
                skipped.push(file_name);
                continue;
            }
            total += size;
            fonts.push(serde_json::json!({
                "fileName": file_name,
                "base64": BASE64.encode(bytes),
            }));
        }
    }

    Ok(ApiResponse::ok(serde_json::json!({
        "fonts": fonts,
        "skipped": skipped,
        "totalBytes": total,
    })))
}

#[tauri::command]
pub async fn import_custom_fonts(
    state: tauri::State<'_, AppState>,
    req: ImportCustomFontsRequest,
) -> Result<ApiResponse<Value>, AppError> {
    let root = custom_fonts_root(&state);
    fs::create_dir_all(&root)
        .await
        .map_err(|e| AppError::Internal(e.into()))?;
    let mut imported = 0usize;
    for font in req.fonts {
        if !valid_custom_font_file_name(&font.file_name) {
            return Err(AppError::BadRequest("备份中的字体文件名无效".to_string()));
        }
        let bytes = BASE64
            .decode(font.base64.as_bytes())
            .map_err(|_| AppError::BadRequest("备份中的字体内容无效".to_string()))?;
        fs::write(root.join(&font.file_name), bytes)
            .await
            .map_err(|e| AppError::Internal(e.into()))?;
        imported += 1;
    }
    Ok(ApiResponse::ok(
        serde_json::json!({ "imported": imported }),
    ))
}

#[tauri::command]
pub async fn export_reading_stats(
    state: tauri::State<'_, AppState>,
) -> Result<ApiResponse<Value>, AppError> {
    let user_ns = "default";
    let daily = state
        .reading_stats_service
        .get_all_daily(user_ns)
        .await?;
    let by_book = state
        .reading_stats_service
        .get_all_book_rows(user_ns)
        .await?;
    Ok(ApiResponse::ok(serde_json::json!({
        "daily": daily,
        "byBook": by_book,
    })))
}

#[tauri::command]
pub async fn import_reading_stats(
    state: tauri::State<'_, AppState>,
    req: ImportReadingStatsRequest,
) -> Result<ApiResponse<Value>, AppError> {
    let user_ns = "default";
    let valid_date = |value: &str| {
        value.len() == 10 && value.chars().enumerate().all(|(i, c)| match i {
            4 | 7 => c == '-',
            _ => c.is_ascii_digit(),
        })
    };
    if !req.daily.iter().all(|row| valid_date(&row.date))
        || !req.by_book.iter().all(|row| valid_date(&row.date))
    {
        return Err(AppError::BadRequest("备份中的阅读统计数据无效".to_string()));
    }
    state
        .reading_stats_service
        .replace_all(user_ns, &req.daily, &req.by_book)
        .await?;
    Ok(ApiResponse::ok(serde_json::json!({
        "daily": req.daily.len(),
        "byBook": req.by_book.len(),
    })))
}
