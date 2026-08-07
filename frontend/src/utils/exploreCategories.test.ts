import { describe, expect, it } from 'vitest'
import {
  getExploreCategoryKey,
  getInitialExploreCategoryUrl,
  isExploreCategorySection,
  parseExploreCategories,
} from './exploreCategories'

describe('exploreCategories', () => {
  it('keeps Legado section headers while selecting the first real category', () => {
    const categories = parseExploreCategories(JSON.stringify([
      { title: '排行🏷榜单', url: '' },
      { title: '总排行榜', url: '/rank/' },
      { title: '月排行榜', url: '/rank/monthvisit/' },
      { title: '标签🏷分类', url: '' },
      { title: '全部分类', url: '/fenlei/{{page}}.html' },
    ]))

    expect(categories.map((item) => item.title)).toEqual([
      '排行🏷榜单',
      '总排行榜',
      '月排行榜',
      '标签🏷分类',
      '全部分类',
    ])
    expect(isExploreCategorySection(categories[0])).toBe(true)
    expect(isExploreCategorySection(categories[1])).toBe(false)
    expect(getInitialExploreCategoryUrl(categories)).toBe('/rank/')
  })

  it('creates stable unique keys for repeated empty-url section headers', () => {
    const categories = parseExploreCategories(JSON.stringify([
      { title: '排行🏷榜单', url: '' },
      { title: '标签🏷分类', url: '' },
    ]))

    expect(getExploreCategoryKey(categories[0], 0)).not.toBe(getExploreCategoryKey(categories[1], 1))
  })

  it('parses relaxed json categories with angle bracket style objects', () => {
    const categories = parseExploreCategories(
      '[{"title":"排行🏷榜单","url":"","style":<"layout_flexBasisPercent":1,"layout_flexGrow":1>},{"title":"总排行榜","url":"/rank/","style":<"layout_flexBasisPercent":0.4,"layout_flexGrow":1>}]'
    )

    expect(categories).toEqual([
      { title: '排行🏷榜单', url: '' },
      { title: '总排行榜', url: '/rank/' },
    ])
    expect(getInitialExploreCategoryUrl(categories)).toBe('/rank/')
  })

  it('parses relaxed json categories with angle bracket item objects', () => {
    const categories = parseExploreCategories(
      '[<"style":<"layout_flexBasisPercent":1.0,"layout_flexGrow":1>,"title":"书 库","url":"/book/category/catalog.html">,<"style":<"layout_flexBasisPercent":0.25,"layout_flexGrow":1>,"title":"排 行","url":"/book/ranking.html">,<"style":<"layout_flexBasisPercent":0.25,"layout_flexGrow":1>,"title":">","url":"/ranking/hits/2.html">]'
    )

    expect(categories).toEqual([
      { title: '书 库', url: '/book/category/catalog.html' },
      { title: '排 行', url: '/book/ranking.html' },
      { title: '>', url: '/ranking/hits/2.html' },
    ])
    expect(getInitialExploreCategoryUrl(categories)).toBe('/book/category/catalog.html')
  })
})
