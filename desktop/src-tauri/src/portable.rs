//! Portable-mode path resolution.
//!
//! Release builds keep everything next to the executable so the whole app can be
//! moved between machines by copying one folder. Debug builds instead use a
//! fixed location in the source tree.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{anyhow, Context};

pub struct Paths {
    pub data_dir: PathBuf,
    pub storage: PathBuf,
    pub assets: PathBuf,
    pub db: PathBuf,
    /// Set the first time data lands outside the executable's directory because
    /// that directory was read-only.
    pub relocated_notice: bool,
}

pub fn resolve() -> anyhow::Result<Paths> {
    let (data_dir, relocated_notice) = data_dir()?;
    let storage = data_dir.join("storage");
    let assets = storage.join("assets");
    fs::create_dir_all(&assets)
        .with_context(|| format!("创建数据目录失败: {}", assets.display()))?;

    Ok(Paths {
        db: data_dir.join("reader.db"),
        data_dir,
        storage,
        assets,
        relocated_notice,
    })
}

/// Returns the data directory and whether the user should be told it moved.
fn data_dir() -> anyhow::Result<(PathBuf, bool)> {
    if cfg!(debug_assertions) {
        return Ok((dev_root().join(".dev-data"), false));
    }

    let portable = exe_dir()?.join("data");
    if is_writable(&portable) {
        return Ok((portable, false));
    }

    let fallback = fallback_data_dir()?.join("reader").join("data");
    let first_time = !fallback.exists();
    Ok((fallback, first_time))
}

fn is_writable(dir: &Path) -> bool {
    if fs::create_dir_all(dir).is_err() {
        return false;
    }
    let probe = dir.join(".write-test");
    let Ok(mut file) = fs::File::create(&probe) else {
        return false;
    };
    let ok = file.write_all(b"ok").is_ok();
    drop(file);
    let _ = fs::remove_file(&probe);
    ok
}

fn exe_dir() -> anyhow::Result<PathBuf> {
    let exe = std::env::current_exe().context("无法定位可执行文件路径")?;
    exe.parent()
        .map(Path::to_path_buf)
        .ok_or_else(|| anyhow!("可执行文件没有上级目录"))
}

fn env_path(name: &str) -> Option<PathBuf> {
    std::env::var_os(name)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

#[cfg(target_os = "windows")]
fn fallback_data_dir() -> anyhow::Result<PathBuf> {
    env_path("LOCALAPPDATA")
        .or_else(|| env_path("USERPROFILE").map(|path| path.join("AppData").join("Local")))
        .or_else(|| env_path("APPDATA"))
        .ok_or_else(|| anyhow!("无法定位 Windows 用户数据目录"))
}

#[cfg(target_os = "macos")]
fn fallback_data_dir() -> anyhow::Result<PathBuf> {
    env_path("HOME")
        .map(|path| path.join("Library").join("Application Support"))
        .ok_or_else(|| anyhow!("无法定位 macOS 用户数据目录"))
}

#[cfg(all(not(target_os = "windows"), not(target_os = "macos")))]
fn fallback_data_dir() -> anyhow::Result<PathBuf> {
    if let Some(path) = env_path("XDG_DATA_HOME").filter(|path| path.is_absolute()) {
        return Ok(path);
    }
    env_path("HOME")
        .map(|path| path.join(".local").join("share"))
        .ok_or_else(|| anyhow!("无法定位 Linux 用户数据目录"))
}

/// `desktop/` in the source tree. Only meaningful for debug builds.
fn dev_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..")
}
