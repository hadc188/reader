import { post } from './invoke'
import { openSse } from './sse'
import type { SearchBook } from '../types'

export function searchBookMulti(params: {
  key: string
  page?: number
  bookSourceGroup?: string
  bookSourceUrl?: string
}) {
  return post<SearchBook[]>('/searchBookMulti', params).then((r) => r.data)
}

/**
 * SSE-based multi-source search. Returns an SseLike stream.
 * Caller is responsible for closing the connection.
 */
export function searchBookMultiSSE(params: {
  key: string
  bookSourceGroup?: string
  bookSourceUrl?: string
  concurrentCount?: number
  searchSize?: number
}) {
  return openSse('search_book_multi_sse', {
    key: params.key,
    ...(params.bookSourceGroup ? { bookSourceGroup: params.bookSourceGroup } : {}),
    ...(params.bookSourceUrl ? { bookSourceUrl: params.bookSourceUrl } : {}),
    ...(params.concurrentCount ? { concurrentCount: params.concurrentCount } : {}),
    ...(params.searchSize ? { searchSize: params.searchSize } : {}),
  })
}

export function getAvailableBookSource(params: {
  url?: string
  name?: string
  author?: string
  origin?: string
  refresh?: number
  lastIndex?: number
  resultLimit?: number
  concurrentCount?: number
}) {
  return post<SearchBook[] | AvailableBookSourceResult>('/getAvailableBookSource', params).then(
    (r) => normalizeAvailableBookSourceResult(r.data),
  )
}

export function getAvailableBookSourceSSE(params: {
  url?: string
  name?: string
  author?: string
  origin?: string
  refresh?: number
  lastIndex?: number
  resultLimit?: number
  concurrentCount?: number
}) {
  return openSse('get_available_book_source_sse', {
    ...(params.url ? { url: params.url } : {}),
    ...(params.name ? { name: params.name } : {}),
    ...(params.author ? { author: params.author } : {}),
    ...(params.origin ? { origin: params.origin } : {}),
    ...(typeof params.refresh !== 'undefined' ? { refresh: params.refresh } : {}),
    lastIndex: params.lastIndex ?? -1,
    ...(typeof params.resultLimit !== 'undefined' ? { resultLimit: params.resultLimit } : {}),
    concurrentCount: params.concurrentCount ?? 8,
  })
}

export interface AvailableBookSourceResult {
  books: SearchBook[]
  lastIndex: number
  hasMore: boolean
}

function normalizeAvailableBookSourceResult(
  data: SearchBook[] | AvailableBookSourceResult,
): AvailableBookSourceResult {
  if (Array.isArray(data)) {
    return {
      books: data,
      lastIndex: data.length - 1,
      hasMore: false,
    }
  }
  return data
}

export function searchBookSourceSSE(params: {
  url: string
  bookSourceGroup?: string
  lastIndex?: number
  concurrentCount?: number
  searchSize?: number
  refresh?: number
}) {
  return openSse('search_book_source_sse', {
    url: params.url,
    concurrentCount: params.concurrentCount ?? 24,
    lastIndex: params.lastIndex ?? -1,
    ...(typeof params.refresh !== 'undefined' ? { refresh: params.refresh } : {}),
    ...(params.bookSourceGroup !== undefined ? { bookSourceGroup: params.bookSourceGroup } : {}),
    ...(params.searchSize ? { searchSize: params.searchSize } : {}),
  })
}
