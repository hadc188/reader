use crate::{
    crawler::{fetcher::{data_uri_response, RequestSpec}, source_session::SourceHttpContext},
    model::book_source::BookSource,
    parser::js::source_http_context,
    service::book_service::{register_source_panel, ShowBrowserRequest, SourcePanelEntry},
};
use serde::{Deserialize, Serialize};
use std::{cell::RefCell, collections::HashMap, sync::{mpsc, Arc, LazyLock, Mutex}, time::Duration};

pub type BrowserHandler = Arc<dyn Fn(ShowBrowserRequest) -> Result<(), String> + Send + Sync>;

#[derive(Clone)]
struct BrowserContext {
    source_url: String,
    http: SourceHttpContext,
    handler: BrowserHandler,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct BrowserPage {
    pub url: String,
    pub body: String,
}

thread_local! {
    static BROWSER_CONTEXT: RefCell<Option<BrowserContext>> = const { RefCell::new(None) };
    static BROWSER_CANCELLED: RefCell<bool> = const { RefCell::new(false) };
}

type BrowserReply = Result<BrowserPage, String>;
static PENDING: LazyLock<Mutex<HashMap<String, mpsc::SyncSender<BrowserReply>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

pub fn with_browser_handler<T>(
    source: &BookSource,
    handler: Option<BrowserHandler>,
    run: impl FnOnce() -> T,
) -> anyhow::Result<T> {
    let context = handler.map(|handler| {
        Ok::<_, anyhow::Error>(BrowserContext {
            source_url: source.book_source_url.clone(),
            http: source_http_context(&source.book_source_url)?,
            handler,
        })
    }).transpose()?;
    struct Restore(Option<BrowserContext>);
    impl Drop for Restore {
        fn drop(&mut self) {
            BROWSER_CONTEXT.with(|cell| cell.replace(self.0.take()));
        }
    }
    let _restore = Restore(BROWSER_CONTEXT.with(|cell| cell.replace(context)));
    Ok(run())
}

pub fn browser_was_cancelled() -> bool {
    BROWSER_CANCELLED.with(|cell| *cell.borrow())
}

pub fn take_browser_cancelled() -> bool {
    BROWSER_CANCELLED.with(|cell| cell.replace(false))
}

/// Host data: pages on the reader origin so they can use the source bridge.
/// Keep the source's HTTP context even when its identifier is not a URL.
pub fn register_browser_request(
    mut request: ShowBrowserRequest,
    mut entry: SourcePanelEntry,
) -> anyhow::Result<ShowBrowserRequest> {
    entry.url = request.url.clone();
    entry.html = match request.html.as_ref() {
        Some(html) => html.clone(),
        None => data_uri_response(&RequestSpec { url: request.url.clone(), ..Default::default() })?
            .map(|response| response.body).unwrap_or_default(),
    };
    entry.preload_js = request.preload_js.clone();
    request.panel_id = Some(register_source_panel(entry));
    // HTML and preload code are served through /sourcePanel, not duplicated in IPC.
    request.html = None;
    request.preload_js = None;
    if request.url.starts_with("data:") { request.url.clear(); }
    Ok(request)
}

pub fn source_panel_entry(source_url: &str) -> anyhow::Result<SourcePanelEntry> {
    Ok(SourcePanelEntry {
        book_source_url: source_url.to_string(),
        http: Some(source_http_context(source_url)?),
        ..Default::default()
    })
}

/// Suspend only the blocking JS worker. The desktop receives the panel over an
/// IPC channel and confirms/cancels it through a separate request; the original
/// QuickJS stack then continues once, with the user's actual document.
pub fn await_browser(mut request: ShowBrowserRequest) -> BrowserReply {
    let context = BROWSER_CONTEXT.with(|cell| cell.borrow().clone())
        .ok_or_else(|| "此操作需要打开书源登录窗口后确认网页结果".to_string())?;
    request.await_result = true;
    let request = register_browser_request(request, SourcePanelEntry {
        book_source_url: context.source_url,
        http: Some(context.http),
        ..Default::default()
    }).map_err(|err| err.to_string())?;
    let panel_id = request.panel_id.clone().expect("registered panel");
    let (sender, receiver) = mpsc::sync_channel(1);
    PENDING.lock().unwrap_or_else(|e| e.into_inner()).insert(panel_id.clone(), sender);
    struct PendingPanel(String);
    impl Drop for PendingPanel {
        fn drop(&mut self) {
            PENDING.lock().unwrap_or_else(|e| e.into_inner()).remove(&self.0);
            crate::service::book_service::remove_source_panel(&self.0);
        }
    }
    let _pending = PendingPanel(panel_id);
    let outcome = (context.handler)(request).and_then(|_| {
        receiver.recv_timeout(Duration::from_secs(30 * 60))
            .map_err(|_| "网页操作已结束或等待超时".to_string())?
    });
    if outcome.is_err() {
        BROWSER_CANCELLED.with(|cell| cell.replace(true));
    }
    outcome
}

/// 把「看一眼就走」的链接交给独立窗口：**只投递请求，不等待**。
///
/// 与 `await_browser` 的区别是不同步等用户操作 —— 注册页/用户后台这类页面
/// 打开即可，脚本不需要从里面回读数据，所以直接返回让 JS 继续跑，避免把
/// 整个解析线程挂在一个可能永远关不掉的窗口上。
pub fn request_external_window(request: ShowBrowserRequest) -> Result<(), String> {
    let context = BROWSER_CONTEXT
        .with(|cell| cell.borrow().clone())
        .ok_or_else(|| "此操作需要先打开书源登录窗口".to_string())?;
    (context.handler)(request)
}

/// 打开独立窗口并**等它关闭**，再让 JS 继续执行。
///
/// 这是 `java.startBrowserAwait(url, title)` 的语义（legado 的
/// `SourceVerificationHelp.getVerificationResult`）：脚本会一直阻塞，直到用户
/// 把窗口关掉，然后才继续往下跑。
///
/// ## 为什么必须真的等
///
/// 书源 8054 的番茄登录就是典型用法：
///
/// ```js
/// jv.startBrowserAwait('https://fanqienovel.com/', '番茄登录（登录完成后返回）');
/// return _dpFqGrabSave(jv, ckObj);   // ← 抓 cookie 并上传
/// ```
///
/// 如果不等（之前是 fire-and-forget），用户在窗口里还没登录，下一行就已经去
/// 抓 cookie 了 —— 抓到的是空会话，上传失败，那个「番茄凭证抓取诊断」页面
/// 自然也不会出现。手机端能正常工作，正是因为 legado 在那里阻塞等待。
///
/// 与 [`await_browser`] 的区别：那个等的是**iframe 面板**回传网页正文；
/// 这个等的是**独立窗口关闭**，不读回数据（脚本要的是窗口里产生的 Cookie，
/// 不是页面文本）。
pub fn await_external_window(request: ShowBrowserRequest) -> BrowserReply {
    let context = BROWSER_CONTEXT.with(|cell| cell.borrow().clone())
        .ok_or_else(|| "此操作需要打开书源登录窗口后确认网页结果".to_string())?;
    let await_panel_id = format!("external-{}", uuid::Uuid::new_v4());
    let mut request = request;
    request.external_window = true;
    request.await_result = true;
    // 前端读这个字段：非空表示脚本正等这个窗口关闭，关窗要唤醒。
    request.await_panel_id = Some(await_panel_id.clone());
    let (sender, receiver) = mpsc::sync_channel(1);
    PENDING
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .insert(await_panel_id.clone(), sender);
    struct PendingPanel(String);
    impl Drop for PendingPanel {
        fn drop(&mut self) {
            PENDING
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .remove(&self.0);
        }
    }
    let _pending = PendingPanel(await_panel_id.clone());
    // 投递给前端（前端收到后开窗口）。失败说明没有前端在听 —— 直接返回，
    // 别让脚本空等 30 分钟。
    (context.handler)(request)?;
    // 与 `await_browser` 同一量级的超时：用户可能把窗口晾很久。
    match receiver.recv_timeout(Duration::from_secs(30 * 60)) {
        Ok(outcome) => outcome,
        Err(_) => Err("网页操作已结束或等待超时".to_string()),
    }
}

/// 独立窗口关闭时调用，唤醒 [`await_external_window`] 的等待方。
///
/// 窗口不读回网页正文，所以这里用空 body 的 [`BrowserPage`] 表示「用户已完成
/// 操作」。调用方拿到后自己去读 Cookie（那才是脚本要的东西）。
pub fn complete_external_window(panel_id: &str, url: &str) {
    let sender = PENDING
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .remove(panel_id);
    if let Some(sender) = sender {
        let _ = sender.send(Ok(BrowserPage {
            url: url.to_string(),
            body: String::new(),
        }));
    }
}
pub fn complete_browser(panel_id: &str, page: BrowserPage) -> Result<(), String> {
    if page.body.trim().is_empty() {
        return Err("网页尚未加载完成，请稍后确认".to_string());
    }
    let sender = PENDING.lock().unwrap_or_else(|e| e.into_inner()).remove(panel_id)
        .ok_or_else(|| "网页操作已结束，请重新打开".to_string())?;
    sender.send(Ok(page)).map_err(|_| "网页操作已结束".to_string())
}

pub fn cancel_browser(panel_id: &str) {
    if let Some(sender) = PENDING.lock().unwrap_or_else(|e| e.into_inner()).remove(panel_id) {
        let _ = sender.send(Err("已取消网页操作".to_string()));
    }
}
