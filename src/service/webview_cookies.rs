//! 从独立登录窗口读回 Cookie 与 localStorage，写进书源的会话。
//!
//! 独立窗口打开的是**上游真实站点**，页面在那个域下写的 Cookie 只存在于
//! WebView 自己的存储里；书源后续请求走的是我们自己维护的 cookie jar，两边
//! 不通 —— 表现就是「窗口里登录成功了，回到面板还是显示未登录」。
//!
//! 手机端 legado 也是同一原理，只是 Android 提供同步的
//! `CookieManager.getCookie(url)`。
//!
//! ## Cookie：用 Tauri 的跨平台接口，不再手写 WebView2 COM
//!
//! 早先这里用 `ICoreWebView2CookieManager` + `GetCookiesCompletedHandler` 手动读
//! Cookie，并把整块关在 `#[cfg(target_os = "windows")]` 里。那套写法的代价是
//! macOS / Linux 没有实现（登录态不会回传），而调用方又无条件引用这些符号，
//! 非 Windows 直接编译失败。
//!
//! Tauri 2 的 `WebviewWindow::cookies()` 已经跨平台，底层由 wry 分平台实现：
//!   - Windows: `ICoreWebView2_2::CookieManager().GetCookies`
//!   - macOS:   `WKHTTPCookieStore.getAllCookies`
//!   - Linux:   WebKitGTK `CookieManager::all_cookies`
//!
//! 三者在等待异步回调时都会自己泵消息循环（`with_with_bump` /
//! `wait_for_blocking_operation` / `gtk::main_iteration`），因此可以从
//! `on_window_event` 这种同步回调里直接调，不会死锁 —— 和原先手写 COM 的
//! 做法本质相同，但一份代码覆盖三平台。
//!
//! `document.cookie` 那条路不可行：登录态最常见的是 `HttpOnly` Cookie。

/// 一条 Cookie 的必需字段。
#[derive(Debug, Clone)]
pub struct CookieRecord {
    pub name: String,
    pub value: String,
    pub domain: String,
    pub path: String,
    pub secure: bool,
    pub http_only: bool,
}

impl CookieRecord {
    /// 拼成 `name=value`，供 `Cookie` 请求头使用。
    pub fn pair(&self) -> String {
        format!("{}={}", self.name, self.value)
    }
}

/// 读出窗口**当前可见的全部** Cookie（跨所有域）。
///
/// 取全部而不是按单个 URL 取：登录流程会跨多个子域写 Cookie。
pub fn read_all(window: &tauri::WebviewWindow) -> anyhow::Result<Vec<CookieRecord>> {
    let cookies = window
        .cookies()
        .map_err(|err| anyhow::anyhow!("读取 Cookie 失败: {err}"))?;
    Ok(cookies
        .into_iter()
        .filter(|cookie| !cookie.name().is_empty())
        .map(|cookie| CookieRecord {
            name: cookie.name().to_string(),
            value: cookie.value().to_string(),
            // `domain` / `path` 在个别平台可能为空串；调用方会退回按 URL 推导。
            domain: cookie.domain().unwrap_or_default().to_string(),
            path: cookie.path().unwrap_or("/").to_string(),
            secure: cookie.secure().unwrap_or(false),
            http_only: cookie.http_only().unwrap_or(false),
        })
        .collect())
}

/// 把书源 jar 里现有的 Cookie **注入窗口**（反向同步）。
///
/// 对应 legado 的 `AppCookieManager.applyToWebView(url)`（`WebViewActivity`
/// 建窗口时调用）。作用有两个：
///
/// 1. 用户第二次点「打开浏览器完成验证」时，窗口直接带上上次的 `cf_clearance`，
///    不用再过一遍人机验证；
/// 2. 站点按 Cookie 决定「已登录」的界面（会员中心、书架）能正确显示。
///
/// 注入前必须先设好窗口 UA —— Cloudflare 绑定 UA，用错 UA 的 token 带过去
/// 只会让站点认定会话异常。
pub fn write_into_window(
    window: &tauri::WebviewWindow,
    cookies: &[CookieRecord],
) -> anyhow::Result<usize> {
    let mut written = 0;
    for record in cookies {
        if record.name.is_empty() || record.name.chars().any(char::is_control) {
            continue;
        }
        if record.value.chars().any(char::is_control) {
            continue;
        }
        let mut builder = tauri::webview::Cookie::build((record.name.clone(), record.value.clone()))
            .path(if record.path.starts_with('/') { record.path.clone() } else { "/".to_string() });
        // 与 `SourceCookieJar::snapshot` 的约定一致：**带前导点才是域级**。
        // 不带点的（`www.example.com`）是 host-only，不能设 `domain`，
        // 否则会升级成整域、把会话泄漏给其它子域。
        if let Some(suffix) = record.domain.trim().strip_prefix('.') {
            if !suffix.is_empty() {
                builder = builder.domain(suffix.to_string());
            }
        }
        if record.secure {
            builder = builder.secure(true);
        }
        if record.http_only {
            builder = builder.http_only(true);
        }
        if window.set_cookie(builder.build()).is_ok() {
            written += 1;
        }
    }
    Ok(written)
}

/// 读出窗口**当前页面 origin 下**的 localStorage（`key=value` 对）。
///
/// 为什么需要它：一类书源的「段评 / 本章说」页面是 SPA，鉴权要两样东西 ——
/// `Authorization`（在 Cookie 里）和 `X-Device-ID`（在 `localStorage` 里）。
/// 独立窗口跑在**上游真实域名**下，localStorage 天然是对的；但段评面板挂在
/// `reader.localhost` origin 下，localStorage 与上游完全隔离，SPA 取不到
/// device id 就现生成一个假的，服务端判定异常返回空。
///
/// 因此窗口关闭时把 localStorage 一起收走，之后渲染面板时再注入回去。
///
/// ## 为什么是异步的
///
/// 读 localStorage 只能执行脚本，而脚本求值在 WebView 里天然异步。Tauri 的
/// `eval_with_callback` 只是把任务投递到事件循环、立刻返回；**回调在事件循环
/// 线程上执行**。若在 `on_window_event` 里同步 `recv()` 等它，事件循环被自己
/// 堵住，回调永远派发不出来 —— 死锁。
///
/// （`read_all` 能同步是因为 wry 的 `cookies()` 内部用了 `with_with_bump` /
/// `gtk::main_iteration` 自己泵消息；`eval` 这条路径没有等价物。）
///
/// 所以调用方要先把关闭拦下来（`api.prevent_close()`），拿到结果后再关窗。
pub fn read_local_storage_async(
    window: &tauri::WebviewWindow,
    done: impl FnOnce(Vec<(String, String)>) + Send + 'static,
) -> anyhow::Result<()> {
    // `{k:v,...}` 形式回传，宿主侧再解析。
    //
    // 外层 try/catch 是必要的：脚本抛错时 wry 在部分平台只会调用回调传空串，
    // 在另一些平台则静默丢弃，返回 `'{}'` 能让两条路径都收敛到「没有数据」。
    const SCRIPT: &str = r#"(function () {
  try {
    var out = {};
    for (var i = 0; i < localStorage.length; i++) {
      var key = localStorage.key(i);
      if (key === null) continue;
      out[key] = localStorage.getItem(key);
    }
    return JSON.stringify(out);
  } catch (e) { return '{}'; }
})()"#;

    // Tauri 的 `eval_with_callback` 收的是 `Fn`（可能被调多次），而我们要的
    // 是「只处理一次」的语义：用 Option 包住，取走后就置空，重复回调直接忽略。
    let done = std::sync::Mutex::new(Some(done));
    window
        .eval_with_callback(SCRIPT, move |raw| {
            let Some(done) = done.lock().unwrap_or_else(|e| e.into_inner()).take() else {
                return;
            };
            // wry 把结果序列化成 JSON 字符串再交回来：脚本返回的是
            // JSON.stringify 的字符串，所以这里先解一层字符串、再解一层对象。
            let decoded: String = serde_json::from_str(&raw).unwrap_or_default();
            let parsed: std::collections::BTreeMap<String, serde_json::Value> =
                serde_json::from_str(&decoded).unwrap_or_default();
            done(parsed
                .into_iter()
                .filter_map(|(key, value)| match value {
                    serde_json::Value::String(text) => Some((key, text)),
                    serde_json::Value::Null => None,
                    other => Some((key, other.to_string())),
                })
                .collect());
        })
        .map_err(|err| anyhow::anyhow!("读取 localStorage 失败: {err}"))
}
