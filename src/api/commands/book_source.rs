use crate::api::AppState;
use crate::error::error::{ApiResponse, AppError};
use crate::model::book_source::{book_source_from_value, BookSource};
use crate::service::book_source_service::{
    book_source_has_group, set_invalid_book_source_group, INVALID_BOOK_SOURCE_GROUP,
};
use crate::util::text::normalize_source_url;
use serde::{Deserialize, Serialize};
use std::{collections::HashSet, sync::Arc};
use tauri_plugin_dialog::DialogExt;
use tokio::{sync::Semaphore, task::JoinSet};

const MAX_TEST_SOURCE_BATCH_SIZE: usize = 100;

#[derive(Debug, Deserialize)]
pub struct BookSourceUrlParam {
    #[serde(rename = "bookSourceUrl")]
    book_source_url: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ExploreKindsRequest {
    #[serde(rename = "bookSourceUrl")]
    book_source_url: Option<String>,
    #[serde(rename = "bookSource")]
    book_source: Option<BookSource>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
pub struct TestBookSourcesRequest {
    pub book_source_urls: Option<Vec<String>>,
    pub keyword: Option<String>,
    pub mark_invalid: Option<bool>,
    pub concurrent: Option<usize>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct TestBookSourceItem {
    book_source_url: String,
    book_source_name: String,
    valid: bool,
    search_ok: bool,
    explore_ok: bool,
    keyword: String,
    explore_url: Option<String>,
    search_error: Option<String>,
    explore_error: Option<String>,
    marked_invalid: bool,
    group: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct TestBookSourcesResponse {
    total: usize,
    valid: usize,
    invalid: usize,
    marked_invalid: usize,
    results: Vec<TestBookSourceItem>,
}

#[derive(Debug, Deserialize)]
pub struct UsernameParam {
    pub username: Option<String>,
}

#[tauri::command]
pub async fn save_book_source(
    state: tauri::State<'_, AppState>,
    req: serde_json::Value,
) -> Result<ApiResponse<serde_json::Value>, AppError> {
    let user_ns = "default";
    let source =
        book_source_from_value(req).map_err(|e| AppError::BadRequest(e.to_string()))?;
    state.book_source_service.save(&user_ns, source).await?;
    Ok(ApiResponse::ok(serde_json::json!({"saved": true})))
}

#[tauri::command]
pub async fn save_book_sources(
    state: tauri::State<'_, AppState>,
    req: serde_json::Value,
) -> Result<ApiResponse<serde_json::Value>, AppError> {
    let user_ns = "default";
    let sources = extract_sources(req)?;
    if sources.is_empty() {
        return Err(AppError::BadRequest("empty book sources".to_string()));
    }
    let count = sources.len();
    state
        .book_source_service
        .save_many(&user_ns, sources)
        .await?;
    Ok(ApiResponse::ok(
        serde_json::json!({"saved": true, "count": count}),
    ))
}

#[tauri::command]
pub async fn get_book_source(
    state: tauri::State<'_, AppState>,
    req: BookSourceUrlParam,
) -> Result<ApiResponse<serde_json::Value>, AppError> {
    let user_ns = "default";
    let url = req
        .book_source_url
        .ok_or_else(|| AppError::BadRequest("bookSourceUrl required".to_string()))?;
    let source = state
        .book_source_service
        .get(&user_ns, &url)
        .await?
        .ok_or_else(|| AppError::NotFound("bookSource not found".to_string()))?;
    Ok(ApiResponse::ok(
        serde_json::to_value(source).unwrap_or_default(),
    ))
}

#[tauri::command]
pub async fn get_book_sources(
    state: tauri::State<'_, AppState>,
) -> Result<ApiResponse<serde_json::Value>, AppError> {
    let user_ns = "default";
    let list = state.book_source_service.list(&user_ns).await?;
    Ok(ApiResponse::ok(
        serde_json::to_value(list).unwrap_or_default(),
    ))
}

#[tauri::command]
pub async fn get_default_book_source_owner(
    _state: tauri::State<'_, AppState>,
) -> Result<ApiResponse<serde_json::Value>, AppError> {
    // Single-user desktop: no default-source ownership model.
    Ok(ApiResponse::ok(serde_json::json!({ "username": null })))
}

#[tauri::command]
pub async fn pin_book_source(
    state: tauri::State<'_, AppState>,
    req: BookSourceUrlParam,
) -> Result<ApiResponse<serde_json::Value>, AppError> {
    let user_ns = "default";
    let url = req
        .book_source_url
        .ok_or_else(|| AppError::BadRequest("bookSourceUrl required".to_string()))?;
    state.book_source_service.pin(&user_ns, &url).await?;
    Ok(ApiResponse::ok(serde_json::json!({ "success": true })))
}

#[tauri::command]
pub async fn unpin_book_source(
    state: tauri::State<'_, AppState>,
    req: BookSourceUrlParam,
) -> Result<ApiResponse<serde_json::Value>, AppError> {
    let user_ns = "default";
    let url = req
        .book_source_url
        .ok_or_else(|| AppError::BadRequest("bookSourceUrl required".to_string()))?;
    state.book_source_service.unpin(&user_ns, &url).await?;
    Ok(ApiResponse::ok(serde_json::json!({ "success": true })))
}

#[tauri::command]
pub async fn login_book_source(
    state: tauri::State<'_, AppState>,
    req: BookSourceUrlParam,
) -> Result<ApiResponse<serde_json::Value>, AppError> {
    let user_ns = "default";
    let url = req
        .book_source_url
        .ok_or_else(|| AppError::BadRequest("bookSourceUrl required".to_string()))?;
    let source = state
        .book_source_service
        .get(&user_ns, &url)
        .await?
        .ok_or_else(|| AppError::NotFound("bookSource not found".to_string()))?;
    let result = state.book_service.login_book_source(&source).await?;
    Ok(ApiResponse::ok(result))
}

#[tauri::command]
pub async fn get_explore_kinds(
    state: tauri::State<'_, AppState>,
    req: ExploreKindsRequest,
) -> Result<ApiResponse<serde_json::Value>, AppError> {
    let user_ns = "default";

    let source = if let Some(source) = req.book_source {
        source
    } else {
        let url = req
            .book_source_url
            .ok_or_else(|| AppError::BadRequest("bookSourceUrl required".to_string()))?;
        state
            .book_source_service
            .get(&user_ns, &url)
            .await?
            .ok_or_else(|| AppError::NotFound("bookSource not found".to_string()))?
    };

    let kinds = state.book_service.explore_kinds(&source)?;
    Ok(ApiResponse::ok(
        serde_json::to_value(kinds).unwrap_or_default(),
    ))
}

#[tauri::command]
pub async fn test_book_sources(
    state: tauri::State<'_, AppState>,
    req: TestBookSourcesRequest,
) -> Result<ApiResponse<serde_json::Value>, AppError> {
    let user_ns = "default";

    let requested = normalize_requested_source_urls(req.book_source_urls.as_deref())?;
    let sources = state
        .book_source_service
        .list(&user_ns)
        .await?
        .into_iter()
        .filter(|source| {
            requested
                .as_ref()
                .map(|urls| urls.contains(&normalize_source_url(&source.book_source_url)))
                .unwrap_or(true)
        })
        .collect::<Vec<_>>();

    let concurrent = req.concurrent.unwrap_or(12).clamp(1, 12);
    let keyword = req.keyword.clone();
    let mark_invalid = req.mark_invalid.unwrap_or(true);
    let outcomes = test_sources_in_parallel(
        state.book_service.clone(),
        user_ns.to_string(),
        keyword,
        sources,
        concurrent,
    )
    .await;

    let mut results = Vec::with_capacity(outcomes.len());
    let mut marked_invalid = 0usize;
    for (mut source, availability) in outcomes {
        let changed = if mark_invalid {
            set_invalid_book_source_group(&mut source, !availability.valid)
        } else {
            false
        };
        if changed {
            state
                .book_source_service
                .save(&user_ns, source.clone())
                .await?;
            if !availability.valid {
                marked_invalid += 1;
            }
        }

        results.push(TestBookSourceItem {
            book_source_url: availability.book_source_url,
            book_source_name: availability.book_source_name,
            valid: availability.valid,
            search_ok: availability.search_ok,
            explore_ok: availability.explore_ok,
            keyword: availability.keyword,
            explore_url: availability.explore_url,
            search_error: availability.search_error,
            explore_error: availability.explore_error,
            marked_invalid: changed && !availability.valid,
            group: source.book_source_group,
        });
    }

    results.sort_by(|a, b| a.book_source_name.cmp(&b.book_source_name));
    let valid = results.iter().filter(|item| item.valid).count();
    let invalid = results.len().saturating_sub(valid);
    let response = TestBookSourcesResponse {
        total: results.len(),
        valid,
        invalid,
        marked_invalid,
        results,
    };
    Ok(ApiResponse::ok(
        serde_json::to_value(response).unwrap_or_default(),
    ))
}

fn normalize_requested_source_urls(
    urls: Option<&[String]>,
) -> Result<Option<HashSet<String>>, AppError> {
    let Some(urls) = urls else {
        return Ok(None);
    };
    if urls.len() > MAX_TEST_SOURCE_BATCH_SIZE {
        return Err(AppError::BadRequest(format!(
            "bookSourceUrls 最多支持 {} 条",
            MAX_TEST_SOURCE_BATCH_SIZE
        )));
    }
    Ok(Some(
        urls.iter()
            .map(|url| normalize_source_url(url))
            .filter(|url| !url.is_empty())
            .collect::<HashSet<_>>(),
    ))
}

async fn test_sources_in_parallel(
    book_service: Arc<crate::service::book_service::BookService>,
    user_ns: String,
    keyword: Option<String>,
    sources: Vec<BookSource>,
    concurrent: usize,
) -> Vec<(
    BookSource,
    crate::service::book_service::BookSourceAvailability,
)> {
    let permits = Arc::new(Semaphore::new(concurrent.max(1)));
    let mut tasks = JoinSet::new();

    for source in sources {
        let permit = match permits.clone().acquire_owned().await {
            Ok(permit) => permit,
            Err(_) => break,
        };
        let book_service = book_service.clone();
        let user_ns = user_ns.clone();
        let keyword = keyword.clone();
        tasks.spawn(async move {
            let _permit = permit;
            let availability = book_service
                .test_book_source_availability(&user_ns, &source, keyword.as_deref())
                .await;
            (source, availability)
        });
    }

    let mut outcomes = Vec::with_capacity(tasks.len());
    while let Some(result) = tasks.join_next().await {
        match result {
            Ok(outcome) => outcomes.push(outcome),
            Err(err) => tracing::error!("book source test task failed: {err}"),
        }
    }
    outcomes
}

#[tauri::command]
pub async fn delete_invalid_book_sources(
    state: tauri::State<'_, AppState>,
) -> Result<ApiResponse<serde_json::Value>, AppError> {
    let user_ns = "default";
    let sources = state.book_source_service.list(&user_ns).await?;
    let invalid_urls = sources
        .iter()
        .filter(|source| book_source_has_group(source, INVALID_BOOK_SOURCE_GROUP))
        .map(|source| source.book_source_url.clone())
        .collect::<Vec<_>>();
    for url in &invalid_urls {
        state.book_source_service.delete(&user_ns, url).await?;
    }
    Ok(ApiResponse::ok(serde_json::json!({
        "deleted": invalid_urls.len()
    })))
}

#[tauri::command]
pub async fn delete_book_source(
    state: tauri::State<'_, AppState>,
    req: BookSourceUrlParam,
) -> Result<ApiResponse<serde_json::Value>, AppError> {
    let user_ns = "default";
    let url = req
        .book_source_url
        .ok_or_else(|| AppError::BadRequest("bookSourceUrl required".to_string()))?;
    state.book_source_service.delete(&user_ns, &url).await?;
    Ok(ApiResponse::ok(serde_json::json!({"deleted": true})))
}

#[tauri::command]
pub async fn delete_book_sources(
    state: tauri::State<'_, AppState>,
    req: Vec<BookSourceUrlParam>,
) -> Result<ApiResponse<serde_json::Value>, AppError> {
    let user_ns = "default";
    for item in req {
        if let Some(url) = item.book_source_url {
            state.book_source_service.delete(&user_ns, &url).await?;
        }
    }
    Ok(ApiResponse::ok(serde_json::json!({"deleted": true})))
}

#[tauri::command]
pub async fn delete_all_book_sources(
    state: tauri::State<'_, AppState>,
) -> Result<ApiResponse<serde_json::Value>, AppError> {
    let user_ns = "default";
    state.book_source_service.delete_all(&user_ns).await?;
    Ok(ApiResponse::ok(serde_json::json!({"deleted": true})))
}

fn extract_sources(payload: serde_json::Value) -> Result<Vec<BookSource>, AppError> {
    if let Some(items) = payload.as_array() {
        return items
            .iter()
            .cloned()
            .map(|value| {
                book_source_from_value(value).map_err(|e| AppError::BadRequest(e.to_string()))
            })
            .collect();
    }
    if let Some(obj) = payload.as_object() {
        for key in ["bookSourceList", "bookSources", "data", "sources"] {
            if let Some(v) = obj.get(key) {
                if let Some(items) = v.as_array() {
                    return items
                        .iter()
                        .cloned()
                        .map(|value| {
                            book_source_from_value(value)
                                .map_err(|e| AppError::BadRequest(e.to_string()))
                        })
                        .collect();
                }
            }
        }
    }
    Err(AppError::BadRequest(
        "invalid book sources payload".to_string(),
    ))
}

#[derive(Debug, Deserialize)]
pub struct RemoteSourceParam {
    url: String,
}

#[tauri::command]
pub async fn read_remote_source_file(
    req: RemoteSourceParam,
) -> Result<ApiResponse<Vec<String>>, AppError> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36")
        .danger_accept_invalid_certs(true)
        .build()
        .map_err(|e| AppError::Internal(e.into()))?;

    let text = client
        .get(&req.url)
        .send()
        .await
        .map_err(|e| {
            tracing::error!("Failed to fetch remote source from {}: {:?}", req.url, e);
            AppError::BadRequest(format!("网络请求失败: {}", e))
        })?
        .text()
        .await
        .map_err(|e| AppError::BadRequest(format!("读取响应失败: {}", e)))?;

    let sources: Vec<BookSource> = serde_json::from_str::<serde_json::Value>(&text)
        .map_err(|_| AppError::BadRequest("invalid book sources json format".to_string()))
        .and_then(extract_sources)?;

    // Return as array of JSON strings (frontend expects each item to be a JSON string)
    let json_str = serde_json::to_string(&sources)
        .map_err(|e| AppError::BadRequest(format!("序列化书源失败: {}", e)))?;

    Ok(ApiResponse::ok(vec![json_str]))
}

#[tauri::command]
pub async fn read_source_file(
    file: Vec<u8>,
    file_name: String,
) -> Result<ApiResponse<serde_json::Value>, AppError> {
    if file_name.ends_with(".json") || file_name.ends_with(".txt") {
        let text = String::from_utf8_lossy(&file);
        let sources: Vec<BookSource> = serde_json::from_str::<serde_json::Value>(&text)
            .map_err(|_| {
                AppError::BadRequest("invalid book sources json format".to_string())
            })
            .and_then(extract_sources)?;
        return Ok(ApiResponse::ok(serde_json::to_value(sources).unwrap_or_default()));
    }
    Err(AppError::BadRequest("No json file uploaded".to_string()))
}

/// Opens the native save dialog on desktop and writes a formatted book-source
/// export. Browser callers should keep using the frontend download fallback.
#[tauri::command]
pub async fn export_book_sources_to_file(
    app: tauri::AppHandle,
    sources: Vec<BookSource>,
) -> Result<ApiResponse<serde_json::Value>, AppError> {
    let Some(file) = app
        .dialog()
        .file()
        .set_title("导出书源")
        .set_file_name(format!("reader-book-sources-{}.json", export_timestamp()))
        .add_filter("JSON 文件", &["json"])
        .blocking_save_file()
    else {
        return Ok(ApiResponse::ok(serde_json::json!({"saved": false, "cancelled": true})));
    };
    let path = file
        .into_path()
        .map_err(|error| AppError::BadRequest(format!("无法访问导出路径：{error}")))?;
    let json = serde_json::to_vec_pretty(&sources)
        .map_err(|error| AppError::Internal(error.into()))?;
    tokio::fs::write(&path, json)
        .await
        .map_err(|error| AppError::Internal(error.into()))?;
    Ok(ApiResponse::ok(serde_json::json!({
        "saved": true,
        "path": path.to_string_lossy(),
    })))
}

fn export_timestamp() -> String {
    chrono::Local::now().format("%Y%m%d-%H%M%S").to_string()
}

#[tauri::command]
pub async fn set_as_default_book_sources(
    _state: tauri::State<'_, AppState>,
    _req: UsernameParam,
) -> Result<ApiResponse<serde_json::Value>, AppError> {
    // Single-user desktop: no default-source ownership model.
    Ok(ApiResponse::ok(
        serde_json::json!({"success": true, "count": 0}),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requested_source_urls_rejects_batches_above_limit() {
        let urls = (0..101)
            .map(|index| format!("https://source-{index}.example"))
            .collect::<Vec<_>>();

        let err = normalize_requested_source_urls(Some(&urls)).unwrap_err();

        assert!(matches!(err, AppError::BadRequest(message) if message.contains("100")));
    }
}
