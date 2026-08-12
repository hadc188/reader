use crate::api::AppState;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::PathBuf;
use tokio::fs;

use crate::error::error::{ApiResponse, AppError};

#[derive(Debug, Deserialize)]
pub struct DeleteFileRequest {
    pub url: Option<String>,
}

const MAX_FONT_BYTES: usize = 32 * 1024 * 1024;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CustomFontEntry {
    pub id: String,
    pub name: String,
    pub url: String,
}

#[tauri::command]
pub async fn save_user_config(
    state: tauri::State<'_, AppState>,
    req: Value,
) -> Result<ApiResponse<Value>, AppError> {
    let user_ns = "default";
    state.user_service.save_user_config(&user_ns, req).await?;
    Ok(ApiResponse::ok(Value::String("".to_string())))
}

#[tauri::command]
pub async fn get_user_config(
    state: tauri::State<'_, AppState>,
) -> Result<ApiResponse<Value>, AppError> {
    let user_ns = "default";
    let cfg = state.user_service.get_user_config(&user_ns).await?;
    Ok(ApiResponse::ok(cfg))
}

#[tauri::command]
pub async fn upload_file(
    state: tauri::State<'_, AppState>,
    file: Vec<u8>,
    file_name: String,
    file_type: Option<String>,
) -> Result<ApiResponse<Value>, AppError> {
    let user_ns = "default";
    let file_type = file_type
        .filter(|t| !t.is_empty())
        .unwrap_or_else(|| "images".to_string());
    let dir = PathBuf::from(&state.config.storage_dir)
        .join("assets")
        .join(&user_ns)
        .join(&file_type);
    fs::create_dir_all(&dir)
        .await
        .map_err(|e| AppError::Internal(e.into()))?;
    let path = dir.join(&file_name);
    fs::write(&path, &file)
        .await
        .map_err(|e| AppError::Internal(e.into()))?;
    let url = format!("/assets/{}/{}/{}", user_ns, file_type, file_name);
    Ok(ApiResponse::ok(Value::from(vec![Value::String(url)])))
}

#[tauri::command]
pub async fn delete_file(
    state: tauri::State<'_, AppState>,
    req: DeleteFileRequest,
) -> Result<ApiResponse<Value>, AppError> {
    let user_ns = "default";
    let url = req.url.unwrap_or_default();
    if url.is_empty() {
        return Ok(ApiResponse::err("请输入文件链接"));
    }
    let prefix = format!("/assets/{}/", user_ns);
    if !url.starts_with(&prefix) {
        return Ok(ApiResponse::err("文件链接错误"));
    }
    let full_path = PathBuf::from(&state.config.storage_dir).join(url.trim_start_matches('/'));
    let _ = fs::remove_file(full_path).await;
    Ok(ApiResponse::ok(Value::String("".to_string())))
}

#[tauri::command]
pub async fn list_custom_fonts(
    state: tauri::State<'_, AppState>,
) -> Result<ApiResponse<Vec<CustomFontEntry>>, AppError> {
    let dir = custom_font_dir(&state);
    fs::create_dir_all(&dir)
        .await
        .map_err(|error| AppError::Internal(error.into()))?;
    let mut fonts = Vec::new();
    let mut entries = fs::read_dir(&dir)
        .await
        .map_err(|error| AppError::Internal(error.into()))?;
    while let Some(entry) = entries
        .next_entry()
        .await
        .map_err(|error| AppError::Internal(error.into()))?
    {
        let file_name = entry.file_name().to_string_lossy().to_string();
        let Some((id, name)) = decode_custom_font_name(&file_name) else {
            continue;
        };
        fonts.push(CustomFontEntry {
            id,
            name,
            url: format!("http://reader.localhost/files?path=default/fonts/{file_name}"),
        });
    }
    fonts.sort_by(|left, right| left.name.cmp(&right.name));
    Ok(ApiResponse::ok(fonts))
}

#[tauri::command]
pub async fn upload_custom_font(
    state: tauri::State<'_, AppState>,
    file: Vec<u8>,
    file_name: String,
) -> Result<ApiResponse<CustomFontEntry>, AppError> {
    if file.is_empty() || file.len() > MAX_FONT_BYTES {
        return Err(AppError::BadRequest("字体文件为空或超过 32 MB".to_string()));
    }
    let original = PathBuf::from(&file_name);
    let extension = original
        .extension()
        .and_then(|value| value.to_str())
        .map(str::to_ascii_lowercase)
        .filter(|value| matches!(value.as_str(), "ttf" | "otf" | "woff" | "woff2"))
        .ok_or_else(|| AppError::BadRequest("仅支持 TTF、OTF、WOFF 和 WOFF2 字体".to_string()))?;
    let display_name = original
        .file_stem()
        .and_then(|value| value.to_str())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| AppError::BadRequest("字体文件名无效".to_string()))?;
    let safe_name = display_name
        .chars()
        .map(|character| if character.is_alphanumeric() || matches!(character, '-' | '_' | ' ') { character } else { '_' })
        .collect::<String>();
    let id = uuid::Uuid::new_v4().simple().to_string();
    let stored_name = format!("{id}__{safe_name}.{extension}");
    let dir = custom_font_dir(&state);
    fs::create_dir_all(&dir)
        .await
        .map_err(|error| AppError::Internal(error.into()))?;
    fs::write(dir.join(&stored_name), file)
        .await
        .map_err(|error| AppError::Internal(error.into()))?;
    Ok(ApiResponse::ok(CustomFontEntry {
        id,
        name: display_name.to_string(),
        url: format!("http://reader.localhost/files?path=default/fonts/{stored_name}"),
    }))
}

#[tauri::command]
pub async fn delete_custom_font(
    state: tauri::State<'_, AppState>,
    id: String,
) -> Result<ApiResponse<Value>, AppError> {
    if id.is_empty() || !id.chars().all(|character| character.is_ascii_hexdigit()) {
        return Err(AppError::BadRequest("字体标识无效".to_string()));
    }
    let dir = custom_font_dir(&state);
    let mut entries = match fs::read_dir(&dir).await {
        Ok(entries) => entries,
        Err(_) => return Ok(ApiResponse::ok(Value::Null)),
    };
    while let Some(entry) = entries
        .next_entry()
        .await
        .map_err(|error| AppError::Internal(error.into()))?
    {
        if entry.file_name().to_string_lossy().starts_with(&format!("{id}__")) {
            fs::remove_file(entry.path())
                .await
                .map_err(|error| AppError::Internal(error.into()))?;
            break;
        }
    }
    Ok(ApiResponse::ok(Value::Null))
}

fn custom_font_dir(state: &AppState) -> PathBuf {
    PathBuf::from(&state.config.storage_dir)
        .join("assets")
        .join("default")
        .join("fonts")
}

fn decode_custom_font_name(file_name: &str) -> Option<(String, String)> {
    let (id, tail) = file_name.split_once("__")?;
    if id.is_empty() || !id.chars().all(|character| character.is_ascii_hexdigit()) {
        return None;
    }
    let name = PathBuf::from(tail).file_stem()?.to_string_lossy().to_string();
    Some((id.to_string(), name))
}
