//! 窗口尺寸/位置与迷你模式的持久化。
//!
//! 不使用 `tauri-plugin-window-state`: 它把状态写进 Tauri 的 app config 目录,
//! 而本应用是便携式的(数据固定在 exe 旁的 `data/`), 换机器后状态会错位。
//! 这里只读写 `data/window-state.json`, 并在恢复前校验显示器, 避免窗口跑到
//! 屏幕外导致"应用启动了但看不到窗口"。
//!
//! 本模块放在 `reader-rust`(而不是桌面壳)是必需的: 迷你模式开关由 IPC 命令
//! 落盘, 桌面壳才能在下次启动时直接以迷你尺寸建窗。桌面壳依赖 reader-rust,
//! 反向引用不成立。

use std::path::PathBuf;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use tauri::{LogicalPosition, LogicalSize, Monitor, WebviewWindow};

/// 窗口几何(逻辑像素)。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Geometry {
    pub width: f64,
    pub height: f64,
    pub x: f64,
    pub y: f64,
}

/// 窗口的持久化状态(逻辑像素)。
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct WindowState {
    // ─── 常规模式几何 ───
    pub width: f64,
    pub height: f64,
    pub x: f64,
    pub y: f64,
    #[serde(default)]
    pub maximized: bool,
    // ─── 迷你模式 ───
    /// 迷你模式开关。开启时下次启动直接以迷你尺寸建窗, 避免先弹大窗再缩小。
    #[serde(default)]
    pub mini_mode: bool,
    /// 迷你模式下的尺寸/位置, 用户调整过才有值(缺省用内置默认值)。
    #[serde(default)]
    pub mini_width: Option<f64>,
    #[serde(default)]
    pub mini_height: Option<f64>,
    #[serde(default)]
    pub mini_x: Option<f64>,
    #[serde(default)]
    pub mini_y: Option<f64>,
}

impl Default for WindowState {
    fn default() -> Self {
        Self {
            width: InitialGeometry::DEFAULT_WIDTH,
            height: InitialGeometry::DEFAULT_HEIGHT,
            x: 0.0,
            y: 0.0,
            maximized: false,
            mini_mode: false,
            mini_width: None,
            mini_height: None,
            mini_x: None,
            mini_y: None,
        }
    }
}

impl WindowState {
    /// 迷你模式的尺寸: 用户调整过就用记下的, 否则用默认值。
    pub fn mini_size(&self) -> (f64, f64) {
        (
            self.mini_width.unwrap_or(InitialGeometry::MINI_WIDTH),
            self.mini_height.unwrap_or(InitialGeometry::MINI_HEIGHT),
        )
    }

    /// 迷你模式的位置。用户还没调整过时退回常规模式的位置, 让窗口"就地缩小"
    /// 而不是跳到屏幕左上角。
    pub fn mini_position(&self) -> (f64, f64) {
        (
            self.mini_x.unwrap_or(self.x),
            self.mini_y.unwrap_or(self.y),
        )
    }
}

/// 写盘节流: 拖拽窗口会连续触发 Resized/Moved, 不能每次都落盘。
const SAVE_THROTTLE: Duration = Duration::from_millis(1000);

pub struct WindowStateStore {
    path: PathBuf,
    last_save: Option<Instant>,
    /// 上次读写到的状态。事件处理里每个节流窗口都会用到它, 缓存避免反复读盘;
    /// 最大化与迷你模式也靠它保留另一套几何信息。
    cached: Option<WindowState>,
}

impl WindowStateStore {
    pub fn new(path: PathBuf) -> Self {
        // 状态文件损坏不值得让应用启动失败, 当作没有历史状态即可。
        let cached = std::fs::read_to_string(&path)
            .ok()
            .and_then(|raw| serde_json::from_str(&raw).ok());
        Self {
            path,
            last_save: None,
            cached,
        }
    }

    pub fn load(&self) -> Option<WindowState> {
        self.cached
    }

    /// 距离上次写盘是否已超过节流窗口。拖动窗口会连续触发 Resized/Moved,
    /// 逐次落盘既浪费又可能截断正在写入的文件。
    pub fn should_save_now(&mut self) -> bool {
        let now = Instant::now();
        match self.last_save {
            Some(last) if now.duration_since(last) < SAVE_THROTTLE => false,
            _ => {
                self.last_save = Some(now);
                true
            }
        }
    }

    pub fn save(&mut self, state: WindowState) {
        self.last_save = Some(Instant::now());
        self.cached = Some(state);
        if let Some(parent) = self.path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Ok(json) = serde_json::to_string_pretty(&state) {
            // 状态丢失只会让下次启动回到默认尺寸, 不影响数据, 因此失败不打断。
            let _ = std::fs::write(&self.path, json);
        }
    }
}

/// 读取窗口当前的几何数据。最小化时位置没有意义(Windows 会报告 -32000),
/// 此时返回 None, 避免把垃圾坐标记下来。
pub fn read_geometry(window: &WebviewWindow) -> Option<Geometry> {
    if window.is_minimized().unwrap_or(false) {
        return None;
    }
    let scale = window.scale_factor().unwrap_or(1.0);
    if scale <= 0.0 {
        return None;
    }
    let position = window.outer_position().ok()?.to_logical::<f64>(scale);
    // 记录内尺寸而非外框尺寸: 恢复时用的是 `set_size`(内尺寸), 两者混用会让
    // 窗口在每次启动后逐渐变大。
    let size = window.inner_size().ok()?.to_logical::<f64>(scale);
    Some(Geometry {
        width: size.width,
        height: size.height,
        x: position.x,
        y: position.y,
    })
}

/// 把一次读到的几何数据合并进状态。
///
/// 抽成纯函数(不碰窗口)有两个好处: 可被单测覆盖; 且能在持锁时安全调用 ——
/// 它只读取已有数据, 不会触发窗口事件。
///
/// 常规与迷你两套几何互不覆盖: 否则来回切换迷你模式时, 退出后会回到"迷你
/// 尺寸", 或下次进入时沿用被覆盖掉的常规尺寸。
pub fn merge(state: WindowState, geometry: Option<Geometry>, maximized: bool) -> WindowState {
    let mut state = state;

    if state.mini_mode {
        // 最大化与迷你模式互斥, 这里不需要处理 maximized。
        if let Some(geometry) = geometry {
            state.mini_width = Some(geometry.width);
            state.mini_height = Some(geometry.height);
            state.mini_x = Some(geometry.x);
            state.mini_y = Some(geometry.y);
        }
        return state;
    }

    state.maximized = maximized;
    // 最大化时读到的是最大化尺寸, 记下来会导致下次还原后窗口比屏幕还大,
    // 因此只翻转标志、保留最大化前的几何信息。
    if maximized {
        return state;
    }
    if let Some(geometry) = geometry {
        state.width = geometry.width;
        state.height = geometry.height;
        state.x = geometry.x;
        state.y = geometry.y;
    }
    state
}

/// 读取窗口当前状态并合并到已有状态上。
pub fn capture(window: &WebviewWindow, previous: Option<WindowState>) -> Option<WindowState> {
    let state = previous.unwrap_or_default();
    let maximized = window.is_maximized().unwrap_or(false);
    let geometry = read_geometry(window);

    // 最小化且未最大化时读不到有意义的位置, 此时保持原状态不动, 避免把
    // Windows 报告的 -32000 之类垃圾坐标写进状态文件。
    if geometry.is_none() && !maximized {
        return None;
    }
    Some(merge(state, geometry, maximized))
}

/// 显示器的逻辑矩形。抽成纯数据是为了让"窗口跑到屏幕外"这一边界可被
/// 单测覆盖 —— `Monitor` 无法在测试中构造。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MonitorRect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl MonitorRect {
    fn from_monitor(monitor: &Monitor) -> Self {
        let scale = monitor.scale_factor();
        let origin = monitor.position().to_logical::<f64>(scale);
        let size = monitor.size().to_logical::<f64>(scale);
        Self {
            x: origin.x,
            y: origin.y,
            width: size.width,
            height: size.height,
        }
    }
}

/// 当前所有显示器的逻辑矩形, **主显示器排在首位**。
///
/// 顺序很关键: `sanitize_geometry` 在窗口位置越界时会回退到首元素来夹取尺寸上限,
/// 而调用方随后用 `center()` 把窗口放到主显示器上 —— 两者必须指向同一块屏, 否则
/// 会拿副屏的尺寸去限制一个将要在主屏上居中的窗口。Tauri 的 `available_monitors()`
/// 不保证主显示器在首位, 因此这里显式把它提到前面。
///
/// 取不到显示器时返回空表, 调用方会据此走居中逻辑。
pub fn monitor_rects(window: &WebviewWindow) -> Vec<MonitorRect> {
    let mut monitors = window
        .available_monitors()
        .unwrap_or_default()
        .iter()
        .map(MonitorRect::from_monitor)
        .collect::<Vec<_>>();
    if let Some(primary) = window.primary_monitor().ok().flatten() {
        let primary = MonitorRect::from_monitor(&primary);
        // 用精确相等匹配: 同一块屏的位置/尺寸应当逐位一致。匹配不到就保持原顺序
        // (不猜), 后续回退到首元素仍是一个"某块真实屏幕"的合理上限。
        if let Some(index) = monitors.iter().position(|monitor| *monitor == primary) {
            monitors.swap(0, index);
        }
    }
    monitors
}

/// 检查一个窗口位置是否仍落在某块显示器上。
///
/// 拔掉外接显示器或修改分辨率后, 保存的位置可能落在所有屏幕之外。此时必须
/// 忽略位置, 否则窗口不可见, 用户会以为应用没启动。
fn position_is_visible(monitors: &[MonitorRect], x: f64, y: f64, width: f64, height: f64) -> bool {
    monitors.iter().any(|monitor| {
        // 要求窗口标题栏区域(顶部一小条)可见, 保证用户还能拖动它。
        let visible_height = height.min(80.0);
        x < monitor.x + monitor.width
            && x + width > monitor.x
            && y < monitor.y + monitor.height
            && y + visible_height > monitor.y
    })
}

/// 找到包含给定坐标的显示器。
fn monitor_containing(monitors: &[MonitorRect], x: f64, y: f64) -> Option<&MonitorRect> {
    monitors.iter().find(|monitor| {
        x >= monitor.x && x < monitor.x + monitor.width && y >= monitor.y && y < monitor.y + monitor.height
    })
}

/// 把一组几何数据裁剪成安全值: 尺寸限制在显示器与最小尺寸之间, 位置不越出
/// 显示器。越界(或没有显示器信息)时返回 None 位置, 交由调用方居中。
pub fn sanitize_geometry(
    width: f64,
    height: f64,
    x: f64,
    y: f64,
    monitors: &[MonitorRect],
    min_size: (f64, f64),
) -> (LogicalSize<f64>, Option<LogicalPosition<f64>>) {
    let mut width = width.max(min_size.0);
    let mut height = height.max(min_size.1);

    // 上限: 从大屏换到小屏后, 保存的尺寸可能比当前屏幕还大, 会让界面
    // 有一部分永远看不到。夹到所在显示器内(仍不低于最小尺寸)。
    //
    // 位置不在任何显示器上时(拔掉副屏/改过分辨率)必须回退到主显示器:
    // 调用方随后会 `center()`, 落点就是主显示器; 若此时跳过上限夹取, 窗口
    // 会以超出屏幕的尺寸居中, 四周都被裁掉。
    let target = monitor_containing(monitors, x, y).or_else(|| monitors.first());
    if let Some(monitor) = target {
        width = width.min(monitor.width.max(min_size.0));
        height = height.min(monitor.height.max(min_size.1));
    }

    let position = if monitors.is_empty() || !position_is_visible(monitors, x, y, width, height) {
        None
    } else {
        Some(LogicalPosition::new(x, y))
    };

    (LogicalSize::new(width, height), position)
}

/// 把保存的状态裁剪成安全值。
pub fn sanitize(
    state: WindowState,
    monitors: &[MonitorRect],
    min_size: (f64, f64),
) -> (LogicalSize<f64>, Option<LogicalPosition<f64>>, bool) {
    let (size, position) =
        sanitize_geometry(state.width, state.height, state.x, state.y, monitors, min_size);
    (size, position, state.maximized)
}

/// 应用启动时解析出的窗口几何参数。
pub struct InitialGeometry {
    pub size: LogicalSize<f64>,
    pub position: Option<LogicalPosition<f64>>,
    pub maximized: bool,
    /// 启动时是否处于迷你模式(桌面壳据此设置窗口置顶)。
    pub mini_mode: bool,
}

impl InitialGeometry {
    pub const DEFAULT_WIDTH: f64 = 1180.0;
    pub const DEFAULT_HEIGHT: f64 = 820.0;
    /// 前端的响应式断点最窄到 420px(见 ReadSettings.vue), 布局在该宽度下
    /// 有对应样式, 因此可以把它作为常规模式的窗口下限。
    pub const MIN_WIDTH: f64 = 420.0;
    pub const MIN_HEIGHT: f64 = 320.0;
    /// 迷你模式默认尺寸。刻意小于常规下限 —— 迷你模式的控件只有
    /// 上一章/下一章/目录, 不需要完整布局的宽度, 因此单独放开下限。
    pub const MINI_WIDTH: f64 = 320.0;
    pub const MINI_HEIGHT: f64 = 240.0;
    /// 迷你模式下窗口允许的最小尺寸。比 `MINI_WIDTH/HEIGHT` 再小一点,
    /// 让用户能继续往里拖, 但不至于小到无法点击那三个按钮。
    pub const MINI_MIN_WIDTH: f64 = 240.0;
    pub const MINI_MIN_HEIGHT: f64 = 180.0;

    /// 某一模式下的尺寸下限。迷你模式用更小的下限, 否则 320×240 会被常规
    /// 下限(420×320)抬回去, 用户永远缩不到目标尺寸。
    pub fn min_size_for(mini_mode: bool) -> (f64, f64) {
        if mini_mode {
            (Self::MINI_MIN_WIDTH, Self::MINI_MIN_HEIGHT)
        } else {
            (Self::MIN_WIDTH, Self::MIN_HEIGHT)
        }
    }

    fn default_size() -> LogicalSize<f64> {
        LogicalSize::new(Self::DEFAULT_WIDTH, Self::DEFAULT_HEIGHT)
    }

    /// 没有历史状态时用默认尺寸并居中。
    pub fn fallback() -> Self {
        Self {
            size: Self::default_size(),
            position: None,
            maximized: false,
            mini_mode: false,
        }
    }

    /// 依据历史状态与当前显示器布局计算初始几何参数。
    pub fn resolve(window: &WebviewWindow, saved: Option<WindowState>) -> Self {
        let Some(saved) = saved else {
            return Self::fallback();
        };
        // 走 `monitor_rects` 而不是自己构造: 它保证主显示器在首位, 而
        // `sanitize_geometry` 在位置越界时会用首元素夹取尺寸上限 —— 两处必须
        // 用同一份列表, 否则该不变量在这里会悄悄失效。
        let monitors = monitor_rects(window);

        // 迷你模式: 直接用迷你几何建窗, 并且不恢复最大化(两者互斥)。
        if saved.mini_mode {
            let (width, height) = saved.mini_size();
            let (x, y) = saved.mini_position();
            let (size, position) = sanitize_geometry(
                width,
                height,
                x,
                y,
                &monitors,
                Self::min_size_for(true),
            );
            return Self {
                size,
                position,
                maximized: false,
                mini_mode: true,
            };
        }

        let (size, position, maximized) = sanitize(saved, &monitors, Self::min_size_for(false));
        Self {
            size,
            position,
            maximized,
            mini_mode: false,
        }
    }
}

/// 供测试使用: 构造一块显示器。
#[cfg(test)]
mod tests {
    use super::*;

    fn state(x: f64, y: f64, width: f64, height: f64) -> WindowState {
        WindowState {
            width,
            height,
            x,
            y,
            ..Default::default()
        }
    }

    fn monitor(x: f64, y: f64, width: f64, height: f64) -> MonitorRect {
        MonitorRect {
            x,
            y,
            width,
            height,
        }
    }

    /// 尺寸下限必须生效: 旧状态可能是在下限调小前保存的。
    #[test]
    fn clamps_size_to_minimum() {
        let (size, _, _) = sanitize(state(0.0, 0.0, 100.0, 100.0), &[], (420.0, 320.0));
        assert_eq!(size.width, 420.0);
        assert_eq!(size.height, 320.0);
    }

    #[test]
    fn clamps_negative_size_to_minimum() {
        let (size, _, _) = sanitize(state(0.0, 0.0, -50.0, 0.0), &[], (420.0, 320.0));
        assert_eq!(size.width, 420.0);
        assert_eq!(size.height, 320.0);
    }

    /// 没有显示器信息(极少见)时必须忽略位置, 让窗口走居中逻辑。
    #[test]
    fn ignores_position_without_monitors() {
        let (_, position, _) = sanitize(state(10.0, 10.0, 800.0, 600.0), &[], (420.0, 320.0));
        assert!(position.is_none());
    }

    #[test]
    fn keeps_valid_state_untouched() {
        let (size, position, maximized) = sanitize(
            state(50.0, 60.0, 900.0, 700.0),
            &[monitor(0.0, 0.0, 1920.0, 1080.0)],
            (420.0, 320.0),
        );
        assert_eq!(size.width, 900.0);
        assert_eq!(size.height, 700.0);
        let position = position.expect("on-screen position must be kept");
        assert_eq!(position.x, 50.0);
        assert_eq!(position.y, 60.0);
        assert!(!maximized);
    }

    /// 关键边界: 外接显示器被拔掉后, 保存的位置落在已不存在的屏幕上。
    /// 必须丢弃该位置让窗口居中, 否则用户看不到窗口。
    #[test]
    fn drops_position_of_removed_monitor() {
        let only_laptop = [monitor(0.0, 0.0, 1920.0, 1080.0)];
        // 之前保存在右侧副屏(x=1920 起)上的窗口。
        let (_, position, _) = sanitize(
            state(2400.0, 300.0, 800.0, 600.0),
            &only_laptop,
            (420.0, 320.0),
        );
        assert!(position.is_none(), "off-screen position must be dropped");
    }

    /// 位置恰好压在屏幕右下角之外时同样应被丢弃。
    #[test]
    fn drops_position_beyond_monitor_edges() {
        let monitors = [monitor(0.0, 0.0, 1920.0, 1080.0)];
        let (_, position, _) = sanitize(
            state(1900.0, 500.0, 800.0, 600.0),
            &monitors,
            (420.0, 320.0),
        );
        // 左侧仍有 20px 可见, 属于"还能拖回来", 因此允许保留。
        assert!(position.is_some());

        let (_, far_off, _) = sanitize(
            state(2000.0, 500.0, 800.0, 600.0),
            &monitors,
            (420.0, 320.0),
        );
        assert!(far_off.is_none());
    }

    /// 负坐标(副屏在主屏左侧)是合法布局, 不能被误判为越界。
    #[test]
    fn accepts_negative_coordinates_on_left_monitor() {
        let monitors = [
            monitor(-1920.0, 0.0, 1920.0, 1080.0),
            monitor(0.0, 0.0, 1920.0, 1080.0),
        ];
        let (_, position, _) = sanitize(
            state(-1500.0, 200.0, 800.0, 600.0),
            &monitors,
            (420.0, 320.0),
        );
        let position = position.expect("left monitor position must be kept");
        assert_eq!(position.x, -1500.0);
    }

    /// 关键边界: 从大屏换到小屏后, 保存的尺寸可能比当前屏幕还大, 会让界面
    /// 有一部分永远看不到, 因此要夹到屏幕尺寸之内。
    #[test]
    fn clamps_size_down_to_smaller_monitor() {
        let small = [monitor(0.0, 0.0, 1366.0, 768.0)];
        let (size, _, _) = sanitize(state(0.0, 0.0, 2560.0, 1440.0), &small, (420.0, 320.0));
        assert_eq!(size.width, 1366.0);
        assert_eq!(size.height, 768.0);
    }

    /// 屏幕比最小尺寸还小时, 最小尺寸优先(否则窗口无法正常使用)。
    #[test]
    fn minimum_size_wins_over_tiny_monitor() {
        let tiny = [monitor(0.0, 0.0, 300.0, 200.0)];
        let (size, _, _) = sanitize(state(0.0, 0.0, 300.0, 200.0), &tiny, (420.0, 320.0));
        assert_eq!(size.width, 420.0);
        assert_eq!(size.height, 320.0);
    }

    /// 尺寸上限应按窗口所在的那块屏计算, 而不是任意一块。
    #[test]
    fn clamps_against_the_monitor_the_window_is_on() {
        let monitors = [
            monitor(0.0, 0.0, 1366.0, 768.0),
            monitor(1366.0, 0.0, 3840.0, 2160.0),
        ];
        // 窗口在右侧大屏上, 不应被左侧小屏限制。
        let (size, _, _) = sanitize(
            state(2000.0, 100.0, 3000.0, 2000.0),
            &monitors,
            (420.0, 320.0),
        );
        assert_eq!(size.width, 3000.0);
        assert_eq!(size.height, 2000.0);
    }

    /// 最大化标志必须原样带出, 供启动时恢复。
    #[test]
    fn preserves_maximized_flag() {
        let mut saved = state(0.0, 0.0, 800.0, 600.0);
        saved.maximized = true;
        let (_, _, maximized) = sanitize(saved, &[], (420.0, 320.0));
        assert!(maximized);
    }

    #[test]
    fn throttles_repeated_saves() {
        let dir = std::env::temp_dir().join("reader-window-state-test");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("window-state.json");
        let _ = std::fs::remove_file(&path);

        let mut store = WindowStateStore::new(path.clone());
        assert!(store.should_save_now());
        store.save(state(0.0, 0.0, 800.0, 600.0));
        // 紧接着的第二次调用应被节流, 避免拖拽时疯狂写盘。
        assert!(!store.should_save_now());

        assert_eq!(store.load().map(|s| s.width), Some(800.0));
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn load_returns_none_for_missing_or_broken_file() {
        let dir = std::env::temp_dir().join("reader-window-state-test-broken");
        let _ = std::fs::create_dir_all(&dir);

        let missing = WindowStateStore::new(dir.join("does-not-exist.json"));
        assert!(missing.load().is_none());

        let broken_path = dir.join("broken.json");
        std::fs::write(&broken_path, "{ not json").unwrap();
        let broken = WindowStateStore::new(broken_path.clone());
        assert!(broken.load().is_none());
        let _ = std::fs::remove_file(&broken_path);
    }

    #[test]
    fn round_trips_all_fields() {
        let dir = std::env::temp_dir().join("reader-window-state-test-roundtrip");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("window-state.json");

        let mut store = WindowStateStore::new(path.clone());
        store.save(WindowState {
            width: 1234.0,
            height: 567.0,
            x: -20.0,
            y: 40.0,
            maximized: true,
            mini_mode: true,
            mini_width: Some(460.0),
            mini_height: Some(340.0),
            mini_x: Some(10.0),
            mini_y: Some(20.0),
        });

        let loaded = store.load().unwrap();
        assert_eq!(loaded.width, 1234.0);
        assert_eq!(loaded.height, 567.0);
        assert_eq!(loaded.x, -20.0);
        assert_eq!(loaded.y, 40.0);
        assert!(loaded.maximized);
        assert!(loaded.mini_mode);
        assert_eq!(loaded.mini_width, Some(460.0));
        assert_eq!(loaded.mini_y, Some(20.0));
        let _ = std::fs::remove_file(&path);
    }

    /// 旧版本的状态文件没有迷你字段, 必须仍能读出来(不能因为加了字段就丢失
    /// 用户已有的窗口尺寸)。
    #[test]
    fn loads_legacy_state_without_mini_fields() {
        let dir = std::env::temp_dir().join("reader-window-state-test-legacy");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("window-state.json");
        std::fs::write(
            &path,
            r#"{"width":900.0,"height":700.0,"x":30.0,"y":40.0,"maximized":false}"#,
        )
        .unwrap();

        let store = WindowStateStore::new(path.clone());
        let loaded = store.load().expect("legacy state must still load");
        assert_eq!(loaded.width, 900.0);
        assert!(!loaded.mini_mode);
        assert_eq!(loaded.mini_width, None);
        // 未调整过迷你尺寸时应退回内置默认值(320×240)。
        assert_eq!(
            loaded.mini_size(),
            (InitialGeometry::MINI_WIDTH, InitialGeometry::MINI_HEIGHT)
        );
        // 未调整过迷你位置时应就地缩小, 而不是跳到 (0,0)。
        assert_eq!(loaded.mini_position(), (30.0, 40.0));
        let _ = std::fs::remove_file(&path);
    }

    /// 迷你尺寸同样受最小尺寸与显示器范围约束。
    #[test]
    fn mini_geometry_is_sanitized_too() {
        // 比迷你下限还小的尺寸要被抬高(注意用的是迷你下限, 不是常规下限)。
        let (size, position) = sanitize_geometry(
            100.0,
            80.0,
            10.0,
            10.0,
            &[monitor(0.0, 0.0, 1920.0, 1080.0)],
            InitialGeometry::min_size_for(true),
        );
        assert_eq!(size.width, InitialGeometry::MINI_MIN_WIDTH);
        assert_eq!(size.height, InitialGeometry::MINI_MIN_HEIGHT);
        assert!(position.is_some());

        // 迷你位置落在已拔掉的副屏上时要被丢弃。
        let (_, position) = sanitize_geometry(
            320.0,
            240.0,
            2600.0,
            300.0,
            &[monitor(0.0, 0.0, 1920.0, 1080.0)],
            InitialGeometry::min_size_for(true),
        );
        assert!(position.is_none());
    }

    /// 关键: 320×240 的迷你尺寸必须能原样保留。
    /// 若误用常规下限(420×320), 它会被抬回去, 用户永远缩不到目标尺寸。
    #[test]
    fn mini_size_is_not_clamped_by_normal_minimum() {
        let monitors = [monitor(0.0, 0.0, 1920.0, 1080.0)];
        let (size, _) = sanitize_geometry(
            InitialGeometry::MINI_WIDTH,
            InitialGeometry::MINI_HEIGHT,
            100.0,
            100.0,
            &monitors,
            InitialGeometry::min_size_for(true),
        );
        assert_eq!(size.width, 320.0);
        assert_eq!(size.height, 240.0);

        // 同一尺寸在常规下限下会被抬到 420×320 —— 这正是需要按模式取下限的原因。
        let (normal_size, _) = sanitize_geometry(
            InitialGeometry::MINI_WIDTH,
            InitialGeometry::MINI_HEIGHT,
            100.0,
            100.0,
            &monitors,
            InitialGeometry::min_size_for(false),
        );
        assert_eq!(normal_size.width, 420.0);
        assert_eq!(normal_size.height, 320.0);
    }

    /// 迷你下限必须小于迷你默认尺寸, 否则用户没有可拖动的余量。
    #[test]
    fn mini_limits_are_smaller_than_mini_defaults() {
        let (min_w, min_h) = InitialGeometry::min_size_for(true);
        assert!(min_w < InitialGeometry::MINI_WIDTH);
        assert!(min_h < InitialGeometry::MINI_HEIGHT);
        // 常规下限仍应大于迷你默认尺寸, 保证两套模式的定位不混淆。
        let (normal_w, normal_h) = InitialGeometry::min_size_for(false);
        assert!(normal_w > InitialGeometry::MINI_WIDTH);
        assert!(normal_h > InitialGeometry::MINI_HEIGHT);
    }

    /// 迷你模式解析出的初始几何不应带最大化标志(两者互斥)。
    ///
    /// `resolve` 需要窗口句柄, 无法在单测里构造; 因此这里验证它依赖的两条纯逻辑:
    /// `fallback` 默认非迷你非最大化, 且 `sanitize` 只回传标志而不改写它
    /// (强制为 false 由 `resolve` 在迷你分支完成)。
    #[test]
    fn mini_mode_geometry_is_not_maximized() {
        let geometry = InitialGeometry::fallback();
        assert!(!geometry.mini_mode);
        assert!(!geometry.maximized);

        // 迷你模式下仍带最大化标志的旧状态: sanitize 原样回传, 由 resolve 压掉。
        let (size, _, maximized) = sanitize(
            WindowState {
                mini_mode: true,
                maximized: true,
                ..state(0.0, 0.0, 800.0, 600.0)
            },
            &[],
            (420.0, 320.0),
        );
        assert!(maximized, "sanitize 只回传标志, 不负责压制");
        assert_eq!(size.width, 800.0);
    }

    /// capture 在迷你模式下必须只更新迷你几何, 不能覆盖常规几何 ——
    /// 否则退出迷你模式会回到"迷你尺寸"。
    ///
    /// 直接驱动 `merge`(capture 的纯函数内核): 之前这版只是手工给字段赋值再断言,
    /// 等于把被测代码抄了一遍, 任何回归都发现不了。
    #[test]
    fn capture_in_mini_mode_keeps_normal_geometry() {
        let previous = WindowState {
            width: 1180.0,
            height: 820.0,
            x: 100.0,
            y: 100.0,
            mini_mode: true,
            ..Default::default()
        };
        let merged = merge(previous, Some(geometry(460.0, 340.0, 40.0, 50.0)), false);

        // 迷你几何被更新。
        assert_eq!(merged.mini_size(), (460.0, 340.0));
        assert_eq!(merged.mini_position(), (40.0, 50.0));
        // 常规几何原样保留, 这是"退出迷你模式能回到原尺寸"的前提。
        assert_eq!(merged.width, 1180.0);
        assert_eq!(merged.height, 820.0);
        assert_eq!(merged.x, 100.0);
        assert_eq!(merged.y, 100.0);
    }

    /// 位置不在任何显示器上时, 尺寸上限必须回退到主显示器而不是被跳过。
    ///
    /// 调用方随后会 `center()`(落点是主显示器); 若此处不夹, 窗口会以超出屏幕的
    /// 尺寸居中, 四周被裁掉 —— 正是"拔掉副屏后界面看不全"的成因。
    #[test]
    fn clamps_size_to_primary_monitor_when_position_is_offscreen() {
        let laptop = [monitor(0.0, 0.0, 1366.0, 768.0)];
        // 窗口此前在已拔掉的 3840x2160 副屏上, 位置 (5000, 300) 已越界。
        let (size, position) = sanitize_geometry(
            2560.0,
            1440.0,
            5000.0,
            300.0,
            &laptop,
            (420.0, 320.0),
        );

        assert!(position.is_none(), "越界位置应被丢弃以走居中");
        assert_eq!(size.width, 1366.0, "尺寸应夹到主显示器宽度");
        assert_eq!(size.height, 768.0, "尺寸应夹到主显示器高度");
    }

    fn geometry(width: f64, height: f64, x: f64, y: f64) -> Geometry {
        Geometry {
            width,
            height,
            x,
            y,
        }
    }

    /// 迷你模式下 merge 只写迷你几何, 常规几何必须原样保留。
    #[test]
    fn merge_in_mini_mode_only_updates_mini_geometry() {
        let state = WindowState {
            width: 1180.0,
            height: 820.0,
            x: 100.0,
            y: 100.0,
            mini_mode: true,
            ..Default::default()
        };
        let merged = merge(state, Some(geometry(460.0, 340.0, 40.0, 50.0)), false);

        assert_eq!(merged.mini_size(), (460.0, 340.0));
        assert_eq!(merged.mini_position(), (40.0, 50.0));
        // 常规几何不受影响, 这是"退出迷你模式能回到原位"的前提。
        assert_eq!(merged.width, 1180.0);
        assert_eq!(merged.height, 820.0);
        assert_eq!(merged.x, 100.0);
    }

    /// 常规模式下 merge 只写常规几何, 已保存的迷你几何不能被覆盖。
    #[test]
    fn merge_in_normal_mode_only_updates_normal_geometry() {
        let state = WindowState {
            mini_mode: false,
            mini_width: Some(460.0),
            mini_height: Some(340.0),
            mini_x: Some(7.0),
            mini_y: Some(8.0),
            ..Default::default()
        };
        let merged = merge(state, Some(geometry(1000.0, 700.0, 20.0, 30.0)), false);

        assert_eq!(merged.width, 1000.0);
        assert_eq!(merged.y, 30.0);
        // 迷你几何保留, 下次进入迷你模式仍用用户调好的尺寸。
        assert_eq!(merged.mini_size(), (460.0, 340.0));
        assert_eq!(merged.mini_position(), (7.0, 8.0));
    }

    /// 最大化时只翻转标志: 把最大化尺寸记成常规尺寸会让下次还原后窗口比屏幕还大。
    #[test]
    fn merge_keeps_geometry_when_maximized() {
        let state = WindowState {
            width: 1180.0,
            height: 820.0,
            x: 100.0,
            y: 100.0,
            ..Default::default()
        };
        let merged = merge(state, Some(geometry(2560.0, 1440.0, 0.0, 0.0)), true);

        assert!(merged.maximized);
        assert_eq!(merged.width, 1180.0);
        assert_eq!(merged.height, 820.0);
        assert_eq!(merged.x, 100.0);
    }

    /// 读不到几何(最小化)时不应改动任何坐标。
    #[test]
    fn merge_without_geometry_keeps_previous_values() {
        let state = WindowState {
            width: 900.0,
            height: 700.0,
            x: 11.0,
            y: 12.0,
            ..Default::default()
        };
        let merged = merge(state, None, false);

        assert_eq!(merged.width, 900.0);
        assert_eq!(merged.x, 11.0);
        assert_eq!(merged.y, 12.0);
    }
}
