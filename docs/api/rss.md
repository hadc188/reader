# RSS 订阅 API

## 获取 RSS 源列表

```
GET /reader3/getRssSources
```

响应:
```json
{
  "success": true,
  "data": [
    {
      "sourceUrl": "https://example.com/feed.xml",
      "sourceName": "RSS源名称",
      "enabled": true
    }
  ],
  "errorMsg": null
}
```

## 添加 RSS 源

```
POST /reader3/saveRssSource
```

请求体:
```json
{
  "sourceUrl": "https://example.com/feed.xml",
  "sourceName": "RSS源名称",
  "enabled": true
}
```

## 删除 RSS 源

```
POST /reader3/deleteRssSource
```

请求体:
```json
{
  "sourceUrl": "https://example.com/feed.xml"
}
```

## 获取 RSS 内容

```
GET /reader3/getRssContent
```

查询参数:
- `url` - RSS 源URL
- `page` - 页码

响应:
```json
{
  "success": true,
  "data": [
    {
      "title": "文章标题",
      "link": "https://...",
      "description": "摘要",
      "pubDate": "2024-01-01"
    }
  ],
  "errorMsg": null
}
```
