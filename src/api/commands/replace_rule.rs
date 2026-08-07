use crate::api::AppState;
use serde_json::Value;

use crate::error::error::{ApiResponse, AppError};
use crate::model::replace_rule::ReplaceRule;

#[tauri::command]
pub async fn get_replace_rules(
    state: tauri::State<'_, AppState>,
) -> Result<ApiResponse<Value>, AppError> {
    let user_ns = "default";
    let list = read_list::<ReplaceRule>(&state, &user_ns, "replaceRule.json").await?;
    Ok(ApiResponse::ok(
        serde_json::to_value(list).unwrap_or_default(),
    ))
}

#[tauri::command]
pub async fn save_replace_rule(
    state: tauri::State<'_, AppState>,
    req: ReplaceRule,
) -> Result<ApiResponse<Value>, AppError> {
    let user_ns = "default";
    if req.name.is_empty() {
        return Err(AppError::BadRequest("名称不能为空".to_string()));
    }
    if req.pattern.is_empty() {
        return Err(AppError::BadRequest("替换规则不能为空".to_string()));
    }
    let mut list = read_list::<ReplaceRule>(&state, &user_ns, "replaceRule.json").await?;
    upsert_by_key(&mut list, req, |r| r.name.clone());
    write_list(&state, &user_ns, "replaceRule.json", &list).await?;
    Ok(ApiResponse::ok(Value::String("".to_string())))
}

#[tauri::command]
pub async fn save_replace_rules(
    state: tauri::State<'_, AppState>,
    mut req: Vec<ReplaceRule>,
) -> Result<ApiResponse<Value>, AppError> {
    let user_ns = "default";
    let mut list = read_list::<ReplaceRule>(&state, &user_ns, "replaceRule.json").await?;
    req.retain(|r| !r.name.is_empty() && !r.pattern.is_empty());
    for r in req {
        upsert_by_key(&mut list, r, |v| v.name.clone());
    }
    write_list(&state, &user_ns, "replaceRule.json", &list).await?;
    Ok(ApiResponse::ok(Value::String("".to_string())))
}

#[tauri::command]
pub async fn delete_replace_rule(
    state: tauri::State<'_, AppState>,
    req: ReplaceRule,
) -> Result<ApiResponse<Value>, AppError> {
    let user_ns = "default";
    let mut list = read_list::<ReplaceRule>(&state, &user_ns, "replaceRule.json").await?;
    list.retain(|r| r.name != req.name);
    write_list(&state, &user_ns, "replaceRule.json", &list).await?;
    Ok(ApiResponse::ok(Value::String("".to_string())))
}

#[tauri::command]
pub async fn delete_replace_rules(
    state: tauri::State<'_, AppState>,
    req: Vec<ReplaceRule>,
) -> Result<ApiResponse<Value>, AppError> {
    let user_ns = "default";
    let mut list = read_list::<ReplaceRule>(&state, &user_ns, "replaceRule.json").await?;
    for r in req {
        list.retain(|v| v.name != r.name);
    }
    write_list(&state, &user_ns, "replaceRule.json", &list).await?;
    Ok(ApiResponse::ok(Value::String("".to_string())))
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