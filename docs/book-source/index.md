---
outline: deep
---

# 书源开发概述

书源定义了 Reader-Rust 如何请求站点、解析搜索结果、读取目录和抓取正文。

## 书源结构

一个最小可用的书源通常长这样：

```json
{
  "bookSourceName": "书源名称",
  "bookSourceUrl": "https://example.com",
  "bookSourceType": 0,
  "enabled": true,
  "jsLib": "",
  "searchUrl": "/search?wd={key}&page={page}",
  "ruleSearch": { /* 搜索规则 */ },
  "ruleBookInfo": { /* 书籍信息规则 */ },
  "ruleToc": { /* 目录规则 */ },
  "ruleContent": { /* 正文规则 */ }
}
```

常见源级字段：

| 字段 | 说明 |
|------|------|
| `bookSourceName` | 书源名称 |
| `bookSourceGroup` | 分组 |
| `bookSourceUrl` | 书源基础 URL，用于相对路径补全 |
| `enabled` | 是否启用 |
| `enabledExplore` | 是否启用发现页 |
| `header` | 全局请求头，JSON 字符串 |
| `jsLib` | 共享 JS 库，会在该书源的所有 JS 规则前执行 |
| `searchUrl` | 搜索请求规则 |
| `exploreUrl` | 发现页请求规则 |
| `loginUrl` | 登录请求规则 |
| `loginCheckJs` | 登录页返回后的校验脚本 |
| `ruleSearch` | 搜索结果解析 |
| `ruleBookInfo` | 详情页解析 |
| `ruleToc` | 目录页解析 |
| `ruleContent` | 正文页解析 |

## 解析方式

当前解析器支持以下模式：

| 方式 | 标识 | 说明 |
|------|------|------|
| CSS 选择器 | 默认 / `@css:` | HTML 解析默认模式 |
| JSONPath | `@json:` 或自动检测 | 响应体是 JSON 时可直接取字段 |
| XPath | `/`、`./` 或 `@xpath:` | XML / HTML 路径提取 |
| 正则 | `:` 或 `@regex:` | 主要用于列表规则 |
| JavaScript | `js:`、`@js:`、`<js>...</js>` | 复杂逻辑或预处理 |

当前各阶段的能力边界：

| 阶段 | 当前支持 |
|------|----------|
| `ruleSearch` / `ruleExplore` | CSS / JSONPath / XPath / Regex / JS |
| `ruleBookInfo` | CSS / JSONPath / XPath |
| `ruleToc` | CSS / JSONPath / XPath / Regex / JS |
| `ruleContent.content` | CSS / JSONPath / XPath / JS |
| `ruleContent.nextContentUrl` | CSS / JSONPath / XPath |

## 规则流程

```text
搜索关键词
    ↓
ruleSearch → 书籍列表 → 选择书籍
    ↓
ruleBookInfo → 书籍详情
    ↓
ruleToc → 章节列表
    ↓
ruleContent → 章节正文
```

正文阶段的实际执行顺序是：

```text
sourceRegex
  ↓
webJs
  ↓
content
  ↓
replaceRegex
```

## URL 规则

`searchUrl`、`exploreUrl`、`loginUrl` 使用的是请求规则，不是字段选择器。

当前支持：

- `{key}`：搜索关键词，会自动 URL 编码
- `{page}`：页码
- <code v-pre>{{ ... }}</code>：执行 JS 并把返回值插入 URL
- `, { ... }`：在 URL 后追加请求配置 JSON

示例：

::: v-pre

```json
{
  "searchUrl": "/search?wd={key}&page={page}",
  "exploreUrl": "/rank/{{page}}",
  "loginUrl": "https://example.com/login,{\"method\":\"POST\",\"body\":\"a=1&b=2\",\"headers\":{\"Content-Type\":\"application/x-www-form-urlencoded\"}}"
}
```

:::

## 调试技巧

1. 使用 `/reader3/bookSourceDebugSse` 调试搜索规则
2. 检查 `log_level=debug` 的服务端日志
3. 优先先确认站点响应是 HTML 还是 JSON，再写规则
4. JS 规则运行在服务端 QuickJS 中，不是浏览器 DOM 环境

## 下一步

- [规则语法](./rules) - 了解每种解析方式的语法
- [搜索规则](./search-rule) - 编写搜索功能
- [书籍信息](./book-info) - 提取书籍详情
- [目录规则](./toc-rule) - 获取章节列表
- [正文规则](./content-rule) - 获取章节内容
- [JavaScript](./javascript) - 使用 JS 处理复杂逻辑
