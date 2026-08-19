import { openSse } from './sse'
import { post } from './invoke'

/**
 * SSE-based book caching. Returns an SseLike stream.
 */
export function cacheBookSSE(params: {
  bookUrl: string
  tocUrl?: string
  count?: number
  refresh?: number
  concurrentCount?: number
}) {
  return openSse('cache_book_sse', {
    bookUrl: params.bookUrl,
    ...(params.tocUrl ? { tocUrl: params.tocUrl } : {}),
    ...(params.count ? { count: params.count } : {}),
    ...(params.refresh ? { refresh: params.refresh } : {}),
    ...(params.concurrentCount ? { concurrentCount: params.concurrentCount } : {}),
  })
}

export interface CacheBookProgressPayload {
  totalChapters?: number
  cachedCount?: number
  successCount?: number
  failedCount?: number
  aborted?: boolean
}

/** 中断指定书籍正在进行的整书缓存任务。 */
export function cancelCacheBook(bookUrl: string) {
  return post<{ cancelled: boolean }>('/cancelCacheBook', { bookUrl }).then((r) => r.data)
}
