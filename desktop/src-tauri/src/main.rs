// Release builds are GUI apps: no console window should flash on launch.
#![cfg_attr(
    all(not(debug_assertions), target_os = "windows"),
    windows_subsystem = "windows"
)]

mod portable;
mod win;

use reader_rust::api::protocol::handle_reader_scheme;
use reader_rust::app::bootstrap;
use reader_rust::app::config::AppConfig;
use tauri::menu::MenuItemBuilder;
use tauri::tray::{MouseButton, TrayIconBuilder, TrayIconEvent};
use tauri::webview::NewWindowResponse;
use tauri::{App, Emitter, Manager, WebviewUrl, WebviewWindowBuilder, WindowEvent};

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

    let result = tauri::Builder::default()
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            // Surface the existing window instead of starting a second instance.
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.unminimize();
                let _ = window.show();
                let _ = window.set_focus();
            }
        }))
        .invoke_handler(reader_rust::api::invoke_handler())
        .register_asynchronous_uri_scheme_protocol("reader", handle_reader_scheme)
        .setup(|app| {
            if let Err(err) = start(app) {
                let message = format!("启动失败：{err:#}");
                win::fatal(&message);
                return Err(message.into());
            }
            Ok(())
        })
        .run(tauri::generate_context!());

    if let Err(err) = result {
        win::fatal(&format!("应用运行失败：{err}"));
        std::process::exit(1);
    }
}

fn start(app: &mut App) -> anyhow::Result<()> {
    let paths = portable::resolve()?;

    if paths.relocated_notice {
        win::warn(&format!(
            "程序所在目录不可写，书库数据已改存到：\n{}",
            paths.data_dir.display()
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

    // Load the embedded frontend assets in both debug and release. The old
    // debug path pointed at a Vite dev server (localhost:5173) that isn't
    // running in a plain `cargo build`, which showed ERR_CONNECTION_REFUSED.
    let window = WebviewWindowBuilder::new(app, "main", WebviewUrl::App("index.html".into()))
        .title("阅读")
        .inner_size(1180.0, 820.0)
        .min_inner_size(880.0, 600.0)
        // Custom title bar drawn by the frontend; this hides the native frame.
        .decorations(false)
        .center()
        .on_navigation(|url| {
            let is_internal = matches!(
                url.host_str(),
                Some("tauri.localhost") | Some("reader.localhost") | Some("localhost")
            );
            match url.scheme() {
                // Exporting book sources and downloading backup files navigate
                // to blob: URLs created by the page itself.
                "blob" | "about" => true,
                // Keep the application origins inside the webview. Ordinary
                // links should use the system browser on every platform.
                "http" | "https" if is_internal => true,
                "http" | "https" => {
                    win::open_external(url.as_str());
                    false
                }
                _ => false,
            }
        })
        .on_new_window(|url, _features| {
            if matches!(url.scheme(), "http" | "https") {
                win::open_external(url.as_str());
            }
            NewWindowResponse::Deny
        })
        .build()?;

    // Some Linux desktop environments do not provide a status notifier. The
    // app must still start there, and closing the window must really exit.
    let tray_available = match setup_tray(app) {
        Ok(()) => true,
        Err(err) => {
            eprintln!("阅读：系统托盘不可用：{err:#}");
            win::warn("当前桌面环境无法创建系统托盘，关闭窗口时将直接退出应用。");
            false
        }
    };

    if tray_available {
        // The frontend resolves each intercepted close request using the
        // persisted preference: hide to tray or exit.
        let app_handle = app.handle().clone();
        window.on_window_event(move |event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = app_handle.emit("close-requested", ());
            }
        });
    }

    Ok(())
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
                    let _ = window.show();
                    let _ = window.unminimize();
                    let _ = window.set_focus();
                }
            }
        })
        .on_menu_event(|app, event| match event.id.as_ref() {
            "show" => {
                if let Some(window) = app.get_webview_window("main") {
                    let _ = window.show();
                    let _ = window.unminimize();
                    let _ = window.set_focus();
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
