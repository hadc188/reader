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
- **macOS 的数据目录绝不能在 `Reader.app` 里**：macOS 版固定用 `~/Library/Application Support/reader/data`，其它平台仍是 exe 同级 `data/`（见 `portable::release_data_dir`）。历史原因：可执行文件在 `Contents/MacOS/` 下，早先按「exe 同级」落盘，于是书库、书源和本地备份全住进了应用包，DMG / app-zip 的升级方式是整包替换，每次升级必然清空（issue #32）。两条迁移路径都只在目标位置缺 `reader.db` 或首次迁移时执行，且**只复制不删除**：`.app` 内增删任何文件都会让代码签名失效，macOS 随后直接拒绝启动；删除逻辑只在 Windows 的 `%LOCALAPPDATA%` 回退目录上有（那份目录不在签名范围内）。改这段时不要为了"清理干净"把 macOS 侧改成 `remove_dir_all`。
- **净化规则的 `@js:` 脚本替换**：Legado 的 `replacement` 可以是以 `@js:` 开头的脚本（输入绑定 `result`，**取最后一个表达式语句的值**，等价 Rhino 的 `eval` 语义，脚本里通常没有 `return`）。桌面版在 `src/parser/replace_rule_js.rs` 的 rquickjs 沙盒里执行，**绝不要挪到前端** —— 渲染进程里跑第三方脚本等于把 DOM 与 `__TAURI_INTERNALS__` 交出去。前端只在**存在脚本规则**时才发一次 `apply_replace_rules`，纯正则/字面量规则仍走同步快路径（`displayContent` 先给同步结果，沙盒返回后自动替换）。
- **Legado 正则要用 Unicode 的 `\w`**：`utils/replaceRuleRegex.ts` 会把 `\w` / `\W` 展开成显式 Unicode 字符类。这不是可选项：规则里 `[^\n\w…]{4,}`、`(?<=\n)\W+$` 这类写法按"`\w` 含汉字"写的，若沿用 JS 的 ASCII `\w`，**整段中文正文会被当成符号删掉**（实测 `#07 标点——`、`#15 净化词语` 会输出空正文）。`\b` 仍是 JS 的 ASCII 语义，属已知近似。
- **净化规则的「替换为」是 Java 替换模板，不是纯文本**：手机端走 `Matcher.appendReplacement(buf, replacement)`，所以 `$1` / `${name}` 是捕获组引用、`\$` 是字面量 `$`、`\x` 是转义（**`\1` 得到字符 `1`，不是分组引用**），裸 `$` 与越界组号直接抛异常（整条规则不生效）。`utils/replaceRuleRegex.ts` 的 `toJsReplacement(replacement, captureGroups(...))` 负责翻译成 JS 替换串：组号解析与 JDK 一致（两位超范围逐位退让：`$99` → 组 9 + `9`，`$0005` → 组 5），字面量 `$` 转 `$$`，一位组号后紧接数字改写 `$0n` 防 JS 吃成两位分组，无法解析时返回错误而不是静默插入错误正文。**同步路径与 Rust 沙盒路径共用这一个函数**，改语义时两边一起变。
- **净化规则的范围与作用目标是四条独立判定，别合并**：`utils/replaceRuleScope.ts` 按手机端 DAO 的 SQL 语义实现 —— `scope` / `excludeScope` 都是**包含**匹配（`scope LIKE '%书名%' or scope LIKE '%书源%'`），空串与 `*` 表示全部书籍，桌面端自己的 `book:` / `source:` 前缀仍保持精确相等（那是「按本书/按书源替换」写下的约定，不能放宽）。**排除优先于生效范围**，且 `excludeScope` 的方向是「排除串里要出现完整书名或书源」，不是反过来。作用目标由 `scopeContent` / `scopeTitle` 两个开关决定，缺省按手机端实体默认（正文开、标题关）——缺省不能当成 `false`，否则老规则一保存就把正文规则关掉。标题净化走 `store.displayChapterTitle` / `displayTitleOf()`，**只改显示不改 `chapters[].title`**：原值要参与换源与进度恢复的按标题定位。两条执行路径（前端同步、Rust 沙盒）共用同一套语义。
- **替换前必须保护特殊内容，但「规则点名要处理的标签」要放开**：`parser/special_content_protector.rs` 与 `utils/replaceRuleProtect.ts` 是同一套实现的两份（改一边要改另一边），在跑规则前把 `<img>` / `<script>` / `<style>` / 独立成行的 `[newpage]` 换成私用区占位符，跑完再还原。不保护的话用户抄来的 `.*` 会把正文插图整段吃掉。**但 `kinds_for_patterns` / `kindsForPatterns` 会先看规则 pattern 是否提到该类标签**：写 `<img\b[^>]*>` 的「删除评论图」规则必须能看到真实 `<img>`，否则既有规则会集体失效 —— 这是"保护"与"兼容"的取舍点，改动时别把这条判定删掉。注意 `String.replace(/.*/g, x)` 末尾还会命中一个空串（`'abc'` → `xx`），这是 JS 既有语义、与保护无关。
- **阅读页「点击区域」有两套分区，别把老的三分区删掉**：`clickAreaMode: 'auto'`（默认）走原来的「左右翻页按列 / 上下滚动按行，两端翻页、中间呼出菜单」，`'custom'` 才用 `clickAreas` 九宫格。之所以不做成"一张固定九宫格"，是因为两种阅读模式的分区方向本来就相反（分页要左右、滚动要上下），固定网格必然让其中一种变味。九宫格的动作分派在 `ReaderView.runClickAreaAction`，复用的是 `pageForward/pageBackward/prevChapter/nextChapter`（它们自己处理分页边界与自动滚动中断），**不要另写一套翻页**。菜单是唯一出口：九格都没有 `menu` 时 `setClickAreaAction` 会把正中一格抢回来，对应手机端 `AppConfig.detectClickArea()`。`clickAreas` 是对象，`resetConfig` 与 `loadConfig` 都要深拷一份，否则重置排版会把用户配好的九宫格一起打回默认、浅拷还会让多个实例共用同一份对象。设置界面本身就是手机端那个全屏对话框（`ClickAreaSettings.vue`，半透明黑遮罩 + 九格铺满整屏 + 点格子弹动作列表），**只能从 ReaderView 挂**：设置抽屉只占侧边一条，在抽屉里看九宫格看不出"点屏幕哪里命中哪格"，所以 ReadSettings 只 `emit('openClickArea')`，由 ReaderView 先 `closePanel()` 再挂面板。它也因此要进「返回」层级（`readerBack.ts` 的 `clickAreaOpen` 排最前）——否则 Esc 会一次把抽屉和阅读页都退掉。
- **`##匹配##替换` 在详情页字段上也要生效**：`ruleBookInfo` 字段走 `eval_field_html_doc_with_ctx`（整页求值），这条分支早先只 `extract_js`、没切 `##`，于是 `…@text##作\s*者[：:]\s*` 整条被当成 CSS 选择器 → 选不到 → 作者/分类恒为空。**「详情页书名有、作者没有」就是这个坑**。新增字段求值分支时，`##` 必须走 `split_legado_regex` + `apply_legado_regex`，与 `eval_field_html_with_ctx` 保持一致。
- **HTML5 解析器会丢掉「无上下文」的表格/列表标签**：把 `tr` / `td` / `li` 这类元素快照还原成 HTML 再 `Html::parse_document` 时，没有 `<table>`/`<ul>` 祖先的标签会被整段丢弃（`body` 里只剩文本）。书源 `bookList` 交回 `<tr id="nr">` 列表时，字段规则 `td:nth-of-type(1) a@text` 因此恒为空 —— 症状正是「搜索有结果、解析 0 条」。`parser::html::parse_document` 现在按首个标签自动补上下文并打 `data-reader-fragment-wrap` 标记，`document_top_elements()` 再把补上去的祖先跳过；**改这块时不要退回裸 `Html::parse_document`**，也不要用 `body > *` 直接取顶层元素（会只看到补上的 `<table>`）。
- **书源请求的首部必须按浏览器写法发**：Cloudflare 的托管挑战对某些站点会按首部**大小写 + `Host` 位置**判客户端 —— `Host` 在第一位、其余 Title-Case 返回 200；hyper 默认的「全小写 + `Host` 最后」直接 `403 cf-mitigated: challenge`（实测，逐项二分确认；与 TLS 指纹、`Accept-Encoding`、HTTP/2 都无关）。因此 `build_source_client*` 都走 `build_source_builder`（`http1_title_case_headers` + `http1_only`），`fetcher::fetch_inner` 还会显式注入排在首位的 `Host`（书源显式写过就保留它的值）。**别把 `http1_title_case_headers` 去掉**：失效表现是「手机端正常、桌面端整页被拦」，403 页面里只有 `Just a moment`。
- **JSoup 元素在桌面端是可变的**：`js.rs` 的 `wrapElement` 带 `__owner`（宿主元素），`select()` 会把宿主关系传给子元素，`remove()` 才能真的从宿主 `innerHtml` 里摘掉自己；`clone()` 是深拷贝、不影响原文档。`org.jsoup.select.Elements` / `org.jsoup.nodes.Element` 是**可构造**的（书源 jsLib 用 `new` + `appendChild` 手工拼搜索结果卡片）；缺 `org.jsoup.select` 这个子包时 jsLib 直接抛 `cannot read property 'Elements' of undefined`，搜索阶段就会失败。
- **`@` 链式选择器的每一步都要能命中「当前元素自身」**：JSoup 的 `Element.select(sel)` 把 root 自己也放进匹配集（`Collector.collect` 从 root 开始测），W3C 的 `querySelectorAll` / `scraper` 的 `ElementRef::select` 则跳过自身。书源把这当常规写法：目录规则 `class.zjlb@class.lb@ul@li` 里 `<ul class="lb fk">` 的下一层直接就是 `<li>`，第二步 `@ul` 靠的正是自身命中。`html::select_css_from_element` 因此先 `sel.matches(&el)` 再 `el.select(&sel)`；**去掉这一步的症状是「详情页正常、目录 0 章」**。
- **表单型 POST 正文要按书源声明的 `charset` 编码**：书源把 `"charset": "GBK"` 写在搜索 URL 选项里，站点按 GBK 解码关键词；正文一律按 UTF-8 编码时站点收到的是乱码，症状是**搜索永远 0 条、发现页却正常**（发现页走 GET，编码在 `encode_get_query` 那条路上）。`url_analyzer::encode_post_body` 的每个键值都按 `charset` 走 `URLEncoder` 语义（`escape` 标记 → `escape_encode`，未知标签回落 UTF-8），已含 `%` 的片段原样保留防二次编码。
- **元素必须自带 `toJSON`，内部字段必须不可枚举**：书源的 `bookList` / `chapterList` 脚本常写 `var out = []; out.push(li); return out;` —— 交回宿主的是 `JSON.stringify(out)`。元素若没 `toJSON`，每项会变成 `{"__snapshot":{…},"__owner":null,…}` 包装：宿主认不出这是元素快照（顶层缺 `outerHtml`），字段规则 `span.s2 a@text` 全部取空，症状是**发现页/搜索页 0 条**（用 `Elements` 集合时被 `elementsApi.toJSON` 兜住了，裸数组就漏）。所以 `wrapElement` 里挂了 `toJSON: () => s`，并且 `__snapshot` / `__owner` / `__prevOuter` 都用 `defineProperty` 设成 `enumerable: false`。**新增元素内部字段时一定要走这个模式**，否则会被序列化出去。
- **`java.webView(...)` 必须有降级实现，不能让它缺失**：legado 用它把 HTML 交给后台 WebView 渲染后再取内容，一类书源的正文规则第一行就是 `java.webView(result, "div.read-content:not(:empty)", null)`。桌面端的规则跑在 Rust 侧 rquickjs 沙盒里，**没有**同步驱动 WebView2 的通道，但 `js.rs` 的 `J.webView` 仍要存在并做等价降级：有 `html` 就静态解析（脚本只是把正文显示出来，DOM 里本来就有），按 `js` 求值取不到再用 `url` 重抓一次。缺失时的症状是正文恒空、日志只有一行 `[正文 JS] JS 异常: not a function`。`webViewGetSource` / `webViewGetOverrideUrl` 同理，无匹配返回空串让书源走 `||` 兜底。
- **在线朗读引擎的 URL 会带重复查询参数**：在线集合里 71/84 条引擎把 `voiceId` 写了两遍（基础地址自带占位音色，末尾再追加真实音色），而 dds 网关对重复参数直接回 `400 Bad Request`。`src/parser/http_tts_template.rs` 的 `normalize_url` 会在**字符串层面**去重（同名保留最后一个、位置沿用首次出现），不要换成 `Url::query_pairs_mut` 重建——那会解码再编码，把模板要求的 `text=%25E4...` 双重编码弄变形。
- **旁白/对话切分是两层启发式，顺序不能反**：`utils/speechSegments.ts` 先认成对中文引号（`“”`/`「」`/`『』`），再认「短说话人前缀 + 全角冒号」（`张三：`、`他笑道：`）。冒号规则必须**保守**：前缀 ≤20 字、冒号后紧跟引号时不切、数字开头与 `时间：`/`注意：` 这类标签不切，**前缀里还要带说话动作词（`说/道/问/笑道`…）或本身是 ≤4 字的人名/称呼**——只按「短前缀」判会把 `他明白了：这一切都是骗局`、`结论只有一个：撤退` 读成对话音色。冒号对话只在 `。！？!?` 结束：`；`/`…` 在台词中间很常见，当成句末会把一句台词劈成「对话 + 旁白」。`utils/speechChunks.ts` 必须**先切旁白/对话段、再把每段按 ≤70 字切块**；反过来先按句切块再逐块认引号，长对话的后续小块会丢引号上下文（症状：一句长对话前半截对话音色、后半截突然变旁白）。跨段对话（上一段只有开引号、下一段只有闭符）靠 `splitSpeechSegmentsDetailed` 的 `openCloser` 在段落之间传递。不切块通道（`http`）下，只有说话人标签的旁白段（`张三：`）会并进前一段旁白，省一次合成请求。
- **朗读的「字符串路径」必须走 `utils/chapterSpeechText`**：本地 EPUB 的章节正文是后端拼的 `<div class="epub-chapter-content"><style>整本作用域化 CSS</style>正文</div>`。当前章朗读取的是 DOM 的 `innerText`（浏览器不把 `<style>` 当可见文本），所以正常；而跨章预取（`prefetchNextChapterUnits`）和 `startTTS()` 的兜底只有字符串，只做 `replace(/<[^>]+>/g,'')` 会把 CSS 规则本体当正文，一行一句地念成英文——这正是 issue #30「第一章中文正常、下一章开始念英文，换书也一样」。因此字符串路径一律用 `splitChapterContentIntoParagraphs()` / `stripInvisibleMarkup()`：先整段丢掉 `<script>`/`<style>`/`<head>`，再去标签。**别退回裸的标签正则**，判断「哪些标签不可见」要靠标签名配对，而不是只削尖括号。
- **朗读要跟随用户手动跳章**：`useReaderAutoPlayback` 里 watch `store.currentIndex`，非 TTS 自己发起的切章（目录 / 上下一章 / 拖进度条 / 连续阅读滚过章边界）就停掉旧流、从**目标章第一段**续读，否则缓冲流会继续播旧章音频直到流自己跨章才发现索引不符。四个坑：
  - TTS 自身的跨章调用必须包在 `runSpeechDrivenChapterChange()` 里；
  - 连续阅读的 DOM 同时挂着上一章和当前章，续读起点要按 `paragraphChapterIndex(p) === currentIndex` 过滤，别取 `getFilteredParagraphs()[0]`（那是上一章第一段，会让流逐段空转整章、迟迟不开始新章）；
  - 区分「TTS 自动滚到章边界」与「用户滚动/跳章」不能只靠时间窗口：`ReaderView.handleScroll` 反推索引时调 `markChapterChangeFromScroll()`，`handleManualReadingInput()` 调 `markManualReadingInput()`，`isSpeechSelfScroll()` 三者取交集才判自滚（窗口 1.5s）。只看时间窗口会把用户刚滚过之后的真实跳章一起吞掉；
  - 停止入口（面板停止 / 快捷键 / 定时停止 / 关闭面板）都要取消挂起的跟随：`store.stopTTS()` 在 `resetCallbacks` 为真时通知 `addSpeechStopListener` 订阅者，composable 借此调 `cancelChapterJumpFollow()`；内部重启用的是 `stopTTS(false)`，不会误伤续读。
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
- **列表规则里可以夹 `<js>…</js>`**：legado 的 `AnalyzeRule.splitSourceRule` 用 `JS_PATTERN` 把 JS 段单独切出来，剩下的文本前后各成一段，**逐段串联求值**（上一段的输出是下一段的输入）。书源常见写法是 `class.book_cell\n<js>…</js>\na`：先选一批元素、脚本按关键词过滤、再在结果 HTML 上选 `a`。桌面端的 `parse_search_list` 现在走 `split_rule_segments` + `eval_list_rule_chain`；**别退回「只认规则以 JS 开头」**——那种写法会被整条当成一个 CSS 选择器，选择器里含 `<js>` 恒选不到，症状是**搜索永远 0 条**（发现页不走这段脚本，所以看着正常）。
- **列表规则 JS 段的 `result` 是 JSoup `Elements`**：书源写 `list = result.toArray(); for (i in list) {...}`。宿主用 `@elements:` 前缀（`elements_binding`）注入快照数组，`__wrapElements` 负责包成带 `size()` / `toArray()` / 索引的集合。`@element:`（单个元素）和 `@elements:`（集合）是两套，别合并：单元素上 `toArray()` 不存在，脚本第二行就抛 `not a function`。
- **正文规则的 `##匹配##替换` 要先生成主体再替换**：`extract_content` 里没有组合符（`&&`/`||`/`%%`）时必须把 `split_legado_regex` 切出的**主体**交给提取器，再把 `replace_part` 套在结果上。整条一起传会让 `##` 后的替换正则被当成提取器的一部分（`…p@text##广告行.*` 里的 `text##…` 不是合法提取器），症状是**正文解析为空**。
- **登录页的接口请求要补会话段**：登录 iframe 挂在 `/__login/<session>/<上游路径>` 下，但只有**文档路径**带会话段。单页应用的接口地址是绝对路径（axios `baseURL = "/api"`），运行时拼出 `http://reader.localhost/api/...`，兜底路由既拿不到会话（WebView2 对自定义协议请求不带 Cookie/Referer）也无法还原上游地址，只能 404 —— 这就是「单页应用型登录页报 404」。`build_login_request_rewrite_script` 在 `<head>` 之后（**必须排在页面自己的 `<script>` 之前**）注入 fetch/XHR/sendBeacon 包装，给本站与上游同域的地址补 `/__login/<s>/` 前缀；相对路径要相对**上游文档路径**解析（`/__login/<s>/user/me` 里的 `api/x` 是上游的 `/user/api/x`）。不要改成在脚本文本里替换字面量：`"/api"` 是 axios 的 baseURL，字符串替换认不出边界。
- **字段规则里「脚本段 + 后续选择器段」也要串联**：详情页字段有一类写法是「`<js>` 先另抓一页，后面再跟选择器」（例如 `<js>java.ajax(另一个页面地址)</js>` 配一段 `tag.p.1:2@text`）—— 先用脚本换掉内容，再**在新内容上**选元素。`eval_field_rule_chain` 负责这条链（中间规则段保留 HTML 结构、末段按 `@text`/`@html` 取值）。**别退回只认 `</js>` 之前的部分**：那样脚本输出会被原样当成字段值，症状是**简介显示成一整页 HTML 源码**（详情弹窗里那屏 `<!DOCTYPE html…`）。
- **脚本求值要模拟 Rhino 的 `lastResult`，不能只看 ECMAScript 完成值**：Rhino 返回「最后一条**有值**的语句」，ECMAScript 返回「完成值」。差别只在末尾是空完成语句时暴露 —— 书源正文规则常写 `result = decode(raw); if (result.length < 500) { result += "…" }`，条件为假时 QuickJS 的完成值是 `undefined`，前面算好的正文被整个丢掉，症状是**正文恒空且日志里没有任何异常**。`eval_js_inner_with_source` 因此在结果为 `undefined` 时回退「脚本是否改写过 `result`」：改写过就用它，没改写就返回空（不把输入原文当输出）。
- **详情弹窗要补拉封面**：很多书源的 `ruleSearch.coverUrl` 是空的（搜索列表页没有封面图），封面只写在 `ruleBookInfo.coverUrl` 里。`BookDetailModal` 的 `loadDetailIntro` 除了补简介，也要把详情页的 `coverUrl` 存进 `detailCover` 并参与 `coverSrc` 的兜底链，否则「搜索结果有、点进详情弹窗就是空封面」。

