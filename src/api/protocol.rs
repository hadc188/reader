//! Custom `reader` URI-scheme handler.
//!
//! Serves cover images, EPUB assets, uploaded files and the bookSourceProxy
//! login iframe over the reader scheme so `<img>`/`<iframe>` in the frontend
//! keep loading synchronously without async blob-URL plumbing.
//!
//! The scheme origin is platform-dependent: WebView2 (Windows) serves custom
//! schemes as `http://reader.localhost`, WebKitGTK (Linux) and WKWebView
//! (macOS) as `reader://localhost`. See [`reader_scheme_origin`].

use crate::api::AppState;
use crate::error::error::AppError;
use crate::model::book_source::BookSource;
use crate::util::text::{normalize_source_url, repair_encoded_url};
use regex::{Captures, Regex};
use std::collections::HashMap;
use tauri::http;
use tauri::Manager;
use url::Url;

/// Origin of the custom `reader` URI scheme, which differs per webview
/// backend: WebView2 (Windows/Android) serves custom schemes as
/// `http://<scheme>.localhost`, WebKitGTK/WKWebView (Linux/macOS) as
/// `<scheme>://localhost`. Mirrors the mapping in Tauri's injected
/// `convertFileSrc` used by the frontend (`frontend/src/api/scheme.ts`).
pub fn reader_scheme_origin() -> &'static str {
    if cfg!(any(target_os = "windows", target_os = "android")) {
        "http://reader.localhost"
    } else {
        "reader://localhost"
    }
}

/// Entry point registered via `register_asynchronous_uri_scheme_protocol("reader", ...)`.
pub fn handle_reader_scheme(
    ctx: tauri::UriSchemeContext<'_, tauri::Wry>,
    request: tauri::http::Request<Vec<u8>>,
    responder: tauri::UriSchemeResponder,
) {
    let path = request.uri().path().to_string();
    let query = request.uri().query().unwrap_or_default().to_string();
    let app_handle = ctx.app_handle().clone();

    tauri::async_runtime::spawn(async move {
        let state = app_handle.state::<AppState>();
        let response = route(&state, &path, &query, request).await;
        responder.respond(response);
    });
}

async fn route(
    state: &AppState,
    path: &str,
    query: &str,
    request: tauri::http::Request<Vec<u8>>,
) -> tauri::http::Response<Vec<u8>> {
    match path {
        "/sourcePanelBridge" => source_panel_bridge_route(state, query, &request).await,
        "/sourcePanel" => source_panel_route(state, query, &request).await,
        "/cover" => cover_route(state, query).await,
        "/epub" => epub_route(state, query).await,
        "/pdf" => pdf_route(state, query).await,
        "/files" => files_route(state, query).await,
        // 其余路径一律 404。
        //
        // 这里**曾经**有登录页代理（`/__login/<session>/<上游路径>`、
        // `/loginCookieBridge`、`/bookSourceProxy`）：登录页跑在 iframe 里，
        // 会话只能编码进路径，还要注入脚本改写 `src`/`href`、劫持
        // `document.cookie` 才能把登录态收回书源 jar。
        //
        // 现在登录页改用**独立窗口**直接加载上游地址（`open_external_window`），
        // 页面在自己的域下运行，Cookie 由 WebView2 原生管理、关窗时经
        // `ICoreWebView2CookieManager` 读出写回 jar —— 上述整条链路不再需要。
        _ => http::Response::builder()
            .status(404)
            .body(Vec::new())
            .unwrap_or_default(),
    }
}

// ─────────────────────────── 书源面板 (showBrowser) ───────────────────────────

/// `${readerOrigin}/sourcePanel?__readerPanelId=...`: 面板 HTML(已注入宿主桥)。
///
/// 面板由后端托管而不是用 iframe `srcdoc`, 因为 `srcdoc` 是 `about:srcdoc` 源,
/// 无法同源访问 `/sourcePanelBridge`; 挂在 `reader.localhost` 下才能同步回调宿主。
async fn source_panel_route(
    state: &AppState,
    query: &str,
    request: &tauri::http::Request<Vec<u8>>,
) -> tauri::http::Response<Vec<u8>> {
    let params = parse_query(query);
    // Keep transport fields separate from the upstream page's own query.
    let namespaced = params.contains_key("__readerPanelId");
    let Some(panel_id) = params.get(if namespaced { "__readerPanelId" } else { "panelId" }) else {
        return text_response(400, "missing panelId");
    };
    let Some(entry) = crate::service::book_service::get_source_panel(panel_id) else {
        return text_response(404, "panel not found");
    };
    let target = params.get(if namespaced { "__readerPanelUrl" } else { "url" });
    // 面板页 JS 改写自身 URL 的 query 后整页导航时(某聚合源的登录页的
    // `sw('register')`: `new URL(location.href)` + searchParams.set +
    // location.href), 改动只落在代理 URL 的外层 query 上 —— 桥脚本的
    // proxyUrl() 把上游 query 复制到外层, 且初始加载会按设计删掉
    // __readerPanelUrl。只认 __readerPanelUrl 会让这类导航退回改写前的 URL,
    // 页面永远停在原状态(点注册始终停在登录页)。
    //
    // 注意这里**不改变**抓取/静态渲染的判据: 存有面板 HTML 的形式
    // (showBrowser(url, html), 如内嵌设置页)仍只在显式导航(target 存在)时
    // 抓取上游, 其余继续静态渲染, 保持既有行为。合并只影响进入 fetch 分支后
    // 实际请求的上游地址。
    let base = target.map(String::as_str).unwrap_or(&entry.url);
    let merged = merge_panel_outer_query(base, &params, namespaced);
    let navigation = merged.as_deref().or(target.map(String::as_str));
    if target.is_some() || (entry.html.is_empty() && !entry.url.starts_with("data:")) {
        return match fetch_source_panel_page(state, panel_id, &entry, navigation, request).await {
            Ok(response) => response,
            Err(error) => text_response(502, &error.to_string()),
        };
    }
    let html = render_source_panel_page(panel_id, &entry, &entry.url, &entry.html);
    http::Response::builder()
        .status(200)
        .header(http::header::CONTENT_TYPE, "text/html; charset=utf-8")
        .header(http::header::CACHE_CONTROL, "no-store")
        .body(html.into_bytes())
        .unwrap_or_else(|_| http::Response::new(Vec::new()))
}

/// 把代理 URL 外层 query 合并进上游 URL。
///
/// 面板内 JS 用 `new URL(location.href)` + `searchParams.set` + `location.href`
/// 这类整页导航改写自身 query 时(某聚合源的登录页的 `sw('register')`), 改动只会
/// 落在代理 URL 的外层 query 上, 而 `__readerPanelUrl` 仍是改写前的地址。不透传
/// 就会永远停在原状态(点注册始终回到登录页)。
///
/// 合并采用「外层覆盖内层」语义: 同为 `mode` 时以外层为准, 其余上游参数保留。
/// 返回 `None` 表示无需合并(没有额外参数、外层与 base 一致、非 http(s)、或 base
/// 不是绝对 URL), 调用方沿用原 target。
fn merge_panel_outer_query(
    base: &str,
    params: &HashMap<String, String>,
    namespaced: bool,
) -> Option<String> {
    let mut url = Url::parse(base).ok()?;
    if !matches!(url.scheme(), "http" | "https") {
        return None;
    }
    // 只排除本协议的传输字段。namespaced 形式下 `url`/`panelId` 可能是上游页面
    // 自己的业务参数, 不能一并丢掉。
    let transport: [&str; 2] = if namespaced {
        ["__readerPanelId", "__readerPanelUrl"]
    } else {
        ["panelId", "url"]
    };
    let mut outer: Vec<(&str, &str)> = params
        .iter()
        .filter(|(key, _)| !transport.contains(&key.as_str()))
        .map(|(key, value)| (key.as_str(), value.as_str()))
        .collect();
    // `params` 是 HashMap, 迭代序随机; 排序让追加顺序可复现(日志、缓存键)。
    outer.sort_unstable();
    if outer.is_empty() {
        return None;
    }

    let existing: Vec<(String, String)> = url
        .query_pairs()
        .map(|(key, value)| (key.into_owned(), value.into_owned()))
        .collect();
    // 面板内导航的常态是外层 query 复制自上游(`proxyUrl` 的 `proxy.search = url.search`),
    // 此时每个外层参数都已在 base 中原样存在, 无事可做 —— 返回 `None` 让调用方
    // 沿用原 target, 避免 query 解码/重编码往返(如 `%20` → `+`、`%zz` → `%25zz`)
    // 改写上游 URL 形式。上游按 query 原文签名/比对时, 这种改写会让请求失配。
    let pending: Vec<(&str, &str)> = outer
        .into_iter()
        .filter(|&(key, value)| {
            !existing
                .iter()
                .any(|(name, current)| name.as_str() == key && current.as_str() == value)
        })
        .collect();
    if pending.is_empty() {
        return None;
    }

    // 纯追加(无同名冲突, 即该页点注册这类场景)时直接在原 query 后拼接,
    // 保留既有片段原文 —— `query_pairs` 重建会把 `%20` 变 `+`、`%zz` 变
    // `%25zz`, 上游按 query 原文签名/比对时会失配。
    let overrides = pending
        .iter()
        .any(|(key, _)| existing.iter().any(|(name, _)| name.as_str() == *key));
    if !overrides {
        let mut query = url.query().unwrap_or_default().to_string();
        for (key, value) in &pending {
            if !query.is_empty() {
                query.push('&');
            }
            query.push_str(&urlencoding::encode(key));
            query.push('=');
            query.push_str(&urlencoding::encode(value));
        }
        url.set_query(Some(&query));
        return Some(url.to_string());
    }

    // 与桥脚本的 `searchParams.set` 一致: 同名参数先整体删除再追加, 而不是只覆盖
    // 第一项 —— 上游 `?a=1&a=2` 遇外层 `a=3` 应得 `?a=3`, 不是 `?a=3&a=2`。
    let mut merged: Vec<(String, String)> = existing
        .into_iter()
        .filter(|(name, _)| !pending.iter().any(|(key, _)| name == key))
        .collect();
    for (key, value) in pending {
        merged.push((key.to_string(), value.to_string()));
    }
    url.set_query(None);
    {
        let mut pairs = url.query_pairs_mut();
        for (key, value) in &merged {
            pairs.append_pair(key, value);
        }
    }
    Some(url.to_string())
}

/// 组装面板页: 面板 HTML + 注入的宿主桥脚本。面板不存在时返回 `None`。
pub fn build_source_panel_page(panel_id: &str) -> Option<String> {
    let entry = crate::service::book_service::get_source_panel(panel_id)?;
    Some(render_source_panel_page(panel_id, &entry, &entry.url, &entry.html))
}

fn script_string(text: &str) -> String {
    serde_json::to_string(text).unwrap_or_default().replace('<', "\\u003c")
}

fn render_source_panel_page(panel_id: &str, entry: &crate::service::book_service::SourcePanelEntry, page_url: &str, html: &str) -> String {
    let jar_cookie = entry.http.as_ref()
        .and_then(|http| url::Url::parse(page_url).ok().map(|url| http.session.cookies.get(&url)))
        .unwrap_or_default();
    render_source_panel_page_with_cookie(panel_id, entry, page_url, html, &jar_cookie)
}

/// `jar_cookie`: 书源 cookie jar 中该页面域的 cookie 串 (`k=v; k2=v2`)。
/// 面板页面 JS 常用 `document.cookie` 判断登录态（如某书源 `/user` 的
/// `getCookie('qttoken')`）。自定义协议 origin 下 `Set-Cookie` 响应头
/// 未必被 WebView 落盘，这里直接在页面脚本之前注入并劫持
/// `document.cookie` 的 getter（jar 优先合并），保证入口检查可读到。
fn render_source_panel_page_with_cookie(panel_id: &str, entry: &crate::service::book_service::SourcePanelEntry, page_url: &str, html: &str, jar_cookie: &str) -> String {
    let page_url = if page_url.starts_with("data:") { "" } else { page_url };
    let mut bridge = format!("<script>window.__readerPanelUrl={};window.__readerSourceKey={};</script>", script_string(page_url), script_string(&entry.book_source_url));
    let panel_url = script_string(page_url);
    // 即使 jar 里还没有 cookie 也要注入: 劫持的 setter 负责把页面写入的
    // document.cookie **回写书源 jar**。登录脚本用 `document.cookie = 'user_token=…'`
    // 记录会话, 而上游后续页面(/user_center 等)由**服务端**读 Cookie 判断登录态 ——
    // 只写浏览器的话这份 cookie 留在面板 origin(reader.localhost)下, 转发请求带不上,
    // 上游就会当作未登录并把页面踢回登录页(表现为「登录过期」)。
    if page_url.starts_with("http://") || page_url.starts_with("https://") {
        bridge.push_str(&format!(
            r#"<script>
(function () {{
  var jarCookie = {};
  var pageUrl = {};
  var pairs = jarCookie.split(/;\s*/).filter(function (p) {{ return p.indexOf('=') > 0; }});
  var jar = {{}};
  pairs.forEach(function (p) {{
    var i = p.indexOf('=');
    jar[p.slice(0, i).trim()] = p.slice(i + 1).trim();
  }});
  var desc = Object.getOwnPropertyDescriptor(Document.prototype, 'cookie')
    || Object.getOwnPropertyDescriptor(HTMLDocument.prototype, 'cookie')
    || Object.getOwnPropertyDescriptor(document, 'cookie');
  if (!desc || !desc.get || !desc.set) return;
  // 先尽力把 jar cookie 写进浏览器(部分环境可写, 子资源请求也能带上)
  pairs.forEach(function (p) {{
    try {{ desc.set.call(document, p.trim() + '; path=/'); }} catch (e) {{}}
  }});
  var nativeGet = desc.get, nativeSet = desc.set;
  // 页面写入的 cookie 回写书源 jar: 上游靠服务端读 Cookie 校验登录态, 只有落到
  // jar 里才能随转发请求送达。异步桥不可用(setter 是同步属性), 因此等宿主桥
  // 注入后用其中的同步桥提交; 桥尚未就绪时排队, 就绪后补发。
  var pending = [];
  function flush() {{
    if (!window.__readerPanelBridgeReady || !pending.length) return;
    var queued = pending; pending = [];
    queued.forEach(function (pair) {{
      try {{ window.__readerPanelCookieSet(pageUrl, pair); }} catch (e) {{}}
    }});
  }}
  window.__readerPanelQueueCookie = function (pair) {{ pending.push(pair); flush(); }};
  window.__readerPanelCookieFlush = flush;
  Object.defineProperty(document, 'cookie', {{
    configurable: true,
    get: function () {{
      var merged = {{}};
      try {{
        String(nativeGet.call(document)).split(/;\s*/).forEach(function (p) {{
          var i = p.indexOf('=');
          if (i > 0) merged[p.slice(0, i).trim()] = p.slice(i + 1).trim();
        }});
      }} catch (e) {{}}
      // 书源 jar 优先: 浏览器里的同名旧值被实时 jar 值覆盖
      Object.keys(jar).forEach(function (k) {{ merged[k] = jar[k]; }});
      return Object.keys(merged).map(function (k) {{ return k + '=' + merged[k]; }}).join('; ');
    }},
    set: function (value) {{
      try {{ nativeSet.call(document, value); }} catch (e) {{}}
      // 只取 `名字=值` 部分, 属性(expires/path/domain)交给 jar 侧按域处理。
      try {{
        var raw = String(value);
        var head = raw.split(';')[0].trim();
        if (head && head.indexOf('=') > 0) {{
          var name = head.slice(0, head.indexOf('=')).trim();
          var val = head.slice(head.indexOf('=') + 1).trim();
          // 页面用 `x=; expires=<过去时间>` 删除 cookie(该页的 userLogout 即如此)。
          // 空值/已过期应把条目从 jar 移除, 否则空值会被当成有效登录态回写上游。
          // 注意只取 `expires=` 之后到下一个 `;` 之前的部分交给 Date.parse ——
          // 整串(含其它属性)会解析成 NaN, 使过期判断永远不成立。
          var expired = false;
          var expires = /;\s*expires\s*=\s*([^;]+)/i.exec(raw);
          if (expires) {{
            var at = Date.parse(expires[1].trim());
            expired = !isNaN(at) && at <= Date.now();
          }}
          if (expired || !val) {{
            delete jar[name];
            // 删除必须发**空值**(`name=`), 而不是原样发 `head`: 宿主侧按「值为空」
            // 判定删除, 发非空值会被当成有效 cookie 写回 jar, 结果删不掉。
            if (window.__readerPanelQueueCookie) window.__readerPanelQueueCookie(name + '=');
            return;
          }}
          jar[name] = val;
          if (window.__readerPanelQueueCookie) window.__readerPanelQueueCookie(head);
        }}
      }} catch (e) {{}}
    }}
  }});
}})();
</script>"#,
            script_string(jar_cookie),
            panel_url
        ));
    }
    // 上游 origin 的 localStorage（由独立窗口在真实域名下收走）。
    // 段评这类 SPA 的鉴权需要设备 id，缺了就只能现生成假的。
    let source_storage = crate::service::book_service::source_window_storage_for(&entry.book_source_url);
    bridge.push_str(&build_source_panel_bridge_script(panel_id, entry.external_links, &source_storage));
    if let Some(preload) = &entry.preload_js {
        bridge.push_str(&format!("<script>(0,eval)({});</script>", script_string(preload)));
    }
    if page_url.starts_with("http://") || page_url.starts_with("https://") {
        let base = page_url.replace('&', "&amp;").replace('"', "&quot;").replace('<', "&lt;");
        bridge.push_str(&format!("<base href=\"{base}\">"));
    }
    // Install the bridge and preload before scripts in the original head execute.
    let head = Regex::new(r"(?i)<head\b[^>]*>").expect("head pattern");
    if let Some(found) = head.find(html) {
        format!("{}{}{}", &html[..found.end()], bridge, &html[found.end()..])
    } else {
        format!("{bridge}{html}")
    }
}

async fn fetch_source_panel_page(
    state: &AppState, panel_id: &str, entry: &crate::service::book_service::SourcePanelEntry,
    target: Option<&str>, request: &tauri::http::Request<Vec<u8>>,
) -> Result<tauri::http::Response<Vec<u8>>, AppError> {
    let target = resolve_proxy_target_url(target.unwrap_or(&entry.url), &entry.url)?;

    // RSS / 单 URL 订阅源的面板没有归属书源(`book_source_url` 为空), 因此不能走
    // 下面那条「查书源 → 用书源规则」的路径。它对应手机端 `ReadRssActivity` 的
    // 那个 WebView: `handleCommonSchemes` 对 http/https 返回 false, 也就是
    // 「在原地打开」, 不限制域名。源仓库的二级页面正是跨域跳转
    // (旧域名 → 新域名 / 镜像站), 若沿用书源的同域校验只会得到
    // 「panel target is outside the page domain」。
    if entry.book_source_url.is_empty() {
        return fetch_rss_panel_page(panel_id, entry, &target, request).await;
    }

    // 书源面板保持严格同域: 它带着书源的 cookie 与登录态, 放宽等于把登录凭证
    // 交给任意目标域。
    if !proxy_target_matches_source(&target, &entry.url) {
        return Ok(text_response(403, "panel target is outside the page domain"));
    }
    let source = state.book_source_service.get("default", &entry.book_source_url).await?
        .ok_or_else(|| AppError::NotFound("bookSource not found".into()))?;
    fetch_source_panel_response(&source, panel_id, entry, &target, request).await
}

/// RSS 面板的二级页抓取: 不依赖书源, 用面板自身的 http 上下文(源级 cookie jar
/// 与代理), 并保留内网地址拦截 —— 面板 HTML 托管在 `reader` origin, 桥能触达
/// 宿主网络, 而 RSS 源可从远程 JSON 导入(不受信任)。
async fn fetch_rss_panel_page(
    panel_id: &str,
    entry: &crate::service::book_service::SourcePanelEntry,
    target: &str,
    request: &tauri::http::Request<Vec<u8>>,
) -> Result<tauri::http::Response<Vec<u8>>, AppError> {
    crate::api::commands::rss::reject_non_public_target(target)?;
    let context = match entry.http.clone() {
        Some(context) => context,
        None => crate::parser::js::source_http_context(&entry.url)
            .map_err(AppError::Internal)?,
    };
    let client = context.client.clone();
    let method = request.method().clone();
    let mut forwarded = http::HeaderMap::new();
    if let Some(content_type) = request.headers().get(http::header::CONTENT_TYPE) {
        forwarded.insert(http::header::CONTENT_TYPE, content_type.clone());
    }
    let mut builder = client.request(method, target).headers(forwarded);
    if !request.body().is_empty() {
        builder = builder.body(request.body().clone());
    }
    let response = builder.send().await.map_err(AppError::Http)?;
    let status = response.status();
    let final_url = response.url().to_string();
    let content_type = response
        .headers()
        .get(http::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("application/octet-stream")
        .to_string();
    let bytes = response.bytes().await.map_err(AppError::Http)?;
    let is_html = is_html_response(Some(&content_type), &bytes);
    let body = if is_html {
        let html = crate::crawler::fetcher::decode_body(&bytes, None, Some(&content_type));
        // 跨域放行: 这类页面会跳到任意同族/合作域名。
        let html = rewrite_source_panel_html(&html, &final_url, &entry.url, panel_id);
        render_source_panel_page(panel_id, entry, &final_url, &html).into_bytes()
    } else {
        bytes.to_vec()
    };
    let built = http::Response::builder()
        .status(status.as_u16())
        .header(
            http::header::CONTENT_TYPE,
            if is_html { "text/html; charset=utf-8" } else { &content_type },
        )
        .header(http::header::CACHE_CONTROL, "no-store");
    Ok(built.body(body).unwrap_or_default())
}

async fn fetch_source_panel_response(
    source: &BookSource, panel_id: &str, entry: &crate::service::book_service::SourcePanelEntry,
    target: &str, request: &tauri::http::Request<Vec<u8>>,
) -> Result<tauri::http::Response<Vec<u8>>, AppError> {
    let context = entry.http.clone().map(Ok).unwrap_or_else(|| crate::parser::js::source_http_context(&entry.book_source_url))
        .map_err(AppError::Internal)?;
    let header_source = source.clone();
    let header_context = context.clone();
    let headers = tokio::task::spawn_blocking(move || {
        crate::parser::js::with_http_context(header_context, || {
            crate::parser::js::with_source(&header_source, || {
                crate::crawler::url_analyzer::source_header_spec(&header_source)
            })
        })
    }).await.map_err(|err| AppError::Internal(err.into()))??;
    let client = match headers.proxy.as_deref() {
        Some(proxy) => crate::crawler::http_client::build_source_client(context.timeout_secs, Some(proxy), context.session.cookies.clone()).map_err(AppError::Internal)?,
        None => context.client.clone(),
    };
    let mut forwarded = merge_proxy_headers(headers.headers, request.headers())?;
    if !forwarded.contains_key(http::header::REFERER) {
        forwarded.insert(http::header::REFERER, http::HeaderValue::from_str(&entry.url)
            .map_err(|err| AppError::BadRequest(format!("网页来源地址无效: {err}")))?);
    }
    // WebView2 交给宿主的自定义协议请求可能不带 Content-Type, 也可能带一个与实际
    // 请求体不符的头。后端(PHP 等)在缺该头、或头与体不匹配时都**完全不解析请求体**
    // (`$_POST` 为空), 表现为「后端收不到任何字段」—— 某聚合源的注册接口即回
    // 「无效的操作」。因此以请求体的真实字节为准: 体是 multipart 就按体首行的
    // boundary 声明, 否则仅在缺头时按 urlencoded 兜底。
    if request.method() == http::Method::POST {
        inherit_form_content_type(&mut forwarded, request.body());
    }
    let mut builder = client.request(request.method().clone(), target).headers(forwarded);
    if !request.body().is_empty() { builder = builder.body(request.body().clone()); }
    let response = builder.send().await.map_err(AppError::Http)?;
    let status = response.status();
    let final_url = response.url().to_string();
    let content_type = response.headers().get(http::header::CONTENT_TYPE).and_then(|v| v.to_str().ok()).unwrap_or("application/octet-stream").to_string();
    let bytes = response.bytes().await.map_err(AppError::Http)?;
    let is_html = is_html_response(Some(&content_type), &bytes);
    let body = if is_html {
        let html = crate::crawler::fetcher::decode_body(&bytes, None, Some(&content_type));
        let html = rewrite_source_panel_html(&html, &final_url, &entry.url, panel_id);
        let jar_cookie = url::Url::parse(&final_url)
            .map(|url| context.session.cookies.get(&url)).unwrap_or_default();
        render_source_panel_page_with_cookie(panel_id, entry, &final_url, &html, &jar_cookie).into_bytes()
    } else { bytes.to_vec() };
    let mut built = http::Response::builder().status(status.as_u16())
        .header(http::header::CONTENT_TYPE, if is_html { "text/html; charset=utf-8" } else { &content_type })
        .header(http::header::CACHE_CONTROL, "no-store");
    // 面板页在 reader origin 里运行, 页面 JS 读 document.cookie 判断登录态
    // (如面板页的 getCookie('qttoken'))。真实 cookie 在书源 jar 中,
    // 以 Set-Cookie 透出给面板 origin, 让入口检查通过; 后续请求仍由代理
    // 携带 jar cookie, 浏览器侧的这份仅供 JS 读取。每个 cookie 对一个头。
    if is_html {
        if let Ok(url) = url::Url::parse(&final_url) {
            for pair in context.session.cookies.get(&url).split("; ").filter(|p| !p.is_empty()) {
                built = built.header(http::header::SET_COOKIE, format!("{pair}; Path=/"));
            }
        }
    }
    Ok(built.body(body).unwrap_or_default())
}

/// 把上游 HTML 里的资源/链接改写成经本应用代理的地址。
///
/// 只改写**同域**目标。跨域资源(第三方 CDN 的 css/js/图片、广告统计等)保持原样,
/// 由 WebView 直接去取 —— 这是刻意的:
///
///   1. 每个改写过的地址都会变成一次 `/sourcePanel` 宿主请求, 而宿主请求是
///      **串行经过本进程**的(reqwest → 上游 → 回写)。页面引用几十个跨域资源时,
///      就是几十个并发任务, 每个都要占住连接的 15s 超时才释放; WebView2 对同一
///      host 的并发连接数又有上限(通常 6), 于是请求互相排队直到超时。
///   2. 跨域页面往往引用大量第三方资源, 其中不少在直连网络下根本连不通。
///      一旦把它们全部收进代理, 主文档加载会被这些挂起的子资源拖住。
///
/// 实测: 放开跨域改写后打开源仓库二级页, 系统出现大量挂起连接, 资源管理器
/// (explorer.exe) 被拖到重启。因此这里保持「只代理同域」的保守做法; 跨域二级页
/// 本身仍由 `fetch_rss_panel_page` 直接抓取并托管(不受同域限制), 只是它引用的
/// 外域资源不再经过我们中转。
fn rewrite_source_panel_html(
    html: &str,
    page_url: &str,
    allowed_url: &str,
    panel_id: &str,
) -> String {
    let tags = Regex::new(r"(?is)<(?:script|link|img|iframe|source|video|audio|form|a)\b[^>]*>").unwrap();
    let attrs = Regex::new(r#"(?i)\b(src|href|action)\s*=\s*(?:"([^"]*)"|'([^']*)')"#).unwrap();
    tags.replace_all(html, |tag: &Captures| {
        attrs.replace_all(&tag[0], |attr: &Captures| {
            let raw = attr.get(2).or_else(|| attr.get(3)).map(|v| v.as_str()).unwrap_or_default();
            if raw.starts_with('#') || raw.is_empty() { return attr[0].to_string(); }
            let target = crate::crawler::url_analyzer::absolute_url(page_url, &raw.replace("&amp;", "&"));
            // 跨域面板也按「本次页面所在域」判定, 而不是按订阅源初始地址 ——
            // 否则跨域二级页连自己的资源都会被判成外站。
            if !proxy_target_matches_source(&target, page_url)
                && !proxy_target_matches_source(&target, allowed_url)
            {
                return attr[0].to_string();
            }
            format!("{}=\"{}/sourcePanel?__readerPanelId={}&amp;__readerPanelUrl={}\"", &attr[1], reader_scheme_origin(), urlencoding::encode(panel_id), urlencoding::encode(&target))
        }).into_owned()
    }).into_owned()
}

/// 面板内桥的同步端点。请求体: `{ "action": "...", "payload": {...} }`
/// 响应体: `{ "ok": true, "value": ... }` 或 `{ "ok": false, "error": "..." }`
async fn source_panel_bridge_route(
    state: &AppState,
    query: &str,
    request: &tauri::http::Request<Vec<u8>>,
) -> tauri::http::Response<Vec<u8>> {
    // 信任边界: 面板 HTML 来自用户导入的书源, 而桥能触达宿主的网络与脚本能力。
    // 只有本应用自己的 origin 能调用, 外部网页(带具体 Origin)一律拒绝。
    let request_origin = request
        .headers()
        .get(http::header::ORIGIN)
        .and_then(|value| value.to_str().ok());
    if !panel_bridge_origin_allowed(request_origin) {
        return json_response(403, serde_json::json!({ "ok": false, "error": "forbidden origin" }));
    }
    // 无 Origin 头是同源请求(面板与桥同为 `reader` 协议 origin), 不需要 CORS 头;
    // 带 Origin 且已通过上面的校验时回显它, 绝不使用通配符。
    let allow_origin = request_origin.map(|_| reader_scheme_origin());

    let params = parse_query(query);
    let Some(panel_id) = params.get("panelId") else {
        return json_response_panel(
            400,
            serde_json::json!({ "ok": false, "error": "missing panelId" }),
            allow_origin,
        );
    };
    let Some(entry) = crate::service::book_service::get_source_panel(panel_id) else {
        return json_response_panel(
            404,
            serde_json::json!({ "ok": false, "error": "panel not found" }),
            allow_origin,
        );
    };
    let body: serde_json::Value = serde_json::from_slice(request.body()).unwrap_or_default();
    let action = body.get("action").and_then(|v| v.as_str()).unwrap_or("");
    let payload = body.get("payload").cloned().unwrap_or_default();

    if action == "complete" {
        let page = crate::service::source_browser::BrowserPage {
            url: payload.get("url").and_then(|v| v.as_str()).filter(|v| !v.is_empty()).unwrap_or(&entry.url).to_string(),
            body: payload.get("body").and_then(|v| v.as_str()).unwrap_or_default().to_string(),
        };
        let result = match crate::service::source_browser::complete_browser(panel_id, page) {
            Ok(()) => serde_json::json!({"ok":true}),
            Err(error) => serde_json::json!({"ok":false,"error":error}),
        };
        return json_response_panel(200, result, allow_origin);
    }

    let outcome = crate::service::book_service::handle_source_panel_bridge(
        &state.book_source_service,
        &entry,
        action,
        payload,
    )
    .await;
    match outcome {
        Ok(value) => json_response_panel(
            200,
            serde_json::json!({ "ok": true, "value": value }),
            allow_origin,
        ),
        Err(error) => json_response_panel(
            200,
            serde_json::json!({ "ok": false, "error": error }),
            allow_origin,
        ),
    }
}

/// 面板桥的调用者是否可信。
///
/// 放行两种情况: 不带 `Origin`(同源同步 XHR / 原生调用), 或 Origin 等于
/// `reader` 协议 origin(面板自身)。其余(如外部 https 页面)一律拒绝 ——
/// 否则任何本机网页都能借桥驱动宿主去请求书源接口或执行脚本。
fn panel_bridge_origin_allowed(origin: Option<&str>) -> bool {
    match origin {
        None => true,
        Some(value) => value.trim() == reader_scheme_origin(),
    }
}

fn parse_query(query: &str) -> HashMap<String, String> {
    url::form_urlencoded::parse(query.as_bytes())
        .map(|(k, v)| (k.into_owned(), v.into_owned()))
        .collect()
}

fn text_response(status: u16, message: &str) -> tauri::http::Response<Vec<u8>> {
    http::Response::builder()
        .status(status)
        .header(http::header::CONTENT_TYPE, "text/plain; charset=utf-8")
        .body(message.as_bytes().to_vec())
        .unwrap_or_else(|_| http::Response::new(Vec::new()))
}

fn json_response(status: u16, value: serde_json::Value) -> tauri::http::Response<Vec<u8>> {
    http::Response::builder()
        .status(status)
        .header(http::header::CONTENT_TYPE, "application/json; charset=utf-8")
        .header(http::header::CACHE_CONTROL, "no-store")
        .body(value.to_string().into_bytes())
        .unwrap_or_else(|_| http::Response::new(Vec::new()))
}

/// 面板桥专用响应。
///
/// 面板与桥同为 `reader` 协议 origin(因此请求通常不带 `Origin`)。只有调用方
/// 确实带了 `Origin` 时才回显**该已通过校验的 origin**, 不再使用通配符 ——
/// 通配符会让任意本机页面都能读走桥的响应。
fn json_response_panel(
    status: u16,
    value: serde_json::Value,
    allow_origin: Option<&str>,
) -> tauri::http::Response<Vec<u8>> {
    let mut builder = http::Response::builder()
        .status(status)
        .header(http::header::CONTENT_TYPE, "application/json; charset=utf-8")
        .header(http::header::CACHE_CONTROL, "no-store")
        .header(http::header::VARY, "Origin");
    if let Some(origin) = allow_origin {
        builder = builder.header(http::header::ACCESS_CONTROL_ALLOW_ORIGIN, origin);
    }
    builder
        .body(value.to_string().into_bytes())
        .unwrap_or_else(|_| http::Response::new(Vec::new()))
}

// ─────────────────────────── cover / epub / files ───────────────────────────
/// In-process LRU so re-rendered covers do not refetch. WebView2 does not
/// cache custom-scheme responses.
const COVER_CACHE_CAP: usize = 200;

fn cover_cache() -> &'static std::sync::Mutex<std::collections::HashMap<String, (Vec<u8>, String)>> {
    static CACHE: std::sync::OnceLock<
        std::sync::Mutex<std::collections::HashMap<String, (Vec<u8>, String)>>,
    > = std::sync::OnceLock::new();
    CACHE.get_or_init(|| std::sync::Mutex::new(std::collections::HashMap::new()))
}

/// Drop released URLs from the in-process cover cache. Disk cleanup alone is not
/// enough: the current process would otherwise keep serving the deleted bytes
/// until the 200-entry cap clears the whole map.
pub(crate) fn remove_cover_cache_urls(urls: &[String]) {
    if let Ok(mut cache) = cover_cache().lock() {
        for url in urls {
            cache.remove(url);
        }
    }
}

pub async fn cover_route(state: &AppState, query: &str) -> tauri::http::Response<Vec<u8>> {
    let params: HashMap<String, String> =
        serde_urlencoded::from_str(query).unwrap_or_default();
    let url = params.get("path").map(String::as_str).unwrap_or_default();
    if url.is_empty() {
        return http::Response::builder()
            .status(404)
            .body(Vec::new())
            .unwrap_or_default();
    }

    let cache_key = url.to_string();
    if let Some((bytes, content_type)) = cover_cache()
        .lock()
        .ok()
        .and_then(|cache| cache.get(&cache_key).cloned())
    {
        return bytes_response(bytes, &content_type, Some("86400"));
    }

    // 本地 PDF 封面: /local-pdf/<hash>/cover.jpg → storage 下的书目录
    if let Some(rest) = url.strip_prefix("/local-pdf/") {
        return local_pdf_cover_route(state, rest).await;
    }

    // 书源反查：`coverDecodeJs` 是**书源**上的字段，而封面请求只带图片地址。
    // 前端因此把书籍的 `origin`（= bookSourceUrl）作为 `source` 一并传来；
    // 老链接/直接引用没有该参数时，再按书架里同一条封面地址反查一次。
    let source = resolve_cover_source(state, params.get("source").map(String::as_str), url).await;

    // "public" namespace matches the original unauthenticated cover endpoint.
    match state
        .book_service
        .get_cover_for_source("public", url, source.as_ref())
        .await
    {
        Ok((bytes, content_type)) => {
            if let Ok(mut cache) = cover_cache().lock() {
                if cache.len() >= COVER_CACHE_CAP {
                    cache.clear();
                }
                cache.insert(cache_key, (bytes.clone(), content_type.clone()));
            }
            bytes_response(bytes, &content_type, Some("86400"))
        }
        Err(_) => {
            // 带时效签名的图床地址（`x-expires`/`x-signature`）过期后回源必然 403。
            // 书架只存了那条旧地址，清缓存或重试都拿不到图 —— 只能重新拉详情页
            // 换一条新签名地址。取图成功才回写，失败时保持书架原样。
            if let Some((book_url, new_url, bytes, content_type)) =
                refresh_stale_shelf_cover(state, url).await
            {
                // 让旧地址立刻能命中缓存：书架上这张卡片仍按旧地址请求，
                // 不然重启进程后又要重新走一次回源。
                let _ = state
                    .book_service
                    .cache_cover_bytes("public", url, &bytes, &content_type)
                    .await;
                let _ = state
                    .book_service
                    .replace_shelf_cover_url("default", &book_url, url, &new_url)
                    .await;
                if let Ok(mut cache) = cover_cache().lock() {
                    if cache.len() >= COVER_CACHE_CAP {
                        cache.clear();
                    }
                    cache.insert(cache_key.clone(), (bytes.clone(), content_type.clone()));
                }
                return bytes_response(bytes, &content_type, Some("86400"));
            }
            http::Response::builder()
                .status(404)
                .body(Vec::new())
                .unwrap_or_default()
        }
    }
}

/// 按失效的封面地址反查书架书，重新拉详情页并试取新地址的图。
///
/// 只对带时效签名的地址生效：普通封面 403 多半是防盗链或站点临时故障，
/// 重新拉详情页也换不出新地址，白白多一次网络请求。失败结果做 5 分钟
/// 冷却，避免书源不可用时每张卡片都触发一次详情请求。
async fn refresh_stale_shelf_cover(
    state: &AppState,
    stale_url: &str,
) -> Option<(String, String, Vec<u8>, String)> {
    if !cover_url_expired(stale_url) {
        return None;
    }
    {
        let Ok(mut cooldown) = cover_refresh_cooldown().lock() else {
            return None;
        };
        if let Some(at) = cooldown.get(stale_url) {
            if at.elapsed() < COVER_REFRESH_COOLDOWN {
                return None;
            }
        }
        if cooldown.len() >= COVER_REFRESH_COOLDOWN_CAP {
            cooldown.clear();
        }
        cooldown.insert(stale_url.to_string(), std::time::Instant::now());
    }

    let books = state.book_service.get_bookshelf("default").await.ok()?;
    let book = books
        .into_iter()
        .find(|book| book.cover_url.as_deref() == Some(stale_url))?;
    if book.book_url.trim().is_empty() || book.origin.trim().is_empty() {
        return None;
    }
    let source = state
        .book_source_service
        .get("default", &book.origin)
        .await
        .ok()
        .flatten()?;
    // 先试当前源：详情页有时会下发新的签名地址；拿不到或取图失败就换下一个。
    let mut tried: std::collections::HashSet<String> = std::collections::HashSet::new();
    tried.insert(stale_url.trim().to_string());

    let mut candidates: Vec<(String, Option<BookSource>)> = Vec::new();
    if let Ok(info) = state
        .book_service
        .get_book_info("default", &source, &book.book_url)
        .await
    {
        if let Some(url) = info.cover_url.filter(|url| !url.trim().is_empty()) {
            candidates.push((url, Some(source.clone())));
        }
    }
    // 同一本书在别的源里往往有一份不带签名的封面（搜索页用的就是它）。
    // 候选源的封面也必须能取到才算数，避免把另一个失效地址写进书架。
    // 解密规则按候选自己的 `origin` 取：封面是用哪个源解析出来的，就只有
    // 那个源的 `coverDecodeJs` 解得开。
    for candidate in book.source_candidates.clone().unwrap_or_default() {
        let Some(url) = candidate.cover_url.filter(|url| !url.trim().is_empty()) else {
            continue;
        };
        let candidate_origin = candidate.origin.trim();
        if candidate_origin.is_empty() || candidate_origin == book.origin {
            continue;
        }
        let candidate_source = state
            .book_source_service
            .get("default", candidate_origin)
            .await
            .ok()
            .flatten();
        candidates.push((url, candidate_source));
    }

    for (candidate_url, candidate_source) in candidates {
        let candidate_url = candidate_url.trim().to_string();
        if candidate_url.is_empty() || !tried.insert(candidate_url.clone()) {
            continue;
        }
        let source_for_fetch = candidate_source;
        if let Ok((bytes, content_type)) = state
            .book_service
            .get_cover_for_source("public", &candidate_url, source_for_fetch.as_ref())
            .await
        {
            return Some((book.book_url, candidate_url, bytes, content_type));
        }
    }
    None
}

/// 地址里的 `x-expires` 是否已经过期。
///
/// 只认能解析成秒级时间戳的 `x-expires`：没有该参数、值不是数字、或还没到期
/// 都返回 false。这个判定必须严格 —— 它决定要不要为一张封面额外发起详情页
/// 请求，误判会把书架首屏的每一张图都拖成一次网络往返。
fn cover_url_expired(url: &str) -> bool {
    let Some((_, query)) = url.split_once('?') else {
        return false;
    };
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs())
        .unwrap_or(0);
    query.split('&').any(|pair| {
        let Some((name, value)) = pair.split_once('=') else {
            return false;
        };
        name.eq_ignore_ascii_case("x-expires")
            && value
                .parse::<u64>()
                .map(|expires| expires <= now)
                .unwrap_or(false)
    })
}

const COVER_REFRESH_COOLDOWN: std::time::Duration = std::time::Duration::from_secs(300);
const COVER_REFRESH_COOLDOWN_CAP: usize = 128;

fn cover_refresh_cooldown() -> &'static std::sync::Mutex<HashMap<String, std::time::Instant>> {
    static COOLDOWN: std::sync::OnceLock<
        std::sync::Mutex<HashMap<String, std::time::Instant>>,
    > = std::sync::OnceLock::new();
    COOLDOWN.get_or_init(|| std::sync::Mutex::new(HashMap::new()))
}

/// 为一次封面请求找出它所属的书源。
///
/// 优先用调用方显式给的 `source`（书籍的 `origin`），其次按封面地址在书架里
/// 反查 —— 只为了拿到 `coverDecodeJs`，找不到就返回 `None`（不解密，等调用方
/// 下次带上参数）。
///
/// 搜索结果页一次要渲几十张封面，每张都读一次书架 JSON + 查一次书源库会很贵，
/// 因此按 `source` 地址缓存一小份（书源被编辑后最多错一个封面）。
async fn resolve_cover_source(
    state: &AppState,
    explicit: Option<&str>,
    cover_url: &str,
) -> Option<BookSource> {
    let explicit = explicit.map(str::trim).filter(|value| !value.is_empty());
    let source_url = match explicit {
        Some(value) => value.to_string(),
        None => {
            // 没有显式来源（老链接/直接引用）：按封面地址在书架里反查。
            let books = state.book_service.get_bookshelf("default").await.ok()?;
            books
                .iter()
                .find(|book| {
                    book.cover_url.as_deref() == Some(cover_url)
                        || book.custom_cover_url.as_deref() == Some(cover_url)
                })
                .map(|book| book.origin.clone())
                .filter(|origin| !origin.trim().is_empty())?
        }
    };
    if let Some(cached) = cover_source_cache().lock().ok().and_then(|cache| {
        cache
            .get(&source_url)
            .filter(|(_, at)| at.elapsed() < COVER_SOURCE_CACHE_TTL)
            .map(|(source, _)| source.clone())
    }) {
        return Some(cached);
    }
    let found = state
        .book_source_service
        .get("default", &source_url)
        .await
        .ok()
        .flatten();
    if let Some(source) = &found {
        if let Ok(mut cache) = cover_source_cache().lock() {
            if cache.len() >= COVER_SOURCE_CACHE_CAP {
                cache.clear();
            }
            cache.insert(source_url, (source.clone(), std::time::Instant::now()));
        }
    }
    found
}

const COVER_SOURCE_CACHE_CAP: usize = 64;

/// 书源反查缓存：`bookSourceUrl -> (书源, 写入时刻)`（含 `coverDecodeJs`）。
///
/// 带 TTL 是刻意的：书源随时可能在「书源管理」里被编辑（改 key、改规则），
/// 没有失效钩子时缓存会永远返回旧规则，表现为"改了书源封面还是花图"。
/// 30 秒足够覆盖一次列表渲染（几十张封面），又不会让改动等太久。
fn cover_source_cache() -> &'static std::sync::Mutex<HashMap<String, (BookSource, std::time::Instant)>> {
    static CACHE: std::sync::OnceLock<
        std::sync::Mutex<HashMap<String, (BookSource, std::time::Instant)>>,
    > = std::sync::OnceLock::new();
    CACHE.get_or_init(|| std::sync::Mutex::new(HashMap::new()))
}

const COVER_SOURCE_CACHE_TTL: std::time::Duration = std::time::Duration::from_secs(30);

/// 本地 PDF 封面: rest = "<hash>/cover.<ext>"。hash 在 service 侧校验
/// (32 位 hex), 文件名只接受三个固定值, 无路径穿越面。
async fn local_pdf_cover_route(
    state: &AppState,
    rest: &str,
) -> tauri::http::Response<Vec<u8>> {
    let Some((hash, file_name)) = rest.split_once('/') else {
        return http::Response::builder()
            .status(404)
            .body(Vec::new())
            .unwrap_or_default();
    };
    if !matches!(file_name, "cover.jpg" | "cover.png" | "cover.jp2") {
        return http::Response::builder()
            .status(404)
            .body(Vec::new())
            .unwrap_or_default();
    }
    match state
        .local_pdf_book_service
        .read_cover_file("default", hash, file_name)
        .await
    {
        Ok((bytes, content_type)) => {
            bytes_response(bytes, &content_type, Some("private, max-age=86400"))
        }
        Err(_) => http::Response::builder()
            .status(404)
            .body(Vec::new())
            .unwrap_or_default(),
    }
}

/// 本地 PDF 原始文件: `/pdf?bookUrl=local-pdf:<hash>`。
///
/// 前端 pdf.js 整本取走自己渲染 (与 Legado 的 PdfRenderer 同一分工: 后端只
/// 管文件, 页面光栅化在前端)。bookUrl 在 service 侧校验 32 位 hex, 无路径穿越面。
pub async fn pdf_route(state: &AppState, query: &str) -> tauri::http::Response<Vec<u8>> {
    let params: HashMap<String, String> =
        serde_urlencoded::from_str(query).unwrap_or_default();
    let book_url = params.get("bookUrl").map(String::as_str).unwrap_or_default();
    if book_url.is_empty() {
        return http::Response::builder()
            .status(404)
            .body(Vec::new())
            .unwrap_or_default();
    }
    match state
        .local_pdf_book_service
        .read_pdf_file("default", book_url)
        .await
    {
        Ok(bytes) => bytes_response(bytes, "application/pdf", Some("private, max-age=3600")),
        Err(_) => http::Response::builder()
            .status(404)
            .body(Vec::new())
            .unwrap_or_default(),
    }
}

pub async fn epub_route(state: &AppState, query: &str) -> tauri::http::Response<Vec<u8>> {
    let params: HashMap<String, String> =
        serde_urlencoded::from_str(query).unwrap_or_default();
    let book_url = params.get("bookUrl").map(String::as_str).unwrap_or_default();
    let path = params.get("path").map(String::as_str).unwrap_or_default();
    if book_url.is_empty() || path.is_empty() {
        return http::Response::builder()
            .status(404)
            .body(Vec::new())
            .unwrap_or_default();
    }
    match state
        .local_epub_book_service
        .get_asset("default", book_url, path)
        .await
    {
        Ok(asset) => bytes_response(asset.bytes, &asset.content_type, Some("private, max-age=3600")),
        Err(_) => http::Response::builder()
            .status(404)
            .body(Vec::new())
            .unwrap_or_default(),
    }
}

/// Serves user-uploaded files from `storage/assets/<ns>/<type>/<name>`.
/// The path query is the asset URL minus the leading slash, e.g.
/// `files?path=default/ai-maps/map.png`.
///
/// 校验走 `util::path_safe` 的**唯一实现**, 与 Web 端静态资源共用。
/// 早先这里自己写了一版"按 `/` 分段、拒绝 `..` 段"的判断, 在 Windows 上
/// 被 `..\..\Windows\win.ini` 绕过(反斜杠不参与 `split('/')`, 整串作为一个
/// 普通段通过), 而 `/files` 现在同时挂在未鉴权的 Web 服务上 —— 等于把
/// 本机任意文件读权限交给了同网段。不要再在本文件里另写一套判定。
pub async fn files_route(state: &AppState, query: &str) -> tauri::http::Response<Vec<u8>> {
    let params: HashMap<String, String> =
        serde_urlencoded::from_str(query).unwrap_or_default();
    let Some(rel) = params.get("path") else {
        return http::Response::builder()
            .status(404)
            .body(Vec::new())
            .unwrap_or_default();
    };
    // `serde_urlencoded` 已经解过百分号转义, 这里不要再解一次(会变成二次解码,
    // `%252e%252e` 被解成 `..` 之后反而绕过校验)。
    let Some(relative) = crate::util::path_safe::safe_relative_path(rel) else {
        return http::Response::builder()
            .status(400)
            .body(Vec::new())
            .unwrap_or_default();
    };
    if relative.as_os_str().is_empty() {
        return http::Response::builder()
            .status(400)
            .body(Vec::new())
            .unwrap_or_default();
    }
    let full = std::path::PathBuf::from(&state.config.storage_dir)
        .join("assets")
        .join(relative);
    match tokio::fs::read(&full).await {
        Ok(bytes) => {
            let content_type = mime_from_ext(&full).to_string();
            bytes_response(bytes, &content_type, None)
        }
        Err(_) => http::Response::builder()
            .status(404)
            .body(Vec::new())
            .unwrap_or_default(),
    }
}

fn bytes_response(
    bytes: Vec<u8>,
    content_type: &str,
    cache_control: Option<&str>,
) -> tauri::http::Response<Vec<u8>> {
    let mut builder = http::Response::builder().status(200);
    if let Ok(value) = http::HeaderValue::from_str(content_type) {
        builder = builder.header(http::header::CONTENT_TYPE, value);
    }
    if let Some(cc) = cache_control {
        if let Ok(value) = http::HeaderValue::from_str(cc) {
            builder = builder.header(http::header::CACHE_CONTROL, value);
        }
    }
    builder
        .header("access-control-allow-origin", "*")
        .header("cross-origin-resource-policy", "cross-origin")
        .body(bytes)
        .unwrap_or_else(|_| http::Response::new(Vec::new()))
}

fn mime_from_ext(path: &std::path::Path) -> &'static str {
    match path.extension().and_then(|e| e.to_str()).unwrap_or_default().to_ascii_lowercase().as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "svg" => "image/svg+xml",
        "ttf" => "font/ttf",
        "otf" => "font/otf",
        "woff" => "font/woff",
        "woff2" => "font/woff2",
        "avif" => "image/avif",
        "json" => "application/json",
        "txt" => "text/plain; charset=utf-8",
        _ => "application/octet-stream",
    }
}



fn resolve_proxy_target_url(
    raw_target_url: &str,
    book_source_url: &str,
) -> Result<String, AppError> {
    let repaired = repair_encoded_url(raw_target_url);
    if let Ok(url) = Url::parse(&repaired) {
        return Ok(url.to_string());
    }

    let base = normalize_source_url(book_source_url);
    let base = Url::parse(&base)
        .map_err(|e| AppError::BadRequest(format!("invalid bookSourceUrl: {}", e)))?;
    base.join(&repaired)
        .map(|u| u.to_string())
        .map_err(|e| AppError::BadRequest(format!("invalid proxy target url: {}", e)))
}

/// 面板页写入 cookie 时, 把目标地址解析成可写入 cookie jar 的绝对 URL。
///
/// 面板里的地址可能是相对的(上游页在子路径下), 也可能带 `__readerPanelUrl`
/// 这类代理参数, 因此统一按代理目标地址的规则解析: 先尝试直接当作绝对 URL,
/// 失败再以书源地址为基准拼接。
pub(crate) fn resolve_panel_cookie_url(
    raw_target_url: &str,
    book_source_url: &str,
) -> Result<Url, AppError> {
    let repaired = repair_encoded_url(raw_target_url);
    if let Ok(url) = Url::parse(&repaired) {
        return Ok(url);
    }
    let base = normalize_source_url(book_source_url);
    let base = Url::parse(&base)
        .map_err(|e| AppError::BadRequest(format!("invalid bookSourceUrl: {}", e)))?;
    base.join(&repaired)
        .map_err(|e| AppError::BadRequest(format!("invalid cookie target url: {}", e)))
}

/// Login pages are rendered through the local URI scheme. Restrict every
/// proxied target to the source's host family so a valid login session cannot
/// be reused as a cross-domain request or SSRF primitive.
pub(crate) fn proxy_target_matches_source(target_url: &str, source_url: &str) -> bool {
    let Ok(target) = Url::parse(target_url) else {
        return false;
    };
    let Ok(source) = Url::parse(&normalize_source_url(source_url)) else {
        return false;
    };
    if !matches!(target.scheme(), "http" | "https")
        || !matches!(source.scheme(), "http" | "https")
    {
        return false;
    }
    let Some(target_host) = target.host_str() else {
        return false;
    };
    let Some(source_host) = source.host_str() else {
        return false;
    };
    let target_host = target_host.trim_end_matches('.').to_ascii_lowercase();
    let source_host = source_host.trim_end_matches('.').to_ascii_lowercase();
    match (source.port(), target.port()) {
        (Some(source_port), Some(target_port)) if source_port != target_port => return false,
        (Some(source_port), None) if target.port_or_known_default() != Some(source_port) => {
            return false
        }
        (None, Some(target_port)) if !matches!(target_port, 80 | 443) => return false,
        _ => {}
    }
    if target_host == source_host {
        return true;
    }

    // IP addresses and local development hosts have no meaningful parent
    // domain, so only the exact host is allowed for them.
    if target_host.parse::<std::net::IpAddr>().is_ok()
        || source_host.parse::<std::net::IpAddr>().is_ok()
        || source_host == "localhost"
        || source_host.ends_with(".localhost")
        || target_host == "localhost"
        || target_host.ends_with(".localhost")
    {
        return false;
    }

    let target_root = registrable_domain_from_host(&target_host);
    let source_root = registrable_domain_from_host(&source_host);
    !target_root.is_empty() && target_root == source_root
}

// Source settings override the embedded browser's defaults. HeaderMap::insert
// replaces case-insensitive matches so Referer/User-Agent are sent exactly once.
fn merge_proxy_headers(
    source_headers: Vec<(String, String)>,
    browser_headers: &http::HeaderMap,
) -> Result<http::HeaderMap, AppError> {
    let mut merged = http::HeaderMap::new();
    for (name, value) in browser_headers {
        if should_forward_request_header(name.as_str()) {
            merged.insert(name.clone(), value.clone());
        }
    }
    for (name, value) in source_headers {
        let name = http::HeaderName::from_bytes(name.as_bytes())
            .map_err(|err| AppError::BadRequest(format!("书源请求头名称无效: {err}")))?;
        let value = http::HeaderValue::from_str(&value)
            .map_err(|err| AppError::BadRequest(format!("书源请求头值无效: {err}")))?;
        merged.insert(name, value);
    }
    // The browser's boundary/encoding describes the actual submitted bytes.
    if let Some(value) = browser_headers.get(http::header::CONTENT_TYPE) {
        merged.insert(http::header::CONTENT_TYPE, value.clone());
    }
    Ok(merged)
}

/// 按请求体的真实形态校正表单 `Content-Type`。
///
/// WebView2 把自定义协议请求交给宿主时可能不提供 `Content-Type`, 也可能给一个与
/// 实际请求体不符的头。而后端(PHP 等)在缺该头、或头与体不匹配时都**完全不解析
/// 请求体**: `$_POST` 为空, 表现为「后端收不到任何字段」(某聚合源的注册接口即回
/// 「无效的操作」)。此时也不能一律按 urlencoded 兜底 —— multipart 体被错标后同样
/// 解析不出字段, 症状不变。因此以体为准:
/// - 体的首行是 `--boundary` -> 声明为该 boundary 的 multipart 体;
///   若已有头描述的就是这份体(boundary 一致), 原样保留浏览器给的头。
/// - 否则仅在缺头时按 urlencoded 兜底, 不覆盖已有的非 multipart 头。
fn inherit_form_content_type(headers: &mut http::HeaderMap, body: &[u8]) {
    let declared = headers
        .get(http::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default()
        .to_string();
    if let Some(boundary) = multipart_boundary(body) {
        let describes_body = declared.to_ascii_lowercase().starts_with("multipart/form-data")
            && declared.contains(&boundary);
        if describes_body {
            return;
        }
        if let Ok(value) =
            http::HeaderValue::from_str(&format!("multipart/form-data; boundary={boundary}"))
        {
            headers.insert(http::header::CONTENT_TYPE, value);
        }
        return;
    }
    if headers.contains_key(http::header::CONTENT_TYPE) {
        return;
    }
    headers.insert(
        http::header::CONTENT_TYPE,
        http::HeaderValue::from_static("application/x-www-form-urlencoded; charset=UTF-8"),
    );
}

/// 取 multipart 体的首行 boundary(不含前导 `--`)。非 multipart 返回 `None`。
fn multipart_boundary(body: &[u8]) -> Option<String> {
    let rest = body.strip_prefix(b"--")?;
    let end = rest.iter().position(|&b| b == b'\r' || b == b'\n')?;
    let boundary = &rest[..end];
    // RFC 2046: boundary 为 1..=70 个 bchars, 不含空白与控制字符。
    if boundary.is_empty() || boundary.len() > 70 || !boundary.is_ascii() {
        return None;
    }
    if boundary
        .iter()
        .any(|b| b.is_ascii_whitespace() || b.is_ascii_control())
    {
        return None;
    }
    String::from_utf8(boundary.to_vec()).ok()
}

fn should_forward_request_header(name: &str) -> bool {
    !matches!(
        name.to_ascii_lowercase().as_str(),
        "host"
            | "content-length"
            | "authorization"
            | "cookie"
            | "referer"
            | "origin"
            | "connection"
    )
}

fn is_html_response(content_type: Option<&str>, body: &[u8]) -> bool {
    if let Some(ct) = content_type {
        if ct.to_ascii_lowercase().contains("text/html") {
            return true;
        }
    }
    // 避免 String 分配: 在原始 bytes 上做大小写不敏感匹配。
    // HTML 前缀是 ASCII, 多字节 UTF-8 的首字节 >= 0x80 不会与 "<html" 混淆。
    let prefix = &body[..body.len().min(256)];
    prefix
        .windows(5)
        .any(|w| w.eq_ignore_ascii_case(b"<html"))
        || prefix
            .windows(14)
            .any(|w| w.eq_ignore_ascii_case(b"<!doctype html"))
}

/// 书源面板(`java.showBrowser`)注入的宿主桥。
///
/// 插件面板通过 `window.java` / `window.cache` / `window.run` 回调宿主
/// (legado `WebJsExtensions`)。面板里 `java.ajax` 是**同步**调用, 而 Tauri IPC
/// 是异步的, 因此这里把 bridge 请求发到同源的 `/sourcePanelBridge`, 用同步
/// `XMLHttpRequest` 取回结果 —— 与 legado 的同步 Java 桥语义一致。
///
/// `external_links`: 面板内的二级链接是否交给系统浏览器打开(见
/// `SourcePanelEntry::external_links`)。
///
/// 注意: 二级链接现在**统一**留在面板内经代理导航(见 click 兜底那段注释),
/// 这个标志暂时没有消费方, 保留是为了让 `SourcePanelEntry` 的既有语义与
/// 备份/调用方不受影响, 也便于将来按需恢复「RSS 外链走系统浏览器」。
pub fn build_source_panel_bridge_script(panel_id: &str, external_links: bool, source_storage: &[(String, String)]) -> String {
    let panel_id_json = script_string(panel_id);
    // 按 JSON 对象序列化后嵌进脚本：键值都可能是任意文本（含引号/换行）。
    let storage_json = {
        let mut map = serde_json::Map::new();
        for (key, value) in source_storage {
            map.insert(key.clone(), serde_json::Value::String(value.clone()));
        }
        script_string(&serde_json::Value::Object(map).to_string())
    };
    let external_links_json = if external_links { "true" } else { "false" };
    format!(
        r#"<script>
(function () {{
  var panelId = {panel_id_json};
  // 见 Rust 侧 build_source_panel_bridge_script 的说明: 当前仅为兼容保留。
  var externalLinks = {external_links_json};
  var bridgeBase = new URL("/sourcePanelBridge?panelId=" + encodeURIComponent(panelId), window.location.href).href;
  var nativeFetch = window.fetch.bind(window);
  var pageUrl = window.__readerPanelUrl || "";
  var documentId = window.crypto && window.crypto.randomUUID ? window.crypto.randomUUID() : String(Date.now()) + "-" + Math.random();
  // 诊断: 桥开始执行。面板打不开时先看这一条在不在 —— 不在说明脚本没跑
  // (被 CSP 拦、被上游页面脚本抛错打断), 在则继续看后面的 ready。
  try {{ bridgeSync("log", {{ message: "[bridge] start doc=" + documentId }}); }} catch (e) {{}}

  // 把**上游站点**的 localStorage 注入到本面板。
  //
  // 一类书源的段评页是 SPA，鉴权 = Cookie 里的 token + localStorage 里的设备 id
  // (`app_device_id` 之类)。本面板跑在 reader origin 下，localStorage 与
  // 上游隔离，SPA 取不到就现生成一个假的 id → 服务端判定异常 → 面板空白。
  // 这些值由独立窗口（真实域名）关闭时收走，这里按「缺才写」回填：
  // 页面自己写的值优先，避免覆盖新状态。
  var sourceStorage = JSON.parse({storage_json});
  if (sourceStorage && typeof sourceStorage === "object") {{
    try {{
      Object.keys(sourceStorage).forEach(function (key) {{
        if (window.localStorage.getItem(key) === null) {{
          window.localStorage.setItem(key, String(sourceStorage[key]));
        }}
      }});
    }} catch (e) {{}}
  }}

  // 同步桥: 与 legado 的 Java 同步接口语义一致(插件大量使用同步 ajax)。
  //
  // 面板 iframe 保留 `allow-same-origin`(插件用 localStorage 缓存书籍信息,
  // opaque origin 下会抛 SecurityError)。桥与面板同为 reader 协议 origin,
  // 因此这是同源请求; 服务端仍会校验 Origin, 只放行 reader origin 自身。
  function bridgeSync(action, payload) {{
    var xhr = new XMLHttpRequest();
    var body = JSON.stringify({{ action: action, payload: payload || {{}} }});
    xhr.open("POST", bridgeBase, false); // false = 同步
    xhr.setRequestHeader("Content-Type", "text/plain;charset=UTF-8");
    try {{
      xhr.send(body);
    }} catch (e) {{
      return {{ ok: false, error: String(e) }};
    }}
    if (xhr.status < 200 || xhr.status >= 300) {{
      return {{ ok: false, error: "bridge HTTP " + xhr.status }};
    }}
    try {{
      return JSON.parse(xhr.responseText);
    }} catch (e) {{
      return {{ ok: false, error: "bridge bad response" }};
    }}
  }}

  // 面板页 JS 用 `document.cookie = 'k=v'` 记录登录态(某聚合源的登录页写
  // `user_token`), 而上游后续页面由**服务端**读 Cookie 判断登录, 只留在浏览器里
  // 转发请求带不上, 上游会当作未登录并踢回登录页。这里把页面写入的 cookie 经
  // 同步桥落到书源 jar。注入顺序上 cookie 劫持脚本在本脚本之前, 它会把待发送的
  // 条目排队, 由这里的 ready 标志触发补发。
  window.__readerPanelCookieSet = function (url, pair) {{
    bridgeSync("cookieSet", {{ url: String(url || pageUrl), cookie: String(pair) }});
  }};
  window.__readerPanelBridgeReady = true;
  if (window.__readerPanelCookieFlush) {{ try {{ window.__readerPanelCookieFlush(); }} catch (e) {{}} }}

  // 通知宿主(父窗口): 面板要关闭 / 要换成另一个面板。
  // 面板 origin 为 reader.localhost, 父应用为 tauri.localhost, 用 postMessage 跨源通信。
  function postToHost(message) {{
    try {{
      message.panelId = panelId;
      message.documentId = documentId;
      window.parent.postMessage(message, "*");
    }} catch (e) {{}}
  }}


  var initialPageUrl = pageUrl;
  var panelOrigin = new URL(bridgeBase).origin;
  var navigating = false;
  var nativeSubmit = HTMLFormElement.prototype.submit;
  function panelTarget(url) {{
    return url.searchParams.get(url.searchParams.has("__readerPanelId") ? "__readerPanelUrl" : "url");
  }}
  function isPanelUrl(url) {{
    var id = url.searchParams.get(url.searchParams.has("__readerPanelId") ? "__readerPanelId" : "panelId");
    return url.origin === panelOrigin && url.pathname === "/sourcePanel" && id === panelId;
  }}
  // 面板页 JS 用 location.href 改写自身 query 做整页导航时(某聚合源的登录页的
  // sw('register')), 改动落在代理 URL 的外层 query 上, 而初始加载会按设计删掉
  // __readerPanelUrl。不并回真实 URL, 导航就会退回改动前的地址 —— 点注册永远
  // 停在登录页。同名参数以**外层为准**且整体替换(与 searchParams.set 一致),
  // 与服务端 merge_panel_outer_query 的语义严格对应。
  function mergeOuterQuery(from, to) {{
    var namespaced = from.searchParams.has("__readerPanelId");
    var skip = namespaced ? ["__readerPanelId", "__readerPanelUrl"] : ["panelId", "url"];
    var pending = [];
    from.searchParams.forEach(function (value, key) {{
      if (skip.indexOf(key) >= 0) return;
      // 已原样存在则跳过: 面板内导航的外层 query 常态是复制自上游, 此时不触碰
      // to 的 query, 避免 searchParams 重新序列化(%20 → +、%zz → %25zz)改写
      // 上游 URL 形式。
      if (to.searchParams.getAll(key).indexOf(value) >= 0) return;
      pending.push([key, value]);
    }});
    if (!pending.length) return to;
    // 无同名冲突时直接追加到原文末尾, 与 Rust 侧的纯追加路径一致, 不改写既有
    // query 片段; 有冲突才走 searchParams.delete + set(整体替换同名项)。
    var conflicts = pending.some(function (pair) {{ return to.searchParams.has(pair[0]); }});
    if (conflicts) {{
      for (var i = 0; i < pending.length; i++) {{
        to.searchParams.delete(pending[i][0]);
        to.searchParams.set(pending[i][0], pending[i][1]);
      }}
      return to;
    }}
    var extra = pending.map(function (pair) {{
      return encodeURIComponent(pair[0]) + "=" + encodeURIComponent(pair[1]);
    }}).join("&");
    var raw = to.search.replace(/^\?/, "");
    to.search = (raw ? raw + "&" : "") + extra;
    return to;
  }}
  function realUrl(raw) {{
    var url = new URL(raw, pageUrl || window.location.href);
    if (isPanelUrl(url)) {{
      var original = panelTarget(url) || initialPageUrl;
      if (original) {{
        var decoded = mergeOuterQuery(url, new URL(original));
        if (url.hash) decoded.hash = url.hash;
        return decoded;
      }}
    }}
    return url;
  }}
  function proxyUrl(url, storedHtml) {{
    var proxy = new URL("/sourcePanel", bridgeBase);
    proxy.search = url.search;
    proxy.searchParams.set("__readerPanelId", panelId);
    if (storedHtml) proxy.searchParams.delete("__readerPanelUrl");
    else proxy.searchParams.set("__readerPanelUrl", url.href);
    proxy.hash = url.hash;
    return proxy.href;
  }}
  // Pages read location.search during their first inline script (e.g. comment
  // book/chapter ids). Publish it before preload/page scripts, without fetching
  // again or turning a stored HTML panel into an upstream navigation on reload.
  if (/^https?:/.test(pageUrl)) {{
    var loadedUrl = new URL(window.location.href);
    window.history.replaceState(window.history.state, "", proxyUrl(new URL(pageUrl), !panelTarget(loadedUrl)));
  }}
  function updatePageUrl(url) {{
    pageUrl = url.href;
    window.__readerPanelUrl = pageUrl;
    var base = document.querySelector("base");
    if (base && /^https?:$/.test(url.protocol)) base.href = pageUrl;
    postToHost({{ type: "location", url: pageUrl }});
  }}
  function startNavigation(event) {{
    navigating = true;
    postToHost({{ type: "navigating" }});
    if (event && event.signal) event.signal.addEventListener("abort", function () {{
      navigating = false;
      postToHost({{ type: "navigationCancelled" }});
    }}, {{ once: true }});
  }}
  // 书源仓库的导入协议: legado://import/[path]?src=[json] 与
  // yuedu://booksource/importonline?src=[json](手机端 Legado 的
  // OnLineImportActivity / 源仓库页面用的都是这两个)。
  // 返回规范化后的 href, 非导入协议返回空串。
  function importProtocolUrl(raw) {{
    try {{
      var url = new URL(raw, pageUrl || window.location.href);
      if (!/^(legado|yuedu):$/i.test(url.protocol)) return "";
      return url.href;
    }} catch (error) {{
      return "";
    }}
  }}
  function requestImport(href) {{
    postToHost({{ type: "importProtocol", url: href }});
  }}
  function submitToProxy(event, url) {{
    var source = event.sourceElement;
    var original = source && (source.form || (source.tagName === "FORM" ? source : null));
    var form = document.createElement("form");
    form.method = "post";
    form.action = proxyUrl(url);
    form.enctype = source && source.getAttribute("formenctype") || original && original.enctype || "application/x-www-form-urlencoded";
    if (original) form.acceptCharset = original.acceptCharset;
    form.hidden = true;
    for (var pair of event.formData.entries()) {{
      var field = document.createElement("input");
      field.name = pair[0];
      if (typeof pair[1] === "string") {{
        field.type = "hidden";
        field.value = pair[1];
      }} else {{
        field.type = "file";
        var transfer = new DataTransfer();
        transfer.items.add(pair[1]);
        field.files = transfer.files;
      }}
      form.appendChild(field);
    }}
    (document.body || document.documentElement).appendChild(form);
    nativeSubmit.call(form);
    form.remove();
  }}
  if (window.navigation && typeof window.navigation.addEventListener === "function") {{
    window.navigation.addEventListener("navigate", function (event) {{
      if (event.downloadRequest || event.hashChange) return;
      var destination = new URL(event.destination.url);
      var source = event.sourceElement;
      var form = source && (source.form || (source.tagName === "FORM" ? source : null));
      var target = realUrl(destination.href);
      var rebuildGet = false;
      if (form && !event.formData) {{
        var action = new URL(source.getAttribute("formaction") || form.action || window.location.href, pageUrl || window.location.href);
        if (isPanelUrl(action)) {{
          // GET submissions replace the action query, including the proxy's id.
          target = realUrl(action.href);
          target.search = destination.search;
          target.hash = destination.hash;
          rebuildGet = true;
        }}
      }}
      if (event.destination.sameDocument && !rebuildGet) return;
      if (isPanelUrl(destination) && !rebuildGet) {{
        startNavigation(event);
        return;
      }}
      // 书源仓库的「一键导入」链接: legado://import/...?src= 与
      // yuedu://booksource/importonline?src=。这类协议系统浏览器不认识,
      // 交出去就是静默失败, 必须在面板里截下并转给宿主导入。
      var importTargetUrl = importProtocolUrl(destination.href) || importProtocolUrl(event.destination.url);
      if (importTargetUrl) {{
        event.preventDefault();
        requestImport(importTargetUrl);
        return;
      }}
      if (!/^https?:$/.test(target.protocol)) return;
      // 二级页面统一留在面板内。
      //
      // 早先这里把 RSS 面板的二级链接交给系统浏览器, 理由是上游的
      // X-Frame-Options / CSP frame-ancestors 会让 iframe 拒绝渲染(白屏)。
      // 但那个理由只在**直接内嵌上游地址**时成立: 现在跳转走 `proxyUrl`,
      // 由后端把页面抓回来重写后托管在 reader origin 上, 而代理只透传
      // Content-Type / Cache-Control / Set-Cookie, 不会转发这些限制头,
      // 所以经代理渲染不会白屏。改成留在面板内之后, 用户在源仓库里点进去
      // 的详情/列表页不再跳出应用。
      if (!event.cancelable) {{
        startNavigation(event);
        return;
      }}
      event.preventDefault();
      try {{
        if (!event.formData && pageUrl) {{
          var current = new URL(pageUrl);
          if (current.origin === target.origin && current.pathname === target.pathname && current.search === target.search && current.hash !== target.hash) {{
            updatePageUrl(target);
            window.location.hash = target.hash;
            return;
          }}
        }}
        if (event.formData) submitToProxy(event, target);
        else if (event.navigationType === "replace") window.location.replace(proxyUrl(target));
        else window.location.assign(proxyUrl(target));
      }} catch (error) {{
        navigating = false;
        postToHost({{ type: "navigationError", message: "网页跳转失败，请重新加载后重试：" + String(error) }});
      }}
    }});
  }}
  // Older webviews may not expose cancellable navigation. The host still
  // invalidates readiness when the frame leaves and verifies the next document.
  window.addEventListener("beforeunload", function () {{ if (!navigating) startNavigation(); }});

  // click 兜底: `window.navigation` 不可用时(老 WebView)上面的 navigate 拦截
  // 不会触发, 只能靠捕获阶段的 click 拦下 <a> 跳转。用事件委托而不是改写
  // href: 页面可能动态插入链接(JS 渲染的列表)。
  //
  // 两种面板合并成同一段逻辑(此前 RSS 与书源各写了一份, 行为还不一致):
  //   - `legado://` / `yuedu://` 导入协议 → 交给宿主导入;
  //   - http(s) 链接 → 一律改走代理, 留在面板内打开。
  //
  // RSS 面板早先把链接交给系统浏览器, 理由是上游 X-Frame-Options 会让 iframe
  // 白屏; 但那只是**直接内嵌上游地址**时的问题, 走 `proxyUrl` 后页面由后端
  // 抓取重写再托管, 代理不透传这些限制头, 因此不会白屏。
  document.addEventListener("click", function (event) {{
    if (event.defaultPrevented || event.button !== 0 || event.metaKey || event.ctrlKey || event.shiftKey || event.altKey) return;
    var anchor = event.target && event.target.closest ? event.target.closest("a[href]") : null;
    if (!anchor || anchor.hasAttribute("download")) return;
    var raw = anchor.getAttribute("href") || "";
    // 这里用单引号比较: 双引号加井号的字面量会提前终止外层的原始字符串。
    if (!raw || raw.charAt(0) === '#') return;
    if (/^(mailto|tel|javascript):/i.test(raw)) return;
    // 书源仓库的「一键导入」是自定义协议。系统浏览器不认识它们, 交出去只会
    // 静默失败, 必须在面板里截下并转给宿主。
    var importTarget = importProtocolUrl(raw);
    if (importTarget) {{
      event.preventDefault();
      requestImport(importTarget);
      return;
    }}
    var url;
    try {{ url = realUrl(raw); }} catch (error) {{ return; }}
    if (!/^https?:$/.test(url.protocol)) return;
    // 面板自身 URL(同文档跳转/桥导航)不拦截, 保持原有行为。
    if (isPanelUrl(url)) return;
    if (pageUrl) {{
      var current = new URL(pageUrl);
      if (current.origin === url.origin && current.pathname === url.pathname && current.search === url.search) return;
    }}
    // 现代 WebView 的 `window.navigation` 已经处理过这次跳转; 这里再拦一次
    // 会重复导航。只接管它没管的 `target="_blank"` 与老 WebView 的普通点击。
    if (window.navigation && typeof window.navigation.addEventListener === "function") {{
      var anchorTarget = (anchor.getAttribute("target") || "").toLowerCase();
      if (anchorTarget !== "_blank") return;
    }}
    event.preventDefault();
    try {{
      // 留在面板内跳转: iframe 的 src 变成代理地址, 页面由后端重写后渲染。
      window.location.assign(proxyUrl(url));
    }} catch (error) {{
      postToHost({{ type: "navigationError", message: "网页跳转失败，请重新加载后重试：" + String(error) }});
    }}
  }}, true);
  ["pushState", "replaceState"].forEach(function (method) {{
    var native = window.history[method];
    window.history[method] = function (state, title, url) {{
      if (arguments.length < 3 || url == null || !/^https?:/.test(pageUrl)) return native.apply(window.history, arguments);
      var target = realUrl(String(url));
      if (target.origin !== new URL(pageUrl).origin) throw new DOMException("History URL must stay on the page origin", "SecurityError");
      var result = native.call(window.history, state, title, proxyUrl(target));
      updatePageUrl(target);
      return result;
    }};
  }});
  window.addEventListener("popstate", function () {{
    var target = realUrl(window.location.href);
    if (/^https?:$/.test(target.protocol)) updatePageUrl(target);
  }});
  window.addEventListener("hashchange", function () {{
    if (!pageUrl) return;
    var target = new URL(pageUrl);
    target.hash = window.location.hash;
    updatePageUrl(target);
  }});

  // 面板内 console 转发到宿主日志, 便于排查
  async function bridgeAsync(action, payload, signal) {{
    var response = await nativeFetch(bridgeBase, {{
      method: "POST", headers: {{ "Content-Type": "text/plain;charset=UTF-8" }},
      body: JSON.stringify({{ action: action, payload: payload || {{}} }}), signal: signal
    }});
    if (!response.ok) throw new Error("bridge HTTP " + response.status);
    var result = await response.json();
    if (!result.ok) throw new Error(result.error || "书源操作失败");
    return result.value;
  }}


  // axios 在浏览器里**默认用 XMLHttpRequest**（只有显式 `adapter: 'fetch'` 才用 fetch）。
  // 段评 / 本章说这类页面是 SPA，全部走 axios —— 只重写 `fetch` 拦不住它们，
  // 请求会从 reader origin 直连上游：CORS 不放行、Cookie 也带不过去，服务端返空，
  // 表现为面板一片空白（宿主 5 秒后报「当前网页无法确认」）。
  // 这里把跨域 XHR 也接到同一个桥接口上。
  var NativeXHR = window.XMLHttpRequest;
  function installXhrBridge() {{
    function BridgedXHR() {{
      this.readyState = 0; this.status = 0; this.statusText = "";
      this.response = ""; this.responseText = ""; this.responseType = "";
      this.timeout = 0; this.withCredentials = false;
      this.upload = {{ addEventListener: function () {{}}, removeEventListener: function () {{}} }};
      this._headers = {{}}; this._responseHeaders = {{}};
      this._listeners = {{}}; this._url = null; this._method = "GET"; this._async = true;
    }}
    BridgedXHR.prototype.addEventListener = function (type, fn) {{
      (this._listeners[type] = this._listeners[type] || []).push(fn);
    }};
    BridgedXHR.prototype.removeEventListener = function (type, fn) {{
      var list = this._listeners[type]; if (!list) return;
      this._listeners[type] = list.filter(function (item) {{ return item !== fn; }});
    }};
    BridgedXHR.prototype._emit = function (type) {{
      var self = this;
      (this._listeners[type] || []).forEach(function (fn) {{
        try {{ fn.call(self, {{ type: type, target: self }}); }} catch (e) {{}}
      }});
      var handler = this["on" + type];
      if (typeof handler === "function") {{ try {{ handler.call(this, {{ type: type, target: this }}); }} catch (e) {{}} }}
    }};
    BridgedXHR.prototype.open = function (method, url, async) {{
      this._method = String(method || "GET").toUpperCase();
      this._url = new URL(String(url), pageUrl || window.location.href);
      this._async = async !== false;
      this.readyState = 1;
      this._emit("readystatechange");
    }};
    BridgedXHR.prototype.setRequestHeader = function (name, value) {{
      this._headers[String(name)] = String(value);
    }};
    BridgedXHR.prototype.getAllResponseHeaders = function () {{
      var self = this;
      return Object.keys(this._responseHeaders).map(function (name) {{
        return name + ": " + self._responseHeaders[name];
      }}).join("\r\n");
    }};
    BridgedXHR.prototype.getResponseHeader = function (name) {{
      var lowered = String(name).toLowerCase();
      var map = this._responseHeaders || {{}};
      var found = null;
      Object.keys(map).forEach(function (key) {{
        if (key.toLowerCase() === lowered) found = map[key];
      }});
      return found;
    }};
    BridgedXHR.prototype.abort = function () {{ this._aborted = true; }};
    BridgedXHR.prototype.send = function (body) {{
      var self = this;
      if (!this._url || this._url.origin === window.location.origin || !/^https?:$/.test(this._url.protocol)) {{
        // 同源/非 http 的请求交回原生实现（桥自身也用 XHR，必须走原生）。
        var native = new NativeXHR();
        this._native = native;
        ["onload", "onerror", "ontimeout", "onabort", "onprogress", "onreadystatechange"].forEach(function (name) {{
          if (typeof self[name] === "function") native[name] = function (event) {{ self[name].call(self, event); }};
        }});
        native.open(this._method, this._url ? this._url.href : "", this._async);
        Object.keys(this._headers).forEach(function (name) {{ try {{ native.setRequestHeader(name, self._headers[name]); }} catch (e) {{}} }});
        try {{ native.send(body); }} catch (e) {{ self._fail(e); }}
        this._native = native;
        // 让调用方读到原生对象的进度状态。
        ["status", "statusText", "responseText", "response", "responseType", "readyState"].forEach(function (name) {{
          Object.defineProperty(self, name, {{ get: function () {{ return native[name]; }}, configurable: true }});
        }});
        return;
      }}
      var payload = {{
        url: this._url.href, method: this._method, headers: this._headers,
        body: body == null ? "" : (typeof body === "string" ? body : String(body))
      }};
      bridgeAsync("fetch", payload).then(function (response) {{
        if (self._aborted) return;
        self.status = response.code || 200;
        self.statusText = response.statusText || "";
        self._responseHeaders = response.headers || {{}};
        self.responseText = response.body || "";
        self.response = response.body || "";
        self.readyState = 4;
        self._emit("readystatechange");
        self._emit("load");
        self._emit("loadend");
      }}).catch(function (error) {{
        self._fail(error);
      }});
    }};
    BridgedXHR.prototype._fail = function (error) {{
      if (this._aborted) return;
      this.status = 0; this.readyState = 4;
      this._error = error;
      this._emit("readystatechange");
      this._emit("error");
      this._emit("loadend");
    }};
    window.XMLHttpRequest = BridgedXHR;
  }}
  installXhrBridge();

  // Inline settings pages use the source's native requests for CORS and cookies.
  window.fetch = async function (input, init) {{
    var rawUrl = input instanceof Request ? input.url : String(input);
    var url = new URL(rawUrl, pageUrl || window.location.href);
    if (!/^https?:$/.test(url.protocol) || url.origin === window.location.origin) return nativeFetch(input, init);
    var request = new Request(url.href, input instanceof Request ? input : init);
    if (input instanceof Request && init) request = new Request(request, init);
    if (input instanceof Request || init && (init.body instanceof FormData || init.body instanceof Blob || init.body instanceof ArrayBuffer || ArrayBuffer.isView(init.body))) {{
      // Preserve multipart boundaries and binary bodies through the raw proxy;
      // relative form submissions must resolve against the upstream page.
      var sameOrigin = pageUrl && url.origin === new URL(pageUrl).origin;
      if (!sameOrigin) return nativeFetch(request);
      if (init && init.body != null) {{
        // 用原始 BodyInit 重建, 而不是把已构造的 Request 再包一层: 后者会让请求体
        // 变成 ReadableStream, 经自定义协议发送时可能送不出体(后端于是收不到任何
        // 表单字段, 某聚合源的注册接口回「无效的操作」)。原始 FormData/Blob 重建则由
        // 浏览器在发送时按真实字节生成 Content-Type 与 boundary。
        return nativeFetch(new Request(proxyUrl(url), init));
      }}
      return nativeFetch(new Request(proxyUrl(url), request));
    }}
    var headers = {{}};
    request.headers.forEach(function (value, name) {{ headers[name] = value; }});
    var body = /^(GET|HEAD)$/i.test(request.method) ? "" : await request.text();
    var response = await bridgeAsync("fetch", {{ url: url.href, method: request.method, headers: headers, body: body }}, request.signal);
    if (!response.code) throw new Error(response.error || "网络请求失败");
    var responseHeaders = new Headers(response.headers || {{}});
    ["set-cookie", "content-encoding", "content-length"].forEach(function (name) {{ responseHeaders.delete(name); }});
    return new Response([204, 205, 304].includes(response.code) || request.method === "HEAD" ? null : response.body, {{ status: response.code, headers: responseHeaders }});
  }};

  var rawLog = console.log.bind(console);
  console.log = function () {{
    var parts = Array.prototype.slice.call(arguments).map(function (v) {{
      return typeof v === "string" ? v : JSON.stringify(v);
    }});
    bridgeSync("log", {{ message: parts.join(" ") }});
    rawLog.apply(null, arguments);
  }};
  window.onerror = function (msg, src, line) {{
    bridgeSync("log", {{ message: "面板 JS 错误: " + msg + " @" + line }});
  }};

  // 后端把 headers 当 JSON 文本解析(见 parser/js.rs 的 run_panel_request),
  // 而插件习惯写 java.get(url, {{'Referer': ...}}) 传对象, 直接透传会被后端的
  // 字符串取值丢掉, 表现为自定义头静默失效。这里统一序列化成文本。
  function hdrJson(h) {{
    if (h === null || h === undefined) return "";
    if (typeof h === "string") return h;
    try {{ return JSON.stringify(h); }} catch (e) {{ return ""; }}
  }}

  window.java = {{
    ajax: function (spec) {{
      var r = bridgeSync("ajax", {{ spec: String(spec) }});
      return r.ok ? r.value : "";
    }},
    get: function (url, headers) {{
      var r = bridgeSync("http", {{ method: "GET", url: String(url), body: "", headers: hdrJson(headers) }});
      return r.ok ? r.value : "";
    }},
    post: function (url, body, headers) {{
      var r = bridgeSync("http", {{ method: "POST", url: String(url), body: String(body || ""), headers: hdrJson(headers) }});
      return r.ok ? r.value : "";
    }},
    connect: function (url, headers) {{
      var r = bridgeSync("connect", {{ spec: String(url), headers: hdrJson(headers) }});
      return r.ok ? r.value : "";
    }},
    log: function (m) {{ bridgeSync("log", {{ message: String(m) }}); return m; }},
    toast: function (m) {{ postToHost({{ type: "toast", message: String(m) }}); }},
    longToast: function (m) {{ postToHost({{ type: "toast", message: String(m) }}); }},
    upConfig: function (config) {{
      try {{ postToHost({{ type: "config", options: typeof config === "string" ? JSON.parse(config) : config }}); }} catch (e) {{}}
    }},
    upLoginData: function (values) {{ postToHost({{ type: "loginData", values: values }}); }},
    reLoginView: function () {{ postToHost({{ type: "refreshLogin" }}); }},
    getCache: function (k) {{
      var r = bridgeSync("cacheGet", {{ key: String(k) }});
      return r.ok ? (r.value === undefined ? null : r.value) : null;
    }},
    putCache: function (k, v) {{ bridgeSync("cachePut", {{ key: String(k), value: String(v) }}); return true; }},
    // 面板需要与书源规则相同的加密能力(书源用它给接口签名)。
    // 缺了这些方法插件会降级到 CryptoJS 回退, 那条路径依赖注入 <script>, 容易失败。
    createSymmetricCrypto: function (transformation, key, iv) {{
      var t = String(transformation || "");
      var k = String(key == null ? "" : key);
      var v = String(iv == null ? "" : iv);
      function run(op, data, format) {{
        var r = bridgeSync("crypto", {{
          op: op, transformation: t, key: k, iv: v,
          data: String(data == null ? "" : data), format: format
        }});
        return r.ok ? String(r.value) : "";
      }}
      return {{
        encrypt: function (d) {{ return run("enc", d, "base64"); }},
        encryptBase64: function (d) {{ return run("enc", d, "base64"); }},
        encryptHex: function (d) {{ return run("enc", d, "hex"); }},
        decrypt: function (d) {{ return run("dec", d, "auto"); }},
        decryptStr: function (d) {{ return run("dec", d, "auto"); }},
        setIv: function (next) {{ v = String(next == null ? "" : next); return this; }}
      }};
    }},
    aesEncodeToBase64String: function (d, k, t, iv) {{
      return window.java.createSymmetricCrypto(t, k, iv).encryptBase64(d);
    }},
    aesEncodeToString: function (d, k, t, iv) {{
      return window.java.createSymmetricCrypto(t, k, iv).encryptHex(d);
    }},
    aesDecodeToString: function (d, k, t, iv) {{
      return window.java.createSymmetricCrypto(t, k, iv).decryptStr(d);
    }},
    desEncodeToBase64String: function (d, k, t, iv) {{
      return window.java.createSymmetricCrypto(t, k, iv).encryptBase64(d);
    }},
    desEncodeToString: function (d, k, t, iv) {{
      return window.java.createSymmetricCrypto(t, k, iv).encryptHex(d);
    }},
    desDecodeToString: function (d, k, t, iv) {{
      return window.java.createSymmetricCrypto(t, k, iv).decryptStr(d);
    }},
    tripleDESEncodeBase64Str: function (d, k, m, p, iv) {{
      return window.java.createSymmetricCrypto("DESede/" + m + "/" + p, k, iv).encryptBase64(d);
    }},
    tripleDESDecodeStr: function (d, k, m, p, iv) {{
      return window.java.createSymmetricCrypto("DESede/" + m + "/" + p, k, iv).decryptStr(d);
    }},
    // 面板里会检测这些哈希/编码方法是否存在
    md5Encode: function (v) {{ var r = bridgeSync("digest", {{ kind: "md5", input: String(v) }}); return r.ok ? String(r.value) : ""; }},
    base64Encode: function (v) {{ var r = bridgeSync("digest", {{ kind: "base64", input: String(v) }}); return r.ok ? String(r.value) : ""; }},
    base64Decode: function (v) {{ var r = bridgeSync("digest", {{ kind: "unbase64", input: String(v) }}); return r.ok ? String(r.value) : ""; }},
    // 面板内再次弹层: 后端登记为新面板, 由桥通知宿主切换
    showBrowser: function (url, html, preloadJs, options) {{
      var r = bridgeSync("showBrowser", {{ url: String(url), html: html == null ? null : String(html), preloadJs: preloadJs == null ? null : String(preloadJs), options: options }});
      if (r.ok && r.value) {{
        postToHost({{ type: "showBrowser", request: r.value }});
      }}
      return r.ok;
    }}
  }};

  function sourceGet(key) {{ var r = bridgeSync("sourceGet", {{ key: key }}); return r.ok ? r.value : null; }}
  function sourceSet(key, value) {{ return bridgeSync("sourceSet", {{ key: key, value: value == null ? null : String(value) }}).ok; }}
  function sourceMap(key) {{ try {{ return JSON.parse(sourceGet(key) || "{{}}"); }} catch (e) {{ return {{}}; }} }}
  window.source = {{
    getKey: function () {{ return window.__readerSourceKey || ""; }},
    getVariable: function () {{ return sourceGet("variable") || ""; }},
    setVariable: function (v) {{ return sourceSet("variable", v); }},
    putVariable: function (v) {{ return sourceSet("variable", v); }},
    getLoginInfo: function () {{ return sourceGet("loginInfo"); }},
    getLoginInfoMap: function () {{ return sourceMap("loginInfo"); }},
    putLoginInfo: function (v) {{ return sourceSet("loginInfo", v); }},
    removeLoginInfo: function () {{ return sourceSet("loginInfo", null); }},
    getLoginHeader: function () {{ return sourceGet("loginHeader"); }},
    getLoginHeaderMap: function () {{ return sourceMap("loginHeader"); }},
    putLoginHeader: function (v) {{ return sourceSet("loginHeader", typeof v === "string" ? v : JSON.stringify(v)); }},
    removeLoginHeader: function () {{ return sourceSet("loginHeader", null); }},
    get: function (k) {{ return window.java.getCache(k) || ""; }},
    put: function (k, v) {{ return window.java.putCache(k, v); }}
  }};

  window.cache = {{
    get: function (k) {{ return window.java.getCache(k); }},
    put: function (k, v) {{ return window.java.putCache(k, v); }},
    getFromMemory: function (k) {{ return window.java.getCache(k); }},
    putMemory: function (k, v) {{ return window.java.putCache(k, v); }},
    // 插件用它缓存 crypto-js 等大体积库(避免每次同步拉 CDN)
    getFile: function (k) {{
      var r = bridgeSync("cacheGet", {{ key: "file:" + String(k) }});
      return r.ok ? (r.value === undefined || r.value === null ? "" : r.value) : "";
    }},
    putFile: function (k, v) {{
      bridgeSync("cachePut", {{ key: "file:" + String(k), value: String(v) }});
      return v;
    }},
    "delete": function (k) {{ bridgeSync("cachePut", {{ key: String(k), value: "" }}); }}
  }};

  // window.run(script): 在宿主书源 jsLib 作用域里执行脚本, 返回 Promise
  window.run = async function (script) {{
    var value = await bridgeAsync("run", {{ script: String(script) }});
    if (value.showBrowser) postToHost({{ type: "showBrowser", request: value.showBrowser }});
    var effects = value.loginEffects || {{}};
    if (effects.reset) postToHost({{ type: "loginData", values: null }});
    if (effects.values && Object.keys(effects.values).length) postToHost({{ type: "loginData", values: effects.values }});
    if (effects.refresh) postToHost({{ type: "refreshLogin" }});
    (value.logs || []).forEach(function (line) {{ if (line.startsWith("[toast]")) postToHost({{ type: "toast", message: line.slice(7).trim() }}); }});
    return value.result;
  }};

  window.java.run = window.run;
  window.ajaxAwait = function (url) {{ return bridgeAsync("ajax", {{ spec: String(url) }}); }};

  function snapshotDocument() {{
    var copy = document.documentElement.cloneNode(true);
    var live = document.querySelectorAll("input,textarea,select");
    var cloned = copy.querySelectorAll("input,textarea,select");
    live.forEach(function (field, index) {{
      var target = cloned[index];
      if (field.tagName === "TEXTAREA") target.textContent = field.value;
      else if (field.tagName === "SELECT") Array.from(target.options).forEach(function (option, i) {{ option.toggleAttribute("selected", field.options[i].selected); }});
      else {{ target.setAttribute("value", field.value); target.toggleAttribute("checked", field.checked); }}
    }});
    return "<!DOCTYPE html>" + copy.outerHTML;
  }}
window.addEventListener("message", function (event) {{
    if (event.source !== window.parent || !event.data || event.data.panelId !== panelId) return;
    if (event.data.type === "reader:ready") {{
      if (!navigating) ready(event.data.requestId);
      return;
    }}
    if (event.data.type !== "reader:confirm" || event.data.documentId !== documentId) return;
    var requestId = event.data.requestId;
    if (navigating) {{
      postToHost({{ type: "error", requestId: requestId, message: "网页正在跳转，请加载完成后再确认" }});
      return;
    }}
    var r;
    try {{ r = bridgeSync("complete", {{ url: pageUrl, body: snapshotDocument() }}); }}
    catch (error) {{ r = {{ ok: false, error: String(error) }}; }}
    postToHost(r.ok ? {{ type: "complete", requestId: requestId }} : {{ type: "error", requestId: requestId, message: r.error }});
  }});
  function ready(requestId) {{
    try {{
      bridgeSync("log", {{ message: "[bridge] ready id=" + requestId + " rs=" + document.readyState + " scripts=" + document.scripts.length + " body=" + (document.body ? document.body.innerHTML.length : -1) }});
    }} catch (e) {{}}
    postToHost({{ type: "ready", requestId: requestId, title: document.title, url: pageUrl }});
  }}
  if (document.readyState === "loading") document.addEventListener("DOMContentLoaded", function () {{ ready(); }}, {{ once: true }});
  else ready();

  // 插件用 window.close() 关闭面板。
  // iframe 内 window.close() 通常无效(浏览器只允许关脚本自开的窗口),
  // 因此先通知宿主关掉弹层, 再尝试原生 close。
  window.close = function () {{
    postToHost({{ type: "close" }});
  }};
}})();
</script>"#
    )
}

/// 从主机名取可注册域(registrable domain), 用于判断两个主机是否属同一站点。
///
/// 注意这是**近似实现**: 只覆盖常见的多标签公共后缀, 不读取完整的 Public
/// Suffix List(`publicsuffix` crate 是动态列表, 需要额外的数据源, 与这里
/// 「不引入运行时下载依赖」的前提冲突)。表外的多标签后缀会被当成普通域名,
/// 例如 `www.abc.co.nz` 与 `evil.co.nz` 都会被折叠成 `co.nz` 而**误判为同站** ——
/// 因此调用方不得把「同站」当作安全边界, 见 `set_panel_cookie` 的说明。
fn registrable_domain_from_host(host: &str) -> String {
    let host = host.trim_end_matches('.').to_ascii_lowercase();
    let parts: Vec<&str> = host.split('.').filter(|part| !part.is_empty()).collect();
    if parts.len() < 2 {
        return host;
    }
    // 常见多标签公共后缀。表越长越准, 但**永远不可能完备**, 这是本函数近似的根源。
    const TWO_LABEL_SUFFIXES: &[&str] = &[
        "co.uk", "org.uk", "me.uk", "ac.uk", "gov.uk", "net.uk", "sch.uk",
        "com.cn", "net.cn", "org.cn", "gov.cn", "edu.cn", "ac.cn",
        "com.au", "net.au", "org.au", "edu.au", "gov.au",
        "co.jp", "ne.jp", "or.jp", "ac.jp", "go.jp",
        "co.kr", "or.kr", "ne.kr", "go.kr", "re.kr",
        "com.br", "net.br", "org.br", "gov.br",
        "com.hk", "org.hk", "net.hk", "edu.hk", "gov.hk",
        "com.tw", "org.tw", "net.tw", "edu.tw", "gov.tw",
        "co.in", "net.in", "org.in", "gen.in", "firm.in",
        "co.nz", "net.nz", "org.nz", "govt.nz", "ac.nz",
        "com.sg", "net.sg", "org.sg", "edu.sg", "gov.sg",
        "com.my", "net.my", "org.my", "edu.my", "gov.my",
        "co.id", "or.id", "ac.id", "go.id", "web.id",
        "co.th", "or.th", "ac.th", "go.th", "in.th",
        "co.za", "net.za", "org.za", "gov.za", "ac.za",
        "com.ar", "net.ar", "org.ar", "gov.ar", "edu.ar",
        "com.mx", "net.mx", "org.mx", "gob.mx", "edu.mx",
        "com.tr", "net.tr", "org.tr", "gov.tr", "edu.tr",
        "co.il", "org.il", "net.il", "ac.il", "gov.il",
        "com.ua", "net.ua", "org.ua", "gov.ua", "edu.ua",
        "com.vn", "net.vn", "org.vn", "gov.vn", "edu.vn",
        "co.jp", "com.ph", "com.pk", "com.eg", "com.sa",
        "com.co", "com.pe", "com.ve", "com.uy", "com.ec",
        "co.ke", "co.tz", "co.ug", "co.zw", "co.bw",
    ];
    let suffix = format!("{}.{}", parts[parts.len() - 2], parts[parts.len() - 1]);
    if parts.len() >= 3 && TWO_LABEL_SUFFIXES.contains(&suffix.as_str()) {
        return parts[parts.len() - 3..].join(".");
    }
    parts[parts.len() - 2..].join(".")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    #[ignore = "reads an explicitly supplied source export and its public registration page"]
    async fn live_source_registration_page() {
        let path = std::env::var("READER_REGISTRATION_SOURCE_FILE").expect("source JSON path");
        let value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
        let source = crate::model::book_source::book_source_from_value(value).unwrap();
        let target = std::env::var("READER_REGISTRATION_URL").expect("public registration URL");
        let http = crate::crawler::http_client::HttpClient::new(20, None).unwrap();
        let context = http.source_context("registration-smoke", &source.book_source_url, None).unwrap();
        let entry = crate::service::book_service::SourcePanelEntry {
            url: target.clone(), book_source_url: source.book_source_url.clone(), http: Some(context), ..Default::default()
        };
        let response = fetch_source_panel_response(&source, "fixture-settings", &entry, &target, &http::Request::new(Vec::new())).await.unwrap();
        assert_eq!(response.status(), 200);
        let html = String::from_utf8(response.into_body()).unwrap();
        assert!(html.contains("reg-email"));
        assert!(html.contains("window.java ="));
        std::fs::create_dir_all("target/source-panel-fixtures").unwrap();
        std::fs::write("target/source-panel-fixtures/registration.html", html).unwrap();
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn panel_pages_and_form_posts_evaluate_dynamic_source_headers() {
        use axum::{body::Bytes, http::HeaderMap, routing::get, Router};
        use crate::crawler::http_client::HttpClient;
        use crate::service::book_service::SourcePanelEntry;

        let app = Router::new().route("/register", get(|headers: HeaderMap| async move {
            assert_eq!(headers.get("user-agent").unwrap(), "Legado/3.0");
            assert!(headers.get("authorization").is_none());
            ([("content-type", "text/html")], "<html><head><title>注册</title></head><body><input id=\"reg-email\"><script src=\"/static/auth.js\"></script></body></html>")
        }).post(|headers: HeaderMap, body: Bytes| async move {
            assert_eq!(headers.get("authorization").unwrap(), "Bearer fixture-token");
            assert_eq!(headers.get("content-type").unwrap(), "multipart/form-data; boundary=fixture");
            assert_eq!(body.as_ref(), b"--fixture\r\nContent-Disposition: form-data; name=\"email\"\r\n\r\nreader@example.test\r\n--fixture--\r\n");
            ([("content-type", "application/json")], r#"{"code":200}"#)
        })).route("/static/auth.js", get(|| async { ([("content-type", "text/javascript")], "window.registrationReady = true;") }));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let source = BookSource {
            book_source_url: format!("custom://rapid/{}", uuid::Uuid::new_v4()),
            header: Some("@js:var v=JSON.parse(source.getVariable()||'{}');JSON.stringify(v.token?{'Authorization':'Bearer '+v.token,'User-Agent':'Legado/3.0'}:{'User-Agent':'Legado/3.0'})".into()),
            ..Default::default()
        };
        let http = HttpClient::new(3, None).unwrap();
        let context = http.source_context("panel-registration", &source.book_source_url, None).unwrap();
        let entry = SourcePanelEntry { url: format!("{base}/register"), book_source_url: source.book_source_url.clone(), http: Some(context.clone()), ..Default::default() };
        let response = fetch_source_panel_response(&source, "registration", &entry, &entry.url, &http::Request::new(Vec::new())).await.unwrap();
        assert_eq!(response.status(), 200);
        let html = String::from_utf8(response.into_body()).unwrap();
        assert!(html.contains("reg-email"));
        assert!(html.contains("window.java ="));
        assert!(html.contains("__readerPanelUrl="));
        let script = fetch_source_panel_response(&source, "registration", &entry, &format!("{base}/static/auth.js"), &http::Request::new(Vec::new())).await.unwrap();
        assert_eq!(script.body(), b"window.registrationReady = true;");
        context.session.put("variable", Some(r#"{"token":"fixture-token"}"#)).unwrap();
        let request = http::Request::builder().method("POST")
            .header("content-type", "multipart/form-data; boundary=fixture")
            .body(b"--fixture\r\nContent-Disposition: form-data; name=\"email\"\r\n\r\nreader@example.test\r\n--fixture--\r\n".to_vec()).unwrap();
        let response = fetch_source_panel_response(&source, "registration", &entry, &entry.url, &request).await.unwrap();
        assert_eq!(response.status(), 200);
        assert_eq!(response.body(), br#"{"code":200}"#);
        server.abort();
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn panel_form_post_infers_missing_content_type() {
        // 某聚合源的注册页用 FormData 提交(send_code), WebView2 交给宿主的自定义
        // 协议请求可能不带 Content-Type。后端(PHP)缺该头时完全不解析请求体,
        // 表现为注册接口回「无效的操作」。这里断言转发时按体的真实形态补齐,
        // 且 multipart 的 boundary 取自体本身(不能一律按 urlencoded 兜底, 那会
        // 把 multipart 体错标, 照样解析不出字段)。
        use axum::{body::Bytes, http::HeaderMap, routing::post, Router};
        use crate::crawler::http_client::HttpClient;
        use crate::service::book_service::SourcePanelEntry;

        let seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
        let sink = seen.clone();
        let app = Router::new().route("/register_api", post(move |headers: HeaderMap, body: Bytes| {
            let sink = sink.clone();
            async move {
                sink.lock().unwrap().push(format!(
                    "{}|{}",
                    headers.get("content-type").and_then(|v| v.to_str().ok()).unwrap_or("<none>"),
                    String::from_utf8_lossy(&body)
                ));
                ([("content-type", "application/json")], r#"{"success":false}"#)
            }
        }));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let source = BookSource { book_source_url: base.clone(), ..Default::default() };
        let http = HttpClient::new(3, None).unwrap();
        let context = http.source_context("panel-ct", &source.book_source_url, None).unwrap();
        let entry = SourcePanelEntry {
            url: format!("{base}/register_api"),
            book_source_url: source.book_source_url.clone(),
            http: Some(context.clone()),
            ..Default::default()
        };
        let target = format!("{base}/register_api");

        // 1. multipart 体, 无 Content-Type -> 必须推断出体的 boundary
        let multipart = b"--fixture\r\nContent-Disposition: form-data; name=\"action\"\r\n\r\nsend_code\r\n--fixture--\r\n".to_vec();
        let request = http::Request::builder().method("POST").body(multipart).unwrap();
        let response = fetch_source_panel_response(&source, "ct", &entry, &target, &request).await.unwrap();
        assert_eq!(response.status(), 200);

        // 2. urlencoded 体, 无 Content-Type -> 兜底为 urlencoded
        let urlencoded = b"action=send_code&email=reader%40example.test".to_vec();
        let request = http::Request::builder().method("POST").body(urlencoded).unwrap();
        fetch_source_panel_response(&source, "ct", &entry, &target, &request).await.unwrap();

        // 3. 显式给定且与体一致(同 boundary)的 Content-Type 原样保留
        let request = http::Request::builder().method("POST")
            .header("content-type", "multipart/form-data; boundary=explicit")
            .body(b"--explicit\r\nContent-Disposition: form-data; name=\"action\"\r\n\r\nsend_code\r\n--explicit--\r\n".to_vec())
            .unwrap();
        fetch_source_panel_response(&source, "ct", &entry, &target, &request).await.unwrap();

        // 4. 头与体不符(multipart 体却声明 urlencoded) -> 必须按体重写,
        //    否则后端照样解析不出字段(实测即「无效的操作」)
        let request = http::Request::builder().method("POST")
            .header("content-type", "application/x-www-form-urlencoded")
            .body(b"--mismatch\r\nContent-Disposition: form-data; name=\"action\"\r\n\r\nsend_code\r\n--mismatch--\r\n".to_vec())
            .unwrap();
        fetch_source_panel_response(&source, "ct", &entry, &target, &request).await.unwrap();

        // 5. 非 multipart 体带显式头 -> 不覆盖调用方给的头
        let request = http::Request::builder().method("POST")
            .header("content-type", "application/json")
            .body(br#"{"action":"send_code"}"#.to_vec())
            .unwrap();
        fetch_source_panel_response(&source, "ct", &entry, &target, &request).await.unwrap();

        let recorded = seen.lock().unwrap().clone();
        assert_eq!(recorded.len(), 5);
        assert!(recorded[0].starts_with("multipart/form-data; boundary=fixture|"), "缺头时必须从体推断 boundary: {}", recorded[0]);
        assert!(recorded[0].contains("action"), "体必须完整转发: {}", recorded[0]);
        assert!(recorded[1].starts_with("application/x-www-form-urlencoded"), "非 multipart 体按 urlencoded 兜底: {}", recorded[1]);
        assert!(recorded[2].starts_with("multipart/form-data; boundary=explicit|"), "与体一致的头不得被改写: {}", recorded[2]);
        assert!(recorded[3].starts_with("multipart/form-data; boundary=mismatch|"), "头与体不符时必须按体重写: {}", recorded[3]);
        assert!(recorded[4].starts_with("application/json|"), "非 multipart 体的显式头不得被覆盖: {}", recorded[4]);
        server.abort();
    }

    #[test]
    fn multipart_boundary_is_taken_from_body() {
        // 体的首行即 `--boundary`; 非 multipart 体不得被误判。
        assert_eq!(
            multipart_boundary(b"--fixture\r\nContent-Disposition: form-data\r\n").as_deref(),
            Some("fixture")
        );
        assert_eq!(
            multipart_boundary(b"--WebKitFormBoundaryABC123\n").as_deref(),
            Some("WebKitFormBoundaryABC123")
        );
        assert_eq!(multipart_boundary(b"action=send_code&email=x"), None);
        assert_eq!(multipart_boundary(b""), None);
        // 只有 `--` 没有内容 / 含空白与控制字符 -> 不是合法 boundary
        assert_eq!(multipart_boundary(b"--\r\n"), None);
        assert_eq!(multipart_boundary(b"--bad boundary\r\n"), None);
        assert_eq!(multipart_boundary(b"--bad\tboundary\r\n"), None);
        // 超长(>70)不是合法 boundary
        let long = format!("--{}\r\n", "a".repeat(71));
        assert_eq!(multipart_boundary(long.as_bytes()), None);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn panel_cookie_writes_reach_source_jar() {
        // 某聚合源的登录页用 `document.cookie = 'user_token=…'` 记录会话, 而 /user_center
        // 由**服务端**读 Cookie 判断登录态。只写浏览器的话这份 cookie 留在面板
        // origin(reader.localhost)下, 转发请求带不上, 上游就把页面踢回登录页
        // (用户看到「登录过期」)。这里断言: 面板页写入的 cookie 能落到书源 jar,
        // 从而让后续转发请求带上它。
        use axum::{http::HeaderMap, routing::get, Router};
        use crate::crawler::http_client::HttpClient;
        use crate::service::book_service::SourcePanelEntry;

        let seen = std::sync::Arc::new(std::sync::Mutex::new(String::new()));
        let sink = seen.clone();
        let app = Router::new().route("/user_login", get(move |headers: HeaderMap| async move {
            *sink.lock().unwrap() = headers.get("cookie").and_then(|v| v.to_str().ok()).unwrap_or("").to_string();
            ([("content-type", "text/html")], "<html><head></head><body>登录</body></html>")
        }));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

        let source = BookSource { book_source_url: base.clone(), ..Default::default() };
        let http = HttpClient::new(3, None).unwrap();
        let context = http.source_context("panel-cookie-write", &source.book_source_url, None).unwrap();
        let entry = SourcePanelEntry {
            url: format!("{base}/user_login"),
            book_source_url: source.book_source_url.clone(),
            http: Some(context.clone()),
            ..Default::default()
        };
        // 桥按 panelId 从全局注册表取面板, 因此必须登记。
        let panel_id = crate::service::book_service::register_source_panel(entry.clone());

        // jar 为空时也必须注入劫持脚本 —— 登录前 jar 正是空的, 而回写方向与
        // jar 是否为空无关; 少了这一步页面写入的 cookie 就永远进不了 jar。
        let response = fetch_source_panel_response(&source, &panel_id, &entry, &entry.url, &http::Request::new(Vec::new())).await.unwrap();
        let html = String::from_utf8(response.into_body()).unwrap();
        assert!(html.contains("Object.defineProperty(document, 'cookie'"), "jar 为空时也要注入劫持脚本");
        assert!(html.contains("__readerPanelQueueCookie"), "setter 必须把写入排队回传: {}", html.chars().take(300).collect::<String>());
        assert!(html.contains("__readerPanelBridgeReady"), "桥应置就绪标志以触发补发");
        assert!(html.contains("__readerPanelCookieFlush"), "桥应触发排队项的补发");

        // 首次请求 jar 里没有 cookie
        assert!(!seen.lock().unwrap().contains("user_token"), "首次请求不应带 cookie");

        // 走真实的桥动作提交 cookie(面板页写入 document.cookie 后桥走同步 XHR)。
        // 这里直接调用 cookieSet 的实现函数 —— 该分支不使用 book_source_service,
        // 因此可以只验证 cookie 落 jar 与越域拦截这两条逻辑。
        let bridge_set = |url: &str, cookie: &str| {
            crate::service::book_service::set_panel_cookie(&entry, url, cookie)
        };
        bridge_set(&entry.url, "user_token=token-abc123").expect("cookieSet 应成功写入 jar");

        // 再次请求: 转发必须带上刚才写入的 cookie
        fetch_source_panel_response(&source, &panel_id, &entry, &entry.url, &http::Request::new(Vec::new())).await.unwrap();
        assert!(seen.lock().unwrap().contains("user_token=token-abc123"), "写入 jar 的 cookie 应随转发请求送达: {}", seen.lock().unwrap());

        // 越域的 cookie 目标必须被拒绝(不能把会话写到书源域之外)
        assert!(bridge_set("https://evil.example.com/", "user_token=leak").is_err(), "跨域 cookie 目标必须被拒绝");
        assert!(!context.session.cookies.get(&url::Url::parse("https://evil.example.com/").unwrap()).contains("user_token"), "不得写入书源域之外的 jar");

        // 非法 cookie 串(无 `=`)不得写入
        assert!(bridge_set(&entry.url, "not-a-cookie").is_err(), "非法 cookie 应被拒绝");

        // 含控制字符的 cookie 不得写入(header 注入边界收紧在写入处)
        assert!(bridge_set(&entry.url, "evil=va\r\nlue").is_err(), "含 CRLF 的 cookie 应被拒绝");
        assert!(bridge_set(&entry.url, "ev\ril=value").is_err(), "名字含控制字符应被拒绝");

        // https 源不得把会话写进 http 目标(cookie 不区分 scheme, 明文请求会带上它)。
        // 注意本测试的 entry 是 http(本地 axum), 因此这里用 https 源单独验。
        let https_source = BookSource { book_source_url: "https://secure.example.test".to_string(), ..Default::default() };
        let https_http = HttpClient::new(3, None).unwrap();
        let https_context = https_http.source_context("panel-https", &https_source.book_source_url, None).unwrap();
        let https_entry = SourcePanelEntry {
            url: "https://secure.example.test/login".to_string(),
            book_source_url: https_source.book_source_url.clone(),
            http: Some(https_context.clone()),
            ..Default::default()
        };
        assert!(
            crate::service::book_service::set_panel_cookie(&https_entry, "http://secure.example.test/login", "user_token=x").is_err(),
            "https 源写入 http 目标必须被拒绝"
        );
        assert!(
            crate::service::book_service::set_panel_cookie(&https_entry, "https://secure.example.test/login", "user_token=x").is_ok(),
            "https 源写入 https 目标应放行"
        );

        // 空值代表页面删除 cookie(退出登录, 该页的 userLogout 即写 `user_token=;`
        // 配合过去时间)。必须从 jar 移除, 而不是留下空值发给上游。
        bridge_set(&entry.url, "user_token=").expect("删除应成功");
        fetch_source_panel_response(&source, &panel_id, &entry, &entry.url, &http::Request::new(Vec::new())).await.unwrap();
        assert!(!seen.lock().unwrap().contains("user_token"), "删除后的 cookie 不应再被转发: {}", seen.lock().unwrap());

        server.abort();
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn panel_html_exposes_source_jar_cookies_to_page_scripts() {
        // 这类站点的前端用 document.cookie 判断登录态(getCookie('qttoken')),
        // 面板 HTML 响应需要把书源 jar 的 cookie 以 Set-Cookie 透出给面板 origin。
        use axum::{http::HeaderMap, routing::get, Router};
        use crate::crawler::http_client::HttpClient;
        use crate::service::book_service::SourcePanelEntry;

        let seen = std::sync::Arc::new(std::sync::Mutex::new(String::new()));
        let sink = seen.clone();
        let app = Router::new().route("/user", get(move |headers: HeaderMap| async move {
            *sink.lock().unwrap() = headers.get("cookie").and_then(|v| v.to_str().ok()).unwrap_or("").to_string();
            ([("content-type", "text/html")], "<html><head><title>用户后台</title></head><body>用户中心</body></html>")
        }));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

        let source = BookSource { book_source_url: format!("https://aggregate-{}.test", uuid::Uuid::new_v4()), ..Default::default() };
        let http = HttpClient::new(3, None).unwrap();
        let context = http.source_context("panel-cookie", &source.book_source_url, None).unwrap();
        let url = url::Url::parse(&format!("{base}/user")).unwrap();
        context.session.cookies.set(&url, "qttoken=localtoken123456789; deviceId=device-fixture").unwrap();
        let entry = SourcePanelEntry { url: url.to_string(), book_source_url: source.book_source_url.clone(), http: Some(context.clone()), ..Default::default() };

        let response = fetch_source_panel_response(&source, "user", &entry, &entry.url, &http::Request::new(Vec::new())).await.unwrap();
        assert_eq!(response.status(), 200);
        // 转发请求带上 jar cookie
        assert!(seen.lock().unwrap().contains("qttoken=localtoken123456789"), "上游应收到 jar 中的 qttoken");
        // 面板 origin 的 JS 可读到透出的 cookie（每个 cookie 对一个 Set-Cookie 头）
        let set_cookies: Vec<&str> = response.headers().get_all("set-cookie").iter()
            .filter_map(|v| v.to_str().ok()).collect();
        assert!(set_cookies.iter().any(|c| c.starts_with("qttoken=localtoken123456789;")), "应透出 qttoken: {set_cookies:?}");
        assert!(set_cookies.iter().any(|c| c.starts_with("deviceId=device-fixture;")), "应透出 deviceId: {set_cookies:?}");
        // 不依赖 WebView cookie 行为: HTML 内注入 document.cookie 劫持脚本,
        // jar cookie 以字面量嵌入（页面 getCookie('qttoken') 直接可读）
        let html = String::from_utf8(response.into_body()).unwrap();
        assert!(html.contains("jarCookie"), "应注入 jar cookie 变量");
        assert!(html.contains("qttoken=localtoken123456789"), "jar cookie 应嵌入注入脚本: {}", html.chars().take(200).collect::<String>());
        assert!(html.contains("deviceId=device-fixture"), "jar cookie 应完整嵌入(含 deviceId)");
        assert!(html.contains("Object.defineProperty(document, 'cookie'"), "应劫持 document.cookie");
        assert!(html.matches("<script>").count() >= 2, "注入脚本应在页面 head 中先于页面脚本执行");
        server.abort();
    }

    #[test]
    fn reader_scheme_origin_matches_platform() {
        let origin = reader_scheme_origin();
        if cfg!(any(target_os = "windows", target_os = "android")) {
            assert_eq!(origin, "http://reader.localhost");
        } else {
            assert_eq!(origin, "reader://localhost");
        }
    }

    #[test]
    fn panel_bridge_rejects_foreign_origins() {
        // 同源(浏览器同步 XHR 不带 Origin)放行
        assert!(panel_bridge_origin_allowed(None));
        // 面板自身 origin 放行
        assert!(panel_bridge_origin_allowed(Some(reader_scheme_origin())));
        // 外部网页一律拒绝: 否则任何本机页面都能借桥驱动宿主的网络/脚本能力
        assert!(!panel_bridge_origin_allowed(Some("https://evil.example")));
        assert!(!panel_bridge_origin_allowed(Some("http://tauri.localhost")));
        // 不能因为大小写/空白差异被绕过
        assert!(!panel_bridge_origin_allowed(Some("HTTP://EVIL.EXAMPLE")));
        assert!(panel_bridge_origin_allowed(Some(&format!(
            "  {}  ",
            reader_scheme_origin()
        ))));
    }

    #[test]
    fn panel_bridge_response_echoes_origin_without_wildcard() {
        let echoed = json_response_panel(
            200,
            serde_json::json!({ "ok": true }),
            Some(reader_scheme_origin()),
        );
        assert_eq!(
            echoed
                .headers()
                .get(http::header::ACCESS_CONTROL_ALLOW_ORIGIN)
                .and_then(|v| v.to_str().ok()),
            Some(reader_scheme_origin())
        );
        // 同源请求不回 CORS 头, 更不能出现通配符
        let same_origin = json_response_panel(200, serde_json::json!({ "ok": true }), None);
        assert!(same_origin
            .headers()
            .get(http::header::ACCESS_CONTROL_ALLOW_ORIGIN)
            .is_none());
    }


    #[test]
    fn registrable_domain_covers_common_multi_label_suffixes() {
        // 多标签公共后缀必须整体保留, 否则不同注册域会被误判为同站
        // (cookie 写入授权复用了这个判定, 误判即等于放开跨域写入)。
        for (host, expected) in [
            ("www.abc.co.nz", "abc.co.nz"),
            ("evil.co.nz", "evil.co.nz"),
            ("www.shop.com.sg", "shop.com.sg"),
            ("a.b.com.my", "b.com.my"),
            ("www.abc.gov.cn", "abc.gov.cn"),
            ("x.ac.uk", "x.ac.uk"),
            ("www.example.co.uk", "example.co.uk"),
            ("www.example.com.cn", "example.com.cn"),
            // 表外的普通域名仍折叠到末两段(近似行为, 已知不完备)
            ("a.b.example.com", "example.com"),
        ] {
            assert_eq!(registrable_domain_from_host(host), expected, "host={host}");
        }
        // co.nz 已在表内: 两个不同的注册域必须被区分开, 否则跨域 cookie 写入会被放行。
        assert_ne!(
            registrable_domain_from_host("www.abc.co.nz"),
            registrable_domain_from_host("evil.co.nz"),
            "不同注册域不得折叠成同一个值(否则跨域写入会被放行)"
        );
    }

    #[test]
    fn login_target_preserves_nested_jump_url_query() {
        let target = resolve_proxy_target_url(
            "https://alpha.example.com/login.php?jumpurl=https://alpha.example.com/",
            "https://alpha.example.com/",
        )
        .unwrap();
        assert_eq!(target, "https://alpha.example.com/login.php?jumpurl=https://alpha.example.com/");
        assert!(proxy_target_matches_source(&target, "https://alpha.example.com/"));
    }

    #[test]
    fn panel_outer_query_overrides_upstream_query() {
        // 某聚合源的登录页 `sw('register')` 把 mode 写进代理 URL 外层 query,
        // 初始加载已删掉 __readerPanelUrl(桥的 proxyUrl(url, storedHtml=true)),
        // 合并必须把 mode=register 送到上游, 否则页面永远是登录态。
        let params = parse_query("__readerPanelId=panel-1&mode=register");
        let merged = merge_panel_outer_query("https://v1.vossc.com/user_login", &params, true).unwrap();
        assert_eq!(merged, "https://v1.vossc.com/user_login?mode=register");
        // 与真实域名校验配合, 合并结果仍在书源域名内
        assert!(proxy_target_matches_source(&merged, "https://v1.vossc.com"));
    }

    #[test]
    fn panel_outer_query_preserves_other_upstream_params() {
        // 外层与内层同名时外层优先; 内层独有的参数保留。
        let params = parse_query("__readerPanelId=panel-1&mode=register&page=3");
        let merged = merge_panel_outer_query("https://example.com/list?mode=login&keep=1", &params, true).unwrap();
        // 用有序序列断言, 不用 HashMap 收集(query_pairs 收集到 HashMap 会天然去重, 掩盖重复项问题)。
        let pairs: Vec<(String, String)> = Url::parse(&merged).unwrap()
            .query_pairs().map(|(k, v)| (k.into_owned(), v.into_owned())).collect();
        assert!(pairs.contains(&("mode".into(), "register".into())));
        assert!(pairs.contains(&("keep".into(), "1".into())));
        assert!(pairs.contains(&("page".into(), "3".into())));
        // 被覆盖的旧值不得残留
        assert!(!pairs.contains(&("mode".into(), "login".into())));
    }

    #[test]
    fn panel_outer_query_replaces_every_duplicate_key() {
        // review P1: 外层 `a=3` 必须整体替换上游 `?a=1&a=2`, 得 `?a=3` 而不是
        // `?a=3&a=2` —— 后者会静默改变上游请求语义, 且与 JS 的 searchParams.set
        // (删全部同名再设一个)不一致。
        let params = parse_query("__readerPanelId=panel-1&a=3");
        let merged = merge_panel_outer_query("https://example.com/x?a=1&a=2", &params, true).unwrap();
        let pairs: Vec<(String, String)> = Url::parse(&merged).unwrap()
            .query_pairs().map(|(k, v)| (k.into_owned(), v.into_owned())).collect();
        assert_eq!(pairs, vec![("a".to_string(), "3".to_string())]);
    }

    #[test]
    fn panel_outer_query_leaves_identical_upstream_query_untouched() {
        // review P5/P6: 面板内导航的外层 query 常态是 proxyUrl 复制自上游。
        // 参数原样存在时必须返回 None, 让调用方沿用原 target, 不做
        // query_pairs 解码/重编码往返(否则 %20 变 +、%zz 变 %25zz, 上游按
        // query 原文签名时会失配), 也不因 HashMap 迭代序造成结果不稳定。
        let base = "https://example.com/x?q=a%20b%2Fc&sig=%zz&keep=1";
        let params = parse_query("__readerPanelId=panel-1&q=a%20b%2Fc&sig=%zz&keep=1");
        assert!(merge_panel_outer_query(base, &params, true).is_none());
    }

    #[test]
    fn panel_outer_query_skips_transport_and_untouched_cases() {
        // 只有传输字段 → 无需合并, 调用方沿用原 target(不改变既有行为)。
        let params = parse_query("__readerPanelId=panel-1&__readerPanelUrl=https%3A%2F%2Fexample.com%2Fx");
        assert!(merge_panel_outer_query("https://example.com/x", &params, true).is_none());
        // 非 namespaced 形态排除 url/panelId, 但上游自己的业务参数不可丢。
        let params = parse_query("panelId=panel-1&url=https%3A%2F%2Fexample.com%2Fx&mode=register");
        let merged = merge_panel_outer_query("https://example.com/x", &params, false).unwrap();
        assert_eq!(merged, "https://example.com/x?mode=register");
        // Non-http(s) 与 data: 面板不参与合并。
        let params = parse_query("__readerPanelId=panel-1&mode=register");
        assert!(merge_panel_outer_query("data:text/html,<p>x</p>", &params, true).is_none());
        // base 不是绝对 URL(相对路径且无 scheme)时返回 None。
        assert!(merge_panel_outer_query("/relative/path", &params, true).is_none());
    }

    #[test]
    fn panel_outer_query_appends_without_rewriting_existing_query() {
        // review P5: 纯追加场景(该页点注册)不得重建整串 query, 否则
        // `%20` 会被重编码成 `+`, 上游按 query 原文签名时会失配。
        let params = parse_query("__readerPanelId=panel-1&mode=register");
        let merged = merge_panel_outer_query("https://example.com/x?q=a%20b&sig=%zz", &params, true).unwrap();
        assert!(merged.contains("q=a%20b"), "既有片段的原文必须保留: {merged}");
        assert!(merged.contains("sig=%zz"), "非法百分号序列不得被改写: {merged}");
        assert!(merged.ends_with("&mode=register"), "新参数追加在末尾: {merged}");
    }

    #[test]
    fn panel_outer_query_has_stable_order_for_multiple_new_params() {
        // review P6: params 是 HashMap, 迭代序随机; 排序后追加顺序可复现。
        let params = parse_query("__readerPanelId=panel-1&zz=1&aa=2&mm=3");
        let first = merge_panel_outer_query("https://example.com/x", &params, true).unwrap();
        let second = merge_panel_outer_query("https://example.com/x", &params, true).unwrap();
        assert_eq!(first, second);
        assert_eq!(first, "https://example.com/x?aa=2&mm=3&zz=1");
    }

    /// 某聚合源的登录页的端到端链路: 初始加载(删掉 __readerPanelUrl)后页面 JS
    /// 改写外层 query 触发注册导航, 上游必须收到 mode=register 才能渲染注册态。
    #[test]
    fn panel_register_navigation_reaches_upstream_with_mode() {
        // 初始加载形态: proxyUrl(url, storedHtml=true) 会删掉 __readerPanelUrl
        let initial = "http://reader.localhost/sourcePanel?__readerPanelId=panel-1";
        let params = parse_query("__readerPanelId=panel-1");
        assert!(merge_panel_outer_query("https://v1.vossc.com/user_login", &params, true).is_none());
        // 页面 sw('register') 等价: 在 location.href 上加 mode 后整页导航
        let navigated = format!("{initial}&mode=register");
        let params = parse_query("__readerPanelId=panel-1&mode=register");
        let merged = merge_panel_outer_query("https://v1.vossc.com/user_login", &params, true).unwrap();
        assert_eq!(merged, "https://v1.vossc.com/user_login?mode=register");
        assert!(proxy_target_matches_source(&merged, "https://v1.vossc.com"));
        // 面板 URL 自身不得被当成上游地址
        assert!(!proxy_target_matches_source(&navigated, "https://v1.vossc.com"));
    }

    /// HTML 改写只代理同域资源。
    ///
    /// 跨域资源一律保持原样, 由 WebView 自己去取 —— 见 `rewrite_source_panel_html`
    /// 的说明: 把它们一起收进代理会让每个子资源都变成一次宿主请求, 直连不通时
    /// 全部挂到 15s 超时, 足以把整个应用(乃至桌面)拖垮。这条用例固定住该边界。
    #[test]
    fn rewrite_source_panel_html_only_proxies_same_domain() {
        let html = r#"<html><head>
          <link rel="stylesheet" href="https://cdn.other.example/a.css">
          <script src="/static/app.js"></script>
          <a href="https://repo.other.example/detail/1">跨域详情</a>
          <a href="javascript:void(0)">脚本</a>
        </head></html>"#;
        let page_url = "https://panel.example.vip/";
        let allowed = "https://panel.example.vip/";

        let output = rewrite_source_panel_html(html, page_url, allowed, "panel-1");

        // 同域资源被代回代理。
        assert!(
            output.contains("__readerPanelUrl=https%3A%2F%2Fpanel.example.vip%2Fstatic%2Fapp.js"),
            "同域资源应代回代理: {output}"
        );
        // 跨域资源保持原样, 不经过本进程。
        assert!(
            output.contains("https://cdn.other.example/a.css"),
            "跨域资源必须保持原样: {output}"
        );
        assert!(
            !output.contains("__readerPanelUrl=https%3A%2F%2Fcdn.other.example"),
            "跨域资源不能被收进代理"
        );
        assert!(output.contains("https://repo.other.example/detail/1"));
        // 非 http(s) 不改写。
        assert!(output.contains(r#"<a href="javascript:void(0)">"#));
    }

    /// 跨域二级页自己的**同域**资源仍要代理: 判定基准必须包含当前页面的域,
    /// 而不是只看订阅源最初的那个地址。
    #[test]
    fn rewrite_source_panel_uses_current_page_domain() {
        let html = r#"<a href="/detail/2">同域</a>"#;
        // 页面已经跳到新域名, 订阅源原始地址仍是旧域名。
        let output = rewrite_source_panel_html(
            html,
            "https://repo.example.com/",
            "https://repo.example.vip/",
            "panel-1",
        );

        assert!(
            output.contains("__readerPanelUrl=https%3A%2F%2Frepo.example.com%2Fdetail%2F2"),
            "应基于当前页面域改写: {output}"
        );
    }

    #[test]
    fn proxy_target_is_limited_to_source_domain_family() {
        assert!(proxy_target_matches_source(
            "https://passport.example.com/login",
            "https://www.example.com"
        ));
        assert!(proxy_target_matches_source(
            "https://www.example.co.uk/api",
            "https://m.example.co.uk"
        ));
        assert!(!proxy_target_matches_source(
            "https://evil.example.net/steal",
            "https://www.example.com"
        ));
        assert!(!proxy_target_matches_source(
            "https://127.0.0.1:8080/",
            "https://localhost:3000"
        ));
        assert!(!proxy_target_matches_source(
            "https://source.example:9000/",
            "https://source.example"
        ));
        assert!(!proxy_target_matches_source(
            "https://other.localhost/",
            "https://app.localhost"
        ));
    }

    // ─────────────────── 书源面板 (showBrowser) ───────────────────

    /// 二级链接统一留在面板内经代理导航, 不再交给系统浏览器。
    ///
    /// 背景: RSS 面板只托管**首屏** HTML; 页面里的链接指向上游站点。早先为
    /// 规避上游 `X-Frame-Options` 造成的白屏, 这些链接被交给系统浏览器。
    /// 但跳转实际走 `proxyUrl` —— 页面由后端抓取、重写后托管在 reader origin,
    /// 代理不透传那些限制头, 所以经代理渲染不会白屏。现在两种面板共用同一段
    /// click 兜底, 行为一致。
    #[test]
    fn panel_bridge_keeps_secondary_links_inside_panel() {
        let rss = build_source_panel_bridge_script("panel-rss", true, &[]);
        let book = build_source_panel_bridge_script("panel-book", false, &[]);

        for (label, script) in [("RSS", &rss), ("书源", &book)] {
            // 两种面板都不应再调用 window.open 把链接甩给系统浏览器。
            assert!(
                !script.contains(r#"window.open(url.href, "_blank", "noopener")"#),
                "{label} 面板不应再把二级链接交给系统浏览器"
            );
            assert!(
                !script.contains(r#"window.open(target.href, "_blank", "noopener")"#),
                "{label} 面板不应再用 window.open 外跳"
            );
            // 二级链接改走代理导航, 留在面板内。
            assert!(
                script.contains("window.location.assign(proxyUrl(url))"),
                "{label} 面板应把二级链接代回代理 URL"
            );
            // click 兜底挂在捕获阶段, 否则页面自己的 handler 可能先执行。
            assert!(script.contains("}, true);"));
            assert!(script.contains("event.button !== 0"));
        }

        // 标志仍按面板类型注入(保留兼容), 但不再影响外链处理。
        assert!(rss.contains("var externalLinks = true"));
        assert!(book.contains("var externalLinks = false"));

        // 书源面板的表单提交仍走代理(登录/注册依赖它)。
        assert!(book.contains("submitToProxy(event, target)"));
    }

    /// 源仓库的「一键导入」是 `legado://` / `yuedu://` 自定义协议, 系统浏览器
    /// 不认识, 交出去就是静默失败; `target="_blank"` 的二级链接若不拦, 会被
    /// 系统浏览器接管(表现为"点一下就跳出应用")。
    #[test]
    fn panel_bridge_handles_import_protocol_and_blank_links() {
        let book = build_source_panel_bridge_script("panel-book", false, &[]);

        // 两种面板都要识别导入协议并转给宿主, 由前端拉取并入库。
        assert!(
            book.contains("function importProtocolUrl(raw)"),
            "书源面板应能识别 legado:// / yuedu:// 导入链接"
        );
        assert!(
            book.contains("postToHost({ type: \"importProtocol\", url: href })"),
            "导入链接需要转给宿主处理"
        );
        assert!(
            book.contains(r#"/^(legado|yuedu):$/i"#),
            "只接管这两种导入协议"
        );

        // 两种面板都要处理导入协议(源仓库既可能从书源面板打开, 也可能从
        // RSS 面板打开), 且都要把 _blank 链接留在面板内经代理导航。
        let rss = build_source_panel_bridge_script("panel-rss", true, &[]);
        for (label, script) in [("RSS", &rss), ("书源", &book)] {
            assert!(
                script.contains("function importProtocolUrl(raw)"),
                "{label} 面板应能识别导入协议"
            );
            assert!(
                script.contains("window.location.assign(proxyUrl(url))"),
                "{label} 面板的 _blank 链接应留在面板内"
            );
        }
    }

    #[test]
    fn panel_bridge_script_exposes_legado_api_surface() {
        let script = build_source_panel_bridge_script("panel-1", false, &[]);

        // 插件面板依赖的宿主接口 (legado WebJsExtensions)
        for expected in [
            "window.java",
            "window.cache",
            "window.run",
            "ajax:",
            "showBrowser:",
            "getFromMemory:",
            "getFile:",
            "putFile:",
        ] {
            assert!(script.contains(expected), "桥缺少 {expected}");
        }
        // 同步 XHR(插件大量使用同步 ajax)与免预检 Content-Type
        assert!(script.contains("false); // false = 同步"));
        assert!(script.contains("text/plain"));
        // panelId 需转义进脚本
        assert!(script.contains("\"panel-1\""));
    }

    /// 插件用 `typeof java.createSymmetricCrypto !== 'undefined'` 探测原生加密能力;
    /// 探测失败会降级到「注入 CryptoJS」的回退路径(那条路径在桌面端会直接失败,
    /// 导致面板所有 API 调用无签名 → 书籍信息为空、段评加载不出来)。
    #[test]
    fn panel_bridge_script_declares_crypto_methods() {
        let script = build_source_panel_bridge_script("panel-1", false, &[]);
        for method in [
            "createSymmetricCrypto:",
            "aesEncodeToBase64String:",
            "aesEncodeToString:",
            "aesDecodeToString:",
            "desEncodeToBase64String:",
            "desEncodeToString:",
            "desDecodeToString:",
            "md5Encode:",
            "base64Encode:",
            "base64Decode:",
        ] {
            assert!(script.contains(method), "桥缺少加密方法 {method}");
        }
        // 需要能完成对外的 crypto 桥调用
        assert!(script.contains("bridgeSync(\"crypto\""));
    }

    #[test]
    fn panel_bridge_script_does_not_patch_xhr() {
        // 曾经 patch XMLHttpRequest.prototype 导致桥自身的 XHR 也被拦截(自递归)。
        let script = build_source_panel_bridge_script("panel-1", false, &[]);
        assert!(!script.contains("XMLHttpRequest.prototype"));
    }

    #[test]
    fn source_panel_page_injects_bridge_into_head() {
        let id = crate::service::book_service::register_source_panel(
            crate::service::book_service::SourcePanelEntry {
                url: "https://panel.example/panel".to_string(),
                html: "<html><head><title>面板</title></head><body>内容</body></html>".to_string(),
                book_source_url: "https://source.example".to_string(),
                book_url: "https://source.example/book/1".to_string(),
                chapter_url: "https://source.example/book/1/1".to_string(),
                book_json: "{}".to_string(),
                chapter_json: "{}".to_string(),
                ..Default::default()
            },
        );

        let page = build_source_panel_page(&id).expect("面板页应可构建");
        // 原内容保留
        assert!(page.contains("<title>面板</title>"));
        assert!(page.contains("内容"));
        // 桥注入到 </head> 之前
        let bridge_at = page.find("window.java").expect("桥应被注入");
        let head_end = page.find("</head>").expect("应有 </head>");
        assert!(bridge_at < head_end, "桥应注入在 </head> 之前");
        // 页面里带上自己的 panelId
        assert!(page.contains(&id));

        // 未登记的面板返回 None
        assert!(build_source_panel_page("no-such-panel").is_none());
    }

    #[test]
    fn panel_registry_keeps_newest_and_evicts_oldest() {
        let make = |url: &str| crate::service::book_service::SourcePanelEntry {
            url: url.to_string(),
            html: "<html></html>".to_string(),
            book_source_url: "https://evict.example".to_string(),
            book_url: String::new(),
            chapter_url: String::new(),
            book_json: "{}".to_string(),
            chapter_json: "{}".to_string(),
            ..Default::default()
        };

        // 登记超过上限, 最旧的应被淘汰, 最新的应仍在
        let mut ids = Vec::new();
        for index in 0..12 {
            ids.push(crate::service::book_service::register_source_panel(make(&format!(
                "https://evict.example/{index}"
            ))));
        }
        let newest = ids.last().unwrap();
        let oldest = ids.first().unwrap();
        assert!(
            crate::service::book_service::get_source_panel(newest).is_some(),
            "最新面板不应被淘汰"
        );
        assert!(
            crate::service::book_service::get_source_panel(oldest).is_none(),
            "最旧面板应被淘汰(按插入顺序, 而非 HashMap 随机序)"
        );
    }

    /// RSS 的 `singleUrl` 源(源地址本身就是"一篇文章")复用书源面板的**静态渲染**
    /// 路径: 后端先抓好 HTML 再登记面板, 因此不查书源表。
    ///
    /// 这条用例守住该复用点 —— 面板 HTML 必须带上桥脚本与 `<base>`, 否则面板里的
    /// 相对资源会以 `reader.localhost` 为基准解析而全部 404。
    ///
    /// 另外验证 `external_links` 从面板条目一路传到渲染出的桥脚本: 这正是
    /// "点二级链接白屏" 的修复开关(上游页面的 X-Frame-Options 会让 iframe 拒绝渲染)。
    #[test]
    fn rss_static_panel_renders_bridge_and_base_for_non_book_source() {
        let page_url = "https://panel.example.vip/";
        let html = "<html><head><title>面板页</title></head><body><a href=\"/a\">x</a></body></html>";
        // 关键: 面板归属留空 —— RSS 源没有书源记录, 走的正是静态渲染分支。
        // `external_links: true` 与 `open_rss_source_panel` 里的设置一致。
        let entry = crate::service::book_service::SourcePanelEntry {
            url: page_url.to_string(),
            html: html.to_string(),
            external_links: true,
            ..Default::default()
        };
        let panel_id = crate::service::book_service::register_source_panel(entry.clone());

        let rendered = render_source_panel_page(&panel_id, &entry, page_url, html);

        assert!(
            rendered.contains("__readerPanelBridgeReady") || rendered.contains("readerPanelBridge"),
            "面板必须注入桥脚本: {}",
            rendered.chars().take(200).collect::<String>()
        );
        assert!(
            rendered.contains("<base href"),
            "必须注入 <base> 让相对资源指回上游站点"
        );
        assert!(
            rendered.contains("panel.example.vip"),
            "面板 URL 应出现在页面里(桥用它做同源校验与代理拼接)"
        );
        // 原页面内容保留(静态渲染不是重写页面, 只是插入桥)。
        assert!(rendered.contains("面板页"), "原页面内容应保留");
        // 外链开关必须随条目进入渲染结果, 否则二级链接仍会在 iframe 内白屏。
        assert!(
            rendered.contains("var externalLinks = true"),
            "RSS 面板渲染结果必须开启外链跳转"
        );
        assert!(
            rendered.contains(r#"addEventListener("click""#),
            "必须带 click 兜底(老 WebView 无 navigation API)"
        );
    }

    /// 封面路由必须把 `source` 参数解析出来（加密封面的 `coverDecodeJs` 靠它
    /// 反查书源，issue #23）。这里只测查询串解析这一段：`cover_route` 本身
    /// 需要完整 `AppState`，不适合在单测里构造。
    #[test]
    fn cover_query_carries_source_and_path() {
        let query = format!(
            "path={}&source={}",
            urlencoding::encode("https://xi.dzuxta.cn/a/b.jpeg?auth_key=1-2-3"),
            urlencoding::encode("https://18mh.net/novel"),
        );
        let params: HashMap<String, String> =
            serde_urlencoded::from_str(&query).unwrap_or_default();
        assert_eq!(
            params.get("path").map(String::as_str),
            Some("https://xi.dzuxta.cn/a/b.jpeg?auth_key=1-2-3"),
            "封面地址里的 & ? 不能被查询串解析吃掉"
        );
        assert_eq!(
            params.get("source").map(String::as_str),
            Some("https://18mh.net/novel")
        );
    }

    /// 只有 `x-expires` 真的过期才触发换地址。
    ///
    /// 这条判定必须严格：它决定是否为一张封面额外拉一次详情页，误判会把
    /// 书架首屏每张图都拖成一次网络往返。
    #[test]
    fn detects_only_expired_cover_urls() {
        assert!(cover_url_expired(
            "https://img.example/c.webp?lk3s=8d&x-expires=1&x-signature=abc%3D"
        ));
        assert!(cover_url_expired("https://img.example/c.jpg?X-Expires=1"));
        // 还没到期: 缓存未命中时正常走回源，不要走换地址。
        assert!(!cover_url_expired(
            "https://img.example/c.jpg?x-expires=99999999999&x-signature=abc"
        ));
        // 没有 x-expires: 仅 x-signature 不代表会过期。
        assert!(!cover_url_expired(
            "https://img.example/c.jpg?x-signature=abc"
        ));
        assert!(!cover_url_expired("https://img.example/c.jpg"));
        assert!(!cover_url_expired(
            "https://img.example/c.jpg?auth_key=1-2-3"
        ));
        // 值不是时间戳: 不做猜测性触发。
        assert!(!cover_url_expired(
            "https://img.example/c.jpg?x-expires=abc"
        ));
        assert!(!cover_url_expired(
            "https://img.example/c.jpg?redirect=https://a/b?x-expires=1"
        ));
    }
}
