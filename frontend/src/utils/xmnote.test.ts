import { describe, expect, it } from 'vitest'
import type { Book, Bookmark } from '../types'
import {
  buildXmnoteBookPayload,
  groupXmnoteBookDrafts,
  normalizeXmnoteEndpoint,
} from './xmnote'

const book: Book = {
  name: '山茶文具店',
  author: '小川糸',
  bookUrl: 'book-url',
  origin: 'source-url',
  intro: '内容简介',
  coverUrl: 'https://example.com/cover.jpg',
  durChapterIndex: 9,
  durChapterPos: 5000,
  durChapterTime: 1711886400000,
  totalChapterNum: 20,
}

const bookmarks: Bookmark[] = [
  {
    time: 1711886400000,
    bookName: book.name,
    bookAuthor: book.author,
    chapterName: '第一章',
    bookText: '原文',
    content: '想法',
  },
  {
    time: 1711886500000,
    bookName: book.name,
    bookAuthor: book.author,
    chapterName: '第二章',
    bookText: '',
    content: '',
  },
]

describe('xmnote', () => {
  it('补全基础地址的 /send 路径并移除 hash', () => {
    expect(normalizeXmnoteEndpoint('192.168.1.8:8080/#/import')).toBe(
      'http://192.168.1.8:8080/send',
    )
    expect(normalizeXmnoteEndpoint('http://xmnote.local:8080/send')).toBe(
      'http://xmnote.local:8080/send',
    )
  })

  it('拒绝非 HTTP 地址', () => {
    expect(() => normalizeXmnoteEndpoint('ftp://xmnote.local/send')).toThrow(
      '只支持 HTTP 或 HTTPS',
    )
  })

  it('把章节内进度换算成纸间书摘的百分比位置', () => {
    const payload = buildXmnoteBookPayload(book, bookmarks)
    expect(payload).toMatchObject({
      title: '山茶文具店',
      author: '小川糸',
      bookSummary: '内容简介',
      cover: 'https://example.com/cover.jpg',
      type: 1,
      locationUnit: 0,
      totalPageCount: 20,
      currentPage: 47.5,
      readingStatus: 2,
      readingStatusChangedDate: 1711886400,
    })
  })

  it('书签映射为摘录，空摘录不发送', () => {
    expect(buildXmnoteBookPayload(book, bookmarks).entries).toEqual([
      {
        text: '原文',
        note: '想法',
        chapter: '第一章',
        time: 1711886400,
      },
    ])
  })

  it('可以只发送进度或只发送摘录', () => {
    const progressOnly = buildXmnoteBookPayload(book, bookmarks, [], { includeEntries: false })
    expect(progressOnly.entries).toBeUndefined()
    expect(progressOnly.currentPage).toBe(47.5)

    const entriesOnly = buildXmnoteBookPayload(book, bookmarks, [], { includeProgress: false })
    expect(entriesOnly.currentPage).toBeUndefined()
    expect(entriesOnly.entries).toHaveLength(1)
  })

  it('缺少 totalChapterNum 时用完整目录长度计算进度，避免误报 100%', () => {
    const incomplete = {
      ...book,
      totalChapterNum: undefined,
      durChapterIndex: 0,
      durChapterPos: 0,
    }
    const payload = buildXmnoteBookPayload(
      incomplete,
      [],
      Array.from({ length: 10 }, (_, index) => `第 ${index + 1} 章`),
    )
    expect(payload.currentPage).toBe(0)
    expect(payload.readingStatus).toBe(2)
  })

  it('按书名和作者把书签关联到书架书籍', () => {
    const drafts = groupXmnoteBookDrafts([book], bookmarks)
    expect(drafts).toHaveLength(1)
    expect(drafts[0].bookmarks).toEqual(bookmarks)
  })
})
