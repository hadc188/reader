# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Build and Run Commands

### Rust 库（reader-rust）

```bash
cargo test -j 4        # 单测 + 集成测试。必须低并发，见下方「杀软并发竞态」
cargo test --lib -j 4  # 只跑库单测
cargo build -p reader-desktop -j 4   # 编译桌面壳（debug）
```

### Web Frontend (Vue 3)

```bash
cd frontend
npm install            # 安装依赖（含 @tauri-apps/api）
npm run dev            # Vite dev server（仅配合 tauri dev 使用）
npm run build          # vue-tsc 类型检查 + 生产构建 → dist/
npm test               # vitest 单测
```

### Desktop App (Tauri, Windows only)

```bash
cd desktop
npm install
npm run dev            # tauri dev：Vite HMR + 桌面窗口，数据在 desktop/.dev-data/
npm run build          # 便携包 zip 在 desktop/dist/
npm run icon           # 仅重新生成图标；图标已提交
```

## 架构（纯单机桌面，无 HTTP 服务端）

reader-rust 是「阅读3.0」的 Rust 实现，当前形态为 **Tauri v2 桌面应用**：无 axum、无 Docker、无多用户。前端与后端同进程，通过 IPC 通信。

### 模块结构

- **`src/api/commands/`** — 全部 `#[tauri::command]`（原 axum handler 原地转型）。返回 `Result<ApiResponse<T>, AppError>` 信封。
- **`src/api/protocol.rs`** — 自定义 URI 协议 `reader`（origin `http://reader.localhost`）：`/cover`、`/epub`、`/files`、`/bookSourceProxy`（登录代理 HTML 重写）、`/bookSourceClientLog`。
- **`src/api/mod.rs`** — `AppState`（10 个 Arc service）+ `invoke_handler()` 聚合注册所有 command。
- **`src/service/`** — 业务逻辑（book / book_source / book_group / local_txt / local_epub / json_document / ai_book / ai_model / update / user）。
- **`src/parser/`** — 内容提取引擎（CSS/XPath/JSONPath/Regex/JS 模式自动识别）。
- **`src/crawler/`** — reqwest HTTP 抓取 + URL 分析。
- **`src/storage/`** — SQLite（sqlx + migrations）、文件缓存、存储目录。
- **`src/error/error.rs`** — `ApiResponse<T>` 信封 + `AppError`（自定义 `Serialize`，序列化为消息字符串，供 Tauri reject）。
- **`desktop/src-tauri/`** — Tauri 壳：窗口、`reader` 协议注册、便携数据目录解析、WebView2 预检、单实例。

### 前后端契约

- 前端 `frontend/src/api/invoke.ts` 将旧 `/reader3/*` 路径映射到 command 名，解包 `{isSuccess, errorMsg, data}`；`get/post` 返回 `{data}` 形状（保留原 axios 调用模式）。
- SSE 流（多源搜索/缓存进度/可用源探测/书源调试）走 `tauri::ipc::Channel`，前端 `api/sse.ts` 的 `openSse()` 提供 EventSource 兼容表面；payload 含 `event: "data"|"end"|"error"` 判别字段。
- 二进制响应（`ai_proxy`/`ai_proxy_image`/`get_webdav_file`）返回 `{status, contentType, body: Vec<u8>}`，前端重建 DOM `Response`。上传传 `Uint8Array`/`Vec<u8>`（类型化数组，绝不 JSON 数字数组）。
- command 参数默认 camelCase 匹配（Rust `on_event` ↔ JS `onEvent`）。
- 单用户：无登录/鉴权，全部数据在 `"default"` 命名空间。

### 书源格式

书源是 JSON 对象：`bookSourceUrl`/`bookSourceName`/`searchUrl`/`exploreUrl` + `ruleSearch`/`ruleBookInfo`/`ruleToc`/`ruleContent` 规则。规则前缀：`@css:`/`@json:`/`@xpath:`/`@regex:`/`js:`（rquickjs 沙盒执行）。

## 已知注意事项

- **杀软并发编译竞态**：Windows 杀软实时扫描与 cargo 并行写 `.rlib` 冲突（随机 `invalid metadata files` / `link.exe 0xC000012D`）。**构建和测试务必用 `-j 4` 或更低并发**。建议给 `target/` 加 Defender 排除。
- **`toml` 版本钉在 0.8.2**：共享 lockfile 因 `tauri -> gtk -> proc-macro-crate` 约束无法升级。无害（`config` 只用 Environment source，运行时从不解析 TOML）。不要尝试修复。
- **桌面 origin 迁移**：`http://127.0.0.1:*` → `http://tauri.localhost` 后 localStorage 偏好重置一次；书库数据（SQLite/文件）不受影响。
