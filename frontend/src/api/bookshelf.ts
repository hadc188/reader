import { invoke, Channel } from '@tauri-apps/api/core'
import { get, post, invokeEnvelope } from './invoke'
import { readerOrigin } from './scheme'
import type { ApiResponse, Book, BookChapter, BookGroup } from '../types'

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

export async function uploadPdfBook(file: File) {
  return invokeEnvelope<Book>('upload_pdf_book', {
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

/** 章节加载阶段（与后端 `ContentLoadStage` 一致）。 */
export type ContentLoadStage = 'fetching' | 'parsing' | 'caching'

export function getBookContent(params: {
  bookUrl?: string
  chapterUrl?: string
  bookSourceUrl?: string
  index?: number
  refresh?: number
  /** 加载阶段回调：让「书源脚本 / 段评解析」这类长耗时可见 */
  onStage?: (stage: ContentLoadStage) => void
}) {
  const { onStage, ...req } = params
  // 后端命令的 Channel 参数是必需的（Tauri 不支持 Option<Channel>），
  // 所以不关心进度时也要传一个通道，忽略其消息即可。
  const channel = new Channel<{ stage?: ContentLoadStage }>()
  if (onStage) {
    channel.onmessage = (payload) => {
      if (payload?.stage) onStage(payload.stage)
    }
  }
  return invoke<ApiResponse<string>>('get_book_content', { req, onStage: channel }).then((res) => {
    if (!res.isSuccess) throw new Error(res.errorMsg || '加载章节失败')
    return res.data as string
  })
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

export function getCachedChapterUrls(bookUrl: string, chapterUrls: string[]) {
  return post<string[]>('/getCachedChapterUrls', { bookUrl, chapterUrls }).then((r) => r.data)
}

// ─── Groups ───
export function getBookGroups() {
  return get<BookGroup[]>('/getBookGroups').then((r) => r.data)
}

export function saveBookGroup(group: BookGroup) {
  return post<string>('/saveBookGroup', group).then((r) => r.data)
}

/** 整表覆盖分组列表。
 *
 *  与 `saveBookGroup` 的区别很重要: 后者是 upsert —— `groupId` 为 0 时后端会
 *  分配一个新 id 再 push, 逐个写入既可能凭空多出分组, 也无法表达「以这份列表
 *  为准」。恢复备份时必须用这个, 才能保证分组 id 与备份里一致
 *  (书架的 `group` 位域引用的是这些 id)。 */
export function saveBookGroupOrder(groups: BookGroup[]) {
  return post<string>('/saveBookGroupOrder', groups).then((r) => r.data)
}

export function deleteBookGroup(groupId: number) {
  return post<string>('/deleteBookGroup', { groupId }).then((r) => r.data)
}

/** 批量删除分组。一次 IPC 完成, 避免逐个删除时的 N 次全量重写。 */
export function deleteBookGroups(groupIds: number[]) {
  return post<{ removed: number }>('/deleteBookGroups', { groupIds }).then((r) => r.data)
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
    return `${readerOrigin}/epub?bookUrl=${encodeURIComponent(bookUrl)}&path=${encodeURIComponent(path)}`
  }
  // 本应用自己存的文件(自定义封面)已经是可直接访问的 reader 协议地址,
  // 必须原样返回 —— 下面那条分支会把它当成远端地址再包一层 /cover,
  // 结果必然 404(封面显示不出来)。
  if (coverUrl.startsWith(`${readerOrigin}/`)) {
    return coverUrl
  }
  if (coverUrl.startsWith('http') || coverUrl.startsWith('/')) {
    return `${readerOrigin}/cover?path=${encodeURIComponent(coverUrl)}`
  }
  return coverUrl
}
