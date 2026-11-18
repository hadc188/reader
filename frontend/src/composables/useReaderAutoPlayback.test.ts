import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { ref } from 'vue'
import { createPinia, setActivePinia } from 'pinia'
import {
  findParagraphPageIndex,
  getHorizontalAutoPageDelayMs,
  getSpeechProgressItemIndex,
  isSpeechSelfScroll,
  shouldFollowChapterJump,
  useReaderAutoPlayback,
} from './useReaderAutoPlayback'

describe('merged speech paragraph progress', () => {
  it('moves the active paragraph according to each paragraph text length', () => {
    const lengths = [20, 60, 20]

    expect(getSpeechProgressItemIndex(lengths, 0)).toBe(0)
    expect(getSpeechProgressItemIndex(lengths, 0.19)).toBe(0)
    expect(getSpeechProgressItemIndex(lengths, 0.2)).toBe(1)
    expect(getSpeechProgressItemIndex(lengths, 0.79)).toBe(1)
    expect(getSpeechProgressItemIndex(lengths, 0.8)).toBe(2)
    expect(getSpeechProgressItemIndex(lengths, 1)).toBe(2)
  })

describe('horizontal auto page failure paths', () => {
  beforeEach(() => {
    const storage = new Map<string, string>()
    vi.stubGlobal('localStorage', {
      getItem: (key: string) => storage.get(key) ?? null,
      setItem: (key: string, value: string) => storage.set(key, value),
      removeItem: (key: string) => storage.delete(key),
      clear: () => storage.clear(),
    })
    vi.stubGlobal('window', globalThis)
    vi.stubGlobal('requestAnimationFrame', vi.fn(() => 1))
    vi.stubGlobal('cancelAnimationFrame', vi.fn())
    setActivePinia(createPinia())
  })

  afterEach(() => {
    vi.useRealTimers()
    vi.unstubAllGlobals()
  })

  function setupAutoPage(options: {
    ready?: () => boolean
    lastPage?: () => boolean
    hasNext?: boolean
    nextChapter?: () => Promise<void>
  }) {
    vi.useFakeTimers()
    const store = {
      isAutoScrolling: true,
      currentIndex: 0,
      hasNext: options.hasNext ?? true,
      addSpeechStopListener: () => () => undefined,
    }
    const playback = useReaderAutoPlayback(
      store as never,
      ref({ clickAction: 'auto', autoScrollSpeed: 500 }) as never,
      ref(false) as never,
      ref({ clientHeight: 600, querySelectorAll: () => [], scrollTo: () => undefined } as unknown as HTMLElement),
      ref(undefined) as never,
      (options.nextChapter ?? (async () => undefined)) as never,
      vi.fn() as never,
      ref(true) as never,
      ref(0) as never,
      {
        isReady: options.ready ?? (() => true),
        isLastPage: options.lastPage ?? (() => true),
        turnNext: vi.fn(),
      },
    )
    return { playback, store }
  }

  it('末章(hasNext=false)在末页停止，不再换章', async () => {
    const { playback, store } = setupAutoPage({ hasNext: false })
    playback.startAutoScroll()
    await vi.advanceTimersByTimeAsync(1200)
    expect(store.isAutoScrolling).toBe(false)
  })

  it('换章没有推进索引(加载失败/无新章)时停止自动阅读', async () => {
    const { playback, store } = setupAutoPage({})
    playback.startAutoScroll()
    await vi.advanceTimersByTimeAsync(1200)
    expect(store.isAutoScrolling).toBe(false)
  })

  it('换章抛错时停止自动阅读', async () => {
    const { playback, store } = setupAutoPage({
      nextChapter: async () => { throw new Error('network') },
    })
    playback.startAutoScroll()
    await vi.advanceTimersByTimeAsync(1200)
    expect(store.isAutoScrolling).toBe(false)
  })

  it('分页长期不就绪时停止，不留下空转定时器', async () => {
    const { playback, store } = setupAutoPage({ ready: () => false })
    playback.startAutoScroll()
    // 150 次 × 200ms 预算 + 首轮整页停留。
    await vi.advanceTimersByTimeAsync(1200 + 200 * 160)
    expect(store.isAutoScrolling).toBe(false)
  })
})
})

describe('horizontal page lookup for speech following', () => {
  /** 假页容器: 只需要 `contains`, 与运行时用到的能力一致。 */
  function fakePage(...members: object[]): { contains(node: unknown): boolean } {
    return { contains: (node: unknown) => members.includes(node as object) }
  }

  it('finds the page that contains the spoken paragraph', () => {
    const secondParagraph = { text: '第三段' }
    const pages = [fakePage({ text: '第一段' }, { text: '第二段' }), fakePage(secondParagraph)]

    expect(findParagraphPageIndex(pages, secondParagraph)).toBe(1)
  })

  it('returns -1 when the paragraph is not rendered yet', () => {
    // 换章/重建分页期间段落可能已被卸载: 此时不能翻页, 也不能越界。
    const detached = { text: '不在任何页里' }
    expect(findParagraphPageIndex([fakePage({ text: '正文' })], detached)).toBe(-1)
    expect(findParagraphPageIndex([], detached)).toBe(-1)
    expect(findParagraphPageIndex([fakePage({ text: '正文' })], null)).toBe(-1)
  })
})

describe('manual chapter jump following', () => {
  /** 用户手动跳章、朗读进行中、没有过渡、TTS 也没有刚自己滚过正文。 */
  const userJump = {
    speechDriven: false,
    hasSpeechSession: true,
    transitioning: false,
    selfScroll: false,
  }

  it('用户跳章且朗读进行中时跟随', () => {
    expect(shouldFollowChapterJump(userJump)).toBe(true)
  })

  it('TTS 自己切章时不跟随（否则会自己打断自己）', () => {
    expect(shouldFollowChapterJump({ ...userJump, speechDriven: true })).toBe(false)
  })

  it('没有朗读会话时不跟随（纯阅读换章）', () => {
    expect(shouldFollowChapterJump({ ...userJump, hasSpeechSession: false })).toBe(false)
  })

  it('正在切段/切章过渡时不跟随', () => {
    expect(shouldFollowChapterJump({ ...userJump, transitioning: true })).toBe(false)
  })

  it('TTS 刚自己滚过正文时不跟随（连续模式下滚动到章边界也会改索引）', () => {
    expect(shouldFollowChapterJump({ ...userJump, selfScroll: true })).toBe(false)
  })
})

describe('区分「TTS 自动滚动」与「用户滚动」', () => {
  const now = 10_000

  it('滚动反推 + 保护窗口内 + 无用户输入 → TTS 自滚', () => {
    expect(isSpeechSelfScroll({
      scrollDriven: true,
      now,
      lastSpeechScrollAt: now - 100,
      lastUserInputAt: now - 5_000,
    })).toBe(true)
  })

  it('用户刚滚过（输入晚于 TTS 滚动）→ 不算自滚，跳章要被跟随', () => {
    // 用户点的跳章/滚轮发生在 TTS 上次滚动之后：这是真实操作，不能被窗口吞掉。
    expect(isSpeechSelfScroll({
      scrollDriven: true,
      now,
      lastSpeechScrollAt: now - 100,
      lastUserInputAt: now - 50,
    })).toBe(false)
  })

  it('超出保护窗口 → 不算自滚', () => {
    expect(isSpeechSelfScroll({
      scrollDriven: true,
      now,
      lastSpeechScrollAt: now - 5_000,
      lastUserInputAt: 0,
    })).toBe(false)
  })

  it('不是滚动反推（目录/上下一章/进度条）→ 一律不算自滚', () => {
    expect(isSpeechSelfScroll({
      scrollDriven: false,
      now,
      lastSpeechScrollAt: now - 10,
      lastUserInputAt: 0,
    })).toBe(false)
  })
})

describe('horizontal auto page timing', () => {
  it('按一屏高度换算每页停留时长', () => {
    // 600px 一屏、300px/秒 → 2 秒。
    expect(getHorizontalAutoPageDelayMs(600, 300)).toBe(2000)
  })

  it('速度再快也保留最短停留', () => {
    expect(getHorizontalAutoPageDelayMs(100, 500)).toBe(800)
  })

  it('异常尺寸与速度不产生 0 或负时长', () => {
    expect(getHorizontalAutoPageDelayMs(0, 30)).toBe(800)
    expect(getHorizontalAutoPageDelayMs(600, 0)).toBeGreaterThan(0)
  })
})

describe('horizontal auto page runs page by page (issue #49)', () => {
  beforeEach(() => {
    const storage = new Map<string, string>()
    vi.stubGlobal('localStorage', {
      getItem: (key: string) => storage.get(key) ?? null,
      setItem: (key: string, value: string) => storage.set(key, value),
      removeItem: (key: string) => storage.delete(key),
      clear: () => storage.clear(),
    })
    vi.stubGlobal('window', globalThis)
    // 纵向循环的帧调度: 同步执行第一拍。旧实现会在这一拍里判定「已到章末」并跳章,
    // 因此这个桩能让回归测试真正覆盖 issue #49 的原始路径。
    vi.stubGlobal('requestAnimationFrame', vi.fn((cb: FrameRequestCallback) => {
      cb(0)
      return 1
    }))
    vi.stubGlobal('cancelAnimationFrame', vi.fn())
    setActivePinia(createPinia())
  })

  afterEach(() => {
    vi.useRealTimers()
    vi.unstubAllGlobals()
  })

  function setup(options: { pages: number; startPage?: number; hasNext?: boolean; clickAction?: string }) {
    vi.useFakeTimers()
    vi.stubGlobal('window', globalThis)
    let page = options.startPage ?? 0
    let pageCount = options.pages
    const turnNext = vi.fn(() => {
      page += 1
    })
    const store = { currentIndex: 0 } as { currentIndex: number } & Record<string, unknown>
    const nextChapter = vi.fn(async () => {
      // 真实换章: 索引前进、页索引归 0、新章重新分页成 3 页。
      store.currentIndex += 1
      page = 0
      pageCount = 3
    })
    Object.assign(store, {
      isAutoScrolling: true,
      book: null,
      hasNext: options.hasNext ?? true,
      addSpeechStopListener: () => () => undefined,
    })
    const containerScrollBy = vi.fn()
    const container = ref({
      clientHeight: 600,
      clientWidth: 600,
      // 分页容器的真实特征: 正文高度恒等于一屏, 纵向完全没有可滚动空间。
      // 旧实现据此第一帧就判定「已到章末」→ 直接 nextChapter。
      scrollTop: 0,
      scrollHeight: 600,
      querySelectorAll: () => [],
      scrollTo: () => undefined,
      scrollBy: containerScrollBy,
    } as unknown as HTMLElement)
    const playback = useReaderAutoPlayback(
      store as never,
      ref({ clickAction: options.clickAction ?? 'auto', autoScrollSpeed: 500 }) as never,
      ref(false) as never,
      container,
      ref(undefined) as never,
      nextChapter as never,
      vi.fn() as never,
      ref(true) as never,
      ref(0) as never,
      {
        isReady: () => true,
        isLastPage: () => page >= pageCount - 1,
        turnNext,
      },
    )
    return { playback, turnNext, nextChapter, store, containerScrollBy }
  }

  it('启动时不立刻跳章，只在停留一页后翻一页', async () => {
    const { playback, turnNext, nextChapter, containerScrollBy } = setup({ pages: 3 })
    playback.startAutoScroll()

    // 旧实现启动第一帧就判定「已到章末」→ 直接 nextChapter。
    expect(nextChapter).not.toHaveBeenCalled()
    expect(turnNext).not.toHaveBeenCalled()

    expect(containerScrollBy).not.toHaveBeenCalled()
    // 600px 一屏、500px/秒 → 1200ms 后翻页。
    await vi.advanceTimersByTimeAsync(1200)
    expect(turnNext).toHaveBeenCalledTimes(1)
    expect(nextChapter).not.toHaveBeenCalled()
  })

  it('停在末页才换章，且换章后继续逐页推进', async () => {
    const { playback, turnNext, nextChapter, store } = setup({ pages: 1 })
    playback.startAutoScroll()

    await vi.advanceTimersByTimeAsync(1200)
    expect(nextChapter).toHaveBeenCalledTimes(1)
    expect(turnNext).not.toHaveBeenCalled()

    // 新章有 3 页: 换章后先按 200ms 轮询分页就绪, 再完整停留一页(1200ms)。
    await vi.advanceTimersByTimeAsync(200 + 1200)
    expect(turnNext).toHaveBeenCalledTimes(1)
    expect(nextChapter).toHaveBeenCalledTimes(1)
    expect(store.isAutoScrolling).toBe(true)
  })
})
