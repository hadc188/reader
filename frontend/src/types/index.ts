// ─── API 统一返回 ───
export interface ApiResponse<T = unknown> {
  isSuccess: boolean
  errorMsg: string
  data: T
}

// ─── 书籍 ───
export interface BookReadConfig {
  useReplaceRule?: boolean | null
  [key: string]: unknown
}

export interface Book {
  name: string
  author: string
  bookUrl: string
  origin: string
  originName?: string
  coverUrl?: string
  tocUrl?: string
  charset?: string
  readConfig?: BookReadConfig | null
  customCoverUrl?: string
  /** 首次自定义之前的原始值, 供「还原」回退。
   *  后端是 Rust `Option<String>`, 未记录时经 JSON 往返为 `null`(key 存在)。 */
  originalName?: string | null
  originalAuthor?: string | null
  originalIntro?: string | null
  canUpdate?: boolean
  durChapterIndex?: number
  durChapterPos?: number
  durChapterTime?: number
  durChapterTitle?: string
  intro?: string
  latestChapterTitle?: string
  lastCheckTime?: number
  totalChapterNum?: number
  type?: number
  group?: number
  /** Legado 的 `order`: 手动排序时的位置。缺失时该书排在已有序号之后。 */
  order?: number
  wordCount?: string
  infoHtml?: string
  tocHtml?: string
  kind?: string
  updateTime?: string
  cachedChapterCount?: number
  /** 已发现的同一本书的可读书源，加入书架后继续保留。 */
  sourceCandidates?: SearchBook[]
  recentKind?: 'book' | 'rss'
  rssSourceUrl?: string
  rssLink?: string
  rssPubDate?: string
}

// ─── 搜索结果 ───
export interface SearchBook {
  name: string
  author: string
  bookUrl: string
  origin: string
  originName?: string
  originGroup?: string
  coverUrl?: string
  intro?: string
  kind?: string
  lastChapter?: string
  updateTime?: string
  wordCount?: string
  bookSourceUrls?: string[]
  /** Full per-source search hits retained when duplicate books are merged. */
  sourceCandidates?: SearchBook[]
}

// ─── 章节 ───
export interface BookChapter {
  title: string
  url: string
  index: number
  /**
   * 卷标题章（`第一卷` / `第二部` 之类）。这类章节正文天然为空，
   * 阅读页应渲染成居中的大标题页，而不是报「正文解析为空」(issue #33)。
   */
  isVolume?: boolean
}

// ─── 书源 ───
export interface BookSource {
  bookSourceName: string
  bookSourceGroup?: string
  bookSourceUrl: string
  bookSourceType?: number
  enabled?: boolean
  enabledExplore?: boolean
  enabledCookieJar?: boolean
  customOrder?: number
  weight?: number
  searchUrl?: string
  exploreUrl?: string
  header?: string
  loginUrl?: string
  loginUi?: string
  jsLib?: string
  loginCheckJs?: string
  loadWithBaseUrl?: boolean
  singleUrl?: boolean
  lastUpdateTime?: number
  ruleSearch?: Record<string, unknown>
  ruleExplore?: Record<string, unknown>
  ruleBookInfo?: Record<string, unknown>
  ruleToc?: Record<string, unknown>
  ruleContent?: Record<string, unknown>
}

export interface BookSourceTestResult {
  bookSourceName: string
  bookSourceUrl: string
  valid: boolean
  searchOk: boolean
  exploreOk: boolean
  keyword: string
  exploreUrl?: string
  searchError?: string
  exploreError?: string
  markedInvalid: boolean
  /** 该条因用户中止而未检测完成(既不算有效也不算失效)。 */
  cancelled?: boolean
  group?: string
}

export interface BookSourceTestResponse {
  total: number
  valid: number
  invalid: number
  markedInvalid: number
  cancelled: boolean
  /** 因中止而未完成的条数。 */
  cancelledCount?: number
  results: BookSourceTestResult[]
}

export interface DebugTrace {
  requestUrl: string
  status: number
  body: string
  result: unknown
  /** 反爬/异常特征提示(202、var buid、验证码、重定向等)。 */
  warnings?: string[]
  /** 响应头(Set-Cookie / Location / Content-Type 等)。 */
  headers?: [string, string][]
}

// ─── 分组 ───
export interface BookGroup {
  groupId: number
  groupName: string
  orderNo?: number
  /** 该分组使用的书架排序方式; 缺省或 -1 表示跟随全局设置。
   *  取值见 `utils/bookshelfSort.ts` 的 `BOOK_SORT_KEYS`。 */
  bookSort?: number
}

// ─── 应用更新 ───
export interface VersionUpdateAsset {
  name: string
  browserDownloadUrl: string
  size: number
}

export interface VersionUpdateInfo {
  currentVersion: string
  latestVersion: string | null
  latestName: string | null
  releaseUrl: string | null
  publishedAt: string | null
  updateAvailable: boolean
  shouldRemind: boolean
  dismissedVersion: string | null
  checkedAt: number
  error: string | null
  assets: VersionUpdateAsset[]
}

export interface DesktopUpdateResult {
  mode: 'installer' | 'portable'
  platform: 'windows' | 'macos' | 'linux'
  assetName: string
  message: string
}

export interface DesktopUpdateProgress {
  stage: 'checking' | 'downloading' | 'verifying' | 'ready' | 'failed'
  percent: number | null
  downloaded: number
  total: number
  message: string
}

// ─── 书签 ───
export interface Bookmark {
  time?: number
  bookName: string
  bookAuthor: string
  chapterIndex?: number
  chapterPos?: number
  chapterName?: string
  bookText?: string
  content?: string
}

// ─── 净化规则 ───
export interface ReplaceRule {
  id: number
  name: string
  group?: string
  pattern: string
  replacement: string
  scope?: string
  isEnabled: boolean
  isRegex: boolean
  order: number
}

// ─── 在线朗读引擎 (Legado httpTTS) ───
/**
 * 一条「在线朗读引擎」配置，字段与 Legado TTS 规则 JSON 对齐。
 *
 * `url` 里含由**后端**展开的模板（`{{java.encodeURI(java.encodeURI(speakText))}}`
 * 等），前端只负责原样保存与挑选引擎，绝不解析模板 —— 求值要复用 rquickjs
 * 沙盒，前端没有等价能力。
 *
 * 除 `id`/`name`/`url` 外全部可选：Legado 源站实际数据里大量条目缺字段，
 * 反序列化必须宽容，缺了也不能整条丢弃。
 */
export interface HttpTts {
  id: number
  name: string
  url: string
  /** 引擎期望的响应 MIME。多数条目为空 —— 后端 IPC 返回的是裸字节，没有 HTTP
   *  头，因此播放前必须靠魔数嗅探，这个字段只作兜底。
   *  可选字段是 Rust `Option`，经 IPC 序列化后**可能是 null**而不是缺失。 */
  contentType?: string | null
  header?: string | null
  concurrentRate?: string | null
  /** Legado 的合成并发线程数（1–8）。缺省/0 表示不限并发（本应用的前端缓冲流
   *  自己控制在 2 个），只有引擎明确写了数字时后端才按它限流。 */
  synthesisThreadCount?: number | null
  enabledCookieJar?: boolean | null
  loginUrl?: string | null
  loginUi?: string | null
  loginCheckJs?: string | null
  lastUpdateTime?: number | string | null
}

// ─── RSS ───
export interface RssSource {
  sourceUrl: string
  sourceName: string
  sourceIcon?: string
  sourceGroup?: string
  sourceComment?: string
  enabled?: boolean
  enabledCookieJar?: boolean
  concurrentRate?: string
  header?: string
  loginUrl?: string
  loginCheckJs?: string
  sortUrl?: string
  singleUrl?: boolean
  articleStyle?: number
  ruleArticles?: string
  ruleNextPage?: string
  ruleTitle?: string
  rulePubDate?: string
  ruleDescription?: string
  ruleImage?: string
  ruleLink?: string
  ruleContent?: string
  style?: string
  enableJs?: boolean
  loadWithBaseUrl?: boolean
  customOrder?: number
  lastUpdateTime?: number
}

export interface RssArticle {
  origin: string
  sort: string
  title: string
  order: number
  link: string
  pubDate?: string
  description?: string
  content?: string
  image?: string
  read?: boolean
  variable?: string
}

