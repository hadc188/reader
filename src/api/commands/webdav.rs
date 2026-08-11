use crate::api::AppState;
use crate::error::error::{ApiResponse, AppError};
use crate::util::time::now_ts;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::io::{Cursor, Read, Write};
use std::path::Path;
use std::path::PathBuf;
use tokio::fs;

const MAX_BACKUP_ARCHIVE_BYTES: u64 = 128 * 1024 * 1024;
const MAX_BACKUP_ENTRY_BYTES: u64 = 64 * 1024 * 1024;
const MAX_BACKUP_CONTENT_BYTES: u64 = 128 * 1024 * 1024;
const COMPATIBLE_BACKUP_FILES: [&str; 7] = [
    "reader-rust.json",
    "bookshelf.json",
    "bookmark.json",
    "bookGroup.json",
    "bookSource.json",
    "rssSources.json",
    "replaceRule.json",
];

#[derive(Debug, Deserialize)]
pub struct WebdavPathRequest {
    pub path: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct WebdavDeleteListRequest {
    pub path: Option<Vec<String>>,
}

#[derive(Debug, Deserialize)]
pub struct WebdavUploadFile {
    pub name: String,
    pub file: Vec<u8>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupArchiveFile {
    pub name: String,
    pub content: String,
}

/// Binary download result; the frontend reconstructs a Blob/TextDecoder.
#[derive(Debug, Serialize)]
pub struct BinaryResponse {
    pub bytes: Vec<u8>,
    pub content_type: Option<String>,
}

#[tauri::command]
pub async fn get_webdav_file_list(
    state: tauri::State<'_, AppState>,
    req: WebdavPathRequest,
) -> Result<ApiResponse<Value>, AppError> {
    let user_ns = "default";
    let home = webdav_home(&state, &user_ns).await?;
    let path = req.path.unwrap_or_else(|| "/".to_string());
    let parts = normalize_rel_path(&path)?;
    let full = join_parts(&home, &parts);
    if !full.exists() {
        return Ok(ApiResponse::err("路径不存在"));
    }
    if !full.is_dir() {
        return Ok(ApiResponse::err("路径不是目录"));
    }
    let mut list = Vec::new();
    let mut dir = fs::read_dir(full)
        .await
        .map_err(|e| AppError::Internal(e.into()))?;
    while let Some(entry) = dir
        .next_entry()
        .await
        .map_err(|e| AppError::Internal(e.into()))?
    {
        let meta = entry
            .metadata()
            .await
            .map_err(|e| AppError::Internal(e.into()))?;
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with('.') {
            continue;
        }
        let child_path = build_relative_path(&parts, &name);
        list.push(serde_json::json!({
            "name": name,
            "size": meta.len(),
            "path": child_path,
            "lastModified": meta.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_millis() as i64).unwrap_or(now_ts()),
            "isDirectory": meta.is_dir()
        }));
    }
    Ok(ApiResponse::ok(Value::from(list)))
}

#[tauri::command]
pub async fn get_webdav_file(
    state: tauri::State<'_, AppState>,
    req: WebdavPathRequest,
) -> Result<BinaryResponse, AppError> {
    let user_ns = "default";
    let home = webdav_home(&state, &user_ns).await?;
    let path = req.path.unwrap_or_default();
    if path.is_empty() {
        return Err(AppError::BadRequest("参数错误".to_string()));
    }
    let parts = normalize_rel_path(&path)?;
    let full = join_parts(&home, &parts);
    if !full.exists() || full.is_dir() {
        return Err(AppError::NotFound("路径不存在".to_string()));
    }
    let bytes = fs::read(full)
        .await
        .map_err(|e| AppError::Internal(e.into()))?;
    Ok(BinaryResponse {
        bytes,
        content_type: None,
    })
}

#[tauri::command]
pub async fn upload_file_to_webdav(
    state: tauri::State<'_, AppState>,
    files: Vec<WebdavUploadFile>,
    path: Option<String>,
) -> Result<ApiResponse<Value>, AppError> {
    let user_ns = "default";
    let home = webdav_home(&state, &user_ns).await?;
    let path = path.unwrap_or_else(|| "/".to_string());
    let mut file_list = Vec::new();

    let rel = normalize_rel_path(&path)?;
    let dir = join_parts(&home, &rel);
    fs::create_dir_all(&dir)
        .await
        .map_err(|e| AppError::Internal(e.into()))?;
    for upload in files {
        let target = dir.join(&upload.name);
        fs::write(&target, &upload.file)
            .await
            .map_err(|e| AppError::Internal(e.into()))?;
        let meta = fs::metadata(&target)
            .await
            .map_err(|e| AppError::Internal(e.into()))?;
        file_list.push(serde_json::json!({
            "name": upload.name,
            "size": meta.len(),
            "path": target.to_string_lossy().replace(home.to_string_lossy().as_ref(), ""),
            "lastModified": meta.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_millis() as i64).unwrap_or(now_ts()),
            "isDirectory": meta.is_dir()
        }));
    }
    Ok(ApiResponse::ok(Value::from(file_list)))
}

#[tauri::command]
pub async fn create_webdav_backup_archive(
    state: tauri::State<'_, AppState>,
    filename: String,
    files: Vec<BackupArchiveFile>,
    path: Option<String>,
) -> Result<ApiResponse<Value>, AppError> {
    validate_archive_filename(&filename)?;
    let archive_bytes = tokio::task::spawn_blocking(move || build_backup_archive(files))
        .await
        .map_err(|e| AppError::Internal(e.into()))??;

    let home = webdav_home(&state, "default").await?;
    let rel = normalize_rel_path(path.as_deref().unwrap_or("/"))?;
    let dir = join_parts(&home, &rel);
    fs::create_dir_all(&dir)
        .await
        .map_err(|e| AppError::Internal(e.into()))?;
    let target = dir.join(&filename);
    fs::write(&target, archive_bytes)
        .await
        .map_err(|e| AppError::Internal(e.into()))?;
    let meta = fs::metadata(&target)
        .await
        .map_err(|e| AppError::Internal(e.into()))?;

    Ok(ApiResponse::ok(serde_json::json!({
        "name": filename,
        "size": meta.len(),
        "path": build_relative_path(&rel, target.file_name().and_then(|name| name.to_str()).unwrap_or_default()),
        "lastModified": meta.modified().ok().and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok()).map(|duration| duration.as_millis() as i64).unwrap_or(now_ts()),
        "isDirectory": false
    })))
}

#[tauri::command]
pub async fn get_webdav_backup_archive(
    state: tauri::State<'_, AppState>,
    req: WebdavPathRequest,
) -> Result<ApiResponse<Value>, AppError> {
    let home = webdav_home(&state, "default").await?;
    let path = req.path.unwrap_or_default();
    if path.is_empty() {
        return Err(AppError::BadRequest("参数错误".to_string()));
    }
    let rel = normalize_rel_path(&path)?;
    let full = join_parts(&home, &rel);
    let meta = fs::metadata(&full)
        .await
        .map_err(|_| AppError::NotFound("备份文件不存在".to_string()))?;
    if !meta.is_file() {
        return Err(AppError::BadRequest("备份路径不是文件".to_string()));
    }
    if meta.len() > MAX_BACKUP_ARCHIVE_BYTES {
        return Err(AppError::BadRequest("备份压缩包超过 128 MB".to_string()));
    }
    let bytes = fs::read(full)
        .await
        .map_err(|e| AppError::Internal(e.into()))?;
    let contents = tokio::task::spawn_blocking(move || read_backup_archive(bytes))
        .await
        .map_err(|e| AppError::Internal(e.into()))??;
    Ok(ApiResponse::ok(
        serde_json::to_value(contents).unwrap_or_default(),
    ))
}

#[tauri::command]
pub async fn delete_webdav_file(
    state: tauri::State<'_, AppState>,
    req: WebdavPathRequest,
) -> Result<ApiResponse<Value>, AppError> {
    let user_ns = "default";
    let home = webdav_home(&state, &user_ns).await?;
    let path = req.path.unwrap_or_default();
    if path.is_empty() {
        return Ok(ApiResponse::err("参数错误"));
    }
    let rel = normalize_rel_path(&path)?;
    let target = join_parts(&home, &rel);
    if !target.exists() {
        return Ok(ApiResponse::err("路径不存在"));
    }
    if target.is_dir() {
        fs::remove_dir_all(target)
            .await
            .map_err(|e| AppError::Internal(e.into()))?;
    } else {
        fs::remove_file(target)
            .await
            .map_err(|e| AppError::Internal(e.into()))?;
    }
    Ok(ApiResponse::ok(Value::String("".to_string())))
}

#[tauri::command]
pub async fn delete_webdav_file_list(
    state: tauri::State<'_, AppState>,
    req: WebdavDeleteListRequest,
) -> Result<ApiResponse<Value>, AppError> {
    let user_ns = "default";
    let home = webdav_home(&state, &user_ns).await?;
    let paths = req.path.unwrap_or_default();
    for p in paths {
        if p.is_empty() {
            continue;
        }
        let rel = normalize_rel_path(&p)?;
        let target = join_parts(&home, &rel);
        if target.exists() {
            if target.is_dir() {
                let _ = fs::remove_dir_all(target).await;
            } else {
                let _ = fs::remove_file(target).await;
            }
        }
    }
    Ok(ApiResponse::ok(Value::String("".to_string())))
}

async fn webdav_home(state: &AppState, user_ns: &str) -> Result<PathBuf, AppError> {
    let dir = PathBuf::from(&state.config.storage_dir)
        .join("webdav")
        .join(user_ns);
    fs::create_dir_all(&dir)
        .await
        .map_err(|e| AppError::Internal(e.into()))?;
    Ok(dir)
}

/// Return the absolute path of the local backup directory so the UI can show
/// where backups are stored.
#[tauri::command]
pub async fn get_webdav_home(
    state: tauri::State<'_, AppState>,
) -> Result<ApiResponse<Value>, AppError> {
    let user_ns = "default";
    let home = webdav_home(&state, &user_ns).await?;
    Ok(ApiResponse::ok(serde_json::json!({
        "path": home.to_string_lossy().into_owned()
    })))
}

/// Open the local backup directory in the system file explorer.
#[tauri::command]
pub async fn open_webdav_folder(
    state: tauri::State<'_, AppState>,
) -> Result<ApiResponse<Value>, AppError> {
    let user_ns = "default";
    let home = webdav_home(&state, &user_ns).await?;
    let path = home.to_string_lossy().into_owned();
    // Windows-only: explorer.exe opens a directory when given its path.
    let _ = std::process::Command::new("explorer.exe").arg(&path).spawn();
    Ok(ApiResponse::ok(serde_json::json!({ "opened": true })))
}

fn normalize_rel_path(path: &str) -> Result<Vec<String>, AppError> {
    let mut parts = Vec::new();
    for p in path.split('/') {
        if p.is_empty() || p == "." {
            continue;
        }
        if p == ".." {
            return Err(AppError::BadRequest("非法路径".to_string()));
        }
        parts.push(p.to_string());
    }
    Ok(parts)
}

fn join_parts(home: &PathBuf, parts: &Vec<String>) -> PathBuf {
    let mut p = home.clone();
    for part in parts {
        p = p.join(part);
    }
    p
}

fn build_relative_path(parts: &[String], name: &str) -> String {
    if parts.is_empty() {
        format!("/{}", name)
    } else {
        format!("/{}/{}", parts.join("/"), name)
    }
}

fn validate_archive_filename(filename: &str) -> Result<(), AppError> {
    let trimmed = filename.trim();
    let is_plain_name = Path::new(trimmed)
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name == trimmed);
    if !is_plain_name || !trimmed.to_ascii_lowercase().ends_with(".zip") {
        return Err(AppError::BadRequest("备份文件名无效".to_string()));
    }
    Ok(())
}

fn validate_archive_entry_name(name: &str) -> Result<(), AppError> {
    if !COMPATIBLE_BACKUP_FILES.contains(&name) {
        return Err(AppError::BadRequest(format!("不支持的备份条目: {name}")));
    }
    Ok(())
}

fn build_backup_archive(files: Vec<BackupArchiveFile>) -> Result<Vec<u8>, AppError> {
    if files.is_empty() {
        return Err(AppError::BadRequest("备份内容为空".to_string()));
    }
    let cursor = Cursor::new(Vec::new());
    let mut archive = zip::ZipWriter::new(cursor);
    let options = zip::write::FileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated)
        .unix_permissions(0o644);
    let mut total_bytes = 0u64;

    for file in files {
        validate_archive_entry_name(&file.name)?;
        let content_bytes = file.content.as_bytes();
        if content_bytes.len() as u64 > MAX_BACKUP_ENTRY_BYTES {
            return Err(AppError::BadRequest(format!(
                "备份条目 {} 超过 64 MB",
                file.name
            )));
        }
        total_bytes += content_bytes.len() as u64;
        if total_bytes > MAX_BACKUP_CONTENT_BYTES {
            return Err(AppError::BadRequest("备份内容超过 128 MB".to_string()));
        }
        archive
            .start_file(&file.name, options)
            .map_err(|e| AppError::BadRequest(format!("创建备份压缩包失败: {e}")))?;
        archive
            .write_all(content_bytes)
            .map_err(|e| AppError::Internal(e.into()))?;
    }

    let cursor = archive
        .finish()
        .map_err(|e| AppError::BadRequest(format!("创建备份压缩包失败: {e}")))?;
    Ok(cursor.into_inner())
}

fn read_backup_archive(bytes: Vec<u8>) -> Result<HashMap<String, String>, AppError> {
    let cursor = Cursor::new(bytes);
    let mut archive = zip::ZipArchive::new(cursor)
        .map_err(|_| AppError::BadRequest("无法识别该备份压缩包".to_string()))?;
    let mut contents = HashMap::new();
    let mut total_bytes = 0u64;

    for index in 0..archive.len() {
        let mut entry = archive
            .by_index(index)
            .map_err(|_| AppError::BadRequest("备份压缩包存在损坏条目".to_string()))?;
        if entry.is_dir() {
            continue;
        }
        let Some(file_name) = Path::new(entry.name())
            .file_name()
            .and_then(|name| name.to_str())
            .map(str::to_string)
        else {
            continue;
        };
        if !COMPATIBLE_BACKUP_FILES.contains(&file_name.as_str())
            || contents.contains_key(&file_name)
        {
            continue;
        }
        if entry.size() > MAX_BACKUP_ENTRY_BYTES {
            return Err(AppError::BadRequest(format!(
                "备份条目 {file_name} 超过 64 MB"
            )));
        }
        total_bytes += entry.size();
        if total_bytes > MAX_BACKUP_CONTENT_BYTES {
            return Err(AppError::BadRequest("备份解压内容超过 128 MB".to_string()));
        }
        let mut content = String::new();
        entry
            .read_to_string(&mut content)
            .map_err(|_| AppError::BadRequest(format!("备份条目 {file_name} 不是有效文本")))?;
        contents.insert(file_name, content);
    }

    if contents.is_empty() {
        return Err(AppError::BadRequest(
            "压缩包中未找到可恢复的数据".to_string(),
        ));
    }
    Ok(contents)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compatible_backup_archive_round_trip() {
        let bytes = build_backup_archive(vec![
            BackupArchiveFile {
                name: "bookshelf.json".to_string(),
                content: "[{\"name\":\"测试书籍\"}]".to_string(),
            },
            BackupArchiveFile {
                name: "bookSource.json".to_string(),
                content: "[]".to_string(),
            },
        ])
        .unwrap();

        let contents = read_backup_archive(bytes).unwrap();

        assert_eq!(contents["bookshelf.json"], "[{\"name\":\"测试书籍\"}]");
        assert_eq!(contents["bookSource.json"], "[]");
    }

    #[test]
    fn rejects_archive_without_compatible_entries() {
        let cursor = Cursor::new(Vec::new());
        let mut archive = zip::ZipWriter::new(cursor);
        archive
            .start_file("unknown.json", zip::write::FileOptions::default())
            .unwrap();
        archive.write_all(b"[]").unwrap();
        let bytes = archive.finish().unwrap().into_inner();

        let error = read_backup_archive(bytes).unwrap_err();

        assert!(matches!(error, AppError::BadRequest(message) if message.contains("未找到可恢复的数据")));
    }
}
