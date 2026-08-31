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

/** Keep the complete source hit when merging rows, so the detail modal does
 * not have to rediscover a source that the initial search already found. */
export function initializeSearchResult(book: SearchBook): SearchBook {
  const candidate = searchSourceCandidate(book)
  return {
    ...book,
    bookSourceUrls: book.origin ? [book.origin] : [],
    sourceCandidates: candidate.origin && candidate.bookUrl ? [candidate] : [],
  }
}

export function mergeSearchResult(existing: SearchBook, incoming: SearchBook): SearchBook {
  const urls = new Set(existing.bookSourceUrls ?? (existing.origin ? [existing.origin] : []))
  if (incoming.origin) urls.add(incoming.origin)

  const candidates = [...(existing.sourceCandidates ?? [searchSourceCandidate(existing)])]
  for (const candidate of incoming.sourceCandidates ?? [searchSourceCandidate(incoming)]) {
    if (!candidate.origin || !candidate.bookUrl) continue
    const sameSourceIndex = candidates.findIndex((item) => item.origin === candidate.origin)
    if (sameSourceIndex === -1) {
      candidates.push(candidate)
    } else if (!candidates[sameSourceIndex].bookUrl) {
      candidates[sameSourceIndex] = candidate
    }
  }

  return {
    ...existing,
    bookSourceUrls: Array.from(urls),
    sourceCandidates: candidates,
    coverUrl: existing.coverUrl || incoming.coverUrl,
    intro: existing.intro || incoming.intro,
    kind: existing.kind || incoming.kind,
    lastChapter: existing.lastChapter || incoming.lastChapter,
    updateTime: existing.updateTime || incoming.updateTime,
    wordCount: existing.wordCount || incoming.wordCount,
  }
}

function searchSourceCandidate(book: SearchBook): SearchBook {
  const { bookSourceUrls: _urls, sourceCandidates: _candidates, ...candidate } = book
  return candidate
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

/** Normalize source-switch authors with the same punctuation rules as comparison badges. */
export function normalizeSourceSwitchAuthor(value?: string): string {
  return normalizeSearchAuthor((value || '').replace(/\s+/g, ''))
}

/** Keep a candidate when its author is missing or matches the current book. */
export function matchesSourceSwitchAuthor(
  currentAuthor: string | undefined,
  candidateAuthor: string | undefined,
): boolean {
  const current = normalizeSourceSwitchAuthor(currentAuthor)
  const candidate = normalizeSourceSwitchAuthor(candidateAuthor)
  return !current || !candidate || current === candidate
}

function normalizeSearchText(value: string): string {
  return value.trim().toLocaleLowerCase().replace(/\s+/g, '')
}
