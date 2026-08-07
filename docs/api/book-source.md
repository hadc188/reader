# 书源管理 API

## 获取书源列表

```
GET /reader3/getBookSources
```

响应:
```json
{
  "success": true,
  "data": [
    {
      "bookSourceUrl": "https://example.com",
      "bookSourceName": "示例书源",
      "bookSourceGroup": "网络小说",
      "bookSourceType": 0,
      "enabled": true,
      "enabledExplore": true
    }
  ],
  "errorMsg": null
}
```

## 添加/更新书源

```
POST /reader3/saveBookSource
```

请求体:
```json
{
  "bookSourceUrl": "https://example.com",
  "bookSourceName": "示例书源",
  "bookSourceGroup": "网络小说",
  "ruleSearch": {
    "bookList": ".book-list li",
    "name": "h3@text",
    "author": ".author@text",
    "bookUrl": "a@href"
  }
}
```

## 删除书源

```
POST /reader3/deleteBookSource
```

请求体:
```json
{
  "url": "https://example.com"
}
```

## 批量删除书源

```
POST /reader3/deleteBookSources
```

请求体:
```json
{
  "urls": ["https://example1.com", "https://example2.com"]
}
```

## 测试书源

```
POST /reader3/testBookSource
```

请求体:
```json
{
  "bookSourceUrl": "https://example.com",
  "key": "测试关键词"
}
```

## 导入书源

```
POST /reader3/importBookSources
```

请求体:
```json
[
  { /* 书源对象1 */ },
  { /* 书源对象2 */ }
]
```

## 导出书源

```
POST /reader3/exportBookSources
```

请求体 (留空导出全部，或指定URL):
```json
{
  "urls": ["https://example.com"]
}
```
