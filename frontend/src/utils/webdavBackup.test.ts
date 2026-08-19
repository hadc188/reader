import { describe, expect, it, vi } from 'vitest'
import type { WebdavBackupPayload } from './webdavBackup'
import {
  createCompatibleBackupArchiveFiles,
  createWebdavBackupPayload,
  parseCompatibleBackupArchive,
  parseWebdavBackup,
  restoreWebdavBackup,
} from './webdavBackup'
import { importCustomFonts, importLocalBooks, importReadingStats } from '../api/backup'

vi.mock('../api/bookmark', () => ({
  getBookmarks: vi.fn(async () => []),
  deleteBookmarks: vi.fn(async () => undefined),
  saveBookmarks: vi.fn(async () => undefined),
}))
vi.mock('../api/bookshelf', () => ({
  getBookshelf: vi.fn(async () => []),
  getBookGroups: vi.fn(async () => []),
  saveBooks: vi.fn(async () => undefined),
  saveBookGroup: vi.fn(async () => undefined),
  deleteBooks: vi.fn(async () => undefined),
  deleteBookGroup: vi.fn(async () => undefined),
}))
vi.mock('../api/replaceRule', () => ({
  getReplaceRules: vi.fn(async () => []),
  deleteReplaceRules: vi.fn(async () => undefined),
  saveReplaceRules: vi.fn(async () => undefined),
}))
vi.mock('../api/rss', () => ({
  getRssSources: vi.fn(async () => []),
  deleteRssSource: vi.fn(async () => undefined),
  saveRssSources: vi.fn(async () => undefined),
}))
vi.mock('../api/source', () => ({
  deleteAllBookSources: vi.fn(async () => undefined),
  getBookSources: vi.fn(async () => []),
  saveBookSources: vi.fn(async () => undefined),
}))
vi.mock('../api/backup', () => ({
  exportLocalBooks: vi.fn(async () => ({
    books: [{ id: 'a'.repeat(32), files: [{ path: 'book.txt', base64: 'dGVzdA==' }] }],
    skipped: [{ id: 'b'.repeat(32), sizeBytes: 123 }],
    totalBytes: 4,
  })),
  exportCustomFonts: vi.fn(async () => ({
    fonts: [{ fileName: `${'c'.repeat(32)}__楷体.ttf`, base64: 'Zm9udA==' }],
    skipped: [],
    totalBytes: 5,
  })),
  exportReadingStats: vi.fn(async () => ({
    daily: [{ date: '2026-08-11', seconds: 90, characters: 12 }],
    byBook: [{ date: '2026-08-11', bookUrl: 'https://example.test/book', bookName: '书', bookAuthor: '', seconds: 90, characters: 12 }],
  })),
  importLocalBooks: vi.fn(async () => ({ imported: 1 })),
  importCustomFonts: vi.fn(async () => ({ imported: 1 })),
  importReadingStats: vi.fn(async () => ({ daily: 1, byBook: 1 })),
}))

function createPayload(): WebdavBackupPayload {
  return {
    version: 2,
    createdAt: '2026-08-11T00:00:00.000Z',
    app: 'reader-rust-frontend',
    bookshelf: {
      books: [
        { name: '网络书', author: '作者', bookUrl: 'https://example.test/book', origin: 'source' },
        { name: '本地书', author: '', bookUrl: 'local-txt:test', origin: 'local-txt' },
      ],
      groups: [{ groupId: 2, groupName: '收藏', orderNo: 3 }],
    },
    bookSources: [],
    rssSources: [],
    bookmarks: [],
    replaceRules: [],
    localState: { theme: 'dark' },
    localBooks: [{ id: 'a'.repeat(32), files: [{ path: 'book.txt', base64: 'dGVzdA==' }] }],
    customFonts: [{ fileName: `${'c'.repeat(32)}__楷体.ttf`, base64: 'Zm9udA==' }],
    readingStats: {
      daily: [{ date: '2026-08-11', seconds: 90, characters: 12 }],
      byBook: [],
    },
  }
}

describe('compatible backup archives', () => {
  it('writes reader data and Legado-compatible common data together', () => {
    const files = createCompatibleBackupArchiveFiles(createPayload())
    const fileMap = Object.fromEntries(files.map((file) => [file.name, file.content]))
    const commonBooks = JSON.parse(fileMap['bookshelf.json'] || '[]') as unknown[]
    const groups = JSON.parse(fileMap['bookGroup.json'] || '[]') as Array<Record<string, unknown>>

    expect(fileMap['reader-rust.json']).toBeTruthy()
    expect(commonBooks).toHaveLength(1)
    expect(groups[0]?.order).toBe(3)
    // 本地书内容与字体嵌入 reader-rust.json
    const embedded = JSON.parse(fileMap['reader-rust.json'])
    expect(embedded.localBooks).toHaveLength(1)
    expect(embedded.customFonts).toHaveLength(1)
  })

  it('restores a Legado archive and skips Android-only local books', () => {
    const result = parseCompatibleBackupArchive({
      'bookshelf.json': JSON.stringify([
        {
          name: '网络书',
          author: '作者',
          bookUrl: 'https://example.test/book',
          origin: 'https://source.test',
          durChapterIndex: 12,
          durChapterPos: 34,
        },
        {
          name: '安卓本地书',
          author: '',
          bookUrl: '/storage/emulated/0/book.txt',
          origin: 'loc_book',
          type: 256,
        },
      ]),
      'bookGroup.json': '[{"groupId":2,"groupName":"收藏","order":4}]',
      'bookSource.json': '[{"bookSourceName":"测试源","bookSourceUrl":"https://source.test"}]',
      'bookmark.json': '[]',
      'rssSources.json': '[]',
      'replaceRule.json': '[]',
    })

    expect(result.format).toBe('legado')
    expect(result.skippedLocalBooks).toBe(1)
    expect(result.payload.bookshelf.books).toHaveLength(1)
    expect(result.payload.bookshelf.books[0]?.durChapterIndex).toBe(12)
    expect(result.payload.bookshelf.groups[0]?.orderNo).toBe(4)
    expect(result.payload.localState).toEqual({})
  })

  it('prefers the embedded reader backup when it is available', () => {
    const payload = createPayload()
    const result = parseCompatibleBackupArchive({
      'reader-rust.json': JSON.stringify(payload),
      'bookshelf.json': '[]',
    })

    expect(result.format).toBe('reader')
    expect(result.payload.bookshelf.books).toHaveLength(2)
    expect(result.payload.localState.theme).toBe('dark')
  })

  it('rejects malformed compatible data files', () => {
    expect(() => parseCompatibleBackupArchive({
      'bookshelf.json': '{}',
    })).toThrow('bookshelf.json 不是数据列表')
  })

  it('accepts v1 backups without local books, fonts or stats', () => {
    const parsed = parseWebdavBackup(JSON.stringify({
      version: 1,
      createdAt: '2026-08-11T00:00:00.000Z',
      app: 'reader-rust-frontend',
      bookshelf: { books: [], groups: [] },
      localState: {},
    }))

    expect(parsed.localBooks).toEqual([])
    expect(parsed.customFonts).toEqual([])
    expect(parsed.readingStats).toEqual({ daily: [], byBook: [] })
  })
})

describe('backup payload v2', () => {
  it('includes local books, fonts and reading stats from the backend exports', async () => {
    vi.stubGlobal('localStorage', {
      getItem: () => null,
      setItem: () => undefined,
      removeItem: () => undefined,
    })
    try {
      const payload = await createWebdavBackupPayload()

      expect(payload.version).toBe(2)
      expect(payload.localBooks).toHaveLength(1)
      expect(payload.skippedLocalBooks).toEqual([{ id: 'b'.repeat(32), sizeBytes: 123 }])
      expect(payload.customFonts).toHaveLength(1)
      expect(payload.readingStats?.daily).toHaveLength(1)
    } finally {
      vi.unstubAllGlobals()
    }
  })

  it('captures the app preference keys', async () => {
    const store: Record<string, string> = {
      'reader-close-to-tray': '1',
      'reader-boss-key': 'ctrl+alt+b',
      'reader-hidden-features': '["rss"]',
    }
    vi.stubGlobal('localStorage', {
      getItem: (key: string) => store[key] ?? null,
      setItem: (key: string, value: string) => { store[key] = value },
      removeItem: (key: string) => { delete store[key] },
    })
    try {
      const payload = await createWebdavBackupPayload()

      expect(payload.localState['reader-close-to-tray']).toBe('1')
      expect(payload.localState['reader-boss-key']).toBe('ctrl+alt+b')
      expect(payload.localState['reader-hidden-features']).toBe('["rss"]')
    } finally {
      vi.unstubAllGlobals()
    }
  })

  it('restores local book files before shelf records and imports fonts and stats', async () => {
    const store: Record<string, string> = {}
    vi.stubGlobal('localStorage', {
      getItem: (key: string) => store[key] ?? null,
      setItem: (key: string, value: string) => { store[key] = value },
      removeItem: (key: string) => { delete store[key] },
    })
    try {
      await restoreWebdavBackup(createPayload())

      expect(importLocalBooks).toHaveBeenCalledWith(createPayload().localBooks)
      expect(importCustomFonts).toHaveBeenCalledWith(createPayload().customFonts)
      expect(importReadingStats).toHaveBeenCalledWith(expect.objectContaining({
        daily: [{ date: '2026-08-11', seconds: 90, characters: 12 }],
      }))
    } finally {
      vi.unstubAllGlobals()
    }
  })
})
