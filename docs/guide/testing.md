# 标准测试流程

本文定义 `reader-rust` 的标准回归测试流程，目标是让前端、后端和交互流程在发布前都有一套统一检查。

## 测试目标

- 验证核心页面和主要导航是否正常
- 验证书架、搜索、发现、RSS、设置等高频功能是否可用
- 验证需要真实数据的流程有固定的手工回归步骤
- 提供一套可用 `Playwright` 自动执行的基础冒烟测试

## 测试分层

### 1. 构建与静态检查

发布前至少执行：

```bash
cargo check
cargo test
cd frontend && npm run build
```

这层负责发现：

- Rust 编译错误
- 解析器回归
- 前端类型错误
- 前端打包错误

### 2. Playwright 自动冒烟

Playwright 目录在仓库根目录：

- [playwright.config.ts](/Users/mac/project/reder/reader-rust/playwright.config.ts)
- [tests/e2e/app-shell.spec.ts](/Users/mac/project/reder/reader-rust/tests/e2e/app-shell.spec.ts)
- [tests/e2e/standard-flow.spec.ts](/Users/mac/project/reder/reader-rust/tests/e2e/standard-flow.spec.ts)

安装依赖：

```bash
npm install
npx playwright install
```

执行：

```bash
npm run test:e2e
```

可视化调试：

```bash
npm run test:e2e:headed
npm run test:e2e:ui
```

默认测试地址：

```text
http://127.0.0.1:8080/#/
```

如果服务地址不同：

```bash
PLAYWRIGHT_BASE_URL=http://127.0.0.1:9000/#/ npm run test:e2e
```

### 3. 手工全量回归

有些功能依赖真实书源、账号、缓存、RSS 数据或设备行为，不适合全部硬编码进自动测试。发布前建议按下面流程人工回归。

## 自动测试覆盖

当前 Playwright 冒烟覆盖这些能力：

- 顶部/底部导航切换
- 书架、书海、最近、RSS 主页面可打开
- 设置抽屉可打开
- 书源管理、用户管理、备份恢复入口可见
- 全局搜索可进入搜索模式
- 搜索结果范围切换 UI 可见
- 分组管理、缓存管理弹窗可打开
- RSS 管理页跳转可用

## 可选自动回归

有些用例需要环境变量提供测试数据。

### 登录回归

```bash
E2E_USERNAME=admin \
E2E_PASSWORD=12345678 \
npm run test:e2e
```

### 搜索结果详情回归

```bash
E2E_SEARCH_KEYWORD=凡人修仙传 \
E2E_EXPECTED_BOOK_NAME=凡人修仙传 \
npm run test:e2e
```

### 书源导入回归

```bash
E2E_SOURCE_IMPORT_FILE=/absolute/path/to/book-sources.json \
npm run test:e2e
```

## 标准手工回归清单

### A. 书架

1. 打开首页，确认书架列表正常显示
2. 打开一本已有书籍，确认能进入阅读页
3. 返回书架，确认最近阅读顺序更新
4. 打开书籍详情弹窗，确认目录能加载
5. 测试编辑模式、批量选择、分组移动、删除

### B. 搜索

1. 输入关键词进入搜索模式
2. 验证 `全部书源 / 按分组 / 单个书源` 都能切换
3. 验证搜索结果里显示书源信息和最新章节
4. 点击封面打开书籍详情
5. 点击结果项进入阅读
6. 点击“加入书架”后书架中可见

### C. 书源管理

1. 打开设置中的书源管理
2. 本地导入书源 JSON
3. 远程订阅同步一次
4. 启用/禁用书源
5. 编辑并保存一个书源
6. 触发书源登录，确认状态返回正常

### D. 阅读器

1. 打开章节目录并切换章节
2. 打开章节搜索面板并搜索当前章节
3. 修改字体、间距、主题等阅读设置
4. 测试正文分页或滚动模式是否正常
5. 如启用 TTS，验证播放、暂停、续播、下一章
6. iOS/iPadOS 桌面模式验证首开和前后台切换

### E. 发现页

1. 切换不同发现书源
2. 切换不同分类
3. 滚动加载更多
4. 点击书籍进入阅读
5. 点击“加入书架”验证写入成功

### F. 最近阅读

1. 验证最近阅读列表显示正常
2. 搜索最近阅读
3. 打开历史条目继续阅读
4. 删除最近阅读条目

### G. RSS

1. 进入 RSS 首页
2. 切换当前源、全部文章、分组文章
3. 打开文章正文
4. 进入 RSS 管理页，新增/编辑/删除 RSS 源
5. 刷新文章并确认分页加载

### H. 用户与备份

1. 未登录时打开设置，确认登录入口正常
2. 登录后确认用户名和角色显示正常
3. 修改密码
4. 多用户模式下打开用户管理
5. 开启 WebDAV/服务器备份时验证备份恢复入口

## 发布前建议流程

推荐每次发布都按这个顺序执行：

1. `cargo check`
2. `cargo test`
3. `cd frontend && npm run build`
4. `npm run test:e2e`
5. 按“标准手工回归清单”执行一次完整人工检查

如果这次改动涉及以下模块，必须追加专项回归：

- 解析器：补书源导入、搜索、详情、目录、正文测试
- 阅读器：补阅读页、TTS、移动端和 iOS/iPadOS 测试
- 用户系统：补登录、登出、用户管理、权限测试
- RSS：补 RSS 管理和正文加载测试

## 后续建议

后续可以继续补这些方向：

- 增加 `Playwright` 测试专用 fixture 数据
- 增加测试账号和测试书源初始化脚本
- 增加 CI 中的 E2E 冒烟任务
- 增加移动端视口和 PWA 模式专项测试
