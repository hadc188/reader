# 规则语法

## HTML / CSS 规则

HTML 是默认模式，通常不用显式写 `@css:`。

### 基本语法

| 选择器 | 说明 |
|--------|------|
| `#id` | ID 选择器 |
| `.class` | 类选择器 |
| `tag` | 标签选择器 |
| `parent > child` | 子选择器 |
| `[attr]` | 属性选择器 |
| `class.mod block` | Legado 风格多 class，会转成 `.mod.block` |
| `id.main` | 会转成 `#main` |
| `tag.div` | 会转成 `div` |
| `children` | 选择当前元素的直接子元素 |
| `text.关键字` | 选择自身文本包含关键字的元素 |

### 索引语法

当前解析器同时支持旧式索引和 `[]` 索引：

| 语法 | 说明 |
|------|------|
| `.list.0` | 第 1 个元素 |
| `.list.-1` | 最后 1 个元素 |
| `.list!0` | 排除第 1 个元素 |
| `a[0,2,-1]` | 选择多个下标 |
| `a[1:3]` | 选择 1 到 3，下标包含结束位 |
| `a[-1:0]` | 倒序选择 |
| `a[!1,2]` | 排除多个下标 |

### 列表组合

仅在列表规则里支持：

| 语法 | 说明 |
|------|------|
| `rule1&&rule2` | 结果拼接 |
| `rule1||rule2` | 前一个为空时回退到后一个 |
| `rule1%%rule2` | 交错合并两个结果列表 |

### 内容提取

规则可以用 `@` 后缀指定提取内容：

| 后缀 | 说明 | 示例 |
|------|------|------|
| `@text` | 提取文本 | `h1@text` |
| `@textNodes` | 递归提取所有文本节点，按换行拼接 | `div@textNodes` |
| `@ownText` | 只取当前元素自己的文本节点 | `div@ownText` |
| `@html` | 提取当前元素 HTML | `div.content@html` |
| `@all` | 当前实现等同于 `@html` | `div@all` |
| `@href` | 直接读取属性 | `a@href` |
| `@attr[href]` | 属性读取的兼容写法 | `a@attr[href]` |

### 链式选择

用 `@` 继续向下选择：

```text
.book-list@h3@a@href
```

### 二次解析 `@@`

`@@` 会把前一步得到的文本重新当成 HTML 再解析：

```text
div.script-data@html@@a@href
```

## JSONPath

用于解析 JSON API 响应。

### 基本语法

| 表达式 | 说明 |
|--------|------|
| `$` | 根对象 |
| `$.data` | 访问 data 属性 |
| `$.data.list` | 访问 data.list |
| `$..name` | 递归查找 name |
| `$.data[0]` | 数组第一个元素 |

### 示例

```json
{
  "bookList": "$.data.list",
  "name": "$.title",
  "author": "$.author_name"
}
```

字段规则里也可以直接写对象字段名：

```json
{
  "bookList": "$.data.list",
  "name": "title",
  "author": "author"
}
```

## XPath

用于 XML 或复杂的 HTML 结构。

### 基本语法

| 表达式 | 说明 |
|--------|------|
| `//tag` | 选择所有 tag |
| `/html/body/div` | 绝对路径 |
| `//div[@class='book']` | 属性选择 |
| `//div/text()` | 提取文本 |

### 显式指定

```text
@xpath://div[@class='book']
```

## 正则表达式

当前解析器里的正则有两种主要用法。

### 1. 作为列表规则

适用于 `bookList`、`chapterList` 这类整段文本匹配：

```text
:<a href="([^"]+)">([^<]+)</a>
@regex:<a href="([^"]+)">([^<]+)</a>
```

字段里再通过 `$1`、`$2` 取捕获组：

```json
{
  "bookList": ":<a href=\"([^\"]+)\">([^<]+)</a>",
  "bookUrl": "$1",
  "name": "$2"
}
```

### 2. 作为后处理替换

适用于大部分字段规则，以及 `ruleContent.sourceRegex` / `ruleContent.replaceRegex`。

语法固定为：

```text
##正则##替换文本
```

可串联多个替换：

```text
##^作者[:：]?## ##\s+## 
```

最后一个替换加 `###` 表示只替换首个匹配：

```text
##广告.*?## ###
```

## JavaScript

支持三种写法：

| 写法 | 用途 |
|------|------|
| `js:...` | 整条规则就是 JS |
| `@js:...` | 在字段提取后做 JS 后处理 |
| `<js>...</js>` | 与 `@js:` 等价 |

示例：

```json
{
  "author": ".author@text@js:input.replace(/^作者[:：]?/, '').trim()",
  "chapterList": "js:[{ chapterName: '第一章', chapterUrl: '/1' }]"
}
```

详细的 JS 运行时见 [JavaScript](./javascript)。

## 模板与变量

### `@put` / `@get`

当前解析器支持在 HTML / JSON / XPath 字段中缓存临时变量：

```json
{
  "init": "@put:{bookId:.book@data-id,bookName:h1@text}",
  "tocUrl": "/book/@get:{bookId}/chapters",
  "name": "@get:{bookName}"
}
```

### <code v-pre>{{ ... }}</code>

<code v-pre>{{ ... }}</code> 会执行 JS 并把结果插回规则中。

在 JSON 规则中，还支持直接取当前项字段：

::: v-pre

```json
{
  "bookUrl": "https://example.com/book/{{id}}",
  "coverUrl": "{{$.cover}}"
}
```

:::

## URL 参数

请求规则当前使用这些占位符：

| 占位符 | 说明 |
|--------|------|
| `{key}` | 搜索关键词，自动 URL 编码 |
| `{page}` | 页码 |
| <code v-pre>{{...}}</code> | 执行 JS |

示例：

```text
/search?q={key}&page={page}
```

也可以在 URL 后追加请求配置 JSON：

```text
/search,{ "method": "POST", "body": "wd={key}", "headers": { "Content-Type": "application/x-www-form-urlencoded" } }
```
