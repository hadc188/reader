import type { SearchBook } from '../types'

/**
 * Legado-style relevance ranking for multi-source search results.
 *
 * Buckets (highest priority first):
 *   0. exact title OR author match to the search key
 *   1. search key appears in the kind/tag
 *   2. search key is a substring of title or author
 * Within a bucket, books found in more sources rank higher.
 */
export function rankSearchResults(list: SearchBook[], searchKey: string): SearchBook[] {
  const key = searchKey.trim()
  if (!key) return list

  const tier = (book: SearchBook): number => {
    const name = book.name.trim()
    const author = book.author.trim()
    if (name === key || author === key) return 0
    if (book.kind?.includes(key)) return 1
    if (name.includes(key) || author.includes(key)) return 2
    return 3
  }

  return list.filter((book) => isSearchResultRelevant(book, key)).sort((a, b) => {
    const ta = tier(a)
    const tb = tier(b)
    if (ta !== tb) return ta - tb
    const na = a.bookSourceUrls?.length ?? 1
    const nb = b.bookSourceUrls?.length ?? 1
    return nb - na
  })
}

export function isSearchResultRelevant(
  book: Pick<SearchBook, 'name' | 'author' | 'kind'>,
  searchKey: string,
): boolean {
  const key = normalizeSearchText(searchKey).replace(/^作者[:：]?/, '')
  if (!key) return true

  return [book.name, book.author.replace(/^\s*作者\s*[:：]?\s*/, ''), book.kind || '']
    .some((value) => normalizeSearchText(value).includes(key))
}

/** Normalized merge key for deduping search results across sources. */
export function searchMergeKey(book: Pick<SearchBook, 'name' | 'author'>): string {
  return `${normalizeSearchName(book.name)}|${normalizeSearchAuthor(book.author)}`
}

/** Strip whitespace so "遮 天" and "遮天" merge. */
function normalizeSearchName(value: string): string {
  return value.trim().toLowerCase().replace(/\s+/g, '')
}

/** Strip whitespace and the "作者：" label prefix so "作者：辰东" and "辰东" merge. */
function normalizeSearchAuthor(value: string): string {
  const compact = value.trim().toLowerCase().replace(/\s+/g, '')
  return compact
    .replace(/^作者[:：]/, '')
    .replace(/^作者/, '')
    .replace(/^[:：]/, '')
}

function normalizeSearchText(value: string): string {
  return value.trim().toLocaleLowerCase().replace(/\s+/g, '')
}
