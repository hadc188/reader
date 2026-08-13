use tauri::Manager;
use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};

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
                } else {
                    let _ = window.show();
                    let _ = window.set_focus();
                }
            }
        })
        .map_err(|error| format!("快捷键注册失败：{error}"))
}
