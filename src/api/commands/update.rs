use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::Deserialize;
use serde::Serialize;
use serde_json::Value;

use crate::api::AppState;
use crate::error::error::{ApiResponse, AppError};
use crate::service::update_service::GithubAsset;

#[derive(Debug, Deserialize)]
pub struct VersionUpdateQuery {
    pub force: Option<bool>,
}

#[derive(Debug, Deserialize)]
pub struct DismissVersionUpdateRequest {
    pub version: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DesktopUpdateResult {
    mode: String,
    platform: String,
    asset_name: String,
    message: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DesktopUpdateProgress {
    stage: &'static str,
    percent: Option<u8>,
    downloaded: u64,
    total: u64,
    message: String,
}

#[tauri::command]
pub async fn get_version_update(
    state: tauri::State<'_, AppState>,
    req: VersionUpdateQuery,
) -> Result<ApiResponse<Value>, AppError> {
    let info = state
        .update_service
        .check(req.force.unwrap_or(false))
        .await?;
    Ok(ApiResponse::ok(
        serde_json::to_value(info).map_err(|err| AppError::BadRequest(err.to_string()))?,
    ))
}

#[tauri::command]
pub async fn dismiss_version_update(
    state: tauri::State<'_, AppState>,
    req: DismissVersionUpdateRequest,
) -> Result<ApiResponse<Value>, AppError> {
    let version = req.version.unwrap_or_default();
    let info = state.update_service.dismiss(&version).await?;
    Ok(ApiResponse::ok(
        serde_json::to_value(info).map_err(|err| AppError::BadRequest(err.to_string()))?,
    ))
}

#[tauri::command]
pub async fn apply_desktop_update(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    on_event: tauri::ipc::Channel<DesktopUpdateProgress>,
) -> Result<ApiResponse<DesktopUpdateResult>, AppError> {
    if cfg!(debug_assertions) {
        return Err(AppError::BadRequest(
            "开发模式不能执行自动更新，请使用发布版测试".to_string(),
        ));
    }

    send_progress(&on_event, "checking", Some(0), 0, 0, "正在确认最新版本");
    let info = match state.update_service.check(false).await {
        Ok(info) => info,
        Err(error) => {
            send_progress(&on_event, "failed", None, 0, 0, "检查更新失败");
            return Err(error);
        }
    };
    if !info.update_available {
        send_progress(&on_event, "failed", None, 0, 0, "当前已经是最新版本");
        return Err(AppError::BadRequest("当前已经是最新版本".to_string()));
    }

    let mode = installation_mode();
    let platform = current_platform();
    let asset = select_update_asset(&info.assets, platform, mode).ok_or_else(|| {
        let message = format!(
            "发行版中没有适用于当前系统的更新文件（{}，{}）",
            platform_label(platform),
            mode_label(mode)
        );
        send_progress(&on_event, "failed", None, 0, 0, &message);
        AppError::BadRequest(message)
    })?;
    let (temporary_path, package_path) = update_package_paths(asset)?;
    let mut last_percent = None;
    let mut last_reported_bytes = 0_u64;
    let download_result = state
        .update_service
        .download_asset_to_path(asset, &temporary_path, |downloaded, total| {
            let percent = (total > 0).then(|| {
                ((downloaded.saturating_mul(100) / total).min(100)) as u8
            });
            let should_report = percent != last_percent
                || downloaded.saturating_sub(last_reported_bytes) >= 1024 * 1024;
            if should_report {
                last_percent = percent;
                last_reported_bytes = downloaded;
                send_progress(
                    &on_event,
                    "downloading",
                    percent,
                    downloaded,
                    total,
                    "正在下载更新文件",
                );
            }
        })
        .await;
    let downloaded = match download_result {
        Ok(downloaded) => downloaded,
        Err(error) => {
            cleanup_file(&temporary_path);
            send_progress(&on_event, "failed", None, 0, 0, "更新文件下载失败");
            return Err(error);
        }
    };

    send_progress(
        &on_event,
        "verifying",
        Some(100),
        downloaded,
        asset.size,
        "正在校验更新文件",
    );
    if let Err(error) = validate_update_package(asset, &temporary_path, downloaded) {
        cleanup_file(&temporary_path);
        send_progress(
            &on_event,
            "failed",
            None,
            downloaded,
            asset.size,
            "更新文件校验失败",
        );
        return Err(error);
    }
    if package_path.exists() {
        cleanup_file(&package_path);
    }
    if let Err(error) = fs::rename(&temporary_path, &package_path) {
        cleanup_file(&temporary_path);
        send_progress(
            &on_event,
            "failed",
            None,
            downloaded,
            asset.size,
            "保存更新文件失败",
        );
        return Err(AppError::BadRequest(format!("保存更新文件失败: {error}")));
    }
    send_progress(
        &on_event,
        "ready",
        Some(100),
        downloaded,
        asset.size,
        "更新文件已准备完成",
    );

    #[cfg(target_os = "windows")]
    {
        if let Err(error) = launch_windows_update(&app, &package_path, mode) {
            cleanup_file(&package_path);
            send_progress(
                &on_event,
                "failed",
                None,
                downloaded,
                asset.size,
                "启动更新失败",
            );
            return Err(error);
        }
        return Ok(ApiResponse::ok(DesktopUpdateResult {
            mode: mode.to_string(),
            platform: platform.to_string(),
            asset_name: asset.name.clone(),
            message: "更新已下载，应用将退出并开始更新".to_string(),
        }));
    }

    #[cfg(not(target_os = "windows"))]
    {
        if let Err(error) = open_update_package(&package_path) {
            cleanup_file(&package_path);
            send_progress(
                &on_event,
                "failed",
                None,
                downloaded,
                asset.size,
                "打开更新文件失败",
            );
            return Err(error);
        }
        Ok(ApiResponse::ok(DesktopUpdateResult {
            mode: mode.to_string(),
            platform: platform.to_string(),
            asset_name: asset.name.clone(),
            message: "更新文件已下载并打开，请按系统提示完成安装".to_string(),
        }))
    }
}

fn send_progress(
    channel: &tauri::ipc::Channel<DesktopUpdateProgress>,
    stage: &'static str,
    percent: Option<u8>,
    downloaded: u64,
    total: u64,
    message: &str,
) {
    let _ = channel.send(DesktopUpdateProgress {
        stage,
        percent,
        downloaded,
        total,
        message: message.to_string(),
    });
}

fn current_platform() -> &'static str {
    if cfg!(target_os = "windows") {
        "windows"
    } else if cfg!(target_os = "macos") {
        "macos"
    } else {
        "linux"
    }
}

fn installation_mode() -> &'static str {
    #[cfg(target_os = "windows")]
    {
        let exe = std::env::current_exe().ok();
        let portable_marker = exe
            .as_ref()
            .and_then(|path| path.parent())
            .map(|dir| dir.join(".reader-portable").is_file())
            .unwrap_or(false);
        let installed_location = exe
            .as_ref()
            .and_then(|path| path.parent())
            .map(|dir| {
                dir.join("uninstall.exe").is_file()
                    || dir.join("Uninstall.exe").is_file()
                    || dir.join("unins000.exe").is_file()
                    || dir
                        .to_string_lossy()
                        .to_ascii_lowercase()
                        .contains("\\program files\\")
                    || dir
                        .to_string_lossy()
                        .to_ascii_lowercase()
                        .contains("\\appdata\\local\\programs\\")
            })
            .unwrap_or(false);
        let portable_name = exe
            .as_ref()
            .and_then(|path| path.file_name())
            .and_then(|name| name.to_str())
            .map(|name| name.eq_ignore_ascii_case("Reader.exe"))
            .unwrap_or(false);
        if portable_marker || (portable_name && !installed_location) {
            return "portable";
        }
    }

    #[cfg(target_os = "linux")]
    if std::env::var_os("APPIMAGE").is_some() {
        return "portable";
    }

    "installer"
}

fn select_update_asset<'a>(
    assets: &'a [GithubAsset],
    platform: &str,
    mode: &str,
) -> Option<&'a GithubAsset> {
    let suffix = match (platform, mode) {
        ("windows", "portable") => "windows-x64-portable.zip",
        ("windows", _) => "windows-x64-setup.exe",
        ("macos", "portable") => "macos-universal.zip",
        ("macos", _) => "macos-universal.dmg",
        ("linux", "portable") => "linux-x64.appimage",
        ("linux", _) => "linux-x64.deb",
        _ => return None,
    };

    assets
        .iter()
        .find(|asset| asset.name.to_ascii_lowercase().ends_with(suffix))
}

fn update_package_paths(asset: &GithubAsset) -> Result<(PathBuf, PathBuf), AppError> {
    if asset.name.is_empty()
        || !asset
            .name
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | '-' | '_'))
    {
        return Err(AppError::BadRequest("更新文件名无效".to_string()));
    }
    let update_dir = std::env::temp_dir().join("reader-updates");
    fs::create_dir_all(&update_dir)
        .map_err(|error| AppError::BadRequest(format!("创建更新目录失败: {error}")))?;
    let path = update_dir.join(&asset.name);
    let temporary = update_dir.join(format!("{}.{}.part", asset.name, std::process::id()));
    cleanup_file(&temporary);
    Ok((temporary, path))
}

fn validate_update_package(
    asset: &GithubAsset,
    path: &Path,
    downloaded: u64,
) -> Result<(), AppError> {
    let metadata = fs::metadata(path)
        .map_err(|error| AppError::BadRequest(format!("读取更新文件失败: {error}")))?;
    if !metadata.is_file() || metadata.len() == 0 || metadata.len() != downloaded {
        return Err(AppError::BadRequest("更新文件不完整，已取消更新".to_string()));
    }
    if asset.size > 0 && metadata.len() != asset.size {
        return Err(AppError::BadRequest(format!(
            "更新文件大小校验失败（应为 {} 字节，实际为 {} 字节）",
            asset.size,
            metadata.len()
        )));
    }

    let lower_name = asset.name.to_ascii_lowercase();
    let valid_header = validate_file_signature(&lower_name, path, metadata.len())?;
    if !valid_header {
        return Err(AppError::BadRequest(
            "更新文件格式校验失败，已取消更新".to_string(),
        ));
    }

    if lower_name.ends_with("windows-x64-portable.zip") {
        validate_windows_portable_archive(path)?;
    }
    Ok(())
}

fn validate_file_signature(name: &str, path: &Path, length: u64) -> Result<bool, AppError> {
    use std::io::{Read, Seek, SeekFrom};

    let mut file = fs::File::open(path)
        .map_err(|error| AppError::BadRequest(format!("读取更新文件失败: {error}")))?;
    let mut header = [0_u8; 64];
    let header_len = file
        .read(&mut header)
        .map_err(|error| AppError::BadRequest(format!("读取更新文件失败: {error}")))?;

    if name.ends_with(".exe") {
        if header_len < 64 || &header[..2] != b"MZ" {
            return Ok(false);
        }
        let pe_offset = u32::from_le_bytes(header[60..64].try_into().unwrap()) as u64;
        if pe_offset > length.saturating_sub(4) {
            return Ok(false);
        }
        file.seek(SeekFrom::Start(pe_offset))
            .map_err(|error| AppError::BadRequest(format!("读取更新文件失败: {error}")))?;
        let mut signature = [0_u8; 4];
        return Ok(file.read_exact(&mut signature).is_ok() && &signature == b"PE\0\0");
    }
    if name.ends_with(".zip") {
        return Ok(zip::ZipArchive::new(file).is_ok());
    }
    if name.ends_with(".deb") {
        return Ok(header_len >= 8 && &header[..8] == b"!<arch>\n");
    }
    if name.ends_with(".appimage") {
        return Ok(header_len >= 10 && &header[..4] == b"\x7fELF" && &header[8..10] == b"AI");
    }
    if name.ends_with(".dmg") {
        if length < 512 {
            return Ok(false);
        }
        file.seek(SeekFrom::End(-512))
            .map_err(|error| AppError::BadRequest(format!("读取更新文件失败: {error}")))?;
        let mut trailer = [0_u8; 4];
        return Ok(file.read_exact(&mut trailer).is_ok() && &trailer == b"koly");
    }
    Ok(false)
}

fn validate_windows_portable_archive(path: &Path) -> Result<(), AppError> {
    let file = fs::File::open(path)
        .map_err(|error| AppError::BadRequest(format!("读取便携版更新包失败: {error}")))?;
    let mut archive = zip::ZipArchive::new(file)
        .map_err(|error| AppError::BadRequest(format!("便携版更新包损坏: {error}")))?;
    let mut has_executable = false;
    let mut has_marker = false;
    for index in 0..archive.len() {
        let entry = archive
            .by_index(index)
            .map_err(|error| AppError::BadRequest(format!("便携版更新包损坏: {error}")))?;
        let Some(path) = entry.enclosed_name() else {
            return Err(AppError::BadRequest("便携版更新包包含不安全路径".to_string()));
        };
        let normalized = path.to_string_lossy().replace('\\', "/");
        let normalized = normalized.trim_start_matches("./");
        has_executable |= normalized.eq_ignore_ascii_case("Reader.exe");
        has_marker |= normalized.eq_ignore_ascii_case(".reader-portable");
    }
    if !has_executable || !has_marker {
        return Err(AppError::BadRequest(
            "便携版更新包缺少程序文件或便携标记".to_string(),
        ));
    }
    Ok(())
}

fn cleanup_file(path: &Path) {
    if path.is_file() {
        let _ = fs::remove_file(path);
    }
}

#[cfg(target_os = "windows")]
fn launch_windows_update(
    app: &tauri::AppHandle,
    package_path: &Path,
    mode: &str,
) -> Result<(), AppError> {
    use std::os::windows::process::CommandExt;

    let current_exe = std::env::current_exe()
        .map_err(|error| AppError::BadRequest(format!("无法定位当前程序: {error}")))?;
    let target_dir = current_exe
        .parent()
        .ok_or_else(|| AppError::BadRequest("无法定位程序目录".to_string()))?;
    let script_path = package_path.with_extension("update.ps1");
    let package = powershell_literal(package_path);
    let executable = powershell_literal(&current_exe);
    let script = if mode == "portable" {
        let staging = powershell_literal(
            &target_dir.join(format!(".reader-update-stage-{}", std::process::id())),
        );
        let backup = powershell_literal(
            &target_dir.join(format!(".reader-update-backup-{}.exe", std::process::id())),
        );
        format!(
            "$ErrorActionPreference = 'Stop'\n\
             Wait-Process -Id {} -ErrorAction SilentlyContinue\n\
             $staging = {}\n\
             $backup = {}\n\
             $installed = $false\n\
             $restored = $false\n\
             try {{\n\
               Remove-Item -LiteralPath $staging -Recurse -Force -ErrorAction SilentlyContinue\n\
               New-Item -ItemType Directory -Path $staging -Force | Out-Null\n\
               Expand-Archive -LiteralPath {} -DestinationPath $staging -Force\n\
               $newExe = Join-Path $staging 'Reader.exe'\n\
               $marker = Join-Path $staging '.reader-portable'\n\
               if (!(Test-Path -LiteralPath $newExe -PathType Leaf) -or !(Test-Path -LiteralPath $marker -PathType Leaf)) {{ throw '更新包内容不完整' }}\n\
               Move-Item -LiteralPath {} -Destination $backup -Force\n\
               Copy-Item -LiteralPath $newExe -Destination {} -Force\n\
               Copy-Item -LiteralPath $marker -Destination (Join-Path {} '.reader-portable') -Force\n\
               $newProcess = Start-Process -FilePath {} -PassThru\n\
               Start-Sleep -Seconds 3\n\
               if ($newProcess.HasExited) {{ throw '新版启动失败' }}\n\
               $installed = $true\n\
             }} catch {{\n\
               if (Test-Path -LiteralPath $backup -PathType Leaf) {{\n\
                 Get-Process -Name 'Reader','reader-desktop' -ErrorAction SilentlyContinue | Stop-Process -Force -ErrorAction SilentlyContinue\n\
                 Copy-Item -LiteralPath $backup -Destination {} -Force\n\
                 Start-Process -FilePath {} -ErrorAction SilentlyContinue\n\
                 $restored = $true\n\
               }}\n\
             }} finally {{\n\
               Remove-Item -LiteralPath $staging -Recurse -Force -ErrorAction SilentlyContinue\n\
               if ($installed -or $restored) {{ Remove-Item -LiteralPath $backup -Force -ErrorAction SilentlyContinue }}\n\
               Remove-Item -LiteralPath {} -Force -ErrorAction SilentlyContinue\n\
             }}\n\
             Remove-Item -LiteralPath $PSCommandPath -Force -ErrorAction SilentlyContinue\n",
            std::process::id(),
            staging,
            backup,
            package,
            executable,
            executable,
            powershell_literal(target_dir),
            executable,
            executable,
            executable,
            package
        )
    } else {
        format!(
            "$ErrorActionPreference = 'Stop'\n\
             Wait-Process -Id {} -ErrorAction SilentlyContinue\n\
             $installer = Start-Process -FilePath {} -PassThru\n\
             $installer.WaitForExit()\n\
             Remove-Item -LiteralPath {} -Force -ErrorAction SilentlyContinue\n\
             Remove-Item -LiteralPath $PSCommandPath -Force -ErrorAction SilentlyContinue\n",
            std::process::id(),
            package,
            package
        )
    };
    fs::write(&script_path, script)
        .map_err(|error| AppError::BadRequest(format!("创建更新脚本失败: {error}")))?;

    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let launch_result = Command::new("powershell.exe")
        .args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-File"])
        .arg(&script_path)
        .creation_flags(CREATE_NO_WINDOW)
        .spawn();
    if let Err(error) = launch_result {
        cleanup_file(&script_path);
        return Err(AppError::BadRequest(format!("启动更新程序失败: {error}")));
    }
    app.exit(0);
    Ok(())
}

#[cfg(target_os = "windows")]
fn powershell_literal(path: &Path) -> String {
    format!("'{}'", path.to_string_lossy().replace('\'', "''"))
}

#[cfg(target_os = "macos")]
fn open_update_package(path: &Path) -> Result<(), AppError> {
    Command::new("open")
        .arg(path)
        .spawn()
        .map(|_| ())
        .map_err(|error| AppError::BadRequest(format!("打开更新文件失败: {error}")))
}

#[cfg(all(not(target_os = "windows"), not(target_os = "macos")))]
fn open_update_package(path: &Path) -> Result<(), AppError> {
    Command::new("xdg-open")
        .arg(path)
        .spawn()
        .map(|_| ())
        .map_err(|error| AppError::BadRequest(format!("打开更新文件失败: {error}")))
}

fn platform_label(platform: &str) -> &'static str {
    match platform {
        "windows" => "Windows",
        "macos" => "macOS",
        _ => "Linux",
    }
}

fn mode_label(mode: &str) -> &'static str {
    if mode == "portable" {
        "便携版"
    } else {
        "安装包版"
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::{select_update_asset, validate_update_package};
    use crate::service::update_service::GithubAsset;

    fn asset(name: &str) -> GithubAsset {
        GithubAsset {
            name: name.to_string(),
            browser_download_url: format!("https://github.com/example/{name}"),
            size: 1,
        }
    }

    #[test]
    fn selects_windows_package_by_installation_mode() {
        let assets = vec![
            asset("Reader-v2-windows-x64-setup.exe"),
            asset("Reader-v2-windows-x64-portable.zip"),
        ];
        assert!(select_update_asset(&assets, "windows", "installer")
            .unwrap()
            .name
            .ends_with("setup.exe"));
        assert!(select_update_asset(&assets, "windows", "portable")
            .unwrap()
            .name
            .ends_with("portable.zip"));
    }

    #[test]
    fn rejects_update_package_when_release_size_does_not_match() {
        let path = std::env::temp_dir().join(format!(
            "reader-invalid-update-{}.exe",
            std::process::id()
        ));
        fs::write(&path, b"MZpayload").unwrap();
        let mut release_asset = asset("Reader-v2-windows-x64-setup.exe");
        release_asset.size = 99;

        let error = validate_update_package(&release_asset, &path, 9).unwrap_err();
        let _ = fs::remove_file(path);

        assert!(error.to_string().contains("大小校验失败"));
    }

    #[test]
    fn rejects_executable_without_pe_header() {
        let path = std::env::temp_dir().join(format!(
            "reader-invalid-header-{}.exe",
            std::process::id()
        ));
        fs::write(&path, b"not-an-exe").unwrap();
        let mut release_asset = asset("Reader-v2-windows-x64-setup.exe");
        release_asset.size = 10;

        let error = validate_update_package(&release_asset, &path, 10).unwrap_err();
        let _ = fs::remove_file(path);

        assert!(error.to_string().contains("格式校验失败"));
    }
}
