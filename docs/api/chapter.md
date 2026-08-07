# 章节内容 API

## 获取章节正文

```
GET /reader3/getBookContent
```

查询参数:
- `url` - 章节URL

响应:
```json
{
  "success": true,
  "data": {
    "title": "章节标题",
    "content": "正文内容...",
    "nextUrl": "下一章URL",
    "prevUrl": "上一章URL"
  },
  "errorMsg": null
}
```

## 缓存章节

```
POST /reader3/cacheBookContent
```

请求体:
```json
{
  "urls": ["https://...", "https://..."]
}
```

## 获取阅读进度

```
GET /reader3/getReadProgress
```

查询参数:
- `bookUrl` - 书籍URL

响应:
```json
{
  "success": true,
  "data": {
    "bookUrl": "https://...",
    "chapterUrl": "https://...",
    "chapterTitle": "章节标题",
    "readProgress": 50.5,
    "updateTime": 1699999999999
  },
  "errorMsg": null
}
```

## 保存阅读进度

```
POST /reader3/saveReadProgress
```

请求体:
```json
{
  "bookUrl": "https://...",
  "chapterUrl": "https://...",
  "chapterTitle": "章节标题",
  "readProgress": 50.5
}
```
