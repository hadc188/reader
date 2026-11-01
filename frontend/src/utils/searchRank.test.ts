import { describe, expect, it } from 'vitest'
import {
  initializeSearchResult,
  isSearchResultRelevant,
  mergeSearchResult,
  mergeSourceCandidates,
  rankSearchResults,
  searchMergeKey,
  shelfMatchKey,
  matchesSourceSwitchTitle,
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

describe('shelfMatchKey', () => {
  it('keeps web books interchangeable across sources', () => {
    // 同一本网络书在不同书源 URL 不同，但书架里只该算一本。
    expect(shelfMatchKey({ name: '遮天', author: '辰东', origin: 'https://a.com' }))
      .toBe(shelfMatchKey({ name: '遮天', author: '辰东', origin: 'https://b.com/book' }))
  })

  it('separates local books from web books', () => {
    // 回归：书架里导入过本地《遮天》时，搜索到的网络《遮天》被标成「已加入」。
    const local = shelfMatchKey({ name: '遮天', author: '辰东', origin: 'local-txt' })
    const web = shelfMatchKey({ name: '遮天', author: '辰东', origin: 'https://a.com' })
    expect(local).not.toBe(web)
  })

  it('covers all three local kinds as the same class', () => {
    const txt = shelfMatchKey({ name: '遮天', author: '辰东', origin: 'local-txt' })
    const epub = shelfMatchKey({ name: '遮天', author: '辰东', origin: 'local-epub' })
    const pdf = shelfMatchKey({ name: '遮天', author: '辰东', origin: 'local-pdf' })
    expect(txt).toBe(epub)
    expect(epub).toBe(pdf)
  })

  it('still normalizes name and author like searchMergeKey', () => {
    expect(shelfMatchKey({ name: ' 遮 天 ', author: '作者：辰 东', origin: 'https://a.com' }))
      .toBe(shelfMatchKey({ name: '遮天', author: '辰东', origin: 'https://a.com' }))
  })

  it('treats a missing origin as a web book', () => {
    // 老数据可能没有 origin；当成网络书比当成本地书更安全。
    expect(shelfMatchKey({ name: '遮天', author: '辰东' }))
      .toBe(shelfMatchKey({ name: '遮天', author: '辰东', origin: 'https://a.com' }))
  })
})

describe('source switch title matching', () => {
  it('keeps the exact title with source-specific whitespace', () => {
    expect(matchesSourceSwitchTitle('斗破苍穹', '斗破苍穹')).toBe(true)
    expect(matchesSourceSwitchTitle('斗破苍穹', ' 斗破 苍穹　')).toBe(true)
  })

  it('rejects related books and empty titles', () => {
    expect(matchesSourceSwitchTitle('斗破苍穹', '斗破苍穹之无上之境')).toBe(false)
    expect(matchesSourceSwitchTitle('斗破苍穹', '斗破苍穹后传')).toBe(false)
    expect(matchesSourceSwitchTitle('', '')).toBe(false)
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
      name: '测试书目', author: '测试作者', origin: 'source-a', bookUrl: 'https://a/book/1',
    }))

    expect(merged.bookSourceUrls).toEqual(['source-a'])
    expect(merged.sourceCandidates).toHaveLength(1)
  })

  it('keeps every aggregate-site link under one source while counting the source once', () => {
    const first = initializeSearchResult(book({
      name: '斗破苍穹', author: '天蚕土豆', origin: 'source-aggregate',
      bookUrl: 'https://aggregate/book/1?site=maoyan', lastChapter: '猫眼',
    }))
    const merged = mergeSearchResult(first, book({
      name: '斗破苍穹', author: '天蚕土豆', origin: 'source-aggregate',
      bookUrl: 'https://aggregate/book/1?site=baidu', lastChapter: '百度',
    }))

    expect(merged.bookSourceUrls).toEqual(['source-aggregate'])
    expect(merged.sourceCandidates?.map((candidate) => candidate.lastChapter)).toEqual(['猫眼', '百度'])
    expect(first.sourceCandidates).toHaveLength(1)
  })

  it('retains candidates already attached to an incoming merged book', () => {
    const one = book({ name: '斗破苍穹', author: '天蚕土豆', origin: 'source-a', bookUrl: 'https://a/1' })
    const two = book({ ...one, bookUrl: 'https://a/2' })
    const three = book({ ...one, origin: 'source-b', bookUrl: 'https://b/3' })
    const restored = initializeSearchResult({
      ...one,
      bookSourceUrls: ['source-a', 'source-b'],
      sourceCandidates: [one, two, three],
    })
    expect(restored.sourceCandidates).toEqual([one, two, three])
    expect(restored.bookSourceUrls).toEqual(['source-a', 'source-b'])

    const merged = mergeSearchResult(initializeSearchResult(one), restored)
    expect(merged.sourceCandidates).toEqual([one, two, three])
    expect(merged.bookSourceUrls).toEqual(['source-a', 'source-b'])
    expect(merged.sourceCandidates?.every((candidate) => !candidate.sourceCandidates)).toBe(true)
  })
})

describe('mergeSourceCandidates', () => {
  it('updates the same link and preserves known metadata when the new hit omits it', () => {
    const previous = book({
      name: '斗破苍穹', author: '天蚕土豆', origin: 'https://source.example/',
      bookUrl: 'https://books.example/1', coverUrl: 'https://books.example/cover.jpg', lastChapter: '第1章',
    })
    const updated = book({
      name: '斗破苍穹', origin: 'https://source.example',
      bookUrl: previous.bookUrl, lastChapter: '第2章',
    })
    expect(mergeSourceCandidates([previous], [updated])).toEqual([
      expect.objectContaining({ ...previous, origin: updated.origin, lastChapter: '第2章' }),
    ])
    expect(previous.lastChapter).toBe('第1章')
  })

  it('keeps different source rules for a shared book URL and ignores unusable hits', () => {
    const one = book({ name: '斗破苍穹', origin: 'source-a', bookUrl: 'https://books.example/1' })
    const two = book({ ...one, origin: 'source-b' })
    expect(mergeSourceCandidates([one, two], [
      book({ ...one, bookUrl: '' }),
      book({ ...one, origin: '' }),
    ])).toEqual([one, two])
  })
})
