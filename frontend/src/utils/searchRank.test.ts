import { describe, expect, it } from 'vitest'
import {
  initializeSearchResult,
  isSearchResultRelevant,
  mergeSearchResult,
  rankSearchResults,
  searchMergeKey,
  matchesSourceSwitchAuthor,
} from './searchRank'
import type { SearchBook } from '../types'

function book(partial: Partial<SearchBook>): SearchBook {
  return {
    name: '',
    author: '',
    bookUrl: '',
    origin: '',
    ...partial,
  }
}

describe('rankSearchResults', () => {
  it('puts exact title match first, then substring, and removes unrelated results', () => {
    const exact = book({ name: '遮天', author: '另一作者', bookSourceUrls: ['a'] })
    const contains = book({ name: '遮天之龙起微末', author: '某', bookSourceUrls: ['b'] })
    const other = book({ name: '凡人修仙传', author: '忘语', bookSourceUrls: ['c'] })
    const ranked = rankSearchResults([other, contains, exact], '遮天')
    expect(ranked[0].name).toBe('遮天')
    expect(ranked[1].name).toBe('遮天之龙起微末')
    expect(ranked).toHaveLength(2)
  })

  it('ranks exact author match as tier 0', () => {
    const byAuthor = book({ name: '某书', author: '另一作者', bookSourceUrls: ['a'] })
    const bySubstring = book({ name: '遮天纪', author: '乙', bookSourceUrls: ['b'] })
    const ranked = rankSearchResults([bySubstring, byAuthor], '另一作者')
    expect(ranked[0].author).toBe('另一作者')
  })

  it('sorts within a tier by source count descending', () => {
    const one = book({ name: '遮天', author: '另一作者', bookSourceUrls: ['a'] })
    const three = book({ name: '遮天', author: '另一作者', bookSourceUrls: ['a', 'b', 'c'] })
    const ranked = rankSearchResults([one, three], '遮天')
    expect(ranked[0].bookSourceUrls?.length).toBe(3)
  })

  it('returns list unchanged when key is empty', () => {
    const list = [book({ name: 'b' }), book({ name: 'a' })]
    expect(rankSearchResults(list, '  ')).toBe(list)
  })

  it('keeps matches in author and kind while rejecting unrelated books', () => {
    expect(isSearchResultRelevant(book({ name: '某书', author: '作者：测试作者' }), '测 试作者')).toBe(true)
    expect(isSearchResultRelevant(book({ name: '某书', kind: '都市 / 重生' }), '重生')).toBe(true)
    expect(isSearchResultRelevant(book({ name: '凡人修仙传', author: '忘语' }), '遮天')).toBe(false)
  })
})

describe('searchMergeKey', () => {
  it('normalizes case and trims', () => {
    expect(searchMergeKey({ name: ' 遮天 ', author: '另一作者' })).toBe('遮天|另一作者')
    expect(searchMergeKey({ name: 'ABC', author: 'X' })).toBe('abc|x')
  })

  it('strips 作者 label prefix and whitespace from author', () => {
    expect(searchMergeKey({ name: '遮天', author: '作者：测试作者' })).toBe('遮天|测试作者')
    expect(searchMergeKey({ name: '遮天', author: '作者: 测 试 作 者' })).toBe('遮天|测试作者')
    expect(searchMergeKey({ name: '遮 天', author: '测试作者' })).toBe('遮天|测试作者')
  })
})

describe('source switch author matching', () => {
  it('keeps missing authors and rejects different authors', () => {
    expect(matchesSourceSwitchAuthor('作者：测试作者', '测试作者')).toBe(true)
    expect(matchesSourceSwitchAuthor('测试作者', undefined)).toBe(true)
    expect(matchesSourceSwitchAuthor(undefined, '未知')).toBe(true)
    expect(matchesSourceSwitchAuthor('测试作者', '另一作者')).toBe(false)
  })
})

describe('merged search source candidates', () => {
  it('retains the complete hit for every merged source', () => {
    const first = initializeSearchResult(book({
      name: '测试书目', author: '测试作者', origin: 'source-a', bookUrl: 'https://a/book/1',
    }))
    const merged = mergeSearchResult(first, book({
      name: '测试书目', author: '测试作者', origin: 'source-b', bookUrl: 'https://b/book/2',
      lastChapter: '第20章',
    }))

    expect(merged.bookSourceUrls).toEqual(['source-a', 'source-b'])
    expect(merged.sourceCandidates).toMatchObject([
      { origin: 'source-a', bookUrl: 'https://a/book/1' },
      { origin: 'source-b', bookUrl: 'https://b/book/2', lastChapter: '第20章' },
    ])
  })

  it('does not count duplicate hits from the same source twice', () => {
    const first = initializeSearchResult(book({
      name: '测试书目', author: '测试作者', origin: 'source-a', bookUrl: 'https://a/book/1',
    }))
    const merged = mergeSearchResult(first, book({
      name: '测试书目', author: '测试作者', origin: 'source-a', bookUrl: 'https://a/book/duplicate',
    }))

    expect(merged.bookSourceUrls).toEqual(['source-a'])
    expect(merged.sourceCandidates).toHaveLength(1)
  })
})
