//! WebView2 失活时的内存降级。
//!
//! `MemoryUsageTargetLevel::Low` 只请求浏览器回收缓存并换出可回收内存，
//! 不会暂停脚本或定时器；窗口重新激活时必须显式恢复 `Normal`。

use tauri::WebviewWindow;
#[cfg(target_os = "windows")]
use windows_core::Interface;
#[derive(Clone, Copy)]
pub(crate) enum MemoryLevel {
    Normal,
    Low,
}

pub(crate) fn apply(window: &WebviewWindow, level: MemoryLevel) {
    #[cfg(target_os = "windows")]
    {
        use webview2_com::Microsoft::Web::WebView2::Win32::{
            ICoreWebView2_19, COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_LOW,
            COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_NORMAL,
        };

        let target = match level {
            MemoryLevel::Normal => COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_NORMAL,
            MemoryLevel::Low => COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_LOW,
        };
        let _ = window.with_webview(move |platform| {
            let Ok(controller) = platform.controller().cast::<ICoreWebView2_19>() else {
                // 旧版 WebView2 Runtime 没有该接口，保持原行为。
                return;
            };
            // SAFETY: controller 是当前窗口的活 WebView2 控制器，且调用在
            // Tauri 主线程派发上下文中执行。
            let _ = unsafe { controller.SetMemoryUsageTargetLevel(target) };
        });
    }

    #[cfg(not(target_os = "windows"))]
    {
        let _ = (window, level);
    }
}


