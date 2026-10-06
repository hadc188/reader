import { getBookmarks, deleteBookmarks, saveBookmarks } from '../api/bookmark'
import {
  deleteBookGroups,
  deleteBooks,
  getBookGroups,
  getBookshelf,
  saveBookGroupOrder,
  saveBooks,
} from '../api/bookshelf'
import { listCustomFonts, deleteCustomFont } from '../api/fonts'
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
  // 阅读页专属背景图与透明度: 存在独立的 key 里(不随 readConfig 走),
  // 漏掉它会导致恢复备份后阅读页背景图丢失。
  'reader-page-background-image',
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
  // 自定义快捷键绑定。
  'reader-hotkeys',
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

/** 恢复前的清理: 把当前数据读出来再逐个删除。
 *
 *  这里**故意不吞异常**(此前的 `.catch(() => [])` 是错的): 一旦读取失败退回空数组,
 *  后面的清理就会被 `length` 守卫整块跳过, 而写入侧的分组/书签/替换规则都是
 *  **upsert 合并**语义(见 save_book_group / save_bookmarks / save_replace_rules),
 *  于是旧数据原地残留、和新数据混在一起 —— 恢复出来的是两批数据的并集,
 *  而且全程没有任何报错。宁可让恢复失败并报错, 也不要静默产生脏数据。 */
async function clearCurrentData() {
  const [groups, books, bookmarks, replaceRules, rssSources] = await Promise.all([
    getBookGroups(),
    getBookshelf(),
    getBookmarks(),
    getReplaceRules(),
    getRssSources(),
  ])

  await Promise.all([
    // 必须一次批量删, 不能用 Promise.all(groups.map(deleteBookGroup)) 并发逐个删:
    // 后端 delete_group 是「读列表 → 删一个 → 写回」, 并发时会交错
    // (A 读 [1,2,3]、B 读 [1,2,3]、A 写 [2,3]、B 写 [1,3] → 分组 1 复活),
    // 于是"恢复后旧分组残留"依然会出现 —— 这正是用户最初报的那个现象。
    groups.length
      ? deleteBookGroups(groups.map((group) => group.groupId))
      : Promise.resolve(),
    books.length
      ? deleteBooks(books.map((book) => ({ bookUrl: book.bookUrl, origin: book.origin })) as Book[])
      : Promise.resolve(),
    bookmarks.length ? deleteBookmarks(bookmarks) : Promise.resolve(),
    replaceRules.length ? deleteReplaceRules(replaceRules) : Promise.resolve(),
    rssSources.length
      ? Promise.all(rssSources.map((source) => deleteRssSource({
          sourceUrl: source.sourceUrl,
          sourceName: source.sourceName,
        })))
      : Promise.resolve(),
    // 书源没有「按列表删」的接口, 直接整表清空。
    deleteAllBookSources(),
    // 自定义字体此前完全没清: import_custom_fonts 只 fs::write, 不同名的旧字体会
    // 一直留在字体目录里, 继续出现在字体列表里。
    clearCustomFonts(),
  ])
}

/** 清空全部自定义字体文件。 */
async function clearCustomFonts() {
  const fonts = await listCustomFonts()
  if (!fonts.length) return
  await Promise.all(fonts.map((font) => deleteCustomFont(font.id)))
}

/** 补齐 id 为 0 的分组。
 *
 *  注意: 正常路径下这里收不到 groupId 为 0 的分组 —— `toBookGroup` 会把
 *  「groupId 为 0 / 缺失」的条目当成无效数据剔除(既有防呆, 位置见 normalizeList),
 *  因为写进书架一个没有 id 的分组无法被任何书引用。本函数只作为防御: 若将来
 *  有人放宽了解析层的校验, 这里保证不会把 0 原样写进存储。
 *
 *  只动 0, 其余一律原样保留 —— 尤其是**负数**: Legado 用一批负值作保留/虚拟分组
 *  (IdRoot=-100, IdAll=-1, IdLocal=-2, IdAudio=-3, IdNetNone=-4, IdLocalNone=-5,
 *  IdVideo=-6, IdError=-11), 它们是合法且有语义的。用 `groupId > 0` 当判据会把
 *  这些负数当成「缺 id」改写掉, 破坏从 Legado 导入的分组语义。 */
function normalizeGroupIds(groups: BookGroup[]): BookGroup[] {
  const used = new Set(groups.map((g) => g.groupId).filter((id) => id !== 0))
  let next = 1
  return groups.map((group) => {
    if (group.groupId !== 0) return group
    while (used.has(next)) next += 1
    used.add(next)
    return { ...group, groupId: next }
  })
}

/** 恢复过程所处的阶段, 用于给用户准确的失败提示。
 *
 *  `clear` 阶段已经在删旧数据(所以失败后数据可能不完整), 但还**一个字都没写入**;
 *  `write` 阶段则是真的写了一半。两者的提示语不该相同 —— 早先调用方只有一个
 *  `writing` 布尔量, 在调用前就置位, 于是清理阶段失败也会报"已开始写入"。 */
export type RestorePhase = 'clear' | 'write'

export interface RestoreOptions {
  /** 每次进入新阶段时回调, 让调用方把提示文案对齐到真实进度。 */
  onPhase?: (phase: RestorePhase) => void
}

export async function restoreWebdavBackup(
  payload: WebdavBackupPayload,
  options: RestoreOptions = {},
) {
  options.onPhase?.('clear')
  await clearCurrentData()

  options.onPhase?.('write')

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
  // 分组用「整表覆盖」写入, 而不是逐个 saveBookGroup: 后者是 upsert, groupId 为 0
  // 时后端会另分配一个新 id 再 push —— 备份里的 id 与书架 group 位域就对不上了。
  if (payload.bookshelf.groups.length) {
    await saveBookGroupOrder(normalizeGroupIds(payload.bookshelf.groups))
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
