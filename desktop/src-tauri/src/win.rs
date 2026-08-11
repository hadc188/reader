//! Platform helpers used outside the webview.

#[cfg(target_os = "windows")]
mod imp {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        MessageBoxW, MB_ICONERROR, MB_ICONWARNING, MB_OK, SW_SHOWNORMAL,
    };
    use windows_sys::Win32::UI::Shell::ShellExecuteW;

    fn wide(value: &str) -> Vec<u16> {
        value.encode_utf16().chain(std::iter::once(0)).collect()
    }

    fn message_box(message: &str, style: u32) {
        let text = wide(message);
        let caption = wide("阅读");
        // SAFETY: both buffers are NUL-terminated and outlive the call.
        unsafe {
            MessageBoxW(
                std::ptr::null_mut(),
                text.as_ptr(),
                caption.as_ptr(),
                style,
            );
        }
    }

    pub fn fatal(message: &str) {
        message_box(message, MB_OK | MB_ICONERROR);
    }

    pub fn warn(message: &str) {
        message_box(message, MB_OK | MB_ICONWARNING);
    }

    pub fn open_external(url: &str) {
        let operation = wide("open");
        let target = wide(url);
        // SAFETY: both strings are NUL-terminated, and null optional arguments
        // ask Windows to use the registered default browser.
        unsafe {
            ShellExecuteW(
                std::ptr::null_mut(),
                operation.as_ptr(),
                target.as_ptr(),
                std::ptr::null(),
                std::ptr::null(),
                SW_SHOWNORMAL,
            );
        }
    }
}

#[cfg(not(target_os = "windows"))]
mod imp {
    pub fn fatal(message: &str) {
        eprintln!("阅读启动失败：{message}");
    }

    pub fn warn(message: &str) {
        eprintln!("阅读：{message}");
    }

    pub fn open_external(url: &str) {
        let command = if cfg!(target_os = "macos") {
            "open"
        } else {
            "xdg-open"
        };
        let _ = std::process::Command::new(command).arg(url).spawn();
    }
}

pub use imp::{fatal, open_external, warn};
