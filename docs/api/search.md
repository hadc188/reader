# 书籍搜索 API

## 搜索书籍

```
GET /reader3/searchBook
```

查询参数:
- `key` - 搜索关键词
- `page` - 页码，默认1
- `bookSourceUrl` - 指定书源(可选)

响应:
```json
{
  "success": true,
  "data": [
    {
      "name": "书名",
      "author": "作者",
      "coverUrl": "https://...",
      "bookUrl": "https://...",
      "intro": "简介",
      "kind": "玄幻",
      "wordCount": "100万字",
      "lastChapter": "最新章节",
      "sourceName": "书源名称"
    }
  ],
  "errorMsg": null
}
```

## 获取书籍详情

```
GET /reader3/getBookInfo
```

查询参数:
- `url` - 书籍URL

响应:
```json
{
  "success": true,
  "data": {
    "name": "书名",
    "author": "作者",
    "coverUrl": "https://...",
    "intro": "书籍简介",
    "kind": "分类",
    "wordCount": "字数",
    "lastChapter": "最新章节",
    "tocUrl": "目录页URL"
  },
  "errorMsg": null
}
```

## 获取目录

```
GET /reader3/getChapterList
```

查询参数:
- `url` - 目录页URL或书籍URL

响应:
```json
{
  "success": true,
  "data": [
    {
      "title": "第一章 xxx",
      "url": "https://...",
      "index": 0
    }
  ],
  "errorMsg": null
}
```
