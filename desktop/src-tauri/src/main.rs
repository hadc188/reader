// Release builds are GUI apps: no console window should flash on launch.
#![cfg_attr(
    all(not(debug_assertions), target_os = "windows"),
    windows_subsystem = "windows"
)]

mod portable;
mod webview_memory;
mod win;

use reader_rust::api::commands::window::CLOSE_REQUEST_HANDLED;
use reader_rust::api::protocol::handle_reader_scheme;
use reader_rust::app::bootstrap;
use reader_rust::app::config::AppConfig;
use reader_rust::app::window_state::{capture, InitialGeometry, WindowStateStore};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tauri::menu::MenuItemBuilder;
use tauri::tray::{MouseButton, TrayIconBuilder, TrayIconEvent};
use tauri::webview::{NewWindowResponse, PageLoadEvent};
use tauri::{
    App, Emitter, Manager, WebviewUrl, WebviewWindowBuilder, WindowEvent,
};

/// Hosts allowed to navigate / open a new window inside the app webview:
/// the application origins themselves plus the login page family and the
/// third-party OAuth hosts (QQ/WeChat/captcha).
///
/// `on_navigation` and `on_new_window` share this check: an http(s) URL whose
/// host is not in this list must be handed to the system browser, while the
/// listed hosts are allowed to stay in-app (the QQ OAuth popup needs a new
/// app window — opening it in the system browser would log the user in there
/// and leave the app's session untouched).
fn host_allowed_in_app(url_host: Option<&str>) -> bool {
    matches!(
        url_host,
        Some("tauri.localhost")
            | Some("reader.localhost")
            | Some("localhost")
            | Some("passport.qidian.com")
            | Some("passport.yuewen.com")
            | Some("login.qidian.com")
            | Some("ywloginstatic.yuewen.com")
            | Some("open.weixin.qq.com")
            | Some("xui.ptlogin2.qq.com")
            | Some("ptlogin2.qq.com")
            | Some("graph.qq.com")
            | Some("ssl.ptlogin2.qq.com")
            | Some("turing.captcha.qcloud.com")
            | Some("ssl.captcha.qq.com")
            | Some("captcha.qq.com")
            | Some("qidian.gtimg.com")
            | Some("tcaptcha.qq.com")
    )
}

/// 窗口显示的兜底超时: 页面加载失败(WebView2 异常、资源损坏)时
/// `on_page_load` 永远不触发, 窗口会一直隐藏着 —— 那比白屏更糟, 用户会以为
/// 应用根本没启动。超过这个时间无论如何都显示窗口。
const REVEAL_FALLBACK: Duration = Duration::from_secs(3);

/// 关闭请求交给前端后的宽限期。
///
/// 前端会先 flush 阅读位置(自身有 1.5s 上限)再 hide/destroy, 这里留出余量;
/// 超过仍未见前端回执 `acknowledge_close_request`, 说明 `close-requested` 没人
/// 处理(监听未注册、IPC 卡死), 由看门狗结束窗口 —— 关不掉窗口比少存一次进度更糟。
const CLOSE_FRONTEND_GRACE: Duration = Duration::from_secs(3);

/// 首屏显示状态。
///
/// 窗口以隐藏方式建窗, 页面加载完成后再显示。`on_page_load` 与兜底超时会
/// 竞争, `revealed` 保证只有先到的那次生效(显示动作必须幂等)。
#[derive(Default)]
struct RevealState {
    revealed: AtomicBool,
    /// 是否需要以最大化状态显示。不能在 `on_page_load` 之前调用 `maximize()`:
    /// tao 在 Windows 上的 `set_maximized` 内部就是 `ShowWindow(SW_MAXIMIZE)`,
    /// 隐藏窗口会因此立刻可见, 白屏又会露出来。
    maximize: AtomicBool,
}

/// 显示主窗口。首次调用生效, 之后是空操作。
fn reveal(window: &tauri::WebviewWindow, state: &RevealState) {
    if state.revealed.swap(true, Ordering::SeqCst) {
        return;
    }
    // 最大化必须在显示之前完成: 否则用户会先看到常规尺寸的窗口, 下一帧才跳成
    // 整屏。(`set_maximized` 自身就会显示窗口, 随后的 `show` 是幂等的兜底。)
    if state.maximize.load(Ordering::SeqCst) {
        let _ = window.maximize();
    }
    // `maximize()` 会让 tao 重算窗口样式（同迷你模式的置顶切换），
    // 重放一次整窗透明度，避免启动即最大化时透明度丢失。
    reader_rust::api::commands::window::reapply_window_opacity(window);
    let _ = window.show();
    let _ = window.set_focus();
}

/// 显示主窗口前先恢复 WebView2 的正常内存级别。
fn show_main_window(window: &tauri::WebviewWindow) {
    webview_memory::apply(window, webview_memory::MemoryLevel::Normal);
    let _ = window.show();
    let _ = window.unminimize();
    let _ = window.set_focus();
}

fn main() {
    #[cfg(target_os = "windows")]
    const WEBVIEW2_DOWNLOAD_URL: &str = "https://developer.microsoft.com/microsoft-edge/webview2/";

    // Without this check Tauri panics deep inside window creation with a message
    // no end user can act on.
    #[cfg(target_os = "windows")]
    if let Err(err) = tauri::webview_version() {
        win::fatal(&format!(
            "未检测到 Microsoft Edge WebView2 运行时，应用无法启动。\n\n\
             请先安装 WebView2 Runtime 后重试：\n{WEBVIEW2_DOWNLOAD_URL}\n\n\
             详细信息：{err}"
        ));
        win::open_external(WEBVIEW2_DOWNLOAD_URL);
        std::process::exit(1);
    }

    let mut context = tauri::generate_context!();
    // 每个数据目录一个实例 identifier: 单实例插件按它做互斥量, WebView2 的数据
    // 目录也按它派生。否则两个便携副本会互相顶掉、共用一份 localStorage
    // (issue #20 第 4 条)。必须在 build 之前改 —— 插件在 setup 时读取。
    let paths = match portable::resolve() {
        Ok(paths) => paths,
        Err(err) => {
            win::fatal(&format!("启动失败：{err:#}"));
            std::process::exit(1);
        }
    };
    context.config_mut().identifier = paths.instance_identifier.clone();

    // 冷启动：系统通过 `legado://` 唤起时，URL 在命令行参数里。此时 webview 还没
    // 建好，前端也还没挂载，所以只暂存，等前端就绪后自己来取。
    if let Some(url) =
        reader_rust::api::deep_link::find_deeplink(&std::env::args().collect::<Vec<_>>())
    {
        reader_rust::api::deep_link::set_pending(url);
    }

    let app = tauri::Builder::default()
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_single_instance::init(|app, args, _cwd| {
            // 应用已经在运行时又被 `legado://` 唤起：URL 由第二个进程用启动参数带进来，
            // 这里转交给已运行实例。先暂存，再发事件让前端立刻来取。
            if let Some(url) = reader_rust::api::deep_link::find_deeplink(&args) {
                reader_rust::api::deep_link::set_pending(url);
                let _ = app.emit("legado-deeplink", ());
            }

            // Surface the existing window instead of starting a second instance.
            if let Some(window) = app.get_webview_window("main") {
                show_main_window(&window);
            }
        }))
        .invoke_handler(reader_rust::api::invoke_handler())
        .register_asynchronous_uri_scheme_protocol("reader", handle_reader_scheme)
        .setup(move |app| {
            if let Err(err) = start(app, &paths) {
                let message = format!("启动失败：{err:#}");
                win::fatal(&message);
                return Err(message.into());
            }
            Ok(())
        })
        .build(context);

    let app = match app {
        Ok(app) => app,
        Err(err) => {
            win::fatal(&format!("应用运行失败：{err}"));
            std::process::exit(1);
        }
    };

    app.run(|app_handle, event| {
        // 退出前补写最后一次窗口变化, 否则刚调整完尺寸就退出的用户会丢失它。
        if matches!(event, tauri::RunEvent::ExitRequested { .. }) {
            if let Some(window) = app_handle.get_webview_window("main") {
                persist_window_state(app_handle, &window, true);
            }
        }
    });
}

fn start(app: &mut App, paths: &portable::Paths) -> anyhow::Result<()> {
    if paths.relocated_notice {
        win::warn(&format!(
            "程序所在目录不可写，书库数据已改存到：\n{}",
            paths.data_dir.display()
        ));
    }
    // 旧版浏览器数据(主题、字号、书架筛选等界面偏好)被占用没搬过来时要说一声,
    // 否则用户只会看到"升级后设置全没了"。说明文件里有手动迁移的步骤。
    if let Some(notice) = &paths.webview_migration_skipped {
        win::warn(&format!(
            "旧版界面设置仍在被另一个旧窗口使用，本次未自动迁移。\n\n\
             详情：\n{}\n\n\
             （书库、书源、阅读进度不受影响。）",
            notice.display()
        ));
    }

    let cfg = AppConfig {
        // A bare path rather than a `sqlite:` URL: Windows paths with a drive
        // letter, backslashes or spaces do not survive URL parsing. See
        // reader_rust::storage::db::connect_options.
        database_url: paths.db.to_string_lossy().into_owned(),
        storage_dir: paths.storage.to_string_lossy().into_owned(),
        assets_dir: paths.assets.to_string_lossy().into_owned(),
        ..AppConfig::default()
    };
    bootstrap::init_tracing(&cfg.log_level);

    let state = tauri::async_runtime::block_on(bootstrap::build_state(&cfg))?;
    app.manage(state);

    // Web 访问: 状态容器 + 按上次保存的配置自动启动。
    // 必须在 `app.manage(state)` 之后 —— autostart 需要 `AppState`。
    app.manage(reader_rust::server::control::WebServerState::new(
        paths.data_dir.clone(),
    ));
    {
        let handle = app.handle().clone();
        // 放到后台线程: 端口被占用时 `TcpListener::bind` 会阻塞在系统调用上,
        // 不能让 Web 的失败拖慢桌面窗口首帧。
        //
        // `autostart` 是 async 的, 所以这里要显式 block_on —— 但**在这个专用
        // 线程上**做, 不能像早先那样在 Tauri 命令线程里 block_on 整个启动流程。
        std::thread::spawn(move || {
            let Some(web_state) = handle
                .try_state::<reader_rust::server::control::WebServerState>()
            else {
                return;
            };
            let Some(app_state) = handle.try_state::<reader_rust::api::AppState>() else {
                return;
            };
            let static_source = reader_rust::server::control::resolve_static_source(&handle);
            tauri::async_runtime::block_on(reader_rust::server::control::autostart(
                &web_state,
                &app_state,
                static_source,
            ));
        });
    }

    // Load the embedded frontend assets in both debug and release. The old
    // debug path pointed at a Vite dev server (localhost:5173) that isn't
    // running in a plain `cargo build`, which showed ERR_CONNECTION_REFUSED.
    let store = WindowStateStore::new(paths.window_state.clone());
    let saved_state = store.load();
    // 迷你模式下窗口下限更小(控件只有上一章/下一章/目录, 不需要完整布局宽度)。
    let initial_mini = saved_state.map(|s| s.mini_mode).unwrap_or(false);
    let (min_width, min_height) = InitialGeometry::min_size_for(initial_mini);

    // 首屏显示状态。窗口先隐藏建窗, 等页面加载完成(或兜底超时)再一次性显示,
    // 避免用户在 WebView2 还没画出任何内容时看到一块白板 —— 见 reveal。
    let reveal_state = Arc::new(RevealState::default());

    let window = WebviewWindowBuilder::new(app, "main", WebviewUrl::App("index.html".into()))
        .title("阅读")
        .inner_size(InitialGeometry::DEFAULT_WIDTH, InitialGeometry::DEFAULT_HEIGHT)
        .min_inner_size(min_width, min_height)
        // 自定义标题栏: 隐藏原生边框。
        .decorations(false);
    // WebView2 默认把 profile 放在 `%LOCALAPPDATA%\<identifier>`, 与程序目录
    // 无关 —— 两个便携副本会共用同一份 localStorage(issue #20 第 4 条)。
    // 显式指到数据目录下, 让"拷一个文件夹"真的带走全部状态。
    // 只有 Windows 支持自定义 profile 目录; macOS/Linux 的等价开关是另一种 API,
    // 那边本来就按 identifier 分开, 不需要改。
    #[cfg(target_os = "windows")]
    let window = window.data_directory(paths.webview_data.clone());
    let window = window
        .center()
        // 先不显示。Tauri 默认为 visible(true), 于是窗口在 WebView2 尚未渲染出
        // 首帧时就弹出来, 用户先看到一块白板 —— 这正是启动白闪的第一段。
        .visible(false)
        .on_page_load({
            let reveal_state = Arc::clone(&reveal_state);
            move |window, payload| {
                if payload.event() == PageLoadEvent::Finished {
                    reveal(&window, &reveal_state);
                }
            }
        })
        .on_navigation(|url| {
            match url.scheme() {
                // Exporting book sources and downloading backup files navigate
                // to blob: URLs created by the page itself.
                "blob" | "about" => true,
                // The app itself and the reader scheme. On Linux/macOS the
                // webview serves them as tauri://localhost and reader://localhost,
                // while Windows (WebView2) maps them to http://tauri.localhost /
                // http://reader.localhost (covered by the http arms below).
                // Denying these cancels even the initial page load — the window
                // then stays permanently white.
                "tauri" | "reader" => true,
                // Keep the application origins and login/OAuth hosts inside the
                // webview. Ordinary links go to the system browser.
                "http" | "https" if host_allowed_in_app(url.host_str()) => true,
                "http" | "https" => {
                    win::open_external(url.as_str());
                    false
                }
                _ => false,
            }
        })
        .on_new_window(|url, _features| {
            // 与 on_navigation 的 host_allowed_in_app 保持一致: 登录/OAuth 弹窗
            // (如 QQ OAuth 的 window.open(open.weixin.qq.com/...)) 必须留在应用内,
            // 否则用户在系统浏览器登录后, 应用 webview 会话仍处于未登录态,
            // 登录态采集脚本读不到 cookie。非允许列表的 host 才交给系统浏览器。
            if matches!(url.scheme(), "http" | "https")
                && !host_allowed_in_app(url.host_str())
            {
                win::open_external(url.as_str());
                return NewWindowResponse::Deny;
            }
            NewWindowResponse::Allow
        })
        .build()?;

    // 用保存的尺寸/位置覆盖默认值。要在 `.center()` 之后应用, 否则会先居中
    // 再跳走, 用户能看到闪烁; 先建窗再调整是因为安全范围校验需要窗口句柄。
    // 此时窗口尚未显示(见 `.visible(false)`), 这些调整都在用户看到窗口之前
    // 完成 —— 早先窗口是可见的, 于是"先按默认尺寸弹出、再跳成保存的尺寸/整屏"
    // 成了启动白闪的第二段。
    let geometry = InitialGeometry::resolve(&window, saved_state);
    let _ = window.set_size(geometry.size);
    match geometry.position {
        Some(position) => {
            let _ = window.set_position(position);
        }
        // 位置越界(显示器被拔掉)或没有历史状态时居中。
        None => {
            let _ = window.center();
        }
    }
    if geometry.maximized {
        // 只记下意图, 显示时再最大化 —— 见 RevealState::maximize 的说明。
        reveal_state.maximize.store(true, Ordering::SeqCst);
    }
    // 迷你模式: 自动置顶, 并在下次启动时仍以迷你尺寸建窗(见 resolve)。
    if geometry.mini_mode {
        let _ = window.set_always_on_top(true);
    }

    // 兜底: 页面加载失败(WebView2 异常、资源损坏)时 `on_page_load` 不会触发,
    // 窗口会一直隐藏着。宁可露出白屏, 也不能让用户以为应用没启动。
    {
        let window = window.clone();
        let reveal_state = Arc::clone(&reveal_state);
        std::thread::spawn(move || {
            std::thread::sleep(REVEAL_FALLBACK);
            reveal(&window, &reveal_state);
        });
    }

    app.manage(Mutex::new(store));

    // Some Linux desktop environments do not provide a status notifier. The
    // app must still start there, and closing the window must really exit.
    //
    // 托盘是否可用只影响"关闭时隐藏还是退出"的**默认**行为(没有托盘就没有
    // 可隐藏的目标), 关闭请求本身的拦截已与它无关 —— 见下方那段说明。
    if let Err(err) = setup_tray(app) {
        eprintln!("阅读：系统托盘不可用：{err:#}");
        win::warn("当前桌面环境无法创建系统托盘，关闭窗口时将直接退出应用。");
    }

    // 关闭时补写一次几何, 并跳过节流。
    //
    // 写盘只由 Resized/Moved 驱动, 而 1s 节流会丢掉拖动末尾那次事件 —— 拖动
    // 结束时窗口的真实位置往往还没落盘。关闭之后窗口要么隐藏(托盘)要么被前端
    // `destroy()`, 两者都不会再触发写盘, 于是"拖完随即关闭"保存的是拖动过程中
    // 的中间位置(偏差可达节流窗口内的拖动距离), 表现为位置记录时对时错。
    //
    // 这个监听器只补写几何, **不拦截**关闭本身; 真正拦截关闭的是下面那个监听器。
    {
        let app_handle = app.handle().clone();
        let tracked = window.clone();
        window.on_window_event(move |event| {
            if matches!(event, WindowEvent::CloseRequested { .. }) {
                persist_window_state(&app_handle, &tracked, true);
            }
        });
    }

    // 关闭请求先交给前端: 它要在窗口消失前把阅读位置落到书架(对应 legado 在
    // `ReadBookActivity.onPause` 里的 saveRead)。
    //
    // 这里**不能**只在托盘可用时拦截: 托盘不可用(部分 Linux 桌面环境)时, 关闭会
    // 直接销毁窗口, 前端收不到任何通知, `beforeunload` 与 Vue 的 onUnmounted 都不
    // 保证执行 —— 在阅读页关窗就会丢掉当前阅读位置。拦截后由前端根据「关闭到
    // 托盘 / 直接退出」的偏好决定 hide 还是 destroy, 两条路径都能先保存再动手。
    //
    // 但"拦截"必须有两层兜底, 否则窗口会关不掉:
    // 1. 窗口还没显示出来时(首帧未渲染, 或加载失败后由 3s 兜底超时显示)前端可能
    //    还没注册 `close-requested` 监听, 这时**不拦截**, 保持原有的直接关闭;
    // 2. 窗口已显示但监听尚未注册 / IPC 卡住时, 用一个看门狗在宽限期后替前端结束
    //    窗口。判断依据是"这一次关闭请求有没有被前端回执接管"(见
    //    `CLOSE_REQUEST_HANDLED`), 而不是轮询窗口可见性 —— 后者会把"隐藏到托盘后
    //    又立刻从托盘打开"误判成"前端没响应", 进而把用户刚打开的窗口销毁。
    {
        let app_handle = app.handle().clone();
        let page_ready = Arc::clone(&reveal_state);
        let window_for_event = window.clone();
        window.on_window_event(move |event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                if !page_ready.revealed.load(Ordering::SeqCst) {
                    // 窗口尚未显示: 前端可能还没装好监听, 放行让关闭按默认行为走。
                    return;
                }
                api.prevent_close();
                CLOSE_REQUEST_HANDLED.store(false, Ordering::SeqCst);
                let _ = app_handle.emit("close-requested", ());
                let fallback = window_for_event.clone();
                std::thread::spawn(move || {
                    std::thread::sleep(CLOSE_FRONTEND_GRACE);
                    if CLOSE_REQUEST_HANDLED.swap(true, Ordering::SeqCst) {
                        // 前端已回执(窗口已隐藏或已销毁), 看门狗退让。
                        return;
                    }
                    // 没有任何一方响应: 宁可丢一次进度, 也不能让窗口关不掉。
                    let _ = fallback.destroy();
                });
            }
        });
    }

    // 窗口尺寸/位置持久化。与上面的关闭拦截分开注册是安全的: Tauri 会同时
    // 调用同一窗口的多个监听器, 互不覆盖。
    {
        let app_handle = app.handle().clone();
        let tracked = window.clone();
        window.on_window_event(move |event| {
            if matches!(event, WindowEvent::Resized(_) | WindowEvent::Moved(_)) {
                persist_window_state(&app_handle, &tracked, false);
            }
        });
    }

    #[cfg(target_os = "windows")]
    setup_webview_memory_events(window.clone(), Arc::clone(&reveal_state));

    Ok(())
}

/// 最小化/隐藏后延迟进入低内存模式，恢复可见时回到正常级别。
#[cfg(target_os = "windows")]
fn setup_webview_memory_events(window: tauri::WebviewWindow, page_ready: Arc<RevealState>) {
    let event_window = window.clone();
    window.on_window_event(move |event| match event {
        WindowEvent::Focused(true) => {
            webview_memory::apply(&event_window, webview_memory::MemoryLevel::Normal);
        }
        WindowEvent::Resized(_) if event_window.is_minimized().unwrap_or(false) => {
            let target = event_window.clone();
            let page_ready = Arc::clone(&page_ready);
            std::thread::spawn(move || {
                std::thread::sleep(std::time::Duration::from_secs(2));
                if page_ready.revealed.load(Ordering::SeqCst)
                    && target.is_minimized().unwrap_or(false)
                {
                    webview_memory::apply(&target, webview_memory::MemoryLevel::Low);
                }
            });
        }
        _ => {}
    });
}

/// 记录窗口尺寸/位置。`force` 为真时跳过节流, 用于退出前补写最后一次变化。
fn persist_window_state(app: &tauri::AppHandle, window: &tauri::WebviewWindow, force: bool) {
    let Some(store) = app.try_state::<Mutex<WindowStateStore>>() else {
        return;
    };
    let Ok(mut store) = store.lock() else {
        return;
    };
    if !force && !store.should_save_now() {
        return;
    }
    // 最小化时位置无意义(Windows 会报告 -32000), capture 内部会返回 None,
    // 因此不会把垃圾坐标写进状态文件。
    if let Some(state) = capture(window, store.load()) {
        store.save(state);
    }
}

/// System tray so the app can keep running in the background after the window
/// is hidden. Left-click / double-click shows the window; the menu offers show
/// and quit.
fn setup_tray(app: &mut App) -> anyhow::Result<()> {
    let icon = app
        .default_window_icon()
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("应用图标不可用"))?;
    let show = MenuItemBuilder::with_id("show", "显示主界面").build(app)?;
    let quit = MenuItemBuilder::with_id("quit", "退出").build(app)?;
    let menu = tauri::menu::MenuBuilder::new(app)
        .item(&show)
        .item(&quit)
        .build()?;

    let tray = TrayIconBuilder::with_id("reader-tray")
        .icon(icon)
        .tooltip("阅读")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_tray_icon_event(|tray, event| {
            // Only a left click shows the window. A right click must be left to
            // the context menu (show/quit); showing the window on right-click
            // steals focus and makes the menu flicker and close immediately.
            let is_left = match event {
                TrayIconEvent::Click { button, .. } | TrayIconEvent::DoubleClick { button, .. } => {
                    button == MouseButton::Left
                }
                _ => false,
            };
            if is_left {
                if let Some(window) = tray.app_handle().get_webview_window("main") {
                    show_main_window(&window);
                }
            }
        })
        .on_menu_event(|app, event| match event.id.as_ref() {
            "show" => {
                if let Some(window) = app.get_webview_window("main") {
                    show_main_window(&window);
                }
            }
            "quit" => {
                app.exit(0);
            }
            _ => {}
        })
        .build(app)?;

    // Keep the tray icon alive for the app's whole lifetime. Dropping the
    // TrayIcon removes it, but Windows often leaves a stale shell icon whose
    // context menu is dead — which manifests as a frozen, unclickable tray.
    Box::leak(Box::new(tray));

    Ok(())
}
