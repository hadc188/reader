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

## 架构（Tauri 桌面为主 + 可选的进程内 Web 服务）

reader-rust 是「阅读3.0」的 Rust 实现，主形态是 **Tauri v2 桌面应用**：前端与后端同进程，通过 IPC 通信，无 Docker、无多用户。

另有**可选的 Web 阅读模式**（`src/server/`）：在桌面进程内起一个 axum 服务，让手机浏览器读同一个书架。默认只监听 `127.0.0.1:1122`，可在设置里放开到局域网。**没有鉴权**（与 Legado 手机端一致）——放开到局域网即等于同网段可读，UI 与 `docs/web-reader.md` 都写明了这一点。详细说明见那份文档。

### 模块结构

- **`src/api/commands/`** — 全部 `#[tauri::command]`（原 axum handler 原地转型）。返回 `Result<ApiResponse<T>, AppError>` 信封。**每个命令都配一个 `xxx_core(&AppState, req)`**，Web 端 HTTP handler 只调 core，不复制业务分支 —— 新增接口时先抽 core 再挂路由。
- **`src/api/protocol.rs`** — 自定义 URI 协议 `reader`（origin `http://reader.localhost`）：`/cover`、`/epub`、`/pdf`、`/files`、`/bookSourceProxy`（登录代理 HTML 重写）、`/bookSourceClientLog`。浏览器里同一批路径由 `src/server/router.rs` 以同源路由提供。
- **`src/server/`** — Web 阅读模式：`router.rs`（路由表，handler 全为 core 的薄适配）、`assets.rs`（静态资源 + SPA 回退）、`settings.rs`（`web-settings.json`，原子写）、`control.rs`（启停/状态/配置应用，IPC 命令）。
- **`src/util/path_safe.rs`** — 路径校验的唯一实现，`/files` 与静态资源都用它（Windows 上 `..\..\` 必须按反斜杠分段拦）。
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

- **简繁转换用 legado 同款词表，别换回 OpenCC 预设**（`frontend/src/utils/chinese.js` + `legadoT2sDict.js`/`legadoS2tDict.js`，由 `scripts/build-legado-chinese-dicts.mjs` 从 quick-transfer-core 0.2.17 的 `tc/t2s.txt`、`tc/s2t.txt` 生成，t2s 侧合并了 legado `ChineseUtils.fixT2sDict()` 的排除表）。OpenCC 的 preset 与手机端**不是同一份词表**：`twp` 会把「序列」变「串行」（#12），`tw` 的 TSCharacters 把「乾」无条件映射成「干」，于是「乾國」→「干国」（#25）；而 legado 词表里「乾」没有单字映射，只在「乾燥/乾淨/乾杯」等短语里转换。两端显示不一致才是 bug，因此这里逐字照搬手机端词表（CJK 基本区 + GB2312 全表做过逐字比对）。`chineseMode` 有 `off`/`simplified`/`traditional` 三档，`off` 不加载那 ~1.2MB 词典分块。
- **杀软并发编译竞态**：Windows 杀软实时扫描与 cargo 并行写 `.rlib` 冲突（随机 `invalid metadata files` / `link.exe 0xC000012D`）。**构建和测试务必用 `-j 4` 或更低并发**。建议给 `target/` 加 Defender 排除。
- **不要在测试里构造 `server::assets::StaticSource`**：它带 `tauri::AppHandle`，链接器会把 tauri 的 `TaskDialogIndirect`（comctl32 v6）拉进测试 exe 的导入表；而 cargo 生成的测试 exe 没有应用 manifest，只加载得到 comctl32 v5，进程会在进 `main` 之前以 `STATUS_ENTRYPOINT_NOT_FOUND (0xc0000139)` 退出 —— 输出里没有任何 Rust panic，看起来像"链接器坏了"。`assets.rs` 因此把路径判定抽成不碰 tauri 的 `resolve_lookup()` 纯函数来测。
- **`ring` 的 C 编译会随机失败**：报 `cl.exe ... D8050: cannot execute c1.dll: failed to get command line into debug records`。同样是杀软竞态，但表现不同 —— 它发生在 `ring` 的 C 源码（`crypto/curve25519/curve25519.c`）编译阶段，重启一次通常就好。想让它稳定可复现，构建时加 `CARGO_PROFILE_DEV_DEBUG=0` 关掉调试信息（debug 构建不需要逐行调试依赖）。
- **本机 `%TEMP%` 可能被组策略/沙箱挡住写入**：`std::env::temp_dir()` 下的 `create_dir_all` 会返回 `PermissionDenied (os error 5)`，于是所有用临时目录的测试（`app::window_state`、`export::epub`、`service::book_group_service`、`service::http_tts_service` 等约 48 个）集体失败。**这不是代码 bug**。绕开办法是把 `TEMP`/`TMP` 指到工作区内可写目录再跑测试：
  ```powershell
  $env:TEMP="$PWD\.cargo-tmp"; $env:TMP=$env:TEMP
  New-Item -ItemType Directory -Force -Path $env:TEMP | Out-Null
  cargo test -p reader-rust --lib -j 2
  ```
- **启动隔离靠「按数据目录派生 identifier」**：`portable::resolve()` 为每个数据目录算一个 `instance_identifier`，`main.rs` 在建窗前把它写进 `context.config_mut().identifier`，并在 Windows 上把 WebView2 的 profile 显式指到 `<数据目录>/webview2`。单实例互斥体 `{identifier}-sim` 因此也跟着分开 —— 「开发版 + 安装版」以及「同一份程序拷两份」都能同时运行，不会再出现 `0x800700AA`（请求的资源在使用中）导致的"窗口永远不出现"。改这块时**不要**再按 debug/release 特判单实例：identifier 已按数据目录分开，无条件注册才是对的。
- **净化规则的 `@js:` 脚本替换**：Legado 的 `replacement` 可以是以 `@js:` 开头的脚本（输入绑定 `result`，**取最后一个表达式语句的值**，等价 Rhino 的 `eval` 语义，脚本里通常没有 `return`）。桌面版在 `src/parser/replace_rule_js.rs` 的 rquickjs 沙盒里执行，**绝不要挪到前端** —— 渲染进程里跑第三方脚本等于把 DOM 与 `__TAURI_INTERNALS__` 交出去。前端只在**存在脚本规则**时才发一次 `apply_replace_rules`，纯正则/字面量规则仍走同步快路径（`displayContent` 先给同步结果，沙盒返回后自动替换）。
- **Legado 正则要用 Unicode 的 `\w`**：`utils/replaceRuleRegex.ts` 会把 `\w` / `\W` 展开成显式 Unicode 字符类。这不是可选项：规则里 `[^\n\w…]{4,}`、`(?<=\n)\W+$` 这类写法按"`\w` 含汉字"写的，若沿用 JS 的 ASCII `\w`，**整段中文正文会被当成符号删掉**（实测 `#07 标点——`、`#15 净化词语` 会输出空正文）。`\b` 仍是 JS 的 ASCII 语义，属已知近似。
- **`##匹配##替换` 在详情页字段上也要生效**：`ruleBookInfo` 字段走 `eval_field_html_doc_with_ctx`（整页求值），这条分支早先只 `extract_js`、没切 `##`，于是 `…@text##作\s*者[：:]\s*` 整条被当成 CSS 选择器 → 选不到 → 作者/分类恒为空。**「详情页书名有、作者没有」就是这个坑**。新增字段求值分支时，`##` 必须走 `split_legado_regex` + `apply_legado_regex`，与 `eval_field_html_with_ctx` 保持一致。
- **HTML5 解析器会丢掉「无上下文」的表格/列表标签**：把 `tr` / `td` / `li` 这类元素快照还原成 HTML 再 `Html::parse_document` 时，没有 `<table>`/`<ul>` 祖先的标签会被整段丢弃（`body` 里只剩文本）。书源 `bookList` 交回 `<tr id="nr">` 列表时，字段规则 `td:nth-of-type(1) a@text` 因此恒为空 —— 症状正是「搜索有结果、解析 0 条」。`parser::html::parse_document` 现在按首个标签自动补上下文并打 `data-reader-fragment-wrap` 标记，`document_top_elements()` 再把补上去的祖先跳过；**改这块时不要退回裸 `Html::parse_document`**，也不要用 `body > *` 直接取顶层元素（会只看到补上的 `<table>`）。
- **书源请求的首部必须按浏览器写法发**：Cloudflare 的托管挑战对某些站点会按首部**大小写 + `Host` 位置**判客户端 —— `Host` 在第一位、其余 Title-Case 返回 200；hyper 默认的「全小写 + `Host` 最后」直接 `403 cf-mitigated: challenge`（实测，逐项二分确认；与 TLS 指纹、`Accept-Encoding`、HTTP/2 都无关）。因此 `build_source_client*` 都走 `build_source_builder`（`http1_title_case_headers` + `http1_only`），`fetcher::fetch_inner` 还会显式注入排在首位的 `Host`（书源显式写过就保留它的值）。**别把 `http1_title_case_headers` 去掉**：失效表现是「手机端正常、桌面端整页被拦」，403 页面里只有 `Just a moment`。
- **JSoup 元素在桌面端是可变的**：`js.rs` 的 `wrapElement` 带 `__owner`（宿主元素），`select()` 会把宿主关系传给子元素，`remove()` 才能真的从宿主 `innerHtml` 里摘掉自己；`clone()` 是深拷贝、不影响原文档。`org.jsoup.select.Elements` / `org.jsoup.nodes.Element` 是**可构造**的（书源 jsLib 用 `new` + `appendChild` 手工拼搜索结果卡片）；缺 `org.jsoup.select` 这个子包时 jsLib 直接抛 `cannot read property 'Elements' of undefined`，搜索阶段就会失败。
- **元素必须自带 `toJSON`，内部字段必须不可枚举**：书源的 `bookList` / `chapterList` 脚本常写 `var out = []; out.push(li); return out;` —— 交回宿主的是 `JSON.stringify(out)`。元素若没 `toJSON`，每项会变成 `{"__snapshot":{…},"__owner":null,…}` 包装：宿主认不出这是元素快照（顶层缺 `outerHtml`），字段规则 `span.s2 a@text` 全部取空，症状是**发现页/搜索页 0 条**（用 `Elements` 集合时被 `elementsApi.toJSON` 兜住了，裸数组就漏）。所以 `wrapElement` 里挂了 `toJSON: () => s`，并且 `__snapshot` / `__owner` / `__prevOuter` 都用 `defineProperty` 设成 `enumerable: false`。**新增元素内部字段时一定要走这个模式**，否则会被序列化出去。
- **在线朗读引擎的 URL 会带重复查询参数**：在线集合里 71/84 条引擎把 `voiceId` 写了两遍（基础地址自带占位音色，末尾再追加真实音色），而 dds 网关对重复参数直接回 `400 Bad Request`。`src/parser/http_tts_template.rs` 的 `normalize_url` 会在**字符串层面**去重（同名保留最后一个、位置沿用首次出现），不要换成 `Url::query_pairs_mut` 重建——那会解码再编码，把模板要求的 `text=%25E4...` 双重编码弄变形。
- **旁白/对话切分是两层启发式，顺序不能反**：`utils/speechSegments.ts` 先认成对中文引号（`“”`/`「」`/`『』`），再认「短说话人前缀 + 全角冒号」（`张三：`、`他笑道：`）。冒号规则必须**保守**：前缀 ≤20 字、冒号后紧跟引号时不切、数字开头与 `时间：`/`注意：` 这类标签不切——放宽会让叙述里的冒号整段变成对话音色。`utils/speechChunks.ts` 必须**先切旁白/对话段、再把每段按 ≤70 字切块**；反过来先按句切块再逐块认引号，长对话的后续小块会丢引号上下文（症状：一句长对话前半截对话音色、后半截突然变旁白）。跨段对话（上一段只有开引号、下一段只有闭符）靠 `splitSpeechSegmentsDetailed` 的 `openCloser` 在段落之间传递。
- **朗读要跟随用户手动跳章**：`useReaderAutoPlayback` 里 watch `store.currentIndex`，非 TTS 自己发起的切章（目录 / 上下一章 / 拖进度条 / 连续阅读滚过章边界）就停掉旧流、从新章开头续读，否则缓冲流会继续播旧章音频直到流自己跨章才发现索引不符。TTS 自身的跨章调用必须包在 `runSpeechDrivenChapterChange()` 里，`showParagraph()` 会刷新 `lastSpeechOwnScrollAt` —— 少了这两条，跟随 watcher 会把朗读自己的滚动/推进当成用户操作而反复重启。
- **`legado://` 一键导入**：书源站点的「一键导入」按钮是 `legado://import/<kind>?src=<url>`，只有注册了该协议的客户端会被系统唤起。协议由 **NSIS 安装器**在 `windows-hooks.nsh` 的 `NSIS_HOOK_POSTINSTALL` 里写入 `Software\Classes\legado`（卸载时按归属清理），所以**只有安装版**有；便携包解压即用、不经过安装器，拿不到协议归属。同一协议只能指向一个 exe，后安装的覆盖先安装的。
  - 接收端：`main.rs` 冷启动时从 `std::env::args()` 取 URL 暂存，运行中被再次唤起则走 single-instance 回调（args 转发）并发 `legado-deeplink` 事件；前端 `App.vue` 取暂存后交给 `utils/legadoImport.ts` 分发到已有的导入通道（书源 / RSS / 净化规则 / 朗读引擎）。主题、排版等桌面版没有的实体只提示不支持。
  - 运行中唤起在 debug 与 release 都可用：单实例插件是无条件注册的（identifier 已按数据目录分开，见上一条），第二个进程只会把参数转发给已运行实例。
- **WebView2 版本漂移是唯一的真实运行时风险**：默认打包用 `downloadBootstrapper`（安装时按需下载/升级），用户内核版本由系统决定，渲染与媒体解码行为可能不一致。需要锁死内核时，用 `READER_FIXED_WEBVIEW2=<解压后的 Fixed Version 目录>` 跑 `npm run build`，`scripts/build-desktop.mjs` 会切到 `fixedRuntime`（安装包 +约 180MB）。默认不开启。
- **`toml` 版本钉在 0.8.2**：共享 lockfile 因 `tauri -> gtk -> proc-macro-crate` 约束无法升级。无害（`config` 只用 Environment source，运行时从不解析 TOML）。不要尝试修复。
- **`pdf-extract` 是 vendor 副本**（`vendor/pdf-extract`，经 `[patch.crates-io]` 接线），三处改动，都别退回上游行为：
  - **中文 CMap**：上游遇到无 ToUnicode 的 `GBK-EUC-H` 等 Type0 编码会 panic、嵌入式 GB CMap 的分段 `begincidrange` 解析失败。副本加了「恒等字节映射 + `encoding_rs` 逐码位解码伪 ToUnicode 表」的降级路径。
  - **字体缓存键**：上游按资源名（`F1`）缓存字体，而资源名只在单页内唯一 —— 一个 `Processor` 跨页复用时会串字体，表现为大段文字凭空消失。副本改为按字体对象 id 缓存。
  - **`extract_text_for_page_ids`**：按显式页对象 id 提取并**共享一个 `Processor`**。上游的逐页接口每页重建解析器，1318 页中文书要 70s+；共享后同样内容 1.4s（字节级一致，已逐页比对验证）。
- **`tao` 用官方 crates.io 版本，不要再 vendor**：曾因 [tauri-apps/tao#1353](https://github.com/tauri-apps/tao/pull/1353)（无边框窗口在**最大化状态**下进全屏时，`WM_NCCALCSIZE` 把客户区钳到显示器工作区 `rcWork`，于是底部露出任务栏高度的黑边）未发布而维护过一份 `vendor/tao` 副本（给 `src/platform_impl/windows/event_loop.rs` 里 `util::is_maximized(window)` 那个判断加 `&& !is_fullscreen`）。
  - **该修复已随 `tao 0.37.2` 发布**（2026-10-23，CHANGELOG 明确列出 #1353），`vendor/tao` 与 `[patch.crates-io]` 里的 `tao` 条目已删除，现在走官方版本。
  - **升级约束**：`tao` 由 `tauri-runtime-wry` 引入，版本必须与 `tauri` 配套升级。`tauri 2.11.x` 的 `tauri-runtime-wry` 要求 `tao = "0.35"`，换不到 0.37；`tauri 2.12.x`（`tauri-runtime-wry 2.12.3`）才放宽到 `tao = "0.37"`。想升 tao 就得一起升 tauri，别只改 `Cargo.toml` 里的 `tao`。
  - **收益**：前端不必用「进全屏前先 `unmaximize`、退出后再 `maximize`」的变通。那个变通也能绕开黑边，但两次 `ShowWindow`（`SW_RESTORE` / `SW_MAXIMIZE`）都由 tao 异步派发到事件循环线程，退出全屏时会先看到一次窗口化动画、再来一次最大化动画。
- **本地 PDF 管线**（`src/service/local_pdf_book.rs` + `frontend/src/utils/pdfPages.ts`）：阅读形态与 Legado 的 `PdfFile` 一致——**按页渲染**，不做全文重排。导入只解析页数、书签、封面与 `/Info` 元数据，原文件落盘为书目录下的 `book.pdf`；章节正文是每页一个 `<div class="pdf-page" data-pdf-page="N">` 占位，前端经 `reader://` 的 `/pdf?bookUrl=` 取整本 PDF，用 pdf.js 逐页画到 canvas（扫描件、复杂版式都能原样显示）。目录优先用 PDF 书签（`/Root/Outlines`，深度优先扁平化），无书签时从正文认「第X回/章」标题（`detect_headings`），都没有才按 20 页/章分段；书签章超过 `PAGES_PER_CHAPTER` 页会再细分并加 `(1/n)` 序号，避免整本只标一个书签时出现上百页的巨型章节。`collect_page_ids` 仍是页序的唯一来源。本地 PDF 强制单章模式：不走左右翻页（会把整页图切块），也不进连续阅读（页渲染按单章创建与释放，换章要回收上一章的 canvas）。`EXTRACTOR_VERSION` 5，更早的文本缓存需重新导入同一文件升级。

- **桌面 origin 迁移**：`http://127.0.0.1:*` → `http://tauri.localhost` 后 localStorage 偏好重置一次；书库数据（SQLite/文件）不受影响。

