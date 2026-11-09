import type { Book, Bookmark } from '../types'

export interface XmnoteEntry {
  text?: string
  note?: string
  chapter?: string
  time?: number
}

export interface XmnoteBookPayload {
  title: string
  author?: string
  bookSummary?: string
  cover?: string
  type: 1
  locationUnit: 0
  chapters?: Array<{ title: string }>
  totalPageCount?: number
  currentPage?: number
  readingStatus?: 2 | 3
  readingStatusChangedDate?: number
  entries?: XmnoteEntry[]
}

export interface XmnoteBookDraft {
  book: Book
  bookmarks: Bookmark[]
  chapterTitles?: string[]
}

export interface XmnoteBuildOptions {
  includeProgress?: boolean
  includeEntries?: boolean
}

const PROGRESS_SCALE = 10000

/** 接受地址栏里的 IP、基础地址或完整 `/send` 地址。 */
export function normalizeXmnoteEndpoint(raw: string): string {
  let value = raw.trim()
  if (!value) throw new Error('请填写纸间书摘导入地址')
  if (!/^[a-z][a-z\d+.-]*:\/\//i.test(value)) value = `http://${value}`

  let url: URL
  try {
    url = new URL(value)
  } catch {
    throw new Error('纸间书摘导入地址格式不正确')
  }
  if (url.protocol !== 'http:' && url.protocol !== 'https:') {
    throw new Error('纸间书摘导入地址只支持 HTTP 或 HTTPS')
  }
  if (!url.hostname) throw new Error('纸间书摘导入地址缺少主机名')

  if (!url.pathname || url.pathname === '/') {
    url.pathname = '/send'
  }
  url.hash = ''
  return url.toString()
}

function normalizeChapterProgress(position?: number): number {
  if (typeof position !== 'number' || !Number.isFinite(position) || position <= 0) return 0
  if (position > 1) return Math.max(0, Math.min(1, position / PROGRESS_SCALE))
  return Math.max(0, Math.min(1, position))
}

function toUnixSeconds(value?: number): number | undefined {
  if (typeof value !== 'number' || !Number.isFinite(value) || value <= 0) return undefined
  return Math.floor(value < 1e11 ? value : value / 1000)
}

export function buildXmnoteBookPayload(
  book: Book,
  bookmarks: Bookmark[],
  chapterTitles: string[] = [],
  options: XmnoteBuildOptions = {},
): XmnoteBookPayload {
  const includeProgress = options.includeProgress !== false
  const includeEntries = options.includeEntries !== false
  const totalChapters = Math.max(
    1,
    book.totalChapterNum || 0,
    chapterTitles.length,
    (book.durChapterIndex || 0) + 1,
    bookmarks.reduce((max, item) => Math.max(max, (item.chapterIndex || 0) + 1), 0),
  )

  const payload: XmnoteBookPayload = {
    title: book.name.trim() || '未命名书籍',
    type: 1,
    locationUnit: 0,
  }

  if (book.author.trim()) payload.author = book.author.trim()
  if (book.intro?.trim()) payload.bookSummary = book.intro.trim()
  if (book.coverUrl && /^https?:\/\//i.test(book.coverUrl)) payload.cover = book.coverUrl
  if (chapterTitles.length) {
    payload.chapters = chapterTitles.map((title) => ({ title }))
  }

  if (includeProgress) {
    const chapterIndex = Math.max(0, book.durChapterIndex || 0)
    const chapterProgress = normalizeChapterProgress(book.durChapterPos)
    const progressPercent = Math.max(
      0,
      Math.min(100, ((chapterIndex + chapterProgress) / totalChapters) * 100),
    )
    payload.totalPageCount = totalChapters
    payload.currentPage = Number(progressPercent.toFixed(2))
    payload.readingStatus = progressPercent >= 99.5 ? 3 : 2
    const changedAt = toUnixSeconds(book.durChapterTime)
    if (changedAt) payload.readingStatusChangedDate = changedAt
  }

  if (includeEntries) {
    const entries = bookmarks
      .map((bookmark): XmnoteEntry | null => {
        const text = bookmark.bookText?.trim() || ''
        const note = bookmark.content?.trim() || ''
        if (!text && !note) return null
        const entry: XmnoteEntry = {}
        if (text) entry.text = text
        if (note) entry.note = note
        if (bookmark.chapterName?.trim()) entry.chapter = bookmark.chapterName.trim()
        const time = toUnixSeconds(bookmark.time)
        if (time) entry.time = time
        return entry
      })
      .filter((entry): entry is XmnoteEntry => entry !== null)
    if (entries.length) payload.entries = entries
  }

  return payload
}

export function groupXmnoteBookDrafts(books: Book[], bookmarks: Bookmark[]): XmnoteBookDraft[] {
  const bookmarkMap = new Map<string, Bookmark[]>()
  for (const bookmark of bookmarks) {
    const key = `${bookmark.bookName}\u0000${bookmark.bookAuthor}`
    const list = bookmarkMap.get(key)
    if (list) list.push(bookmark)
    else bookmarkMap.set(key, [bookmark])
  }
  return books.map((book) => ({
    book,
    bookmarks: bookmarkMap.get(`${book.name}\u0000${book.author}`) || [],
  }))
}
