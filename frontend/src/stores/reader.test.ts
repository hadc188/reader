import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'
import { useAppStore } from './app'
import { nightThemeIndex, useReaderStore } from './reader'
import { getBookContent } from '../api/bookshelf'
import { getBrowserCachedChapter } from '../utils/browserCache'
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

vi.mock('../utils/browserCache', () => ({
  getBrowserCachedChapter: vi.fn(),
  setBrowserCachedChapter: vi.fn(),
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
    vi.mocked(getBrowserCachedChapter).mockReset()
  })

  it('fetches uploaded local txt content from backend even when browser reports offline', async () => {
    vi.mocked(getBookContent).mockResolvedValue('本地正文')
    vi.mocked(getBrowserCachedChapter).mockResolvedValue(null)
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

    expect(getBrowserCachedChapter).not.toHaveBeenCalled()
    expect(getBookContent).toHaveBeenCalledWith({
      chapterUrl: 'local-txt:abc123#0',
      bookSourceUrl: 'local-txt',
      refresh: 0,
    })
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
