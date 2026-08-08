use crate::api::AppState;
use crate::error::error::{ApiResponse, AppError};
use crate::util::time::now_ts;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::PathBuf;
use tokio::fs;

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