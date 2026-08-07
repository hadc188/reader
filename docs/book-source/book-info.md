# 书籍信息规则 (ruleBookInfo)

定义如何从书籍详情页提取详细信息。

## 规则字段

```json
{
  "ruleBookInfo": {
    "init": "",
    "name": "",
    "author": "",
    "coverUrl": "",
    "intro": "",
    "kind": "",
    "wordCount": "",
    "lastChapter": "",
    "updateTime": "",
    "tocUrl": "",
    "canReName": "",
    "downloadUrls": ""
  }
}
```

## 字段说明

| 字段 | 必需 | 说明 |
|------|------|------|
| `init` | 否 | 初始化规则。XPath / JSON 常用于缩小作用域；HTML 模式更适合做 `@put` 预处理 |
| `name` | 是 | 书名 |
| `author` | 是 | 作者 |
| `coverUrl` | 否 | 封面图片 URL |
| `intro` | 否 | 书籍简介 |
| `kind` | 否 | 分类标签 |
| `wordCount` | 否 | 总字数 |
| `lastChapter` | 否 | 最新章节标题 |
| `updateTime` | 否 | 更新时间 |
| `tocUrl` | 否 | 目录页 URL（默认当前详情页） |
| `canReName` | 否 | 可重命名标记，当前仅按普通字符串解析 |
| `downloadUrls` | 否 | 下载链接，当前仅按普通字符串解析 |

## 示例

### 标准详情页

```json
{
  "ruleBookInfo": {
    "name": ".book-info h1@text",
    "author": ".book-info .author a@text",
    "coverUrl": ".book-cover img@src",
    "intro": ".book-intro@html",
    "kind": ".tags a@text",
    "wordCount": ".book-status .words@text",
    "lastChapter": ".book-status .latest a@text",
    "updateTime": ".book-status .time@text",
    "tocUrl": ".read-link@href"
  }
}
```

### 用 `init` + `@put` 复用字段

```json
{
  "ruleBookInfo": {
    "init": "@put:{bookId:.book-detail@data-book-id}",
    "name": "h1@text",
    "author": ".author@text",
    "tocUrl": "/book/@get:{bookId}/chapters"
  }
}
```

### 简介后处理

```json
{
  "ruleBookInfo": {
    "intro": ".book-intro@html@js:input.replace(/<br\\s*\\/?>/gi, '\\n').replace(/<[^>]+>/g, '').trim()"
  }
}
```

## 注意事项

1. `tocUrl` 如未指定，默认使用当前页面作为目录页
2. 某些网站目录在单独页面，需要指定 `tocUrl`
3. `bookUrl` 会作为当前详情页 URL 传入解析流程
4. HTML 详情字段推荐使用直接选择器或 `@js:` 后处理，不要用只写单个 `##xxx` 的旧写法
