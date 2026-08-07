# API 概述

Reader-Rust 提供 RESTful API，所有接口都以 `/reader3` 为前缀。

## 基础 URL

```
http://localhost:8080/reader3
```

## 响应格式

所有响应均为 JSON 格式:

```json
{
  "success": true,
  "data": { ... },
  "errorMsg": null
}
```

错误响应:

```json
{
  "success": false,
  "data": null,
  "errorMsg": "错误信息"
}
```

## 请求方法

| 方法 | 用途 |
|------|------|
| GET | 获取资源 |
| POST | 创建资源 |
| PUT | 更新资源 |
| DELETE | 删除资源 |

## API 分类

- [书源管理](./book-source) - 书源的增删改查
- [书籍搜索](./search) - 搜索书籍、获取详情
- [章节内容](./chapter) - 获取章节列表和正文
- [用户管理](./user) - 用户注册、登录、配置
- [RSS订阅](./rss) - RSS 源管理

## 认证

部分 API 需要认证，通过 `Authorization` Header 传递:

```
Authorization: Bearer <token>
```

[获取认证方式见用户管理 API](./user)
