import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'
import type { Book } from '../types'
import { getBookContent, getBookInfo, getChapterList, saveBookProgress } from '../api/bookshelf'
import { useAppStore } from './app'
import { useReaderStore } from './reader'

vi.mock('../api/bookshelf', async (importOriginal) => ({
  ...await importOriginal<typeof import('../api/bookshelf')>(),
  getBookContent: vi.fn(),
  getBookInfo: vi.fn(),
  getChapterList: vi.fn(),
  saveBookProgress: vi.fn(),
}))

const originalContent = '广告\n正文汉字<img src="comment.png"/>'

function setupReader(readConfig?: Book['readConfig']) {
  const reader = useReaderStore()
  reader.book = {
    name: '配置测试书籍',
    author: '测试作者',
    origin: 'https://config.fixture.test',
    bookUrl: 'https://config.fixture.test/book',
    readConfig,
  }
  reader.content = originalContent
  reader.replaceRules = [
    { id: 1, name: '广告净化', pattern: '广告\n', replacement: '', isEnabled: true, isRegex: false, order: 0 },
    { id: 2, name: '图片净化', pattern: '<img\\b[^>]*>', replacement: '', isEnabled: true, isRegex: true, order: 1 },
  ]
  return reader
}

describe('per-book replace rules', () => {
  beforeEach(() => {
    setActivePinia(createPinia())
    const storage = new Map<string, string>()
    vi.stubGlobal('localStorage', {
      getItem: (key: string) => storage.get(key) ?? null,
      setItem: (key: string, value: string) => storage.set(key, value),
      removeItem: (key: string) => storage.delete(key),
    })
    vi.mocked(getBookInfo).mockReset()
    vi.mocked(getChapterList).mockReset()
    vi.mocked(getChapterList).mockResolvedValue([{ title: '第一章', url: 'chapter-1', index: 0 }])
    vi.mocked(getBookContent).mockReset()
    vi.mocked(getBookContent).mockResolvedValue(originalContent)
    vi.mocked(saveBookProgress).mockReset()
    vi.mocked(saveBookProgress).mockResolvedValue('ok')
    useAppStore().setOnlineStatus(false)
  })

  afterEach(() => {
    vi.unstubAllGlobals()
  })

  it('preserves source content and comment images when explicitly disabled', () => {
    const reader = setupReader({ useReplaceRule: false })
    expect(reader.displayContent).toBe(originalContent)
    expect(reader.processContentForDisplay(originalContent)).toBe(originalContent)
  })

  it.each<Book['readConfig']>([undefined, null, {}, { useReplaceRule: null }])(
    'applies existing purification when the per-book setting is absent (%j)',
    (readConfig) => {
      const reader = setupReader(readConfig)
      expect(reader.displayContent).toBe('正文汉字')
    },
  )

  it('restores purification when the same book explicitly enables it', () => {
    const reader = setupReader({ useReplaceRule: false })
    expect(reader.displayContent).toBe(originalContent)
    reader.book!.readConfig!.useReplaceRule = true
    expect(reader.displayContent).toBe('正文汉字')
  })

  it('keeps Chinese conversion active when purification is disabled', async () => {
    const reader = setupReader({ useReplaceRule: false })
    reader.updateConfig('chineseMode', 'traditional')
    // 词典是 ~1.2MB 的懒加载分块，全量并发跑时首次 import 偶尔超过 waitFor
    // 默认的 1s 上限（单独跑稳定通过）。放宽到 5s，避免这条与功能无关的用例
    // 把整套测试拖成"每次都有 1 个失败"。
    await vi.waitFor(() => {
      expect(reader.displayContent).toBe('廣告\n正文漢字<img src="comment.png"/>')
    }, { timeout: 5000 })
  })

  it('loads source reading configuration before displaying a book opened from search', async () => {
    const reader = setupReader()
    const book = reader.book!
    vi.mocked(getBookInfo).mockResolvedValue({
      ...book,
      tocUrl: 'https://config.fixture.test/toc',
      readConfig: { useReplaceRule: false, imageStyle: 'fit' },
    })

    await reader.loadBook(book, false)

    expect(getBookInfo).toHaveBeenCalledExactlyOnceWith(book.bookUrl, book.origin)
    expect(getChapterList).toHaveBeenCalledExactlyOnceWith({
      bookUrl: book.bookUrl, bookSourceUrl: book.origin, tocUrl: 'https://config.fixture.test/toc',
    })
    expect(reader.book?.readConfig).toEqual({ useReplaceRule: false, imageStyle: 'fit' })
    expect(reader.displayContent).toBe(originalContent)
  })

  it('continues through the chapter API if the detail request fails', async () => {
    const reader = setupReader()
    const book = reader.book!
    vi.mocked(getBookInfo).mockRejectedValue(new Error('暂时无法读取详情'))

    await reader.loadBook(book, false)

    expect(getChapterList).toHaveBeenCalledExactlyOnceWith({
      bookUrl: book.bookUrl, bookSourceUrl: book.origin, tocUrl: undefined,
    })
    expect(reader.displayContent).toBe('正文汉字')
  })

  it('uses an existing catalog and reading configuration directly', async () => {
    const reader = setupReader({ useReplaceRule: false })
    const book = { ...reader.book!, tocUrl: 'https://config.fixture.test/toc' }

    await reader.loadBook(book, false)

    expect(getBookInfo).not.toHaveBeenCalled()
    expect(reader.displayContent).toBe(originalContent)
  })

  it('opens local books without requesting remote book details', async () => {
    const reader = setupReader()
    await reader.loadBook({ name: '本地书', author: '作者', origin: 'local-txt', bookUrl: 'local-txt:fixture' }, false)
    expect(getBookInfo).not.toHaveBeenCalled()
    expect(reader.displayContent).toBe('正文汉字')
  })

  it('keeps images when a greedy rule matches the whole line', () => {
    const reader = setupReader()
    reader.replaceRules = [
      { id: 1, name: '整行重写', pattern: '.*', replacement: '替换', isEnabled: true, isRegex: true, order: 0 },
    ]
    const output = reader.processContentForDisplay('广告<img src="a.png">尾巴')
    // 整行被规则吃掉，图片自然也不在结果里；关键是没有留下被切碎的 `<img` 残片。
    // 结果是两段「替换」而不是一段，因为 `.*` 在 JS 里末尾还会命中一个空串
    // （`'abc'.replace(/.*/g, 'X')` → `'XX'`），与本次改动无关。
    expect(output).toBe('替换替换')
    expect(output).not.toContain('<img')
  })

  it('protects images from rules that only target text around them', () => {
    const reader = setupReader()
    reader.replaceRules = [
      { id: 1, name: '删广告词', pattern: '广告', replacement: '', isEnabled: true, isRegex: false, order: 0 },
    ]
    const output = reader.processContentForDisplay('广告<img src="a.png">正文')
    expect(output).toContain('<img src="a.png">')
    expect(output).not.toContain('广告')
  })

  it('applies title-only rules to the displayed title only', () => {
    const reader = setupReader()
    reader.chapters = [{ title: '第十二章 免费阅读', url: 'chapter-1', index: 0 }]
    reader.currentIndex = 0
    reader.replaceRules = [
      {
        id: 1,
        name: '去标题后缀',
        pattern: '免费阅读',
        replacement: '',
        isEnabled: true,
        isRegex: false,
        order: 0,
        scopeTitle: true,
        scopeContent: false,
      },
    ]
    expect(reader.displayChapterTitle).toBe('第十二章 ')
    // 章节原值必须保留：换源与进度恢复都靠标题定位。
    expect(reader.chapters[0].title).toBe('第十二章 免费阅读')
    // 只作用于标题的规则不能改正文。
    expect(reader.processContentForDisplay('免费阅读')).toBe('免费阅读')
  })

  it('honors excludeScope so a rule can skip one book', () => {
    const reader = setupReader()
    reader.replaceRules = [
      {
        id: 1,
        name: '通用去广告',
        pattern: '广告',
        replacement: '',
        isEnabled: true,
        isRegex: false,
        order: 0,
        scope: '*',
        excludeScope: reader.book!.name,
      },
    ]
    // 当前书被排除 → 规则不生效。
    expect(reader.processContentForDisplay('广告正文')).toBe('广告正文')
  })

  it('matches scope by substring like the phone app does', () => {
    const reader = setupReader()
    const book = reader.book!
    reader.replaceRules = [
      {
        id: 1,
        name: '混合串范围',
        pattern: '广告',
        replacement: '',
        isEnabled: true,
        isRegex: false,
        order: 0,
        // Legado 导出的混合串：精确相等匹配不到，包含匹配才能命中。
        scope: `${book.name},${book.origin}`,
      },
    ]
    expect(reader.processContentForDisplay('广告正文')).toBe('正文')
  })

  it('ignores a late detail response after another book has opened', async () => {
    const reader = setupReader()
    const firstBook = reader.book!
    const secondBook = { ...firstBook, name: '第二本书', bookUrl: 'https://config.fixture.test/second' }
    let resolveFirst!: (book: Book) => void
    vi.mocked(getBookInfo).mockImplementationOnce(() => new Promise((resolve) => { resolveFirst = resolve }))
    vi.mocked(getBookInfo).mockResolvedValueOnce({
      ...secondBook, tocUrl: 'https://config.fixture.test/second/toc', readConfig: { useReplaceRule: true },
    })
    const firstLoad = reader.loadBook(firstBook, false)
    await reader.loadBook(secondBook, false)
    resolveFirst({ ...firstBook, tocUrl: 'https://config.fixture.test/first/toc', readConfig: { useReplaceRule: false } })
    await firstLoad

    expect(getChapterList).toHaveBeenCalledTimes(1)
    expect(reader.book?.bookUrl).toBe(secondBook.bookUrl)
    expect(reader.book?.readConfig?.useReplaceRule).toBe(true)
    expect(reader.displayContent).toBe('正文汉字')
  })

  it('does not carry a disabled setting into another book', () => {
    const reader = setupReader({ useReplaceRule: false })
    expect(reader.displayContent).toBe(originalContent)
    reader.book = { name: '另一本书', author: '作者', origin: 'fixture', bookUrl: 'fixture:other' }
    expect(reader.displayContent).toBe('正文汉字')
  })

  it('preloads nearby chapters by default when no config has been saved', () => {
    const reader = useReaderStore()
    expect(reader.config.enablePreload).toBe(true)
  })

  it('still honours an explicitly saved preload switch', () => {
    localStorage.setItem('readConfig', JSON.stringify({ enablePreload: false }))
    const reader = useReaderStore()
    // 默认值只影响没有存档的用户; 用户自己关过就必须保持关闭。
    expect(reader.config.enablePreload).toBe(false)
  })
})
