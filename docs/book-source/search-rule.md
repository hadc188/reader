# 搜索规则 (ruleSearch)

定义如何从搜索结果页提取书籍列表。`ruleExplore` 结构与它相同。

## 规则字段

```json
{
  "ruleSearch": {
    "checkKeyWord": "",
    "bookList": "",
    "name": "",
    "author": "",
    "intro": "",
    "kind": "",
    "lastChapter": "",
    "updateTime": "",
    "wordCount": "",
    "coverUrl": "",
    "bookUrl": ""
  }
}
```

## 字段说明

| 字段 | 必需 | 说明 |
|------|------|------|
| `checkKeyWord` | 否 | 当前主要用于调试接口默认关键词，不参与搜索结果校验 |
| `bookList` | 是 | 书籍列表规则，支持 CSS / JSONPath / XPath / Regex / JS |
| `name` | 是 | 书名选择器 |
| `author` | 否 | 作者选择器 |
| `intro` | 否 | 简介 |
| `kind` | 否 | 分类/标签 |
| `lastChapter` | 否 | 最新章节 |
| `updateTime` | 否 | 更新时间 |
| `wordCount` | 否 | 总字数 |
| `bookUrl` | 是 | 书籍详情页链接 |
| `coverUrl` | 否 | 封面图片 URL |

## 示例

### CSS 选择器示例

```json
{
  "searchUrl": "/search?wd={key}&page={page}",
  "ruleSearch": {
    "bookList": ".search-list li",
    "name": "h3 a@text",
    "author": ".author@text##^作者[:：]?##",
    "bookUrl": "h3 a@href",
    "coverUrl": "img@src",
    "intro": ".intro@text",
    "wordCount": ".words@text",
    "lastChapter": ".latest@text",
    "updateTime": ".updated@text"
  }
}
```

### JSON API 示例

::: v-pre

```json
{
  "searchUrl": "https://api.example.com/search?q={key}&page={page}",
  "ruleSearch": {
    "bookList": "$.data.books",
    "name": "title",
    "author": "author",
    "bookUrl": "https://example.com/book/{{id}}",
    "coverUrl": "cover",
    "lastChapter": "latestChapter"
  }
}
```

:::

### 正则列表示例

```json
{
  "ruleSearch": {
    "bookList": ":<a href=\"([^\"]+)\">([^<]+)</a>",
    "bookUrl": "$1",
    "name": "$2"
  }
}
```

### JS 列表示例

```json
{
  "ruleSearch": {
    "bookList": "js:[{ name: '演示书', author: '作者', bookUrl: '/book/1' }]",
    "name": "name",
    "author": "author",
    "bookUrl": "bookUrl"
  }
}
```

## 当前实现说明

- `bookList` 支持前缀 `-` 和 `+`
- `-bookListRule`：结果反转
- `+bookListRule`：显式保持原顺序
- `bookUrl`、`coverUrl` 返回相对路径时会自动补全为绝对 URL
- HTML 搜索当前仍建议把 `bookList` 视为必填
- 如果搜索结果为空、且未配置 `bookUrlPattern`，当前实现会尝试把当前页面按 `ruleBookInfo` 当作详情页回退解析
