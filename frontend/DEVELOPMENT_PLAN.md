# 阅读 App 前端开发计划

> 技术栈：Vue 3 + TypeScript + Vite + Pinia + Vue Router  
> 设计风格：纯 CSS 设计系统，暖色调（琥珀/橙色），支持亮色/暗色主题  
> 后端 API：`http://127.0.0.1:8080/reader3/...`

---

## 一、已完成功能 ✅

### 1. 基础架构

- [x] Vite + Vue 3 + TypeScript 项目初始化
- [x] CSS 变量设计系统（`variables.css`），亮色/暗色主题
- [x] 全局样式重置、滚动条、过渡动画（`global.css`）
- [x] Axios 封装（Token 注入、统一 `ApiResponse<T>` 解包、全局错误拦截）
- [x] Vue Router 路由配置（`/` 书架首页、`/reader` 阅读页）
- [x] Pinia 状态管理（`appStore`、`bookshelfStore`、`readerStore`）
- [x] TypeScript 类型定义（`Book`、`SearchBook`、`BookChapter`、`BookSource` 等）

### 2. 顶部导航栏

- [x] Logo + 品牌名（"阅读"）
- [x] 全局搜索框（回车触发搜索）
- [x] 书海按钮（触发 explore 事件）
- [x] 刷新按钮（刷新书架，带旋转动画）
- [x] 主题切换按钮（亮色/暗色快捷切换）
- [x] 设置按钮（打开设置抽屉）
- [x] 用户头像（已登录时显示）

### 3. 书架首页（HomeView）

- [x] 书架标题 + 书籍数量显示
- [x] 分组标签页（全部 / 未分组 / 自定义分组）
- [x] 书籍响应式网格布局（`BookGrid`）
- [x] 书籍卡片（`BookCard`）：封面图/占位符、书名、作者、未读角标
- [x] 编辑模式：删除按钮覆盖在封面上
- [x] 空状态提示（"书架空空如也，搜索添加新书吧"）

### 4. 搜索功能

- [x] SSE 实时多源搜索（`searchBookMultiSSE`）
- [x] 搜索结果列表展示（`SearchResults` 组件）
- [x] 搜索中脉冲动画指示器
- [x] 搜索结果 → 加入书架
- [x] 返回书架按钮

### 5. 设置抽屉（SettingsDrawer）

- [x] 用户区域：登录/注册按钮、用户信息卡片、注销
- [x] 书源管理入口：打开书源管理弹窗
- [x] 导入书源：远程 URL 导入
- [x] 书架设置：刷新缓存
- [x] 外观设置：亮色/暗色主题卡片式切换
- [x] 应用区域：在线/离线状态、安装到主屏幕、PWA 更新入口
- [x] 阅读统计：累计时长、打开书籍数、阅读章节数、读完书籍数

### 6. 弹窗组件

- [x] **登录弹窗**（`LoginModal`）：登录/注册切换、表单验证、loading 状态
- [x] **书籍详情弹窗**（`BookDetailModal`）：封面大图、元信息、简介、章节目录、开始/继续阅读
- [x] **书源管理弹窗**（`SourceManager`）：搜索过滤、分组筛选、启用/禁用开关、删除

### 7. 阅读页面（ReaderView）- 基础版

- [x] 全屏阅读区域，点击切换控制栏
- [x] 顶部栏：返回按钮、书名、章节名、目录按钮
- [x] 底部栏：上一章/下一章导航、进度显示
- [x] 简单字号调节（A- / A+）
- [x] 行距选择（1.5 / 1.8 / 2.0 / 2.5）
- [x] 章节目录抽屉（左侧滑出，高亮当前章节）
- [x] 阅读页代码拆分：抽屉/弹层异步加载，繁体转换按需加载

### 8. API 层

- [x] 书架 API：`getBookshelf`、`saveBook`、`deleteBook`、`getChapterList`、`getBookContent`、`saveBookProgress`、`getCoverUrl`
- [x] 搜索 API：`searchBook`、`searchBookMulti`、`searchBookMultiSSE`
- [x] 书源 API：`getBookSources`、`saveBookSource`、`saveBookSources`、`deleteBookSource`、`readRemoteSourceFile`
- [x] 用户 API：`login`、`register`、`logout`、`getUserInfo`

---

## 二、待完成功能 📋

### 阶段 1：阅读页面增强（高优先级）⭐

参考旧版 UI 截图，阅读页面需要大幅增强，从简单的上下控制栏升级为左侧导航栏 + 右侧工具栏的布局：

#### 1.1 左侧导航栏

| 功能 | 说明 | 对应 API |
|------|------|----------|
| 📖 书架 | 弹出书架面板，快速切换书籍 | `getBookshelf` |
| 📋 书源 | 弹出书源切换面板，切换当前书的书源 | `setBookSource` |
| 📑 目录 | 弹出章节目录列表 | `getChapterList` |
| ⚙️ 设置 | 弹出阅读设置面板（详见 1.3） | 本地设置 |
| ◀ 首页 | 返回书架首页 | — |
| ⬆ 顶部 | 滚动到页面顶部 | — |
| ⬇ 底部 | 滚动到页面底部 | — |

#### 1.2 右侧工具栏（浮动按钮）

| 功能 | 说明 | 对应 API |
|------|------|----------|
| 🔖 书签 | 添加/管理书签 | `saveBookmark`、`getBookmarks`、`deleteBookmark` |
| 🔍 搜索 | 章节内容全文搜索 | `searchBookContent` (如有) |
| ℹ️ 书籍信息 | 显示当前书籍详细信息弹窗 | `getBookInfo` |
| 🔄 刷新 | 重新加载当前章节内容 | `getBookContent` |
| 👁 自动翻页 | 像素滚动 / 段落滚动模式 | 本地设置 |
| 🎧 听书 (TTS) | Web Speech API 朗读内容 | 浏览器原生 |
| 🌙 夜间模式 | 快速切换日/夜模式 | 本地设置 |
| 📊 阅读进度 | 显示百分比进度 | 本地计算 |
| ◀ 上一章 | 翻到上一章 | `getBookContent` |
| ▶ 下一章 | 翻到下一章 | `getBookContent` |

#### 1.3 阅读设置面板（完整版）

从截图分析，阅读设置需要支持以下配置项：

**基本设置**

| 配置项 | 控件类型 | 可选值 | 默认值 |
|--------|----------|--------|--------|
| 特殊模式 | 按钮组 | 正常 / 简洁 | 正常 |
| 配置方案 | 按钮组 | 内置白天 / 内置黑夜 / 新增方案 / 自动切换 | 内置白天 |
| 方案类型 | 按钮组 | 白天默认 / 黑夜默认 | 白天默认 |
| 阅读主题 | 色板选择 | 多种预设颜色 + 自定义 | 默认暖白 |
| 正文字体 | 按钮组 | 系统 / 黑体 / 楷体 / 宋体 / 仿宋 | 系统 |
| 简繁转换 | 按钮组 | 简体 / 繁体 | 简体 |

**排版设置**

| 配置项 | 控件类型 | 范围 | 默认值 |
|--------|----------|------|--------|
| 字体大小 | 步进按钮 A-/A+ | 12-32 | 18 |
| 字体粗细 | 步进按钮 -/+ | 100-900 | 400 |
| 段落行高 | 步进按钮 -/+ | 1.0-3.0 | 1.8 |
| 段落间距 | 步进按钮 -/+ | 0-2.0 | 0.2 |
| 字体颜色 | 颜色选择器 | — | 跟随主题 |
| 页面模式 | 按钮组 | 自适应 / 手机模式 | 自适应 |
| 页面宽度 | 步进按钮 目-/目+ | 400-1200 | 800 |

**翻页设置**

| 配置项 | 控件类型 | 可选值 | 默认值 |
|--------|----------|--------|--------|
| 翻页方式 | 按钮组 | 上下滑动 / 左右翻页 / 上下滚动 / 上下滚动2 | 上下滑动 |
| 动画时长 | 步进按钮 -/+ | 0-1000ms | 300 |
| 自动翻页 | 按钮组 | 像素滚动 / 段落滚动 | 像素滚动 |
| 滚动像素 | 步进按钮 -/+ | 1-10 | 1 |
| 翻页速度 | 步进按钮 -/+ | 100-5000 | 1000 |
| 全屏点击 | 按钮组 | 下一页 / 自动 / 不翻页 | 自动 |
| 选择文字 | 按钮组 | 操作弹窗 / 忽略 | 忽略 |

**额外操作**

- 显示翻页区域（可视化点击区域划分）
- 过滤规则管理（管理内容替换规则）
- 重置为默认配置

#### 1.4 其他阅读页增强

- [x] 阅读进度百分比显示（右侧工具栏 + 底部栏）
- [x] 预加载：阅读末尾提前预加载下一章（连续滚动/分页模式）
- [x] 缓存章节内容（服务器缓存 + 浏览器缓存 + 缓存管理）
- [x] 翻页点击区域可视化
- [x] 上下滚动模式（无限滚动，自动加载前后章节）
- [x] 键盘快捷键支持（←/→ 翻页，空格下一页，ESC 退出）
- [x] 自动翻页模式（可调速度的自动滚动）
- [x] 底部信息栏（当前页/总页数、阅读进度、时间显示）

---

### 阶段 2：书海（探索书源）功能 ⭐

书海是发现新书的核心入口，对应后端 `exploreBook` API。

#### 2.1 书海主页面

- [x] 创建 `ExploreView.vue` 或 `ExploreModal` 弹窗组件
- [x] 展示所有配置了 `exploreUrl` 的书源列表
- [x] 按书源分组（`bookSourceGroup`）标签过滤
- [x] 显示可用书源数量
- [x] 每个书源可展开，显示其分类标签（玄幻、武侠、都市、科幻 等）

#### 2.2 探索结果

- [x] 点击分类标签 → 调用 `exploreBook` API 获取书单
- [x] 书单网格/列表展示（复用 `BookCard` 组件）
- [x] 分页加载（"加载更多" 按钮）
- [x] 点击书籍 → 查看详情 / 加入书架

#### 2.3 对应 API

```typescript
// 需新增
export async function exploreBook(params: {
  ruleFindUrl: string
  bookSourceUrl?: string
  page?: number
}) {
  return http.post('/reader3/exploreBook', params)
}

export async function getBookSourcesWithExplore() {
  // 获取书源列表，筛选带 exploreUrl 的
  const sources = await getBookSources()
  return sources.filter(s => s.exploreUrl)
}
```

---

### 阶段 3：书签管理

- [x] 添加书签（保存当前阅读位置 + 选中文本）
- [x] 书签列表弹窗（显示所有书签）
- [x] 点击书签 → 跳转到对应章节位置
- [x] 删除书签
- [x] 对应 API：`getBookmarks`、`saveBookmark`、`deleteBookmark`、`deleteBookmarks`

---

### 阶段 4：听书 (TTS) 功能

- [x] 使用 Web Speech Synthesis API
- [x] 语音库选择（系统可用语音列表）
- [x] 播放控制：暂停/继续、上一段/下一段
- [x] 语速调节（0.5-3.0）
- [x] 语调调节（0.5-2.0）
- [x] 定时停止（0-180 分钟）
- [x] 自动翻页：朗读完当前章节自动加载下一章
- [x] 底部控制栏 UI（朗读模式专属）

---

### 阶段 5：章节内容搜索

- [x] 搜索弹窗：输入关键词
- [x] 搜索当前章节
- [x] 搜索结果列表（当前章节匹配文本片段）
- [x] 点击结果 → 跳转并高亮匹配文本


---

### 阶段 6：内容过滤规则

- [x] 过滤规则管理弹窗
- [x] 添加/编辑/删除替换规则（正则 / 纯文本）
- [x] 规则实时预览效果
- [x] 内置默认净化规则（去广告）
- [x] 规则持久化存储（`localStorage`）

---

### 阶段 7：简繁转换

- [x] 集成简繁转换库（或移植旧版 `chinese.js`）
- [x] 简体 ↔ 繁体实时切换
- [x] 转换设置持久化

---

### 阶段 8：书源切换

- [x] 阅读页面内切换当前书的书源
- [x] 显示当前书在各书源下的完整信息（当前书/目标源基础信息对照、匹配提示、目标源预览）
- [x] 切换后重新加载章节列表和内容
- [x] 对应 API：`setBookSource`、`getAvailableBookSource`、`searchBookSourceSSE`

---

### 阶段 9：其他功能

| 功能 | 优先级 | 说明 |
|------|--------|------|
| WebDAV 备份/恢复 | 低 | 备份书架/进度/书签到 WebDAV |
| RSS 订阅 | 已完成 | RSS 源管理、文章列表与正文阅读 |
| 分组管理 | 中 | 创建/编辑/删除书架分组 |
| 批量操作 | 中 | 多选书籍批量删除/移动 |
| 拖拽排序 | 低 | 书架书籍拖拽排序 |
| 阅读统计 | 已完成 | 本地统计阅读时长、打开书籍数、阅读章节数、读完书籍数 |
| PWA 支持 | 已完成 | 离线访问、安装到主屏幕、更新应用、离线章节联动 |
| 移动端适配 | 已完成 | 触摸手势、安全区、弹窗与工具栏适配已完成，后续仅保留设备级微调 |

---

## 三、当前未完成清单（按优先级）

### P1

#### PWA 离线联动子计划

1. [x] 书架增加离线缓存状态标记（显示浏览器离线缓存章节数）
2. [x] 阅读页增加离线模式提示，并明确未缓存章节不可读
3. [x] 应用启动时恢复最近阅读书与本地可读章节状态
4. [x] `offline.html` 与应用壳联动，显示最近阅读与恢复入口
5. [x] 持久化最近阅读元数据，供离线页与冷启动恢复复用

### P2

- [ ] WebDAV 备份/恢复
- [ ] 拖拽排序
- [ ] 构建体积继续优化（如更轻的简繁转换实现 / 进一步细分阅读页依赖）

---

## 四、实施优先级

```
阶段 1  ████████████████████  阅读页面增强     ← 当前重点
阶段 2  ████████████████      书海（探索书源）
阶段 3  ██████████            书签管理
阶段 4  ██████████            听书 (TTS)
阶段 5  ████████              章节搜索
阶段 6  ██████                内容过滤
阶段 7  ████                  简繁转换
阶段 8  ████████              书源切换
阶段 9  ██████                其他功能
```

---

## 五、文件改动计划

### 阶段 1 需修改/新增的文件

```
src/
├── components/
│   ├── reader/
│   │   ├── ReaderSidebar.vue      [NEW]  左侧导航栏
│   │   ├── ReaderToolbar.vue      [NEW]  右侧浮动工具栏
│   │   ├── ReadSettings.vue       [NEW]  完整阅读设置面板
│   │   ├── BookmarkPanel.vue      [NEW]  书签面板
│   │   ├── ChapterSearch.vue      [NEW]  章节内容搜索
│   │   ├── BookInfoPanel.vue      [NEW]  书籍信息面板
│   │   ├── TTSBar.vue             [NEW]  朗读控制栏
│   │   └── ClickZone.vue          [NEW]  翻页区域可视化
│   └── ...
├── views/
│   └── ReaderView.vue             [MODIFY] 重写为完整布局
├── stores/
│   └── reader.ts                  [MODIFY] 增加设置配置项
├── api/
│   ├── bookshelf.ts               [MODIFY] 补充书签/缓存 API
│   └── explore.ts                 [NEW]   书海探索 API
├── utils/
│   ├── speech.ts                  [NEW]   TTS 封装
│   └── chinese.ts                 [NEW]   简繁转换
└── types/
    └── index.ts                   [MODIFY] 增加 Bookmark 等类型
```

### 阶段 2 新增的文件

```
src/
├── components/
│   └── explore/
│       ├── ExploreView.vue        [NEW]  书海主视图
│       ├── SourceExplore.vue      [NEW]  单书源探索
│       └── ExploreGrid.vue        [NEW]  探索结果网格
├── api/
│   └── explore.ts                 [NEW]  探索 API
└── stores/
    └── explore.ts                 [NEW]  探索状态管理
```

---

## 六、后端 API 参考

> 完整 API 文档见 `docs/backend-api.md`

### 书签相关

| 接口 | 方法 | 参数 |
|------|------|------|
| `/reader3/getBookmarks` | GET | `Bookmark[]` |
| `/reader3/saveBookmark` | POST | `Bookmark` |
| `/reader3/deleteBookmark` | POST | `Bookmark` |

### 探索相关

| 接口 | 方法 | 参数 |
|------|------|------|
| `/reader3/exploreBook` | GET/POST | `ruleFindUrl`, `page?`, `bookSourceUrl?` |

### 书源切换

| 接口 | 方法 | 参数 |
|------|------|------|
| `/reader3/setBookSource` | POST | `bookUrl`, `newUrl`, `bookSourceUrl` |

### 章节缓存

| 接口 | 方法 | 参数 |
|------|------|------|
| `/reader3/cacheBookContent` | POST | `bookUrl`, 各种参数 |

---

*文档最后更新：2026-04-03*
