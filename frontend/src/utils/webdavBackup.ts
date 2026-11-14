import { invokeEnvelope } from '../api/invoke'
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
import { getSourceCandidateCache } from './sourceCandidateCache'

const BACKUP_VERSION = 2
/**
 * 不进入备份的 localStorage key。
 *
 * `reader-background-image` / `reader-page-background-image` 存的是图片文件地址
 * (`http://reader.localhost/files?path=default/backgrounds/...`), 图片本体在
 * 本机存储目录里 —— 备份到另一台机器后该地址必然失效, 写进备份只会在恢复时留下
 * 一张打不开的背景。备份保存"难以重建的元数据", 背景图重新选一次即可。
 */
const LOCAL_STORAGE_KEYS_EXCLUDED = new Set([
  'reader-background-image',
  'reader-page-background-image',
])

const LOCAL_STORAGE_KEYS = [
  'theme',
  'reader-stats',
  'readConfig',
  'reader-themeIndex',
  'reader-isNight',
  // 两个背景图 key 本来也在这份清单里, 现已在 LOCAL_STORAGE_KEYS_EXCLUDED 中
  // 显式排除(体积原因), 故不再列入 —— 排除集合对它们仍然生效, 将来的过滤逻辑
  // 统一走那里。
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
    if (LOCAL_STORAGE_KEYS_EXCLUDED.has(key)) return acc
    const value = localStorage.getItem(key)
    if (value != null) {
      acc[key] = value
    }
    return acc
  }, {})
}

function applyLocalState(localState: Record<string, string> = {}) {
  LOCAL_STORAGE_KEYS.forEach((key) => {
    // 背景图不参与备份, 恢复时也就不能因为"备份里没有"而删掉用户当前那张图。
    if (LOCAL_STORAGE_KEYS_EXCLUDED.has(key)) return
    const value = localState[key]
    if (value == null) {
      localStorage.removeItem(key)
    } else {
      localStorage.setItem(key, value)
    }
  })
}

export async function createWebdavBackupPayload(): Promise<WebdavBackupPayload> {
  const [books, groups, bookSources, rssSources, bookmarks, replaceRules, customFonts, readingStats] = await Promise.all([
    getBookshelf(),
    getBookGroups(),
    getBookSources(),
    getRssSources(),
    getBookmarks(),
    getReplaceRules(),
    exportCustomFonts().catch(() => ({ fonts: [], skipped: [], totalBytes: 0 })),
    exportReadingStats().catch(() => ({ daily: [], byBook: [] })),
  ])

  // 本地书正文**不进备份** —— 与手机端 Legado 一致(它的 Backup.kt 只备份书架/
  // 书源/书签等元数据, 不含任何本地书文件)。
  //
  // 这里原本把每本本地书按 base64 塞进 reader-rust.json, 有两条硬伤:
  //   1. base64 膨胀 4/3, 而导出的单本上限是 100MB、合计 300MB, 打包侧却是
  //      单条目 64MB、总内容 128MB —— 两边上限互相矛盾, 本地书合计约 48MB 就
  //      会让整包失败(本地备份与 WebDAV 备份共用这条 payload, 于是同时失败);
  //   2. 备份体积动辄数百 MB, 上传/下载/恢复都很慢。
  //
  // 本地书的正文本来就是用户自己的文件, 换机后重新导入即可 —— 与 Legado 用户
  // 承担的代价相同。这里仍记下"哪些本地书没有备份"(只有 id 与大小, 不含内容),
  // 供恢复后提示用户重新导入。
  const localBooks = await exportLocalBooks().catch(() => ({ books: [], skipped: [], totalBytes: 0 }))
  const skippedLocalBooks = [
    ...localBooks.skipped,
    ...localBooks.books.map((book) => ({ id: book.id, sizeBytes: book.sizeBytes || 0 })),
  ]

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
    // 保持字段存在但为空: 旧版本读取时按"没有本地书"处理, 不需要版本判断。
    localBooks: [],
    skippedLocalBooks,
    customFonts: customFonts.fonts,
    readingStats,
  }
}

export function serializeWebdavBackup(payload: WebdavBackupPayload) {
  return JSON.stringify(payload, null, 2)
}

/**
 * 手机端 Legado 的 Room 实体大量字段是 Kotlin 非空类型；Gson 反序列化时
 * 显式 `null` 会绕过构造器默认值直接写入字段，随后 Room 在 `bindString` /
 * `bindLong` 处抛 "Parameter specified as non-null is null"。
 *
 * 桌面端 Rust 模型里这些字段多为 `Option<T>`，`None` 会序列化成 `null`。
 * 因此生成手机端兼容归档前必须递归清掉 null，并给手机端带默认值的非空字段
 * 补齐默认值 —— 只影响兼容包，不影响 `reader-rust.json` 的原样备份。
 */
function stripNullValues(value: unknown): unknown {
  if (Array.isArray(value)) {
    return value
      .filter((item) => item !== null && item !== undefined)
      .map(stripNullValues)
  }
  if (value !== null && typeof value === 'object') {
    const record: JsonRecord = {}
    for (const [key, item] of Object.entries(value as JsonRecord)) {
      if (item === null || item === undefined) continue
      record[key] = stripNullValues(item)
    }
    return record
  }
  return value
}

function recordOf(value: unknown): JsonRecord {
  return (stripNullValues(value) ?? {}) as JsonRecord
}

function textValue(value: unknown, fallback = '') {
  return typeof value === 'string' ? value : fallback
}

function numberValue(value: unknown, fallback = 0) {
  const number = typeof value === 'number' ? value : Number(value)
  return Number.isFinite(number) ? number : fallback
}

function booleanValue(value: unknown, fallback: boolean) {
  return typeof value === 'boolean' ? value : fallback
}

function compactRecords<T>(
  items: T[],
  normalize: (value: T, fallbackValue: number) => JsonRecord | null,
): JsonRecord[] {
  const records: JsonRecord[] = []
  // 同批缺省主键必须互不相同: 用固定基准 + 序号, 避免 Date.now() 随机兜底撞值。
  const fallbackBase = Date.now()
  for (const [index, item] of items.entries()) {
    const record = normalize(item, fallbackBase + index)
    if (record !== null) records.push(record)
  }
  return records
}

function toLegadoBook(value: Book): JsonRecord | null {
  const record = recordOf(value)
  const bookUrl = textValue(record.bookUrl)
  const origin = textValue(record.origin)
  if (!bookUrl.trim() || !origin.trim()) return null
  return {
    ...record,
    name: textValue(record.name),
    author: textValue(record.author),
    bookUrl,
    origin,
    originName: textValue(record.originName),
    tocUrl: textValue(record.tocUrl),
    type: numberValue(record.type, 0),
    group: numberValue(record.group, 0),
    latestChapterTime: numberValue(record.latestChapterTime, 0),
    lastCheckTime: numberValue(record.lastCheckTime, 0),
    lastCheckCount: numberValue(record.lastCheckCount, 0),
    totalChapterNum: numberValue(record.totalChapterNum, 0),
    durChapterIndex: numberValue(record.durChapterIndex, 0),
    durVolumeIndex: numberValue(record.durVolumeIndex, 0),
    chapterInVolumeIndex: numberValue(record.chapterInVolumeIndex, 0),
    durChapterPos: numberValue(record.durChapterPos, 0),
    durChapterTime: numberValue(record.durChapterTime, 0),
    canUpdate: booleanValue(record.canUpdate, true),
    order: numberValue(record.order, 0),
    originOrder: numberValue(record.originOrder, 0),
    syncTime: numberValue(record.syncTime, 0),
    // 手机端 downloadUrls 是 List<String>?，桌面端存的是字符串；类型不符时
    // Gson 会把整条书架记录判为解析失败（Go 端遗留字段）。统一丢弃，避免
    // 一条脏字段拖垮整本书。
    downloadUrls: undefined,
  }
}

function toLegadoBookGroup(value: BookGroup): JsonRecord | null {
  const record = recordOf(value)
  const groupName = textValue(record.groupName)
  if (!groupName.trim()) return null
  return {
    ...record,
    groupId: numberValue(record.groupId, 1),
    groupName,
    order: numberValue(record.orderNo ?? record.order, 0),
    enableRefresh: booleanValue(record.enableRefresh, true),
    show: booleanValue(record.show, true),
    bookSort: numberValue(record.bookSort, -1),
    onlyUpdateRead: booleanValue(record.onlyUpdateRead, false),
  }
}

function toLegadoBookmark(value: Bookmark, fallbackTime = Date.now()): JsonRecord | null {
  const record = recordOf(value)
  const bookName = textValue(record.bookName)
  const bookAuthor = textValue(record.bookAuthor)
  if (!bookName && !bookAuthor) return null
  return {
    ...record,
    // 手机端 time 是主键；缺失/0 会让多条书签互相覆盖。
    time: numberValue(record.time, 0) || fallbackTime,
    bookName,
    bookAuthor,
    chapterIndex: numberValue(record.chapterIndex, 0),
    chapterPos: numberValue(record.chapterPos, 0),
    chapterName: textValue(record.chapterName),
    bookText: textValue(record.bookText),
    content: textValue(record.content),
  }
}

function toLegadoBookSource(value: BookSource): JsonRecord | null {
  const record = recordOf(value)
  const bookSourceName = textValue(record.bookSourceName)
  const bookSourceUrl = textValue(record.bookSourceUrl)
  if (!bookSourceName.trim() || !bookSourceUrl.trim()) return null
  return {
    ...record,
    bookSourceName,
    bookSourceUrl,
    bookSourceType: numberValue(record.bookSourceType, 0),
    customOrder: numberValue(record.customOrder, 0),
    enabled: booleanValue(record.enabled, true),
    enabledExplore: booleanValue(record.enabledExplore, true),
    enabledCookieJar: booleanValue(record.enabledCookieJar, true),
    lastUpdateTime: numberValue(record.lastUpdateTime, 0),
    respondTime: numberValue(record.respondTime, 180000),
    weight: numberValue(record.weight, 0),
    eventListener: booleanValue(record.eventListener, false),
    customButton: booleanValue(record.customButton, false),
  }
}

function toLegadoRssSource(value: RssSource): JsonRecord | null {
  const record = recordOf(value)
  const sourceName = textValue(record.sourceName)
  const sourceUrl = textValue(record.sourceUrl)
  if (!sourceName.trim() || !sourceUrl.trim()) return null
  return {
    ...record,
    sourceName,
    sourceUrl,
    sourceIcon: textValue(record.sourceIcon),
    enabled: booleanValue(record.enabled, true),
    singleUrl: booleanValue(record.singleUrl, false),
    articleStyle: numberValue(record.articleStyle, 0),
    enableJs: booleanValue(record.enableJs, true),
    loadWithBaseUrl: booleanValue(record.loadWithBaseUrl, true),
    showWebLog: booleanValue(record.showWebLog, false),
    lastUpdateTime: numberValue(record.lastUpdateTime, 0),
    customOrder: numberValue(record.customOrder, 0),
    type: numberValue(record.type, 0),
    preload: booleanValue(record.preload, false),
    cacheFirst: booleanValue(record.cacheFirst, false),
  }
}

function toLegadoReplaceRule(value: ReplaceRule, fallbackId = Date.now()): JsonRecord | null {
  const record = recordOf(value)
  const name = textValue(record.name)
  const pattern = textValue(record.pattern)
  if (!name.trim() || !pattern.trim()) return null
  return {
    ...record,
    // 手机端 id 是自增主键：0 会被 Room 当成"未赋值"重新分配，
    // 多条 0 会撞成同一主键。桌面端 0 视为缺省，给一个稳定且不重复的值。
    id: numberValue(record.id, 0) || fallbackId,
    name,
    pattern,
    replacement: textValue(record.replacement),
    scopeTitle: booleanValue(record.scopeTitle, false),
    scopeContent: booleanValue(record.scopeContent, true),
    isEnabled: booleanValue(record.isEnabled, true),
    isRegex: booleanValue(record.isRegex, true),
    timeoutMillisecond: numberValue(record.timeoutMillisecond, 3000),
    order: numberValue(record.order, 0),
  }
}

export function createCompatibleBackupArchiveFiles(
  payload: WebdavBackupPayload,
): BackupArchiveFile[] {
  const commonBooks = compactRecords(
    payload.bookshelf.books.filter((book) => !isReaderLocalBook(book)),
    toLegadoBook,
  )
  const commonGroups = compactRecords(payload.bookshelf.groups, toLegadoBookGroup)
  const commonBookmarks = compactRecords(payload.bookmarks, toLegadoBookmark)
  const commonBookSources = compactRecords(payload.bookSources, toLegadoBookSource)
  const commonRssSources = compactRecords(payload.rssSources, toLegadoRssSource)
  const commonReplaceRules = compactRecords(payload.replaceRules, toLegadoReplaceRule)

  return [
    { name: 'reader-rust.json', content: serializeWebdavBackup(payload) },
    { name: 'bookshelf.json', content: JSON.stringify(commonBooks) },
    { name: 'bookmark.json', content: JSON.stringify(commonBookmarks) },
    { name: 'bookGroup.json', content: JSON.stringify(commonGroups) },
    { name: 'bookSource.json', content: JSON.stringify(commonBookSources) },
    { name: 'rssSources.json', content: JSON.stringify(commonRssSources) },
    { name: 'replaceRule.json', content: JSON.stringify(commonReplaceRules) },
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
    // 书源整表清空后，详情页/换源面板的「可读书源」候选缓存全部失效
    // （签名是对旧书源清单算的）。这里整份丢掉 —— 这是少见的**全量**变更，
    // 单删/启用/停用都只让对应条目的签名自然对不上即可，不必清空。
    Promise.resolve().then(() => getSourceCandidateCache().clear()),
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
  // 补出来的 id 必须是 2 的幂 —— 书的 group 是位域, 非 2 幂 id 会让书同时
  // 命中多个分组(issue #9)。used 里的非 2 幂旧 id 不在幂序列上, 天然无冲突。
  let next = 1
  return groups.map((group) => {
    if (group.groupId !== 0) return group
    while (used.has(next)) next *= 2
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

  // 旧版本备份可能带非 2 幂的分组 id(saveBookGroupOrder 整表覆盖会原样写回),
  // 书架会重新出现「一本书同时在多个分组」的串扰。迁移只在启动时跑, 这里
  // 立即修一次, 免得用户以为修复无效; 失败静默 —— 下次启动还会兜底重试。
  await invokeEnvelope('repair_book_groups', {}).catch(() => undefined)
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
  const group: BookGroup = {
    groupId,
    groupName,
    orderNo: toFiniteNumber(value.orderNo ?? value.order),
  }
  // 分组级排序偏好(Legado 的 `BookGroup.bookSort`): 缺省表示跟随全局,
  // 有值时原样带上, 否则从手机端恢复后每个分组的排序设置都会被重置。
  if (typeof value.bookSort === 'number' && Number.isFinite(value.bookSort)) {
    group.bookSort = value.bookSort
  }
  return group
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

function isReaderLocalBook(book: Book | null | undefined) {
  // IPC 数据理论上非空，但旧备份/异常数据可能把字段写成 null；
  // 这里先收口，避免判断本身抛 TypeError。
  if (!book) return false
  const origin = typeof book.origin === 'string' ? book.origin : ''
  const bookUrl = typeof book.bookUrl === 'string' ? book.bookUrl : ''
  return origin === 'local-txt'
    || origin === 'local-epub'
    || origin === 'local-pdf'
    || bookUrl.startsWith('local-txt:')
    || bookUrl.startsWith('local-epub:')
    || bookUrl.startsWith('local-pdf:')
}
