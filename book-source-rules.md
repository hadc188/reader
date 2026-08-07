# 书源规则解析器兼容规格

本文档面向重新实现一套兼容当前项目 `BookSource` 书源规则的解析器。它不是书源编写教程，而是实现规格：实现者应能按本文重建数据模型、URL 编译器、规则求值器、书籍解析流水线和必要的运行时环境。

当前规格以项目代码为准，覆盖范围如下：

- 书源数据模型：`BookSource`、`BaseSource`、`SearchRule`、`ExploreRule`、`BookInfoRule`、`TocRule`、`ContentRule`、`ReviewRule`。
- URL 规则：`AnalyzeUrl` 的 URL 生成、参数解析、请求配置、编码、Cookie 和 WebView 入口。
- 内容规则：`AnalyzeRule`、`RuleAnalyzer`、`AnalyzeByJSoup`、`AnalyzeByXPath`、`AnalyzeByJSonPath`、`AnalyzeByRegex`。
- 书籍流水线：搜索、发现、详情、目录、正文。
- JS 运行时绑定、变量作用域、登录和共享 JS 库。

非目标：

- 不规定 UI 交互细节。
- 不实现 RSS、TTS、本地 TXT 目录规则。
- 不要求复刻 Android Room、OkHttp、Rhino、JSoup、JsonPath、XPath 的具体库；但替代实现必须匹配本文描述的可观察行为。

## 1. 兼容级别

实现者可以按三层交付：

| 级别 | 要求 |
| --- | --- |
| L1 Parser | 能解析书源 JSON、URL 字符串和内容规则字符串，输出可执行中间表示。 |
| L2 Evaluator | 在给定 HTML/JSON/文本和上下文对象时，能得到与当前项目一致的字符串、列表、元素和 URL 结果。 |
| L3 Engine | 能执行完整搜索、发现、详情、目录、正文流水线，包含网络请求、Cookie、登录检测、JS、WebView 或等价能力。 |

若目标是“兼容当前项目书源规则”，至少需要 L2。若目标是替代当前书源运行时，需要 L3。

本文使用以下术语：

- MUST：必须实现，否则大量书源不兼容。
- SHOULD：建议实现，缺失会影响部分书源或边界行为。
- MAY：可选能力，缺失时应明确降级。

## 2. 核心数据模型

### 2.1 BookSource

`BookSource` 是书源顶层对象。JSON 导入时字段名必须保持一致。

| 字段 | 类型 | 默认值 | 运行时语义 |
| --- | --- | --- | --- |
| `bookSourceUrl` | String | `""` | 主键和基础地址。用于相对 URL 补全、Cookie 域、缓存 key、书源变量命名。必填。 |
| `bookSourceName` | String | `""` | 显示名。必填。 |
| `bookSourceGroup` | String? | `null` | 分组。项目用 `[,;，；]` 拆分。 |
| `bookSourceType` | Int | `0` | `0` 文本，`1` 音频，`2` 图片，`3` 文件。 |
| `bookUrlPattern` | String? | `null` | 详情页 URL 正则。搜索响应 URL 命中时直接按详情页解析；URL 入库匹配也使用它。`NONE` 表示不参与 URL 匹配。 |
| `customOrder` | Int | `0` | 排序编号。 |
| `enabled` | Boolean | `true` | 是否启用搜索。 |
| `enabledExplore` | Boolean | `true` | 是否启用发现。 |
| `jsLib` | String? | `null` | 共享 JS 库，见 12.3。 |
| `enabledCookieJar` | Boolean? | `true` | 是否启用自动 CookieJar 标记。 |
| `concurrentRate` | String? | `null` | 请求频率限制，见 11.3。 |
| `header` | String? | `null` | 书源级请求头规则，见 11.1。 |
| `loginUrl` | String? | `null` | 登录 URL 或登录 JS，见 12.1。 |
| `loginUi` | String? | `null` | 登录表单 JSON，见 12.1。 |
| `loginCheckJs` | String? | `null` | 每次主要网络请求后执行，输入 `StrResponse`，输出新的 `StrResponse`。 |
| `coverDecodeJs` | String? | `null` | 封面图片解密 JS。 |
| `bookSourceComment` | String? | `null` | 注释。 |
| `variableComment` | String? | `null` | 书源变量说明，仅展示。 |
| `lastUpdateTime` | Long | `0` | 更新时间。 |
| `respondTime` | Long | `180000` | 响应时间。 |
| `weight` | Int | `0` | 搜索权重。 |
| `exploreUrl` | String? | `null` | 发现分类入口，见 7。 |
| `exploreScreen` | String? | `null` | 当前执行代码未读取。实现 MAY 保留字段。 |
| `ruleExplore` | ExploreRule? | `null` | 发现列表规则。 |
| `searchUrl` | String? | `null` | 搜索入口 URL 规则。 |
| `ruleSearch` | SearchRule? | `null` | 搜索列表规则。 |
| `ruleBookInfo` | BookInfoRule? | `null` | 详情页规则。 |
| `ruleToc` | TocRule? | `null` | 目录页规则。 |
| `ruleContent` | ContentRule? | `null` | 正文页规则。 |
| `ruleReview` | ReviewRule? | `null` | 预留。当前项目持久化时固定转成 `null`。 |

实现 MUST 对空规则对象提供默认空对象。例如 `getSearchRule()` 在字段为空时返回新的 `SearchRule()`。

### 2.2 规则对象

`SearchRule`：

```kotlin
data class SearchRule(
  var checkKeyWord: String? = null,
  var bookList: String? = null,
  var name: String? = null,
  var author: String? = null,
  var intro: String? = null,
  var kind: String? = null,
  var lastChapter: String? = null,
  var updateTime: String? = null,
  var bookUrl: String? = null,
  var coverUrl: String? = null,
  var wordCount: String? = null
)
```

`ExploreRule` 与 `SearchRule` 相同，但没有 `checkKeyWord`。

`BookInfoRule`：

```kotlin
data class BookInfoRule(
  var init: String? = null,
  var name: String? = null,
  var author: String? = null,
  var intro: String? = null,
  var kind: String? = null,
  var lastChapter: String? = null,
  var updateTime: String? = null,
  var coverUrl: String? = null,
  var tocUrl: String? = null,
  var wordCount: String? = null,
  var canReName: String? = null,
  var downloadUrls: String? = null
)
```

`TocRule`：

```kotlin
data class TocRule(
  var preUpdateJs: String? = null,
  var chapterList: String? = null,
  var chapterName: String? = null,
  var chapterUrl: String? = null,
  var formatJs: String? = null,
  var isVolume: String? = null,
  var isVip: String? = null,
  var isPay: String? = null,
  var updateTime: String? = null,
  var nextTocUrl: String? = null
)
```

`ContentRule`：

```kotlin
data class ContentRule(
  var content: String? = null,
  var title: String? = null,
  var nextContentUrl: String? = null,
  var webJs: String? = null,
  var sourceRegex: String? = null,
  var replaceRegex: String? = null,
  var imageStyle: String? = null,
  var imageDecode: String? = null,
  var payAction: String? = null
)
```

`ReviewRule` 当前仅为预留字段：

```kotlin
data class ReviewRule(
  var reviewUrl: String? = null,
  var avatarRule: String? = null,
  var contentRule: String? = null,
  var postTimeRule: String? = null,
  var reviewQuoteUrl: String? = null,
  var voteUpUrl: String? = null,
  var voteDownUrl: String? = null,
  var postReviewUrl: String? = null,
  var postQuoteUrl: String? = null,
  var deleteUrl: String? = null
)
```

### 2.3 反序列化兼容

规则对象 MUST 支持两种 JSON 输入：

1. 正常对象：

```json
{"bookList":".item","name":".title@text"}
```

1. JSON 字符串，字符串内容再解析成对象：

```json
"{\"bookList\":\".item\",\"name\":\".title@text\"}"
```

这对应当前项目里每个规则类的 `JsonDeserializer` 行为。

## 3. 运行时对象和上下文

完整引擎至少需要以下运行时数据：

| 对象 | 必要字段 |
| --- | --- |
| `SearchBook` | `name`、`author`、`kind`、`wordCount`、`latestChapterTitle`、`intro`、`coverUrl`、`bookUrl`、`origin`、`originName`、`originOrder`、`type`、`variable`、`infoHtml`。 |
| `Book` | `name`、`author`、`kind`、`wordCount`、`latestChapterTitle`、`intro`、`coverUrl`、`bookUrl`、`tocUrl`、`tocHtml`、`downloadUrls`、`origin`、`originName`、`originOrder`、`type`、`variableMap`。 |
| `BookChapter` | `bookUrl`、`baseUrl`、`title`、`url`、`tag`、`isVolume`、`isVip`、`isPay`、`index`、变量 map。 |
| `StrResponse` | `raw`、`body`、`url`、`code`、`headers`、`isSuccessful`。 |
| `RuleData` | 临时 `variableMap`。搜索/发现列表阶段使用。 |

`StrResponse.url` MUST 返回最终响应 URL。当前实现优先取 `raw.networkResponse.request.url`，否则取 `raw.request.url`。

## 4. 总体架构

兼容实现 SHOULD 拆成以下模块：

1. `SourceDeserializer`：导入书源 JSON，兼容对象和字符串化规则对象。
2. `UrlAnalyzer`：把 URL 规则编译成请求描述并执行请求。
3. `RuleAnalyzer`：安全切分 `@`、`&&`、`||`、`%%` 和内嵌规则。
4. `RuleEvaluator`：按模式执行 JSoup/CSS/XPath/JsonPath/JS/Regex。
5. `WebBookPipeline`：按当前项目顺序执行搜索、发现、详情、目录、正文。
6. `JsRuntime`：提供与当前项目等价的 JS 绑定变量和扩展函数。
7. `CookieCacheRuntime`：提供 Cookie、登录头、缓存、书源变量。

实现者 MAY 用不同语言实现，但模块边界和可观察结果 MUST 一致。

## 5. URL 规则规格

本节对应 `AnalyzeUrl`。

### 5.1 构造输入

`UrlAnalyzer` 构造时接收：

| 参数 | 含义 |
| --- | --- |
| `mUrl` | 原始 URL 规则。 |
| `key` | 搜索关键字。 |
| `page` | 当前页码。 |
| `speakText` / `speakSpeed` | 朗读场景参数。书源普通解析可保留但不使用。 |
| `baseUrl` | 基准 URL。 |
| `source` | 当前书源。 |
| `ruleData` | 当前书籍、章节或临时 RuleData。 |
| `chapter` | 当前章节。 |
| `readTimeout` / `callTimeout` | 请求超时。 |
| `headerMapF` | 调用方直接传入的请求头。为空则取书源请求头。 |
| `hasLoginHeader` | 是否合并登录头。当前主要请求默认为 `true`。 |

构造时 MUST 从 `baseUrl` 去除 URL 参数 JSON。当前项目用 `,\s*(?=\{)` 找到第一个参数分隔点，然后截断。

### 5.2 初始化顺序

URL 规则 MUST 按以下顺序处理：

1. 初始化请求头。
2. 执行 `@js:` 或 `<js>...</js>` URL JS。
3. 替换 `{{...}}` 内嵌 JS。
4. 替换 `<...>` 页码选择器。
5. 解析 URL 主体和 URL 参数 JSON。
6. 编码 query 或 POST form。

### 5.3 请求头初始化

若 `headerMapF` 非空，直接使用它。否则调用 `source.getHeaderMap(hasLoginHeader)`。

书源级 `header` 支持：

- 标准 JSON 对象字符串。
- `@js:` 后跟 JS，JS 返回 JSON 字符串。
- `<js>...</js>`，JS 返回 JSON 字符串。

若缺少 `User-Agent`，MUST 补默认 UA。当前项目 key 来自 `AppConst.UA_NAME`。

若请求头中包含 `proxy`，MUST 取出为代理配置，并从请求头删除。

### 5.4 URL JS

JS 标记正则：

```regex
<js>([\w\W]*?)</js>|@js:([\w\W]*)
```

大小写不敏感。

处理算法：

```text
result = originalRuleUrl
start = 0
for each JS_MATCH:
  if match.start > start:
    prefix = trim(ruleUrl[start, match.start])
    if prefix not empty:
      result = prefix.replace("@result", result)
  js = match.group(2) ?: match.group(1)
  result = evalUrlJs(js, result).toString()
  start = match.end
if ruleUrl.length > start:
  suffix = trim(ruleUrl[start, end])
  if suffix not empty:
    result = suffix.replace("@result", result)
ruleUrl = result
```

注意：

- `@js:` 会吞掉后续全部内容。
- `<js>...</js>` 可以和前后普通片段组合。
- 普通片段不是拼接，而是用该片段替换 `@result` 后覆盖 `result`。

URL JS 绑定变量 MUST 包含：

| 变量 | 值 |
| --- | --- |
| `java` | 当前 `UrlAnalyzer`。 |
| `baseUrl` | 当前基准 URL。 |
| `cookie` | CookieStore 等价对象。 |
| `cache` | CacheManager 等价对象。 |
| `page` | 当前页码。 |
| `key` | 搜索关键字。 |
| `speakText` / `speakSpeed` | 构造参数。 |
| `book` | `ruleData` 为 `Book` 时的书籍对象。 |
| `source` | 当前书源。 |
| `result` | 上一步结果。 |

### 5.5 `{{...}}` 替换

如果 `ruleUrl` 同时包含 `{{` 和 `}}`，MUST 扫描所有 `{{...}}` 片段并执行 JS。

替换行为：

- JS 返回 `String`，使用原字符串。
- JS 返回整数值 Double，例如 `1.0`，格式化为无小数的 `"1"`。
- 其他非空返回值调用 `toString()`。

### 5.6 `<...>` 页码选择器

若 `page` 非空，查找所有：

```regex
<(.*?)>
```

对每个匹配：

1. 取括号内文本按 `,` 拆分。
2. 若 `page < pages.size`，使用 `pages[page - 1].trim()`。
3. 否则使用 `pages.last().trim()`。

保持当前项目的边界行为：`page == pages.size` 时走 `else`，结果仍为最后一项。

### 5.7 URL 参数 JSON

主 URL 和参数 JSON 用正则分隔：

```regex
\s*,\s*(?=\{)
```

分隔点前为 URL 主体，分隔点后为 JSON 参数对象。

URL 主体通过 `getAbsoluteURL(baseUrl, urlNoOption)` 转成绝对 URL。规则：

- `baseUrl` 空：返回 `relativePath.trim()`。
- 相对路径为空格：先 trim。
- 绝对 `http://` 或 `https://`：原样返回。
- data URL：原样返回。
- 以 `javascript` 开头：返回空字符串。
- 其他：按标准 `URL(base, relative)` 解析。
- `baseUrl` 自身带 `,{"...":...}` 时，先去掉参数部分。

参数 JSON 支持字段：

| 字段 | 类型 | 行为 |
| --- | --- | --- |
| `method` | String? | 等于 `POST` 忽略大小写时使用 POST，否则 GET。 |
| `charset` | String? | 参数编码字符集。空值默认 UTF-8；`escape` 表示使用 escape 编码。 |
| `headers` | Object/String? | 合并到请求头。 |
| `body` | Any? | POST 请求体。对象或数组序列化成 JSON 字符串。 |
| `origin` | String? | 当前请求流程未使用，保留。 |
| `retry` | Int/String? | 请求重试次数，非法值视为 0。 |
| `type` | String? | 非空时 `getStrResponse()` 读取 bytes 并返回十六进制 body。 |
| `webView` | Any? | `null`、空字符串、`false`、`"false"` 为 false，其他值为 true。 |
| `webJs` | String? | WebView 注入 JS。 |
| `js` | String? | 参数解析后执行，返回值替换最终 URL。 |
| `serverID` | Long/String? | 保存到请求描述中。 |
| `webViewDelayTime` | Long/String? | WebView 延迟，负数按 0。 |

### 5.8 Query 和表单编码

GET：

- 若 URL 包含 `?`，`?` 前为 `urlNoQuery`，后为 query。
- 若 query 已按项目规则判定为 encoded query，原样使用。
- 否则使用 RFC3986 unreserved 加额外允许字符编码。

POST：

- 若 `body` 非空、不是 JSON、不是 XML，且请求头没有 `Content-Type`，按 form 参数编码。
- form 参数按 `&` 和第一个 `=` 分 key/value。
- 如果 `charset` 为空且 key/value 已符合 encoded form，原样保留。
- `charset == "escape"` 时使用 escape 编码。
- 否则使用指定 charset 的 URL 编码。

### 5.9 请求执行

L3 实现 MUST 提供等价请求能力：

1. 请求前合并 Cookie：
   - 从 `CookieStore.getCookie(domain)` 读取。
   - 与请求头 `Cookie` 合并，后者同名值可被合并逻辑覆盖。
   - 若 `source.enabledCookieJar == true`，添加内部 `CookieJar` 标记头。
2. 若 URL 参数 `type` 非空，`getStrResponse()` 返回 `StrResponse(url, hex(bytes))`。
3. 若 `webView == true` 且调用允许 WebView：
   - POST 场景先用 HTTP 获取初始页面，再把响应 URL、HTML、头、JS、sourceRegex 交给 WebView。
   - GET 场景直接用 WebView 打开最终 URL。
4. 普通 HTTP：
   - POST + form：使用 form body。
   - POST + body + Content-Type：按 Content-Type 构造 body。
   - POST + body + 无 Content-Type：按 JSON body。
   - GET：使用 `urlNoQuery` 和 encoded query。
5. 若响应 Content-Type 匹配 XML，且 body 不以 `<?xml` 开头，给 body 前补 `<?xml version="1.0"?>`。

## 6. 内容规则求值器

本节对应 `AnalyzeRule`。

### 6.1 状态

`RuleEvaluator` 持有：

| 状态 | 含义 |
| --- | --- |
| `ruleData` | 当前 `Book`、`SearchBook`、`BookChapter` 或 `RuleData`。 |
| `source` | 当前书源。 |
| `content` | 当前待解析内容。可以是 HTML 字符串、JSON 字符串、DOM 节点、JsonPath 对象、XPath 节点、正则捕获列表等。 |
| `baseUrl` | 当前页面基准 URL。 |
| `redirectUrl` | 响应 URL，用于 URL 补全。 |
| `isJSON` | `content` 不是 DOM Node 且 `content.toString().trim()` 以 `{...}` 或 `[...]` 包裹。 |
| `isRegex` | 曾以 AllInOne 正则模式解析列表。影响后续字段规则默认模式。 |
| `chapter` | 当前章节。 |
| `nextChapterUrl` | 下一章 URL。 |

`setContent(null)` MUST 抛错。`setContent(content, baseUrl)` 会更新 `content`、`isJSON`、`baseUrl`，并清空内部 HTML/JSON/XPath 解析缓存。

### 6.2 公共 API

兼容实现 SHOULD 暴露下列等价 API：

```kotlin
setContent(content: Any, baseUrl: String? = null): RuleEvaluator
setBaseUrl(baseUrl: String?): RuleEvaluator
setRedirectUrl(url: String): URL?
getString(rule: String?, mContent: Any? = null, isUrl: Boolean = false, unescape: Boolean = true): String
getStringList(rule: String?, mContent: Any? = null, isUrl: Boolean = false): List<String>?
getElement(rule: String): Any?
getElements(rule: String): List<Any>
splitSourceRule(rule: String?, allInOne: Boolean = false): List<SourceRule>
evalJS(js: String, result: Any? = null): Any?
put(key: String, value: String): String
get(key: String): String
```

### 6.3 规则片段切分

`splitSourceRule(ruleStr, allInOne=false)`：

1. 空规则返回空列表。
2. 初始模式为 `Default`。
3. 若 `allInOne == true` 且规则以 `:` 开头：
   - 初始模式设为 `Regex`。
   - 全局 `isRegex = true`。
   - 从 `:` 后开始扫描。
4. 否则若全局 `isRegex == true`：
   - 初始模式设为 `Regex`。
5. 使用 JS 标记正则切分：

```regex
<js>([\w\W]*?)</js>|@js:([\w\W]*)
```

6. JS 标记外的普通片段 trim 后非空则生成 `SourceRule(tmp, currentMode)`。
7. JS 标记内生成 `SourceRule(jsCode, Js)`。

### 6.4 SourceRule 构造

`SourceRule` 字段：

| 字段 | 含义 |
| --- | --- |
| `mode` | `XPath`、`Json`、`Default`、`Js`、`Regex`。 |
| `rule` | 当前实际规则文本。 |
| `replaceRegex` | `##` 后的替换正则。 |
| `replacement` | 第二个 `##` 后的替换内容。 |
| `replaceFirst` | 存在第四段 `##` 时为 true。 |
| `putMap` | `@put:{...}` 解析出的变量保存规则。 |
| `ruleParam` / `ruleType` | 内嵌 `@get`、`{{}}`、正则分组引用。 |

模式识别顺序 MUST 一致：

1. 若传入模式是 `Js` 或 `Regex`，保持原规则。
2. `@CSS:`：模式保持 `Default`，规则文本保留 `@CSS:` 前缀，交给 JSoup 求值器处理。
3. `@@`：模式 `Default`，规则去掉前两个 `@`。
4. `@XPath:`：模式 `XPath`，规则去掉 7 个字符。
5. `@Json:`：模式 `Json`，规则去掉 6 个字符。
6. 若当前内容是 JSON，或规则以 `$.`、`$[` 开头：模式 `Json`。
7. 若规则以 `/` 开头：模式 `XPath`。
8. 否则 `Default`。

然后按顺序执行：

1. 分离 `@put:{...}`。
2. 分离 `@get:{...}` 和 `{{...}}`。
3. 分离 `$1` 到 `$99` 分组引用。
4. 每次求值前执行 `makeUpRule(result)`，生成最终 `rule` 和替换参数。

### 6.5 `@put`、`@get`、`{{}}`

`@put` 正则：

```regex
@put:(\{[^}]+?\})
```

大小写不敏感。它只支持到第一个 `}` 的简单 JSON，不支持嵌套对象。解析成功后从规则文本中删除该片段。

求值每个 `SourceRule` 前，必须执行：

```text
for (key, valueRule) in putMap:
  put(key, getString(valueRule))
```

`@get` 与 `{{}}` 识别正则：

```regex
@get:\{[^}]+?\}|\{\{[\w\W]*?\}\}
```

若出现这些片段，并且当前不是 `Js`/`Regex` 模式，且片段前文本位于规则开头或不包含 `##`，则当前模式切到 `Regex`。这是当前项目的隐式兼容行为。

`{{...}}` 中内容的处理：

- 如果内容以 `@`、`$.`、`$[`、`//` 开头，当作单条内容规则执行 `getString()`。
- 否则作为 JS 执行，`result` 传入当前上一步结果。

JS 返回值格式化：

- `null`：插入空字符串。
- `String`：原样插入。
- 整数值 Double：无小数格式化。
- 其他：`toString()`。

### 6.6 正则分组引用

`$1` 到 `$99` 可引用正则捕获组。当前代码也匹配 `$0`，但 `makeUpRule()` 只替换大于 0 的编号，因此 `$0` 实际按普通文本插入。兼容实现 MUST 复刻这一行为，除非明确选择修复并声明不完全兼容。

分组引用只在当前 `result` 是 `List<String?>` 且列表长度大于分组编号时替换，否则保留原 `$n` 文本。

### 6.7 `##` 替换

`makeUpRule()` 最后按 `##` 拆分：

```text
ruleParts = rule.split("##")
rule = trim(ruleParts[0])
if size > 1: replaceRegex = ruleParts[1]
if size > 2: replacement = ruleParts[2]
if size > 3: replaceFirst = true
```

替换行为：

- `replaceFirst == false`：
  - 如果 `replaceRegex` 可编译为正则，执行全局正则替换。
  - 否则执行普通字符串替换。
- `replaceFirst == true`：
  - 如果正则可编译且找到匹配，取第一个匹配片段，对这个片段执行一次 `replaceFirst(regex, replacement)` 并返回。
  - 如果正则可编译但无匹配，返回空字符串。
  - 如果正则不可编译，返回 `replacement`。

### 6.8 getString

算法：

```text
if ruleStr empty: return ""
ruleList = splitSourceRule(ruleStr)
result = mContent ?: current content
for sourceRule in ruleList:
  putRule(sourceRule.putMap)
  sourceRule.makeUpRule(result)
  if result == null: continue
  if sourceRule.rule is not blank OR sourceRule.replaceRegex is empty:
    result = evalByMode(sourceRule.mode, sourceRule.rule, result)
  if result != null AND sourceRule.replaceRegex not empty:
    result = replaceRegex(result.toString(), sourceRule)
if result == null: result = ""
str = result.toString()
if unescape && str contains "&": HTML-unescape str
if isUrl:
  if str blank: return baseUrl ?: ""
  else return absoluteUrl(redirectUrl, str)
return str
```

`evalByMode`：

| 模式 | 行为 |
| --- | --- |
| `Js` | 执行 JS，传入当前 `result`。 |
| `Json` | `AnalyzeByJSonPath(result).getString(rule)`。 |
| `XPath` | `AnalyzeByXPath(result).getString(rule)`。 |
| `Default` | `AnalyzeByJSoup(result).getString(rule)`；若 `isUrl=true` 使用 `getString0()` 只取第一个结果。 |
| `Regex` | 不再执行正则匹配，直接返回 `makeUpRule()` 后的 `rule` 文本。典型用法是 `getElement/getElements` 先产生捕获组列表，字段规则再用 `$1` 等改写成目标文本。 |

### 6.9 getStringList

与 `getString` 类似，但每步使用列表方法：

| 模式 | 行为 |
| --- | --- |
| `Js` | 执行 JS。 |
| `Json` | `getStringList(rule)`。 |
| `XPath` | `getStringList(rule)`。 |
| `Default` | `AnalyzeByJSoup.getStringList(rule)`。 |
| 其他 | 返回规则文本。 |

结束时：

- 若结果是 `String`，按 `\n` 拆成列表。
- 若 `isUrl=true`，逐项用 `redirectUrl` 补绝对 URL，过滤空 URL，并去重保序。
- 不能转成 `List<String>` 时返回 `null`。

### 6.10 getElement / getElements

`getElement(ruleStr)`：

- 使用 `splitSourceRule(ruleStr, allInOne=true)`。
- 按片段顺序更新 `result`。
- 模式行为：
  - `Regex`：`AnalyzeByRegex.getElement(result.toString(), rule.splitNotBlank("&&"))`。
  - `Js`：执行 JS。
  - `Json`：`getObject(rule)`。
  - `XPath`：`getElements(rule)`。
  - `Default`：`AnalyzeByJSoup.getElements(rule)`。
- 若存在替换规则，对 `result.toString()` 执行替换。

`getElements(ruleStr)`：

- 同样使用 `allInOne=true`。
- 模式行为：
  - `Regex`：`AnalyzeByRegex.getElements(result.toString(), rule.splitNotBlank("&&"))`。
  - `Js`：执行 JS。
  - `Json`：`getList(rule)`。
  - `XPath`：`getElements(rule)`。
  - `Default`：`AnalyzeByJSoup.getElements(rule)`。
- 最终若结果可转为 `List<Any>` 返回，否则返回空列表。

## 7. RuleAnalyzer 切分规则

`RuleAnalyzer` 是兼容关键。它负责拆分 `@`、`&&`、`||`、`%%`，并避免拆到括号、中括号、字符串里的分隔符。

### 7.1 splitRule

调用形态：

```kotlin
splitRule("@")
splitRule("&&", "||", "%%")
splitRule("&&", "||")
```

行为要求：

1. 找到最先出现的任一分隔符。
2. 若分隔符位于 `[...]` 或 `(...)` 平衡组内部，忽略它。
3. 平衡组支持单引号和双引号。规则模式下，引号内反斜杠不当作转义；非引号中的 `\` 会跳过下一个字符。
4. JSON/JS 代码模式下，额外处理反斜杠转义。
5. 一旦确定第一个有效分隔符，后续只按同一种分隔符继续切分。
6. `elementsType` 保存实际使用的分隔符。

示例：

```text
div[a="x&&y"]&&span
```

必须拆成：

```text
["div[a=\"x&&y\"]", "span"]
elementsType = "&&"
```

### 7.2 trim

`trim()` 会从当前位置跳过所有前导 `@` 或 ASCII 小于 `!` 的字符。JSoup 默认规则在按 `@` 拆分前会调用它。

### 7.3 innerRule

两种内嵌替换：

1. `innerRule("{{", "}}")`：用于 URL `{{js}}`。
2. `innerRule("{$.", startStep=1, endStep=1)`：用于 JsonPath 的 `{$.path}`，使用平衡花括号。

若没有成功替换：

- `{{...}}` 版本返回原始字符串。
- `{$. ...}` 版本返回空字符串。

## 8. JSoup / CSS 默认规则

本节对应 `AnalyzeByJSoup`。

### 8.1 输入解析

输入是：

- JSoup `Element`：直接使用。
- XPath `JXNode`：若是 element，转成 element；否则 `toString()` 后解析。
- 其他：`toString()` 后用 HTML parser 解析；若以 `<?xml` 开头，用 XML parser。

### 8.2 getStringList

流程：

1. 空规则返回空列表。
2. 若规则去掉 `@CSS:` 后的 `elementsRule` 为空，返回根元素 `data()`。
3. 按 `&&`、`||`、`%%` 拆分。
4. 每条子规则：
   - CSS 模式：取最后一个 `@` 前为 CSS selector，后为取值规则。
   - 默认模式：按 `@` 拆层级，前面层级取元素，最后层级取值。
5. `||` 遇到第一个非空结果停止。
6. `%%` 按列表下标交错合并。
7. 其他情况按顺序合并。

### 8.3 getElements

默认模式下，按 `@` 分层，每层对上一层所有元素执行 `ElementsSingle.getElementsSingle()`。

CSS 模式下，直接对当前元素执行 `select(rule)`。

组合符 `&&`、`||`、`%%` 的合并规则同 `getStringList`。

### 8.4 取值规则

最后一段取值：

| 规则 | 输出 |
| --- | --- |
| `text` | `element.text()`，空值跳过。 |
| `textNodes` | 直接文本节点 trim 后按 `\n` 连接。 |
| `ownText` | `element.ownText()`，空值跳过。 |
| `html` | 移除结果元素里的 `script` 和 `style` 后，返回 `outerHtml()`。 |
| `all` | 返回元素集合 `outerHtml()`。 |
| 其他 | 作为属性名 `element.attr(lastRule)`，空值跳过，并对同一结果列表去重。 |

### 8.5 ElementsSingle 选择器

单层选择规则支持：

| 写法 | 行为 |
| --- | --- |
| 空前缀或 `children` | 当前元素直接子元素。 |
| `class.xxx` | `getElementsByClass("xxx")`。 |
| `tag.xxx` | `getElementsByTag("xxx")`。 |
| `id.xxx` | ID evaluator。 |
| `text.xxx` | `getElementsContainingOwnText("xxx")`。 |
| 其他 | JSoup CSS selector。 |

索引写法：

```text
tag.li.0
tag.li.-1
tag.li!0:2
div.book[0,2,-1]
div.book[1:5:2]
div.book[!0,-1]
```

兼容要求：

- 负数从列表末尾计算。
- `.` 表示选择索引。
- `!` 表示排除索引。
- `:` 旧写法支持区间和步长。
- `[...]` 新写法支持单索引、区间、负数、排除。
- 索引集合去重保序，当前项目通过 `MutableSet` 保存。
- 越界索引忽略；区间端点越界会钳制到有效范围。
- `[start:end:step]` 中省略 start 表示 0，省略 end 表示最后一项，非法或过大 step 按当前算法退化。

## 9. JsonPath、XPath、Regex

### 9.1 JsonPath

输入可以是 JSON 字符串、已解析 JSON 对象或 JsonPath `ReadContext`。

`getString(rule)`：

- 按 `&&`、`||` 拆分。
- 单规则时先尝试替换所有 `{$.path}` 内嵌 JsonPath。
- 若没有内嵌替换结果，则直接 `ctx.read(rule)`。
- 结果是列表时用 `\n` 连接，否则 `toString()`。

`getStringList(rule)`：

- 按 `&&`、`||`、`%%` 拆分。
- 单规则时同样支持 `{$.path}`。
- `ctx.read(rule)` 是列表则逐项 `toString()`；否则单项。

`getObject(rule)` 返回 `ctx.read(rule)`。

`getList(rule)` 返回 `ArrayList<Any>`；组合符合并同 JSoup。

### 9.2 XPath

输入可以是 XPath 节点、JSoup Document/Element/Elements 或字符串。字符串以 `<?xml` 开头时用 XML parser 包装，否则 HTML parser。

特殊包装：

- 字符串以 `</td>` 结尾时，前面加 `<tr>`。
- 字符串以 `</tr>` 或 `</tbody>` 结尾时，前面加 `<table>`。

`getElements` 和 `getStringList` 支持 `&&`、`||`、`%%`。

`getString` 支持 `&&`、`||`，单规则结果用 `\n` 连接。

### 9.3 Regex

`AnalyzeByRegex.getElement(res, regs)`：

1. 对 `regs[index]` 编译正则并查找第一个匹配。
2. 若无匹配，返回 `null`。
3. 若是最后一个正则，返回捕获组列表，包含 group 0。
4. 否则把当前正则所有匹配的完整匹配拼接成字符串，递归进入下一个正则。

`getElements(res, regs)`：

1. 同样逐级正则。
2. 最后一级返回每个匹配的捕获组列表，包含 group 0。
3. 无匹配返回空列表。

与字段 `$n` 配合时，`$1` 起才会被替换；`$0` 保持文本。

## 10. 内容规则 JS 运行时

`RuleEvaluator.evalJS()` MUST 绑定：

| 变量 | 值 |
| --- | --- |
| `java` | 当前 `RuleEvaluator`。 |
| `cookie` | CookieStore。 |
| `cache` | CacheManager。 |
| `source` | 当前书源。 |
| `book` | 当前 `ruleData` 是书籍时的 `Book`。 |
| `result` | 上一步规则结果。 |
| `baseUrl` | 当前页面基准 URL。 |
| `chapter` | 当前章节。 |
| `title` | `chapter?.title`。 |
| `src` | 当前原始 `content`。 |
| `nextChapterUrl` | 当前下一章 URL。 |
| `rssArticle` | RSS 场景对象；书源实现可置空。 |

JS 运行时 SHOULD 提供 `JsExtensions` 等价能力，至少包括：

- `ajax(url)`：使用当前书源头和 Cookie 发起请求，返回 body 字符串；异常时返回 stack trace 字符串。
- `ajaxAll(urlList)`：批量请求。
- `connect(url)` / `connect(url, header)`：返回 `StrResponse`。
- `webView(html, url, js)` 和 `webViewGetSource(...)`：WebView 能力。
- `importScript(path)`：导入远程或本地脚本。
- `cacheFile(url, saveTime)`。
- `getCookie(tag[, key])`。
- `downloadFile(...)`。
- 字符串/bytes/Base64/Hex 编解码。
- `timeFormat`、`encodeURI`、`htmlFormat`、简繁转换。
- 文件读取和压缩包内容读取。
- `queryTTF` 字体工具。
- `toNumChapter`、`toURL`。
- `toast`、`log`、`randomUUID`、`androidId`、`openUrl`。

如果实现目标不包含 Android UI，可把 UI 类函数实现为空操作或返回明确错误，但需要文档声明。

## 11. 通用运行时服务

### 11.1 Header

`source.getHeaderMap(hasLoginHeader)`：

1. 解析 `source.header`。
2. 若 header 以 `@js:` 或 `<js>` 开头，先执行 JS，结果转字符串。
3. 把字符串按 JSON 对象解析成 `Map<String,String>`。
4. 若没有 User-Agent，补默认 UA。
5. 若 `hasLoginHeader == true`，合并 `source.getLoginHeaderMap()`。

### 11.2 Cookie

实现 MUST 支持：

- 按有效顶级域名保存 Cookie。
- 请求前合并持久 Cookie、会话 Cookie 和请求头 Cookie。
- `source.putLoginHeader(json)` 若 JSON 中含 `Cookie` 或 `cookie`，必须同步写入 CookieStore。
- `source.removeLoginHeader()` 必须删除登录头和该源 Cookie。

当前 `NetworkUtils.getSubDomain()` 行为：

- `http://1.2.3.4` -> `1.2.3.4`
- `https://www.example.com` -> `example.com`
- `http://www.biquge.com.cn` -> `biquge.com.cn`
- 解析失败返回原输入或 baseUrl。

### 11.3 concurrentRate

空、`0`：不限制。

不含 `/`：

- 含义是同一时间只允许一个请求。
- 若已有请求进行中，等待 `concurrentRate.toInt()` 毫秒。
- 若没有请求进行中，还要保证距离上次开始时间至少间隔该毫秒数。

含 `/`：

- 格式 `次数/毫秒`。
- 在指定毫秒窗口内最多允许该次数。
- 当前项目判断为 `frequency > limit` 时等待，因此边界上可能允许 `limit + 1` 次；完全兼容实现需复刻此行为。

解析异常时视为不限制。

## 12. 登录、共享 JS、变量

### 12.1 登录

`loginUrl` 有两种语义：

- 普通 URL：WebView 登录页，URL 按 `bookSourceUrl` 补绝对地址。
- `@js:` 或 `<js>`：登录脚本。提交登录 UI 后会拼接脚本并调用其中的 `login()` 函数。

`loginUi` 是 `RowUi` 数组：

```kotlin
data class RowUi(
  var name: String = "",
  var type: String = "text", // text/password/button
  var action: String? = null,
  var style: FlexChildStyle? = null
)
```

按钮：

- `action` 是绝对 URL：打开外部 URL。
- 其他非空：把 `loginUrl` 中的登录 JS 和 `action` 拼接执行，`result` 是当前登录表单数据 map。

登录信息用 AES 保存，JS 中可通过 `source.getLoginInfo()` 或 `source.getLoginInfoMap()` 获取。

### 12.2 loginCheckJs

主要请求结束后，若 `loginCheckJs` 非空：

```text
res = analyzeUrl.evalJS(loginCheckJs, result = res) as StrResponse
```

它出现在搜索、发现、详情、目录、正文每次初始请求后。兼容实现必须允许脚本返回新的 `StrResponse`。

### 12.3 jsLib

`source.getShareScope(jsLib)`：

- 空：不使用共享 scope。
- 普通 JS：执行到共享 scope。
- JSON 对象：遍历对象值；值是绝对 URL 时下载脚本、缓存、执行。当前项目只处理 URL 值，非 URL 值不执行。
- scope 以 `MD5(jsLib)` 缓存。
- 加载后 seal，只读共享。

规则 JS 和 URL JS 的 scope 原型指向该共享 scope。

### 12.4 变量作用域

`RuleEvaluator.put(key,value)` 写入优先级：

1. 当前章节变量。
2. 当前书籍变量。
3. 当前 `ruleData` 变量。
4. 书源持久变量。

`get(key)` 读取优先级：

1. `bookName` 特殊返回当前书名。
2. `title` 特殊返回当前章节标题。
3. 当前章节变量。
4. 当前书籍变量。
5. 当前 `ruleData` 变量。
6. 书源持久变量。
7. 空字符串。

保存 `bookName` 或 `title` 时当前项目只打印调试警告，不阻止保存。

## 13. 搜索和发现流水线

### 13.1 搜索

伪代码：

```text
searchBookAwait(source, key, page=1):
  if source.searchUrl blank: throw "搜索url不能为空"
  ruleData = RuleData()
  analyzeUrl = UrlAnalyzer(source.searchUrl, key, page, baseUrl=source.bookSourceUrl, source, ruleData)
  res = analyzeUrl.getStrResponse()
  if source.loginCheckJs not blank:
    res = analyzeUrl.evalJS(source.loginCheckJs, res) as StrResponse
  return analyzeBookList(source, ruleData, analyzeUrl, baseUrl=res.url, body=res.body, isSearch=true, isRedirect=res.raw.priorResponse?.isRedirect == true)
```

### 13.2 发现

```text
exploreBookAwait(source, url, page=1):
  ruleData = RuleData()
  analyzeUrl = UrlAnalyzer(url, page=page, baseUrl=source.bookSourceUrl, source, ruleData)
  res = analyzeUrl.getStrResponse()
  if source.loginCheckJs not blank:
    res = analyzeUrl.evalJS(source.loginCheckJs, res) as StrResponse
  return analyzeBookList(source, ruleData, analyzeUrl, baseUrl=res.url, body=res.body, isSearch=false)
```

### 13.3 exploreUrl 分类解析

`exploreUrl` 为空：无发现分类。

若以 `@js:` 或 `<js>` 开头：

1. 执行 JS 得到字符串。
2. 结果按 `MD5(bookSourceUrl + exploreUrl)` 缓存。

结果格式：

1. JSON 数组，元素为：

```kotlin
data class ExploreKind(
  val title: String = "",
  val url: String? = null,
  val style: FlexChildStyle? = null
)
```

2. 普通文本：按 `(&&|\n)+` 拆分类，每项按 `::` 拆分：

```text
title::url
```

无 `::` 时，`url=null`。

### 13.4 BookList 解析

输入：响应 body、当前 baseUrl、`isSearch`。

流程：

1. body 为空抛错。
2. 创建 `RuleEvaluator(ruleData, source)`，设置 content/body/baseUrl/redirectUrl。
3. 非搜索时，若处于调试回调，检查 `exploreUrl` JSON 格式，仅记录日志。
4. 搜索场景：
   - 若 `bookUrlPattern` 非空且 `baseUrl.matches(pattern)`，直接按详情页解析成单本书，返回。
5. 选择列表规则：
   - 搜索：`source.getSearchRule()`。
   - 发现且 `ruleExplore.bookList` 空：`source.getSearchRule()`。
   - 其他发现：`source.getExploreRule()`。
6. `bookList` 前缀：
   - `-`：记录 `reverse=true` 并去掉前缀。
   - `+`：去掉前缀，无其他效果。
7. `collections = evaluator.getElements(bookListRule.bookList ?: "")`。
8. 若 `collections` 为空且 `bookUrlPattern` 为空，按详情页解析成单本书。
9. 否则逐项解析字段。
10. 用 `LinkedHashSet` 去重保序。
11. 若 `reverse=true`，反转搜索结果。

字段解析：

| 字段 | 行为 |
| --- | --- |
| `name` | `formatBookName(getString(nameRule))`；为空则丢弃该项。 |
| `author` | `formatBookAuthor(getString(authorRule))`。 |
| `kind` | `getStringList(kindRule)?.joinToString(",")`。 |
| `wordCount` | `wordCountFormat(getString(wordCountRule))`。 |
| `lastChapter` | 写入 `latestChapterTitle`。 |
| `intro` | HTML 格式化后写入。 |
| `coverUrl` | `getString(coverUrlRule)` 后按当前 `baseUrl` 补绝对 URL。 |
| `bookUrl` | `getString(bookUrlRule, isUrl=true)`；为空则用当前 `baseUrl`。 |

## 14. 详情页流水线

```text
getBookInfoAwait(source, book, canReName=true):
  设置书籍类型
  if book.infoHtml not empty:
    analyzeBookInfo(baseUrl=book.bookUrl, redirectUrl=book.bookUrl, body=book.infoHtml)
  else:
    analyzeUrl = UrlAnalyzer(book.bookUrl, baseUrl=source.bookSourceUrl, source, ruleData=book)
    res = analyzeUrl.getStrResponse()
    if loginCheckJs not blank: res = eval loginCheckJs
    analyzeBookInfo(baseUrl=book.bookUrl, redirectUrl=res.url, body=res.body)
```

详情解析：

1. body 为空抛错。
2. 创建 `RuleEvaluator(book, source)`，设置 content/body/baseUrl/redirectUrl。
3. 若 `ruleBookInfo.init` 非空：
   - `evaluator.setContent(evaluator.getElement(init))`。
   - 后续字段在 init 结果上解析。
4. `mCanReName = canReName && !canReNameRule.isNullOrBlank()`。
5. 字段：

| 字段 | 行为 |
| --- | --- |
| `name` | 格式化后，若非空且 `mCanReName` 或原书名为空，则写入。 |
| `author` | 同 name。 |
| `kind` | `getStringList()?.joinToString(",")`，非空写入。 |
| `wordCount` | 格式化后非空写入。 |
| `lastChapter` | 非空写入。 |
| `intro` | HTML 格式化后非空写入。 |
| `coverUrl` | 非空时按 `redirectUrl` 补绝对 URL。 |
| `tocUrl` | 非文件类书源解析。为空则用详情页 `baseUrl`；若等于 `baseUrl`，缓存 `tocHtml=body`。 |
| `downloadUrls` | 文件类书源解析。用 `getStringList(isUrl=true)`；空则抛“下载链接为空”。 |

注意：`canReName` 当前只判断非空，不解析真假。

## 15. 目录流水线

`preUpdateJs`：

- 只有调用方传 `runPerJs=true` 时执行。
- JS 中允许调用 `java.reGetBook()` 和 `java.refreshTocUrl()`；其他上下文调用这两个方法必须抛错。

请求目录：

```text
if book.bookUrl == book.tocUrl && book.tocHtml not empty:
  parse tocHtml
else:
  analyzeUrl = UrlAnalyzer(book.tocUrl, baseUrl=book.bookUrl, source, ruleData=book)
  res = analyzeUrl.getStrResponse()
  if loginCheckJs not blank: res = eval loginCheckJs
  parse res.body with baseUrl=book.tocUrl, redirectUrl=res.url
```

目录解析：

1. body 为空抛错。
2. `listRule = tocRule.chapterList ?: ""`。
3. 前缀：
   - `-`：`reverse=true`，去掉前缀。
   - `+`：去掉前缀。
4. 解析当前页章节和下一页 URL。
5. `nextTocUrl`：
   - 无结果：结束。
   - 1 个 URL：顺序循环请求，直到空或已请求过。
   - 多个 URL：按线程数并发请求。
6. 若章节列表为空，抛目录为空异常。
7. 若 `reverse == false`，先反转 `chapterList`。
8. 用 `LinkedHashSet` 去重保序。
9. 若 `book.getReverseToc() == false`，再次反转。
10. 重新写入每章 `index`。
11. 若 `formatJs` 非空，遍历章节执行 JS，返回值替换标题。

单页章节解析：

| 字段 | 行为 |
| --- | --- |
| `chapterList` | `getElements(listRule)`。 |
| `nextTocUrl` | `getStringList(nextTocUrl, isUrl=true)`，过滤等于当前 redirectUrl 的 URL。 |
| `chapterName` | `getString(nameRule)`。非空才加入列表。 |
| `chapterUrl` | `getString(urlRule)`，不立即补绝对 URL；章节对象后续通过 `getAbsoluteURL()` 使用 `baseUrl` 补。 |
| `updateTime` | `getString(updateTimeRule)` 写入 `BookChapter.tag`。 |
| `isVolume` | `getString(isVolumeRule).isTrue()`。 |
| `isVip` | `getString(isVipRule).isTrue()`。 |
| `isPay` | `getString(isPayRule).isTrue()`。 |

`isTrue()` 行为：

- 空、blank 或 `"null"`：false。
- 忽略大小写匹配 `false|no|not|0`：false。
- 其他非空字符串：true。

章节 URL 为空：

- 若是卷名：`url = title + index`。
- 否则：`url = baseUrl`。

`formatJs` 绑定：

| 变量 | 值 |
| --- | --- |
| `gInt` | 初始为 0，同一个 format 流程中复用。 |
| `index` | 章节序号，从 1 开始。 |
| `chapter` | 当前章节对象。 |
| `title` | 当前章节标题。 |

## 16. 正文流水线

```text
getContentAwait(source, book, chapter, nextChapterUrl=null, needSave=true):
  if source.ruleContent.content empty:
    return chapter.url
  if chapter.isVolume && chapter.url.startsWith(chapter.title):
    return chapter.tag ?: ""
  if chapter.url == book.bookUrl && book.tocHtml not empty:
    parse book.tocHtml
  else:
    analyzeUrl = UrlAnalyzer(chapter.getAbsoluteURL(), baseUrl=book.tocUrl, source, ruleData=book, chapter=chapter)
    res = analyzeUrl.getStrResponse(jsStr=contentRule.webJs, sourceRegex=contentRule.sourceRegex)
    if loginCheckJs not blank: res = eval loginCheckJs
    parse res.body
```

正文解析：

1. body 为空抛错。
2. 若 `contentRule.title` 非空，先解析标题；成功时覆盖章节标题。
3. 解析当前页正文和下一页 URL。
4. `nextContentUrl`：
   - 1 个 URL：顺序循环请求，直到空、重复、或等于下一章 URL。
   - 多个 URL：并发请求，不再递归获取下一页。
5. 多页正文用 `\n` 连接。
6. 若 `replaceRegex` 非空：
   - 对正文按换行拆行，每行 trim，再用 `\n` 连接。
   - 调用 `evaluator.getString(replaceRegex, contentStr)`。
   - 再按换行拆分，每行前加两个全角空格。
7. 非卷名章节正文为空时抛正文为空异常。
8. `needSave=true` 时保存正文。

单页正文：

| 字段 | 行为 |
| --- | --- |
| `content` | `getString(contentRule.content, unescape=false)`。 |
| 内容格式化 | 经过 `HtmlFormatter.formatKeepImg(content, redirectUrl)`，保留图片并补 URL。若含 `&`，再 HTML-unescape。 |
| `nextContentUrl` | `getStringList(nextContentUrl, isUrl=true)`。 |

`webJs` 和 `sourceRegex` 只有在章节 URL 参数启用 `{"webView":true}` 时参与 WebView 请求。

`imageStyle` 支持：

- `DEFAULT`
- `FULL`
- `TEXT`
- `SINGLE`

`imageDecode`：

- 作为正文图片解密 JS。
- 绑定 `book`、`result`、`src`。
- `result` 是 bytes 或 input stream。
- 必须返回解密后的 `ByteArray`。

`payAction`：

- 阅读页触发购买时执行。
- 绑定当前 `book`、`chapter`、`baseUrl=chapter.url`。
- 返回绝对 URL：打开 WebView。
- 返回 true-like 字符串：删除本章正文缓存并刷新目录。

## 17. 段评规则状态

当前项目不可用：

- `BookSource.Converters.stringToReviewRule()` 固定返回 `null`。
- `BookSource.Converters.reviewRuleToString()` 固定返回 `"null"`。
- Android 编辑器保存 `ruleReview` 的代码被注释。

兼容实现 SHOULD 保留 `ReviewRule` 字段用于导入导出，但 MUST 默认不执行段评规则，除非明确实现扩展能力。

## 18. 老书源导入兼容

当前项目在 `ImportOldData` 中支持旧格式迁移。若目标支持旧书源导入，需实现：

| 旧字段 | 新字段 |
| --- | --- |
| `bookSourceUrl` | `bookSourceUrl` |
| `bookSourceName` | `bookSourceName` |
| `bookSourceGroup` | `bookSourceGroup` |
| `loginUrl` / `loginUi` / `loginCheckJs` / `coverDecodeJs` | 同名字段 |
| `bookSourceComment` | `bookSourceComment` |
| `ruleBookUrlPattern` | `bookUrlPattern` |
| `serialNumber` | `customOrder` |
| `httpUserAgent` | `header = {"User-Agent": ua}` |
| `ruleSearchUrl` | `searchUrl` |
| `ruleFindUrl` | `exploreUrl` |
| `bookSourceType == "AUDIO"` | `bookSourceType = 1`，其他为 0 |
| `enable` | `enabled` |
| `ruleSearchList`、`ruleSearchName` 等 | `ruleSearch` |
| `ruleFindList`、`ruleFindName` 等 | `ruleExplore` |
| `ruleBookInfoInit`、`ruleBookName` 等 | `ruleBookInfo` |
| `ruleChapterList`、`ruleChapterName`、`ruleContentUrl`、`ruleChapterUrlNext` | `ruleToc` |
| `ruleBookContent`、`ruleBookContentReplace`、`ruleContentUrlNext` | `ruleContent` |

旧 URL 转换：

- `@Header:{...}` -> URL 参数 `headers`。
- `|charset=...` -> URL 参数 `charset`。
- URL 中 `@body` -> 参数 `{"method":"POST","body":"body"}`。
- `searchKey` -> `{{key}}`。
- `searchPage` -> `{{page}}`。
- `{...}` 页码选择器 -> `<...>`。
- `<js>` 里的 `=searchKey`、`=searchPage` -> `={{key}}`、`={{page}}`。

## 19. 兼容性测试用例建议

实现完成后 SHOULD 建立测试集。以下用例可作为最小验收：

### 19.1 URL 编译

输入：

```text
/search?q={{key}}&page=<1,2,3>,{"headers":{"Referer":"https://a.test"}}
```

上下文：`baseUrl=https://a.test`，`key=斗破`，`page=2`。

期望：

- URL 主体补全到 `https://a.test/search?...`。
- `{{key}}` 执行 JS 返回关键字。
- `<1,2,3>` 替换成 `2`。
- headers 合并 `Referer`。

### 19.2 默认 JSoup 规则

HTML：

```html
<ul><li><a href="/b/1">书名</a><span>作者</span></li></ul>
```

规则：

```text
tag.li.0
```

必须返回第一个 `li` 元素。对该元素：

```text
tag.a.0@text -> 书名
tag.a.0@href -> /b/1
tag.span.0@text -> 作者
```

### 19.3 组合符

HTML：

```html
<h1></h1><div class="title">标题</div>
```

规则：

```text
h1@text||.title@text
```

期望：`标题`。

### 19.4 AllInOne 正则

文本：

```html
<li><a href="/1">第一章</a></li><li><a href="/2">第二章</a></li>
```

列表规则：

```text
:<a href="([^"]+)">([^<]+)</a>
```

字段规则：

```text
$1 -> /1、/2
$2 -> 第一章、第二章
$0 -> 字面量 $0
```

### 19.5 JsonPath

JSON：

```json
{"data":{"name":"书名","author":"作者"}}
```

规则：

```text
$.data.name -> 书名
作者：{$.data.author} -> 作者：作者
```

### 19.6 目录布尔值

输入字符串：

| 输入 | `isTrue()` |
| --- | --- |
| `""` | false |
| `"null"` | false |
| `"false"` | false |
| `"no"` | false |
| `"not"` | false |
| `"0"` | false |
| `"true"` | true |
| `"1"` | true |
| `"VIP"` | true |

## 20. 已知兼容陷阱

- `@js:` 会吞掉后续整段规则；链式规则必须用 `<js>...</js>`。
- `$0` 不会替换成正则完整匹配，而是保留字面量。
- `canReName` 只判断非空，不解析真假。
- `SearchRule.updateTime`、`ExploreRule.updateTime`、`BookInfoRule.updateTime` 当前存在但不执行。
- `exploreScreen` 存在但当前不执行。
- `ruleReview` 会持久化成 `null`。
- `ruleContent.webJs` 和 `sourceRegex` 只有 URL 参数 `webView=true` 时进入 WebView。
- `@put` 的 JSON 提取不支持嵌套对象。
- URL 参数 JSON 分隔符只识别逗号后紧跟 `{` 的位置。
- 书源级 `header` 非标准 JSON 会尝试宽松解析；实现 MAY 只支持标准 JSON，但会降低兼容性。
- 目录排序包含两次反转和一次去重，必须按第 15 节顺序复刻。
