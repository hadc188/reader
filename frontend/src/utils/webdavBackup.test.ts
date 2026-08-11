import { describe, expect, it } from 'vitest'
import type { WebdavBackupPayload } from './webdavBackup'
import {
  createCompatibleBackupArchiveFiles,
  parseCompatibleBackupArchive,
} from './webdavBackup'

function createPayload(): WebdavBackupPayload {
  return {
    version: 1,
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
})
