use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

use serde::Serialize;
use tauri::{Emitter, Manager};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};

use crate::app::window_state::{
    capture, monitor_rects, sanitize, sanitize_geometry, InitialGeometry, WindowState,
    WindowStateStore,
};

/// 窗口整体不透明度的下限。低于该值时文字与控件会淡到几乎不可读，
/// 且 Windows 分层窗口在极低 alpha 下更容易出现整窗“消失”的观感。
pub const WINDOW_OPACITY_MIN: f64 = 0.1;

/// 主窗口当前生效的整窗不透明度。
///
/// 必须由 Rust 侧记住：tao 的 `set_always_on_top`（迷你模式切换）会通过
/// `WindowState::set_window_flags` 整体重算 `GWL_EXSTYLE`，把手工加的
/// `WS_EX_LAYERED` 冲掉。切换迷你模式后必须按这个值重放一次透明度。
static WINDOW_OPACITY: Mutex<f64> = Mutex::new(1.0);

/// 把任意输入钳制到可用的窗口不透明度区间。
///
/// 非有限值（NaN/Infinity）一律回落到 1.0：这类值通常来自前端脏存档，
/// 直接交给 Win32 会得到未定义行为。
pub fn normalize_window_opacity(value: f64) -> f64 {
    if !value.is_finite() {
        return 1.0;
    }
    value.clamp(WINDOW_OPACITY_MIN, 1.0)
}

/// 把不透明度真正写到窗口上（不记录状态）。
///
/// 采用 `WS_EX_LAYERED` + `SetLayeredWindowAttributes(LWA_ALPHA)`：
/// 这是 Win32 上对“整窗 alpha”支持最广、不依赖 DWM 合成开关的做法。
///
/// 注意**不要**在 100% 时移除 `WS_EX_LAYERED`：tao/WebView2 自己就会给顶层
/// 窗口带上这个标志（实测 ex-style 已含 0x80000），贸然摘掉会破坏它们维护的
/// 窗口状态。100% 时把 alpha 设回 255 即可，效果与不透明一致。
#[cfg(target_os = "windows")]
fn apply_window_opacity(window: &tauri::WebviewWindow, opacity: f64) -> Result<(), String> {
    use windows::Win32::Foundation::COLORREF;
    use windows::Win32::UI::WindowsAndMessaging::{
        GetWindowLongW, SetLayeredWindowAttributes, SetWindowLongW, GWL_EXSTYLE, LWA_ALPHA,
        WS_EX_LAYERED,
    };

    let hwnd = window.hwnd().map_err(|error| error.to_string())?;
    // SAFETY: hwnd 来自 Tauri 的存活窗口；GetWindowLongW/SetWindowLongW 只
    // 读写该窗口的扩展样式，SetLayeredWindowAttributes 的目标也是同一个
    // 窗口。三者都是 Win32 文档允许的调用方式。
    unsafe {
        let ex_style = GetWindowLongW(hwnd, GWL_EXSTYLE);
        if ex_style & (WS_EX_LAYERED.0 as i32) == 0 {
            SetWindowLongW(hwnd, GWL_EXSTYLE, ex_style | (WS_EX_LAYERED.0 as i32));
        }
        let alpha = (opacity * 255.0).round() as u8;
        if SetLayeredWindowAttributes(hwnd, COLORREF(0), alpha, LWA_ALPHA).is_err() {
            return Err("设置窗口透明度失败".to_string());
        }
    }
    Ok(())
}

#[cfg(not(target_os = "windows"))]
fn apply_window_opacity(_window: &tauri::WebviewWindow, _opacity: f64) -> Result<(), String> {
    Err("当前平台不支持窗口透明度".to_string())
}

/// 记录并应用整窗不透明度。
fn remember_and_apply(window: &tauri::WebviewWindow, opacity: f64) -> Result<(), String> {
    let opacity = normalize_window_opacity(opacity);
    apply_window_opacity(window, opacity)?;
    if let Ok(mut current) = WINDOW_OPACITY.lock() {
        *current = opacity;
    }
    Ok(())
}

/// 设置主窗口的整体不透明度（仅 Windows 生效）。
///
/// 非 Windows 平台返回错误而不是静默成功：前端据此隐藏入口，
/// 调用方也能区分“不支持”和“设置成功”。
#[tauri::command]
pub fn set_window_opacity(app: tauri::AppHandle, opacity: f64) -> Result<(), String> {
    let window = app
        .get_webview_window("main")
        .ok_or_else(|| "主窗口不存在".to_string())?;
    remember_and_apply(&window, opacity)
}

/// 重新施加当前记录的透明度。
///
/// 供窗口扩展样式被第三方（tao 的置顶切换等）整体重算后调用。
pub fn reapply_window_opacity(window: &tauri::WebviewWindow) {
    let opacity = WINDOW_OPACITY
        .lock()
        .map(|value| *value)
        .unwrap_or(1.0);
    let _ = apply_window_opacity(window, opacity);
}

/// 统一显示主窗口的入口。
///
/// 老板键、托盘和单实例唤起都走这里，确保每次都重放整窗透明度并取消最小化；
/// 只调用 `show()` 会让部分显示路径看起来正常、老板键恢复时却丢失透明度。
pub fn show_main_window(window: &tauri::WebviewWindow) {
    reapply_window_opacity(window);
    let _ = window.show();
    let _ = window.unminimize();
    let _ = window.set_focus();
    // show 之后再重放一次：WebView/分层窗口在首次显示时会重建合成层，
    // 只在 show 前设置，隐藏后恢复仍有概率退回不透明。
    let window_after_show = window.clone();
    let app_handle = window.app_handle().clone();
    let _ = window.app_handle().run_on_main_thread(move || {
        reapply_window_opacity(&window_after_show);
        // 原生 alpha 已经重放后再通知前端，避免前端过早把它改回 100%。
        let _ = app_handle.emit("window-shown", ());
    });
}

/// 当前平台是否支持整窗不透明度。前端据此决定是否展示设置项，
/// 避免 macOS/Linux 上出现一个拖了没反应的滑块。
#[tauri::command]
pub fn is_window_opacity_supported() -> bool {
    cfg!(target_os = "windows")
}

/// 切换主窗口的原生阴影。
///
/// 只调用 Tauri 的 shadow 开关，不改窗口边框、缩放样式或 DWM 非客户区。
/// 这样关闭阴影时仍是当前验证过的无边框形态。
#[tauri::command]
pub fn set_window_shadow(app: tauri::AppHandle, enabled: bool) -> Result<(), String> {
    let window = app
        .get_webview_window("main")
        .ok_or_else(|| "主窗口不存在".to_string())?;
    window
        .set_shadow(enabled)
        .map_err(|error| format!("设置窗口阴影失败：{error}"))
}

/// 本轮关闭请求是否已被前端接管。
///
/// 外壳在拦截关闭请求后启动看门狗: 宽限期内前端若已 hide(关闭到托盘)或
/// destroy(退出), 就在这里置位, 看门狗据此退让; 否则替前端结束窗口, 避免
/// "窗口关不掉"。
///
/// 用显式回执而不是轮询窗口可见性: 隐藏到托盘后用户可能立刻从托盘重新打开
/// 窗口, 轮询会把那次重开误判成"前端没响应"并销毁窗口。
pub static CLOSE_REQUEST_HANDLED: AtomicBool = AtomicBool::new(false);

/// 前端回执: 这一轮关闭请求已经处理完(窗口已隐藏或已销毁)。
#[tauri::command]
pub fn acknowledge_close_request() {
    CLOSE_REQUEST_HANDLED.store(true, Ordering::SeqCst);
}

/// 窗口模式状态, 供前端决定是否使用紧凑布局。
#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WindowMode {
    pub mini_mode: bool,
    pub always_on_top: bool,
}

/// 读取当前状态(短暂持锁即释放)。
fn read_state(app: &tauri::AppHandle) -> Result<WindowState, String> {
    let store = app
        .try_state::<Mutex<WindowStateStore>>()
        .ok_or_else(|| "窗口状态未初始化".to_string())?;
    // `inner()` 返回与 AppHandle 同生命周期的引用, 避免把 MutexGuard 绑在
    // try_state 返回的临时 State 上(那样会借用局部变量)。
    let guard = store
        .inner()
        .lock()
        .map_err(|_| "窗口状态已损坏".to_string())?;
    Ok(guard.load().unwrap_or_default())
}

/// 写回状态(短暂持锁即释放)。
fn write_state(app: &tauri::AppHandle, state: WindowState) -> Result<(), String> {
    let store = app
        .try_state::<Mutex<WindowStateStore>>()
        .ok_or_else(|| "窗口状态未初始化".to_string())?;
    let mut guard = store
        .inner()
        .lock()
        .map_err(|_| "窗口状态已损坏".to_string())?;
    guard.save(state);
    Ok(())
}

/// 查询当前窗口模式(前端启动时同步紧凑布局)。
#[tauri::command]
pub fn get_window_mode(app: tauri::AppHandle) -> Result<WindowMode, String> {
    let mini_mode = read_state(&app)
        .map(|state| state.mini_mode)
        .unwrap_or(false);
    Ok(WindowMode {
        mini_mode,
        always_on_top: mini_mode,
    })
}

/// 切换迷你模式。
///
/// 进入时把当前常规几何先记下来(退出后能回到原位)并自动置顶; 退出时回到常规
/// 几何。两套几何分别保存, 因此来回切换不会丢失各自被调整过的尺寸。
///
/// 注意两点:
/// 1. 全程**不持锁**做窗口操作。`set_size`/`unmaximize` 会触发 `Resized`
///    事件, 其回调要拿同一把锁; 若在这里持锁调用它们就会自锁。
/// 2. 模式标志必须在窗口操作**之前**落盘。否则 `Resized` 回调读到的还是旧的
///    `mini_mode`, 会把新的迷你尺寸错记进常规几何, 退出迷你模式后就回不去了。
#[tauri::command]
pub fn toggle_mini_mode(app: tauri::AppHandle) -> Result<WindowMode, String> {
    let window = app
        .get_webview_window("main")
        .ok_or_else(|| "主窗口不存在".to_string())?;

    let mut state = read_state(&app)?;
    let monitors = monitor_rects(&window);
    let entering_mini = !state.mini_mode;
    let min_size = InitialGeometry::min_size_for(entering_mini);

    // 1) 先把当前几何固化到"切换前"的那一套, 并翻转模式标志落盘。
    if let Some(captured) = capture(&window, Some(state)) {
        state = captured;
    }
    state.mini_mode = entering_mini;
    write_state(&app, state)?;

    // 2) 再做窗口操作。此时事件回调看到的 mode 已经是新值, 会写入正确的那一套。
    //    窗口下限必须跟着切换: 迷你模式用更小的下限, 否则 320×240 会被常规
    //    下限(420×320)抬回去, 用户永远缩不到目标尺寸。
    let _ = window.set_min_size(Some(tauri::LogicalSize::new(min_size.0, min_size.1)));
    if entering_mini {
        // 最大化与迷你模式互斥: 先还原, 否则在最大化状态下 set_size 不生效。
        let _ = window.unmaximize();
        let (width, height) = state.mini_size();
        let (x, y) = state.mini_position();
        let (size, position) = sanitize_geometry(width, height, x, y, &monitors, min_size);
        let _ = window.set_size(size);
        match position {
            Some(position) => {
                let _ = window.set_position(position);
            }
            None => {
                let _ = window.center();
            }
        }
        let _ = window.set_always_on_top(true);
    } else {
        let (size, position, maximized) = sanitize(state, &monitors, min_size);
        let _ = window.set_size(size);
        match position {
            Some(position) => {
                let _ = window.set_position(position);
            }
            None => {
                let _ = window.center();
            }
        }
        if maximized {
            let _ = window.maximize();
        }
        let _ = window.set_always_on_top(false);
    }

    // 3) 收尾: 把系统在 set_size 时做的尺寸修正(最小尺寸约束、显示器上限裁剪)
    //    记下来, 而不是残留我们请求的值。
    if let Some(final_state) = capture(&window, Some(state)) {
        state = final_state;
    }
    write_state(&app, state)?;

    // 4) 重新施加整窗透明度。
    //    上面的 set_always_on_top 会让 tao 通过 `WindowState::set_window_flags`
    //    整体重算 GWL_EXSTYLE，把手工加的 `WS_EX_LAYERED` 冲掉，
    //    表现就是"一切换迷你模式，窗口就不透明了"。这里按记住的值重放。
    reapply_window_opacity(&window);

    Ok(WindowMode {
        mini_mode: state.mini_mode,
        always_on_top: state.mini_mode,
    })
}

#[tauri::command]
pub fn configure_boss_key(
    app: tauri::AppHandle,
    shortcut: Option<String>,
) -> Result<(), String> {
    app.global_shortcut()
        .unregister_all()
        .map_err(|error| error.to_string())?;
    let Some(shortcut) = shortcut
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
    else {
        return Ok(());
    };
    if shortcut.split('+').any(|part| {
        matches!(
            part.trim().to_ascii_lowercase().as_str(),
            "super" | "meta" | "win"
        )
    }) {
        return Err("老板键不能使用系统键".to_string());
    }
    let parsed = shortcut
        .parse::<tauri_plugin_global_shortcut::Shortcut>()
        .map_err(|_| "快捷键格式无效".to_string())?;
    app.global_shortcut()
        .on_shortcut(parsed, |app, _shortcut, event| {
            if event.state != ShortcutState::Pressed {
                return;
            }
            if let Some(window) = app.get_webview_window("main") {
                if window.is_visible().unwrap_or(false) {
                    let _ = window.hide();
                    return;
                }
                // 全局快捷键回调可能不在 Tauri 主线程上，Windows 的 show /
                // 分层窗口调用必须回到主线程，否则偶发显示但样式未恢复。
                let app_handle = app.clone();
                let _ = app.run_on_main_thread(move || {
                    if let Some(window) = app_handle.get_webview_window("main") {
                        show_main_window(&window);
                    }
                });
            }
        })
        .map_err(|error| format!("快捷键注册失败：{error}"))
}

#[cfg(test)]
mod tests {
    use super::{normalize_window_opacity, WINDOW_OPACITY_MIN};

    /// 默认必须是 1：老用户升级后窗口不能突然变半透明。
    #[test]
    fn window_opacity_defaults_to_opaque() {
        assert_eq!(normalize_window_opacity(1.0), 1.0);
        assert_eq!(normalize_window_opacity(0.72), 0.72);
    }

    /// 低于下限会淡到无法阅读，统一钳到下限。
    #[test]
    fn window_opacity_clamps_to_readable_floor() {
        assert_eq!(normalize_window_opacity(0.0), WINDOW_OPACITY_MIN);
        assert_eq!(normalize_window_opacity(0.09), WINDOW_OPACITY_MIN);
        assert_eq!(normalize_window_opacity(-1.0), WINDOW_OPACITY_MIN);
    }

    /// 超过 1 没有意义，Win32 的 alpha 也只有 0-255。
    #[test]
    fn window_opacity_clamps_above_one() {
        assert_eq!(normalize_window_opacity(1.5), 1.0);
    }

    /// NaN/Infinity 来自前端脏存档，必须回落到不透明而不是传播给 Win32。
    #[test]
    fn window_opacity_rejects_non_finite_values() {
        assert_eq!(normalize_window_opacity(f64::NAN), 1.0);
        assert_eq!(normalize_window_opacity(f64::INFINITY), 1.0);
        assert_eq!(normalize_window_opacity(f64::NEG_INFINITY), 1.0);
    }
}
