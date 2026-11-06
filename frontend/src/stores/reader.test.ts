import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'
import { useAppStore } from './app'
import { nightThemeIndex, useReaderStore } from './reader'
import {
  getBookContent,
  getBookInfo,
  getChapterList,
  saveBookProgress,
  type ContentLoadStage,
} from '../api/bookshelf'
import { syncLegadoBookProgress, uploadLegadoBookProgress } from '../api/webdav'
import { requestOpenAISpeechAudio } from '../utils/openaiSpeech'

vi.mock('../api/bookshelf', () => ({
  getChapterList: vi.fn(),
  getBookInfo: vi.fn(),
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
  uploadLegadoBookProgress: vi.fn(),
}))

vi.mock('../utils/recentBooks', () => ({
  saveRecentReadBook: vi.fn(),
}))

vi.mock('../api/background', () => ({
  deleteBackgroundImage: vi.fn(() => Promise.resolve(null)),
  uploadBackgroundImage: vi.fn(),
}))

vi.mock('../utils/openaiSpeech', () => ({
  DEFAULT_OPENAI_BASE_URL: 'https://api.openai.com/v1',
  inferSpeechApiFormat: vi.fn(() => 'openai'),
  getSpeechApiFormatOption: vi.fn(() => ({ supportedFormats: ['mp3', 'wav', 'opus', 'flac', 'pcm'] })),
  requestOpenAISpeechAudio: vi.fn(),
}))

describe('reader local txt chapters', () => {
  afterEach(() => {
    vi.unstubAllGlobals()
  })

  beforeEach(() => {
    setActivePinia(createPinia())
    const storage = new Map<string, string>()
    vi.stubGlobal('localStorage', {
      getItem: vi.fn((key: string) => storage.get(key) ?? null),
      setItem: vi.fn((key: string, value: string) => storage.set(key, value)),
      removeItem: vi.fn((key: string) => storage.delete(key)),
      clear: vi.fn(() => storage.clear()),
    })
    vi.mocked(getBookContent).mockReset()
    vi.mocked(getBookInfo).mockReset()
    vi.mocked(getBookInfo).mockResolvedValue({
      name: '测试书籍', author: '测试作者', origin: 'test-source', bookUrl: 'https://example.test/book',
    })
    vi.mocked(getChapterList).mockReset()
    vi.mocked(saveBookProgress).mockReset()
    vi.mocked(saveBookProgress).mockResolvedValue('ok')
    vi.mocked(syncLegadoBookProgress).mockReset()
    vi.mocked(uploadLegadoBookProgress).mockReset()
    // 加载动画的延迟兜底走 window.setTimeout(与 store 里其它定时器一致)
    vi.stubGlobal('window', {
      setTimeout: (handler: () => void, ms?: number) => setTimeout(handler, ms),
      clearTimeout: (id: number) => clearTimeout(id),
    })
  })

  it('fetches uploaded local txt content from backend when offline', async () => {
    vi.mocked(getBookContent).mockResolvedValue('本地正文')
    const appStore = useAppStore()
    const readerStore = useReaderStore()
    appStore.setOnlineStatus(false)
    readerStore.book = {
      name: '本地书',
      author: '本地导入',
      origin: 'local-txt',
      bookUrl: 'local-txt:abc123',
    }
    readerStore.chapters = [
      { title: '第一章', url: 'local-txt:abc123#0', index: 0 },
    ]

    await expect(readerStore.fetchChapterContent(0)).resolves.toBe('本地正文')

    expect(getBookContent).toHaveBeenCalledWith({
      bookUrl: 'local-txt:abc123',
      chapterUrl: 'local-txt:abc123#0',
      bookSourceUrl: 'local-txt',
      chapterTitle: '第一章',
      refresh: 0,
    })
  })

  it('lets an empty volume chapter through instead of reporting a parse error', async () => {
    const appStore = useAppStore()
    const readerStore = useReaderStore()
    appStore.setOnlineStatus(false)
    readerStore.book = {
      name: '本地书',
      author: '本地导入',
      origin: 'local-txt',
      bookUrl: 'local-txt:abc123',
    }
    readerStore.chapters = [
      { title: '第一卷', url: 'local-txt:abc123#0', index: 0, isVolume: true },
    ]
    vi.mocked(getBookContent).mockResolvedValue('')

    // 卷标题章正文天然为空，必须原样返回空串交给渲染层画标题页，
    // 而不是抛「书源正文解析为空」（issue #33）。
    await expect(readerStore.fetchChapterContent(0)).resolves.toBe('')
  })

  it('still rejects an empty chapter that is not a volume', async () => {
    const appStore = useAppStore()
    const readerStore = useReaderStore()
    // 在线状态下才会暴露原始错误；离线分支会先把任何错误改写成
    // 「当前处于离线状态」，测不到这里要验证的判定。
    appStore.setOnlineStatus(true)
    readerStore.book = {
      name: '网络书',
      author: '作者',
      origin: 'test-source',
      bookUrl: 'https://example.test/book',
    }
    readerStore.chapters = [
      { title: '第一章', url: 'chapter-0', index: 0 },
    ]
    vi.mocked(getBookContent).mockResolvedValue('   ')

    await expect(readerStore.fetchChapterContent(0)).rejects.toThrow('正文解析为空')
  })

  it('preloads nearby chapters and reuses the completed content when switching', async () => {
    const appStore = useAppStore()
    const readerStore = useReaderStore()
    appStore.setOnlineStatus(true)
    readerStore.updateConfig('enablePreload', true)
    readerStore.book = {
      name: '测试书籍',
      author: '测试作者',
      origin: 'test-source',
      bookUrl: 'https://example.test/book',
    }
    readerStore.chapters = [
      { title: '第一章', url: 'chapter-0', index: 0 },
      { title: '第二章', url: 'chapter-1', index: 1 },
      { title: '第三章', url: 'chapter-2', index: 2 },
    ]
    vi.mocked(getBookContent).mockImplementation(async ({ chapterUrl }) => `${chapterUrl}-正文`)

    await readerStore.preloadAroundChapter(0)
    await readerStore.loadChapter(1)

    expect(readerStore.currentIndex).toBe(1)
    expect(readerStore.content).toBe('chapter-1-正文')
    const chapterCalls = vi.mocked(getBookContent).mock.calls
    expect(chapterCalls.filter(([params]) => params.chapterUrl === 'chapter-1')).toHaveLength(1)
    expect(getBookContent).toHaveBeenCalledWith({
      bookUrl: 'https://example.test/book',
      chapterUrl: 'chapter-1',
      bookSourceUrl: 'test-source',
      chapterTitle: '第二章',
      refresh: 0,
    })
  })

  it('does not wait for progress saving after cached chapter content is displayed', async () => {
    const appStore = useAppStore()
    const readerStore = useReaderStore()
    appStore.setOnlineStatus(true)
    readerStore.updateConfig('enablePreload', true)
    readerStore.book = {
      name: '测试书籍',
      author: '测试作者',
      origin: 'test-source',
      bookUrl: 'https://example.test/book',
    }
    readerStore.chapters = [
      { title: '第一章', url: 'chapter-0', index: 0 },
      { title: '第二章', url: 'chapter-1', index: 1 },
    ]
    vi.mocked(getBookContent).mockResolvedValue('缓存章节正文')
    await readerStore.preloadNextChapter(1)

    let resolveProgress: ((value: string) => void) | undefined
    vi.mocked(saveBookProgress).mockReturnValue(new Promise((resolve) => {
      resolveProgress = resolve
    }))

    await readerStore.loadChapter(1)

    expect(readerStore.loading).toBe(false)
    expect(readerStore.content).toBe('缓存章节正文')
    resolveProgress?.('ok')
  })

  it('exposes chapter load stage to the UI and clears it when finished', async () => {
    const appStore = useAppStore()
    const readerStore = useReaderStore()
    appStore.setOnlineStatus(true)
    readerStore.book = {
      name: '测试书籍',
      author: '测试作者',
      origin: 'test-source',
      bookUrl: 'https://example.test/stage',
    }
    readerStore.chapters = [{ title: '第一章', url: 'chapter-0', index: 0 }]

    let resolveContent: ((value: string) => void) | undefined
    vi.mocked(getBookContent).mockImplementation(async (params) => {
      // 模拟后端在请求过程中推阶段（书源脚本 / 段评解析）
      params.onStage?.('parsing')
      return new Promise<string>((resolve) => {
        resolveContent = resolve
      })
    })

    const loading = readerStore.loadChapter(0)
    await vi.waitFor(() => {
      expect(readerStore.loadStage).toBe('parsing')
    })

    resolveContent?.('chapter-0-正文')
    await loading
    // 结束后必须清空，否则界面会一直停在「正在解析正文与段评…」
    expect(readerStore.loadStage).toBe('')
  })

  it('shows the load stage immediately, without waiting for backend stage events', async () => {
    const appStore = useAppStore()
    const readerStore = useReaderStore()
    appStore.setOnlineStatus(true)
    readerStore.book = {
      name: '测试书籍',
      author: '测试作者',
      origin: 'test-source',
      bookUrl: 'https://example.test/stage-immediate',
    }
    readerStore.chapters = [{ title: '第一章', url: 'chapter-0', index: 0 }]

    let resolveContent: ((value: string) => void) | undefined
    // 模拟「后端阶段事件迟迟不来」：跳章后不能表现为点了没反应
    vi.mocked(getBookContent).mockImplementation(
      async () =>
        new Promise<string>((resolve) => {
          resolveContent = resolve
        }),
    )

    const loading = readerStore.loadChapter(0)
    await vi.waitFor(() => {
      expect(readerStore.loadStage).toBe('fetching')
    })

    resolveContent?.('chapter-0-正文')
    await loading
    expect(readerStore.loadStage).toBe('')
  })

  it('does not leave a stuck stage when the chapter comes from preload', async () => {
    const appStore = useAppStore()
    const readerStore = useReaderStore()
    appStore.setOnlineStatus(true)
    readerStore.updateConfig('enablePreload', true)
    readerStore.book = {
      name: '测试书籍',
      author: '测试作者',
      origin: 'test-source',
      bookUrl: 'https://example.test/stage-preload',
    }
    readerStore.chapters = [{ title: '第一章', url: 'chapter-0', index: 0 }]

    vi.mocked(getBookContent).mockResolvedValue('chapter-0-正文')
    await readerStore.preloadNextChapter(0)
    expect(readerStore.loadStage).toBe('')

    // 连续阅读/跳章会对同一章再要一次内容：这一次命中预加载、不发请求，
    // 因此**不能**留下「正在获取章节…」——否则提示会永久卡住。
    const content = await readerStore.fetchChapterContent(0, false, true)
    expect(content).toBe('chapter-0-正文')
    expect(readerStore.loadStage).toBe('')
  })

  it('keeps the next chapter cached while reading forward', async () => {
    const appStore = useAppStore()
    const readerStore = useReaderStore()
    appStore.setOnlineStatus(true)
    readerStore.updateConfig('enablePreload', true)
    readerStore.book = {
      name: '测试书籍',
      author: '测试作者',
      origin: 'test-source',
      bookUrl: 'https://example.test/evict',
    }
    readerStore.chapters = Array.from({ length: 12 }, (_, index) => ({
      title: `第${index + 1}章`,
      url: `chapter-${index}`,
      index,
    }))
    vi.mocked(getBookContent).mockImplementation(async ({ chapterUrl }) => `${chapterUrl}-正文`)

    const fetchCount = (index: number) => vi.mocked(getBookContent).mock.calls
      .filter(([params]) => params.chapterUrl === `chapter-${index}`).length

    await readerStore.loadChapter(0)
    // 一路往下读 10 章。旧实现容量只有 3 且淘汰按插入序, 下一章经常在翻页那一刻
    // 正好被挤掉, 于是每次翻章都要重新请求 —— 用户看到的就是加载动画。
    for (let index = 1; index <= 10; index += 1) {
      await readerStore.loadChapter(index)
      expect(readerStore.content).toBe(`chapter-${index}-正文`)
      // 每章只应真正请求一次: 此后都由预载缓存供正文
      expect(fetchCount(index)).toBe(1)
    }
  })

  it('keeps the chapter currently being read when preloading further ahead', async () => {
    const appStore = useAppStore()
    const readerStore = useReaderStore()
    appStore.setOnlineStatus(true)
    readerStore.updateConfig('enablePreload', true)
    readerStore.book = {
      name: '测试书籍',
      author: '测试作者',
      origin: 'test-source',
      bookUrl: 'https://example.test/touch',
    }
    readerStore.chapters = Array.from({ length: 12 }, (_, index) => ({
      title: `第${index + 1}章`,
      url: `chapter-${index}`,
      index,
    }))
    vi.mocked(getBookContent).mockImplementation(async ({ chapterUrl }) => `${chapterUrl}-正文`)

    await readerStore.loadChapter(0)
    await readerStore.loadChapter(1)
    // 反复预载远端批次, 一路把缓存写满。当前章 1 被反复命中(每次读都被 touch),
    // 不该被后来的远端预载挤出去 —— 否则重排/回看又要重新请求。
    await readerStore.preloadAroundChapter(5)
    await readerStore.preloadAroundChapter(7)
    await readerStore.fetchChapterContent(1)

    const chapterOneCalls = vi.mocked(getBookContent).mock.calls
      .filter(([params]) => params.chapterUrl === 'chapter-1').length
    expect(chapterOneCalls).toBe(1)
  })

  it('does not show the loading state while the next chapter is already preloading', async () => {
    const appStore = useAppStore()
    const readerStore = useReaderStore()
    appStore.setOnlineStatus(true)
    readerStore.updateConfig('enablePreload', true)
    readerStore.book = {
      name: '测试书籍',
      author: '测试作者',
      origin: 'test-source',
      bookUrl: 'https://example.test/inflight',
    }
    readerStore.chapters = [
      { title: '第一章', url: 'chapter-0', index: 0 },
      { title: '第二章', url: 'chapter-1', index: 1 },
    ]

    let resolveContent: ((value: string) => void) | undefined
    vi.mocked(getBookContent).mockImplementation(
      async () =>
        new Promise<string>((resolve) => {
          resolveContent = resolve
        }),
    )

    const preload = readerStore.preloadNextChapter(1)
    await vi.waitFor(() => {
      expect(getBookContent).toHaveBeenCalledTimes(1)
    })

    const loading = readerStore.loadChapter(1)
    // 预载已经在路上, 只是还没返回: 这里亮全屏加载动画就会闪一下,
    // 正是"开了预加载还是有加载动画"的观感来源。
    expect(readerStore.loading).toBe(false)

    resolveContent?.('chapter-1-正文')
    await loading
    await preload
    expect(readerStore.content).toBe('chapter-1-正文')
    // 复用在途预载: 第 1 章不能再发第二次请求(切章后预载上一章是另一回事)
    const chapterOneCalls = vi.mocked(getBookContent).mock.calls
      .filter(([params]) => params.chapterUrl === 'chapter-1')
    expect(chapterOneCalls).toHaveLength(1)
  })

  it('falls back to a real request when the reused in-flight preload failed', async () => {
    const appStore = useAppStore()
    const readerStore = useReaderStore()
    appStore.setOnlineStatus(true)
    readerStore.updateConfig('enablePreload', true)
    readerStore.book = {
      name: '测试书籍',
      author: '测试作者',
      origin: 'test-source',
      bookUrl: 'https://example.test/inflight-fail',
    }
    readerStore.chapters = [
      { title: '第一章', url: 'chapter-0', index: 0 },
      { title: '第二章', url: 'chapter-1', index: 1 },
    ]

    // 预载失败(preloadNextChapter 会把错误吞成 null), 随后的翻章必须自己重试,
    // 否则用户只会看到"翻了下一章什么也没发生"。
    vi.mocked(getBookContent).mockRejectedValueOnce(new Error('网络失败'))
    vi.mocked(getBookContent).mockResolvedValue('chapter-1-正文')

    await readerStore.preloadNextChapter(1)
    await readerStore.loadChapter(1)

    expect(readerStore.content).toBe('chapter-1-正文')
  })

  it('shows the loading state after the delay when the preload is slow', async () => {
    const appStore = useAppStore()
    const readerStore = useReaderStore()
    appStore.setOnlineStatus(true)
    readerStore.updateConfig('enablePreload', true)
    readerStore.book = {
      name: '测试书籍',
      author: '测试作者',
      origin: 'test-source',
      bookUrl: 'https://example.test/inflight-slow',
    }
    readerStore.chapters = [
      { title: '第一章', url: 'chapter-0', index: 0 },
      { title: '第二章', url: 'chapter-1', index: 1 },
    ]

    let resolveContent: ((value: string) => void) | undefined
    vi.mocked(getBookContent).mockImplementation(
      async () =>
        new Promise<string>((resolve) => {
          resolveContent = resolve
        }),
    )

    const preload = readerStore.preloadNextChapter(1)
    await vi.waitFor(() => {
      expect(getBookContent).toHaveBeenCalledTimes(1)
    })

    const loading = readerStore.loadChapter(1)
    // 立刻不亮, 免得命中的预载闪一下
    expect(readerStore.loading).toBe(false)
    // 但书源确实慢: 延迟过后必须给反馈, 否则用户以为翻页没反应
    await vi.waitFor(() => {
      expect(readerStore.loading).toBe(true)
    })

    resolveContent?.('chapter-1-正文')
    await loading
    await preload
    expect(readerStore.content).toBe('chapter-1-正文')
    expect(readerStore.loading).toBe(false)
  })

  it('caches content fetched outside loadChapter so it is not requested twice', async () => {
    const appStore = useAppStore()
    const readerStore = useReaderStore()
    appStore.setOnlineStatus(true)
    readerStore.updateConfig('enablePreload', true)
    readerStore.book = {
      name: '测试书籍',
      author: '测试作者',
      origin: 'test-source',
      bookUrl: 'https://example.test/cache-outside',
    }
    readerStore.chapters = [{ title: '第一章', url: 'chapter-0', index: 0 }]
    vi.mocked(getBookContent).mockResolvedValue('chapter-0-正文')

    // 连续阅读经 buildContinuousChapter 走这条路径取正文, 从不经过 loadChapter。
    // 写缓存若不在这里发生, 同一章每次滚回视口都要重新请求一次。
    await readerStore.fetchChapterContent(0)
    await readerStore.fetchChapterContent(0)

    expect(getBookContent).toHaveBeenCalledTimes(1)
  })

  it('clears a pending load stage when the reader is reset', async () => {
    const appStore = useAppStore()
    const readerStore = useReaderStore()
    appStore.setOnlineStatus(true)
    readerStore.book = {
      name: '测试书籍',
      author: '测试作者',
      origin: 'test-source',
      bookUrl: 'https://example.test/stage-clear',
    }
    readerStore.chapters = [{ title: '第一章', url: 'chapter-0', index: 0 }]

    // 请求不 settle：模拟加载中切书/离开阅读页，原请求的 onStage 之后仍可能回来
    let emitStage: ((stage: ContentLoadStage) => void) | undefined
    vi.mocked(getBookContent).mockImplementation(
      async (params) =>
        new Promise<string>(() => {
          emitStage = params.onStage
        }),
    )

    void readerStore.loadChapter(0)
    await vi.waitFor(() => {
      expect(readerStore.loadStage).toBe('fetching')
    })

    readerStore.clear()
    expect(readerStore.loadStage).toBe('')
    // 必须确实发出过请求，否则下面的断言会因为 emitStage 为空而假绿
    expect(emitStage).toBeTypeOf('function')

    // 旧请求在被作废后仍推阶段：不能把提示重新点亮，否则重进阅读页时浮层卡住。
    // 这里必须同步检查——阶段提示是同步赋值，若持有者没被作废会立刻变成 parsing。
    emitStage?.('parsing')
    expect(readerStore.loadStage).toBe('')
  })

  it('keeps the app appearance unchanged when selecting a reading theme', () => {
    const appStore = useAppStore()
    appStore.setTheme('dark')
    const readerStore = useReaderStore()

    readerStore.setThemeIndex(2)

    expect(appStore.theme).toBe('dark')
    expect(readerStore.isNight).toBe(false)
    expect(readerStore.currentTheme).toEqual({
      name: '琥珀',
      body: '#f5e6ce',
      content: '#faf0e4',
      fontColor: '#5b4636',
      popup: '#faf0e4',
    })
  })

  it('reports unavailable system speech instead of failing silently', () => {
    const appStore = useAppStore()
    const readerStore = useReaderStore()
    const onError = vi.fn()

    readerStore.startTTS('测试朗读', { onError })

    expect(readerStore.systemSpeechSupported).toBe(false)
    expect(onError).toHaveBeenCalledWith(expect.any(Error))
    expect(appStore.toasts.at(-1)?.message).toContain('改用 API 语音')
  })

  it('returns to the previous daytime theme after leaving the night theme', () => {
    const readerStore = useReaderStore()
    readerStore.setThemeIndex(2)

    readerStore.setThemeIndex(nightThemeIndex)

    expect(readerStore.isNight).toBe(true)
    expect(readerStore.themeIndex).toBe(2)
    expect(readerStore.currentTheme.name).toBe('暗夜')

    readerStore.toggleNight()

    expect(readerStore.isNight).toBe(false)
    expect(readerStore.themeIndex).toBe(2)
    expect(readerStore.currentTheme.name).toBe('琥珀')
  })

  it('persists and removes a custom reading background', () => {
    const readerStore = useReaderStore()
    const image = 'http://reader.localhost/files?path=default/backgrounds/desktop-abc.png'

    readerStore.setBackgroundImage(image)
    readerStore.updateConfig('backgroundOpacity', 0.6)
    // 面板底板默认实色: 有了背景图也不会自动变半透明, 由「面板不透明度」决定。
    expect(readerStore.chromeTheme.popup).toBe('#fff')
    readerStore.updateConfig('panelOpacity', 0.84)
    expect(readerStore.chromeTheme.popup).toBe('color-mix(in srgb, #fff 84%, transparent)')
    readerStore.updateConfig('panelOpacity', 1)
    readerStore.updateConfig('applyBackgroundToReader', false)

    expect(readerStore.config.backgroundImage).toBe(image)
    expect(readerStore.config.backgroundOpacity).toBe(0.6)
    expect(readerStore.config.applyBackgroundToReader).toBe(false)
    expect(readerStore.chromeTheme.popup).toBe('#fff')
    expect(localStorage.setItem).toHaveBeenCalledWith(
      'reader-background-image',
      image,
    )

    readerStore.updateConfig('fontSize', 30)
    readerStore.resetConfig()
    expect(readerStore.config.fontSize).toBe(18)
    expect(readerStore.config.backgroundImage).toBe(image)
    expect(readerStore.config.backgroundOpacity).toBe(0.6)
    expect(readerStore.config.applyBackgroundToReader).toBe(false)

    readerStore.clearBackgroundImage()
    expect(readerStore.config.backgroundImage).toBe('')
    expect(localStorage.removeItem).toHaveBeenCalledWith('reader-background-image')
  })

  it('persists the click-area grid and keeps it across a layout reset', () => {
    const readerStore = useReaderStore()
    // 默认是「默认三分区」：老用户升级后手感不变。
    expect(readerStore.config.clickAreaMode).toBe('auto')
    expect(readerStore.config.clickAreas.mc).toBe('menu')

    const custom = { ...readerStore.config.clickAreas, tl: 'bookmark' as const, br: 'tts' as const }
    readerStore.updateConfig('clickAreas', custom)
    readerStore.updateConfig('clickAreaMode', 'custom')

    // 落盘的是普通 JSON，不带响应式代理。
    const saved = JSON.parse(vi.mocked(localStorage.setItem).mock.calls.at(-1)![1] as string)
    expect(saved.clickAreaMode).toBe('custom')
    expect(saved.clickAreas.tl).toBe('bookmark')
    expect(saved.clickAreas.br).toBe('tts')

    // 重置排版参数不该把用户配好的点击区域打回默认。
    readerStore.updateConfig('fontSize', 30)
    readerStore.resetConfig()
    expect(readerStore.config.fontSize).toBe(18)
    expect(readerStore.config.clickAreaMode).toBe('custom')
    expect(readerStore.config.clickAreas.tl).toBe('bookmark')
    expect(readerStore.config.clickAreas.br).toBe('tts')
    // 深拷过：改 config 里的对象不会改到之前那份。
    expect(custom.tl).toBe('bookmark')
  })

  it('updates the open-position snapshot when accepting newer Legado progress', async () => {
    const appStore = useAppStore()
    const readerStore = useReaderStore()
    appStore.setOnlineStatus(true)
    appStore.setLegadoWebdavConfig({
      url: 'https://dav.example.test/',
      account: 'reader',
      password: 'secret',
      directory: 'legado',
    })
    // loadBook 快照的旧入口位置(第0章开头)
    readerStore.book = {
      name: '测试书籍',
      author: '测试作者',
      origin: 'test-source',
      bookUrl: 'https://example.test/book',
      durChapterIndex: 0,
      durChapterPos: 0,
      durChapterTime: 1000,
    }
    readerStore.chapters = [
      { title: '第一章', url: 'chapter-0', index: 0 },
      { title: '第二章', url: 'chapter-1', index: 1 },
    ]
    readerStore.content = '旧章节正文'
    vi.mocked(syncLegadoBookProgress).mockResolvedValue({
      configured: true,
      uploaded: false,
      remote: {
        name: '测试书籍',
        author: '测试作者',
        durChapterIndex: 1,
        durChapterPos: 4,
        durChapterTime: 123456,
        durChapterTitle: '第二章',
      },
    })
    vi.mocked(getBookContent).mockResolvedValue('12345678')

    await readerStore.restoreCurrentBookProgressFromLegado()

    // 快照必须跟着云端进度走(用云端原始字数位置), 否则 loadSavedReadingPosition 会丢弃远端位置
    expect(readerStore.openPosition).toEqual({ index: 1, position: 5000, time: 123456 })
  })

  it('prefers a later remote chapter even when its timestamp is older', async () => {
    const appStore = useAppStore()
    const readerStore = useReaderStore()
    appStore.setOnlineStatus(true)
    appStore.setLegadoWebdavConfig({
      url: 'https://dav.example.test/',
      account: 'reader',
      password: 'secret',
      directory: 'legado',
    })
    const initialTime = 1_700_000_000_000
    const book = {
      name: '测试书籍',
      author: '测试作者',
      origin: 'test-source',
      bookUrl: 'https://example.test/book',
      durChapterIndex: 0,
      durChapterPos: 0,
      durChapterTime: initialTime,
    }
    vi.mocked(getChapterList).mockResolvedValue([
      { title: '第一章', url: 'chapter-0', index: 0 },
      { title: '第二章', url: 'chapter-1', index: 1 },
    ])
    vi.mocked(getBookContent).mockResolvedValue('12345678')
    vi.mocked(syncLegadoBookProgress).mockResolvedValue({
      configured: true,
      uploaded: false,
      remote: {
        name: '测试书籍',
        author: '测试作者',
        durChapterIndex: 1,
        durChapterPos: 4,
        durChapterTime: initialTime - 1000,
        durChapterTitle: '第二章',
      },
    })

    await readerStore.loadBook(book)

    expect(syncLegadoBookProgress).toHaveBeenCalledWith(
      appStore.legadoWebdavConfig,
      expect.objectContaining({ durChapterIndex: 0 }),
      true,
    )
    expect(readerStore.currentIndex).toBe(1)
    expect(readerStore.chapterScrollProgress).toBe(0.5)
    expect(readerStore.book?.durChapterPos).toBe(5000)
  })

  it('still loads the local chapter when Legado sync is disabled', async () => {
    const appStore = useAppStore()
    const readerStore = useReaderStore()
    appStore.setLegadoSyncEnabled(false)
    const book = {
      name: '测试书籍',
      author: '测试作者',
      origin: 'test-source',
      bookUrl: 'https://example.test/book',
      durChapterIndex: 0,
      durChapterPos: 0,
      durChapterTime: 1_700_000_000_000,
    }
    vi.mocked(getChapterList).mockResolvedValue([
      { title: '第一章', url: 'chapter-0', index: 0 },
    ])
    vi.mocked(getBookContent).mockResolvedValue('本地章节正文')

    await readerStore.loadBook(book)

    expect(readerStore.content).toBe('本地章节正文')
    expect(syncLegadoBookProgress).not.toHaveBeenCalled()
  })

  it('prefers the local chapter position over a zeroed shelf position on reopen', async () => {
    const appStore = useAppStore()
    const readerStore = useReaderStore()
    appStore.setLegadoSyncEnabled(false)
    const book = {
      name: '测试书籍',
      author: '测试作者',
      origin: 'test-source',
      bookUrl: 'https://example.test/local-position',
      durChapterIndex: 0,
      durChapterPos: 0,
      durChapterTime: 1_700_000_000_000,
    }
    localStorage.setItem(
      `reader-position:${book.bookUrl}`,
      JSON.stringify({
        chapterIndex: 0,
        progress: 0.72,
        updatedAt: 1_700_000_100_000,
      }),
    )
    vi.mocked(getChapterList).mockResolvedValue([
      { title: '第一章', url: 'chapter-0', index: 0 },
    ])
    vi.mocked(getBookContent).mockResolvedValue('本地章节正文')

    await readerStore.loadBook(book)

    expect(readerStore.chapterScrollProgress).toBe(0.72)
    const savedSession = vi.mocked(localStorage.setItem).mock.calls
      .filter(([key]) => key === 'reader-last-session')
      .at(-1)
    expect(savedSession?.[1]).toContain('"chapterScrollProgress":0.72')
  })

  it('uses the saved chapter when restoring an offline reader session', async () => {
    const appStore = useAppStore()
    const readerStore = useReaderStore()
    appStore.setOnlineStatus(false)
    const savedBook = {
      name: '测试书籍',
      author: '测试作者',
      origin: 'test-source',
      bookUrl: 'https://example.test/book',
      durChapterIndex: 1,
      durChapterPos: 5000,
      durChapterTime: 123456,
    }
    localStorage.setItem('reader-last-session', JSON.stringify({
      book: savedBook,
      chapters: [
        { title: '第一章', url: 'chapter-0', index: 0 },
        { title: '第二章', url: 'chapter-1', index: 1 },
      ],
      currentIndex: 1,
      chapterScrollProgress: 0.5,
      updatedAt: 123456,
    }))
    vi.mocked(getBookContent).mockResolvedValue('离线正文')

    await expect(readerStore.restorePersistedSession()).resolves.toBe(true)

    expect(readerStore.currentIndex).toBe(1)
    expect(readerStore.content).toBe('离线正文')
    expect(readerStore.chapterScrollProgress).toBe(0.5)
    expect(syncLegadoBookProgress).not.toHaveBeenCalled()
  })

  it('restores the newer Legado chapter and position before reading', async () => {
    const appStore = useAppStore()
    const readerStore = useReaderStore()
    appStore.setOnlineStatus(true)
    appStore.setLegadoWebdavConfig({
      url: 'https://dav.example.test/',
      account: 'reader',
      password: 'secret',
      directory: 'legado',
    })
    readerStore.book = {
      name: '测试书籍',
      author: '测试作者',
      origin: 'test-source',
      bookUrl: 'https://example.test/book',
      durChapterIndex: 0,
      durChapterPos: 0,
    }
    readerStore.chapters = [
      { title: '第一章', url: 'chapter-0', index: 0 },
      { title: '第二章', url: 'chapter-1', index: 1 },
    ]
    readerStore.content = '旧章节正文'
    vi.mocked(syncLegadoBookProgress).mockResolvedValue({
      configured: true,
      uploaded: false,
      remote: {
        name: '测试书籍',
        author: '测试作者',
        durChapterIndex: 1,
        durChapterPos: 4,
        durChapterTime: 123456,
        durChapterTitle: '第二章',
      },
    })
    vi.mocked(getBookContent).mockResolvedValue('12345678')

    await readerStore.restoreCurrentBookProgressFromLegado()

    expect(readerStore.currentIndex).toBe(1)
    expect(readerStore.chapterScrollProgress).toBe(0.5)
    expect(syncLegadoBookProgress).toHaveBeenCalledWith(
      appStore.legadoWebdavConfig,
      expect.objectContaining({ durChapterIndex: 0 }),
      true,
    )
  })

  it('converts remote character position before exposing the target chapter', async () => {
    const appStore = useAppStore()
    const readerStore = useReaderStore()
    appStore.setOnlineStatus(true)
    appStore.setLegadoWebdavConfig({
      url: 'https://dav.example.test/',
      account: 'reader',
      password: 'secret',
      directory: 'legado',
    })
    readerStore.book = {
      name: '测试书籍',
      author: '测试作者',
      origin: 'test-source',
      bookUrl: 'https://example.test/book',
      durChapterIndex: 0,
      durChapterPos: 0,
    }
    readerStore.chapters = [
      { title: '第一章', url: 'chapter-0', index: 0 },
      { title: '第二章', url: 'chapter-1', index: 1 },
    ]
    vi.mocked(syncLegadoBookProgress).mockResolvedValue({
      configured: true,
      uploaded: false,
      remote: {
        name: '测试书籍',
        author: '测试作者',
        durChapterIndex: 1,
        durChapterPos: 4,
        durChapterTime: 123456,
        durChapterTitle: '第二章',
      },
    })
    let resolveContent: (() => void) | undefined
    const contentGate = new Promise<void>((resolve) => {
      resolveContent = resolve
    })
    vi.mocked(getBookContent).mockImplementation(async () => {
      await contentGate
      return '12345678'
    })

    const restoreTask = readerStore.restoreCurrentBookProgressFromLegado()
    await Promise.resolve()
    await Promise.resolve()
    const stateWhileContentLoads = readerStore.restoringLegadoProgress
    if (!resolveContent) throw new Error('content gate was not created')
    resolveContent()
    await restoreTask

    expect(stateWhileContentLoads).toBe(true)
    expect(readerStore.restoringLegadoProgress).toBe(false)
    expect(readerStore.openPosition).toEqual({ index: 1, position: 5000, time: 123456 })
  })

  it('keeps the local progress when the phone points to an earlier chapter', async () => {
    const appStore = useAppStore()
    const readerStore = useReaderStore()
    appStore.setOnlineStatus(true)
    appStore.setLegadoWebdavConfig({
      url: 'https://dav.example.test/',
      account: 'reader',
      password: 'secret',
      directory: 'legado',
    })
    readerStore.book = {
      name: '测试书籍',
      author: '测试作者',
      origin: 'test-source',
      bookUrl: 'https://example.test/book',
      durChapterIndex: 2,
      durChapterPos: 5000,
      durChapterTime: 1_700_000_000_000,
    }
    readerStore.chapters = [
      { title: '第一百零三章', url: 'chapter-103', index: 0 },
      { title: '第一百零四章', url: 'chapter-104', index: 1 },
      { title: '第一百零五章', url: 'chapter-105', index: 2 },
    ]
    readerStore.currentIndex = 2
    readerStore.content = '12345678'
    readerStore.setChapterScrollProgress(0.5)
    vi.mocked(syncLegadoBookProgress).mockResolvedValue({
      configured: true,
      uploaded: false,
      remote: {
        name: '测试书籍',
        author: '测试作者',
        durChapterIndex: 0,
        durChapterPos: 2,
        durChapterTime: 1_700_000_100_000,
        durChapterTitle: '第一百零三章',
      },
    })
    vi.mocked(getBookContent).mockResolvedValue('12345678')

    await readerStore.restoreCurrentBookProgressFromLegado()

    expect(readerStore.currentIndex).toBe(2)
    expect(readerStore.chapterScrollProgress).toBe(0.5)
  })

  it('uploads directly on exit without comparing remote progress', async () => {
    const appStore = useAppStore()
    const readerStore = useReaderStore()
    appStore.setLegadoWebdavConfig({
      url: 'https://dav.example.test/',
      account: 'reader',
      password: 'secret',
      directory: 'legado',
    })
    readerStore.book = {
      name: '测试书籍',
      author: '测试作者',
      origin: 'test-source',
      bookUrl: 'https://example.test/book',
    }
    readerStore.chapters = [
      { title: '第一章', url: 'chapter-0', index: 0 },
    ]
    readerStore.content = '12345678'
    readerStore.setChapterScrollProgress(0.5)
    vi.mocked(uploadLegadoBookProgress).mockResolvedValue({
      configured: true,
      uploaded: true,
      remote: null,
    })

    await readerStore.syncCurrentBookProgressFromLegadoOnExit()

    expect(syncLegadoBookProgress).not.toHaveBeenCalled()
    expect(uploadLegadoBookProgress).toHaveBeenCalledWith(
      appStore.legadoWebdavConfig,
      expect.objectContaining({ durChapterIndex: 0, durChapterPos: 4 }),
    )
  })

  it('skips direct upload when Legado sync is unavailable on exit', async () => {
    const appStore = useAppStore()
    const readerStore = useReaderStore()
    appStore.setLegadoSyncEnabled(false)
    readerStore.book = {
      name: '测试书籍',
      author: '测试作者',
      origin: 'test-source',
      bookUrl: 'https://example.test/book',
    }

    await readerStore.syncCurrentBookProgressFromLegadoOnExit()

    expect(syncLegadoBookProgress).not.toHaveBeenCalled()
    expect(uploadLegadoBookProgress).not.toHaveBeenCalled()
  })

  it('skips Legado sync while the chapter is still loading', async () => {
    const appStore = useAppStore()
    const readerStore = useReaderStore()
    appStore.setLegadoWebdavConfig({
      url: 'https://dav.example.test/',
      account: 'reader',
      password: 'secret',
      directory: 'legado',
    })
    readerStore.book = {
      name: '测试书籍',
      author: '测试作者',
      origin: 'test-source',
      bookUrl: 'https://example.test/book',
    }
    readerStore.loading = true

    await readerStore.syncCurrentBookProgressFromLegadoOnExit()

    expect(syncLegadoBookProgress).not.toHaveBeenCalled()
    expect(uploadLegadoBookProgress).not.toHaveBeenCalled()
  })

  it('keeps API audio active and resumes from the paused position', async () => {
    const audioInstances: MockAudio[] = []

    class MockAudio {
      src: string
      paused = true
      ended = false
      duration = 10
      currentTime = 0
      onplay: (() => void) | null = null
      onpause: (() => void) | null = null
      onloadedmetadata: (() => void) | null = null
      ontimeupdate: (() => void) | null = null
      onended: (() => void) | null = null
      onerror: (() => void) | null = null

      constructor(src: string) {
        this.src = src
        audioInstances.push(this)
      }

      async play() {
        this.paused = false
        this.onplay?.()
      }

      pause() {
        this.paused = true
        this.onpause?.()
      }
    }

    vi.stubGlobal('Audio', MockAudio)
    // 注意: 不能把 URL 整个换成普通对象 —— Vite 的模块加载器在 import 新模块时
    // 会 `new URL(...)` 解析路径, 被替换掉之后同 worker 里后续的动态 import 全部
    // 抛 "URL is not a constructor"(表现为简繁转换器静默失效)。这里继承真正的
    // URL, 只覆盖两个静态方法。
    class MockURL extends URL {
      static createObjectURL = vi.fn(() => 'blob:test-audio')
      static revokeObjectURL = vi.fn()
    }
    vi.stubGlobal('URL', MockURL)
    vi.mocked(requestOpenAISpeechAudio).mockResolvedValue(new Blob(['audio']))

    const readerStore = useReaderStore()
    readerStore.setSpeechProvider('openai')
    readerStore.startTTS('测试朗读')

    await vi.waitFor(() => expect(readerStore.isSpeaking).toBe(true))
    const audio = audioInstances[0]
    audio.currentTime = 4
    audio.ontimeupdate?.()
    expect(readerStore.speechProgress).toBeCloseTo(0.4)

    readerStore.pauseTTS()
    expect(readerStore.isSpeaking).toBe(true)
    expect(readerStore.isPaused).toBe(true)
    expect(audio.currentTime).toBe(4)

    readerStore.pauseTTS()
    await vi.waitFor(() => expect(readerStore.isPaused).toBe(false))
    expect(readerStore.isSpeaking).toBe(true)
    expect(audio.currentTime).toBe(4)

    readerStore.stopTTS()
  })
})

describe('reader content processing order', () => {
  beforeEach(() => {
    // 前面的用例会 stub 掉全局 URL/Audio 而不还原; 这里要动态 import 词典模块,
    // 残留的 URL 桩会让模块加载抛错, 转换器因此静默失效 —— 先清干净。
    vi.unstubAllGlobals()
    setActivePinia(createPinia())
    const storage = new Map<string, string>()
    vi.stubGlobal('localStorage', {
      getItem: vi.fn((key: string) => storage.get(key) ?? null),
      setItem: vi.fn((key: string, value: string) => storage.set(key, value)),
      removeItem: vi.fn((key: string) => storage.delete(key)),
      clear: vi.fn(() => storage.clear()),
    })
  })

  afterEach(() => {
    vi.unstubAllGlobals()
  })

  it('先简繁转换, 再应用替换规则, 避免净化结果被词表覆盖', async () => {
    const readerStore = useReaderStore()
    readerStore.book = {
      name: '诡秘之主',
      author: '爱潜水的乌贼',
      origin: 'test-source',
      bookUrl: 'https://example.test/book',
    }
    readerStore.config.chineseMode = 'simplified'
    readerStore.replaceRules = [
      {
        id: 1,
        name: '修正序列',
        pattern: '串行',
        replacement: '序列',
        scope: '*',
        isEnabled: true,
        isRegex: false,
        order: 1,
      },
    ]

    // 等 watcher 中的动态导入完成, 否则转换器尚未就绪。
    await new Promise((resolve) => setTimeout(resolve, 0))

    expect(readerStore.processContentForDisplay('序列')).toBe('序列')
    expect(readerStore.processContentForDisplay('串行')).toBe('序列')
  })

  it('简体档不再改写「乾國」这类词(issue #25 的回归)', async () => {
    const readerStore = useReaderStore()
    readerStore.book = {
      name: '乾国纪事',
      author: '作者',
      origin: 'test-source',
      bookUrl: 'https://example.test/book',
    }
    readerStore.config.chineseMode = 'simplified'
    // 转换器是动态 import 的('chinese-convert' 单独分块, 1MB+ 词表), 首次
    // 加载/转换在并行跑全量测试时要几秒 —— 先显式把模块拉进来(watcher 里的
    // 动态 import 走同一份模块缓存), 再等转换器挂上, 不要赌 setTimeout(0)。
    await vi.waitFor(() => {
      expect(readerStore.processContentForDisplay('乾國')).toBe('乾国')
    }, { timeout: 10_000 })

    // 手机端 legado 显示「乾国」, 阅读器必须一致 —— 曾经这里是「干国」。
    expect(readerStore.processContentForDisplay('乾國')).toBe('乾国')
    // 该转的仍然要转。
    expect(readerStore.processContentForDisplay('乾燥')).toBe('干燥')
    expect(readerStore.processContentForDisplay('乾隆')).toBe('乾隆')
  })

  it('「关闭」档原样输出正文, 不做任何简繁转换', async () => {
    const readerStore = useReaderStore()
    readerStore.book = {
      name: '繁简测试',
      author: '作者',
      origin: 'test-source',
      bookUrl: 'https://example.test/book',
    }
    readerStore.config.chineseMode = 'off'
    await new Promise((resolve) => setTimeout(resolve, 0))

    expect(readerStore.processContentForDisplay('乾國')).toBe('乾國')
    expect(readerStore.processContentForDisplay('乾燥')).toBe('乾燥')
    expect(readerStore.processContentForDisplay('这是简体')).toBe('这是简体')
  })
})

describe('reader toc auto refresh', () => {
  afterEach(() => {
    vi.unstubAllGlobals()
    vi.useRealTimers()
  })

  beforeEach(() => {
    setActivePinia(createPinia())
    const storage = new Map<string, string>()
    vi.stubGlobal('localStorage', {
      getItem: vi.fn((key: string) => storage.get(key) ?? null),
      setItem: vi.fn((key: string, value: string) => storage.set(key, value)),
      removeItem: vi.fn((key: string) => storage.delete(key)),
      clear: vi.fn(() => storage.clear()),
    })
    vi.mocked(getChapterList).mockReset()
  })

  function setupRemoteBook(readerStore: ReturnType<typeof useReaderStore>) {
    readerStore.book = {
      name: '连载书',
      author: '测试作者',
      origin: 'test-source',
      bookUrl: 'https://example.test/book',
    }
    readerStore.chapters = [{ title: '第一章', url: 'chapter-0', index: 0 }]
  }

  it('appends new chapters and toasts when the source toc grows', async () => {
    vi.useFakeTimers()
    const appStore = useAppStore()
    const readerStore = useReaderStore()
    appStore.setOnlineStatus(true)
    setupRemoteBook(readerStore)
    const fullList = [
      { title: '第一章', url: 'chapter-0', index: 0 },
      { title: '第二章', url: 'chapter-1', index: 1 },
      { title: '第三章', url: 'chapter-2', index: 2 },
    ]
    vi.mocked(getChapterList).mockResolvedValue(fullList)

    const task = readerStore.refreshTocFromSource()
    await vi.advanceTimersByTimeAsync(1200)
    await expect(task).resolves.toBe(2)
    expect(readerStore.chapters).toHaveLength(3)
    expect(appStore.toasts.some((toast) => toast.message.includes('目录已更新'))).toBe(true)
  })

  it('keeps the old toc when the source reorders existing chapters', async () => {
    vi.useFakeTimers()
    const appStore = useAppStore()
    const readerStore = useReaderStore()
    appStore.setOnlineStatus(true)
    setupRemoteBook(readerStore)
    vi.mocked(getChapterList).mockResolvedValue([
      { title: '重排第一章', url: 'renumbered-0', index: 0 },
      { title: '重排第二章', url: 'renumbered-1', index: 1 },
    ])

    const task = readerStore.refreshTocFromSource()
    await vi.advanceTimersByTimeAsync(1200)
    await expect(task).resolves.toBe(0)
    expect(readerStore.chapters).toHaveLength(1)
    expect(readerStore.chapters[0].url).toBe('chapter-0')
  })

  it('throttles repeated checks per book', async () => {
    vi.useFakeTimers()
    const appStore = useAppStore()
    const readerStore = useReaderStore()
    appStore.setOnlineStatus(true)
    setupRemoteBook(readerStore)
    vi.mocked(getChapterList).mockResolvedValue([
      { title: '第一章', url: 'chapter-0', index: 0 },
    ])

    const first = readerStore.refreshTocFromSource()
    await vi.advanceTimersByTimeAsync(1200)
    await expect(first).resolves.toBe(0)
    const callsAfterFirst = vi.mocked(getChapterList).mock.calls.length

    const second = readerStore.refreshTocFromSource()
    await expect(second).resolves.toBe(0)
    expect(vi.mocked(getChapterList).mock.calls.length).toBe(callsAfterFirst)
  })

  it('skips local books entirely', async () => {
    const appStore = useAppStore()
    const readerStore = useReaderStore()
    appStore.setOnlineStatus(true)
    readerStore.book = {
      name: '本地书',
      author: '本地导入',
      origin: 'local-txt',
      bookUrl: 'local-txt:abc123',
    }
    readerStore.chapters = [{ title: '第一章', url: 'local-txt:abc123#0', index: 0 }]

    await expect(readerStore.refreshTocFromSource()).resolves.toBe(0)
    expect(getChapterList).not.toHaveBeenCalled()
  })
})

describe('reader in-place chapter refresh', () => {
  afterEach(() => {
    vi.unstubAllGlobals()
  })

  beforeEach(() => {
    setActivePinia(createPinia())
    const storage = new Map<string, string>()
    vi.stubGlobal('localStorage', {
      getItem: vi.fn((key: string) => storage.get(key) ?? null),
      setItem: vi.fn((key: string, value: string) => storage.set(key, value)),
      removeItem: vi.fn((key: string) => storage.delete(key)),
      clear: vi.fn(() => storage.clear()),
    })
    vi.mocked(getBookContent).mockReset()
    vi.mocked(saveBookProgress).mockReset()
    vi.mocked(saveBookProgress).mockResolvedValue('ok')
  })

  function setupChapter(readerStore: ReturnType<typeof useReaderStore>, index: number) {
    readerStore.book = {
      name: '刷新书',
      author: '测试作者',
      origin: 'test-source',
      bookUrl: 'https://example.test/refresh',
    }
    readerStore.chapters = Array.from({ length: 12 }, (_, i) => ({
      title: `第${i + 1}章`,
      url: `chapter-${i}`,
      index: i,
    }))
    readerStore.setActiveChapterState(index, `chapter-${index}-旧正文`, 0.4)
  }

  it('keeps the current index and in-chapter progress when refreshing chapter 9', async () => {
    const appStore = useAppStore()
    const readerStore = useReaderStore()
    appStore.setOnlineStatus(true)
    // 用户场景: 读到第 9 章(index 8)中部, 右键菜单点刷新。
    setupChapter(readerStore, 8)
    vi.mocked(getBookContent).mockResolvedValue('chapter-8-新正文')

    await readerStore.refreshContent()

    expect(readerStore.currentIndex).toBe(8)
    expect(readerStore.content).toBe('chapter-8-新正文')
    expect(readerStore.chapterScrollProgress).toBeCloseTo(0.4, 5)
    expect(readerStore.refreshing).toBe(false)
    // 必须向书源强制重取, 否则拿到的是缓存旧正文。
    // 原地刷新属于用户可感知的等待, 因此会带上阶段回调(onStage)。
    expect(getBookContent).toHaveBeenCalledWith({
      bookUrl: 'https://example.test/refresh',
      chapterUrl: 'chapter-8',
      bookSourceUrl: 'test-source',
      chapterTitle: '第9章',
      refresh: 1,
      onStage: expect.any(Function),
    })
  })

  it('bumps contentRefreshToken so the reader can re-anchor the viewport', async () => {
    const appStore = useAppStore()
    const readerStore = useReaderStore()
    appStore.setOnlineStatus(true)
    setupChapter(readerStore, 8)
    vi.mocked(getBookContent).mockResolvedValue('chapter-8-新正文')
    const before = readerStore.contentRefreshToken

    await readerStore.refreshContent()

    expect(readerStore.contentRefreshToken).toBe(before + 1)
  })

  it('flags refreshing only while the request is in flight', async () => {
    const appStore = useAppStore()
    const readerStore = useReaderStore()
    appStore.setOnlineStatus(true)
    setupChapter(readerStore, 8)
    let release: (value: string) => void = () => undefined
    vi.mocked(getBookContent).mockImplementation(() => new Promise<string>((resolve) => {
      release = resolve
    }))

    const task = readerStore.refreshContent()
    expect(readerStore.refreshing).toBe(true)
    expect(readerStore.loading).toBe(true)

    release('chapter-8-新正文')
    await task

    expect(readerStore.refreshing).toBe(false)
    expect(readerStore.loading).toBe(false)
  })

  it('leaves the index untouched when the source refetch fails', async () => {
    const appStore = useAppStore()
    const readerStore = useReaderStore()
    appStore.setOnlineStatus(true)
    setupChapter(readerStore, 8)
    vi.mocked(getBookContent).mockRejectedValue(new Error('书源超时'))

    await expect(readerStore.refreshContent()).rejects.toThrow('书源超时')

    expect(readerStore.currentIndex).toBe(8)
    expect(readerStore.content).toBe('chapter-8-旧正文')
    expect(readerStore.refreshing).toBe(false)
  })
})

describe('reader custom colors', () => {
  afterEach(() => {
    vi.unstubAllGlobals()
  })

  beforeEach(() => {
    setActivePinia(createPinia())
    const storage = new Map<string, string>()
    vi.stubGlobal('localStorage', {
      getItem: vi.fn((key: string) => storage.get(key) ?? null),
      setItem: vi.fn((key: string, value: string) => storage.set(key, value)),
      removeItem: vi.fn((key: string) => storage.delete(key)),
      clear: vi.fn(() => storage.clear()),
    })
  })

  it('overrides only the reader page colors, leaving the theme presets untouched', () => {
    const readerStore = useReaderStore()
    readerStore.setThemeIndex(2)

    expect(readerStore.readerTheme).toEqual(readerStore.currentTheme)

    readerStore.updateConfig('readerBackgroundColor', '#112233')
    readerStore.updateConfig('fontColor', '#aabbcc')

    expect(readerStore.readerTheme.body).toBe('#112233')
    expect(readerStore.readerTheme.content).toBe('#112233')
    expect(readerStore.readerTheme.fontColor).toBe('#aabbcc')
    // 主题预设本身不受影响, 便于随时还原。
    expect(readerStore.currentTheme.body).toBe('#f5e6ce')
    expect(readerStore.currentTheme.fontColor).toBe('#5b4636')
  })

  it('falls back to the theme when a custom color is cleared', () => {
    const readerStore = useReaderStore()
    readerStore.setThemeIndex(1)

    readerStore.updateConfig('readerBackgroundColor', '#112233')
    readerStore.updateConfig('readerBackgroundColor', '')

    expect(readerStore.readerTheme.body).toBe('#ffffff')
  })

  it('ignores values that are not plain hex colors', () => {
    const readerStore = useReaderStore()
    readerStore.setThemeIndex(0)

    readerStore.updateConfig('readerBackgroundColor', 'url(evil.png)')
    readerStore.updateConfig('fontColor', 'expression(alert(1))')

    expect(readerStore.readerTheme.body).toBe('#f5ede4')
    expect(readerStore.readerTheme.fontColor).toBe('#333')
  })

  it('keeps custom colors across a config reset, like the background image', () => {
    const readerStore = useReaderStore()

    readerStore.updateConfig('readerBackgroundColor', '#112233')
    readerStore.updateConfig('fontSize', 30)
    readerStore.resetConfig()

    expect(readerStore.config.fontSize).toBe(18)
    expect(readerStore.config.readerBackgroundColor).toBe('#112233')
  })
})

describe('reader hotkeys', () => {
  afterEach(() => {
    vi.unstubAllGlobals()
  })

  beforeEach(() => {
    setActivePinia(createPinia())
    const storage = new Map<string, string>()
    vi.stubGlobal('localStorage', {
      getItem: vi.fn((key: string) => storage.get(key) ?? null),
      setItem: vi.fn((key: string, value: string) => storage.set(key, value)),
      removeItem: vi.fn((key: string) => storage.delete(key)),
      clear: vi.fn(() => storage.clear()),
    })
  })

  it('starts from the built-in default table', () => {
    const readerStore = useReaderStore()

    expect(readerStore.hotkeysAreDefault).toBe(true)
    expect(readerStore.hotkeyBindings.ArrowDown).toEqual(['nextPage'])
    expect(readerStore.hotkeyBindings.Escape).toEqual(['back'])
  })

  it('rebinds a single key and persists it', () => {
    const readerStore = useReaderStore()

    readerStore.setHotkeyBinding('ArrowDown', ['nextChapter'])

    expect(readerStore.hotkeyBindings.ArrowDown).toEqual(['nextChapter'])
    expect(readerStore.hotkeysAreDefault).toBe(false)
    expect(localStorage.setItem).toHaveBeenCalledWith(
      'reader-hotkeys',
      JSON.stringify(readerStore.hotkeyBindings),
    )
  })

  it('accepts multiple actions for one key and keeps their order', () => {
    const readerStore = useReaderStore()

    readerStore.setHotkeyBinding('Space', ['nextPage', 'chapterEnd'])

    expect(readerStore.hotkeyBindings.Space).toEqual(['nextPage', 'chapterEnd'])
  })

  it('lets a key be unbound entirely', () => {
    const readerStore = useReaderStore()

    readerStore.setHotkeyBinding('Home', [])

    expect(readerStore.hotkeyBindings.Home).toEqual([])
    expect(readerStore.hotkeysAreDefault).toBe(false)
  })

  it('drops unknown actions instead of storing them', () => {
    const readerStore = useReaderStore()

    readerStore.setHotkeyBinding('Home', ['chapterEnd', 'notAnAction' as never])

    expect(readerStore.hotkeyBindings.Home).toEqual(['chapterEnd'])
  })

  it('restores every default after a reset', () => {
    const readerStore = useReaderStore()
    readerStore.setHotkeyBinding('ArrowDown', [])
    readerStore.setHotkeyBinding('F11', ['nightMode'])

    readerStore.resetHotkeyBindings()

    expect(readerStore.hotkeysAreDefault).toBe(true)
    expect(readerStore.hotkeyBindings.ArrowDown).toEqual(['nextPage'])
    expect(readerStore.hotkeyBindings.F11).toEqual(['fullscreen'])
  })

  it('survives a corrupted storage payload', () => {
    localStorage.setItem('reader-hotkeys', '{not json')
    const readerStore = useReaderStore()

    expect(readerStore.hotkeysAreDefault).toBe(true)
  })

  it('only touches the key being rebound', () => {
    const readerStore = useReaderStore()

    readerStore.setHotkeyBinding('ArrowDown', ['nextChapter'])

    expect(readerStore.hotkeyBindings.ArrowUp).toEqual(['prevPage'])
    expect(readerStore.hotkeyBindings.Escape).toEqual(['back'])
    expect(readerStore.hotkeyBindings.F11).toEqual(['fullscreen'])
  })
})

describe('reader config storage hardening', () => {
  afterEach(() => {
    vi.unstubAllGlobals()
  })

  function stubStorage(seed: Record<string, string> = {}) {
    const storage = new Map<string, string>(Object.entries(seed))
    vi.stubGlobal('localStorage', {
      getItem: vi.fn((key: string) => storage.get(key) ?? null),
      setItem: vi.fn((key: string, value: string) => storage.set(key, value)),
      removeItem: vi.fn((key: string) => storage.delete(key)),
      clear: vi.fn(() => storage.clear()),
    })
  }

  beforeEach(() => {
    setActivePinia(createPinia())
  })

  it('drops an injected non-hex reader colour from storage', () => {
    // readConfig 来自 localStorage, 收紧必须在进 style 之前完成。
    stubStorage({
      readConfig: JSON.stringify({
        fontSize: 20,
        readerBackgroundColor: 'url(javascript:alert(1))',
        fontColor: 'red',
      }),
    })
    const readerStore = useReaderStore()

    expect(readerStore.config.fontSize).toBe(20)
    expect(readerStore.config.readerBackgroundColor).toBe('')
    expect(readerStore.config.fontColor).toBe('')
    expect(readerStore.readerTheme.body).toBe('#f5ede4')
  })

  it('expands valid shorthand hex colours from storage', () => {
    stubStorage({
      readConfig: JSON.stringify({ readerBackgroundColor: '#ABC', fontColor: '#123456' }),
    })
    const readerStore = useReaderStore()

    expect(readerStore.config.readerBackgroundColor).toBe('#aabbcc')
    expect(readerStore.config.fontColor).toBe('#123456')
    expect(readerStore.readerTheme.body).toBe('#aabbcc')
    expect(readerStore.readerTheme.fontColor).toBe('#123456')
  })

  it('defaults the hover reveal switch to off for pre-existing storage', () => {
    // issue #7 问题 1: 悬停呼出会吃掉边缘正文的点击, 默认关闭。
    // 老用户存档里没有这个字段, 升级后同样应该是关闭的。
    stubStorage({ readConfig: JSON.stringify({ fontSize: 20 }) })

    expect(useReaderStore().config.hoverRevealControls).toBe(false)
  })

  it('falls back to off when the stored hover switch is not a boolean', () => {
    stubStorage({ readConfig: JSON.stringify({ hoverRevealControls: 'yes' }) })

    expect(useReaderStore().config.hoverRevealControls).toBe(false)
  })

  it('restores an explicitly enabled hover switch from storage', () => {
    stubStorage({ readConfig: JSON.stringify({ hoverRevealControls: true }) })

    expect(useReaderStore().config.hoverRevealControls).toBe(true)
  })
})

describe('reader page background image', () => {
  afterEach(() => {
    vi.unstubAllGlobals()
  })

  function stubStorage(seed: Record<string, string> = {}) {
    const storage = new Map<string, string>(Object.entries(seed))
    vi.stubGlobal('localStorage', {
      getItem: vi.fn((key: string) => storage.get(key) ?? null),
      setItem: vi.fn((key: string, value: string) => storage.set(key, value)),
      removeItem: vi.fn((key: string) => storage.delete(key)),
      clear: vi.fn(() => storage.clear()),
    })
  }

  beforeEach(() => {
    setActivePinia(createPinia())
    stubStorage()
  })

  it('stores the reader image separately from the desktop image', () => {
    const readerStore = useReaderStore()
    const desktop = 'http://reader.localhost/files?path=default/backgrounds/desktop-aaaa.png'
    const reader = 'http://reader.localhost/files?path=default/backgrounds/reader-bbbb.png'

    readerStore.setBackgroundImage(desktop)
    readerStore.setReaderBackgroundImage(reader)

    expect(readerStore.config.backgroundImage).toBe(desktop)
    expect(readerStore.config.readerBackgroundImage).toBe(reader)
    // 两个 key 各自独立, 互不覆盖。
    expect(localStorage.setItem).toHaveBeenCalledWith('reader-background-image', desktop)
    expect(localStorage.setItem).toHaveBeenCalledWith('reader-page-background-image', reader)
  })

  it('clears only the reader image', () => {
    const readerStore = useReaderStore()
    const desktop = 'http://reader.localhost/files?path=default/backgrounds/desktop-aaaa.png'

    readerStore.setBackgroundImage(desktop)
    readerStore.setReaderBackgroundImage('http://reader.localhost/files?path=default/backgrounds/reader-bbbb.png')
    readerStore.clearReaderBackgroundImage()

    expect(readerStore.config.readerBackgroundImage).toBe('')
    expect(readerStore.config.backgroundImage).toBe(desktop)
    expect(localStorage.removeItem).toHaveBeenCalledWith('reader-page-background-image')
  })

  /** 换图/清空要把上一张的落盘文件删掉, 否则 storage 里会越积越多孤儿图片。 */
  it('deletes the previous background file when replacing or clearing', async () => {
    const readerStore = useReaderStore()
    const { deleteBackgroundImage } = await import('../api/background')
    // mock 是模块级的, 前面的用例也会换图 —— 先清掉它们的调用记录。
    vi.mocked(deleteBackgroundImage).mockClear()
    const first = 'http://reader.localhost/files?path=default/backgrounds/desktop-aaaa.png'
    const second = 'http://reader.localhost/files?path=default/backgrounds/desktop-bbbb.png'

    readerStore.setBackgroundImage(first)
    readerStore.setBackgroundImage(second)
    // 同一张图重复设置不该删掉正在用的文件。
    readerStore.setBackgroundImage(second)
    expect(deleteBackgroundImage).toHaveBeenCalledTimes(1)
    expect(deleteBackgroundImage).toHaveBeenCalledWith(first)

    readerStore.clearBackgroundImage()
    expect(deleteBackgroundImage).toHaveBeenCalledTimes(2)
    expect(deleteBackgroundImage).toHaveBeenLastCalledWith(second)
  })

  it('keeps both images out of the persisted readConfig payload', () => {
    const readerStore = useReaderStore()
    const image = 'data:image/webp;base64,dGVzdA=='

    readerStore.setBackgroundImage(image)
    readerStore.setReaderBackgroundImage(image)
    // 两张图各自存独立 key; saveConfig 是排版参数的持久化入口。
    readerStore.updateConfig('fontSize', 20)

    const payloads = vi.mocked(localStorage.setItem).mock.calls
      .filter(([key]) => key === 'readConfig')
      .map(([, value]) => JSON.parse(value as string) as Record<string, unknown>)

    expect(payloads.length).toBeGreaterThan(0)
    payloads.forEach((payload) => {
      expect(payload).not.toHaveProperty('backgroundImage')
      expect(payload).not.toHaveProperty('readerBackgroundImage')
    })
  })

  it('normalises an out-of-range opacity from storage', () => {
    stubStorage({ readConfig: JSON.stringify({ readerImageOpacity: 42 }) })
    const readerStore = useReaderStore()

    expect(readerStore.config.readerImageOpacity).toBe(1)
  })

  it('keeps the reader image across a config reset', () => {
    const readerStore = useReaderStore()

    readerStore.setReaderBackgroundImage('data:image/webp;base64,dGVzdA==')
    readerStore.updateConfig('fontSize', 30)
    readerStore.resetConfig()

    expect(readerStore.config.fontSize).toBe(18)
    expect(readerStore.config.readerBackgroundImage).toBe('data:image/webp;base64,dGVzdA==')
  })

  /* ─── 滚动进度的持久化成本 ───
   * 回归测试: setChapterScrollProgress 由 handleScroll 在每次滚动事件里调用,
   * 曾直接走 saveReaderSession —— 后者每次都把整个 chapters 数组序列化并写盘。
   * 3000 章时实测单次 ~10ms, 一次翻页补间会因此阻塞主线程上百毫秒, 表现为掉帧
   * 与上一页残留。这里锁住「高频路径不再写全量快照」这一行为。 */

  /** 造一份足够大的目录, 让「是否序列化了目录」在体积上可见。 */
  function bigChapters(count: number) {
    return Array.from({ length: count }, (_, i) => ({
      title: `第${i + 1}章 一个典型的章节标题`,
      url: `https://example.com/book/1/chapter/${i}`,
      index: i,
    }))
  }

  function sessionWrites() {
    return vi.mocked(localStorage.setItem).mock.calls.filter(([key]) => key === 'reader-last-session')
  }

  function progressWrites() {
    return vi.mocked(localStorage.setItem).mock.calls.filter(([key]) => key === 'reader-last-progress')
  }

  it('does not rewrite the whole chapter list on every scroll progress update', () => {
    const readerStore = useReaderStore()
    readerStore.book = { name: '大书', author: '作者', origin: 'https://example.com', bookUrl: 'https://example.com/book/1' }
    readerStore.chapters = bigChapters(300)
    // 换章路径会写一次全量快照, 用来建立基线
    readerStore.setActiveChapterState(0, '正文', 0)
    const sessionWritesBeforeScroll = sessionWrites().length

    // 模拟一次翻页补间产生的多次滚动事件
    for (let i = 0; i < 30; i++) {
      readerStore.setChapterScrollProgress(i / 30)
    }

    expect(sessionWrites().length).toBe(sessionWritesBeforeScroll)
    expect(progressWrites().length).toBeGreaterThanOrEqual(30)
  })

  it('writes only the small progress payload on the hot path', () => {
    const readerStore = useReaderStore()
    readerStore.book = { name: '大书', author: '作者', origin: 'https://example.com', bookUrl: 'https://example.com/book/1' }
    readerStore.chapters = bigChapters(300)

    readerStore.setChapterScrollProgress(0.42)

    const [, value] = progressWrites().at(-1)!
    const payload = JSON.parse(value as string) as Record<string, unknown>
    expect(payload.currentIndex).toBe(0)
    expect(payload.chapterScrollProgress).toBe(0.42)
    expect(payload.bookUrl).toBe('https://example.com/book/1')
    // 关键: 轻量快照里不能出现目录 —— 它正是被优化的那部分。
    expect(payload).not.toHaveProperty('chapters')
    expect(payload).not.toHaveProperty('book')
  })

  it('restores the newer progress snapshot over a stale full session', async () => {
    // 全量快照停留在旧进度, 轻量快照更新 —— 恢复时必须取轻量那个。
    //
    // 这条以前是假测试: 它只断言 getPersistedReaderSession()/readReaderProgress()
    // 两个**存取函数**各自的返回值, 从未调用 restorePersistedSession ——
    // 而"取 updatedAt 较新者"的决策就在那里。setUp 里那个只有 restore 才需要的
    // getBookContent mock 是漏写调用的铁证。现在真的走恢复流程。
    stubStorage({
      'reader-last-session': JSON.stringify({
        book: { name: '书', author: '作者', origin: 'o', bookUrl: 'https://example.com/book/1' },
        chapters: bigChapters(10),
        currentIndex: 3,
        chapterScrollProgress: 0.1,
        updatedAt: 1000,
      }),
      'reader-last-progress': JSON.stringify({
        bookUrl: 'https://example.com/book/1',
        currentIndex: 7,
        chapterScrollProgress: 0.8,
        updatedAt: 2000,
      }),
    })
    vi.mocked(getBookContent).mockResolvedValue('正文')
    const readerStore = useReaderStore()

    await expect(readerStore.restorePersistedSession()).resolves.toBe(true)
    // 关键断言: 用了轻量快照的索引(7)而不是全量快照的(3)。
    expect(readerStore.currentIndex).toBe(7)
    expect(readerStore.chapterScrollProgress).toBe(0.8)
  })

  it('falls back to the full session when the progress snapshot is older', async () => {
    // 反向: 轻量快照反而更旧(例如它写在一次失败的滚动之后) —— 必须用全量快照。
    stubStorage({
      'reader-last-session': JSON.stringify({
        book: { name: '书', author: '作者', origin: 'o', bookUrl: 'https://example.com/book/1' },
        chapters: bigChapters(10),
        currentIndex: 5,
        chapterScrollProgress: 0.4,
        updatedAt: 5000,
      }),
      'reader-last-progress': JSON.stringify({
        bookUrl: 'https://example.com/book/1',
        currentIndex: 1,
        chapterScrollProgress: 0.05,
        updatedAt: 1000,
      }),
    })
    vi.mocked(getBookContent).mockResolvedValue('正文')
    const readerStore = useReaderStore()

    await expect(readerStore.restorePersistedSession()).resolves.toBe(true)
    expect(readerStore.currentIndex).toBe(5)
    expect(readerStore.chapterScrollProgress).toBe(0.4)
  })

  it('ignores a progress snapshot belonging to another book', async () => {
    // 打开了 A 书, 但轻量进度是 B 书的 —— 不能拿 B 的进度定位 A。
    //
    // 这条以前也是假测试: 它只把 stub 里写死的两个 bookUrl 拿来比, 断言恒真。
    // 现在真的走恢复流程, 断言 A 书用的是**自己**的索引(2)而不是 B 书的(9)。
    stubStorage({
      'reader-last-session': JSON.stringify({
        book: { name: 'A书', author: '作者', origin: 'o', bookUrl: 'https://example.com/book/A' },
        chapters: bigChapters(10),
        currentIndex: 2,
        chapterScrollProgress: 0.3,
        updatedAt: 1000,
      }),
      'reader-last-progress': JSON.stringify({
        bookUrl: 'https://example.com/book/B',
        currentIndex: 9,
        chapterScrollProgress: 0.95,
        updatedAt: 9999,
      }),
    })
    vi.mocked(getBookContent).mockResolvedValue('正文')
    const readerStore = useReaderStore()

    await expect(readerStore.restorePersistedSession()).resolves.toBe(true)
    // B 书的 updatedAt 更新(9999 > 1000), 但 bookUrl 不匹配 → 必须回退 A 书自己的快照。
    expect(readerStore.currentIndex).toBe(2)
    expect(readerStore.chapterScrollProgress).toBe(0.3)
    expect(readerStore.book?.bookUrl).toBe('https://example.com/book/A')
  })

  it('returns null for a corrupted progress snapshot instead of throwing', () => {
    stubStorage({ 'reader-last-progress': '{ not json' })
    const readerStore = useReaderStore()

    expect(readerStore.readReaderProgress()).toBeNull()
  })

  it('rejects a progress snapshot without a book url', () => {
    stubStorage({ 'reader-last-progress': JSON.stringify({ currentIndex: 1, chapterScrollProgress: 0.5 }) })
    const readerStore = useReaderStore()

    expect(readerStore.readReaderProgress()).toBeNull()
  })
})

describe('reader opening a book restored from a phone backup', () => {
  afterEach(() => {
    vi.unstubAllGlobals()
  })

  beforeEach(() => {
    setActivePinia(createPinia())
    const storage = new Map<string, string>()
    vi.stubGlobal('localStorage', {
      getItem: vi.fn((key: string) => storage.get(key) ?? null),
      setItem: vi.fn((key: string, value: string) => storage.set(key, value)),
      removeItem: vi.fn((key: string) => storage.delete(key)),
      clear: vi.fn(() => storage.clear()),
    })
    vi.mocked(getChapterList).mockReset()
    vi.mocked(getBookInfo).mockReset()
    vi.mocked(getBookContent).mockReset()
    vi.mocked(saveBookProgress).mockReset()
    vi.mocked(saveBookProgress).mockResolvedValue('ok')
    vi.stubGlobal('window', {
      setTimeout: (handler: () => void, ms?: number) => setTimeout(handler, ms),
      clearTimeout: (id: number) => clearTimeout(id),
    })
  })

  /** Legado 的 `Book.tocUrl` 是 Kotlin 非空 String，备份里就是空串。
   *  必须当成"没有目录地址"去取详情，而不是拿空 URL 直接抓目录。 */
  it('falls back to book info when the restored tocUrl is an empty string', async () => {
    const appStore = useAppStore()
    appStore.setOnlineStatus(true)
    const readerStore = useReaderStore()
    vi.mocked(getBookInfo).mockResolvedValue({
      name: '测试书籍',
      author: '测试作者',
      origin: 'https://example.test',
      bookUrl: 'https://example.test/book',
      tocUrl: 'https://example.test/book/catalog',
    })
    vi.mocked(getChapterList).mockResolvedValue([
      { title: '第一章', url: 'chapter-0', index: 0 },
    ])
    vi.mocked(getBookContent).mockResolvedValue('正文')

    await readerStore.loadBook({
      name: '测试书籍',
      author: '测试作者',
      origin: 'https://example.test',
      bookUrl: 'https://example.test/book',
      tocUrl: '',
    })

    expect(getBookInfo).toHaveBeenCalledWith('https://example.test/book', 'https://example.test')
    expect(getChapterList).toHaveBeenCalledWith(
      expect.objectContaining({ tocUrl: 'https://example.test/book/catalog' }),
    )
    expect(readerStore.content).toBe('正文')
    expect(readerStore.bookLoadError).toBe('')
  })

  /** 打开失败必须留下可读原因，并且不再把加载动画挂着 —— 否则页面永久停在
   *  「加载中…」，用户只能靠反复换源摸索（issue #22 的现象）。 */
  it('records the failure reason and clears the loading state when the toc request fails', async () => {
    const appStore = useAppStore()
    appStore.setOnlineStatus(true)
    const readerStore = useReaderStore()
    vi.mocked(getChapterList).mockRejectedValue(new Error('bookSource not found'))

    await expect(readerStore.loadBook({
      name: '测试书籍',
      author: '测试作者',
      origin: 'https://example.test/',
      bookUrl: 'https://example.test/book',
      tocUrl: 'https://example.test/book/catalog',
    })).rejects.toThrow('bookSource not found')

    expect(readerStore.bookLoadError).toBe('bookSource not found')
    expect(readerStore.loading).toBe(false)
    expect(readerStore.chaptersLoading).toBe(false)
  })

  /** 目录抓成空数组时同样要报错，而不是接受一个空目录让界面停住。 */
  it('reports an empty toc instead of silently accepting it', async () => {
    const appStore = useAppStore()
    appStore.setOnlineStatus(true)
    const readerStore = useReaderStore()
    vi.mocked(getChapterList).mockResolvedValue([])
    vi.mocked(getBookInfo).mockResolvedValue({
      name: '测试书籍',
      author: '测试作者',
      origin: 'https://example.test',
      bookUrl: 'https://example.test/book',
    })

    await expect(readerStore.loadBook({
      name: '测试书籍',
      author: '测试作者',
      origin: 'https://example.test',
      bookUrl: 'https://example.test/book',
    })).rejects.toThrow('目录为空')

    expect(readerStore.bookLoadError).toContain('目录为空')
    expect(readerStore.loading).toBe(false)
  })
})
