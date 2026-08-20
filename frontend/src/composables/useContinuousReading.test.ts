import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { computed, ref } from 'vue'
import { createPinia, setActivePinia } from 'pinia'
import { useReaderStore } from '../stores/reader'
import { getBookContent, saveBookProgress } from '../api/bookshelf'
import { useContinuousReading } from './useContinuousReading'

vi.mock('../api/bookshelf', () => ({
  getChapterList: vi.fn(),
  getBookContent: vi.fn(),
  saveBookProgress: vi.fn(),
  setBookSource: vi.fn(),
}))

vi.mock('../api/bookmark', () => ({
  getBookmarks: vi.fn(),
  saveBookmark: vi.fn(),
  deleteBookmark: vi.fn(),
  deleteBookmarks: vi.fn(),
}))

vi.mock('../api/replaceRule', () => ({
  getReplaceRules: vi.fn(),
}))

vi.mock('../api/webdav', () => ({
  syncLegadoBookProgress: vi.fn(),
}))

vi.mock('../utils/recentBooks', () => ({
  saveRecentReadBook: vi.fn(),
}))

describe('hide-read continuous mode advancement', () => {
  beforeEach(() => {
    setActivePinia(createPinia())
    const storage = new Map<string, string>()
    vi.stubGlobal('localStorage', {
      getItem: vi.fn((key: string) => storage.get(key) ?? null),
      setItem: vi.fn((key: string, value: string) => storage.set(key, value)),
      removeItem: vi.fn((key: string) => storage.delete(key)),
      clear: vi.fn(() => storage.clear()),
    })
    vi.stubGlobal('window', {
      setTimeout: (handler: () => void, ms?: number) => setTimeout(handler, ms),
      clearTimeout: (id: number) => clearTimeout(id),
      requestAnimationFrame: (handler: () => void) => setTimeout(handler, 0),
    })
    vi.mocked(getBookContent).mockReset()
    vi.mocked(getBookContent).mockImplementation(async ({ chapterUrl }) => `${chapterUrl}-正文`)
    vi.mocked(saveBookProgress).mockReset()
    vi.mocked(saveBookProgress).mockResolvedValue('ok')
  })

  afterEach(() => {
    vi.unstubAllGlobals()
  })

  function setupHarness() {
    const store = useReaderStore()
    store.book = {
      name: '回看书',
      author: '测试作者',
      origin: 'test-source',
      bookUrl: 'https://example.test/reread',
    }
    store.chapters = Array.from({ length: 22 }, (_, index) => ({
      title: `第${index + 1}章`,
      url: `chapter-${index}`,
      index,
    }))
    // 模拟回看: 已读 0..20 章, 阅读位置退回到第 11 章(index 11)
    for (let index = 0; index <= 20; index += 1) {
      store.markChapterAsRead(index)
    }

    const harness = useContinuousReading(
      store,
      (text) => text,
      computed(() => true),
      computed(() => true),
      ref<HTMLElement | undefined>(undefined),
    )
    return { store, ...harness }
  }

  it('appends the immediate next chapter even when it is marked read', async () => {
    const { initializeContinuousChapters, continuousChapters } = setupHarness()

    await initializeContinuousChapters(11, false)
    await vi.waitFor(() => {
      // 修复前: 追加的是下一个未读章(index 21), 从 11 直接跳到 20 章标题
      expect(continuousChapters.value.map((chapter) => chapter.index)).toEqual([11, 12])
    })
  })

  it('keeps loading chapters sequentially when scrolling forward', async () => {
    const { initializeContinuousChapters, loadContinuousNext, continuousChapters } = setupHarness()

    await initializeContinuousChapters(11, false)
    await vi.waitFor(() => {
      expect(continuousChapters.value.map((chapter) => chapter.index)).toEqual([11, 12])
    })

    await loadContinuousNext()
    expect(continuousChapters.value.map((chapter) => chapter.index)).toEqual([11, 12, 13])
  })

  it('still prunes read chapters behind the current position', async () => {
    const { store, initializeContinuousChapters, pruneReadChapters, continuousChapters } = setupHarness()

    await initializeContinuousChapters(11, false)
    await vi.waitFor(() => {
      expect(continuousChapters.value.map((chapter) => chapter.index)).toEqual([11, 12])
    })

    await pruneReadChapters(store.currentIndex + 1)
    expect(continuousChapters.value.map((chapter) => chapter.index)).toEqual([12])
  })
})
