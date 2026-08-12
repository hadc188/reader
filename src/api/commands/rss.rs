use crate::api::AppState;
use serde::Deserialize;
use serde_json::Value;

use crate::error::error::{ApiResponse, AppError};
use crate::model::rss::{RssArticle, RssSource};
use crate::util::time::now_ts;

#[derive(Debug, Deserialize)]
pub struct RemoteRssSourceParam {
    url: String,
}

#[derive(Debug, Deserialize)]
pub struct RssArticlesRequest {
    #[serde(rename = "sourceUrl")]
    pub source_url: Option<String>,
    #[serde(rename = "sortName")]
    pub sort_name: Option<String>,
    #[serde(rename = "sortUrl")]
    pub sort_url: Option<String>,
    pub page: Option<i32>,
}

#[derive(Debug, Deserialize)]
pub struct RssContentRequest {
    #[serde(rename = "sourceUrl")]
    pub source_url: Option<String>,
    pub link: Option<String>,
    pub origin: Option<String>,
}

#[tauri::command]
pub async fn get_rss_sources(
    state: tauri::State<'_, AppState>,
) -> Result<ApiResponse<Value>, AppError> {
    let user_ns = "default";
    let list = read_list::<RssSource>(&state, &user_ns, "rssSources.json").await?;
    Ok(ApiResponse::ok(
        serde_json::to_value(list).unwrap_or_default(),
    ))
}

#[tauri::command]
pub async fn save_rss_source(
    state: tauri::State<'_, AppState>,
    req: RssSource,
) -> Result<ApiResponse<Value>, AppError> {
    let user_ns = "default";
    if req.source_url.is_empty() {
        return Err(AppError::BadRequest("RSS链接不能为空".to_string()));
    }
    if req.source_name.is_empty() {
        return Err(AppError::BadRequest("RSS名称不能为空".to_string()));
    }
    let mut list = read_list::<RssSource>(&state, &user_ns, "rssSources.json").await?;
    upsert_by_key(&mut list, req, |s| s.source_url.clone());
    write_list(&state, &user_ns, "rssSources.json", &list).await?;
    Ok(ApiResponse::ok(Value::String("".to_string())))
}

#[tauri::command]
pub async fn save_rss_sources(
    state: tauri::State<'_, AppState>,
    mut req: Vec<RssSource>,
) -> Result<ApiResponse<Value>, AppError> {
    let user_ns = "default";
    let mut list = read_list::<RssSource>(&state, &user_ns, "rssSources.json").await?;
    req.retain(|s| !s.source_url.is_empty() && !s.source_name.is_empty());
    for s in req {
        upsert_by_key(&mut list, s, |v| v.source_url.clone());
    }
    write_list(&state, &user_ns, "rssSources.json", &list).await?;
    Ok(ApiResponse::ok(Value::String("".to_string())))
}

#[tauri::command]
pub async fn delete_rss_source(
    state: tauri::State<'_, AppState>,
    req: RssSource,
) -> Result<ApiResponse<Value>, AppError> {
    let user_ns = "default";
    let mut list = read_list::<RssSource>(&state, &user_ns, "rssSources.json").await?;
    list.retain(|s| s.source_url != req.source_url);
    write_list(&state, &user_ns, "rssSources.json", &list).await?;
    Ok(ApiResponse::ok(Value::String("".to_string())))
}

#[tauri::command]
pub async fn delete_rss_sources(
    state: tauri::State<'_, AppState>,
    req: Vec<RssSource>,
) -> Result<ApiResponse<Value>, AppError> {
    let user_ns = "default";
    let mut list = read_list::<RssSource>(&state, &user_ns, "rssSources.json").await?;
    let deleted = remove_rss_sources_by_url(&mut list, &req);
    write_list(&state, &user_ns, "rssSources.json", &list).await?;
    Ok(ApiResponse::ok(
        serde_json::json!({ "deleted": deleted }),
    ))
}

#[tauri::command]
pub async fn read_remote_rss_source_file(
    state: tauri::State<'_, AppState>,
    req: RemoteRssSourceParam,
) -> Result<ApiResponse<Vec<String>>, AppError> {
    let client = state.book_service.http_client();

    let text = client
        .get(&req.url)
        .send()
        .await
        .map_err(|e| AppError::BadRequest(format!("网络请求失败: {}", e)))?
        .text()
        .await
        .map_err(|e| AppError::BadRequest(format!("读取响应失败: {}", e)))?;

    let sources: Vec<RssSource> = serde_json::from_str(&text).or_else(|_| {
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) {
            extract_rss_sources(v)
        } else {
            Err(AppError::BadRequest(
                "invalid rss sources json format".to_string(),
            ))
        }
    })?;

    let json_str = serde_json::to_string(&sources)
        .map_err(|e| AppError::BadRequest(format!("序列化RSS源失败: {}", e)))?;

    Ok(ApiResponse::ok(vec![json_str]))
}

#[tauri::command]
pub async fn read_rss_source_file(
    file: Vec<u8>,
    file_name: String,
) -> Result<ApiResponse<serde_json::Value>, AppError> {
    if file_name.ends_with(".json") || file_name.ends_with(".txt") {
        let text = String::from_utf8_lossy(&file);
        let sources: Vec<RssSource> = serde_json::from_str(&text).or_else(|_| {
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) {
                extract_rss_sources(v)
            } else {
                Err(AppError::BadRequest(
                    "invalid rss sources json format".to_string(),
                ))
            }
        })?;
        return Ok(ApiResponse::ok(serde_json::to_value(sources).unwrap_or_default()));
    }
    Err(AppError::BadRequest("No json file uploaded".to_string()))
}

#[tauri::command]
pub async fn get_rss_articles(
    state: tauri::State<'_, AppState>,
    req: RssArticlesRequest,
) -> Result<ApiResponse<Value>, AppError> {
    let user_ns = "default";
    let source_url = req
        .source_url
        .ok_or_else(|| AppError::BadRequest("RSS源链接不能为空".to_string()))?;
    let sort_url = req.sort_url.unwrap_or_else(|| source_url.clone());
    let sort_name = req.sort_name.unwrap_or_default();
    let page = req.page.unwrap_or(1).max(1);

    let list = read_list::<RssSource>(&state, &user_ns, "rssSources.json").await?;
    let _rss_source = list
        .into_iter()
        .find(|s| s.source_url == source_url)
        .ok_or_else(|| AppError::BadRequest("RSS源不存在".to_string()))?;

    let res = state
        .book_service
        .http_client()
        .get(&sort_url)
        .send()
        .await
        .map_err(|e| AppError::Internal(e.into()))?;
    let bytes = res
        .bytes()
        .await
        .map_err(|e| AppError::Internal(e.into()))?;
    let feed =
        feed_rs::parser::parse(&bytes[..]).map_err(|e| AppError::BadRequest(e.to_string()))?;

    let mut items = Vec::new();
    for entry in feed.entries {
        let title = entry
            .title
            .as_ref()
            .map(|t| t.content.clone())
            .unwrap_or_default();
        let link = entry
            .links
            .first()
            .map(|l| l.href.clone())
            .unwrap_or_default();
        let description = entry.summary.as_ref().map(|s| s.content.clone());
        let content = entry.content.as_ref().and_then(|c| c.body.clone());
        let pub_date = entry.published.or(entry.updated).map(|d| d.to_rfc3339());
        let image = entry
            .media
            .first()
            .and_then(|m| m.thumbnails.first())
            .map(|t| t.image.uri.clone());
        let order = entry
            .published
            .or(entry.updated)
            .map(|d| d.timestamp())
            .unwrap_or(now_ts());
        items.push(RssArticle {
            origin: sort_url.clone(),
            sort: sort_name.clone(),
            title,
            order,
            link,
            pub_date,
            description,
            content,
            image,
            read: Some(false),
            variable: None,
        });
    }

    let page_size = 50usize;
    let start = ((page - 1) as usize) * page_size;
    let end = std::cmp::min(start + page_size, items.len());
    let page_items = if start < items.len() {
        items[start..end].to_vec()
    } else {
        Vec::new()
    };
    let data = serde_json::json!({"first": page_items, "second": null});
    Ok(ApiResponse::ok(data))
}

#[tauri::command]
pub async fn get_rss_content(
    state: tauri::State<'_, AppState>,
    req: RssContentRequest,
) -> Result<ApiResponse<Value>, AppError> {
    let user_ns = "default";
    let source_url = req
        .source_url
        .ok_or_else(|| AppError::BadRequest("RSS链接不能为空".to_string()))?;
    let link = req
        .link
        .ok_or_else(|| AppError::BadRequest("RSS文章链接不能为空".to_string()))?;
    let _origin = req
        .origin
        .ok_or_else(|| AppError::BadRequest("RSS文章来源不能为空".to_string()))?;

    let list = read_list::<RssSource>(&state, &user_ns, "rssSources.json").await?;
    let _rss_source = list
        .into_iter()
        .find(|s| s.source_url == source_url)
        .ok_or_else(|| AppError::BadRequest("RSS源不存在".to_string()))?;

    let res = state
        .book_service
        .http_client()
        .get(&link)
        .send()
        .await
        .map_err(|e| AppError::Internal(e.into()))?;
    let body = res.text().await.map_err(|e| AppError::Internal(e.into()))?;
    Ok(ApiResponse::ok(Value::String(body)))
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

fn extract_rss_sources(payload: serde_json::Value) -> Result<Vec<RssSource>, AppError> {
    if payload.is_array() {
        return serde_json::from_value::<Vec<RssSource>>(payload)
            .map_err(|e| AppError::BadRequest(e.to_string()));
    }
    if let Some(obj) = payload.as_object() {
        for key in ["rssSources", "rssSourceList", "data", "sources"] {
            if let Some(v) = obj.get(key) {
                if v.is_array() {
                    return serde_json::from_value::<Vec<RssSource>>(v.clone())
                        .map_err(|e| AppError::BadRequest(e.to_string()));
                }
            }
        }
    }
    Err(AppError::BadRequest(
        "invalid rss sources payload".to_string(),
    ))
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

fn remove_rss_sources_by_url(list: &mut Vec<RssSource>, targets: &[RssSource]) -> usize {
    let before = list.len();
    let target_urls = targets
        .iter()
        .filter(|source| !source.source_url.is_empty())
        .map(|source| source.source_url.as_str())
        .collect::<std::collections::HashSet<_>>();
    list.retain(|source| !target_urls.contains(source.source_url.as_str()));
    before.saturating_sub(list.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rss_source(name: &str, url: &str) -> RssSource {
        RssSource {
            source_name: name.to_string(),
            source_url: url.to_string(),
            ..Default::default()
        }
    }

    #[test]
    fn remove_rss_sources_by_url_keeps_unmatched_sources() {
        let mut list = vec![
            rss_source("News", "https://news.example"),
            rss_source("Tech", "https://tech.example"),
            rss_source("Blog", "https://blog.example"),
        ];
        let targets = vec![
            rss_source("Ignored Name", "https://tech.example"),
            rss_source("Missing", "https://missing.example"),
        ];

        let deleted = remove_rss_sources_by_url(&mut list, &targets);

        assert_eq!(deleted, 1);
        assert_eq!(
            list.iter()
                .map(|source| source.source_url.as_str())
                .collect::<Vec<_>>(),
            vec!["https://news.example", "https://blog.example"]
        );
    }
}
