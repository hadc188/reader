import { beforeEach, describe, expect, it, vi } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'
import { useExploreStore } from './explore'
import { useSourceStore } from './source'
import { getBookSources } from '../api/source'
import { exploreBook } from '../api/explore'
import type { BookSource } from '../types'

vi.mock('../api/source', () => ({
  getBookSources: vi.fn(),
}))

vi.mock('../api/explore', () => ({
  exploreBook: vi.fn(),
}))

const getBookSourcesMock = vi.mocked(getBookSources)
const exploreBookMock = vi.mocked(exploreBook)

describe('explore store source sync', () => {
  beforeEach(() => {
    setActivePinia(createPinia())
    getBookSourcesMock.mockReset()
    exploreBookMock.mockReset()
    exploreBookMock.mockResolvedValue([])
  })

  it('repairs a stale active source when explore sources are already loaded', async () => {
    const sourceStore = useSourceStore()
    sourceStore.sources = [sourceWithExplore()]
    const store = useExploreStore()
    store.activeSourceUrl = 'https://missing.example'

    await store.init()

    expect(store.activeSourceUrl).toBe('https://m.cuoceng.com')
    expect(store.categories.map((category) => category.title)).toEqual(['书 库', '排 行'])
    expect(store.activeCategoryUrl).toBe('/book/category/catalog.html')
  })
})

function sourceWithExplore(): BookSource {
  return {
    bookSourceName: 'm.cuoceng.com',
    bookSourceUrl: 'https://m.cuoceng.com',
    enabledExplore: true,
    exploreUrl: JSON.stringify([
      {
        style: { layout_flexBasisPercent: 1.0, layout_flexGrow: 1 },
        title: '书 库',
        url: '/book/category/catalog.html',
      },
      {
        style: { layout_flexBasisPercent: 0.25, layout_flexGrow: 1 },
        title: '排 行',
        url: '/book/ranking.html',
      },
    ]),
  }
}
