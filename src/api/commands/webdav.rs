use crate::api::AppState;
use crate::error::error::{ApiResponse, AppError};
use crate::util::time::now_ts;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::time::Duration;
use std::io::{Cursor, Read, Write};
use std::path::Path;
use std::path::PathBuf;
use tauri_plugin_dialog::DialogExt;
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

const MAX_SYNC_PROGRESS_BYTES: usize = 64 * 1024;

#[derive(Debug, Deserialize, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct LegadoWebdavConfig {
    pub url: String,
    pub account: String,
    pub password: String,
    pub directory: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct LegadoBookProgress {
    pub name: String,
    pub author: String,
    pub dur_chapter_index: i32,
    pub dur_chapter_pos: i32,
    pub dur_chapter_time: i64,
    pub dur_chapter_title: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LegadoProgressRequest {
    pub config: LegadoWebdavConfig,
    pub progress: LegadoBookProgress,
    pub allow_upload: Option<bool>,
    pub force_upload: Option<bool>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LegadoProgressResponse {
    pub configured: bool,
    pub remote: Option<LegadoBookProgress>,
    pub uploaded: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LegadoWebdavBackupRequest {
    pub config: LegadoWebdavConfig,
    pub filename: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LegadoWebdavBackupUploadRequest {
    pub config: LegadoWebdavConfig,
    pub filename: String,
    pub files: Vec<BackupArchiveFile>,
}

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct LegadoWebdavBackupEntry {
    pub name: String,
    pub size: u64,
    pub last_modified: i64,
}

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
pub async fn save_webdav_file_as(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    req: WebdavPathRequest,
) -> Result<ApiResponse<Value>, AppError> {
    let home = webdav_home(&state, "default").await?;
    let path = req.path.unwrap_or_default();
    if path.is_empty() {
        return Err(AppError::BadRequest("参数错误".to_string()));
    }
    let parts = normalize_rel_path(&path)?;
    let source = join_parts(&home, &parts);
    if !source.exists() || source.is_dir() {
        return Err(AppError::NotFound("备份文件不存在".to_string()));
    }
    let file_name = source
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .unwrap_or("reader-backup.zip");
    let Some(file) = app
        .dialog()
        .file()
        .set_title("下载备份")
        .set_file_name(file_name)
        .add_filter("备份文件", &["zip", "json"])
        .blocking_save_file()
    else {
        return Ok(ApiResponse::ok(serde_json::json!({"saved": false, "cancelled": true})));
    };
    let target = file
        .into_path()
        .map_err(|error| AppError::BadRequest(format!("无法访问保存路径：{error}")))?;
    if source != target {
        fs::copy(&source, &target)
            .await
            .map_err(|error| AppError::BadRequest(format!("保存备份失败：{error}")))?;
    }
    Ok(ApiResponse::ok(serde_json::json!({
        "saved": true,
        "path": target.to_string_lossy()
    })))
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

#[tauri::command]
pub async fn test_legado_webdav(
    config: LegadoWebdavConfig,
) -> Result<ApiResponse<Value>, AppError> {
    let client = webdav_client()?;
    let (directory_urls, root_url) = legado_root_webdav_urls(&config)?;
    ensure_webdav_directories(&client, &directory_urls, &config).await?;
    let method = reqwest::Method::from_bytes(b"PROPFIND")
        .map_err(|error| AppError::Internal(error.into()))?;
    let response = authorized_request(&client, method, &root_url, &config)
        .header("Depth", "0")
        .send()
        .await?;
    ensure_webdav_status(response, &[200, 207]).await?;
    Ok(ApiResponse::ok(serde_json::json!({ "connected": true })))
}

#[tauri::command]
pub async fn sync_legado_book_progress(
    req: LegadoProgressRequest,
) -> Result<ApiResponse<LegadoProgressResponse>, AppError> {
    validate_progress(&req.progress)?;
    let client = webdav_client()?;
    let (_, progress_dir) = ensure_legado_progress_dir(&client, &req.config).await?;
    let file_url = format!(
        "{}{}",
        progress_dir,
        legado_progress_file_name(&req.progress.name, &req.progress.author)
    );
    if req.force_upload.unwrap_or(false) {
        upload_legado_progress(&client, &file_url, &req.config, &req.progress).await?;
        return Ok(ApiResponse::ok(LegadoProgressResponse {
            configured: true,
            remote: None,
            uploaded: true,
        }));
    }

    let remote = download_legado_progress(&client, &file_url, &req.config).await?;
    let remote_is_newer = remote
        .as_ref()
        .is_some_and(|value| compare_sync_progress(value, &req.progress).is_gt());

    if remote_is_newer {
        return Ok(ApiResponse::ok(LegadoProgressResponse {
            configured: true,
            remote,
            uploaded: false,
        }));
    }

    let should_upload = req.allow_upload.unwrap_or(true)
        && remote
            .as_ref()
            .is_none_or(|value| compare_sync_progress(&req.progress, value).is_gt());
    if should_upload {
        upload_legado_progress(&client, &file_url, &req.config, &req.progress).await?;
    }

    Ok(ApiResponse::ok(LegadoProgressResponse {
        configured: true,
        remote: None,
        uploaded: should_upload,
    }))
}

#[tauri::command]
pub async fn list_legado_webdav_backups(
    config: LegadoWebdavConfig,
) -> Result<ApiResponse<Vec<LegadoWebdavBackupEntry>>, AppError> {
    let client = webdav_client()?;
    let (directory_urls, root_url) = legado_root_webdav_urls(&config)?;
    ensure_webdav_directories(&client, &directory_urls, &config).await?;
    let method = reqwest::Method::from_bytes(b"PROPFIND")
        .map_err(|error| AppError::Internal(error.into()))?;
    let response = authorized_request(&client, method, &root_url, &config)
        .header("Depth", "1")
        .header(reqwest::header::CONTENT_TYPE, "application/xml")
        .body("<?xml version=\"1.0\" encoding=\"utf-8\"?><d:propfind xmlns:d=\"DAV:\"><d:prop><d:getcontentlength/><d:getlastmodified/><d:resourcetype/></d:prop></d:propfind>")
        .send()
        .await?;
    let response = ensure_webdav_status(response, &[200, 207]).await?;
    let body = response.bytes().await?;
    let mut entries = parse_webdav_backup_entries(&body, &root_url)?;
    entries.sort_by(|left, right| right.last_modified.cmp(&left.last_modified));
    Ok(ApiResponse::ok(entries))
}

#[tauri::command]
pub async fn upload_legado_webdav_backup(
    req: LegadoWebdavBackupUploadRequest,
) -> Result<ApiResponse<LegadoWebdavBackupEntry>, AppError> {
    validate_archive_filename(&req.filename)?;
    let archive_bytes = tokio::task::spawn_blocking(move || build_backup_archive(req.files))
        .await
        .map_err(|error| AppError::Internal(error.into()))??;
    let client = webdav_client()?;
    let (directory_urls, root_url) = legado_root_webdav_urls(&req.config)?;
    ensure_webdav_directories(&client, &directory_urls, &req.config).await?;
    let file_url = format!("{}{}", root_url, encode_webdav_filename(&req.filename));
    let size = archive_bytes.len() as u64;
    let response = authorized_request(&client, reqwest::Method::PUT, &file_url, &req.config)
        .header(reqwest::header::CONTENT_TYPE, "application/zip")
        .body(archive_bytes)
        .send()
        .await?;
    ensure_webdav_status(response, &[200, 201, 204]).await?;
    Ok(ApiResponse::ok(LegadoWebdavBackupEntry {
        name: req.filename,
        size,
        last_modified: now_ts() * 1000,
    }))
}

#[tauri::command]
pub async fn download_legado_webdav_backup(
    req: LegadoWebdavBackupRequest,
) -> Result<BinaryResponse, AppError> {
    validate_archive_filename(&req.filename)?;
    let client = webdav_client()?;
    let (_, root_url) = legado_root_webdav_urls(&req.config)?;
    let file_url = format!("{}{}", root_url, encode_webdav_filename(&req.filename));
    let response = authorized_request(&client, reqwest::Method::GET, &file_url, &req.config)
        .send()
        .await?;
    let response = ensure_webdav_status(response, &[200]).await?;
    if response.content_length().is_some_and(|size| size > MAX_BACKUP_ARCHIVE_BYTES) {
        return Err(AppError::BadRequest("远端备份压缩包超过 128 MB".to_string()));
    }
    let bytes = response.bytes().await?;
    if bytes.len() as u64 > MAX_BACKUP_ARCHIVE_BYTES {
        return Err(AppError::BadRequest("远端备份压缩包超过 128 MB".to_string()));
    }
    Ok(BinaryResponse { bytes: bytes.to_vec(), content_type: Some("application/zip".to_string()) })
}

#[tauri::command]
pub async fn save_legado_webdav_backup_as(
    app: tauri::AppHandle,
    req: LegadoWebdavBackupRequest,
) -> Result<ApiResponse<Value>, AppError> {
    let file_name = req.filename.clone();
    let binary = download_legado_webdav_backup(req).await?;
    let Some(file) = app
        .dialog()
        .file()
        .set_title("下载网盘备份")
        .set_file_name(&file_name)
        .add_filter("ZIP 备份", &["zip"])
        .blocking_save_file()
    else {
        return Ok(ApiResponse::ok(serde_json::json!({"saved": false, "cancelled": true})));
    };
    let target = file
        .into_path()
        .map_err(|error| AppError::BadRequest(format!("无法访问保存路径：{error}")))?;
    fs::write(&target, binary.bytes)
        .await
        .map_err(|error| AppError::BadRequest(format!("保存备份失败：{error}")))?;
    Ok(ApiResponse::ok(serde_json::json!({
        "saved": true,
        "path": target.to_string_lossy()
    })))
}

#[tauri::command]
pub async fn get_legado_webdav_backup_archive(
    req: LegadoWebdavBackupRequest,
) -> Result<ApiResponse<Value>, AppError> {
    let binary = download_legado_webdav_backup(req).await?;
    let contents = tokio::task::spawn_blocking(move || read_backup_archive(binary.bytes))
        .await
        .map_err(|error| AppError::Internal(error.into()))??;
    Ok(ApiResponse::ok(serde_json::to_value(contents).unwrap_or_default()))
}

#[tauri::command]
pub async fn delete_legado_webdav_backup(
    req: LegadoWebdavBackupRequest,
) -> Result<ApiResponse<String>, AppError> {
    validate_archive_filename(&req.filename)?;
    let client = webdav_client()?;
    let (_, root_url) = legado_root_webdav_urls(&req.config)?;
    let file_url = format!("{}{}", root_url, encode_webdav_filename(&req.filename));
    let response = authorized_request(&client, reqwest::Method::DELETE, &file_url, &req.config)
        .send()
        .await?;
    ensure_webdav_status(response, &[200, 204, 404]).await?;
    Ok(ApiResponse::ok(String::new()))
}

fn webdav_client() -> Result<reqwest::Client, AppError> {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(20))
        .build()
        .map_err(AppError::Http)
}

fn authorized_request(
    client: &reqwest::Client,
    method: reqwest::Method,
    url: &str,
    config: &LegadoWebdavConfig,
) -> reqwest::RequestBuilder {
    client
        .request(method, url)
        .basic_auth(config.account.trim(), Some(config.password.as_str()))
}

async fn ensure_legado_progress_dir(
    client: &reqwest::Client,
    config: &LegadoWebdavConfig,
) -> Result<(String, String), AppError> {
    let (directory_urls, progress_dir) = legado_webdav_urls(config)?;
    ensure_webdav_directories(client, &directory_urls, config).await?;
    Ok((config.url.trim().to_string(), progress_dir))
}

fn legado_webdav_urls(config: &LegadoWebdavConfig) -> Result<(Vec<String>, String), AppError> {
    let (mut urls, root_url) = legado_root_webdav_urls(config)?;
    let mut progress_url = url::Url::parse(&root_url)
        .map_err(|_| AppError::BadRequest("网盘地址格式无效".to_string()))?;
    progress_url
        .path_segments_mut()
        .map_err(|_| AppError::BadRequest("网盘地址不能作为目录使用".to_string()))?
        .pop_if_empty()
        .push("bookProgress")
        .push("");
    urls.push(progress_url.to_string());
    Ok((urls, progress_url.to_string()))
}

fn legado_root_webdav_urls(config: &LegadoWebdavConfig) -> Result<(Vec<String>, String), AppError> {
    let raw_url = config.url.trim();
    if raw_url.is_empty() || config.account.trim().is_empty() || config.password.is_empty() {
        return Err(AppError::BadRequest("请完整填写网盘地址、账号和密码".to_string()));
    }
    let mut url = url::Url::parse(raw_url)
        .map_err(|_| AppError::BadRequest("网盘地址格式无效".to_string()))?;
    if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() {
        return Err(AppError::BadRequest("网盘地址仅支持 HTTP 或 HTTPS".to_string()));
    }
    url.set_query(None);
    url.set_fragment(None);

    let directory = config
        .directory
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("legado");
    let directory_parts: Vec<&str> = directory
        .split('/')
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .collect();
    if directory_parts.is_empty() || directory_parts.iter().any(|part| *part == "." || *part == "..") {
        return Err(AppError::BadRequest("同步子目录无效".to_string()));
    }

    let mut urls = Vec::new();
    for part in directory_parts {
        url.path_segments_mut()
            .map_err(|_| AppError::BadRequest("网盘地址不能作为目录使用".to_string()))?
            .pop_if_empty()
            .push(part)
            .push("");
        urls.push(url.to_string());
    }
    let root_url = urls.last().cloned().unwrap_or_default();
    Ok((urls, root_url))
}

async fn ensure_webdav_directories(
    client: &reqwest::Client,
    directory_urls: &[String],
    config: &LegadoWebdavConfig,
) -> Result<(), AppError> {
    let method = reqwest::Method::from_bytes(b"MKCOL")
        .map_err(|error| AppError::Internal(error.into()))?;
    for url in directory_urls {
        let response = authorized_request(client, method.clone(), url, config)
            .send()
            .await?;
        ensure_webdav_status(response, &[200, 201, 204, 405]).await?;
    }
    Ok(())
}

fn parse_webdav_backup_entries(
    body: &[u8],
    root_url: &str,
) -> Result<Vec<LegadoWebdavBackupEntry>, AppError> {
    use quick_xml::events::Event;
    use quick_xml::Reader;

    let root_path = url::Url::parse(root_url)
        .map_err(|_| AppError::BadRequest("网盘地址格式无效".to_string()))?
        .path()
        .trim_end_matches('/')
        .to_string();
    let mut reader = Reader::from_reader(Cursor::new(body));
    reader.trim_text(true);
    let mut buffer = Vec::new();
    let mut current: Option<LegadoWebdavBackupEntry> = None;
    let mut field = String::new();
    let mut entries = Vec::new();
    loop {
        match reader.read_event_into(&mut buffer) {
            Ok(Event::Start(event)) => {
                let name = String::from_utf8_lossy(event.local_name().as_ref()).to_ascii_lowercase();
                if name == "response" {
                    current = None;
                } else if ["href", "getcontentlength", "getlastmodified"].contains(&name.as_str()) {
                    field = name;
                }
            }
            Ok(Event::Empty(_event)) => {}
            Ok(Event::Text(text)) if !field.is_empty() => {
                let value = text.unescape().map_err(|error| AppError::BadRequest(error.to_string()))?;
                match field.as_str() {
                    "href" => {
                        let href = value.trim();
                        let path = url::Url::parse(href).ok().map(|url| url.path().to_string()).unwrap_or_else(|| href.to_string());
                        let relative = path.strip_prefix(&root_path).unwrap_or(&path).trim_matches('/');
                        if !relative.contains('/') && !relative.is_empty() {
                            let name = urlencoding::decode(relative).unwrap_or_else(|_| relative.into()).into_owned();
                            if name.starts_with("backup") && name.to_ascii_lowercase().ends_with(".zip") {
                                current = Some(LegadoWebdavBackupEntry { name, size: 0, last_modified: 0 });
                            }
                        }
                    }
                    "getcontentlength" => {
                        if let Some(entry) = current.as_mut() { entry.size = value.trim().parse().unwrap_or(0); }
                    }
                    "getlastmodified" => {
                        if let Some(entry) = current.as_mut() { entry.last_modified = parse_http_date_millis(value.trim()); }
                    }
                    _ => {}
                }
                field.clear();
            }
            Ok(Event::End(event)) => {
                if String::from_utf8_lossy(event.local_name().as_ref()).eq_ignore_ascii_case("response") {
                    if let Some(entry) = current.take() { entries.push(entry); }
                }
                field.clear();
            }
            Ok(Event::Eof) => break,
            Err(error) => return Err(AppError::BadRequest(format!("网盘目录格式无效：{}", error))),
            _ => {}
        }
        buffer.clear();
    }
    Ok(entries)
}

fn parse_http_date_millis(value: &str) -> i64 {
    chrono::DateTime::parse_from_rfc2822(value).map(|date| date.timestamp_millis()).unwrap_or(0)
}

fn encode_webdav_filename(name: &str) -> String {
    name.bytes()
        .map(|byte| match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' => (byte as char).to_string(),
            _ => format!("%{:02X}", byte),
        })
        .collect()
}

async fn download_legado_progress(
    client: &reqwest::Client,
    url: &str,
    config: &LegadoWebdavConfig,
) -> Result<Option<LegadoBookProgress>, AppError> {
    let response = authorized_request(client, reqwest::Method::GET, url, config)
        .send()
        .await?;
    if response.status() == reqwest::StatusCode::NOT_FOUND {
        return Ok(None);
    }
    let response = ensure_webdav_status(response, &[200]).await?;
    if response.content_length().is_some_and(|size| size as usize > MAX_SYNC_PROGRESS_BYTES) {
        return Err(AppError::BadRequest("云端阅读进度文件过大".to_string()));
    }
    let bytes = response.bytes().await?;
    if bytes.len() > MAX_SYNC_PROGRESS_BYTES {
        return Err(AppError::BadRequest("云端阅读进度文件过大".to_string()));
    }
    let progress = serde_json::from_slice::<LegadoBookProgress>(&bytes)
        .map_err(|_| AppError::BadRequest("云端阅读进度文件格式无效".to_string()))?;
    validate_progress(&progress)?;
    Ok(Some(progress))
}

async fn upload_legado_progress(
    client: &reqwest::Client,
    url: &str,
    config: &LegadoWebdavConfig,
    progress: &LegadoBookProgress,
) -> Result<(), AppError> {
    let body = serde_json::to_vec(progress)
        .map_err(|error| AppError::Internal(error.into()))?;
    let response = authorized_request(client, reqwest::Method::PUT, url, config)
        .header(reqwest::header::CONTENT_TYPE, "application/json")
        .body(body)
        .send()
        .await?;
    ensure_webdav_status(response, &[200, 201, 204]).await?;
    Ok(())
}

async fn ensure_webdav_status(
    response: reqwest::Response,
    accepted: &[u16],
) -> Result<reqwest::Response, AppError> {
    let status = response.status();
    if accepted.contains(&status.as_u16()) {
        return Ok(response);
    }
    let message = response.text().await.unwrap_or_default();
    let detail = message.trim().chars().take(160).collect::<String>();
    Err(AppError::BadRequest(if detail.is_empty() {
        format!("网盘请求失败（状态码 {}）", status.as_u16())
    } else {
        format!("网盘请求失败（状态码 {}）：{}", status.as_u16(), detail)
    }))
}

fn validate_progress(progress: &LegadoBookProgress) -> Result<(), AppError> {
    if progress.name.trim().is_empty() {
        return Err(AppError::BadRequest("书名不能为空".to_string()));
    }
    if progress.dur_chapter_index < 0 || progress.dur_chapter_pos < 0 {
        return Err(AppError::BadRequest("阅读进度无效".to_string()));
    }
    Ok(())
}

fn compare_progress(left: &LegadoBookProgress, right: &LegadoBookProgress) -> std::cmp::Ordering {
    left.dur_chapter_index
        .cmp(&right.dur_chapter_index)
        .then(left.dur_chapter_pos.cmp(&right.dur_chapter_pos))
}

fn compare_sync_progress(left: &LegadoBookProgress, right: &LegadoBookProgress) -> std::cmp::Ordering {
    let left_time = normalize_progress_time(left.dur_chapter_time);
    let right_time = normalize_progress_time(right.dur_chapter_time);
    left_time
        .cmp(&right_time)
        .then_with(|| compare_progress(left, right))
}

fn normalize_progress_time(value: i64) -> i64 {
    if value > 0 && value < 1_000_000_000_000 {
        value.saturating_mul(1000)
    } else {
        value
    }
}

fn legado_progress_file_name(name: &str, author: &str) -> String {
    let normalized = format!("{}_{}", name, author)
        .chars()
        .map(|character| {
            if matches!(character, '\\' | '/' | ':' | '*' | '?' | '"' | '<' | '>' | '|') {
                '_'
            } else {
                character
            }
        })
        .collect::<String>();
    let mut encoded = String::with_capacity(normalized.len());
    for character in normalized.chars() {
        match character {
            '%' => encoded.push_str("%25"),
            ' ' => encoded.push_str("%20"),
            '"' => encoded.push_str("%22"),
            '#' => encoded.push_str("%23"),
            '&' => encoded.push_str("%26"),
            '(' => encoded.push_str("%28"),
            ')' => encoded.push_str("%29"),
            '+' => encoded.push_str("%2B"),
            ',' => encoded.push_str("%2C"),
            '/' => encoded.push_str("%2F"),
            ':' => encoded.push_str("%3A"),
            ';' => encoded.push_str("%3B"),
            '<' => encoded.push_str("%3C"),
            '=' => encoded.push_str("%3D"),
            '>' => encoded.push_str("%3E"),
            '?' => encoded.push_str("%3F"),
            '@' => encoded.push_str("%40"),
            '\\' => encoded.push_str("%5C"),
            '|' => encoded.push_str("%7C"),
            _ => encoded.push(character),
        }
    }
    format!("{encoded}.json")
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

    #[test]
    fn legado_progress_file_name_matches_android_rules() {
        assert_eq!(
            legado_progress_file_name("测试 书#1", "作者(A)"),
            "测试%20书%231_作者%28A%29.json"
        );
        assert_eq!(legado_progress_file_name("A/B", "C:D"), "A_B_C_D.json");
    }

    #[test]
    fn progress_comparison_prefers_chapter_then_position() {
        let base = LegadoBookProgress {
            name: "书".to_string(),
            author: "作者".to_string(),
            dur_chapter_index: 8,
            dur_chapter_pos: 200,
            dur_chapter_time: 1,
            dur_chapter_title: None,
        };
        assert!(compare_progress(&LegadoBookProgress { dur_chapter_index: 9, dur_chapter_pos: 0, ..base.clone() }, &base).is_gt());
        assert!(compare_progress(&LegadoBookProgress { dur_chapter_pos: 201, ..base.clone() }, &base).is_gt());
    }

    #[test]
    fn sync_comparison_prefers_newer_reading_time_even_on_an_earlier_chapter() {
        let local = LegadoBookProgress {
            name: "书".to_string(),
            author: "作者".to_string(),
            dur_chapter_index: 105,
            dur_chapter_pos: 200,
            dur_chapter_time: 1_700_000_000_000,
            dur_chapter_title: None,
        };
        let remote = LegadoBookProgress {
            dur_chapter_index: 103,
            dur_chapter_pos: 50,
            dur_chapter_time: 1_700_000_100_000,
            ..local.clone()
        };

        assert!(compare_sync_progress(&remote, &local).is_gt());
        assert!(compare_sync_progress(&local, &remote).is_lt());
    }
}
