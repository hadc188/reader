import { describe, expect, it } from 'vitest'
import {
  filterBookSources,
  getBookSourceGroups,
  getBookSourceOverview,
  getBookSourceStats,
  getVisibleSelection,
  toBookSourceDeletePayload,
  toRssSourceDeletePayload,
} from './sourceSelection'
import type { BookSource, RssSource } from '../types'

describe('sourceSelection', () => {
  const bookSources: BookSource[] = [
    { bookSourceName: 'Alpha', bookSourceUrl: 'https://alpha.example', enabled: true },
    { bookSourceName: 'Beta', bookSourceUrl: 'https://beta.example', enabled: true },
  ]

  const rssSources: RssSource[] = [
    { sourceName: 'News', sourceUrl: 'https://news.example', enabled: true },
    { sourceName: 'Tech', sourceUrl: 'https://tech.example', enabled: true },
  ]

  it('keeps bulk selection scoped to currently visible source urls', () => {
    const selected = new Set(['https://alpha.example', 'https://hidden.example'])

    expect(getVisibleSelection(bookSources, selected, (source) => source.bookSourceUrl)).toEqual([
      bookSources[0],
    ])
  })

  it('builds batch delete payloads using stable source identifiers', () => {
    expect(toBookSourceDeletePayload(bookSources)).toEqual([
      { bookSourceUrl: 'https://alpha.example' },
      { bookSourceUrl: 'https://beta.example' },
    ])
    expect(toRssSourceDeletePayload(rssSources)).toEqual([
      { sourceName: 'News', sourceUrl: 'https://news.example' },
      { sourceName: 'Tech', sourceUrl: 'https://tech.example' },
    ])
  })

  it('extracts sorted groups from comma-like source group separators', () => {
    const grouped: BookSource[] = [
      { bookSourceName: 'A', bookSourceUrl: 'a', bookSourceGroup: '小说, API' },
      { bookSourceName: 'B', bookSourceUrl: 'b', bookSourceGroup: 'API；精选、小说' },
      { bookSourceName: 'C', bookSourceUrl: 'c' },
    ]

    expect(getBookSourceGroups(grouped)).toEqual(['API', '小说', '精选'])
  })

  it('filters book sources by text and group', () => {
    const grouped: BookSource[] = [
      { bookSourceName: '猫眼看书', bookSourceUrl: 'https://maoyan.example', bookSourceGroup: 'API' },
      { bookSourceName: '笔趣阁', bookSourceUrl: 'https://biqu.example', bookSourceGroup: '网页' },
    ]

    expect(filterBookSources(grouped, 'mao', '')).toEqual([grouped[0]])
    expect(filterBookSources(grouped, '', '网页')).toEqual([grouped[1]])
  })

  it('summarizes source counts and selected source metadata', () => {
    const list: BookSource[] = [
      { bookSourceName: 'Enabled', bookSourceUrl: 'enabled', enabled: true, ruleSearch: {}, ruleToc: {} },
      { bookSourceName: 'Disabled', bookSourceUrl: 'disabled', enabled: false },
    ]

    expect(getBookSourceStats(list, [list[0]])).toEqual({
      total: 2,
      enabled: 1,
      filtered: 1,
    })
    expect(getBookSourceOverview(list[0])).toMatchObject({
      group: '未分组',
      statusText: '启用',
      hasSearch: true,
      hasToc: true,
    })
  })
})
