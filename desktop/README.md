# 阅读 桌面版（Windows 便携版）

reader-rust 的纯单机桌面形态：**无 HTTP 服务端、无 Docker、无多用户**。前端与后端跑在同一个 Tauri 进程中，通过 IPC（`#[tauri::command]`）通信。

## 架构

```
┌───────────────────────── 单进程 ─────────────────────────┐
│ WebView2 (Vue 3 SPA)          │                          │
│   origin: http://tauri.localhost                          │
│   api/*.ts ── invoke() ──▶ tauri::ipc ──▶ commands/*.rs   │
│   SSE ──▶ tauri::ipc::Channel                              │
│   <img>/<iframe> ──▶ http://reader.localhost/... (自定义协议)│
│                              │                          │
│   Rust 核心（reader-rust 库）                              │
│   build_state() → AppState（10 个 service，全部 Arc）      │
└──────────────────────────────────────────────────────────┘
```

- **前端资源**：Tauri 前端资源协议托管 `frontend/dist`（`frontendDist: "../frontend/dist"`）。
- **IPC**：所有旧 `/reader3/*` 路由变成本地 command（`reader_rust::api::invoke_handler()` 聚合注册）。前端 `api/invoke.ts` 把路径映射到 command 名并解包 `{isSuccess, errorMsg, data}` 信封。
- **SSE → Channel**：多源搜索 / 缓存进度 / 可用源探测等 5 个流改用 `tauri::ipc::Channel`，前端 `api/sse.ts` 的 `openSse()` 保留 EventSource 表面。
- **二进制/资源**：封面（`/cover`）、EPUB 资源（`/epub`）、上传文件（`/files`）、书源登录代理（`/bookSourceProxy`）由自定义 URI 协议 `reader`（origin `http://reader.localhost`）承载，`<img>`/`<iframe>` 保持同步加载。
- **数据目录**：`<exe 所在目录>/data/`（便携模式；只读目录回退 `%LOCALAPPDATA%\reader\data`）。
- **单用户**：无登录/鉴权，所有数据落在 `"default"` 命名空间。

## 开发

```bash
cd desktop
npm install          # 装 @tauri-apps/cli
npm run dev          # tauri dev：Vite HMR + debug 窗口，数据在 desktop/.dev-data/
```

## 构建

```bash
npm run build        # 产物 desktop/dist/reader-portable-v<版本>-win-x64.zip
```

流程：`npm ci` + `npm run build`（frontend）→ `cargo build --release -p reader-desktop` → 组装 `Reader.exe` + `web/` → 压缩。

## 已知限制

- **杀毒软件并发编译竞态**：实时扫描会与 cargo 并行写 `.rlib` 冲突（表现为随机 `invalid metadata`/`link.exe 0xC000012D`）。构建/测试请用 `-j 4` 或更低并发；建议给 `target/` 目录加 Defender 排除。
- **origin 迁移**：从旧 HTTP 版（`http://127.0.0.1:47892`）首次启动，localStorage 偏好（主题/字号/朗读设置等）会重置一次；书架、进度、书源在 SQLite/文件里，不受影响。
- **WebView2 语音**：`getVoices()` 只有本地 SAPI 语音（无 Edge 云端 "Natural" 语音）。OpenAI 兼容 TTS 通道可用。
- **登录预览 iframe**：登录页通过 `bookSourceProxy` 转发；Cookie 只保存在后端按用户和完整书源地址隔离的网络客户端中，不写入 `reader.localhost` 浏览器 Cookie。代理仅允许访问当前书源域名及其子域，切换代理后登录预览会话需要重新打开。
