import { describe, expect, it } from 'vitest'
import { canAddCurrentBookToShelf, resolveShelfMembership } from './readerShelf'

const book = {
  name: '遮天',
  author: '辰东',
  bookUrl: 'https://a.com/book/1',
  origin: 'https://a.com',
}

function shelfOf(
  books: Array<{ bookUrl: string; name?: string; author?: string; origin?: string }>,
  overrides: { loaded?: boolean; loading?: boolean } = {},
) {
  return { books, loaded: true, ...overrides }
}

describe('resolveShelfMembership', () => {
  it('refuses to judge before the shelf has loaded', () => {
    // 关键回归：书架还没加载完就说"未上架"，会对一本已在书架的书显示
    // 「加入书架」，点下去是覆盖写。
    expect(resolveShelfMembership(book, { books: [], loaded: false })).toBe('unknown')
  })

  it('also stays unknown while the shelf is still loading', () => {
    expect(resolveShelfMembership(book, shelfOf([], { loading: true }))).toBe('unknown')
  })

  it('reports off-shelf when the loaded shelf does not contain the book', () => {
    expect(resolveShelfMembership(book, shelfOf([]))).toBe('off-shelf')
    expect(resolveShelfMembership(book, shelfOf([
      { bookUrl: 'https://a.com/book/2', name: '夜无疆', author: '辰东' },
    ]))).toBe('off-shelf')
  })

  it('matches by bookUrl', () => {
    expect(resolveShelfMembership(book, shelfOf([{ bookUrl: book.bookUrl }]))).toBe('on-shelf')
  })

  it('matches by name + author when the url differs (switched source)', () => {
    // 换过源的书 bookUrl 会变，但书名+作者没变，仍应认作同一本。
    expect(resolveShelfMembership(book, shelfOf([
      { bookUrl: 'https://b.com/other', name: '遮天', author: '辰东' },
    ]))).toBe('on-shelf')
  })

  it('does not match a different book with the same name by another author', () => {
    expect(resolveShelfMembership(book, shelfOf([
      { bookUrl: 'https://c.com/x', name: '遮天', author: '别人' },
    ]))).toBe('off-shelf')
  })

  it('does not treat a local book as the same book as a web copy', () => {
    // 回归：书架里有本地导入的《遮天》，搜索页里的网络版《遮天》不该显示
    // 「已在书架」—— 两者内容完全不同，用户需要能把网络版加进来。
    expect(resolveShelfMembership(book, shelfOf([
      { bookUrl: 'local-txt:abc123', name: '遮天', author: '辰东', origin: 'local-txt' },
    ]))).toBe('off-shelf')
  })

  it('does not treat a web book as the same book as a local copy either', () => {
    const localBook = { ...book, bookUrl: 'local-epub:xyz', origin: 'local-epub' }
    expect(resolveShelfMembership(localBook, shelfOf([
      { bookUrl: 'https://a.com/book/1', name: '遮天', author: '辰东', origin: 'https://a.com' },
    ]))).toBe('off-shelf')
  })

  it('still matches two local books with the same name and author', () => {
    // 本地书之间仍然要能认出来，否则同一本 TXT 重复导入两份都像"没加过"。
    const localBook = { ...book, bookUrl: 'local-txt:abc', origin: 'local-txt' }
    expect(resolveShelfMembership(localBook, shelfOf([
      { bookUrl: 'local-txt:def', name: '遮天', author: '辰东', origin: 'local-txt' },
    ]))).toBe('on-shelf')
  })

  it('returns unknown without a book', () => {
    expect(resolveShelfMembership(null, shelfOf([]))).toBe('unknown')
    expect(resolveShelfMembership({ ...book, bookUrl: '' }, shelfOf([]))).toBe('unknown')
  })
})

describe('canAddCurrentBookToShelf', () => {
  it('only offers the action when we are sure it is off-shelf', () => {
    expect(canAddCurrentBookToShelf('off-shelf')).toBe(true)
    expect(canAddCurrentBookToShelf('on-shelf')).toBe(false)
    expect(canAddCurrentBookToShelf('unknown')).toBe(false)
  })

  it('hides during the write to avoid a double add', () => {
    expect(canAddCurrentBookToShelf('off-shelf', { adding: true })).toBe(false)
  })

  it('hides in Web mode where saveBook has no route', () => {
    expect(canAddCurrentBookToShelf('off-shelf', { readonly: true })).toBe(false)
  })
})
