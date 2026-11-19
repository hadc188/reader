import { openSse } from './sse'
import { get, post } from './invoke'
import type { CacheBookOverview, CacheManifest } from '../types'

/**
 * SSE-based book caching. Returns an SseLike stream.
 */
export function cacheBookSSE(params: {
  bookUrl: string
  tocUrl?: string
  count?: number
  refresh?: number
  concurrentCount?: number
  /** range 从阅读进度起补 count 章，missing 补全书缺失章节，failed 只重试失败章节，selected 缓存指定章节。 */
  mode?: 'range' | 'missing' | 'failed' | 'selected'
  /** selected 模式要缓存的章节地址。 */
  chapterUrls?: string[]
  /** 已缓存章节也重新抓取。 */
  overwrite?: boolean
}) {
  return openSse('cache_book_sse', {
    bookUrl: params.bookUrl,
    ...(params.tocUrl ? { tocUrl: params.tocUrl } : {}),
    ...(params.count !== undefined ? { count: params.count } : {}),
    ...(params.refresh ? { refresh: params.refresh } : {}),
    ...(params.concurrentCount ? { concurrentCount: params.concurrentCount } : {}),
    ...(params.mode && params.mode !== 'range' ? { mode: params.mode } : {}),
    ...(params.chapterUrls?.length ? { chapterUrls: params.chapterUrls } : {}),
    ...(params.overwrite ? { overwrite: params.overwrite } : {}),
  })
}

/** 中断指定书籍正在进行的整书缓存任务。 */
export function cancelCacheBook(bookUrl: string) {
  return post<{ cancelled: boolean }>('/cancelCacheBook', { bookUrl }).then((r) => r.data)
}

/** 缓存管理列表。每本书带总章数、已缓存章数与失败章节。 */
export function getCacheOverview() {
  return get<CacheBookOverview[]>('/getCacheOverview').then((r) => r.data)
}

/** 读取单本书的缓存清单。 */
export function getCacheManifest(bookUrl: string) {
  return post<CacheManifest | null>('/getCacheManifest', { bookUrl }).then((r) => r.data)
}