import { getBookmarks, deleteBookmarks, saveBookmarks } from '../api/bookmark'
import {
  deleteBookGroup,
  deleteBooks,
  getBookGroups,
  getBookshelf,
  saveBooks,
  saveBookGroup,
} from '../api/bookshelf'
import { getReplaceRules, deleteReplaceRules, saveReplaceRules } from '../api/replaceRule'
import { getRssSources, deleteRssSource, saveRssSources } from '../api/rss'
import {
  deleteAllBookSources,
  getBookSources,
  saveBookSources,
} from '../api/source'
import {
  exportCustomFonts,
  exportLocalBooks,
  exportReadingStats,
  importCustomFonts,
  importLocalBooks,
  importReadingStats,
} from '../api/backup'
import type {
  CustomFontExportItem,
  LocalBookExportItem,
  ReadingStatsExport,
} from '../api/backup'
import type { BackupArchiveFile } from '../api/webdav'
import type { Book, BookGroup, Bookmark, BookSource, ReplaceRule, RssSource } from '../types'

const BACKUP_VERSION = 2
const LOCAL_STORAGE_KEYS = [
  'theme',
  'reader-stats',
  'readConfig',
  'reader-themeIndex',
  'reader-isNight',
  'reader-background-image',
  'reader-speechConfig',
  'reader-last-session',
  'reader-currentIndex',
  'reader-source-subscriptions',
  'reader-legado-sync-enabled',
  'reader-close-to-tray',
  'reader-boss-key',
  'reader-hidden-features',
  'reader-auto-check-update',
  'reader-toc-check-times',
]

export interface WebdavBackupPayload {
  version: number
  createdAt: string
  app: string
  bookshelf: {
    books: Book[]
    groups: BookGroup[]
  }
  bookSources: BookSource[]
  rssSources: RssSource[]
  bookmarks: Bookmark[]
  replaceRules: ReplaceRule[]
  localState: Record<string, string>
  /** v2: 本地书内容文件(base64), 恢复时写回存储目录。 */
  localBooks?: LocalBookExportItem[]
  /** v2: 因超过导出上限未包含在备份中的本地书。 */
  skippedLocalBooks?: Array<{ id: string; sizeBytes: number }>
  /** v2: 自定义字体文件(base64)。 */
  customFonts?: CustomFontExportItem[]
  /** v2: Rust 侧阅读统计(按日 + 按书)。 */
  readingStats?: ReadingStatsExport
}

/** 备份中因缺少必要标识(如书源名/地址为空)而无法恢复的条目。 */
export interface SkippedEntryReport {
  /** 条目所属的备份文件, 如 `bookSource.json`。 */
  file: string
  /** 被跳过的条目数量。 */
  count: number
  /** 最多若干条可识别的样例名, 便于用户判断跳过是否安全。 */
  samples: string[]
  /**
   * 该文件里的条目是否全部被跳过。
   *
   * 恢复是"先清空再写入", 全部跳过会让这一类数据被清空且没有写回。调用方
   * 应在执行前提醒用户, 这是不可逆的。
   */
  allSkipped: boolean
}

export interface CompatibleBackupParseResult {
  payload: WebdavBackupPayload
  format: 'reader' | 'legado'
  skippedLocalBooks: number
  /**
   * 解析时跳过的无效条目, 按文件汇总; 空数组表示全部有效。
   *
   * 只有"表述性无效"(字段为空、缺少标识)才会被跳过; 文件解析失败、
   * 顶层不是数组、元素不是对象这类"结构性损坏"仍然直接报错。
   */
  skipped: SkippedEntryReport[]
}

type JsonRecord = Record<string, unknown>

function captureLocalState() {
  return LOCAL_STORAGE_KEYS.reduce<Record<string, string>>((acc, key) => {
    const value = localStorage.getItem(key)
    if (value != null) {
      acc[key] = value
    }
    return acc
  }, {})
}

function applyLocalState(localState: Record<string, string> = {}) {
  LOCAL_STORAGE_KEYS.forEach((key) => {
    const value = localState[key]
    if (value == null) {
      localStorage.removeItem(key)
    } else {
      localStorage.setItem(key, value)
    }
  })
}

export async function createWebdavBackupPayload(): Promise<WebdavBackupPayload> {
  const [books, groups, bookSources, rssSources, bookmarks, replaceRules, localBooks, customFonts, readingStats] = await Promise.all([
    getBookshelf(),
    getBookGroups(),
    getBookSources(),
    getRssSources(),
    getBookmarks(),
    getReplaceRules(),
    exportLocalBooks().catch(() => ({ books: [], skipped: [], totalBytes: 0 })),
    exportCustomFonts().catch(() => ({ fonts: [], skipped: [], totalBytes: 0 })),
    exportReadingStats().catch(() => ({ daily: [], byBook: [] })),
  ])

  return {
    version: BACKUP_VERSION,
    createdAt: new Date().toISOString(),
    app: 'reader-rust-frontend',
    bookshelf: {
      books,
      groups,
    },
    bookSources,
    rssSources,
    bookmarks,
    replaceRules,
    localState: captureLocalState(),
    localBooks: localBooks.books,
    skippedLocalBooks: localBooks.skipped,
    customFonts: customFonts.fonts,
    readingStats,
  }
}

export function serializeWebdavBackup(payload: WebdavBackupPayload) {
  return JSON.stringify(payload, null, 2)
}

export function createCompatibleBackupArchiveFiles(
  payload: WebdavBackupPayload,
): BackupArchiveFile[] {
  const commonBooks = payload.bookshelf.books.filter((book) => !isReaderLocalBook(book))
  const commonGroups = payload.bookshelf.groups.map((group) => ({
    ...group,
    order: group.orderNo || 0,
  }))

  return [
    { name: 'reader-rust.json', content: serializeWebdavBackup(payload) },
    { name: 'bookshelf.json', content: JSON.stringify(commonBooks) },
    { name: 'bookmark.json', content: JSON.stringify(payload.bookmarks) },
    { name: 'bookGroup.json', content: JSON.stringify(commonGroups) },
    { name: 'bookSource.json', content: JSON.stringify(payload.bookSources) },
    { name: 'rssSources.json', content: JSON.stringify(payload.rssSources) },
    { name: 'replaceRule.json', content: JSON.stringify(payload.replaceRules) },
  ]
}

export function parseWebdavBackup(raw: string) {
  const payload = JSON.parse(raw) as Partial<WebdavBackupPayload>
  if (!payload || typeof payload !== 'object') {
    throw new Error('备份文件格式无效')
  }
  if (!payload.version || !payload.bookshelf) {
    throw new Error('备份文件缺少必要字段')
  }
  return {
    version: payload.version,
    createdAt: payload.createdAt || new Date().toISOString(),
    app: payload.app || 'reader-rust-frontend',
    bookshelf: {
      books: payload.bookshelf.books || [],
      groups: payload.bookshelf.groups || [],
    },
    bookSources: payload.bookSources || [],
    rssSources: payload.rssSources || [],
    bookmarks: payload.bookmarks || [],
    replaceRules: payload.replaceRules || [],
    localState: payload.localState || {},
    // v1 备份没有这些字段, 默认为空即可。
    localBooks: payload.localBooks || [],
    skippedLocalBooks: payload.skippedLocalBooks || [],
    customFonts: payload.customFonts || [],
    readingStats: payload.readingStats || { daily: [], byBook: [] },
  } as WebdavBackupPayload
}

export function parseCompatibleBackupArchive(
  files: Record<string, string>,
): CompatibleBackupParseResult {
  const readerBackup = files['reader-rust.json']
  if (readerBackup) {
    return {
      payload: parseWebdavBackup(readerBackup),
      format: 'reader',
      skippedLocalBooks: 0,
      skipped: [],
    }
  }

  const skipped: SkippedEntryReport[] = []

  const rawBooks = parseArchiveList(files, 'bookshelf.json')
  // 安卓专属本地书是被有意排除的(见 skippedLocalBooks), 不计入无效条目。
  const networkBooks = rawBooks.filter((book) => !isLegadoLocalBook(book))
  const groups = normalizeList(
    parseArchiveList(files, 'bookGroup.json'),
    toBookGroup,
    { file: 'bookGroup.json', label: (v) => stringField(v, 'groupName') },
    skipped,
  )
  const books = normalizeList(
    networkBooks,
    toBook,
    { file: 'bookshelf.json', label: (v) => stringField(v, 'name') },
    skipped,
  )
  const bookSources = normalizeList(
    parseArchiveList(files, 'bookSource.json'),
    (value) => (hasBookSourceIdentity(value) ? (value as unknown as BookSource) : null),
    { file: 'bookSource.json', label: (v) => stringField(v, 'bookSourceName') },
    skipped,
  )
  const rssSources = normalizeList(
    parseArchiveList(files, 'rssSources.json'),
    (value) => (hasRssSourceIdentity(value) ? (value as unknown as RssSource) : null),
    { file: 'rssSources.json', label: (v) => stringField(v, 'sourceName') },
    skipped,
  )
  const bookmarks = normalizeList(
    parseArchiveList(files, 'bookmark.json'),
    toBookmark,
    { file: 'bookmark.json', label: (v) => stringField(v, 'bookName') },
    skipped,
  )
  const replaceRules = normalizeList(
    parseArchiveList(files, 'replaceRule.json'),
    toReplaceRule,
    { file: 'replaceRule.json', label: (v) => stringField(v, 'name') },
    skipped,
  )

  return {
    payload: {
      version: BACKUP_VERSION,
      createdAt: new Date().toISOString(),
      app: 'legado',
      bookshelf: { books, groups },
      bookSources,
      rssSources,
      bookmarks,
      replaceRules,
      localState: {},
    },
    format: 'legado',
    skippedLocalBooks: rawBooks.length - networkBooks.length,
    skipped,
  }
}

export async function restoreWebdavBackup(payload: WebdavBackupPayload) {
  const currentGroups = await getBookGroups().catch(() => [])
  const currentBooks = await getBookshelf().catch(() => [])
  const currentBookmarks = await getBookmarks().catch(() => [])
  const currentReplaceRules = await getReplaceRules().catch(() => [])
  const currentRssSources = await getRssSources().catch(() => [])

  await Promise.all([
    currentGroups.length
      ? Promise.all(currentGroups.map((group) => deleteBookGroup(group.groupId)))
      : Promise.resolve(),
    currentBooks.length
      ? deleteBooks(currentBooks.map((book) => ({ bookUrl: book.bookUrl, origin: book.origin })) as Book[])
      : Promise.resolve(),
    currentBookmarks.length ? deleteBookmarks(currentBookmarks) : Promise.resolve(),
    currentReplaceRules.length ? deleteReplaceRules(currentReplaceRules) : Promise.resolve(),
    currentRssSources.length
      ? Promise.all(currentRssSources.map((source) => deleteRssSource({
          sourceUrl: source.sourceUrl,
          sourceName: source.sourceName,
        })))
      : Promise.resolve(),
    deleteAllBookSources().catch(() => undefined),
  ])

  if (payload.app !== 'legado' && payload.localBooks?.length) {
    // 本地书内容文件先落盘, 再写书架记录, 保证记录指向的文件已存在。
    await importLocalBooks(payload.localBooks)
  }

  if (payload.bookSources.length) {
    await saveBookSources(payload.bookSources)
  }
  if (payload.rssSources.length) {
    await saveRssSources(payload.rssSources)
  }
  for (const group of payload.bookshelf.groups) {
    await saveBookGroup(group)
  }
  if (payload.bookshelf.books.length) {
    await saveBooks(payload.bookshelf.books)
  }
  if (payload.bookmarks.length) {
    await saveBookmarks(payload.bookmarks)
  }
  if (payload.replaceRules.length) {
    await saveReplaceRules(payload.replaceRules)
  }

  if (payload.app !== 'legado') {
    applyLocalState(payload.localState)
    // 字体与统计恢复失败不阻断整体恢复(书架已就绪), 静默降级。
    if (payload.customFonts?.length) {
      await importCustomFonts(payload.customFonts).catch(() => undefined)
    }
    if (payload.readingStats && (payload.readingStats.daily.length || payload.readingStats.byBook.length)) {
      await importReadingStats(payload.readingStats).catch(() => undefined)
    }
  }
}

function parseArchiveList(files: Record<string, string>, name: string): JsonRecord[] {
  const raw = files[name]
  if (raw == null || raw.trim() === '') return []
  let parsed: unknown
  try {
    parsed = JSON.parse(raw)
  } catch {
    throw new Error(`备份中的 ${name} 格式无效`)
  }
  if (!Array.isArray(parsed)) {
    throw new Error(`备份中的 ${name} 不是数据列表`)
  }
  if (!parsed.every(isJsonRecord)) {
    throw new Error(`备份中的 ${name} 包含无效数据`)
  }
  return parsed
}

function isJsonRecord(value: unknown): value is JsonRecord {
  return typeof value === 'object' && value !== null && !Array.isArray(value)
}

/** 样例名最多保留的条数, 避免提示信息过长。 */
const MAX_SKIP_SAMPLES = 3

function stringField(value: JsonRecord, key: string) {
  const raw = value[key]
  return typeof raw === 'string' && raw.trim() ? raw.trim() : '(未命名)'
}

/**
 * 逐条规范化并跳过无效条目, 把被跳过的数量与样例汇总到 `skipped`。
 *
 * 单条数据无法恢复(如书源名为空)不应中断整个恢复流程: 一份 4095 条书源的
 * 备份里只要有一条空书源, 旧的全有无校验就会让其余 4094 条也全部无法导入。
 * 结构性问题(JSON 解析失败、顶层不是数组、元素不是对象)仍由
 * `parseArchiveList` 直接报错, 不会被静默跳过。
 */
function normalizeList<T>(
  raw: JsonRecord[],
  normalize: (value: JsonRecord) => T | null,
  context: { file: string; label: (value: JsonRecord) => string },
  skipped: SkippedEntryReport[],
): T[] {
  const kept: T[] = []
  const samples: string[] = []
  for (const value of raw) {
    const item = normalize(value)
    if (item !== null) {
      kept.push(item)
    } else if (samples.length < MAX_SKIP_SAMPLES) {
      samples.push(context.label(value))
    }
  }
  const count = raw.length - kept.length
  if (count > 0) {
    skipped.push({
      file: context.file,
      count,
      samples,
      // raw 非空时 kept 为空, 说明这一类数据会被整体清空。
      allSkipped: raw.length > 0 && kept.length === 0,
    })
  }
  return kept
}

function toFiniteNumber(value: unknown, fallback = 0) {
  const number = typeof value === 'number' ? value : Number(value)
  return Number.isFinite(number) ? number : fallback
}

function toBook(value: JsonRecord): Book | null {
  const name = typeof value.name === 'string' ? value.name : ''
  const author = typeof value.author === 'string' ? value.author : ''
  const bookUrl = typeof value.bookUrl === 'string' ? value.bookUrl : ''
  const origin = typeof value.origin === 'string' ? value.origin : ''
  if (!bookUrl || !origin) return null
  return { ...value, name, author, bookUrl, origin } as unknown as Book
}

function toBookGroup(value: JsonRecord): BookGroup | null {
  const groupId = toFiniteNumber(value.groupId)
  const groupName = typeof value.groupName === 'string' ? value.groupName : ''
  if (!groupId || !groupName) return null
  return {
    groupId,
    groupName,
    orderNo: toFiniteNumber(value.orderNo ?? value.order),
  }
}

function toBookmark(value: JsonRecord): Bookmark | null {
  const bookName = typeof value.bookName === 'string' ? value.bookName : ''
  const bookAuthor = typeof value.bookAuthor === 'string' ? value.bookAuthor : ''
  if (!bookName && !bookAuthor) return null
  return {
    time: toFiniteNumber(value.time, Date.now()),
    bookName,
    bookAuthor,
    chapterIndex: toFiniteNumber(value.chapterIndex),
    chapterPos: toFiniteNumber(value.chapterPos),
    chapterName: typeof value.chapterName === 'string' ? value.chapterName : '',
    bookText: typeof value.bookText === 'string' ? value.bookText : '',
    content: typeof value.content === 'string' ? value.content : '',
  }
}

function toReplaceRule(value: JsonRecord): ReplaceRule | null {
  const name = typeof value.name === 'string' ? value.name : ''
  const pattern = typeof value.pattern === 'string' ? value.pattern : ''
  if (!name || !pattern) return null
  return {
    id: toFiniteNumber(value.id, Date.now()),
    name,
    group: typeof value.group === 'string' ? value.group : undefined,
    pattern,
    replacement: typeof value.replacement === 'string' ? value.replacement : '',
    scope: typeof value.scope === 'string' ? value.scope : undefined,
    isEnabled: typeof value.isEnabled === 'boolean' ? value.isEnabled : true,
    isRegex: typeof value.isRegex === 'boolean' ? value.isRegex : true,
    order: toFiniteNumber(value.order),
  }
}

function hasBookSourceIdentity(value: JsonRecord) {
  return typeof value.bookSourceName === 'string'
    && value.bookSourceName.length > 0
    && typeof value.bookSourceUrl === 'string'
    && value.bookSourceUrl.length > 0
}

function hasRssSourceIdentity(value: JsonRecord) {
  return typeof value.sourceName === 'string'
    && value.sourceName.length > 0
    && typeof value.sourceUrl === 'string'
    && value.sourceUrl.length > 0
}

function isLegadoLocalBook(value: JsonRecord) {
  const origin = typeof value.origin === 'string' ? value.origin : ''
  const type = toFiniteNumber(value.type)
  return origin === 'loc_book'
    || origin.startsWith('loc_book::')
    || origin.startsWith('webDav::')
    || (type & 0b100000000) !== 0
}

function isReaderLocalBook(book: Book) {
  return book.origin === 'local-txt'
    || book.origin === 'local-epub'
    || book.origin === 'local-pdf'
    || book.bookUrl.startsWith('local-txt:')
    || book.bookUrl.startsWith('local-epub:')
    || book.bookUrl.startsWith('local-pdf:')
}
