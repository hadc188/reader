//! Win32 helpers for the two things needed outside a webview: a blocking error
//! dialog (WebView2 may be missing, so there is no window to render into) and
//! handing a URL to the user's default browser.

use windows_sys::Win32::UI::WindowsAndMessaging::{
    MessageBoxW, MB_ICONERROR, MB_ICONWARNING, MB_OK,
};

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

fn message_box(message: &str, style: u32) {
    let text = wide(message);
    let caption = wide("阅读");
    // SAFETY: both buffers are NUL-terminated and outlive the call, and a null
    // owner HWND is valid for an ownerless dialog.
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

/// Open `url` in the default browser.
///
/// `explorer.exe` takes the URL as a single argument, which avoids the quoting
/// pitfalls of `cmd /c start` with query strings containing `&`.
pub fn open_external(url: &str) {
    let _ = std::process::Command::new("explorer.exe").arg(url).spawn();
}
