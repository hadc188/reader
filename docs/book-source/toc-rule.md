# 目录规则 (ruleToc)

定义如何从目录页提取章节列表。

## 规则字段

```json
{
  "ruleToc": {
    "preUpdateJs": "",
    "init": "",
    "chapterList": "",
    "chapterName": "",
    "chapterUrl": "",
    "formatJs": "",
    "isVolume": "",
    "isVip": "",
    "isPay": "",
    "nextTocUrl": "",
    "updateTime": ""
  }
}
```

## 字段说明

| 字段 | 必需 | 说明 |
|------|------|------|
| `preUpdateJs` | 否 | 目录解析前对整页响应体做 JS 预处理 |
| `init` | 否 | 初始化规则。XPath / JSON 可缩小作用域；HTML 模式更适合做 `@put` |
| `chapterList` | 是 | 章节列表规则，支持 CSS / JSONPath / XPath / Regex / JS |
| `chapterName` | 是 | 章节标题选择器 |
| `chapterUrl` | 是 | 章节链接选择器 |
| `formatJs` | 否 | 章节解析完成后重写标题，只影响标题 |
| `isVolume` | 否 | 是否为卷名节点，真值时允许没有 `chapterUrl` |
| `isVip` | 否 | 章节是否 VIP |
| `isPay` | 否 | 章节是否付费 |
| `nextTocUrl` | 否 | 下一页目录链接 |
| `updateTime` | 否 | 更新时间选择器 |

## 示例

### 标准目录页

```json
{
  "ruleToc": {
    "chapterList": ".chapter-list li",
    "chapterName": "a@text",
    "chapterUrl": "a@href"
  }
}
```

### 分页目录

```json
{
  "ruleToc": {
    "chapterList": ".chapter-list li",
    "chapterName": "a@text",
    "chapterUrl": "a@href",
    "nextTocUrl": ".next-page@href"
  }
}
```

### 倒序目录

```json
{
  "ruleToc": {
    "chapterList": "-.chapter-list li",
    "chapterName": "a@text",
    "chapterUrl": "a@href"
  }
}
```

### JS 目录列表示例

```json
{
  "ruleToc": {
    "chapterList": "js:[{ chapterName: '第一章', chapterUrl: '/1', isVip: '1' }, { chapterName: '第二章', chapterUrl: '/2' }]",
    "chapterName": "chapterName",
    "chapterUrl": "chapterUrl",
    "isVip": "isVip"
  }
}
```

### 目录预处理与标题格式化

```json
{
  "ruleToc": {
    "preUpdateJs": "js:input.replace(/<script[\\s\\S]*?<\\/script>/gi, '')",
    "chapterList": ".chapter-list li",
    "chapterName": "a@text",
    "chapterUrl": "a@href",
    "formatJs": "`${index}. ${title}`"
  }
}
```

`formatJs` 可用的绑定值：

| 变量 | 说明 |
|------|------|
| `index` | 1 开始的章节序号 |
| `title` | 当前标题 |
| `chapter` | 当前章节对象 |

## 当前实现说明

- `chapterList` 支持前缀 `-` 反转和 `+` 显式保留顺序
- `chapterUrl` 为空时：
- 如果 `isVolume` 为真，会自动生成一个卷节点占位 URL
- 否则退回当前目录页 URL
- `nextTocUrl` 可以返回单个下一页，也可以返回多个分页 URL
- 章节会按 URL 去重
