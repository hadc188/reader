import type { Book, SearchBook } from '../types'

type BookLike = Pick<Book | SearchBook, 'origin' | 'bookUrl'>

export function isLocalTxtBook(book?: BookLike | null) {
  if (!book) return false
  return book.origin?.trim() === 'local-txt' || book.bookUrl?.trim().startsWith('local-txt:')
}

export function isLocalEpubBook(book?: BookLike | null) {
  if (!book) return false
  return book.origin?.trim() === 'local-epub' || book.bookUrl?.trim().startsWith('local-epub:')
}

export function isLocalBook(book?: BookLike | null) {
  return isLocalTxtBook(book) || isLocalEpubBook(book)
}
