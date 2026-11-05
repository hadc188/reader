//! Portable-mode path resolution.
//!
//! 桌面版的默认形态是便携的: release 构建把数据放在可执行文件旁边, 整个文件夹
//! 拷走就能带走全部状态。**macOS 例外** —— 可执行文件位于
//! `Reader.app/Contents/MacOS/`, 而 DMG / app-zip 的升级方式就是整体替换
//! `Reader.app`, 数据放在包内会随升级一起被删(issue #32), 因此那边固定使用
//! `~/Library/Application Support/reader/data`。调试构建统一用源码树内的固定
//! 位置。

#[cfg(any(target_os = "macos", test))]
use std::ffi::OsStr;
use std::fs;
#[cfg(not(target_os = "macos"))]
use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{anyhow, Context};

/// 旧版所有副本共用的 bundle identifier。
///
/// 它同时决定了单实例互斥量名和 WebView2 的数据目录, 于是「两个目录各放一份
/// 便携版」会互相打架: 第二个副本以为已有实例在跑, 启动后直接退出; 就算绕过,
/// 两份副本也共用同一份 localStorage —— 在 A 目录换的背景图会出现在 B 目录
/// (issue #20 第 4 条)。现在每个数据目录有自己的 identifier, 见
/// `instance_identifier`。
pub const LEGACY_IDENTIFIER: &str = "com.hadc188.reader";

pub struct Paths {
    pub data_dir: PathBuf,
    pub storage: PathBuf,
    pub assets: PathBuf,
    pub db: PathBuf,
    /// 窗口尺寸/位置状态。跟随便携数据目录, 而不是 Tauri 的 app config
    /// 目录 —— 后者会让状态在整份拷贝移动到别的机器后错位。
    pub window_state: PathBuf,
    /// WebView2 的用户数据目录(localStorage / IndexedDB / 缓存)。
    ///
    /// 必须跟着数据目录走。WebView2 默认把 profile 放在
    /// `%LOCALAPPDATA%\<identifier>` 下, 与程序所在目录无关 —— 于是两个便携
    /// 副本共用一个 localStorage, 在 A 目录换的背景图会出现在 B 目录
    /// (issue #20 第 4 条)。
    pub webview_data: PathBuf,
    /// 本副本专属的 bundle identifier, 由数据目录派生。
    ///
    /// `tauri-plugin-single-instance` 用它做互斥量/窗口类名: 同一份数据目录
    /// (同一实例)只能开一个进程, 不同目录(不同副本)可以同时开。
    pub instance_identifier: String,
    /// Set the first time data lands outside the executable's directory because
    /// that directory was read-only.
    pub relocated_notice: bool,
    /// 旧版 profile 被占用、本次没能迁移时留下的说明文件; 有值就该告诉用户。
    pub webview_migration_skipped: Option<PathBuf>,
}

pub fn resolve() -> anyhow::Result<Paths> {
    let (data_dir, relocated_notice) = data_dir()?;
    let storage = data_dir.join("storage");
    let assets = storage.join("assets");
    fs::create_dir_all(&assets)
        .with_context(|| format!("创建数据目录失败: {}", assets.display()))?;

    let webview_data = data_dir.join("webview");
    // 调试构建用 `.dev-data`, 不该把正式版的 localStorage 拷进开发环境。
    let webview_migration_skipped = if cfg!(debug_assertions) {
        None
    } else {
        migrate_legacy_webview_storage(&webview_data)
    };

    Ok(Paths {
        instance_identifier: instance_identifier(&data_dir),
        db: data_dir.join("reader.db"),
        window_state: data_dir.join("window-state.json"),
        webview_data,
        data_dir,
        storage,
        assets,
        relocated_notice,
        webview_migration_skipped,
    })
}

/// 由数据目录派生实例 identifier。
///
/// 语义就是"数据目录相同 = 同一个实例": 同一目录的第二份 exe 会撞上同一个
/// 互斥量并让位给已在运行的那个(避免两个进程同时写同一个 SQLite), 而不同
/// 目录的副本互不相干。
///
/// identifier 只允许 `[A-Za-z0-9-.]`, 因此用 FNV-1a 的十六进制摘要而不是
/// 直接把路径拼进去。Windows 路径大小写不敏感, 先统一小写, 否则 `C:\A` 与
/// `c:\a` 会被当成两个实例。
///
/// 只有 Windows 需要 —— 多副本同时运行是 Windows 便携包(zip)的用法, 而
/// macOS / Linux 改 identifier 会把浏览器存储挪到新位置, 老用户升级后偏好
/// 白白重置一次。那边保持原 identifier 不变。
pub fn instance_identifier(data_dir: &Path) -> String {
    #[cfg(not(target_os = "windows"))]
    {
        let _ = data_dir;
        LEGACY_IDENTIFIER.to_string()
    }

    #[cfg(target_os = "windows")]
    {
        let normalized = data_dir.to_string_lossy().to_lowercase();
        format!(
            "{LEGACY_IDENTIFIER}.instance.{}",
            fnv1a_hex(normalized.as_bytes())
        )
    }
}

#[cfg(target_os = "windows")]
fn fnv1a_hex(bytes: &[u8]) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{hash:016x}")
}

/// Returns the data directory and whether the user should be told it moved.
fn data_dir() -> anyhow::Result<(PathBuf, bool)> {
    if cfg!(debug_assertions) {
        return Ok((dev_root().join(".dev-data"), false));
    }
    release_data_dir()
}

/// macOS: 数据固定放在系统用户目录, 绝不放应用包内。
///
/// 可执行文件位于 `X.app/Contents/MacOS/`, 而 DMG / app-zip 的升级方式就是
/// 整体替换 `X.app`。旧版按「exe 同级 `data/`」落盘, 于是书库(`reader.db`)、
/// 书源与本地备份(`storage/`)全都住进了应用包, 每次升级被连包一起删掉
/// (issue #32)。首次启动时把旧版留在包内的那份数据搬出来。
#[cfg(target_os = "macos")]
fn release_data_dir() -> anyhow::Result<(PathBuf, bool)> {
    let fallback = fallback_data_dir()?.join("reader").join("data");
    migrate_data_out_of_app_bundle(&fallback);
    // 数据本来就在用户目录, 不存在"安装目录不可写"这回事, 不必提示迁移。
    Ok((fallback, false))
}

/// 其他平台: 优先 exe 同级 `data/`(Windows 便携包 / Linux AppImage), 目录
/// 不可写时回退到系统用户目录。
///
/// Linux 不需要 macOS 那样的迁出逻辑, 原因是安装位置本身就不可写:
///
/// - **AppImage**: AppImage 运行时把 squashfs 挂到 `$TMPDIR/.mount_*`, 只读。
///   `exe_dir()` 解析到那里面, `is_writable()` 必然失败, 于是直接走
///   `$XDG_DATA_HOME/reader/data`(或 `~/.local/share/reader/data`)。数据从来
///   就没进过 AppImage 文件本身 —— 换一个新 AppImage 覆盖旧文件, 数据不受影响。
/// - **DEB**: 装到 `/usr/bin`, 普通用户不可写, 同样落到 `XDG_DATA_HOME`。
///
/// 换言之 Linux 靠 `is_writable()` 兜住了, 而 macOS 的 `.app` 在用户目录下
/// (下载目录 / 桌面 / `/Applications` 均可写), 探针会成功 —— 这才是 #32 只在
/// macOS 出现的原因。**不要**为了"统一"把 Linux 改成固定用户目录: AppImage 的
/// 便携性是特性, 解压到 U 盘也能带着数据走。
#[cfg(not(target_os = "macos"))]
fn release_data_dir() -> anyhow::Result<(PathBuf, bool)> {
    let portable = exe_dir()?.join("data");
    if is_writable(&portable) {
        cleanup_legacy_fallback_data(&portable);
        return Ok((portable, false));
    }

    let fallback = fallback_data_dir()?.join("reader").join("data");
    let first_time = !fallback.exists();
    Ok((fallback, first_time))
}

/// 可执行文件位于 macOS 应用包内时, 返回旧版写在包里的数据目录。
///
/// 形如 `…/Reader.app/Contents/MacOS/reader-desktop` →
/// `…/Reader.app/Contents/MacOS/data`。判定只看路径形态, 不碰文件系统, 因此
/// 在任意平台上都能单测。
#[cfg(any(target_os = "macos", test))]
fn bundled_data_dir(exe: &Path) -> Option<PathBuf> {
    let macos_dir = exe.parent()?;
    if macos_dir.file_name()? != OsStr::new("MacOS") {
        return None;
    }
    let contents = macos_dir.parent()?;
    if contents.file_name()? != OsStr::new("Contents") {
        return None;
    }
    let bundle = contents.parent()?;
    if bundle.extension()? != OsStr::new("app") {
        return None;
    }
    Some(macos_dir.join("data"))
}

/// 把旧版留在应用包内的数据搬到用户目录。
///
/// **只复制, 不删除旧目录** —— 在 `.app` 里增删任何文件都会让代码签名失效,
/// macOS 随后会直接拒绝启动(「应用已损坏」)。旧目录随下次整包替换自然消失,
/// 不值得为省几百 MB 冒这个风险。
///
/// 成功后写一个标记: 否则用户在新位置删掉的书记录会在下次启动时被包内的旧
/// 副本"复活", 看起来像是删不掉。
#[cfg(any(target_os = "macos", test))]
fn migrate_bundled_data(bundled: &Path, current: &Path) {
    if bundled == current || !bundled.is_dir() {
        return;
    }
    if current.join(APP_BUNDLE_MIGRATED_MARKER).is_file() {
        return;
    }
    match copy_bundled_data(bundled, current) {
        Ok(()) => {
            let _ = fs::create_dir_all(current);
            let _ = fs::write(current.join(APP_BUNDLE_MIGRATED_MARKER), "1");
            eprintln!(
                "阅读：已把旧版留在应用包内的数据迁移到：{}（旧目录原样保留，可手动删除）",
                current.display()
            );
        }
        Err(err) => {
            // 迁移失败不能阻断启动: 新数据目录照常创建, 用户还能继续用, 旧数据
            // 也一份没动, 下次启动会重试。
            eprintln!(
                "阅读：应用包内的旧数据迁移失败, 已原样保留并将在下次启动重试：{}：{err}",
                bundled.display()
            );
        }
    }
}

/// 复制旧数据, 但跳过 `storage/cache`。
///
/// 章节正文缓存可能有几个 GB, 而它在启动路径上是同步执行的 —— 全量拷贝会让
/// 用户对着白屏等上几十秒。缓存本身可重建(重新点开章节即可), 不属于"丢了就
/// 找不回来"的数据; 书库、书源、本地书原件、备份、登录态一律照搬。
#[cfg(any(target_os = "macos", test))]
fn copy_bundled_data(bundled: &Path, current: &Path) -> std::io::Result<()> {
    fs::create_dir_all(current)?;
    for entry in fs::read_dir(bundled)? {
        let entry = entry?;
        let name = entry.file_name();
        let target = current.join(&name);
        // 章节缓存在 `storage/cache`; 顶层同名目录只是防御性跳过。
        if name == OsStr::new("cache") {
            continue;
        }
        if name == OsStr::new("storage") {
            copy_dir_skipping_cache(&entry.path(), &target)?;
            continue;
        }
        if target.exists() {
            continue;
        }
        if entry.file_type()?.is_dir() {
            copy_legacy_data_if_missing(&entry.path(), &target)?;
        } else {
            fs::copy(entry.path(), &target)?;
        }
    }
    Ok(())
}

/// 递归复制 `storage/`, 跳过其中的 `cache` 子目录。
#[cfg(any(target_os = "macos", test))]
fn copy_dir_skipping_cache(source: &Path, destination: &Path) -> std::io::Result<()> {
    fs::create_dir_all(destination)?;
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        if entry.file_name() == OsStr::new("cache") {
            continue;
        }
        let target = destination.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir_skipping_cache(&entry.path(), &target)?;
        } else if !target.exists() {
            fs::copy(entry.path(), &target)?;
        }
    }
    Ok(())
}

#[cfg(target_os = "macos")]
fn migrate_data_out_of_app_bundle(current: &Path) {
    // 1) 就地替换 / 重装: 旧数据还在当前应用包里。
    if let Ok(exe) = std::env::current_exe() {
        if let Some(bundled) = bundled_data_dir(&exe) {
            migrate_bundled_data(&bundled, current);
        }
    }
    // 2) Finder 用新包"替换"旧包时, 旧包是被**移到废纸篓**而不是直接删掉的,
    //    里面往往就是用户升级前的全部书源、书架与本地备份 —— 正是 issue #32 里
    //    "升级后全没了"的那一份。只在目标位置还没有书库时去捞, 绝不用它覆盖
    //    用户已经在新位置产生的数据。
    if !current.join("reader.db").is_file() {
        if let Some(trashed) = latest_trashed_bundle_data() {
            migrate_bundled_data(&trashed, current);
        }
    }
}

/// 在废纸篓里找最近一个 Reader 应用包内的旧数据目录。
///
/// 不按应用名过滤: Finder 在重名覆盖时会生成 `Reader 2.app` 之类的名字。判据
/// 是结构 —— `*.app/Contents/MacOS/data/reader.db` —— 这个组合足够特异, 不会
/// 命中别的应用。
#[cfg(target_os = "macos")]
fn latest_trashed_bundle_data() -> Option<PathBuf> {
    let trash = env_path("HOME")?.join(".Trash");
    pick_latest_bundle_data(&trash)
}

/// 从 `trash` 下挑出书库最新的一份(按 `reader.db` 的修改时间)。
///
/// 纯文件系统遍历, 不涉及平台 API, 因此在任意平台上都能单测。
#[cfg(any(target_os = "macos", test))]
fn pick_latest_bundle_data(trash: &Path) -> Option<PathBuf> {
    let entries = fs::read_dir(trash).ok()?;
    let mut best: Option<(std::time::SystemTime, PathBuf)> = None;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension() != Some(OsStr::new("app")) {
            continue;
        }
        let data = path.join("Contents").join("MacOS").join("data");
        let Ok(meta) = fs::metadata(data.join("reader.db")) else {
            continue;
        };
        let modified = meta.modified().unwrap_or(std::time::UNIX_EPOCH);
        if best.as_ref().map(|(at, _)| modified > *at).unwrap_or(true) {
            best = Some((modified, data));
        }
    }
    best.map(|(_, path)| path)
}

/// 应用包内旧数据已经搬过一次的标记(见 `migrate_bundled_data`)。
#[cfg(any(target_os = "macos", test))]
const APP_BUNDLE_MIGRATED_MARKER: &str = ".app-bundle-data-migrated";

/// 早期版本在安装目录不可写时退回到 `%LOCALAPPDATA%\reader\data`。现在只要
/// 便携目录恢复可写，就把这份旧回退数据收回来，避免两个目录各自保存一份书库。
///
/// 迁移规则必须保守：
/// - 先把旧目录**独有的**文件合并进当前目录，同名文件一律保留当前版本；
/// - 合并失败就整体放弃，旧目录原样保留，下次启动重试；
/// - 只有确认旧目录里每个文件在当前目录都已有对应，才移除旧目录。
///
/// 这里绝不能"没有 `reader.db` 就整个删掉"：书库文件、导入的 EPUB 原件都可能
/// 只存在于旧目录里，数据库丢了不等于这些内容也该丢。
#[cfg(target_os = "windows")]
fn cleanup_legacy_fallback_data(current: &Path) {
    let Ok(local_app_data) = fallback_data_dir() else {
        return;
    };
    let legacy = local_app_data.join("reader").join("data");
    cleanup_legacy_data_dir(current, &legacy);
}

#[cfg(target_os = "windows")]
fn cleanup_legacy_data_dir(current: &Path, legacy: &Path) {
    if legacy == current || !legacy.is_dir() {
        return;
    }

    if copy_legacy_data_if_missing(legacy, current).is_err() {
        // 读/写失败时保留旧目录，不要用"删除"来掩盖失败。
        return;
    }

    if !legacy_is_redundant(legacy, current, current.join("reader.db").is_file()) {
        eprintln!(
            "阅读：旧数据目录仍有未能合并的内容，已原样保留：{}",
            legacy.display()
        );
        return;
    }

    let _ = fs::remove_dir_all(legacy);
    remove_empty_legacy_root(legacy);
}

#[cfg(all(not(target_os = "windows"), not(target_os = "macos")))]
fn cleanup_legacy_fallback_data(_current: &Path) {}

#[cfg(any(target_os = "windows", target_os = "macos", test))]
fn copy_legacy_data_if_missing(source: &Path, destination: &Path) -> std::io::Result<()> {
    fs::create_dir_all(destination)?;
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        let target = destination.join(entry.file_name());
        let file_type = entry.file_type()?;
        if file_type.is_dir() {
            // 目标目录已存在也必须继续下钻：早先在这里直接 `continue` 会跳过整棵
            // 子树，旧目录里独有的文件随后随目录删除一起消失。
            copy_legacy_data_if_missing(&entry.path(), &target)?;
        } else if file_type.is_file() {
            if target.exists() {
                continue;
            }
            fs::copy(entry.path(), &target)?;
        }
    }
    Ok(())
}

/// 旧目录里的每个文件是否都能在当前目录找到**同一份内容**。
///
/// 判据必须是内容而不是名字 + 长度：`storage/assets` 下放的是导入原件的原始
/// 文件名（不按内容寻址），同名等长但内容不同的文件完全可能，只比长度会把
/// "旧目录里唯一的那份原件"误判成冗余，随后随目录删除。
///
/// 唯一放宽的是**顶层数据库文件**：`reader.db` 的取舍规则是"当前库优先"，
/// 两边都有库时旧库本来就要被丢弃，内容不同属预期，不能因此把整个旧目录留下。
/// 那只适用于顶层，子目录里的同名文件仍须内容一致。
///
/// 文件数过多时会逐字节比较, 这是启动路径上的一次性成本; 迁移完成后旧目录
/// 被移除, 后续启动不再付出这个代价。
#[cfg(target_os = "windows")]
fn legacy_is_redundant(legacy: &Path, current: &Path, current_database_wins: bool) -> bool {
    let Ok(entries) = fs::read_dir(legacy) else {
        return false;
    };
    for entry in entries.flatten() {
        let Ok(file_type) = entry.file_type() else {
            return false;
        };
        let name = entry.file_name();
        let target = current.join(&name);
        if file_type.is_dir() {
            if !target.is_dir() || !legacy_is_redundant(&entry.path(), &target, false) {
                return false;
            }
            continue;
        }
        if !file_type.is_file() {
            continue;
        }
        if current_database_wins && is_database_file(&name) {
            // 当前库优先: 旧库只要确实被当前库(或迁移标记)取代即可丢弃。
            // SQLite 的 `-wal` / `-shm` 在新库 checkpoint 后可能不存在, 属正常。
            continue;
        }
        if !files_have_same_content(&entry.path(), &target) {
            return false;
        }
    }
    true
}

/// 两个文件内容是否完全一致(长度不同时直接判否, 避免无谓读取)。
#[cfg(target_os = "windows")]
fn files_have_same_content(left: &Path, right: &Path) -> bool {
    let Ok(left_meta) = fs::metadata(left) else {
        return false;
    };
    let Ok(right_meta) = fs::metadata(right) else {
        return false;
    };
    if !right_meta.is_file() || left_meta.len() != right_meta.len() {
        return false;
    }
    let (Ok(left_bytes), Ok(right_bytes)) = (fs::read(left), fs::read(right)) else {
        return false;
    };
    left_bytes == right_bytes
}

/// SQLite 会在数据库旁边留下 `-wal` / `-shm`, 三者同属"当前库优先"的范围。
#[cfg(target_os = "windows")]
fn is_database_file(name: &std::ffi::OsStr) -> bool {
    let Some(name) = name.to_str() else {
        return false;
    };
    matches!(name, "reader.db" | "reader.db-wal" | "reader.db-shm")
}

#[cfg(target_os = "windows")]
fn remove_empty_legacy_root(legacy: &Path) {
    if let Some(root) = legacy.parent() {
        if root.read_dir().map(|mut entries| entries.next().is_none()).unwrap_or(false) {
            let _ = fs::remove_dir(root);
        }
    }
}

#[cfg(not(target_os = "macos"))]
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

#[cfg(not(target_os = "macos"))]
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

/* ─── 旧版共享 WebView 数据迁移 ───
 *
 * 旧版把 WebView2 profile 固定放在 `%LOCALAPPDATA%\com.hadc188.reader\EBWebView`,
 * 与程序目录无关。改成本地 profile 后, 用户的阅读偏好、书架筛选、主题等全在
 * 那个旧目录的 localStorage 里; 不搬过来就会表现为"升级后所有设置被重置"。
 *
 * 迁移规则保守: 只在本地 profile 还没建立时搬一次, 搬完写标记文件, **不删旧目录**
 * (旧目录可能还被另一个副本用着)。删除旧目录留到以后的版本再考虑。 */

/// 旧版共享 profile 的根目录(Windows)。
#[cfg(target_os = "windows")]
fn legacy_webview_data_dir() -> Option<PathBuf> {
    fallback_data_dir()
        .ok()
        .map(|dir| dir.join(LEGACY_IDENTIFIER).join("EBWebView"))
}

#[cfg(not(target_os = "windows"))]
fn legacy_webview_data_dir() -> Option<PathBuf> {
    None
}

/// 把旧版共享 profile 里的 localStorage 搬到本副本的 profile 下。
///
/// 只搬 localStorage/IndexedDB 这类"用户数据", 不搬 Cache 与各种日志 —— 那些
/// 体积大且可以重建。整个复制是尽力而为: 失败就当作没有旧数据, 绝不能让启动
/// 因此失败。
fn migrate_legacy_webview_storage(webview_data: &Path) -> Option<PathBuf> {
    let legacy = legacy_webview_data_dir()?;
    copy_legacy_webview_storage_from(webview_data, &legacy)
}

/// 旧 profile 的 leveldb 锁状态。
enum LegacyProfileLock {
    /// 旧 profile 从没写过 localStorage: 没有会被写坏的东西, 直接迁。
    Absent,
    /// 拿到了锁: 迁移期间一直持有, 旧进程这时打不开 leveldb。
    Held(fs::File),
    /// 旧 profile 正被另一个进程用着。
    InUse,
}

/// 试着独占旧 profile 的 leveldb。
///
/// leveldb 在 Windows 上以 **不共享** 的方式打开 `LOCK`(CreateFileW 的
/// dwShareMode = 0), 所以这里也用不授予写共享的方式打开: 打不开就说明还有
/// 副本开着, 此时拷出来的是写了一半的 leveldb —— 校验失败, 表现为"升级后
/// 所有偏好被重置"。
///
/// 拿到的句柄必须**持有到迁移结束**, 而不是探测完就放掉: 否则探测与拷贝之间
/// 旧进程正好启动, 拷到的仍是脏数据(TOCTOU)。持有期间旧进程会打不开 leveldb,
/// 这正好把它挡在门外。
///
/// 共享模式只给 `FILE_SHARE_READ`: leveldb 要写 LOCK, 所以旧副本照样进不来;
/// 同时迁移自己还要把 LOCK 一起拷过去(`fs::copy` 会另开一个读句柄, 已有的
/// 句柄不允许读就会失败)。
///
/// 打不开一律保守地当成"在用"(包括权限不足等少见情况), 宁可跳过迁移。
#[cfg(target_os = "windows")]
fn lock_legacy_profile(legacy_eb: &Path) -> LegacyProfileLock {
    use std::os::windows::fs::OpenOptionsExt;

    const FILE_SHARE_READ: u32 = 0x1;

    let lock = legacy_eb
        .join("Default")
        .join("Local Storage")
        .join("leveldb")
        .join("LOCK");
    // 没有 leveldb 目录(旧 profile 从没写过 localStorage): 没有会被写坏的东西。
    if !lock.is_file() {
        return LegacyProfileLock::Absent;
    }
    match fs::OpenOptions::new()
        .read(true)
        .write(true)
        .share_mode(FILE_SHARE_READ)
        .open(&lock)
    {
        Ok(file) => LegacyProfileLock::Held(file),
        Err(_) => LegacyProfileLock::InUse,
    }
}

#[cfg(not(target_os = "windows"))]
fn lock_legacy_profile(_legacy_eb: &Path) -> LegacyProfileLock {
    LegacyProfileLock::Absent
}

/// 迁移的可测部分: 旧 profile(`EBWebView` 目录) → 本地 profile。
///
/// 返回值是"因旧 profile 被占用而放弃"时的提示文件路径, 供调用方决定要不要
/// 告诉用户; 其它情况(成功 / 没得迁 / 已经迁过)都返回 None。
///
/// 先完整拷到同卷的 `EBWebView.migrating`, 全部成功后再 rename 成 `EBWebView`。
/// 这样"拷到一半失败/被强杀"不会留下一个看起来已经迁移好的空骨架 —— 早先直接
/// 往 `EBWebView` 里写, 中途失败后下次启动会被"本地已有 Local Storage"跳过,
/// 于是永远拿不到那份旧数据。
///
/// 注意"被占用"**不会**在下次启动自动重试: 本次 WebView2 就会在 `<data>/webview`
/// 下建好自己的 profile, 之后这里一律按"本地已有数据"返回。所以那条路径必须
/// 明确告诉用户怎么手动补救, 而不是指望下次启动。
fn copy_legacy_webview_storage_from(webview_data: &Path, legacy_eb: &Path) -> Option<PathBuf> {
    if webview_data.join(".legacy-profile-migrated").is_file() {
        return None;
    }    // 本地 profile 已建立: 本地数据更新, 不再拿旧数据覆盖。
    if webview_data.join("EBWebView").exists() {
        return None;
    }
    if !legacy_eb.is_dir() {
        return None;
    }
    // 锁要一直持有到迁移结束(见 lock_legacy_profile 的说明): 绑到具名变量上,
    // 让它在函数返回前不会被提前 drop。
    let _legacy_lock = match lock_legacy_profile(legacy_eb) {
        LegacyProfileLock::Absent => None,
        LegacyProfileLock::Held(file) => Some(file),
        LegacyProfileLock::InUse => {
            // 迁移的机会只有这一次: WebView2 稍后就会建好本地 profile, 之后就一律
            // 按"本地已有数据"处理。所以不能静默放弃 —— 留一份说明, 告诉用户旧
            // 设置为什么没跟过来、以及怎么手动补。
            let _ = fs::create_dir_all(webview_data);
            let notice = webview_data.join("legacy-profile-skipped.txt");
            let _ = fs::write(
                &notice,
                format!(
                    "检测到旧版浏览器数据仍在使用中，本次未自动迁移。\n\n\
                     旧目录：{}\n\n\
                     如需迁移，请先完全退出所有旧版窗口，再删除本文件与同级的 \
                     EBWebView 目录后重新启动。\n",
                    legacy_eb.display()
                ),
            );
            eprintln!(
                "阅读：旧版 profile 正在使用中，跳过迁移：{}",
                legacy_eb.display()
            );
            return Some(notice);
        }
    };

    let staging = webview_data.join("EBWebView.migrating");
    // 上次迁移被强杀留下的临时目录: 内容不可信, 清掉重来。
    if staging.exists() {
        let _ = if staging.is_dir() {
            fs::remove_dir_all(&staging)
        } else {
            fs::remove_file(&staging)
        };
    }
    if fs::create_dir_all(&staging).is_err() {
        return None;
    }

    for relative in [
        Path::new("Default").join("Local Storage"),
        Path::new("Default").join("IndexedDB"),
        Path::new("Default").join("WebStorage"),
    ] {
        let source = legacy_eb.join(&relative);
        if !source.exists() {
            continue;
        }
        let destination = staging.join(&relative);
        let copied = destination
            .parent()
            .map(|parent| fs::create_dir_all(parent).is_ok())
            .unwrap_or(false)
            && copy_entry(&source, &destination).is_ok();
        if !copied {
            // 半份 profile 比没有 profile 更糟(leveldb 会把它当成损坏的库),
            // 整体放弃, 下次启动重试。
            let _ = fs::remove_dir_all(&staging);
            return None;
        }
    }

    let target = webview_data.join("EBWebView");
    // 临时目录与目标同卷, rename 是原子的: 不会出现"半份 profile"。
    if fs::rename(&staging, &target).is_err() {
        let _ = fs::remove_dir_all(&staging);
        return None;
    }

    // 标记: 迁移完成后即使用户删掉本地 profile, 也不再从旧目录重复搬运。
    let _ = fs::write(webview_data.join(".legacy-profile-migrated"), "1");
    eprintln!(
        "阅读：已从旧版共享 profile 迁移浏览器存储：{}",
        legacy_eb.display()
    );
    None
}

/// 递归复制文件或目录(目标已存在时递归合并, 不覆盖已有文件)。
fn copy_entry(source: &Path, destination: &Path) -> std::io::Result<()> {
    let metadata = fs::metadata(source)?;
    if metadata.is_file() {
        if !destination.exists() {
            fs::copy(source, destination)?;
        }
        return Ok(());
    }
    fs::create_dir_all(destination)?;
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        copy_entry(&entry.path(), &destination.join(entry.file_name()))?;
    }
    Ok(())
}

#[cfg(all(test, target_os = "windows"))]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_dir(name: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("reader-portable-{name}-{nonce}"))
    }

    fn write_file(path: &Path, contents: &str) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, contents).unwrap();
    }

    #[test]
    fn legacy_fallback_is_removed_when_current_database_exists() {
        let root = temp_dir("current-wins");
        let current = root.join("current").join("data");
        let legacy = root.join("reader").join("data");
        write_file(&current.join("reader.db"), "current");
        write_file(&current.join("storage").join("book.txt"), "legacy-storage");
        write_file(&legacy.join("reader.db"), "legacy");
        write_file(&legacy.join("storage").join("book.txt"), "legacy-storage");

        cleanup_legacy_data_dir(&current, &legacy);

        // 当前库优先, 不被旧数据覆盖。
        assert_eq!(fs::read_to_string(current.join("reader.db")).unwrap(), "current");
        // 旧目录里的每个文件都已在当前目录有同名同大小的一份, 才允许移除。
        assert!(!legacy.exists());
        assert!(!root.join("reader").exists());
        let _ = fs::remove_dir_all(root);
    }

    /// 旧目录里**独有**的文件必须先合并到当前目录, 不能被"当前库优先"连带删掉。
    #[test]
    fn legacy_only_files_are_merged_before_removal() {
        let root = temp_dir("legacy-only-merge");
        let current = root.join("current").join("data");
        let legacy = root.join("reader").join("data");
        write_file(&current.join("reader.db"), "current");
        write_file(&legacy.join("reader.db"), "legacy");
        write_file(&legacy.join("storage").join("assets").join("book.epub"), "legacy-epub");

        cleanup_legacy_data_dir(&current, &legacy);

        assert_eq!(fs::read_to_string(current.join("reader.db")).unwrap(), "current");
        assert_eq!(
            fs::read_to_string(current.join("storage").join("assets").join("book.epub")).unwrap(),
            "legacy-epub",
            "旧目录独有的书库文件必须搬过来",
        );
        assert!(!legacy.exists(), "合并完成后旧目录才可移除");
        let _ = fs::remove_dir_all(root);
    }

    /// 旧目录完全没有数据库(或为 0 字节)时也必须保留其中的书库内容。
    #[test]
    fn legacy_without_database_keeps_its_storage() {
        let root = temp_dir("no-db");
        let current = root.join("current").join("data");
        let legacy = root.join("reader").join("data");
        write_file(&current.join("reader.db"), "current");
        write_file(&legacy.join("storage").join("assets").join("book.epub"), "legacy-epub");

        cleanup_legacy_data_dir(&current, &legacy);

        assert_eq!(
            fs::read_to_string(current.join("storage").join("assets").join("book.epub")).unwrap(),
            "legacy-epub",
            "没有数据库不代表书库内容可以丢",
        );
        assert!(!legacy.exists());
        let _ = fs::remove_dir_all(root);
    }

    /// 合并失败时宁可保留旧目录, 也不能删掉唯一的一份数据。
    #[test]
    fn legacy_is_kept_when_merge_leaves_unique_files() {
        let root = temp_dir("merge-conflict");
        let current = root.join("current").join("data");
        let legacy = root.join("reader").join("data");
        write_file(&current.join("reader.db"), "current");
        // 同名但内容不同: 保留当前版本, 旧目录不可判为"已冗余"。
        write_file(&current.join("storage").join("book.txt"), "current-storage");
        write_file(&legacy.join("storage").join("book.txt"), "legacy-storage");

        cleanup_legacy_data_dir(&current, &legacy);

        assert_eq!(
            fs::read_to_string(current.join("storage").join("book.txt")).unwrap(),
            "current-storage",
            "同名文件必须保留当前版本",
        );
        assert!(legacy.exists(), "旧目录仍有独有内容时必须保留");
        let _ = fs::remove_dir_all(root);
    }

    /// 同名且**长度相同**但内容不同, 同样不能被判成冗余。
    ///
    /// `storage/assets` 下是导入原件的原始文件名(不按内容寻址), 只比名字 + 长度
    /// 会把旧目录里唯一的那份原件当成"已有", 随后随目录删除一起丢掉。
    #[test]
    fn legacy_keeps_same_length_file_with_different_content() {
        let root = temp_dir("same-length");
        let current = root.join("current").join("data");
        let legacy = root.join("reader").join("data");
        write_file(&current.join("reader.db"), "current");
        // 两边都是 7 字节, 内容不同。
        write_file(&current.join("storage").join("book.txt"), "current");
        write_file(&legacy.join("storage").join("book.txt"), "legacy!");

        cleanup_legacy_data_dir(&current, &legacy);

        assert_eq!(
            fs::read_to_string(current.join("storage").join("book.txt")).unwrap(),
            "current",
            "同名文件必须保留当前版本",
        );
        assert!(
            legacy.exists(),
            "旧目录里等长但不同的文件仍属独有内容, 不能删掉旧目录",
        );
        let _ = fs::remove_dir_all(root);
    }

    /// 旧库的 `-wal` / `-shm` 在当前库 checkpoint 后不存在属正常,
    /// 不能因此把整个旧目录永久留下(每次启动重复扫描)。
    #[test]
    fn legacy_database_sidecar_files_do_not_block_cleanup() {
        let root = temp_dir("db-sidecar");
        let current = root.join("current").join("data");
        let legacy = root.join("reader").join("data");
        write_file(&current.join("reader.db"), "current");
        write_file(&legacy.join("reader.db"), "legacy");
        write_file(&legacy.join("reader.db-wal"), "wal");
        write_file(&legacy.join("reader.db-shm"), "shm");

        cleanup_legacy_data_dir(&current, &legacy);

        assert_eq!(fs::read_to_string(current.join("reader.db")).unwrap(), "current");
        assert!(!legacy.exists(), "数据库副文件缺失不应阻塞旧目录清理");
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn legacy_fallback_is_migrated_when_current_database_is_missing() {
        let root = temp_dir("legacy-wins");
        let current = root.join("current").join("data");
        let legacy = root.join("reader").join("data");
        write_file(&legacy.join("reader.db"), "legacy-db");
        write_file(&legacy.join("storage").join("book.txt"), "legacy-storage");

        cleanup_legacy_data_dir(&current, &legacy);

        assert_eq!(fs::read_to_string(current.join("reader.db")).unwrap(), "legacy-db");
        assert_eq!(
            fs::read_to_string(current.join("storage").join("book.txt")).unwrap(),
            "legacy-storage",
        );
        assert!(!legacy.exists());
        assert!(!root.join("reader").exists());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn empty_legacy_fallback_is_removed_without_touching_current() {
        let root = temp_dir("empty-legacy");
        let current = root.join("current").join("data");
        let legacy = root.join("reader").join("data");
        write_file(&current.join("reader.db"), "current");
        fs::create_dir_all(&legacy).unwrap();
        write_file(&legacy.join("window-state.json"), "{}");

        cleanup_legacy_data_dir(&current, &legacy);

        assert_eq!(fs::read_to_string(current.join("reader.db")).unwrap(), "current");
        // 旧目录里的 window-state.json 会先被搬到当前目录, 随后才移除旧目录。
        assert_eq!(
            fs::read_to_string(current.join("window-state.json")).unwrap(),
            "{}",
        );
        assert!(!legacy.exists());
        assert!(!root.join("reader").exists());
        let _ = fs::remove_dir_all(root);
    }

    /* 多副本隔离(issue #20 第 4 条)。 */

    #[test]
    fn instance_identifier_differs_per_data_directory() {
        let a = temp_dir("instance-a").join("data");
        let b = temp_dir("instance-b").join("data");
        assert_ne!(instance_identifier(&a), instance_identifier(&b));
    }

    /// 同一目录(两份 exe 放在同一文件夹)必须映射到同一个 identifier ——
    /// 否则它们会绕开单实例保护, 两个进程同时写同一个 SQLite。
    #[test]
    fn instance_identifier_is_stable_for_the_same_directory() {
        let dir = temp_dir("instance-same");
        assert_eq!(instance_identifier(&dir), instance_identifier(&dir));
    }

    /// Windows 路径大小写不敏感: `C:\A` 与 `c:\a` 是同一个实例。
    #[test]
    fn instance_identifier_ignores_path_case() {
        let lower = PathBuf::from(r"c:\readers\a\data");
        let upper = PathBuf::from(r"C:\Readers\A\data");
        assert_eq!(instance_identifier(&lower), instance_identifier(&upper));
    }

    /// identifier 只能含 `[A-Za-z0-9-.]`, 否则 Tauri / 插件会拒绝或写出非法路径。
    #[test]
    fn instance_identifier_is_a_valid_bundle_identifier() {
        let identifier = instance_identifier(&temp_dir("instance-valid"));
        assert!(identifier.starts_with(LEGACY_IDENTIFIER));
        assert!(identifier
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '.'));
        // 路径里的反斜杠/盘符/空格都不能漏进来。
        assert!(
            !identifier.contains('\\') && !identifier.contains('/') && !identifier.contains("..")
        );
    }

    /// 迁移只认 localStorage 这类用户数据, 且不覆盖已有的本地 profile。
    #[test]
    fn legacy_webview_storage_is_copied_once() {
        let root = temp_dir("webview-migrate");
        let legacy = root.join("legacy").join("EBWebView");
        write_file(
            &legacy.join("Default").join("Local Storage").join("leveldb").join("000001.ldb"),
            "settings",
        );
        write_file(&legacy.join("Default").join("Cache").join("blob"), "cache");
        let target = root.join("data").join("webview");

        assert!(copy_legacy_webview_storage_from(&target, &legacy).is_none());

        let copied = target
            .join("EBWebView")
            .join("Default")
            .join("Local Storage")
            .join("leveldb")
            .join("000001.ldb");
        assert_eq!(fs::read_to_string(&copied).unwrap(), "settings");
        // 缓存目录体积大且可重建, 不搬。
        assert!(!target.join("EBWebView").join("Default").join("Cache").exists());
        assert!(target.join(".legacy-profile-migrated").is_file());
        // 临时目录必须已经 rename 掉, 不能留在磁盘上。
        assert!(!target.join("EBWebView.migrating").exists());
        // 旧目录原样保留: 另一个副本可能还在用, 且用户可能想回退到旧版本。
        assert!(legacy.is_dir(), "迁移不能动旧目录");
        assert!(
            legacy
                .join("Default")
                .join("Local Storage")
                .join("leveldb")
                .join("000001.ldb")
                .is_file()
        );

        // 已存在本地 profile 时不再重复搬运(否则每次启动都用旧数据覆盖新数据)。
        fs::write(&copied, "newer").unwrap();
        assert!(copy_legacy_webview_storage_from(&target, &legacy).is_none());
        assert_eq!(fs::read_to_string(&copied).unwrap(), "newer");

        let _ = fs::remove_dir_all(root);
    }

    /// 上次迁移被强杀留下的 `EBWebView.migrating` 必须清掉重来 —— 否则半份
    /// leveldb 会被 WebView2 当成损坏的库, 表现同样是"设置全丢"。
    #[test]
    fn stale_staging_directory_is_discarded_before_retrying() {
        let root = temp_dir("webview-stale-staging");
        let legacy = root.join("legacy").join("EBWebView");
        write_file(
            &legacy.join("Default").join("Local Storage").join("leveldb").join("000001.ldb"),
            "good",
        );
        let target = root.join("data").join("webview");
        // 半份残留: 只有一个不完整的文件。
        let leveldb = Path::new("Default").join("Local Storage").join("leveldb");
        write_file(
            &target.join("EBWebView.migrating").join(&leveldb).join("junk.ldb"),
            "half",
        );

        assert!(copy_legacy_webview_storage_from(&target, &legacy).is_none());

        let target_dir = target.join("EBWebView").join(&leveldb);
        assert_eq!(fs::read_to_string(target_dir.join("000001.ldb")).unwrap(), "good");
        assert!(!target_dir.join("junk.ldb").exists());
        assert!(!target.join("EBWebView.migrating").exists());

        let _ = fs::remove_dir_all(root);
    }

    /// 拷贝失败时必须整体放弃, 不留下任何会被下次启动误判成"本地已有 profile"
    /// 的痕迹(空骨架 / 已完成标记)。
    #[test]
    fn failed_copy_leaves_no_profile_behind() {
        let root = temp_dir("webview-copy-fails");
        let legacy = root.join("legacy").join("EBWebView");
        write_file(
            &legacy.join("Default").join("Local Storage").join("000001.ldb"),
            "settings",
        );
        // webview_data 位置上是个文件, 建不了临时目录 —— 模拟磁盘/权限失败。
        let target = root.join("data").join("webview");
        write_file(&target, "not a directory");

        assert!(copy_legacy_webview_storage_from(&target, &legacy).is_none());
        assert!(!target.join("EBWebView").exists());
        assert!(!target.join(".legacy-profile-migrated").is_file());

        let _ = fs::remove_dir_all(root);
    }

    /// 临时目录位置被用户/杀软塞了个同名文件时, 删掉它继续迁移, 而不是卡死。
    #[test]
    fn stray_file_at_staging_path_is_removed() {
        let root = temp_dir("webview-staging-file");
        let legacy = root.join("legacy").join("EBWebView");
        write_file(
            &legacy.join("Default").join("Local Storage").join("000001.ldb"),
            "settings",
        );
        let target = root.join("data").join("webview");
        write_file(&target.join("EBWebView.migrating"), "junk");

        assert!(copy_legacy_webview_storage_from(&target, &legacy).is_none());

        assert_eq!(
            fs::read_to_string(
                target
                    .join("EBWebView")
                    .join("Default")
                    .join("Local Storage")
                    .join("000001.ldb")
            )
            .unwrap(),
            "settings",
        );
        let _ = fs::remove_dir_all(root);
    }

    /// 旧 profile 被占用时不能拷(会拷到写了一半的 leveldb), 并且必须留下说明文件。
    #[test]
    fn locked_legacy_profile_is_skipped_with_a_notice() {
        let root = temp_dir("webview-locked");
        let legacy = root.join("legacy").join("EBWebView");
        let lock = legacy.join("Default").join("Local Storage").join("leveldb").join("LOCK");
        write_file(&lock, "");
        let target = root.join("data").join("webview");

        // 独占打开 LOCK, 模拟"另一个副本正开着"。
        use std::os::windows::fs::OpenOptionsExt;
        let _held = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .share_mode(0)
            .open(&lock)
            .unwrap();

        assert!(matches!(
            lock_legacy_profile(&legacy),
            LegacyProfileLock::InUse
        ));
        let notice = copy_legacy_webview_storage_from(&target, &legacy).expect("应留下说明文件");
        assert!(notice.is_file());
        assert!(!target.join("EBWebView").exists());
        // 没迁移成功就不能写"已完成"标记, 否则下次再也不会重试。
        assert!(!target.join(".legacy-profile-migrated").is_file());

        let _ = fs::remove_dir_all(root);
    }
}

/// macOS 数据目录回归测试(issue #32)。
///
/// 这些用例不依赖文件系统布局之外的平台特性, 因此在任意平台上都能跑: 迁出
/// 应用包这段逻辑一旦退化成「数据留在 `.app` 里」, 升级就会再次清空书库。
#[cfg(test)]
mod macos_data_dir_tests {
    use super::*;

    fn sandbox(name: &str) -> PathBuf {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("reader-macos-data-{name}-{nonce}"))
    }

    fn write_file(path: &Path, contents: &str) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, contents).unwrap();
    }

    /// 可执行文件在 `X.app/Contents/MacOS/` 下时, 旧数据目录必须被认出来 ——
    /// 这就是 issue #32 里随升级被删掉的那个 `data/`。
    #[test]
    fn bundled_data_dir_is_recognised_inside_app_bundle() {
        let exe = PathBuf::from("/Applications/Reader.app/Contents/MacOS/reader-desktop");
        assert_eq!(
            bundled_data_dir(&exe),
            Some(PathBuf::from("/Applications/Reader.app/Contents/MacOS/data")),
        );
    }

    /// 普通可执行文件(Windows 便携包、Linux AppImage、`cargo run`)不能被误判
    /// 成应用包, 否则会去搬一个不存在的目录。
    #[test]
    fn bundled_data_dir_ignores_non_bundle_layouts() {
        for exe in [
            "/Applications/Reader.exe",
            "/home/u/Reader/Reader.exe",
            "/usr/bin/reader",
            "/Applications/Reader.app/Contents/MacOSX/reader",
            "/Applications/Reader.app/Contents/reader",
            "/Applications/Reader/Contents/MacOS/reader",
        ] {
            assert_eq!(bundled_data_dir(Path::new(exe)), None, "{exe}");
        }
    }

    /// 首次升级启动: 包内旧数据整棵树搬到用户目录, 数据库与备份都在。
    #[test]
    fn bundled_data_is_copied_out_of_the_app() {
        let root = sandbox("migrate-out");
        let bundled = root.join("Reader.app/Contents/MacOS/data");
        write_file(&bundled.join("reader.db"), "sources-and-shelf");
        write_file(&bundled.join("storage/webdav/default/backups/backup-1.zip"), "backup");
        write_file(&bundled.join("storage/cache/default/abc/1.txt"), "chapter-cache");

        let current = root.join("Application Support/reader/data");
        migrate_bundled_data(&bundled, &current);

        assert_eq!(
            fs::read_to_string(current.join("reader.db")).unwrap(),
            "sources-and-shelf",
        );
        assert_eq!(
            fs::read_to_string(current.join("storage/webdav/default/backups/backup-1.zip")).unwrap(),
            "backup",
        );
        assert!(current.join(APP_BUNDLE_MIGRATED_MARKER).is_file());
        // 章节缓存体积可能上 GB 且可重建, 迁移必须跳过, 否则首次启动要白屏等很久。
        assert!(
            !current.join("storage/cache").exists(),
            "缓存目录不应随迁移复制",
        );
        // 不能动 .app: 增删包内文件会让代码签名失效, 应用随后直接启动不了。
        assert!(
            bundled.join("reader.db").is_file(),
            "迁移必须只复制, 不删除应用包内的文件",
        );

        // 用户在新位置删掉数据后, 包内旧副本不得再把它"复活"。
        fs::remove_file(current.join("reader.db")).unwrap();
        migrate_bundled_data(&bundled, &current);
        assert!(!current.join("reader.db").exists(), "迁移只应发生一次");

        let _ = fs::remove_dir_all(root);
    }

    /// 新位置已有同名文件时保留新版本 —— 用户升级后继续读写的那个才是最新的。
    #[test]
    fn migration_keeps_newer_files() {
        let root = sandbox("keep-newer");
        let bundled = root.join("Reader.app/Contents/MacOS/data");
        write_file(&bundled.join("reader.db"), "old");
        let current = root.join("Application Support/reader/data");
        write_file(&current.join("reader.db"), "new");

        migrate_bundled_data(&bundled, &current);

        assert_eq!(fs::read_to_string(current.join("reader.db")).unwrap(), "new");
        let _ = fs::remove_dir_all(root);
    }

    /// 没有旧数据(全新安装)时不建标记文件, 也不留下半成品目录。
    #[test]
    fn missing_bundled_data_is_a_noop() {
        let root = sandbox("no-bundled");
        let bundled = root.join("Reader.app/Contents/MacOS/data");
        let current = root.join("Application Support/reader/data");

        migrate_bundled_data(&bundled, &current);

        assert!(!current.exists());
        assert!(!current.join(APP_BUNDLE_MIGRATED_MARKER).exists());
        let _ = fs::remove_dir_all(root);
    }
}
/// 废纸篓捞回: Finder 覆盖安装会把旧 `Reader.app` 移进废纸篓, 那是用户丢失
/// 数据的最后一份副本。
#[cfg(test)]
mod trashed_bundle_tests {
    use super::*;

    fn sandbox(name: &str) -> PathBuf {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("reader-trash-{name}-{nonce}"))
    }

    fn write_bundle(root: &Path, app: &str, db: &str) -> PathBuf {
        let data = root.join(app).join("Contents").join("MacOS").join("data");
        fs::create_dir_all(&data).unwrap();
        fs::write(data.join("reader.db"), db).unwrap();
        data
    }

    /// 废纸篓里可能有多个旧包(多次升级), 必须挑书库最新的那个。
    #[test]
    fn picks_the_most_recent_trashed_bundle() {
        let root = sandbox("pick");
        write_bundle(&root, "Reader.app", "old");
        let newest = write_bundle(&root, "Reader 2.app", "newest");
        // 目录创建顺序不可靠, 显式拉开 mtime。
        let past = std::time::SystemTime::now() - std::time::Duration::from_secs(3600);
        let old_db = root.join("Reader.app/Contents/MacOS/data/reader.db");
        fs::File::options()
            .write(true)
            .open(&old_db)
            .unwrap()
            .set_modified(past)
            .unwrap();

        assert_eq!(pick_latest_bundle_data(&root), Some(newest));
        let _ = fs::remove_dir_all(root);
    }

    /// 废纸篓里的普通文件、没有 `data/reader.db` 的 app 都不能被误认。
    #[test]
    fn ignores_non_library_entries() {
        let root = sandbox("ignore");
        fs::create_dir_all(root.join("Some.app/Contents/MacOS")).unwrap();
        fs::write(root.join("notes.txt"), "hi").unwrap();
        fs::create_dir_all(root.join("Other")).unwrap();

        assert_eq!(pick_latest_bundle_data(&root), None);
        let _ = fs::remove_dir_all(root);
    }

    /// 废纸篓不存在(macOS 首次启动前 / 权限受限)时安静返回 None。
    #[test]
    fn missing_trash_is_not_an_error() {
        let root = sandbox("missing");
        assert_eq!(pick_latest_bundle_data(&root.join("nope")), None);
    }
}