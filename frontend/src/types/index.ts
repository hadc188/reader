// ─── API 统一返回 ───
export interface ApiResponse<T = unknown> {
  isSuccess: boolean
  errorMsg: string
  data: T
}

// ─── 书籍 ───
export interface Book {
  name: string
  author: string
  bookUrl: string
  origin: string
  originName?: string
  coverUrl?: string
  tocUrl?: string
  charset?: string
  customCoverUrl?: string
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
  loginCheckJs?: string
  loadWithBaseUrl?: boolean
  singleUrl?: boolean
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
  group?: string
}

export interface BookSourceTestResponse {
  total: number
  valid: number
  invalid: number
  markedInvalid: number
  cancelled: boolean
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

