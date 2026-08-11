import { get, post, invokeEnvelope } from './invoke'
import type { Book, BookChapter, BookGroup } from '../types'

export function getBookshelf() {
  return get<Book[]>('/getBookshelf').then((r) => r.data)
}

export function getBookshelfWithCacheInfo() {
  return get<Book[]>('/getShelfBookWithCacheInfo').then((r) => r.data)
}

export function saveBook(book: Partial<Book>) {
  return post<Book>('/saveBook', book).then((r) => r.data)
}

export function saveBooks(books: Partial<Book>[]) {
  return post<Book[]>('/saveBooks', books).then((r) => r.data)
}

export async function uploadTxtBook(file: File) {
  return invokeEnvelope<Book>('upload_txt_book', {
    fileName: file.name,
    file: new Uint8Array(await file.arrayBuffer()),
  })
}

export async function uploadEpubBook(file: File) {
  return invokeEnvelope<Book>('upload_epub_book', {
    fileName: file.name,
    file: new Uint8Array(await file.arrayBuffer()),
  })
}

export function deleteBook(book: Partial<Book>) {
  return post<string>('/deleteBook', book).then((r) => r.data)
}

export function deleteBooks(books: Partial<Book>[]) {
  return post<{ deleted: number }>('/deleteBooks', books).then((r) => r.data)
}

export function getBookInfo(url: string, origin?: string) {
  return post<Book>('/getBookInfo', { url, bookSourceUrl: origin }).then((r) => r.data)
}

export function getChapterList(params: {
  bookUrl?: string
  tocUrl?: string
  bookSourceUrl?: string
  refresh?: number
}) {
  return post<BookChapter[]>('/getChapterList', params).then((r) => r.data)
}

export function getBookContent(params: {
  chapterUrl?: string
  bookSourceUrl?: string
  index?: number
  refresh?: number
}) {
  return post<string>('/getBookContent', params).then((r) => r.data)
}

export function saveBookProgress(params: {
  bookUrl: string
  index: number
  position?: number
}) {
  return post<string>('/saveBookProgress', params).then((r) => r.data)
}

export function deleteBookCache(bookUrl: string) {
  return post('/deleteBookCache', { bookUrl }).then((r) => r.data)
}

// ─── Groups ───
export function getBookGroups() {
  return get<BookGroup[]>('/getBookGroups').then((r) => r.data)
}

export function saveBookGroup(group: BookGroup) {
  return post<string>('/saveBookGroup', group).then((r) => r.data)
}

export function deleteBookGroup(groupId: number) {
  return post<string>('/deleteBookGroup', { groupId }).then((r) => r.data)
}

export function saveBookGroupId(bookUrl: string, groupId: number) {
  return post<string>('/saveBookGroupId', { bookUrl, groupId }).then((r) => r.data)
}

export function setBookSource(params: {
  bookUrl: string
  newUrl: string
  bookSourceUrl: string
  name?: string
  author?: string
  coverUrl?: string
  intro?: string
  kind?: string
  latestChapterTitle?: string
  durChapterIndex?: number
  durChapterTitle?: string
  durChapterPos?: number
  durChapterTime?: number
}) {
  return post<Book>('/setBookSource', params).then((r) => r.data)
}

// ─── Cover helper ───
export function getCoverUrl(coverUrl?: string) {
  if (!coverUrl) return ''
  if (coverUrl.startsWith('/reader3/localEpubAsset')) {
    const [, rawQuery = ''] = coverUrl.split('?')
    const params = new URLSearchParams(rawQuery)
    const bookUrl = params.get('bookUrl') ?? ''
    const path = params.get('path') ?? ''
    return `http://reader.localhost/epub?bookUrl=${encodeURIComponent(bookUrl)}&path=${encodeURIComponent(path)}`
  }
  if (coverUrl.startsWith('http') || coverUrl.startsWith('/')) {
    return `http://reader.localhost/cover?path=${encodeURIComponent(coverUrl)}`
  }
  return coverUrl
}
