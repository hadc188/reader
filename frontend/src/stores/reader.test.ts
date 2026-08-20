import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'
import { useAppStore } from './app'
import { nightThemeIndex, useReaderStore } from './reader'
import { getBookContent, getChapterList, saveBookProgress } from '../api/bookshelf'
import { syncLegadoBookProgress } from '../api/webdav'
import { requestOpenAISpeechAudio } from '../utils/openaiSpeech'

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
    vi.mocked(getChapterList).mockReset()
    vi.mocked(saveBookProgress).mockReset()
    vi.mocked(saveBookProgress).mockResolvedValue('ok')
    vi.mocked(syncLegadoBookProgress).mockReset()
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
      refresh: 0,
    })
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
    const image = 'data:image/webp;base64,dGVzdA=='

    readerStore.setBackgroundImage(image)
    readerStore.updateConfig('backgroundOpacity', 0.6)
    expect(readerStore.chromeTheme.popup).toBe('color-mix(in srgb, #fff 84%, transparent)')
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
    expect(readerStore.openPosition).toEqual({ index: 1, position: 4, time: 123456 })
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
      false,
      false,
    )
  })

  it('restores a newer phone progress even when it points to an earlier chapter', async () => {
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

    expect(readerStore.currentIndex).toBe(0)
    expect(readerStore.chapterScrollProgress).toBe(0.25)
  })

  it('force uploads the current progress when leaving the reader', async () => {
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
    vi.mocked(syncLegadoBookProgress).mockResolvedValue({ configured: true, uploaded: true })

    await readerStore.uploadCurrentBookProgressToLegado()

    expect(syncLegadoBookProgress).toHaveBeenCalledWith(
      appStore.legadoWebdavConfig,
      expect.objectContaining({ durChapterIndex: 0, durChapterPos: 4 }),
      true,
      true,
    )
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
    vi.stubGlobal('URL', {
      createObjectURL: vi.fn(() => 'blob:test-audio'),
      revokeObjectURL: vi.fn(),
    })
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
