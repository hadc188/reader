import { getChapterList, getBookContent } from '../api/bookshelf'
import type { Book, BookChapter } from '../types'
import { setBrowserCachedChapter } from './browserCache'

export interface CacheProgress {
  total: number
  completed: number
  chapterTitle: string
}

export async function resolveBookChapters(book: Book) {
  return getChapterList({
    bookUrl: book.bookUrl,
    bookSourceUrl: book.origin,
  })
}

export async function cacheBookToBrowser(params: {
  book: Book
  chapters?: BookChapter[]
  startIndex?: number
  count?: number
  onProgress?: (progress: CacheProgress) => void
  signal?: { cancelled: boolean }
}) {
  const chapters = params.chapters || await resolveBookChapters(params.book)
  const startIndex = Math.max(0, params.startIndex || 0)
  const sliced = chapters.slice(startIndex, params.count ? startIndex + params.count : undefined)
  let completed = 0

  for (const chapter of sliced) {
    if (params.signal?.cancelled) {
      break
    }
    const content = await getBookContent({
      chapterUrl: chapter.url,
      bookSourceUrl: params.book.origin,
    })
    await setBrowserCachedChapter({
      bookUrl: params.book.bookUrl,
      chapterUrl: chapter.url,
      chapterTitle: chapter.title,
      content,
    })
    completed += 1
    params.onProgress?.({
      total: sliced.length,
      completed,
      chapterTitle: chapter.title,
    })
  }
  return {
    total: sliced.length,
    completed,
  }
}
