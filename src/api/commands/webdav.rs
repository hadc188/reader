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
    #[serde(default)]
    pub allow_upload: Option<bool>,
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
    let method = propfind_method()?;
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
    let location =
        locate_legado_progress_file(&client, &progress_dir, &req.config, &req.progress).await?;
    let remote = location.remote.clone();
    let remote_is_newer = remote
        .as_ref()
        .is_some_and(|value| compare_progress(value, &req.progress).is_gt());

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
            .is_none_or(|value| compare_progress(&req.progress, value).is_gt());
    if should_upload {
        upload_legado_progress_to_all(&client, &location, &req.config, &req.progress).await?;
    }

    Ok(ApiResponse::ok(LegadoProgressResponse {
        configured: true,
        remote: None,
        uploaded: should_upload,
    }))
}

#[tauri::command]
pub async fn upload_legado_book_progress(
    req: LegadoProgressRequest,
) -> Result<ApiResponse<LegadoProgressResponse>, AppError> {
    validate_progress(&req.progress)?;
    let client = webdav_client()?;
    let (_, progress_dir) = ensure_legado_progress_dir(&client, &req.config).await?;
    let location =
        locate_legado_progress_file(&client, &progress_dir, &req.config, &req.progress).await?;
    upload_legado_progress_to_all(&client, &location, &req.config, &req.progress).await?;

    Ok(ApiResponse::ok(LegadoProgressResponse {
        configured: true,
        remote: None,
        uploaded: true,
    }))
}

#[tauri::command]
pub async fn list_legado_webdav_backups(
    config: LegadoWebdavConfig,
) -> Result<ApiResponse<Vec<LegadoWebdavBackupEntry>>, AppError> {
    let client = webdav_client()?;
    let (directory_urls, root_url) = legado_root_webdav_urls(&config)?;
    ensure_webdav_directories(&client, &directory_urls, &config).await?;
    let method = propfind_method()?;
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

    let root_path = webdav_root_path(root_url)?;
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
                        if let Some(name) = webdav_href_file_name(&value, &root_path) {
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

/// PROPFIND 请求方法。`Method::from_bytes` 正常不会失败，这里统一成一处。
fn propfind_method() -> Result<reqwest::Method, AppError> {
    reqwest::Method::from_bytes(b"PROPFIND")
        .map_err(|error| AppError::Internal(error.into()))
}

/// 把 PROPFIND 的根 URL 归一成可前缀匹配的路径（去掉末尾斜杠）。
///
/// `href` 里既有绝对路径也有裸文件名，都要先与它对齐才能取出相对名。
fn webdav_root_path(root_url: &str) -> Result<String, AppError> {
    Ok(url::Url::parse(root_url)
        .map_err(|_| AppError::BadRequest("网盘地址格式无效".to_string()))?
        .path()
        .trim_end_matches('/')
        .to_string())
}

/// 从 PROPFIND 的 `href` 值里取出**单层**文件名（已解码）。
///
/// 返回 `None` 表示这条 href 指向目录自身或子目录，调用方应跳过。
/// 相对 href 无法与根路径对齐时按「子目录」处理（真实网盘返回的是以 `/`
/// 开头的绝对路径或裸文件名）。
fn webdav_href_file_name(href: &str, root_path: &str) -> Option<String> {
    let href = href.trim();
    let path = url::Url::parse(href)
        .map(|url| url.path().to_string())
        .unwrap_or_else(|_| href.to_string());
    let relative = path.strip_prefix(root_path).unwrap_or(&path).trim_matches('/');
    if relative.is_empty() || relative.contains('/') {
        return None;
    }
    Some(
        urlencoding::decode(relative)
            .unwrap_or_else(|_| relative.into())
            .into_owned(),
    )
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

/// 这本书在云端的进度文件位置。
///
/// 两端 `author` 字段不一致时会算出不同的文件名，所以上传目标是**一组**
/// URL：把本书在云端出现过的每个文件名都写一遍，任何一端都能读到。
struct LegadoProgressLocation {
    upload_urls: Vec<String>,
    /// 云端已存在的、属于本书且进度最靠前的那份。
    remote: Option<LegadoBookProgress>,
}

/// 把进度写入全部目标 URL。
///
/// 单个目标失败不中止其余写入（某个文件名可能恰好没权限），
/// 但全部失败时必须把错误报出去，否则会静默地「同步成功却没写进去」。
async fn upload_legado_progress_to_all(
    client: &reqwest::Client,
    location: &LegadoProgressLocation,
    config: &LegadoWebdavConfig,
    progress: &LegadoBookProgress,
) -> Result<(), AppError> {
    let mut uploaded_any = false;
    let mut last_error = None;
    for url in &location.upload_urls {
        match upload_legado_progress(client, url, config, progress).await {
            Ok(()) => uploaded_any = true,
            Err(error) => {
                tracing::warn!("写入云端阅读进度失败，跳过 {url}: {error:?}");
                last_error = Some(error);
            }
        }
    }
    match (uploaded_any, last_error) {
        (false, Some(error)) => Err(error),
        _ => Ok(()),
    }
}

/// 定位这本书在云端的进度文件。
///
/// 两端 `author` 字段不一致时（手机端形如 `作者：辰东`，桌面端为 `辰东`），
/// 各自算出的文件名不同，只按精确拼名读写会互相看不见。这里列举目录、
/// 按书名找回两端各自写入的那份，选出进度最靠前的作为远端进度，
/// 并把这些文件名全部作为上传目标，让两端都能读到、真正收敛。
async fn locate_legado_progress_file(
    client: &reqwest::Client,
    progress_dir: &str,
    config: &LegadoWebdavConfig,
    progress: &LegadoBookProgress,
) -> Result<LegadoProgressLocation, AppError> {
    let default_name = legado_progress_file_name(&progress.name, &progress.author);
    let default_url = format!("{}{}", progress_dir, default_name);

    // 目录列举返回的是解码后的明文文件名，这里把默认命名也解码成同一表示，
    // 保证下面的 URL 编码只发生一次（否则「%20」会被二次编码成「%2520」）。
    let default_plain = urlencoding::decode(&default_name)
        .map(|value| value.into_owned())
        .unwrap_or_else(|_| default_name.clone());

    // 目录列举不可用时退回默认命名，保持改动前的行为，不让同步整体失败。
    let listed = match list_legado_progress_files(client, progress_dir, config).await {
        Ok(names) => names,
        Err(error) => {
            // 网盘不支持 PROPFIND 等情况会走到这里：退化为改动前的单文件模式。
            tracing::warn!("列举云端阅读进度目录失败，退回默认命名: {error:?}");
            Vec::new()
        }
    };

    // 同一文件可能以明文名和编码名两种形态出现，按明文名去重避免重复读写。
    let candidates = progress_candidates(&listed, &default_plain);

    // 候选只有默认命名时（目录列举不可用/为空/没有本书的文件）等价于改动前的
    // 单文件模式，读取失败要如实上报；有多候选时才容忍单点失败（见下面）。
    let sole_candidate = candidates.len() <= 1;

    // 逐个读取候选文件，校验确实是同一本书，再按「章节索引 → 章节内位置」
    // 选出进度最靠前的那份（与前端比较口径一致，不看 durChapterTime）。
    let mut best: Option<(LegadoBookProgress, String)> = None;
    let mut known: Vec<String> = Vec::new();
    for file_name in candidates {
        if !progress_file_matches_book(&file_name, &progress.name) {
            continue;
        }
        // 文件名里的空格、`#` 等必须编码，否则 URL 会把它们当成分隔符或片段。
        let file_url = progress_file_url(progress_dir, &file_name, &default_plain, &default_url);
        // 单个候选坏了（格式无效/过大）只跳过它，不能让整次同步失败：
        // 进度目录里可能有别的程序写坏的同名文件，改动前只看自己那一个文件，
        // 没有这个失败面。
        // 但只有默认命名一个候选时等价于改动前的单文件模式，此时要把错误如实
        // 上报 —— 否则「云端文件损坏」会被静默当成「云端没有」，直接用本地
        // 覆盖并报成功。
        let remote = match download_legado_progress(client, &file_url, config).await {
            Ok(remote) => remote,
            Err(error) if !sole_candidate => {
                tracing::warn!("读取云端阅读进度失败，跳过 {file_name}: {error:?}");
                continue;
            }
            Err(error) => return Err(error),
        };
        let Some(remote) = remote else {
            continue;
        };
        if !progress_matches_book(&remote, progress) {
            continue;
        }
        known.push(file_name.clone());
        let is_newer = best
            .as_ref()
            .is_none_or(|(current, _)| compare_progress(&remote, current).is_gt());
        if is_newer {
            best = Some((remote, file_name));
        }
    }

    // 本端命名必须纳入目标（云端什么都没有时它就是唯一目标）：
    // 否则本地进度反超后写回的是对方的名字，本端下次仍要靠列举才找得回来。
    if !known.contains(&default_plain) {
        known.push(default_plain.clone());
    }
    let upload_urls = known
        .into_iter()
        .map(|name| progress_file_url(progress_dir, &name, &default_plain, &default_url))
        .collect();

    Ok(LegadoProgressLocation {
        upload_urls,
        remote: best.map(|(remote, _)| remote),
    })
}

/// 汇总候选明文文件名：目录列举结果 + 本端默认命名，按明文去重。
///
/// 两个入参**都已经是明文**：`listed` 由 `parse_webdav_file_names` 解码后返回，
/// `default_plain` 由调用方预先解好。这里绝不能再解码 —— 书名里含「`%` + 两个
/// hex」这类字面文本时（如「折扣%20」在磁盘上是 `折扣%2520_…`），多解一次会得到
/// 另一个字符串，该候选随即被 `progress_file_matches_book` 筛掉，症状是
/// 「对方写下的进度读不到」。
fn progress_candidates(listed: &[String], default_plain: &str) -> Vec<String> {
    let mut candidates: Vec<String> = Vec::new();
    for plain in listed.iter().map(String::as_str).chain(std::iter::once(default_plain)) {
        if !candidates.iter().any(|existing| existing == plain) {
            candidates.push(plain.to_string());
        }
    }
    candidates
}

/// 把明文文件名拼成 WebDAV URL（编码恰好一次）。
///
/// 默认命名沿用改动前的编码形式，其余按字节全编码，
/// 避免 `#`、空格、`%` 被 URL 解析截断或被二次编码。
fn progress_file_url(
    progress_dir: &str,
    file_name: &str,
    default_plain: &str,
    default_url: &str,
) -> String {
    if file_name == default_plain {
        default_url.to_string()
    } else {
        format!("{}{}", progress_dir, encode_webdav_filename(file_name))
    }
}

/// 列举 `bookProgress` 目录下的进度文件名（已解码，只含文件名）。
async fn list_legado_progress_files(
    client: &reqwest::Client,
    progress_dir: &str,
    config: &LegadoWebdavConfig,
) -> Result<Vec<String>, AppError> {
    let method = propfind_method()?;
    let response = authorized_request(client, method, progress_dir, config)
        .header("Depth", "1")
        .header(reqwest::header::CONTENT_TYPE, "application/xml")
        .body("<?xml version=\"1.0\" encoding=\"utf-8\"?><d:propfind xmlns:d=\"DAV:\"><d:prop><d:resourcetype/></d:prop></d:propfind>")
        .send()
        .await?;
    let response = ensure_webdav_status(response, &[200, 207, 404]).await?;
    if response.status() == reqwest::StatusCode::NOT_FOUND {
        return Ok(Vec::new());
    }
    let body = response.bytes().await?;
    parse_webdav_file_names(&body, progress_dir)
}

/// 从 PROPFIND 响应里提取单层文件名。
fn parse_webdav_file_names(body: &[u8], root_url: &str) -> Result<Vec<String>, AppError> {
    use quick_xml::events::Event;
    use quick_xml::Reader;

    let root_path = webdav_root_path(root_url)?;
    let mut reader = Reader::from_reader(Cursor::new(body));
    reader.trim_text(true);
    let mut buffer = Vec::new();
    let mut in_href = false;
    let mut names = Vec::new();
    loop {
        match reader.read_event_into(&mut buffer) {
            Ok(Event::Start(event)) => {
                in_href = String::from_utf8_lossy(event.local_name().as_ref())
                    .eq_ignore_ascii_case("href");
            }
            Ok(Event::Text(text)) if in_href => {
                let value = text
                    .unescape()
                    .map_err(|error| AppError::BadRequest(error.to_string()))?;
                if let Some(name) = webdav_href_file_name(&value, &root_path) {
                    names.push(name);
                }
            }
            Ok(Event::End(event)) => {
                if String::from_utf8_lossy(event.local_name().as_ref())
                    .eq_ignore_ascii_case("href")
                {
                    in_href = false;
                }
            }
            Ok(Event::Eof) => break,
            Err(error) => return Err(AppError::BadRequest(format!("网盘目录格式无效：{}", error))),
            _ => {}
        }
        buffer.clear();
    }
    Ok(names)
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

fn legado_progress_file_name(name: &str, author: &str) -> String {
    let normalized = sanitize_progress_stem(&format!("{}_{}", name, author));
    let encoded = encode_progress_stem(&normalized);
    format!("{encoded}.json")
}

/// 把 `书名_作者` 中的文件系统保留字符换成下划线。
fn sanitize_progress_stem(value: &str) -> String {
    value
        .chars()
        .map(|character| {
            if matches!(character, '\\' | '/' | ':' | '*' | '?' | '"' | '<' | '>' | '|') {
                '_'
            } else {
                character
            }
        })
        .collect()
}

/// 归一化作者名：手机端可能把「作者：」标签一起写进 author 字段
/// （例如 `作者：辰东` 与桌面端的 `辰东` 其实是同一本书），
/// 去掉前缀与分隔符后再比较，避免两端算出不同的进度文件名。
fn normalize_author(author: &str) -> String {
    let trimmed = author.trim();
    let normalized = trimmed
        .strip_prefix("作者")
        .map(|rest| rest.trim_start_matches(['：', ':', '、', '·', '-', '—', ' ']))
        // 作者名本身就叫「作者」时，剥掉前缀会得到空串，此时保留原值。
        .filter(|rest| !rest.is_empty())
        .unwrap_or(trimmed)
        .trim();
    normalized.to_string()
}

/// 判断云端进度文件是否属于本地这本书（书名与作者归一化后都要一致）。
fn progress_matches_book(remote: &LegadoBookProgress, local: &LegadoBookProgress) -> bool {
    remote.name.trim() == local.name.trim()
        && normalize_author(&remote.author) == normalize_author(&local.author)
}

/// 判断目录里的某个文件名是否**可能**是这本书的进度文件。
///
/// 只做「书名前缀 + .json」的宽松筛选，最终以文件内容
/// （`progress_matches_book`）为准。作者部分不能在文件名层面判断：
/// 另一端的 author 串无法预测（可能是 `作者：辰东`，也可能因文件系统
/// 保留字符被 sanitize 成 `作者_辰东`），而 `legado_progress_file_name`
/// 又是对 `书名_作者` 整体做 sanitize 的。若在文件名层面比对作者，
/// 作者名含 `\ / : * ? " < > |` 的书会连自己刚写下的文件都匹配不上。
fn progress_file_matches_book(file_name: &str, name: &str) -> bool {
    if progress_stem_matches_book(file_name, name) {
        return true;
    }
    // 不同网盘返回的 href 编码程度不一，解码后再比一次。
    urlencoding::decode(file_name).is_ok_and(|decoded| progress_stem_matches_book(&decoded, name))
}

fn progress_stem_matches_book(file_name: &str, name: &str) -> bool {
    let Some(stem) = file_name.strip_suffix(".json") else {
        return false;
    };
    // sanitize 是逐字符映射，所以 `sanitize(书名_作者)` 必然以
    // `sanitize(书名) + "_"` 开头 —— 自己写下的文件一定能通过这道筛选。
    //
    // 比较时忽略空白：两端书名可能只有首尾空白不同（内容校验用 `trim`，
    // 认为它们是同一本书），而文件名用的是各自原样的书名。若只按一边的
    // 空白形态比对，本端「遮天 」与对端「遮天」互相看不见对方的文件。
    // 放宽只会多一次 GET，最终仍由 `progress_matches_book` 的内容校验定性。
    let strip_ws = |value: &str| -> String {
        value.chars().filter(|character| !character.is_whitespace()).collect()
    };
    strip_ws(stem).starts_with(&format!("{}_", strip_ws(&sanitize_progress_stem(name.trim()))))
}

fn encode_progress_stem(normalized: &str) -> String {
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
    encoded
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
    fn normalize_author_strips_mobile_label_prefix() {
        // 手机端把「作者：」标签写进了 author 字段，桌面端是干净的。
        assert_eq!(normalize_author("作者：辰东"), "辰东");
        assert_eq!(normalize_author("作者:辰东"), "辰东");
        assert_eq!(normalize_author("  辰东  "), "辰东");
        assert_eq!(normalize_author("辰东"), "辰东");
        // 作者名本身就以「作者」开头时不能被误伤。
        assert_eq!(normalize_author("作者"), "作者");
        // 只有标签、没有真名时不能塌成空串，否则会与「确实没有作者」的书串味。
        assert_eq!(normalize_author("作者："), "作者：");
        assert_ne!(normalize_author("作者："), normalize_author(""));
    }

    #[test]
    fn progress_file_matches_book_screens_by_name_prefix() {
        // 手机端写下的文件名（作者带标签前缀）。
        assert!(progress_file_matches_book("遮天_作者：辰东.json", "遮天"));
        // 桌面端写下的文件名。
        assert!(progress_file_matches_book("遮天_辰东.json", "遮天"));
        // 按 Android 规则百分号编码后的文件名也要能命中。
        assert!(progress_file_matches_book("测试%20书_作者%28A%29.json", "测试 书"));
        // 解码后的文件名同样命中（不同网盘 href 编码程度不一）。
        assert!(progress_file_matches_book("测试 书_作者(A).json", "测试 书"));

        // 不同书不能命中。
        assert!(!progress_file_matches_book("完美世界_辰东.json", "遮天"));
        // 非 json 文件忽略。
        assert!(!progress_file_matches_book("遮天_辰东.txt", "遮天"));

        // 回归：文件名生成与文件名匹配必须自洽 —— 凡是自己写下的文件名，
        // 都必须能通过这道筛选。曾因在文件名层面比对作者而失败：
        // 生成时对 `书名_作者` 整体 sanitize（`:` → `_`），
        // 匹配时却用裸作者名比较，作者含保留字符的书连自己的文件都读不到。
        for (name, author) in [
            ("A/B", "C:D"),
            ("遮天", "作者：辰东"),
            ("遮天", "作者:辰东"),
            ("测试 书#1", "作者(A)"),
            ("书?名", "作*者"),
        ] {
            let file_name = legado_progress_file_name(name, author);
            let plain = urlencoding::decode(&file_name).unwrap().into_owned();
            assert!(
                progress_file_matches_book(&file_name, name),
                "自产文件名匹配失败: {file_name}"
            );
            assert!(
                progress_file_matches_book(&plain, name),
                "自产明文文件名匹配失败: {plain}"
            );
        }
    }

    #[test]
    fn parse_webdav_file_names_extracts_single_level_files() {
        let dir = "https://dav.example.com/dav/legado/bookProgress/";
        // 真实 WebDAV 响应：含自身条目、命名空间前缀、编码名与明文名混合。
        let body = br#"<?xml version="1.0" encoding="utf-8"?>
<d:multistatus xmlns:d="DAV:">
  <d:response>
    <d:href>/dav/legado/bookProgress/</d:href>
    <d:propstat><d:prop><d:resourcetype><d:collection/></d:resourcetype></d:prop></d:propstat>
  </d:response>
  <d:response>
    <d:href>/dav/legado/bookProgress/%E9%81%AE%E5%A4%A9_%E8%BE%B0%E4%B8%9C.json</d:href>
    <d:propstat><d:prop><d:resourcetype/></d:prop></d:propstat>
  </d:response>
  <d:response>
    <d:href>/dav/legado/bookProgress/%E9%81%AE%E5%A4%A9_%E4%BD%9C%E8%80%85%EF%BC%9A%E8%BE%B0%E4%B8%9C.json</d:href>
    <d:propstat><d:prop><d:resourcetype/></d:prop></d:propstat>
  </d:response>
</d:multistatus>"#;
        let names = parse_webdav_file_names(body, dir).unwrap();
        // 目录自身条目被排除；文件名解码成明文；两个命名都收到。
        assert_eq!(names, vec!["遮天_辰东.json", "遮天_作者：辰东.json"]);
    }

    #[test]
    fn parse_webdav_file_names_handles_relative_href_and_subdirs() {
        let dir = "https://dav.example.com/dav/legado/bookProgress/";
        let body = br#"<?xml version="1.0"?>
<multistatus xmlns="DAV:">
  <response><href>%E9%81%AE%E5%A4%A9_%E8%BE%B0%E4%B8%9C.json</href></response>
  <response><href>/dav/legado/bookProgress/nested/other.json</href></response>
  <response><href>/dav/legado/bookProgress/plain.json</href></response>
</multistatus>"#;
        let names = parse_webdav_file_names(body, dir).unwrap();
        // 不带命名空间前缀、裸文件名(相对 href)、绝对路径都要正确处理；
        // 子目录里的文件只取一层，不递归。
        assert_eq!(names, vec!["遮天_辰东.json", "plain.json"]);
    }

    #[test]
    fn parse_webdav_file_names_skips_href_with_directory_prefix() {
        let dir = "https://dav.example.com/dav/legado/bookProgress/";
        // 含目录成分的相对 href 无法与根路径对齐，按「子目录」丢弃 ——
        // 与既有的 `parse_webdav_backup_entries` 行为一致(真实网盘返回的是
        // 以 `/` 开头的绝对路径或裸文件名，此形态不作为支持面)。
        let body = br#"<multistatus xmlns="DAV:"><response><href>bookProgress/a.json</href></response></multistatus>"#;
        assert!(parse_webdav_file_names(body, dir).unwrap().is_empty());
    }

    #[test]
    fn parse_webdav_file_names_rejects_broken_xml() {
        let dir = "https://dav.example.com/dav/legado/bookProgress/";
        // 纯文本不是 XML 错误 —— quick_xml 把它当文本事件，取不到 href 即空列表。
        let names = parse_webdav_file_names(b"not xml at all", dir).unwrap();
        assert!(names.is_empty());
        // 标签闭合顺序错误才走报错分支（实测：未闭合标签不报错，属 EOF 收尾）。
        assert!(parse_webdav_file_names(b"<a><b></a></b>", dir).is_err());
        // 根路径无法解析时要报错，而不是静默返回空列表。
        assert!(parse_webdav_file_names(b"<multistatus/>", "not a url").is_err());
    }

    #[test]
    fn progress_file_matches_book_accepts_trimmed_and_untrimmed_names() {
        // 本端书名带尾随空格：自己写下的文件名用的是未 trim 的书名，
        // 对端（手机端）用干净书名写，两种都必须能筛出来。
        assert!(progress_file_matches_book("遮天 _辰东.json", "遮天 "));
        assert!(progress_file_matches_book("遮天_辰东.json", "遮天 "));
        // 反向：本端干净、对端带空格。
        assert!(progress_file_matches_book("遮天 _辰东.json", "遮天"));
        // 只有前导空格同理。
        assert!(progress_file_matches_book(" 遮天_辰东.json", "  遮天"));
    }

    #[test]
    fn progress_file_matches_book_prefix_mismatch_is_rejected() {
        // 书名含 `_` 时不会失配（sanitize 是逐字符映射）。
        assert!(progress_file_matches_book("A_B_辰东.json", "A_B"));
        // 前缀相同的另一本书会通过"可能命中"筛选，但由内容校验兜底拒绝 ——
        // 这里锁定筛选层的行为，避免以后误以为它能独立判定归属。
        assert!(progress_file_matches_book("A_B_辰东.json", "A"));
        // 完全不同的书不命中。
        assert!(!progress_file_matches_book("完美世界_辰东.json", "遮天"));
    }

    #[test]
    fn progress_candidate_url_is_encoded_exactly_once() {
        // 目录列举给出解码后的明文名，而默认命名是编码名；两者必须先统一成
        // 明文、再编码一次放进 URL。否则书名含空格时会出现「%2520」，
        // 指向一个不存在的文件，等于同步失效。
        let encoded = legado_progress_file_name("测试 书", "作者(A)");
        assert_eq!(encoded, "测试%20书_作者%28A%29.json");

        let plain = urlencoding::decode(&encoded).unwrap().into_owned();
        assert_eq!(plain, "测试 书_作者(A).json");
        assert!(progress_file_matches_book(&plain, "测试 书"));

        // 明文名编码一次即可；解码回来仍是同一个文件名。
        let url = encode_webdav_filename(&plain);
        assert!(!url.contains("%25"));
        assert_eq!(urlencoding::decode(&url).unwrap().into_owned(), plain);

        // 反面：对已编码的名字再编码一次就会得到「%2520」。
        assert!(encode_webdav_filename(&encoded).contains("%2520"));
    }

    #[test]
    fn parse_webdav_backup_entries_reads_size_and_date() {
        let root = "https://dav.example.com/dav/legado/";
        let body = br#"<?xml version="1.0" encoding="utf-8"?>
<d:multistatus xmlns:d="DAV:">
  <d:response>
    <d:href>/dav/legado/</d:href>
    <d:propstat><d:prop><d:resourcetype><d:collection/></d:resourcetype></d:prop></d:propstat>
  </d:response>
  <d:response>
    <d:href>/dav/legado/backup-2026-10-02.zip</d:href>
    <d:propstat><d:prop>
      <d:getcontentlength>2048</d:getcontentlength>
      <d:getlastmodified>Fri, 02 Oct 2026 10:00:00 GMT</d:getlastmodified>
    </d:prop></d:propstat>
  </d:response>
</d:multistatus>"#;
        let entries = parse_webdav_backup_entries(body, root).unwrap();
        // 目录自身条目被排除；zip 条目解析出大小与时间。
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].name, "backup-2026-10-02.zip");
        assert_eq!(entries[0].size, 2048);
        assert_eq!(
            entries[0].last_modified,
            chrono::DateTime::parse_from_rfc2822("Fri, 02 Oct 2026 10:00:00 GMT")
                .unwrap()
                .timestamp_millis()
        );
    }

    #[test]
    fn parse_webdav_backup_entries_skips_non_zip_and_subdirs() {
        let root = "https://dav.example.com/dav/legado/";
        let body = br#"<multistatus xmlns="DAV:">
  <response><href>/dav/legado/backup-ok.zip</href></response>
  <response><href>/dav/legado/other.zip</href></response>
  <response><href>/dav/legado/nested/backup-deep.zip</href></response>
  <response><href>/dav/legado/backup-readme.txt</href></response>
</multistatus>"#;
        let entries = parse_webdav_backup_entries(body, root).unwrap();
        // 只收「backup 开头 + .zip 结尾」且位于本层的条目。
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].name, "backup-ok.zip");
        // 未提供 size/date 时取默认值。
        assert_eq!(entries[0].size, 0);
        assert_eq!(entries[0].last_modified, 0);
    }

    #[test]
    fn parse_webdav_backup_entries_decodes_and_rejects_broken_root() {
        let root = "https://dav.example.com/dav/legado/";
        // 编码过的 zip 名要解码后再交给筛选。
        let body = br#"<multistatus xmlns="DAV:"><response><href>/dav/legado/backup%20a.zip</href></response></multistatus>"#;
        let entries = parse_webdav_backup_entries(body, root).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].name, "backup a.zip");

        // 根路径无法解析时报错，而不是静默返回空列表。
        assert!(parse_webdav_backup_entries(b"<multistatus/>", "not a url").is_err());
        // 标签闭合顺序错误走报错分支。
        assert!(parse_webdav_backup_entries(b"<a><b></a></b>", root).is_err());
    }

    #[test]
    fn webdav_href_file_name_normalizes_paths() {
        let root_path = "/dav/legado/";
        // 末尾斜杠由 webdav_root_path 去掉，这里按它的输出形态调用。
        let root_path = root_path.trim_end_matches('/');
        assert_eq!(
            webdav_href_file_name("/dav/legado/a.json", root_path).as_deref(),
            Some("a.json")
        );
        // 裸文件名（相对 href）可用。
        assert_eq!(
            webdav_href_file_name("b.json", root_path).as_deref(),
            Some("b.json")
        );
        // 编码名解码一次（%20 → 空格），不重复解码。
        assert_eq!(
            webdav_href_file_name("/dav/legado/%E6%B5%8B%E8%AF%95%20a.json", root_path).as_deref(),
            Some("测试 a.json")
        );
        // 字面 `%` 只解一次：磁盘上的 `%2520` 还原成 `%20`。
        assert_eq!(
            webdav_href_file_name("/dav/legado/x%2520y.json", root_path).as_deref(),
            Some("x%20y.json")
        );
        // 目录自身、子目录、空名都要跳过。
        assert_eq!(webdav_href_file_name("/dav/legado/", root_path), None);
        assert_eq!(webdav_href_file_name("/dav/legado/sub/a.json", root_path), None);
        assert_eq!(webdav_href_file_name("", root_path), None);
    }

    #[test]
    fn progress_candidate_name_is_decoded_exactly_once() {
        // 回归：候选名只能解码一次。`default_plain` 进入候选循环前已是明文，
        // 若与列举结果一起再解一次，书名里含「% + 两个 hex」的字面文本会被
        // 改写成另一个名字 —— 该候选随即被 `progress_file_matches_book` 筛掉，
        // 症状是「对方写下的进度读不到」。这里直接调用生产函数，确保覆盖的是
        // 真实路径而不是 `urlencoding` 的纯函数行为。
        let name = "折扣%20";
        let encoded = legado_progress_file_name(name, "辰东");
        // 字面 `%` 被编码成 `%25`，所以磁盘上是 `%2520`。
        assert_eq!(encoded, "折扣%2520_辰东.json");

        let plain = urlencoding::decode(&encoded).unwrap().into_owned();
        assert_eq!(plain, "折扣%20_辰东.json");

        // 列举返回的正是这个明文名：候选里必须原样保留它。
        let candidates = progress_candidates(&[plain.clone()], &plain);
        assert_eq!(candidates, vec![plain.clone()]);
        // 候选仍能匹配本书（漏匹配就是被修掉的 bug 的表现）。
        assert!(progress_file_matches_book(&candidates[0], name));

        // 若把明文再解一次就会得到别的字符串，且不再匹配 —— 锁定这一点，
        // 防止有人「顺手」再解一遍。
        let twice = urlencoding::decode(&plain).unwrap().into_owned();
        assert_eq!(twice, "折扣 _辰东.json");
        assert!(!progress_file_matches_book(&twice, name));
    }

    #[test]
    fn progress_candidates_merge_listing_and_default_name() {
        // 两个入参都已是明文（`parse_webdav_file_names` 内部已解码 href），
        // 按明文去重，且本端命名必须在列。
        let listed = vec![
            "遮天_辰东.json".to_string(),
            "遮天_作者：辰东.json".to_string(),
        ];
        assert_eq!(
            progress_candidates(&listed, "遮天_辰东.json"),
            vec!["遮天_辰东.json", "遮天_作者：辰东.json"],
            "默认命名与列举结果重复时应只留一份"
        );

        // 列举为空（PROPFIND 不可用/目录为空）：只剩默认命名。
        assert_eq!(
            progress_candidates(&[], "遮天_辰东.json"),
            vec!["遮天_辰东.json"]
        );
    }

    #[test]
    fn progress_matches_book_requires_name_and_normalized_author() {
        let local = LegadoBookProgress {
            name: "遮天".to_string(),
            author: "辰东".to_string(),
            dur_chapter_index: 1,
            dur_chapter_pos: 275,
            dur_chapter_time: 1_791_026_929_669,
            dur_chapter_title: Some("第二章 素问".to_string()),
        };
        // 手机端那份（作者带标签前缀）应判定为同一本书。
        let remote = LegadoBookProgress {
            author: "作者：辰东".to_string(),
            dur_chapter_index: 9,
            dur_chapter_title: Some("第十章 苍茫大地".to_string()),
            ..local.clone()
        };
        assert!(progress_matches_book(&remote, &local));

        // 书名或作者不同就是另一本书。
        assert!(!progress_matches_book(
            &LegadoBookProgress { name: "完美世界".to_string(), ..remote.clone() },
            &local
        ));
        assert!(!progress_matches_book(
            &LegadoBookProgress { author: "辰南".to_string(), ..remote.clone() },
            &local
        ));
    }

    #[test]
    fn progress_upload_urls_cover_both_endpoint_namings() {
        // 用户实际场景：手机端算出的名字带「作者：」标签，桌面端不带。
        let dir = "https://dav.example.com/dav/legado/bookProgress/";
        let default_name = legado_progress_file_name("遮天", "辰东");
        let default_url = format!("{}{}", dir, default_name);
        let default_plain = urlencoding::decode(&default_name).unwrap().into_owned();
        assert_eq!(default_plain, "遮天_辰东.json");

        // 本端命名走原编码形式，对方命名按字节全编码。
        assert_eq!(
            progress_file_url(dir, &default_plain, &default_plain, &default_url),
            default_url
        );
        let other = progress_file_url(dir, "遮天_作者：辰东.json", &default_plain, &default_url);
        assert!(other.starts_with(dir));
        assert!(other.ends_with(".json"));
        // 编码恰好一次：解码回来就是原名，且不含被二次编码的 `%25`。
        assert!(!other.contains("%25"));
        assert_eq!(
            urlencoding::decode(other.trim_start_matches(dir)).unwrap().into_owned(),
            "遮天_作者：辰东.json"
        );
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
    fn progress_comparison_ignores_reading_time() {
        let local = LegadoBookProgress {
            name: "书".to_string(),
            author: "作者".to_string(),
            dur_chapter_index: 46,
            dur_chapter_pos: 0,
            dur_chapter_time: 1_700_000_100_000,
            dur_chapter_title: None,
        };
        let remote = LegadoBookProgress {
            dur_chapter_index: 47,
            dur_chapter_pos: 2_641,
            dur_chapter_time: 1_700_000_000_000,
            ..local.clone()
        };

        assert!(compare_progress(&remote, &local).is_gt());
        assert!(compare_progress(&local, &remote).is_lt());
    }
}
