# Tauri 桌面版（Windows 便携版）实施计划

**目标：** 在不改动 `frontend/` 一行代码、不影响现有 Docker/服务端形态的前提下，把 reader-rust 打包成 Windows 单文件便携桌面应用。

**决策（已确认）：** 仅 Windows · 便携模式（exe 同级 `data/`）· 关闭鉴权（`SECURE=false`）

## 实施状态（2026-08-05）

Task 1–5 已全部完成并构建出便携包。已验证：

- `cargo check -p reader-desktop`：0 错误 0 警告
- `cargo test`：lib 42 项 + 集成测试（epub 4 / txt 10 / version_update 2 / ai_book_memory 等）全部通过，
  后端拆分未破坏既有行为
- 前端 72 个单测通过、`vue-tsc` 类型检查通过，`git diff frontend/` 为空
- `cargo build --release -p reader-desktop`：4 分 20 秒，产物
  `Reader.exe` 27 MB / 整包 29 MB / zip **11 MB**
- 冒烟测试（启动便携包 exe）：
  - `GET /health` → 200 `{"isSuccess":true,"data":"ok"}`
  - `GET /` → 200，前端 index.html 正常返回
  - 便携目录自动生成：`data/{desktop.json, reader.db, storage/assets}`，`desktop.json` = `{"port": 47892}`
  - 无 token 调用 `getBookSources` / `getBookshelf` → `isSuccess: true`，确认 `SECURE=false` 落到 default 命名空间

**仍需人工点验**（Task 6 中依赖 UI 交互的部分）：SSE 多源搜索、封面图显示、
本地书籍导入、TTS 朗读、外链跳系统浏览器、单实例聚焦、便携目录整体搬移。

### 补充：workspace 统一锁定导致 toml 降级（已决定接受）

加入 desktop crate 后共享 `Cargo.lock`，服务端的 `toml` 从 0.8.23 降到 0.8.2
（连带 `toml_datetime` 0.6.11→0.6.3、`toml_edit` 0.22.27→0.20.2）。冲突链：

```
tauri → gtk 0.18 → glib-macros → proc-macro-crate 2.0.2 → toml_datetime =0.6.3
reader-rust → config 0.14 → toml 0.8.23 → toml_datetime ^0.6.11
```

两者同属 0.6.x，Cargo 必须统一，`cargo update --precise` 无法绕过。根因是 tauri 的
Linux 专属依赖 gtk —— 在 Windows 上不参与编译，但 Cargo 生成 lockfile 时考虑所有平台。

影响为零：`config` 只挂了 `Environment` 一个 source（`src/app/config.rs:63`），
全仓库无任何 `config::File`，toml 运行时从不执行。已决定接受，不做 crate 隔离。

---

---

## 架构决策与依据

**最终形态：axum 与 Tauri 同进程运行，绑定 `127.0.0.1` 的固定端口，WebView 直接加载该 http 源。**

这不是随手选的，是被代码里三个硬约束逼出来的唯一解：

| 约束 | 证据 | 排除了什么 |
|---|---|---|
| 前端重度依赖 SSE | `EventSource` 用于多源搜索、书籍缓存、可用源探测（`frontend/src/api/search.ts:34,80,120`、`api/cache.ts:23`） | 排除 Tauri 自定义协议（`register_uri_scheme_protocol` 只能返回一次性完整响应，撑不起流式连接） |
| 大量根相对 URL 直接进 `<img src>` / `fetch` | `/reader3/cover?path=`（`api/bookshelf.ts:115`）、`/reader3/localEpubAsset`（同文件 111）、AI 地图 `/assets/ai-maps/*.png` | 排除「`tauri://localhost` 装前端 + 注入 API base」——那要改一堆 URL 拼接点，且 `initialization_script` 在 External URL 下不执行 |
| localStorage / IndexedDB 按 origin 隔离 | 7 个 key（`readConfig`/`reader-themeIndex`/`reader-speechConfig` 等）+ `utils/browserCache.ts` 的 IndexedDB | 排除「每次随机取空闲端口」——端口变了就是换 origin，阅读偏好会丢 |

选定方案的收益：

- **前端零改动**，`baseURL: '/reader3'` 与所有相对 URL 天然成立
- **单进程单二进制**（约 20–25 MB），没有 Electron 那种子进程孤儿/端口残留问题
- **Service Worker 自动失效**：`utils/pwa.ts` 的 `isLocalhostEnv()` 在 hostname 为 `127.0.0.1` 时会主动注销 SW 并清空 caches，桌面端不会被旧 shell 缓存卡住白屏 —— 白捡的
- **不触发防火墙弹窗**：显式绑 `127.0.0.1` 而非现有默认的 `0.0.0.0`
- 路由是 `createWebHashHistory`（`router/index.ts:1`），无需 SPA fallback

代价（需知悉）：本机其他进程可访问该回环端口。桌面单机自用场景可接受；若要收紧，可后续启用 `SECURE_KEY`。

**技术栈：** Tauri 2.11 · WebView2（本机已装 v151）· 复用现有 axum/sqlx/tokio 依赖 · Node 24 仅用于构建前端与打包脚本

---

## 前置条件（本机当前缺失）

- [ ] 安装 Rust 工具链：https://rustup.rs （本机 `cargo`/`rustc`/`rustup` 均未安装）
- [ ] 安装 MSVC Build Tools（`rquickjs`、`sxd-*` 需要本地编译）
- [x] WebView2 Runtime 已就绪（v151.0.4129.59）
- [x] Node 24.11.1 / npm 11.6.2 已就绪

---

## 文件结构

- 修改：`Cargo.toml`
  - 加 `[workspace] members = ["desktop/src-tauri"]` + `default-members = ["."]`，保证根目录 `cargo build` 与 Dockerfile 行为完全不变
- 修改：`src/app/bootstrap.rs`
  - 拆成 `build_state` / `serve_router` / `run`，使 axum 可被嵌入宿主进程
- 修改：`src/storage/db/mod.rs`
  - 让 `database_url` 同时接受裸文件路径，规避 Windows 盘符/空格无法通过 URL 解析的问题
- 新建：`desktop/package.json` — `@tauri-apps/cli` 与 dev/build 脚本
- 新建：`desktop/src-tauri/Cargo.toml` / `build.rs` / `tauri.conf.json` / `capabilities/default.json`
- 新建：`desktop/src-tauri/src/main.rs` — 主装配
- 新建：`desktop/src-tauri/src/portable.rs` — 便携数据目录解析 + 端口选取与持久化
- 新建：`desktop/src-tauri/icons/*` — 由 `frontend/public/icon-master-1024.png` 生成
- 新建：`scripts/build-desktop.mjs` — 组装便携包
- 不动：`frontend/**`（零改动）

---

## Task 1：后端可嵌入化重构

**Files:** `src/app/bootstrap.rs`、`src/storage/db/mod.rs`

- [ ] **Step 1: 拆分 bootstrap**

现有 `run()` 把「配置加载 → 日志初始化 → 建 state → 绑定 → 永久 serve」焊死在一起，且 `tracing_subscriber::fmt().init()` 二次调用会 panic。拆成：

```rust
pub struct ServerHandle {
    pub addr: SocketAddr,
    pub task: tokio::task::JoinHandle<()>,
}

/// 构建全部 service 与 AppState（含 DB 迁移、legacy 用户迁移）
pub async fn build_state(cfg: &AppConfig) -> anyhow::Result<AppState> { /* 原 run() 中段整体搬入 */ }

/// 绑定并在后台任务中 serve，立即返回真实 addr
pub async fn serve_router(app: Router, addr: SocketAddr) -> anyhow::Result<ServerHandle> {
    let listener = tokio::net::TcpListener::bind(addr).await?;
    let addr = listener.local_addr()?;
    let task = tokio::spawn(async move { let _ = axum::serve(listener, app).await; });
    Ok(ServerHandle { addr, task })
}

/// 服务端入口，行为与改造前完全一致
pub async fn run() -> anyhow::Result<()> {
    let cfg = config::load()?;
    init_tracing(&cfg.log_level);
    let state = build_state(&cfg).await?;
    let handle = serve_router(api::router::build_router(state),
                              SocketAddr::new(cfg.server_host.parse()?, cfg.server_port)).await?;
    tracing::info!("listening on {}", handle.addr);
    handle.task.await?;
    Ok(())
}
```

`init_tracing` 内部改用 `.try_init()` 并忽略「已初始化」错误。

- [ ] **Step 2: DB 路径兼容**

`SqliteConnectOptions::from_str` 走 URL 解析，`sqlite:D:\Reader Data\reader.db` 这类含反斜杠与空格的路径会直接解析失败。改为：

```rust
fn connect_options(database_url: &str) -> anyhow::Result<SqliteConnectOptions> {
    let opts = if database_url.starts_with("sqlite:") {
        SqliteConnectOptions::from_str(database_url)?     // 服务端旧行为不变
    } else {
        SqliteConnectOptions::new().filename(database_url) // 桌面端直接给绝对路径
    };
    Ok(opts.create_if_missing(true).foreign_keys(true))
}
```

- [ ] **Step 3: 回归验证**

`cargo test` 全绿；`cargo run` 仍能在 `0.0.0.0:8080` 起服务并正常打开前端。

---

## Task 2：Workspace 与 Tauri 骨架

**Files:** `Cargo.toml`、`desktop/**`

- [ ] **Step 1: workspace**

根 `Cargo.toml` 追加（`default-members` 是关键，否则 Docker 里 `cargo build` 会连带编译 Tauri 而失败）：

```toml
[workspace]
members = ["desktop/src-tauri"]
default-members = ["."]
```

- [ ] **Step 2: desktop crate**

`desktop/src-tauri/Cargo.toml` 依赖 `reader-rust = { path = "../.." }`、`tauri = { version = "2", features = [] }`、`tauri-plugin-single-instance`、`tauri-plugin-opener`、`serde_json`、`anyhow`、`tokio`。

- [ ] **Step 3: 图标**

`npx tauri icon frontend/public/icon-master-1024.png -o desktop/src-tauri/icons`（源图 1024px 已有，直接可用）。

- [ ] **Step 4: tauri.conf.json**

`productName: "阅读"`，`identifier: "com.givenge.reader"`，`build.frontendDist` 指向仓库内 `frontend/dist`（仅供 Tauri CLI 校验，运行时不走它），`build.devUrl: "http://localhost:5173"`，`build.beforeDevCommand: "npm --prefix ../../frontend run dev"`，`app.security.csp: null`。窗口在 `main.rs` 里动态创建，故配置中 `app.windows: []`。

---

## Task 3：便携数据目录与端口管理

**Files:** `desktop/src-tauri/src/portable.rs`

- [ ] **Step 1: 数据目录解析**

```
data_dir = <exe 所在目录>/data
```

写权限探测：尝试在 `data/` 写入临时文件，失败（典型场景：被解压进 `Program Files`）则回退到 `%LOCALAPPDATA%/reader/data` 并在首次启动时提示用户。目录布局：

```
data/
├── storage/            # STORAGE_DIR
│   ├── assets/         # ASSETS_DIR
│   └── cache/          # FileCache
├── reader.db           # SQLite
└── desktop.json        # { "port": 47892 }
```

- [ ] **Step 2: 端口选取**

读 `desktop.json` 里已保存端口；不可用则在 `47892..47912` 扫描首个可用端口并回写。选中后立刻交给 `serve_router` 绑定，避免检测与绑定之间的竞态。

同时接入 `tauri-plugin-single-instance`：第二次启动直接聚焦已有窗口，不再抢端口。

> 端口万一变化只会丢主题/字号/朗读设置等本地偏好；书架、阅读进度、书源均在 SQLite 服务端侧（`saveBookProgress` 走后端），不受影响。

---

## Task 4：主进程装配

**Files:** `desktop/src-tauri/src/main.rs`

- [ ] **Step 1: WebView2 预检**

`tauri::webview_version()` 失败时弹原生对话框提示安装 WebView2 Runtime 并打开下载页，而不是让 Tauri 直接 panic。

- [ ] **Step 2: 启动序列**

```rust
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

.setup(|app| {
    let paths = portable::resolve(&app.handle())?;      // data 目录 + 写权限回退
    let cfg = AppConfig {
        server_host: "127.0.0.1".into(),
        server_port: paths.port,
        database_url: paths.db.to_string_lossy().into_owned(),  // 裸路径，见 Task 1 Step 2
        storage_dir:  paths.storage.to_string_lossy().into_owned(),
        assets_dir:   paths.assets.to_string_lossy().into_owned(),
        web_root:     paths.web_root.to_string_lossy().into_owned(),
        secure: false,                                   // 关闭鉴权，落 default 命名空间
        ..AppConfig::default()
    };
    let state = tauri::async_runtime::block_on(bootstrap::build_state(&cfg))?;
    let handle = tauri::async_runtime::block_on(
        bootstrap::serve_router(reader_rust::api::router::build_router(state),
                                SocketAddr::from(([127,0,0,1], cfg.server_port))))?;
    let url = format!("http://127.0.0.1:{}", handle.addr.port());
    WebviewWindowBuilder::new(app, "main", WebviewUrl::External(url.parse()?))
        .title("阅读").inner_size(1180.0, 820.0).min_inner_size(880.0, 600.0).center()
        .build()?;
    Ok(())
})
```

`web_root` 取 `current_exe()` 同级的 `web/`（dev 下回退到仓库 `frontend/dist`）——不依赖 Tauri 的 resource 机制，便携包布局完全由我们的打包脚本决定。

若 `build_state` 实测超过 ~1.5s（DB 迁移 + rquickjs 引擎初始化），再补一个内嵌 splash 页，先建窗后 `navigate()`。先按简单版做，实测后再决定。

- [ ] **Step 3: 导航与外链拦截**

`AppTopBar.vue:48,53` 有两个 `target="_blank"` 外链，`SettingsDrawer.vue:496` 有 `window.open`。加：

- `.on_navigation(|url| url.host_str() == Some("127.0.0.1"))` —— 阻止应用被书源页面整个导航走
- `.on_new_window(...)` —— 交给 `tauri-plugin-opener` 用系统浏览器打开

---

## Task 5：便携包打包脚本

**Files:** `scripts/build-desktop.mjs`

- [ ] **Step 1: 流水线**

1. `npm --prefix frontend ci && npm --prefix frontend run build`
2. `cargo build --release -p reader-desktop`
3. 组装 `desktop/dist/reader-portable/`：`阅读.exe` + `web/`（= `frontend/dist` 全量）
4. 打成 `reader-portable-v<version>-win-x64.zip`

版本号从 `Cargo.toml` 的 `package.version` 读取，与 `frontend/package.json` 保持一致（当前均为 1.0.8）。

- [ ] **Step 2: 首次运行自检**

解压到一个普通用户目录 → 双击 exe → `data/` 自动创建 → 窗口打开且书架可用。

---

## Task 6：验收清单

- [ ] `cargo test` 与 `npm --prefix frontend test` 全绿，`git diff frontend/` 为空
- [ ] `cargo run`（服务端模式）与 `docker build` 行为不变
- [ ] 添加书源 → 搜索（**验证 SSE 多源搜索出结果**）→ 打开书 → 翻页 → 关闭重开，进度与书架保留
- [ ] 封面图正常显示（验证 `/reader3/cover` 相对 URL 链路）
- [ ] 导入本地 TXT/EPUB（验证 multipart 上传与 `/reader3/localEpubAsset`）
- [ ] TTS 朗读可用，`getVoices()` 能列出系统 SAPI 语音
- [ ] 点击顶栏 GitHub/文档链接 → 系统浏览器打开，应用窗口不被导航走
- [ ] 整个 `data/` 目录拷到另一台机器/另一个路径，应用行为一致（便携性验证）
- [ ] 二次双击 exe → 聚焦已有窗口，不启动第二个实例

---

## 已知风险

1. **WebView2 的语音数量少于 Edge**：微软出于成本在 WebView2 中禁用了 "Natural/Online" 云端语音（[WebView2Feedback#2660](https://github.com/MicrosoftEdge/WebView2Feedback/issues/2660)），`getVoices()` 只会返回本地 SAPI 语音。Electron 同样拿不到 Edge 云端语音，两者大致持平，但需在你的机器上实测确认可接受。兜底：项目已内置 OpenAI 兼容 TTS 通道（`frontend/src/utils/openaiSpeech.ts`）。
2. **便携模式写权限**：解压到 `Program Files` 会失败，已用回退逻辑兜住（Task 3 Step 1）。
3. **端口被占**：47892–47912 区间内扫描 + 持久化；极端情况下端口变化会重置本地 UI 偏好，不影响书库数据。
4. **首次构建耗时**：Tauri + 现有 40 余个 crate 冷编译预计 5–15 分钟。

## 非本次范围

- macOS / Linux 出包（Linux 下 WebKitGTK 的 `speechSynthesis` 为 undefined，需引入 `tauri-plugin-tts` 另行设计）
- 用 Tauri updater 替换现有 `update_service.rs` 的 GitHub Release 检查（当前逻辑在桌面端仍可用，只是不能自动安装）
- 系统托盘、全局快捷键、原生菜单
