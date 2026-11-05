use crate::api::commands::{decode_header_file_name, decode_upload_body};
use crate::api::AppState;
use crate::error::error::{ApiResponse, AppError};
use crate::model::book_source::{book_source_from_value, BookSource};
use crate::service::book_source_service::{
    book_source_has_group, set_invalid_book_source_group, INVALID_BOOK_SOURCE_GROUP,
};
use crate::util::text::normalize_source_url;
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};
use tauri::Emitter;
use tauri_plugin_dialog::DialogExt;
use tokio::task::JoinSet;

const MAX_TEST_SOURCE_BATCH_SIZE: usize = 100;
static SOURCE_TEST_CANCELLATIONS: Lazy<Mutex<HashMap<String, Arc<AtomicBool>>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));

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
    /// 发现页控件当前取值（legado `InfoMap`）。JS 型 exploreUrl 会读它来决定
    /// 生成哪些分类，并可能写回默认值。
    #[serde(rename = "infoMap")]
    info_map: Option<std::collections::HashMap<String, String>>,
}

/// 发现页控件 `action` 的求值请求。
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExploreActionRequest {
    pub book_source_url: Option<String>,
    pub book_source: Option<BookSource>,
    /// 要执行的 `action` 脚本。
    pub action: String,
    #[serde(default)]
    pub info_map: std::collections::HashMap<String, String>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
pub struct TestBookSourcesRequest {
    pub book_source_urls: Option<Vec<String>>,
    pub keyword: Option<String>,
    pub mark_invalid: Option<bool>,
    pub concurrent: Option<usize>,
    pub task_id: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CancelBookSourceTestRequest {
    pub task_id: String,
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
    /// 该条因用户中止而未检测完成(既不算有效也不算失效)。
    cancelled: bool,
    group: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct TestBookSourcesResponse {
    total: usize,
    valid: usize,
    invalid: usize,
    marked_invalid: usize,
    cancelled: bool,
    /// 因中止而未完成的条数, 供前端如实提示"已中止, N 个未检测"。
    cancelled_count: usize,
    results: Vec<TestBookSourceItem>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct BookSourceTestProgress {
    task_id: String,
    total: usize,
    completed: usize,
    valid: usize,
    invalid: usize,
    cancelled: bool,
}

#[derive(Debug, Deserialize)]
pub struct UsernameParam {
    pub username: Option<String>,
}

/// `save_book_source` 的共享实现 (Tauri command 与 Web HTTP handler 共用)。
pub async fn save_book_source_core(
    state: &AppState,
    req: serde_json::Value,
) -> Result<ApiResponse<serde_json::Value>, AppError> {
    let user_ns = "default";
    let source =
        book_source_from_value(req).map_err(|e| AppError::BadRequest(e.to_string()))?;
    let existing_source = state
        .book_source_service
        .get(&user_ns, &source.book_source_url)
        .await?;
    let source_became_available = source_became_available(existing_source.as_ref(), &source);
    state.book_source_service.save(&user_ns, source).await?;
    if source_became_available {
        invalidate_shelf_book_source_caches(state, &user_ns).await?;
    }
    Ok(ApiResponse::ok(serde_json::json!({"saved": true})))
}

/// `save_book_source` 的 Tauri 入口: 只是 core 的薄壳。
#[tauri::command]
pub async fn save_book_source(
    state: tauri::State<'_, AppState>,
    req: serde_json::Value,
) -> Result<ApiResponse<serde_json::Value>, AppError> {
    save_book_source_core(&state, req).await
}

/// `save_book_sources` 的共享实现 (Tauri command 与 Web HTTP handler 共用)。
pub async fn save_book_sources_core(
    state: &AppState,
    req: serde_json::Value,
) -> Result<ApiResponse<serde_json::Value>, AppError> {
    let user_ns = "default";
    let sources = extract_sources(req)?;
    if sources.is_empty() {
        return Err(AppError::BadRequest("empty book sources".to_string()));
    }
    let existing_sources = state
        .book_source_service
        .list(&user_ns)
        .await?
        .into_iter()
        .map(|source| (source.book_source_url.clone(), source))
        .collect::<HashMap<_, _>>();
    let has_new_available_source = sources
        .iter()
        .any(|source| source_became_available(existing_sources.get(&source.book_source_url), source));
    let count = sources.len();
    let save_result = state
        .book_source_service
        .save_many(&user_ns, sources)
        .await;
    if has_new_available_source {
        // save_many 是"尽力写入"(并发提交, 任一失败仍继续其余写入), 因此返回 Err 时
        // **可能已有部分源入库**。缓存失效必须照常执行, 否则换源结果与刚写入的源
        // 不一致。用 `?` 直接返回会跳过这一步, 留下已入库但缓存陈旧的源。
        let invalidate_result = invalidate_shelf_book_source_caches(state, &user_ns).await;
        // 保存错误优先上报(它更关键), 但不能因此丢掉缓存失效的错误。
        save_result?;
        invalidate_result?;
    } else {
        save_result?;
    }
    Ok(ApiResponse::ok(
        serde_json::json!({"saved": true, "count": count}),
    ))
}

/// `save_book_sources` 的 Tauri 入口: 只是 core 的薄壳。
#[tauri::command]
pub async fn save_book_sources(
    state: tauri::State<'_, AppState>,
    req: serde_json::Value,
) -> Result<ApiResponse<serde_json::Value>, AppError> {
    save_book_sources_core(&state, req).await
}

/// `get_book_source` 的共享实现 (Tauri command 与 Web HTTP handler 共用)。
pub async fn get_book_source_core(
    state: &AppState,
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

/// `get_book_source` 的 Tauri 入口: 只是 core 的薄壳。
#[tauri::command]
pub async fn get_book_source(
    state: tauri::State<'_, AppState>,
    req: BookSourceUrlParam,
) -> Result<ApiResponse<serde_json::Value>, AppError> {
    get_book_source_core(&state, req).await
}

/// `get_book_sources` 的共享实现 (Tauri command 与 Web HTTP handler 共用)。
pub async fn get_book_sources_core(
    state: &AppState,
) -> Result<ApiResponse<serde_json::Value>, AppError> {
    let user_ns = "default";
    let list = state.book_source_service.list(&user_ns).await?;
    Ok(ApiResponse::ok(
        serde_json::to_value(list).unwrap_or_default(),
    ))
}

/// `get_book_sources` 的 Tauri 入口: 只是 core 的薄壳。
#[tauri::command]
pub async fn get_book_sources(
    state: tauri::State<'_, AppState>,
) -> Result<ApiResponse<serde_json::Value>, AppError> {
    get_book_sources_core(&state).await
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

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BookSourceLoginParam {
    pub book_source_url: Option<String>,
    pub action: Option<String>,
    pub values: Option<serde_json::Map<String, serde_json::Value>>,
}

#[tauri::command]
pub async fn login_book_source(
    state: tauri::State<'_, AppState>,
    app: tauri::AppHandle,
    req: BookSourceLoginParam,
    on_browser: tauri::ipc::Channel<crate::service::book_service::ShowBrowserRequest>,
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

    // 执行登录脚本**之前**，先把登录窗口里现有的 Cookie 收进书源 jar。
    //
    // 用户的真实操作是：点「🌐 打开浏览器完成验证」→ 在独立窗口里过完
    // Cloudflare 人机验证 → **不关窗口**直接切回来点「🍪 查看Cookie状态」。
    // 而窗口 Cookie 原本只在 `CloseRequested` 时回收，那一刻 jar 还是空的，
    // 于是必然报「❌ 未检测到 cf_clearance Cookie」。
    //
    // 手机端 legado 没这个问题：`WebViewActivity.onPageFinished` 每次加载完就
    // `CookieStore.setCookie(...)`。桌面端**不能**照抄那个时机 ——
    // `WebviewWindow::cookies()` 底层是 `wait_with_pump`（`GetMessageA` /
    // `DispatchMessageA` 拉取当前线程消息队列），在 `NavigationCompleted`
    // 回调栈里调用会重入消息泵，直接把窗口卡死（实测「一打开登录窗口就
    // 未响应」）。
    //
    // 放在这里就安全：Tauri command 跑在 async 运行时，不在任何 WebView2
    // 事件回调的栈上，泵消息是正常的。
    sync_login_window_cookies(&app, &state.book_service, &source.book_source_url).await;

    let handler: crate::service::source_browser::BrowserHandler = Arc::new(move |request| {
        on_browser.send(request).map_err(|err| err.to_string())
    });
    let mut result = state.book_service.run_source_login_with_browser(&source, req.action, req.values, Some(handler)).await?;
    let login_session = state
        .book_service
        .create_source_login_session(&source.book_source_url)
        .await;
    if let Some(object) = result.as_object_mut() {
        object.insert(
            "loginSession".to_string(),
            serde_json::Value::String(login_session),
        );
    }
    Ok(ApiResponse::ok(result))
}

/// 取书源声明的 `User-Agent`（`header` 里的那一项）。
///
/// 用于把独立验证窗口的 UA 设成与书源请求一致 —— Cloudflare 的 `cf_clearance`
/// 按 UA 绑定，两边不一致时 token 失效。取不到就返回 `None`，窗口用默认 UA。
///
/// `header` 里的 UA 可能是脚本（`@js:`）算出来的，那种情况这里拿不到静态值，
/// 直接跳过：设一个错的 UA 比不设更糟。
async fn source_user_agent(
    source_service: &Arc<crate::service::book_source_service::BookSourceService>,
    book_source_url: Option<&str>,
) -> Option<String> {
    let url = book_source_url?.trim();
    if url.is_empty() {
        return None;
    }
    let source = source_service.get("default", url).await.ok().flatten()?;
    let header = source.header.as_deref()?.trim();
    // 脚本形态的头拿不到静态值（需要执行上下文），放弃。
    if header.starts_with("@js:") || header.starts_with("<js>") {
        return None;
    }
    crate::crawler::url_analyzer::parse_source_headers(header)
        .into_iter()
        .find(|(name, _)| name.eq_ignore_ascii_case("user-agent"))
        .map(|(_, value)| value)
        .filter(|value| !value.trim().is_empty())
}

/// 把独立登录窗口当前的 Cookie 同步进书源 jar（若窗口存在）。
///
/// 只能在**非 WebView2 事件回调**的上下文里调用，见 `login_book_source` 的说明。
async fn sync_login_window_cookies(
    app: &tauri::AppHandle,
    service: &Arc<crate::service::book_service::BookService>,
    source_url: &str,
) {
    use tauri::Manager;
    // 与 `open_external_window` 用的是同一个窗口标签。
    let Some(window) = app.get_webview_window("source-link") else {
        return;
    };
    let cookies = match crate::service::webview_cookies::read_all(&window) {
        Ok(cookies) => cookies,
        Err(err) => {
            tracing::debug!(%err, "读取登录窗口 Cookie 失败（窗口可能已关闭）");
            return;
        }
    };
    if cookies.is_empty() {
        return;
    }
    match service.set_source_window_cookies(source_url, &cookies).await {
        Ok(0) => {}
        Ok(count) => tracing::info!(count, "已把登录窗口 Cookie 同步到书源"),
        Err(err) => tracing::warn!(%err, "同步登录窗口 Cookie 失败"),
    }
}

#[tauri::command]
pub fn close_source_panel(panel_id: String) {
    crate::service::source_browser::cancel_browser(&panel_id);
    crate::service::book_service::remove_source_panel(&panel_id);
}

/// 把书源面板里的「二级链接」（注册页、用户后台等）**交给独立窗口**打开。
///
/// 为什么不能用 iframe：上游站点常常带 `X-Frame-Options: SAMEORIGIN` 或
/// CSP `frame-ancestors`，嵌在 iframe 里会被浏览器拒绝渲染；有的站点还会用
/// 前端安全 SDK 主动检测「自己是否被 iframe 嵌入」，命中就整页替换成
/// 「页面渲染安全策略拦截」。独立窗口里 `window.top === window`，这些检测
/// 都不触发 —— 与手机端 legado 直接 `loadUrl` 的行为一致。
///
/// **Cookie 回传**：窗口里登录成功后，站点把会话写在它自己的域下，那份 Cookie
/// 只存在于 WebView2 profile；书源后续请求走我们自己的 jar，两边不通。所以窗口
/// 关闭时要把 Cookie 读出来写回 jar —— 这正是「窗口里登录成功、回面板仍显示
/// 未登录」的成因。
///
/// 窗口尺寸取主窗口的 92%，居中略偏上，视觉上像「从主窗口里弹出来的」。
/// 移动端 UA 时独立窗口用的宽度（CSS px）。
///
/// 取 420：介于 iPhone（390/393/430）与主流安卓（360-412）之间，绝大多数
/// 移动 H5 站在这个宽度下按单列布局渲染，与手机观感一致。
///
/// 为什么不能直接用桌面尺寸：`<meta name="viewport" content="width=device-width">`
/// 的页面拿到 900px 宽会按 900px 布局 —— 表单横向铺满、固定高度的滚动容器
/// 反而把内容切掉，表现是「显示不全 + 不能下滑」（书源 8054 的番茄登录页）。
const MOBILE_WINDOW_WIDTH: f64 = 420.0;

/// 移动窗口的最小高度。比桌面窗口高一些，尽量一屏放下登录表单。
const MOBILE_WINDOW_HEIGHT: f64 = 760.0;

/// 判断 User-Agent 是否属于移动端（决定独立窗口是否按手机尺寸与视口渲染）。
///
/// 抽成纯函数是为了能测：这段判断直接决定「H5 页面是否显示完整」，
/// 判错的症状与「站点改版」极难区分。
fn is_mobile_user_agent(ua: &str) -> bool {
    let ua = ua.to_ascii_lowercase();
    // `Mobile` 覆盖主流移动 Safari/Chrome；`Android`/`iPhone`/`iPad` 兜住
    // 不写 `Mobile` 的写法（平板、部分定制浏览器）。
    ua.contains("mobile")
        || ua.contains("android")
        || ua.contains("iphone")
        || ua.contains("ipad")
}

/// 独立窗口的**移动端视口修正**脚本（作为 `initialization_script` 注入）。
///
/// 只对移动 UA 的窗口注入。解决两类「H5 页显示不全 / 滑不动」：
///
/// 1. **视口宽度**：有些 H5 站只用 JS 按 UA 判端，HTML 里仍留着桌面的
///    `<meta name="viewport" content="width=1200">`；窗口只有 420px 宽时页面
///    按 1200px 渲染，右侧整块被裁掉。这里把 viewport 强制改成
///    `width=device-width`。
///
/// 2. **滚动被锁**：移动端 H5 惯用 `html,body{height:100%;overflow:hidden}`
///    加一个内部滚动容器，桌面 WebView2 没有手机的惯性滚动与地址栏收起行为，
///    内部容器高度算错就直接滑不动。这里在检测到「文档比视口高但滚不动」时
///    给 `html,body` 放开 `overflow:auto` 并允许纵向滚动。
///
/// 刻意写得保守：只在**确实滑不动**时才动样式，且只用 `overflow-y:auto`
/// （不改 `display`/`height`），避免把本来正常的固定布局页面弄乱。
fn mobile_viewport_script() -> &'static str {
    r##"
(function () {
  if (window.__readerMobileViewport) return;
  window.__readerMobileViewport = true;

  // 1) viewport：把任何固定宽度改成 device-width。
  function fixViewport() {
    var head = document.head || document.documentElement;
    if (!head) return;
    var meta = document.querySelector('meta[name="viewport"]');
    if (!meta) {
      meta = document.createElement('meta');
      meta.name = 'viewport';
      head.appendChild(meta);
    }
    var content = meta.getAttribute('content') || '';
    // 只改「显式固定宽度」的写法；`width=device-width` 保持原样。
    if (/width\s*=\s*\d+/.test(content) || content === '') {
      meta.setAttribute(
        'content',
        'width=device-width,initial-scale=1,maximum-scale=1,user-scalable=no,viewport-fit=cover'
      );
    }
  }

  // 2) 滚动：文档比视口高、却滑不动时放开 overflow。
  function unlockScroll() {
    var doc = document.documentElement;
    var body = document.body;
    if (!doc || !body) return;
    var viewportH = window.innerHeight || doc.clientHeight || 0;
    var contentH = Math.max(doc.scrollHeight || 0, body.scrollHeight || 0);
    if (contentH <= viewportH + 24) return; // 本来就不用滚
    var canScroll = (doc.scrollHeight - doc.clientHeight) > 24;
    if (canScroll) return; // 已经能滚
    var style = doc.style;
    if (getComputedStyle(doc).overflowY === 'hidden') style.overflowY = 'auto';
    if (getComputedStyle(body).overflowY === 'hidden') body.style.overflowY = 'auto';
    // 有些站把高度钉死在 body 上，一并放开（只动 height，不动其它布局属性）。
    if (getComputedStyle(body).height === viewportH + 'px') {
      body.style.height = 'auto';
      body.style.minHeight = viewportH + 'px';
    }
  }

  function run() {
    try { fixViewport(); } catch (e) {}
    try { unlockScroll(); } catch (e2) {}
  }

  if (document.readyState === 'loading') {
    document.addEventListener('DOMContentLoaded', run, { once: true });
  } else {
    run();
  }
  // SPA 会在路由切换后重建 DOM，多跑几次；开销可忽略。
  window.addEventListener('load', run, { once: true });
  setTimeout(run, 600);
  setTimeout(run, 1800);
})();
"##
}

/// 独立窗口的**加载进度条**（作为 `initialization_script` 注入）。
///
/// 上游站点常要几百毫秒到好几秒才出首屏，WebView2 在拿到 HTML 之前是一整片
/// 空底色 —— 用户看到的就是「点了没反应、黑屏卡住」。
///
/// 这里只画**顶部 3px 细进度条 + 一行「正在加载…」**，不遮盖页面：
/// 整层 `pointer-events: none`，页面一就绪就淡出移除。刻意不做全屏遮罩 ——
/// 遮罩反而让「慢」变成「挡」。
///
/// 为什么挂在 `initialization_script` 而不是 `on_page_load(Started)` 里 `eval`：
/// 前者在文档创建时同步执行，能保证**第一帧**就有东西画；后者要等 WebView2 建好
/// 再走一次 IPC，慢站点上仍会先闪一下空白。
fn external_window_progress_script() -> &'static str {
    r##"
(function () {
  if (window.__readerLoadProgress) return;
  window.__readerLoadProgress = true;
  function build() {
    if (!document.documentElement) return null;
    var wrap = document.createElement('div');
    wrap.id = '__reader-load-progress';
    wrap.style.cssText = 'position:fixed;left:0;top:0;right:0;z-index:2147483647;pointer-events:none;font:13px/1.5 system-ui,-apple-system,sans-serif';
    var track = document.createElement('div');
    track.style.cssText = 'height:3px;background:rgba(197,111,53,0.18);overflow:hidden';
    var bar = document.createElement('div');
    bar.style.cssText = 'height:100%;width:12%;background:#c56f35;transition:width .35s ease-out;border-radius:0 2px 2px 0';
    track.appendChild(bar);
    wrap.appendChild(track);
    var tip = document.createElement('div');
    tip.textContent = '\u6b63\u5728\u52a0\u8f7d\u2026';
    tip.style.cssText = 'position:fixed;left:50%;top:50%;transform:translate(-50%,-50%);color:#8a8f94;background:rgba(21,23,25,0.72);padding:8px 16px;border-radius:8px;white-space:nowrap';
    wrap.appendChild(tip);
    document.documentElement.appendChild(wrap);
    return { wrap: wrap, bar: bar };
  }
  var parts = build();
  if (!parts) return;
  var pct = 12;
  var timer = setInterval(function () {
    pct += Math.max(0.8, (90 - pct) * 0.09);
    if (pct > 90) pct = 90;
    parts.bar.style.width = pct + '%';
  }, 260);
  function done() {
    clearInterval(timer);
    parts.bar.style.width = '100%';
    setTimeout(function () {
      parts.wrap.style.transition = 'opacity .25s ease-out';
      parts.wrap.style.opacity = '0';
      setTimeout(function () { parts.wrap.remove(); }, 260);
    }, 180);
  }
  if (document.readyState === 'complete' || document.readyState === 'interactive') {
    done();
  } else {
    document.addEventListener('DOMContentLoaded', done, { once: true });
    setTimeout(done, 25000);
  }
})();
"##
}

#[tauri::command]
pub async fn open_external_window(
    state: tauri::State<'_, AppState>,
    app: tauri::AppHandle,
    url: String,
    title: Option<String>,
    book_source_url: Option<String>,
    // 等待确认的标识。非空表示这个窗口是 `startBrowserAwait` 打开的：
    // 关闭时必须回传「已完成」，唤醒后端阻塞中的书源脚本。
    await_panel_id: Option<String>,
) -> Result<ApiResponse<()>, AppError> {
    use tauri::Manager;

    let parsed = url::Url::parse(url.trim())
        .map_err(|_| AppError::BadRequest(format!("无效的网址: {url}")))?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return Err(AppError::BadRequest("只能打开网页地址".into()));
    }

    // 复用同一个窗口标签：连续点多个链接时只保留最新的那个，不堆窗口。
    const LABEL: &str = "source-link";
    if let Some(existing) = app.get_webview_window(LABEL) {
        let _ = existing.close();
    }

    // 让验证窗口用**书源自己的 User-Agent**，而不是 WebView2 的默认桌面 UA。
    //
    // 这是 Cloudflare 站点「验证能过、搜索仍然 403」的根因：`cf_clearance`
    // 由 Cloudflare 按 UA（连同 IP、TLS 会话）绑定，验证窗口与后续书源请求
    // 必须是**同一个 UA**，否则 token 被视为不匹配而失效。
    //
    // 书源 8045 把 UA 写成 `Mozilla/5.0 (Linux; Android 13; Pixel 7) … Mobile`，
    // 而 WebView2 默认是桌面 Chrome —— 用户在窗口里过完验证拿到 cf_clearance，
    // 书源请求换成移动 UA 后立刻被拦。
    //
    // legado 手机端正是这么做的：`WebViewActivity` 里
    // `headerMap[AppConst.UA_NAME]?.let { userAgentString = it }`。
    //
    // **必须在算窗口尺寸之前取到**：移动 UA 要把窗口收窄成手机宽度，见下。
    let window_ua = source_user_agent(&state.book_source_service, book_source_url.as_deref()).await;
    let is_mobile_ua = window_ua.as_deref().is_some_and(is_mobile_user_agent);

    // 尺寸按主窗口算：比它小一点，用户能看出这是「子窗口」。
    let mut width = 900.0_f64;
    let mut height = 700.0_f64;
    let mut x = None;
    let mut y = None;
    if let Some(main) = app.get_webview_window("main") {
        if let Ok(size) = main.inner_size() {
            let scale = main.scale_factor().unwrap_or(1.0);
            let logical_w = size.width as f64 / scale;
            let logical_h = size.height as f64 / scale;
            // 92% 且不超过 1200×900：大屏上不铺满，小屏上留出边距。
            width = (logical_w * 0.92).clamp(560.0, 1200.0);
            height = (logical_h * 0.92).clamp(420.0, 900.0);
        }
    }
    if is_mobile_ua {
        // 移动端 UA 要配移动窗口宽度：`fanqienovel.com` 这类 H5 站用
        // `<meta name="viewport" content="width=device-width">`，宽 900 的桌面
        // 窗口会让它按 900px 布局，正文/表单横向铺开、竖向又被固定高度的滚动
        // 容器卡住 —— 表现就是「H5 页面显示不全、也没法下滑」（书源 8054 的
        // 番茄登录页）。
        //
        // 手机端 legado 是整屏 WebView，天然是移动视口；桌面端要显式给窄窗口。
        // 只对**移动 UA** 收窄，桌面书源不受影响。
        width = width.min(MOBILE_WINDOW_WIDTH);
        height = height.max(MOBILE_WINDOW_HEIGHT);
    }
    if let Some(main) = app.get_webview_window("main") {
        if let (Ok(pos), Ok(size), Ok(scale)) =
            (main.outer_position(), main.inner_size(), main.scale_factor())
        {
            let logical_x = pos.x as f64 / scale;
            let logical_y = pos.y as f64 / scale;
            let logical_w = size.width as f64 / scale;
            let logical_h = size.height as f64 / scale;
            // 相对主窗口居中（用最终尺寸，收窄后也居中）。
            x = Some(logical_x + (logical_w - width) / 2.0);
            y = Some(logical_y + (logical_h - height) / 2.0 - 20.0);
        }
    }

    let mut builder =
        tauri::WebviewWindowBuilder::new(&app, LABEL, tauri::WebviewUrl::External(parsed.clone()))
            .title(title.unwrap_or_else(|| "书源网页".to_string()))
            .inner_size(width, height)
            .min_inner_size(420.0, 320.0)
            // 窗口底色取应用深色背景，避免 WebView2 首帧的白闪。
            .background_color(tauri::webview::Color(21, 23, 25, 255))
            // 加载进度条：文档创建时就挂上，慢站点也不会看起来像卡死。
            .initialization_script(external_window_progress_script());

    // 应用前面取到的书源 UA（移动 UA 同时注入视口修正脚本）。
    if let Some(ua) = window_ua {
        builder = builder
            .user_agent(&ua)
            .initialization_script(mobile_viewport_script());
    }

    // 站点里的二级链接（目录、详情、翻页、下载页）常带 `target="_blank"`。
    // 不接管这个回调时 Tauri 走默认实现，在 Windows 上把它们丢给系统浏览器 ——
    // 表现就是「窗口里点链接没反应，浏览器却弹出一个新标签」。
    //
    // 这里把二级链接**导航回本窗口**：这个窗口本来就只承载一个网页，
    // 首页 → 二级页 → 详情页在同一窗口里前进后退更像浏览器。
    // 返回 `Deny` 而不是 `Allow`：`Allow` 会新建 WebView2，而新窗口与主窗口
    // 各建一个 Environment（见 `0x8007139F` 的说明），很可能建不出来。
    {
        let app_for_links = app.clone();
        builder = builder.on_new_window(move |target, _features| {
            if matches!(target.scheme(), "http" | "https") {
                if let Some(window) = app_for_links.get_webview_window(LABEL) {
                    let _ = window.navigate(target);
                }
            }
            tauri::webview::NewWindowResponse::Deny
        });
    }

    // 非网页 scheme（`weixin://`、`alipays://`、`magnet:` 等）留给系统处理，
    // 否则 WebView2 会当成未知协议报错。网页内的导航一律放行 —— 这个窗口
    // 的用途就是让人浏览上游站点。
    builder = builder.on_navigation(|target| {
        matches!(
            target.scheme(),
            "http" | "https" | "about" | "data" | "blob" | "javascript"
        )
    });
    if let (Some(x), Some(y)) = (x, y) {
        builder = builder.position(x, y);
    }
    let window = builder
        .build()
        .map_err(|err| AppError::BadRequest(format!("打开窗口失败: {err}")))?;

    // 把书源 jar 里已有的 Cookie 注入窗口 —— 对应 legado `WebViewActivity` 里的
    // `AppCookieManager.applyToWebView(url)`。
    //
    // 用户第二次点「打开浏览器完成验证」时，窗口直接带上上次的 `cf_clearance`，
    // 不用再过一遍人机验证；站点按 Cookie 判定的已登录界面（会员中心/书架）
    // 也能正确显示。
    //
    // 必须在 UA 设好之后（见上面的 `builder.user_agent`）：Cloudflare 把
    // token 绑在 UA 上，UA 不对时带过去反而会让站点认定会话异常。
    //
    // 注意这里的写入发生在 `build()` 之后、页面开始请求**之前**——
    // `set_cookie` 是同步的 WebView2 `AddOrUpdateCookie`，不涉及消息泵，
    // 所以不会像 `cookies()` 那样在导航回调里死锁。
    if let Some(source_url) = book_source_url.as_deref().map(str::trim).filter(|v| !v.is_empty()) {
        let existing = state
            .book_service
            .source_session_cookies(source_url)
            .await
            .unwrap_or_default();
        if !existing.is_empty() {
            match crate::service::webview_cookies::write_into_window(&window, &existing) {
                Ok(count) => tracing::debug!(count, "已把书源 Cookie 注入验证窗口"),
                Err(err) => tracing::debug!(%err, "向验证窗口注入 Cookie 失败"),
            }
        }
    }

    // ── 等待方唤醒 ─────────────────────────────────────────────────────
    //
    // `startBrowserAwait` 打开的这个窗口，脚本正阻塞在
    // `await_external_window` 的 `recv_timeout` 上。用户关窗必须把它唤醒，
    // 否则书源脚本会挂满 30 分钟超时，后面「抓 Cookie / 上传凭证」全都不执行
    // （番茄登录的「保存失败」+ 诊断页不出现就是这么来的）。
    //
    // **顺序很关键**：必须等窗口 Cookie 写回 jar **之后**再唤醒。
    //
    // 书源拿到唤醒信号后第一件事就是 `java.getCookie(url)` 去读 jar
    // （番茄登录的 `_dpFqGrabCookie` 就是这样）。写回是异步的
    // （`set_source_window_cookies(...).await`），若在 `Destroyed` 里就同步唤醒，
    // 脚本会抢在写回之前读 jar —— 拿到空的 26 个域名，诊断页显示
    // 「非空域名数: 0 / 26」，登录白做。
    //
    // 所以唤醒句柄先存起来，由 Cookie 回收分支在写回完成后再触发；没有书源
    // 地址时（纯「看一眼就走」的窗口）退化为关窗即唤醒。
    let wake_on_close = await_panel_id
        .as_deref()
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(str::to_string);
    // 兜底：不回收 Cookie（没传书源地址）时，关窗就唤醒。
    if let Some(panel_id) = wake_on_close.clone().filter(|_| {
        book_source_url.as_deref().map(str::trim).unwrap_or("").is_empty()
    }) {
        let watched = window.clone();
        let notified = Arc::new(std::sync::atomic::AtomicBool::new(false));
        window.on_window_event(move |event| {
            if !matches!(event, tauri::WindowEvent::Destroyed) {
                return;
            }
            if notified.swap(true, std::sync::atomic::Ordering::SeqCst) {
                return;
            }
            let url = watched.url().map(|u| u.to_string()).unwrap_or_default();
            crate::service::source_browser::complete_external_window(&panel_id, &url);
        });
    }

    // 关闭时把该窗口里的 Cookie 收进 jar。只在**书源侧**登录时做：
    // 显式传了 `bookSourceUrl` 才写，避免把随便开的网页 Cookie 混进书源会话。
    if let Some(source_url) = book_source_url.map(|value| value.trim().to_string()) {
        if !source_url.is_empty() {
            let service = Arc::clone(&state.book_service);
            let watched = window.clone();
            // 关窗后要把「唤醒等待方」的信息带到 Cookie 写回完成之后。
            // 用 Arc<Mutex<..>> 是因为 `on_window_event` 的闭包是 `Fn`（可能被
            // 多次触发），而唤醒只能发生一次。
            let wake_slot = Arc::new(std::sync::Mutex::new(wake_on_close.clone()));
            // 「已经处理过关闭」的标志。
            //
            // 有它兜底：即使某条路径仍然用 `close()`（会再触发一次
            // `CloseRequested`），第二次进来也直接放行，不会再拦一遍。
            let handled = Arc::new(std::sync::atomic::AtomicBool::new(false));
            window.on_window_event(move |event| {
                // 必须在关闭**之前**读：Destroyed 触发时 WebView 已经销毁，
                // Cookie 与 localStorage 都读不出来。
                //
                // 这里要把关闭拦下来：localStorage 只能靠执行脚本读，而脚本
                // 求值是异步的（回调在事件循环线程上跑）。若就地同步等待，
                // 事件循环被自己堵住，回调永远不派发 —— 死锁。所以先
                // `prevent_close()`，读完后由我们自己关窗。
                let tauri::WindowEvent::CloseRequested { api, .. } = event else {
                    return;
                };
                // 已经处理过一次了：放行，别再拦。
                if handled.swap(true, std::sync::atomic::Ordering::SeqCst) {
                    return;
                }
                api.prevent_close();

                // Cookie 这条是同步的（wry 内部自己泵消息），先取。
                let cookies = match crate::service::webview_cookies::read_all(&watched) {
                    Ok(cookies) => std::sync::Arc::new(cookies),
                    Err(err) => {
                        tracing::warn!(%err, "读取登录窗口 Cookie 失败");
                        let _ = watched.destroy();
                        return;
                    }
                };

                // 顺带收走 localStorage：一类书源的段评页是 SPA，鉴权要
                // `X-Device-ID`（存 localStorage），而面板挂在 reader origin 下
                // 与上游隔离，取不到就现生成假的，服务端判定异常 → 面板空白。
                // 窗口跑在真实域名下，这里的值才是对的。
                //
                // 注意 `on_window_event` 的闭包是 `Fn`（可能被多次触发），
                // 因此这里每次都 clone 一份，不能把外层变量 move 进去。
                let service_for_read = Arc::clone(&service);
                let target_for_read = source_url.clone();
                let closing = watched.clone();
                let cookies_for_read = std::sync::Arc::clone(&cookies);
                let wake_for_read = std::sync::Arc::clone(&wake_slot);
                let spawned = crate::service::webview_cookies::read_local_storage_async(
                    &watched,
                    move |storage| {
                        // 用 `destroy()` 而不是 `close()`：后者会再触发一次
                        // `CloseRequested`，把「拦截→读取→关窗」再走一遍，
                        // 变成无限循环（窗口关不掉、日志刷屏）。
                        // `destroy()` 强制关闭且不发事件，正好是这里要的语义。
                        let _ = closing.destroy();
                        let cookies = std::sync::Arc::try_unwrap(cookies_for_read)
                            .unwrap_or_else(|shared| shared.as_ref().clone());
                        let page_url = closing.url().map(|u| u.to_string()).unwrap_or_default();
                        let wake = wake_for_read
                            .lock()
                            .unwrap_or_else(|e| e.into_inner())
                            .take()
                            .map(|panel_id| (panel_id, page_url));
                        persist_window_session(
                            service_for_read,
                            target_for_read,
                            cookies,
                            storage,
                            wake,
                        );
                    },
                );
                if let Err(err) = spawned {
                    tracing::warn!(%err, "读取登录窗口 localStorage 失败");
                    let page_url = watched.url().map(|u| u.to_string()).unwrap_or_default();
                    let _ = watched.destroy();
                    let fallback = std::sync::Arc::try_unwrap(cookies)
                        .unwrap_or_else(|shared| shared.as_ref().clone());
                    let wake = wake_slot
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .take()
                        .map(|panel_id| (panel_id, page_url));
                    persist_window_session(
                        Arc::clone(&service),
                        source_url.clone(),
                        fallback,
                        Vec::new(),
                        wake,
                    );
                }
            });
        }
    }

    Ok(ApiResponse::ok(()))
}

/// 把独立窗口收到的 Cookie / localStorage 写回书源会话。
///
/// `wake_await` 非空时，写回**完成之后**唤醒阻塞中的书源脚本
/// （`startBrowserAwait` 的等待方）。顺序不能颠倒：脚本被唤醒后立刻
/// `java.getCookie(url)` 读 jar，早一步唤醒就会读到空会话。
fn persist_window_session(
    service: Arc<crate::service::book_service::BookService>,
    source_url: String,
    cookies: Vec<crate::service::webview_cookies::CookieRecord>,
    storage: Vec<(String, String)>,
    wake_await: Option<(String, String)>,
) {
    if cookies.is_empty() && storage.is_empty() {
        // 没有可写的东西也要唤醒，否则脚本要空等 30 分钟。
        if let Some((panel_id, url)) = wake_await {
            crate::service::source_browser::complete_external_window(&panel_id, &url);
        }
        return;
    }
    tauri::async_runtime::spawn(async move {
        if !storage.is_empty() {
            service.set_source_window_storage(&source_url, &storage).await;
        }
        match service.set_source_window_cookies(&source_url, &cookies).await {
            Ok(0) => tracing::warn!("登录窗口未产生可写入的 Cookie"),
            Ok(count) => tracing::info!(count, "已把登录窗口的 Cookie 写回书源"),
            Err(err) => tracing::warn!(%err, "写入书源 Cookie 失败"),
        }
        // **写回之后**才唤醒等待方，保证脚本读到的是刚写进去的会话。
        if let Some((panel_id, page_url)) = wake_await {
            crate::service::source_browser::complete_external_window(&panel_id, &page_url);
        }
    });
}
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SetSourceCookieParam {
    pub book_source_url: Option<String>,
    pub cookie: Option<String>,
}

/// 手动导入书源登录 Cookie(如通过抓包获取的会话)。
/// Cookie 按完整书源地址隔离存储，后续仅该书源请求自动携带。
/// 存储前会用该书源做一次搜索校验,避免过期/无效 Cookie 毒化后续所有请求。
#[tauri::command]
pub async fn set_book_source_cookie(
    state: tauri::State<'_, AppState>,
    req: SetSourceCookieParam,
) -> Result<ApiResponse<serde_json::Value>, AppError> {
    let user_ns = "default";
    let url = req
        .book_source_url
        .ok_or_else(|| AppError::BadRequest("bookSourceUrl required".to_string()))?;
    let cookie = req
        .cookie
        .ok_or_else(|| AppError::BadRequest("cookie required".to_string()))?
        .trim()
        .to_string();
    // 先确认书源存在
    let source = state
        .book_source_service
        .get(&user_ns, &url)
        .await?
        .ok_or_else(|| AppError::NotFound("bookSource not found".to_string()))?;
    // 存储前校验: 带 cookie 跑一次搜索, 无效/过期则拒绝写入, 避免毒化。
    // 关键词同样走 check_keyword(默认"我的"), 不用"测试"——后者在很多站点
    // 搜不到任何书, 会让"Cookie 校验失败"误报。
    let validate_keyword = source.check_keyword();
    let validated = if !cookie.is_empty() {
        state.book_service.validate_source_cookie(&user_ns, &source, &cookie, &validate_keyword).await?
    } else {
        state.book_service.clear_source_cookie(&user_ns, &source.book_source_url).await;
        false
    };

    state
        .book_service
        .set_source_cookie(&user_ns, &source.book_source_url, &cookie)
        .await?;
    Ok(ApiResponse::ok(
        serde_json::json!({ "success": true, "saved": true, "validated": validated }),
    ))
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

    let info_map = req.info_map.unwrap_or_default();
    let kinds = state.book_service.explore_kinds_with_info(&source, &info_map)?;
    Ok(ApiResponse::ok(
        serde_json::to_value(kinds).unwrap_or_default(),
    ))
}

/// 执行发现页控件的 `action`（按钮点击 / 下拉选择 / 输入框防抖回调）。
///
/// action 脚本里的 `java.refreshExplore()`、`java.open(...)`、`java.searchBook(...)`
/// 会被捕获成意图标志返回，前端据此刷新分类、打开面板或发起搜索 —— 这些副作用
/// 无法在 WebView 侧凭空产生，必须由宿主执行。
#[tauri::command]
pub async fn run_explore_action(
    state: tauri::State<'_, AppState>,
    req: ExploreActionRequest,
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

    let action = req.action.trim();
    if action.is_empty() {
        return Ok(ApiResponse::ok(serde_json::json!({ "executed": false })));
    }

    let outcome = state
        .book_service
        .explore_action(&source, action, &req.info_map)?;
    Ok(ApiResponse::ok(
        serde_json::to_value(outcome).unwrap_or_default(),
    ))
}

#[tauri::command]
pub async fn test_book_sources(
    state: tauri::State<'_, AppState>,
    app: tauri::AppHandle,
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

    // 上限 8: 每个源在检测过程中会有一次阻塞解析(js: 规则里还可能发起同步 HTTP),
    // 并发越高, 取消时需要收尾的在途任务越多、峰值线程也越高。12 → 8 后,
    // 取消响应更快且不会把两轮 HTTP 超时叠在一起。
    let concurrent = req.concurrent.unwrap_or(8).clamp(1, 8);
    let keyword = req.keyword.clone();
    let mark_invalid = req.mark_invalid.unwrap_or(true);
    let task_id = req.task_id.filter(|id| !id.trim().is_empty());
    let cancel_flag = Arc::new(AtomicBool::new(false));
    if let Some(id) = task_id.as_ref() {
        if let Ok(mut tasks) = SOURCE_TEST_CANCELLATIONS.lock() {
            if let Some(previous) = tasks.insert(id.clone(), cancel_flag.clone()) {
                previous.store(true, Ordering::Relaxed);
            }
        }
        emit_source_test_progress(&app, id, sources.len(), 0, 0, 0, false);
    }

    let (outcomes, cancelled) = test_sources_in_parallel(
        state.book_service.clone(),
        user_ns.to_string(),
        keyword,
        sources,
        concurrent,
        cancel_flag,
        app,
        task_id.clone(),
    )
    .await;
    if let Some(id) = task_id.as_ref() {
        if let Ok(mut tasks) = SOURCE_TEST_CANCELLATIONS.lock() {
            tasks.remove(id);
        }
    }

    // 先把要落库的源收集起来, 循环结束后一次性并发写入。
    //
    // 原实现是在这个循环里逐条 `save().await`: 取消时要收尾的已测源可能有一两百个,
    // 每次都等一次 SQL 往返, 用户点了"中止"却还要盯着界面等好几秒。改成
    // 「收集 → 并发写」后, 写库总耗时从 O(N) 次串行往返降到约一次往返的量级。
    let mut pending_saves: Vec<BookSource> = Vec::new();
    let mut results = Vec::with_capacity(outcomes.len());
    let mut marked_invalid = 0usize;
    let mut cancelled_count = 0usize;
    for (mut source, availability) in outcomes {
        // 被取消的源绝不能按结果写库: 它的 valid 是 false(未检测完),
        // 走到下面会被 set_invalid_book_source_group 误标成"失效"并保存,
        // 把一个可能完全正常的书源标坏。只如实回报, 不改分组。
        if availability.cancelled {
            cancelled_count += 1;
            results.push(TestBookSourceItem {
                book_source_url: availability.book_source_url,
                book_source_name: availability.book_source_name,
                valid: false,
                search_ok: false,
                explore_ok: false,
                keyword: availability.keyword,
                explore_url: None,
                search_error: None,
                explore_error: None,
                marked_invalid: false,
                cancelled: true,
                group: source.book_source_group,
            });
            continue;
        }

        let changed = if mark_invalid {
            set_invalid_book_source_group(&mut source, !availability.valid)
        } else {
            false
        };
        if changed {
            if !availability.valid {
                marked_invalid += 1;
            }
            pending_saves.push(source.clone());
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
            cancelled: false,
            group: source.book_source_group,
        });
    }

    // 并发写回分组。这里**不能**在失败时立刻 `return Err`:
    // `JoinSet` 被 drop 时会 abort 所有剩余任务(tokio 的 Drop 实现是
    // `drain(|h| h.abort())`), 那样"已算好但还没落库"的源会被静默丢弃 ——
    // 用户点"中止"后本应照常写库, 结果这些源既没进 DB、又不会出现在失效列表里,
    // 连"删除失效"都删不掉, 只能重测。所以先全部写完, 再统一上报第一个错误。
    if !pending_saves.is_empty() {
        let mut writes = JoinSet::new();
        for source in pending_saves {
            let service = state.book_source_service.clone();
            let ns = user_ns.to_string();
            writes.spawn(async move { service.save(&ns, source).await });
        }
        let mut first_error: Option<AppError> = None;
        while let Some(joined) = writes.join_next().await {
            let error = match joined {
                Ok(Ok(())) => None,
                Ok(Err(err)) => Some(err),
                Err(err) => Some(AppError::Internal(anyhow::anyhow!(
                    "保存书源分组失败: {err}"
                ))),
            };
            // 只记第一个错误, 但循环继续跑完 —— 失败要上报, 其余写入不能因此中断。
            if let Some(err) = error {
                if first_error.is_none() {
                    first_error = Some(err);
                }
            }
        }
        if let Some(err) = first_error {
            return Err(err);
        }
    }

    results.sort_by(|a, b| a.book_source_name.cmp(&b.book_source_name));
    // 统计只算**已完成**的条目: 被取消的既不算有效也不算失效,
    // 否则中止会让前端看到虚高的"失效 N 个"并可能去点"删除失效"。
    let completed = results.iter().filter(|item| !item.cancelled).collect::<Vec<_>>();
    let valid = completed.iter().filter(|item| item.valid).count();
    let invalid = completed.len().saturating_sub(valid);
    let response = TestBookSourcesResponse {
        total: results.len(),
        valid,
        invalid,
        marked_invalid,
        cancelled: cancelled || cancelled_count > 0,
        cancelled_count,
        results,
    };
    Ok(ApiResponse::ok(
        serde_json::to_value(response).unwrap_or_default(),
    ))
}

#[tauri::command]
pub async fn cancel_book_source_test(
    req: CancelBookSourceTestRequest,
) -> Result<ApiResponse<serde_json::Value>, AppError> {
    let cancelled = SOURCE_TEST_CANCELLATIONS
        .lock()
        .ok()
        .and_then(|tasks| tasks.get(&req.task_id).cloned())
        .map(|flag| {
            flag.store(true, Ordering::Relaxed);
            true
        })
        .unwrap_or(false);
    Ok(ApiResponse::ok(serde_json::json!({ "cancelled": cancelled })))
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
    cancel_flag: Arc<AtomicBool>,
    app: tauri::AppHandle,
    task_id: Option<String>,
) -> (
    Vec<(
        BookSource,
        crate::service::book_service::BookSourceAvailability,
    )>,
    bool,
) {
    let total = sources.len();
    let mut pending = sources.into_iter();
    let mut tasks = JoinSet::new();

    let spawn_next = |tasks: &mut JoinSet<_>, source: BookSource| {
        let book_service = book_service.clone();
        let user_ns = user_ns.clone();
        let keyword = keyword.clone();
        // 把取消信号一路传进单源检测: 解析段在阻塞线程池里跑, 在途 HTTP 用
        // select! 包裹, 取消时 future 被 drop → 连接真断, 而不是等 30s 超时。
        let cancel = cancel_flag.clone();
        tasks.spawn(async move {
            let availability = book_service
                .test_book_source_availability_cancellable(
                    &user_ns,
                    &source,
                    keyword.as_deref(),
                    Some(&cancel),
                )
                .await;
            (source, availability)
        });
    };

    for source in pending.by_ref().take(concurrent.max(1)) {
        spawn_next(&mut tasks, source);
    }

    let mut outcomes = Vec::with_capacity(tasks.len());
    let mut valid = 0usize;
    let mut invalid = 0usize;
    let mut cancelled = false;
    while !tasks.is_empty() {
        if cancel_flag.load(Ordering::Relaxed) {
            cancelled = true;
            tasks.abort_all();
            // 取消后不再等被 abort 的任务: 它们正卡在阻塞线程池的解析段里,
            // 旧实现 `while join_next().await.is_some() {}` 会一直等到它们跑完,
            // 这正是「前端已取消、后端还在检测」的来源。这里直接丢弃句柄。
            break;
        }

        tokio::select! {
            result = tasks.join_next() => {
                if let Some(result) = result {
                    match result {
                        Ok(outcome) => {
                            // 被取消的单源不计入有效/失效: 它的结果不完整,
                            // 计进去会让"失效数"虚高并触发误标失效。
                            if !outcome.1.cancelled {
                                if outcome.1.valid { valid += 1; } else { invalid += 1; }
                            }
                            outcomes.push(outcome);
                            if let Some(id) = task_id.as_deref() {
                                emit_source_test_progress(
                                    &app,
                                    id,
                                    total,
                                    outcomes.len(),
                                    valid,
                                    invalid,
                                    false,
                                );
                            }
                            if let Some(source) = pending.next() {
                                spawn_next(&mut tasks, source);
                            }
                        }
                        Err(err) if !err.is_cancelled() => {
                            tracing::error!("book source test task failed: {err}")
                        }
                        Err(_) => {}
                    }
                }
            }
            _ = tokio::time::sleep(Duration::from_millis(100)) => {}
        }
    }

    if let Some(id) = task_id.as_deref() {
        emit_source_test_progress(
            &app,
            id,
            total,
            outcomes.len(),
            valid,
            invalid,
            cancelled,
        );
    }
    (outcomes, cancelled)
}

fn emit_source_test_progress(
    app: &tauri::AppHandle,
    task_id: &str,
    total: usize,
    completed: usize,
    valid: usize,
    invalid: usize,
    cancelled: bool,
) {
    let _ = app.emit(
        "book-source-test-progress",
        BookSourceTestProgress {
            task_id: task_id.to_string(),
            total,
            completed,
            valid,
            invalid,
            cancelled,
        },
    );
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
    // 一条 SQL 批量删除, 不再逐条 await(删几百条时那是几百次往返)。
    let deleted = state
        .book_source_service
        .delete_many(&user_ns, &invalid_urls)
        .await?;
    state
        .book_service
        .remove_source_candidates(&user_ns, &invalid_urls.iter().cloned().collect())
        .await?;
    // 失效源删除时一并清理登录 Cookie, 避免坏 Cookie 留在内存里继续毒化。
    // 批量版只各取一次写锁(逐条调用是 3N 次锁竞争)。
    state
        .book_service
        .clear_source_cookies(&user_ns, &invalid_urls)
        .await;
    // 用 DB 实际删除行数而不是"待删列表长度": 两者不一致时前者才是真相。
    Ok(ApiResponse::ok(serde_json::json!({
        "deleted": deleted
    })))
}

/// `delete_book_source` 的共享实现 (Tauri command 与 Web HTTP handler 共用)。
pub async fn delete_book_source_core(
    state: &AppState,
    req: BookSourceUrlParam,
) -> Result<ApiResponse<serde_json::Value>, AppError> {
    let user_ns = "default";
    let url = req
        .book_source_url
        .ok_or_else(|| AppError::BadRequest("bookSourceUrl required".to_string()))?;
    state.book_source_service.delete(&user_ns, &url).await?;
    state
        .book_service
        .remove_source_candidates(&user_ns, &HashSet::from([url.clone()]))
        .await?;
    state.book_service.clear_source_cookie(&user_ns, &url).await;
    // 书源已从库里删掉，它的会话目录之后再没有任何读取路径 —— 连目录一起回收，
    // 否则 `source-state/<md5>` 会一直攒下去。
    state.book_service.remove_source_state(&user_ns, &url).await;
    Ok(ApiResponse::ok(serde_json::json!({"deleted": true})))
}

/// `delete_book_source` 的 Tauri 入口: 只是 core 的薄壳。
#[tauri::command]
pub async fn delete_book_source(
    state: tauri::State<'_, AppState>,
    req: BookSourceUrlParam,
) -> Result<ApiResponse<serde_json::Value>, AppError> {
    delete_book_source_core(&state, req).await
}

/// `delete_book_sources` 的共享实现 (Tauri command 与 Web HTTP handler 共用)。
pub async fn delete_book_sources_core(
    state: &AppState,
    req: Vec<BookSourceUrlParam>,
) -> Result<ApiResponse<serde_json::Value>, AppError> {
    let user_ns = "default";
    let mut deleted_urls = HashSet::new();
    for item in req {
        if let Some(url) = item.book_source_url {
            deleted_urls.insert(url);
        }
    }
    // 与"删除失效"同样改成批量: 逐个 await 删除 + 逐个清 Cookie 是 4N 次往返/锁。
    let deleted_list = deleted_urls.iter().cloned().collect::<Vec<_>>();
    state
        .book_source_service
        .delete_many(&user_ns, &deleted_list)
        .await?;
    state
        .book_service
        .remove_source_candidates(&user_ns, &deleted_urls)
        .await?;
    state
        .book_service
        .clear_source_cookies(&user_ns, &deleted_list)
        .await;
    state
        .book_service
        .remove_source_states(&user_ns, &deleted_list)
        .await;
    Ok(ApiResponse::ok(serde_json::json!({"deleted": true})))
}

/// `delete_book_sources` 的 Tauri 入口: 只是 core 的薄壳。
#[tauri::command]
pub async fn delete_book_sources(
    state: tauri::State<'_, AppState>,
    req: Vec<BookSourceUrlParam>,
) -> Result<ApiResponse<serde_json::Value>, AppError> {
    delete_book_sources_core(&state, req).await
}

#[tauri::command]
pub async fn delete_all_book_sources(
    state: tauri::State<'_, AppState>,
) -> Result<ApiResponse<serde_json::Value>, AppError> {
    let user_ns = "default";
    let deleted_urls = state
        .book_source_service
        .list(&user_ns)
        .await?
        .into_iter()
        .map(|source| source.book_source_url)
        .collect::<HashSet<_>>();
    state.book_source_service.delete_all(&user_ns).await?;
    state
        .book_service
        .remove_source_candidates(&user_ns, &deleted_urls)
        .await?;
    // 清空书源时一并清空全部登录 Cookie(批量, 避免 N 次写锁竞争)。
    let deleted_list = deleted_urls.iter().cloned().collect::<Vec<_>>();
    state
        .book_service
        .clear_source_cookies(&user_ns, &deleted_list)
        .await;
    state
        .book_service
        .remove_source_states(&user_ns, &deleted_list)
        .await;
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
        // 单个书源对象直接导入(与复制粘贴保存的惯用格式一致)
        return Ok(vec![book_source_from_value(payload)
            .map_err(|e| AppError::BadRequest(e.to_string()))?]);
    }
    Err(AppError::BadRequest(
        "invalid book sources payload".to_string(),
    ))
}

async fn invalidate_shelf_book_source_caches(
    state: &AppState,
    user_ns: &str,
) -> Result<(), AppError> {
    let books = state.book_service.get_bookshelf(user_ns).await?;
    for book in books {
        state
            .book_service
            .delete_book_sources_cache(user_ns, &book.book_url)
            .await?;
    }
    Ok(())
}

fn source_became_available(existing: Option<&BookSource>, incoming: &BookSource) -> bool {
    incoming.is_enabled() && match existing {
        Some(source) => !source.is_enabled(),
        None => true,
    }
}

#[derive(Debug, Deserialize)]
pub struct RemoteSourceParam {
    url: String,
}

/// 从 `legado://` / `yuedu://` 一键导入链接里取出真正的 http(s) 地址。
///
/// 手机端这些是系统协议, PC 端没有注册, 用户在书源管理里直接粘贴整条链接
/// 是自然用法 —— 直接把它交给 reqwest 会得到 `BadScheme`。这里按
/// `?src=` / `?url=` 取值, 并只接受 http(s)(导入地址来自第三方网页,
/// 不能让 `file:` 之类借道读取本地文件)。
fn resolve_remote_source_url(raw: &str) -> Result<String, String> {
    let trimmed = raw.trim();
    let lower = trimmed.to_ascii_lowercase();
    if !(lower.starts_with("legado://") || lower.starts_with("yuedu://")) {
        // 普通 http(s) 地址: 原样使用, 保持既有行为。
        if lower.starts_with("http://") || lower.starts_with("https://") {
            return Ok(trimmed.to_string());
        }
        return Err("请输入 http(s) 或 legado:// / yuedu:// 开头的一键导入链接".to_string());
    }

    let query = trimmed.split_once('?').map(|(_, q)| q).unwrap_or("");
    let mut candidate: Option<String> = None;
    for pair in query.split('&') {
        let Some((key, value)) = pair.split_once('=') else { continue };
        if key == "src" || key == "url" {
            let decoded = urlencoding::decode(value).map(|v| v.into_owned()).unwrap_or_else(|_| value.to_string());
            candidate = Some(decoded);
            break;
        }
    }

    let Some(candidate) = candidate else {
        return Err("这个导入链接里没有 src 参数".to_string());
    };
    let candidate = candidate.trim();
    let candidate_lower = candidate.to_ascii_lowercase();
    if !(candidate_lower.starts_with("http://") || candidate_lower.starts_with("https://")) {
        return Err("导入链接里的地址必须是 http(s)".to_string());
    }
    Ok(candidate.to_string())
}

#[tauri::command]
pub async fn read_remote_source_file(
    state: tauri::State<'_, AppState>,
    req: RemoteSourceParam,
) -> Result<ApiResponse<Vec<String>>, AppError> {
    let client = state.book_service.http_client();
    let target = resolve_remote_source_url(&req.url).map_err(AppError::BadRequest)?;

    let text = client
        .get(&target)
        .send()
        .await
        .map_err(|e| {
            tracing::error!("Failed to fetch remote source from {}: {:?}", target, e);
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
    request: tauri::ipc::Request<'_>,
) -> Result<ApiResponse<serde_json::Value>, AppError> {
    // 扩展名判断先于 body 解码：与原实现一致，非法扩展名一律返回
    // "No json file uploaded"，而不是先报 body 形式的错。
    let file_name = decode_header_file_name(&request)?;
    if file_name.ends_with(".json") || file_name.ends_with(".txt") {
        let file = decode_upload_body(request.body())?;
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
    use serde_json::json;

    #[test]
    fn requested_source_urls_rejects_batches_above_limit() {
        let urls = (0..101)
            .map(|index| format!("https://source-{index}.example"))
            .collect::<Vec<_>>();

        let err = normalize_requested_source_urls(Some(&urls)).unwrap_err();

        assert!(matches!(err, AppError::BadRequest(message) if message.contains("100")));
    }

    /// 书源管理里的「网络导入」: 用户直接粘贴源仓库的一键导入链接是自然用法,
    /// 直接交给 reqwest 会得到 `BadScheme`。这里必须把 src 取出来。
    #[test]
    fn resolve_remote_source_url_unwraps_import_links() {
        assert_eq!(
            resolve_remote_source_url(
                "yuedu://booksource/importonline?src=https://repo.example.com/yuedu/shuyuan/json/id/1.json"
            )
            .unwrap(),
            "https://repo.example.com/yuedu/shuyuan/json/id/1.json"
        );
        assert_eq!(
            resolve_remote_source_url(
                "legado://import/booksource?src=https%3A%2F%2Fexample.com%2Fs.json"
            )
            .unwrap(),
            "https://example.com/s.json"
        );
        // 普通地址与前后空白照常处理。
        assert_eq!(
            resolve_remote_source_url("  https://example.com/a.json ").unwrap(),
            "https://example.com/a.json"
        );
    }

    #[test]
    fn resolve_remote_source_url_rejects_unsafe_input() {
        // 非 http(s) 目标不能被导入链接借道(避免读取本地文件)。
        assert!(resolve_remote_source_url("legado://import/x?src=file:///C:/a.json").is_err());
        assert!(resolve_remote_source_url("legado://import/x").is_err());
        assert!(resolve_remote_source_url("mailto:a@b.c").is_err());
    }

    #[test]
    fn extract_sources_accepts_single_source_object() {
        let payload = json!({
            "bookSourceName": "single",
            "bookSourceUrl": "https://single.example",
            "ruleSearch": {"bookList": ".item"}
        });
        let sources = extract_sources(payload).expect("single object should import");
        assert_eq!(sources.len(), 1);
        assert_eq!(sources[0].book_source_name, "single");
    }

    #[test]
    fn extract_sources_accepts_array() {
        let payload = json!([
            {"bookSourceName": "a", "bookSourceUrl": "https://a.example"},
            {"bookSourceName": "b", "bookSourceUrl": "https://b.example"}
        ]);
        let sources = extract_sources(payload).expect("array should import");
        assert_eq!(sources.len(), 2);
    }

    #[test]
    fn extract_sources_accepts_wrapper_key() {
        let payload = json!({
            "bookSourceList": [
                {"bookSourceName": "wrapped", "bookSourceUrl": "https://wrapped.example"}
            ]
        });
        let sources = extract_sources(payload).expect("wrapped list should import");
        assert_eq!(sources.len(), 1);
        assert_eq!(sources[0].book_source_name, "wrapped");
    }

    #[test]
    fn extract_sources_rejects_invalid_payload() {
        // 非对象/数组(字符串、数字等)不是合法书源载荷
        let err = extract_sources(json!("just a string")).unwrap_err();
        assert!(matches!(err, AppError::BadRequest(_)));
        let err = extract_sources(json!(42)).unwrap_err();
        assert!(matches!(err, AppError::BadRequest(_)));
    }

    #[test]
    fn source_became_available_only_for_new_or_reenabled_sources() {
        let enabled = BookSource {
            book_source_url: "https://source.example".to_string(),
            enabled: Some(true),
            ..BookSource::default()
        };
        let disabled = BookSource {
            enabled: Some(false),
            ..enabled.clone()
        };

        assert!(source_became_available(None, &enabled));
        assert!(!source_became_available(None, &disabled));
        assert!(source_became_available(Some(&disabled), &enabled));
        assert!(!source_became_available(Some(&enabled), &enabled));
    }

    /// 移动 UA 要能被认出来 —— 它决定独立窗口是否按手机尺寸渲染。
    ///
    /// 书源 8054 的番茄登录页就是移动 H5：UA 判定失败时窗口按 900px 桌面宽度
    /// 渲染，页面显示不全、也滑不动。
    #[test]
    fn detects_mobile_user_agents() {
        for ua in [
            // 8054 / 8045 用的就是这两种
            "Mozilla/5.0 (Linux; Android 13; SM-S918B) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Mobile Safari/537.36",
            "Mozilla/5.0 (Linux; Android 13; Pixel 7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Mobile Safari/537.36",
            "Mozilla/5.0 (iPhone; CPU iPhone OS 17_0 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.0 Mobile/15E148 Safari/604.1",
            "Mozilla/5.0 (iPad; CPU OS 17_0 like Mac OS X) AppleWebKit/605.1.15",
            // 不写 Mobile 的安卓浏览器
            "Mozilla/5.0 (Linux; Android 10) AppleWebKit/537.36",
            // 大小写不应影响判定
            "mozilla/5.0 (linux; android 12) mobile safari",
        ] {
            assert!(is_mobile_user_agent(ua), "应判为移动端: {ua}");
        }
    }

    /// 桌面 UA 不能被误判 —— 否则正常网页会被塞进 420px 窄窗口。
    #[test]
    fn does_not_treat_desktop_user_agents_as_mobile() {
        for ua in [
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36",
            "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36",
            "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36",
            "",
        ] {
            assert!(!is_mobile_user_agent(ua), "不应判为移动端: {ua}");
        }
    }

    /// 视口脚本必须同时处理「固定宽度 viewport」与「滚动被锁」两件事。
    #[test]
    fn mobile_viewport_script_covers_both_failure_modes() {
        let script = mobile_viewport_script();
        // 1) 把 width=<数字> 改成 device-width
        assert!(script.contains("device-width"), "应改写 viewport 为 device-width");
        // 2) 放开被锁死的纵向滚动
        assert!(script.contains("overflowY"), "应处理 overflow 锁死");
        assert!(script.contains("scrollHeight"), "应检测文档是否比视口高");
        // 幂等：重复注入不应叠加
        assert!(script.contains("__readerMobileViewport"));
    }
}
