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
  const candidates = mergeSourceCandidates([book], book.sourceCandidates ?? [])
  return {
    ...book,
    bookSourceUrls: Array.from(new Set([
      ...(book.bookSourceUrls ?? []),
      book.origin,
      ...candidates.map((candidate) => candidate.origin),
    ].filter(Boolean))),
    sourceCandidates: candidates,
  }
}

export function mergeSearchResult(existing: SearchBook, incoming: SearchBook): SearchBook {
  const candidates = mergeSourceCandidates(
    [existing, ...(existing.sourceCandidates ?? [])],
    [incoming, ...(incoming.sourceCandidates ?? [])],
  )
  const urls = new Set([
    ...(existing.bookSourceUrls ?? []),
    ...(incoming.bookSourceUrls ?? []),
    existing.origin,
    incoming.origin,
    ...candidates.map((candidate) => candidate.origin),
  ].filter(Boolean))

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

/** A source can return several editions or aggregate sites for the same book. */
export function sourceCandidateKey(candidate: Pick<SearchBook, 'origin' | 'bookUrl'>): string {
  return JSON.stringify([candidate.origin.trim().replace(/\/+$/, ''), candidate.bookUrl])
}

/** Keep every distinct link, updating repeated hits with their latest metadata. */
export function mergeSourceCandidates(
  current: readonly SearchBook[],
  incoming: readonly SearchBook[] = [],
): SearchBook[] {
  const candidates = new Map<string, SearchBook>()
  for (const book of [...current, ...incoming]) {
    if (!book.origin.trim() || !book.bookUrl.trim()) continue
    const candidate = searchSourceCandidate(book)
    const key = sourceCandidateKey(candidate)
    const previous = candidates.get(key)
    candidates.set(key, previous ? {
      ...previous,
      ...candidate,
      name: candidate.name || previous.name,
      author: candidate.author || previous.author,
      originName: candidate.originName || previous.originName,
      originGroup: candidate.originGroup || previous.originGroup,
      coverUrl: candidate.coverUrl || previous.coverUrl,
      intro: candidate.intro || previous.intro,
      kind: candidate.kind || previous.kind,
      lastChapter: candidate.lastChapter || previous.lastChapter,
      updateTime: candidate.updateTime || previous.updateTime,
      wordCount: candidate.wordCount || previous.wordCount,
    } : candidate)
  }
  return Array.from(candidates.values())
}

function searchSourceCandidate(book: SearchBook): SearchBook {
  const { bookSourceUrls: _urls, sourceCandidates: _candidates, ...candidate } = book
  return candidate
}

/** Normalized merge key for deduping search results across sources. */
export function searchMergeKey(book: Pick<SearchBook, 'name' | 'author'>): string {
  return `${normalizeSearchName(book.name)}|${normalizeSearchAuthor(book.author)}`
}

/**
 * 「书架里是否已有这本书」用的键：在 `searchMergeKey` 前再按**书源类型**分档。
 *
 * 为什么需要多这一档：本地导入的《遮天》(TXT/EPUB/PDF) 和网络书源里的《遮天》
 * 是同名同作者的两份不同内容，`searchMergeKey` 会把它们并成一本，于是搜索结果
 * 里的网络版被标成「已在书架」、也无法加入。
 *
 * 只用 `origin` 判定本地/网络：`local-txt` / `local-epub` / `local-pdf` 是后端
 * 固定前缀（见 utils/localBook）。网络书源之间仍然互相认作同一本（换源场景）。
 */
export function shelfMatchKey(
  book: { name?: string; author?: string; origin?: string },
): string {
  return `${shelfKind(book.origin)}|${searchMergeKey({ name: book.name || '', author: book.author || '' })}`
}

/** 与 localBook 的三种本地源对齐；其余一律算网络书。 */
function shelfKind(origin?: string): 'local' | 'web' {
  const value = (origin || '').trim().toLowerCase()
  return value === 'local-txt' || value === 'local-epub' || value === 'local-pdf'
    ? 'local'
    : 'web'
}

/** Strip whitespace so "遮 天" and "遮天" merge. */
function normalizeSearchName(value: string): string {
  return value.trim().toLowerCase().replace(/\s+/g, '')
}

/** Strip whitespace and the "作者：" label prefix so "作者：另一作者" and "另一作者" merge. */
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

/** Legado's default source switch matches the complete, whitespace-normalized title. */
export function matchesSourceSwitchTitle(currentTitle: string, candidateTitle: string): boolean {
  const current = currentTitle.replace(/\s+/g, '')
  return current.length > 0 && current === candidateTitle.replace(/\s+/g, '')
}

function normalizeSearchText(value: string): string {
  return value.trim().toLocaleLowerCase().replace(/\s+/g, '')
}
