import { post } from './invoke'
import { openSse } from './sse'

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

/**
 * Delete all content cache for a book
 */
export function deleteBookCache(bookUrl: string) {
  return post('/deleteBookCache', { bookUrl }).then((r) => r.data)
}
