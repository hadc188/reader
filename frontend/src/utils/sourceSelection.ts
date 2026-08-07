import type { BookSource, RssSource } from '../types'

export function getVisibleSelection<T>(
  visibleItems: T[],
  selectedKeys: ReadonlySet<string>,
  keyOf: (item: T) => string,
) {
  return visibleItems.filter((item) => selectedKeys.has(keyOf(item)))
}

export function toBookSourceDeletePayload(sources: Pick<BookSource, 'bookSourceUrl'>[]) {
  return sources.map((source) => ({ bookSourceUrl: source.bookSourceUrl }))
}

export function toRssSourceDeletePayload(sources: Pick<RssSource, 'sourceName' | 'sourceUrl'>[]) {
  return sources.map((source) => ({
    sourceName: source.sourceName,
    sourceUrl: source.sourceUrl,
  }))
}

export function getBookSourceGroups(sources: Pick<BookSource, 'bookSourceGroup'>[]) {
  const groups = new Set<string>()
  sources.forEach((source) => {
    source.bookSourceGroup?.split(/[,;；、]/).forEach((group) => {
      const trimmed = group.trim()
      if (trimmed) groups.add(trimmed)
    })
  })
  return Array.from(groups).sort()
}

export function filterBookSources(
  sources: BookSource[],
  filterText: string,
  filterGroup: string,
) {
  const keyword = filterText.trim().toLowerCase()
  return sources.filter((source) => {
    const matchesText = !keyword
      || source.bookSourceName.toLowerCase().includes(keyword)
      || source.bookSourceUrl.toLowerCase().includes(keyword)
    const matchesGroup = !filterGroup || source.bookSourceGroup?.includes(filterGroup)
    return matchesText && matchesGroup
  })
}

export function getBookSourceStats(allSources: BookSource[], filteredSources: BookSource[]) {
  return {
    total: allSources.length,
    enabled: allSources.filter((source) => source.enabled !== false).length,
    filtered: filteredSources.length,
  }
}

export function getBookSourceOverview(source: BookSource | null) {
  if (!source) {
    return null
  }
  return {
    group: source.bookSourceGroup?.trim() || '未分组',
    statusText: source.enabled === false ? '停用' : '启用',
    exploreText: source.enabledExplore === false ? '发现停用' : '发现可用',
    cookieText: source.enabledCookieJar ? 'Cookie 独立' : '默认 Cookie',
    hasSearch: Boolean(source.searchUrl || source.ruleSearch),
    hasExplore: Boolean(source.exploreUrl || source.ruleExplore),
    hasBookInfo: Boolean(source.ruleBookInfo),
    hasToc: Boolean(source.ruleToc),
    hasContent: Boolean(source.ruleContent),
    hasLogin: Boolean(source.loginUrl?.trim()),
  }
}
