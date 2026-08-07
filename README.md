# Reader

「阅读3.0」的 Rust 实现，当前形态为 **Tauri v2 纯单机桌面应用**：无 HTTP 服务端、无 Docker、无多用户。前端（Vue 3）与后端（Rust）跑在同一个进程中，通过 IPC 通信，打包成单个可执行文件。

源自 [reader](https://github.com/hectorqin/reader)，参考 [reader-rust](https://github.com/givenge/reader-rust) 重构。

## 免责声明

本项目仅提供书源管理、内容解析、阅读与缓存等技术能力，不内置、存储、分发或提供任何受版权保护的书籍内容。用户应确保自行添加的书源、上传的本地文件以及通过本服务访问的内容均已获得合法授权，并自行承担由此产生的版权与合规责任。

如任何权利人认为本项目相关内容或使用方式侵犯了其合法权益，请通过项目 Issues 联系维护者，我们将在核实后及时处理。

## 功能特性

- 自定义书源支持（JSON 书源格式）
- CSS 选择器、JSONPath、XPath、正则、JavaScript 多种解析方式
- 自动化规则引擎
- 多源书籍搜索，结果按相关性排序并按书名/作者合并、显示来源数量
- 详情页可切换书源、加入书架
- 书籍目录获取、章节内容缓存
- 书源编辑器与调试器（原始响应 / 各步骤解析结果）
- 书架、最近阅读、阅读进度记录
- 阅读统计（echarts 图表）
- 本地 TXT / EPUB 阅读
- RSS 订阅支持
- WebDAV 同步
- TTS 语音朗读（本地 SAPI + OpenAI 兼容 TTS）

## 技术栈

- 桌面壳：Tauri v2（WebView2）
- 后端：Rust + reqwest + sqlx (SQLite) + rquickjs + tokio
- 前端：Vue 3 + Vite + TypeScript + Pinia + Vue Router

## 安装

从 [Releases](https://github.com/hadc188/reader/releases) 下载 Windows 安装包（`*-setup.exe`）或便携版（`*-win-x64.zip`）。

便携版解压后直接运行 `Reader.exe`，数据默认存放在 exe 同级的 `data/` 目录（只读目录时回退到 `%LOCALAPPDATA%\reader\data`）。

## 开发

```bash
# 前端（Vue 3）
cd frontend
npm install
npm run dev          # Vite dev server（配合 tauri dev 使用）
npm run build        # vue-tsc 类型检查 + 生产构建
npm test             # vitest 单测

# 桌面应用（Tauri，Windows only）
cd desktop
npm install
npm run dev          # tauri dev：Vite HMR + 桌面窗口，数据在 desktop/.dev-data/
npm run build        # 打包产物在 desktop/dist/
```

Rust 库单测 / 集成测试：

```bash
cargo test -j 4      # 必须低并发，见下方「杀软并发竞态」
```

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
│   AppState（各 service，全部 Arc）                        │
└──────────────────────────────────────────────────────────┘
```

- **`src/api/commands/`** — 全部 `#[tauri::command]`（原 axum handler 原地转型）。
- **`src/api/protocol.rs`** — 自定义 URI 协议 `reader`（origin `http://reader.localhost`）。
- **`src/service/`** — 业务逻辑（book / book_source / ai_book / update / user 等）。
- **`src/parser/`** — 内容提取引擎（CSS/XPath/JSONPath/Regex/JS 模式自动识别）。
- **`src/crawler/`** — reqwest HTTP 抓取 + URL 分析。
- **`src/storage/`** — SQLite（sqlx + migrations）、文件缓存、存储目录。
- **`desktop/src-tauri/`** — Tauri 壳：窗口、`reader` 协议注册、便携数据目录解析、WebView2 预检、单实例。

## 书源格式

书源是 JSON 对象：`bookSourceUrl` / `bookSourceName` / `searchUrl` / `exploreUrl` + `ruleSearch` / `ruleBookInfo` / `ruleToc` / `ruleContent` 规则。规则前缀：`@css:` / `@json:` / `@xpath:` / `@regex:` / `js:`（rquickjs 沙盒执行）。

## 已知注意事项

- **杀软并发编译竞态**：Windows 杀软实时扫描与 cargo 并行写 `.rlib` 冲突（随机 `invalid metadata files` / `link.exe 0xC000012D`）。**构建和测试务必用 `-j 4` 或更低并发**。建议给 `target/` 加 Defender 排除。
- **`toml` 版本钉在 0.8.2**：共享 lockfile 因 `tauri -> gtk -> proc-macro-crate` 约束无法升级。无害（`config` 只用 Environment source，运行时从不解析 TOML）。不要尝试修复。

## 许可

MIT
