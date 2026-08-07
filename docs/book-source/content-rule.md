# 正文规则 (ruleContent)

定义如何从章节页提取正文内容。

## 规则字段

```json
{
  "ruleContent": {
    "content": "",
    "nextContentUrl": "",
    "title": "",
    "webJs": "",
    "sourceRegex": "",
    "replaceRegex": "",
    "imageStyle": "",
    "imageDecode": "",
    "payAction": ""
  }
}
```

## 字段说明

| 字段 | 必需 | 说明 |
|------|------|------|
| `content` | 是 | 正文规则，支持 CSS / JSONPath / XPath / JS |
| `nextContentUrl` | 否 | 下一页正文链接，当前支持 CSS / JSONPath / XPath 直接取值 |
| `title` | 否 | 当前模型字段存在，但正文主流程不会单独消费 |
| `webJs` | 否 | 在 `content` 之前，对整页响应体做 JS 预处理 |
| `sourceRegex` | 否 | 在 `content` 之前，对整页响应体做正则替换 |
| `replaceRegex` | 否 | 在 `content` 之后，对提取结果做正则替换 |
| `imageStyle` | 否 | 当前仅保留字段，正文主流程未执行 |
| `imageDecode` | 否 | 当前仅保留字段，正文主流程未执行 |
| `payAction` | 否 | 当前仅保留字段，正文主流程未执行 |

## 执行顺序

```text
sourceRegex
  ↓
webJs
  ↓
content
  ↓
replaceRegex
```

## 示例

### 直接提取纯文本

```json
{
  "ruleContent": {
    "content": ".read-content@textNodes"
  }
}
```

### HTML 正文 + 后处理清洗

```json
{
  "ruleContent": {
    "content": ".read-content@html",
    "replaceRegex": "##<br\\s*\\/?>##\\n##</p>##\\n##<[^>]+>##"
  }
}
```

### 先清理原始页面，再提取正文

```json
{
  "ruleContent": {
    "sourceRegex": "##<script[\\s\\S]*?<\\/script>##",
    "webJs": "js:input.replace(/&nbsp;/g, ' ')",
    "content": ".read-content@textNodes",
    "replaceRegex": "##广告推荐.*$##"
  }
}
```

### JS 直接返回正文

```json
{
  "ruleContent": {
    "content": "js:input.match(/<div class=\\\"content\\\">([\\s\\S]*?)<\\/div>/)?.[1] || ''"
  }
}
```

### 分页正文

```json
{
  "ruleContent": {
    "content": ".article-content@textNodes",
    "nextContentUrl": ".next-page@href"
  }
}
```

## 最佳实践

1. 优先用 `content` 负责提取，把清洗放进 `sourceRegex` / `webJs` / `replaceRegex`
2. `nextContentUrl` 当前不要写 `##...` 或 `@js:` 后处理，直接返回链接即可
3. 如果站点正文本身是 JSON，直接让 `content` 走 JSONPath 更稳定
4. `title`、`imageStyle`、`imageDecode`、`payAction` 目前还不是正文主链路的一部分
