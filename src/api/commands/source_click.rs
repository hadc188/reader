use crate::api::AppState;
use crate::error::error::{ApiResponse, AppError};
use crate::service::book_service::ShowBrowserRequest;
use serde::{Deserialize, Serialize};

/// `img` 的 `,{json}` 里 `click` 脚本的执行结果。
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceClickResult {
    /// 脚本返回值(插件通常返回面板 HTML 或空串)。
    pub result: String,
    /// 本次执行产生的 JS 日志(`java.log` / `java.toast` / 异常)。
    pub logs: Vec<String>,
    /// 脚本调用 `java.showBrowser` 时捕获的面板请求。
    pub show_browser: Option<ShowBrowserRequest>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EvalSourceClickRequest {
    pub book_source_url: Option<String>,
    pub book_url: Option<String>,
    pub chapter_url: Option<String>,
    /// `img` 参数里的 `click` 脚本。
    pub script: Option<String>,
    /// 传给脚本的 `result`(即图片原始 src)。
    pub result: Option<String>,
}

/// 执行正文图片的 `click` 脚本(legado `ReadBookActivity.clickImg`)。
/// 脚本在书源 jsLib 作用域里运行, 因此插件定义的函数可直接调用。
#[tauri::command]
pub async fn eval_source_click(
    state: tauri::State<'_, AppState>,
    req: EvalSourceClickRequest,
) -> Result<ApiResponse<SourceClickResult>, AppError> {
    let user_ns = "default";
    let source_url = req
        .book_source_url
        .ok_or_else(|| AppError::BadRequest("bookSourceUrl required".to_string()))?;
    let script = req
        .script
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(|| AppError::BadRequest("script required".to_string()))?;
    let source = state
        .book_source_service
        .get(&user_ns, &source_url)
        .await?
        .ok_or_else(|| AppError::NotFound("bookSource not found".to_string()))?;

    let (result, logs, show_browser) = state
        .book_service
        .eval_source_click(
            user_ns,
            req.book_url.as_deref().unwrap_or(""),
            &source,
            req.chapter_url.as_deref().unwrap_or(""),
            &script,
            req.result.as_deref().unwrap_or(""),
        )
        .await?;

    Ok(ApiResponse::ok(SourceClickResult {
        result,
        logs,
        show_browser,
    }))
}
