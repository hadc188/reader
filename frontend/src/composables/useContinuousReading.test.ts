import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { computed, ref } from 'vue'
import { createPinia, setActivePinia } from 'pinia'
import { useReaderStore } from '../stores/reader'
import { useAppStore } from '../stores/app'
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

  it('keeps the saved chapter progress when initializing at the open position', async () => {
    const { store, initializeContinuousChapters } = setupHarness()
    // loadBook/loadChapter 设置的上次阅读位置, 打开书时不得被初始化清零
    store.setChapterScrollProgress(0.42)

    await initializeContinuousChapters(11, false)

    expect(store.chapterScrollProgress).toBe(0.42)
    expect(store.book?.durChapterPos).toBe(4200)
  })

  it('resets progress to the chapter top on deliberate jumps', async () => {
    const { store, initializeContinuousChapters } = setupHarness()
    store.setChapterScrollProgress(0.42)

    await initializeContinuousChapters(11, false, true)

    expect(store.chapterScrollProgress).toBe(0)
  })

  describe('preload integration', () => {
    it('does not fetch the next chapter again once preloaded ahead', async () => {
      const { store, initializeContinuousChapters, loadContinuousNext, continuousChapters } = setupHarness()
      store.updateConfig('enablePreload', true)

      await initializeContinuousChapters(11, false)
      await vi.waitFor(() => {
        expect(continuousChapters.value.map((chapter) => chapter.index)).toEqual([11, 12])
      })

      // 初始化完成后应已把下游章节预载进缓存
      await vi.waitFor(() => {
        expect(store.isChapterReady(13)).toBe(true)
      })
      const callsBefore = vi.mocked(getBookContent).mock.calls
        .filter(([params]) => params.chapterUrl === 'chapter-13').length

      await loadContinuousNext()
      expect(continuousChapters.value.map((chapter) => chapter.index)).toEqual([11, 12, 13])

      // 预加载生效: 追加第 13 章不应再发一次请求
      const callsAfter = vi.mocked(getBookContent).mock.calls
        .filter(([params]) => params.chapterUrl === 'chapter-13').length
      expect(callsAfter).toBe(callsBefore)
    })

    it('preloads from the list tail so already-rendered chapters are not refetched', async () => {
      const { store, initializeContinuousChapters, loadContinuousNext, continuousChapters } = setupHarness()
      store.updateConfig('enablePreload', true)

      await initializeContinuousChapters(11, false)
      await vi.waitFor(() => {
        expect(continuousChapters.value.map((chapter) => chapter.index)).toEqual([11, 12])
      })

      // 用户往下滚了三章, 但 store.currentIndex 仍停在 11(尚未跨越章节边界)。
      // 预载必须以列表尾部(index 15)为基准, 否则会反复拉取已在 DOM 里的章节。
      await loadContinuousNext()
      await loadContinuousNext()
      await loadContinuousNext()
      expect(continuousChapters.value.map((chapter) => chapter.index)).toEqual([11, 12, 13, 14, 15])

      await vi.waitFor(() => {
        expect(store.isChapterReady(16)).toBe(true)
      })
      // 已渲染的章节各自只应被真正请求一次, 预载不得回头重复拉取它们
      for (const index of [12, 13, 14, 15]) {
        const calls = vi.mocked(getBookContent).mock.calls
          .filter(([params]) => params.chapterUrl === `chapter-${index}`)
        expect(calls).toHaveLength(1)
      }
    })

    it('skips the previous chapter when hide-read mode prunes it anyway', async () => {
      const { store, initializeContinuousChapters, continuousChapters } = setupHarness()
      store.updateConfig('enablePreload', true)

      await initializeContinuousChapters(11, false)
      await vi.waitFor(() => {
        expect(continuousChapters.value.map((chapter) => chapter.index)).toEqual([11, 12])
      })

      // 隐藏已读形态: 预载 13/14, 不浪费缓存去预载马上会被裁掉的 10
      await vi.waitFor(() => {
        expect(store.isChapterReady(14)).toBe(true)
      })
      expect(store.isChapterReady(10)).toBe(false)
      expect(
        vi.mocked(getBookContent).mock.calls.filter(([params]) => params.chapterUrl === 'chapter-10'),
      ).toHaveLength(0)
    })

    it('retries preloading downstream after a failed preload', async () => {
      const { store, initializeContinuousChapters, preloadContinuousAhead, continuousChapters } = setupHarness()
      store.updateConfig('enablePreload', true)
      useAppStore().setOnlineStatus(true)

      // 下游章节首次预载失败。必须在线: 离线时 store 会把任何错误统一换成
      // 「当前处于离线状态」, 那测的就不是重试逻辑了。当前章本身要能取到,
      // 否则 initializeContinuousChapters 会直接把错误抛出来。
      vi.mocked(getBookContent).mockImplementation(async ({ chapterUrl }) => {
        if (chapterUrl === 'chapter-13' || chapterUrl === 'chapter-14') {
          throw new Error('网络失败')
        }
        return `${chapterUrl}-正文`
      })
      await initializeContinuousChapters(11, false)
      await vi.waitFor(() => {
        expect(continuousChapters.value.map((chapter) => chapter.index)).toEqual([11, 12])
      })
      expect(store.isChapterReady(13)).toBe(false)

      // 恢复后下一次滚动触发必须重试, 不能被"上次已预载过"的守卫永久挡住
      vi.mocked(getBookContent).mockImplementation(async ({ chapterUrl }) => `${chapterUrl}-正文`)
      preloadContinuousAhead()
      await vi.waitFor(() => {
        expect(store.isChapterReady(13)).toBe(true)
      })
    })

    it('shows no next-chapter loading flag while content came from preload', async () => {
      const { store, initializeContinuousChapters, loadContinuousNext, continuousLoadingNext, continuousChapters } = setupHarness()
      store.updateConfig('enablePreload', true)

      await initializeContinuousChapters(11, false)
      await vi.waitFor(() => {
        expect(continuousChapters.value.map((chapter) => chapter.index)).toEqual([11, 12])
      })
      await vi.waitFor(() => {
        expect(store.isChapterReady(13)).toBe(true)
      })

      const pending = loadContinuousNext()
      // 命中预载时不亮提示 —— 提示被渲染出来闪一下正是"预加载没生效"的观感
      expect(continuousLoadingNext.value).toBe(false)
      await pending
      expect(continuousLoadingNext.value).toBe(false)
    })
  })
})

describe('volume chapters in continuous reading', () => {
  function setupVolumeHarness() {
    setActivePinia(createPinia())
    const store = useReaderStore()
    store.book = {
      name: '分卷书',
      author: '测试作者',
      origin: 'local-txt',
      bookUrl: 'local-txt:volume',
    }
    store.chapters = [
      { title: '第一卷', url: 'chapter-0', index: 0, isVolume: true },
      { title: '第一章 正文', url: 'chapter-1', index: 1 },
    ]
    vi.mocked(getBookContent).mockImplementation(async ({ chapterUrl }) => {
      // 卷标题章没有正文 —— store 也不该把它当解析失败
      if (chapterUrl === 'chapter-0') return ''
      return `${chapterUrl} 内容`
    })
    const harness = useContinuousReading(
      store,
      (text) => text,
      computed(() => true),
      computed(() => false),
      ref<HTMLElement | undefined>(undefined),
    )
    return { store, ...harness }
  }

  beforeEach(() => {
    setActivePinia(createPinia())
    // app store 在模块级就读 localStorage，没有 stub 会在建 store 时直接抛错。
    const storage = new Map<string, string>()
    vi.stubGlobal('localStorage', {
      getItem: vi.fn((key: string) => storage.get(key) ?? null),
      setItem: vi.fn((key: string, value: string) => storage.set(key, value)),
      removeItem: vi.fn((key: string) => storage.delete(key)),
      clear: vi.fn(() => storage.clear()),
    })
    vi.mocked(getBookContent).mockReset()
    vi.mocked(getBookContent).mockResolvedValue('正文')
    vi.mocked(saveBookProgress).mockReset()
    vi.mocked(saveBookProgress).mockResolvedValue('ok')
    vi.stubGlobal('window', {
      setTimeout: (handler: () => void, ms?: number) => setTimeout(handler, ms),
      clearTimeout: (id: number) => clearTimeout(id),
      requestAnimationFrame: (handler: () => void) => setTimeout(handler, 0),
    })
  })

  afterEach(() => {
    vi.unstubAllGlobals()
  })

  /**
   * 卷标题章必须把 isVolume 一路带到渲染层：模板据此只画居中标题、
   * 不画正文区。缺了这个标记，模板会先渲染一次标题、再渲染一次（空的）
   * 正文区，用户看到的就是「第一卷」出现两遍（issue #33 截图）。
   */
  it('carries isVolume onto the continuous chapter item', async () => {
    const { initializeContinuousChapters, continuousChapters } = setupVolumeHarness()

    await initializeContinuousChapters(0, false)
    await vi.waitFor(() => {
      expect(continuousChapters.value.map((chapter) => chapter.index)).toEqual([0, 1])
    })

    const volume = continuousChapters.value.find((chapter) => chapter.index === 0)
    expect(volume?.isVolume).toBe(true)
    const normal = continuousChapters.value.find((chapter) => chapter.index === 1)
    expect(normal?.isVolume).toBe(false)
  })

  /** 普通章节不能被误标成卷，否则正文整段消失。 */
  it('does not mark ordinary chapters as volume', async () => {
    const { initializeContinuousChapters, continuousChapters } = setupVolumeHarness()

    await initializeContinuousChapters(1, false)
    await vi.waitFor(() => {
      expect(continuousChapters.value.length).toBeGreaterThan(0)
    })
    for (const chapter of continuousChapters.value) {
      expect(chapter.isVolume).toBe(false)
    }
  })
})