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
            url: format!(
                "{}/files?path=default/fonts/{file_name}",
                crate::api::protocol::reader_scheme_origin()
            ),
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
        url: format!(
            "{}/files?path=default/fonts/{stored_name}",
            crate::api::protocol::reader_scheme_origin()
        ),
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

/* ─── 书籍自定义封面 ───
 *  与自定义字体同一套思路: 真实文件落在 storage/assets/default/covers/,
 *  书架里只保存 reader://files?path=... 的 URL。 */

const MAX_BOOK_COVER_BYTES: usize = 8 * 1024 * 1024;
const BOOK_COVER_CATEGORY: &str = "covers";

/// 允许的封面扩展名。与 protocol.rs 的 mime_from_ext 对齐,
/// 否则存下来浏览器会拿到 application/octet-stream 而显示不出来。
const BOOK_COVER_EXTENSIONS: [&str; 6] = ["png", "jpg", "jpeg", "gif", "webp", "avif"];

fn book_cover_dir(state: &AppState) -> PathBuf {
    PathBuf::from(&state.config.storage_dir)
        .join("assets")
        .join("default")
        .join(BOOK_COVER_CATEGORY)
}

fn book_cover_url(file_name: &str) -> String {
    format!(
        "{}/files?path=default/{BOOK_COVER_CATEGORY}/{file_name}",
        crate::api::protocol::reader_scheme_origin()
    )
}

/// 从 URL 里取出本应用 covers 目录下的文件名; 不是本应用存的封面返回 None。
///
/// 这个校验是必需的: `customCoverUrl` 也可能来自书源(远程图片地址), 前端把它
/// 交回来删除时必须落空, 绝不能顺着任意 URL 去删磁盘文件。
fn book_cover_file_name_from_url(url: &str) -> Option<String> {
    let prefix = format!(
        "{}/files?path=default/{BOOK_COVER_CATEGORY}/",
        crate::api::protocol::reader_scheme_origin()
    );
    let file_name = url.strip_prefix(&prefix)?;
    // 只收 upload_book_cover 生成的形式: 纯 hex 文件名 + 白名单扩展名。
    // 这样连 "../"、"a/b.png" 这类都自然被拒(不是 hex)。
    let (stem, extension) = file_name.rsplit_once('.')?;
    if stem.is_empty() || !stem.chars().all(|character| character.is_ascii_hexdigit()) {
        return None;
    }
    if !BOOK_COVER_EXTENSIONS.contains(&extension.to_ascii_lowercase().as_str()) {
        return None;
    }
    Some(file_name.to_string())
}

/// URL → 本机文件路径。只接受本应用 covers 目录下的 URL。
fn book_cover_path_from_url(state: &AppState, url: &str) -> Option<PathBuf> {
    book_cover_file_name_from_url(url).map(|name| book_cover_dir(state).join(name))
}

#[tauri::command]
pub async fn upload_book_cover(
    state: tauri::State<'_, AppState>,
    file: Vec<u8>,
    file_name: String,
) -> Result<ApiResponse<String>, AppError> {
    if file.is_empty() || file.len() > MAX_BOOK_COVER_BYTES {
        return Err(AppError::BadRequest("封面图片为空或超过 8 MB".to_string()));
    }
    let extension = PathBuf::from(&file_name)
        .extension()
        .and_then(|value| value.to_str())
        .map(str::to_ascii_lowercase)
        .filter(|value| BOOK_COVER_EXTENSIONS.contains(&value.as_str()))
        .ok_or_else(|| {
            AppError::BadRequest("仅支持 PNG、JPG、GIF、WebP 和 AVIF 图片".to_string())
        })?;
    // 文件名不含任何用户输入(原文件名可能带路径分隔符或怪字符), 只用 UUID。
    let stored_name = format!("{}.{extension}", uuid::Uuid::new_v4().simple());
    let dir = book_cover_dir(&state);
    fs::create_dir_all(&dir)
        .await
        .map_err(|error| AppError::Internal(error.into()))?;
    fs::write(dir.join(&stored_name), file)
        .await
        .map_err(|error| AppError::Internal(error.into()))?;
    Ok(ApiResponse::ok(book_cover_url(&stored_name)))
}

#[tauri::command]
pub async fn delete_book_cover(
    state: tauri::State<'_, AppState>,
    url: String,
) -> Result<ApiResponse<Value>, AppError> {
    let Some(path) = book_cover_path_from_url(&state, &url) else {
        // 不是本应用存的封面(例如书源自带的远程图) —— 没有文件要删, 视为成功。
        return Ok(ApiResponse::ok(Value::Null));
    };
    // 不存在也算成功: 重复删除/换封面失败后重试都不该报错。
    let _ = fs::remove_file(path).await;
    Ok(ApiResponse::ok(Value::Null))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn valid_cover_url(name: &str) -> String {
        format!(
            "{}/files?path=default/{BOOK_COVER_CATEGORY}/{name}",
            crate::api::protocol::reader_scheme_origin()
        )
    }

    #[test]
    fn accepts_a_url_this_module_generated() {
        let name = "a1b2c3d4e5f6.png";
        let parsed = book_cover_file_name_from_url(&valid_cover_url(name));
        assert_eq!(parsed.as_deref(), Some(name));
    }

    /// 环回: 生成 URL 与解析 URL 必须互为一对。
    /// 上面那条用的是手工拼的 valid_cover_url, 一旦目录名/查询格式漂移,
    /// 两条格式各写一遍会让测试继续全绿、线上却再也删不掉旧封面。
    ///
    /// 样本必须是**真实会生成的文件名**: 上传侧用的是 UUID(纯 hex), 解析侧也只
    /// 收纯 hex —— 这正是安全边界, 不能为了凑测试放宽。
    #[test]
    fn generated_url_round_trips_through_the_parser() {
        for name in ["0123456789abcdef.png", "deadbeef.jpg", "a1b2c3.webp"] {
            let url = book_cover_url(name);
            assert_eq!(
                book_cover_file_name_from_url(&url).as_deref(),
                Some(name),
                "环回失败: {url}"
            );
        }
    }

    /// 与上传侧保持一致: 文件名确实来自 UUID, 因此纯 hex 一定成立。
    #[test]
    fn a_uuid_based_name_is_accepted_by_the_parser() {
        let stored = format!("{}.png", uuid::Uuid::new_v4().simple());
        assert_eq!(
            book_cover_file_name_from_url(&book_cover_url(&stored)).as_deref(),
            Some(stored.as_str())
        );
    }

    #[test]
    fn accepts_every_supported_extension() {
        for extension in BOOK_COVER_EXTENSIONS {
            let name = format!("deadbeef.{extension}");
            assert!(
                book_cover_file_name_from_url(&valid_cover_url(&name)).is_some(),
                "{extension} 应被接受"
            );
        }
    }

    #[test]
    fn is_case_insensitive_about_the_extension() {
        assert!(book_cover_file_name_from_url(&valid_cover_url("deadbeef.PNG")).is_some());
    }

    /// 这条是本次改动的安全边界: 任何非本应用生成的地址都必须被拒,
    /// 否则前端传什么 URL 就能删什么文件。
    #[test]
    fn rejects_urls_that_are_not_our_covers() {
        let cases: Vec<String> = vec![
            // 书源远程封面: 没有文件可删, 必须落空
            "https://example.com/cover.jpg".to_string(),
            // 本应用其他目录(字体)不该被这个命令碰到
            valid_cover_url("abc.ttf"),
            // 前缀对但扩展名不在图片白名单里
            valid_cover_url("abc.exe"),
            valid_cover_url("abc.txt"),
            valid_cover_url("abc.svg"),
            // 试图跳出 covers 目录
            valid_cover_url("../secrets.png"),
            valid_cover_url("../../bookSource.json"),
            valid_cover_url("sub/dir.png"),
            // 文件名不是 hex(可能是别的流程写进去的)
            valid_cover_url("my-cover.png"),
            // 缺扩展名 / 空文件名
            valid_cover_url("deadbeef"),
            valid_cover_url(".png"),
            // 空串与前缀不符
            String::new(),
            "not a url".to_string(),
        ];
        for case in &cases {
            assert!(
                book_cover_file_name_from_url(case).is_none(),
                "应拒绝: {case}"
            );
        }
    }

    #[test]
    fn rejects_the_fonts_prefix_because_covers_are_separate() {
        let fontish = format!(
            "{}/files?path=default/fonts/deadbeef.png",
            crate::api::protocol::reader_scheme_origin()
        );
        assert!(book_cover_file_name_from_url(&fontish).is_none());
    }

    #[test]
    fn book_cover_url_uses_the_reader_protocol() {
        assert!(book_cover_url("abc123.png").ends_with("/files?path=default/covers/abc123.png"));
    }

    #[test]
    fn cover_size_limit_is_eight_megabytes() {
        assert_eq!(MAX_BOOK_COVER_BYTES, 8 * 1024 * 1024);
    }
}
