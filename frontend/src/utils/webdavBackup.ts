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
import type { BackupArchiveFile } from '../api/webdav'
import type { Book, BookGroup, Bookmark, BookSource, ReplaceRule, RssSource } from '../types'

const BACKUP_VERSION = 1
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
}

export interface CompatibleBackupParseResult {
  payload: WebdavBackupPayload
  format: 'reader' | 'legado'
  skippedLocalBooks: number
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
  const [books, groups, bookSources, rssSources, bookmarks, replaceRules] = await Promise.all([
    getBookshelf(),
    getBookGroups(),
    getBookSources(),
    getRssSources(),
    getBookmarks(),
    getReplaceRules(),
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
    }
  }

  const rawBooks = parseArchiveList(files, 'bookshelf.json')
  const networkBooks = rawBooks.filter((book) => !isLegadoLocalBook(book))
  const rawGroups = parseArchiveList(files, 'bookGroup.json')
  const normalizedGroups = rawGroups.map(toBookGroup)
  const groups = normalizedGroups
    .filter((group): group is BookGroup => group !== null)
  const normalizedBooks = networkBooks.map(toBook)
  const books = normalizedBooks
    .filter((book): book is Book => book !== null)
  const rawBookSources = parseArchiveList(files, 'bookSource.json')
  const bookSources = rawBookSources
    .filter(hasBookSourceIdentity) as unknown as BookSource[]
  const rawRssSources = parseArchiveList(files, 'rssSources.json')
  const rssSources = rawRssSources
    .filter(hasRssSourceIdentity) as unknown as RssSource[]
  const rawBookmarks = parseArchiveList(files, 'bookmark.json')
  const normalizedBookmarks = rawBookmarks.map(toBookmark)
  const bookmarks = normalizedBookmarks
    .filter((bookmark): bookmark is Bookmark => bookmark !== null)
  const rawReplaceRules = parseArchiveList(files, 'replaceRule.json')
  const normalizedReplaceRules = rawReplaceRules.map(toReplaceRule)
  const replaceRules = normalizedReplaceRules
    .filter((rule): rule is ReplaceRule => rule !== null)

  assertAllValid('bookshelf.json', networkBooks.length, books.length)
  assertAllValid('bookGroup.json', rawGroups.length, groups.length)
  assertAllValid('bookSource.json', rawBookSources.length, bookSources.length)
  assertAllValid('rssSources.json', rawRssSources.length, rssSources.length)
  assertAllValid('bookmark.json', rawBookmarks.length, bookmarks.length)
  assertAllValid('replaceRule.json', rawReplaceRules.length, replaceRules.length)

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

function assertAllValid(name: string, inputCount: number, outputCount: number) {
  if (inputCount !== outputCount) {
    throw new Error(`备份中的 ${name} 包含无法恢复的数据`)
  }
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
