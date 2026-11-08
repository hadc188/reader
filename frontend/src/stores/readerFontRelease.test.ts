import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'
import { useReaderStore } from './reader'
import { deleteCustomFont, listCustomFonts } from '../api/fonts'

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
  deleteBackgroundImage: vi.fn(),
  uploadBackgroundImage: vi.fn(),
}))

vi.mock('../utils/openaiSpeech', () => ({
  DEFAULT_OPENAI_BASE_URL: 'https://api.openai.com/v1',
  inferSpeechApiFormat: vi.fn(() => 'openai'),
  getSpeechApiFormatOption: vi.fn(() => ({ supportedFormats: ['mp3'] })),
  requestOpenAISpeechAudio: vi.fn(),
}))

vi.mock('../api/fonts', () => ({
  listCustomFonts: vi.fn(),
  uploadCustomFont: vi.fn(),
  deleteCustomFont: vi.fn(),
}))

// app store 初始化时会调用主题应用（操作 document），与本测试无关；
// 直接隔离掉，测试只关注字体集本身。
vi.mock('../utils/systemUi', () => ({
  applySystemTheme: vi.fn(),
}))

/** document.fonts 的最小替身：记录增删，供断言字体是否真的被释放。 */
function createFontSet() {
  const faces = new Set<FontFace>()
  return {
    faces,
    add(face: FontFace) {
      faces.add(face)
    },
    delete(face: FontFace) {
      faces.delete(face)
    },
  }
}

describe('custom font face release', () => {
  let fontSet: ReturnType<typeof createFontSet>

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
    })

    fontSet = createFontSet()
    vi.stubGlobal('document', {
      fonts: fontSet,
      documentElement: {
        setAttribute: vi.fn(),
        style: {
          setProperty: vi.fn(),
          colorScheme: '',
          background: '',
          backgroundColor: '',
        },
      },
      body: {
        style: {
          background: '',
          backgroundColor: '',
          colorScheme: '',
        },
      },
      getElementById: vi.fn(() => null),
    })
    vi.stubGlobal('FontFace', class MockFontFace {
      family: string
      source: string
      constructor(family: string, source: string) {
        this.family = family
        this.source = source
      }
      load() {
        return Promise.resolve(this)
      }
    })
    vi.stubGlobal('fetch', vi.fn(async () => ({
      ok: true,
      blob: async () => new Blob(['font']),
    })))
    vi.stubGlobal('URL', {
      createObjectURL: vi.fn(() => 'blob:font'),
      revokeObjectURL: vi.fn(),
    })

    vi.mocked(deleteCustomFont).mockReset()
    vi.mocked(deleteCustomFont).mockResolvedValue(null)
  })

  afterEach(() => {
    vi.unstubAllGlobals()
  })

  it('removes the FontFace from document.fonts when a custom font is deleted', async () => {
    vi.mocked(listCustomFonts).mockResolvedValue([
      { id: 'abc', name: '楷体', url: 'reader://fonts/abc' },
    ])
    const store = useReaderStore()
    await store.fetchCustomFonts()
    expect(fontSet.faces.size).toBe(1)

    await store.removeCustomFont('abc')

    // 修复前这里仍然是 1：字体文件删了，但解析后的 FontFace 一直常驻。
    expect(fontSet.faces.size).toBe(0)
  })

  it('does not stack duplicate FontFace instances for the same family', async () => {
    vi.mocked(listCustomFonts).mockResolvedValue([
      { id: 'abc', name: '楷体', url: 'reader://fonts/abc' },
    ])
    const store = useReaderStore()
    await store.fetchCustomFonts()
    await store.fetchCustomFonts()

    expect(fontSet.faces.size).toBe(1)
  })

  it('releases fonts that disappeared from the backend list', async () => {
    vi.mocked(listCustomFonts).mockResolvedValue([
      { id: 'abc', name: '楷体', url: 'reader://fonts/abc' },
    ])
    const store = useReaderStore()
    await store.fetchCustomFonts()
    expect(fontSet.faces.size).toBe(1)

    vi.mocked(listCustomFonts).mockResolvedValue([])
    await store.fetchCustomFonts()

    expect(fontSet.faces.size).toBe(0)
  })
})
