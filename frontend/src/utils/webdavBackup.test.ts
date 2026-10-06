import { describe, expect, it, vi } from 'vitest'
import type { WebdavBackupPayload } from './webdavBackup'
import {
  createCompatibleBackupArchiveFiles,
  createWebdavBackupPayload,
  parseCompatibleBackupArchive,
  parseWebdavBackup,
  restoreWebdavBackup,
} from './webdavBackup'
import { importCustomFonts, importLocalBooks, importReadingStats } from '../api/backup'

vi.mock('../api/bookmark', () => ({
  getBookmarks: vi.fn(async () => []),
  deleteBookmarks: vi.fn(async () => undefined),
  saveBookmarks: vi.fn(async () => undefined),
}))
vi.mock('../api/bookshelf', () => ({
  getBookshelf: vi.fn(async () => []),
  getBookGroups: vi.fn(async () => []),
  saveBooks: vi.fn(async () => undefined),
  saveBookGroup: vi.fn(async () => undefined),
  saveBookGroupOrder: vi.fn(async () => undefined),
  deleteBooks: vi.fn(async () => undefined),
  deleteBookGroup: vi.fn(async () => undefined),
  deleteBookGroups: vi.fn(async () => ({ removed: 0 })),
}))
vi.mock('../api/fonts', () => ({
  listCustomFonts: vi.fn(async () => []),
  deleteCustomFont: vi.fn(async () => undefined),
}))
vi.mock('../api/replaceRule', () => ({
  getReplaceRules: vi.fn(async () => []),
  deleteReplaceRules: vi.fn(async () => undefined),
  saveReplaceRules: vi.fn(async () => undefined),
}))
vi.mock('../api/rss', () => ({
  getRssSources: vi.fn(async () => []),
  deleteRssSource: vi.fn(async () => undefined),
  saveRssSources: vi.fn(async () => undefined),
}))
vi.mock('../api/source', () => ({
  deleteAllBookSources: vi.fn(async () => undefined),
  getBookSources: vi.fn(async () => []),
  saveBookSources: vi.fn(async () => undefined),
}))
vi.mock('../api/backup', () => ({
  exportLocalBooks: vi.fn(async () => ({
    books: [{ id: 'a'.repeat(32), files: [{ path: 'book.txt', base64: 'dGVzdA==' }] }],
    skipped: [{ id: 'b'.repeat(32), sizeBytes: 123 }],
    totalBytes: 4,
  })),
  exportCustomFonts: vi.fn(async () => ({
    fonts: [{ fileName: `${'c'.repeat(32)}__楷体.ttf`, base64: 'Zm9udA==' }],
    skipped: [],
    totalBytes: 5,
  })),
  exportReadingStats: vi.fn(async () => ({
    daily: [{ date: '2026-08-11', seconds: 90, characters: 12 }],
    byBook: [{ date: '2026-08-11', bookUrl: 'https://example.test/book', bookName: '书', bookAuthor: '', seconds: 90, characters: 12 }],
  })),
  importLocalBooks: vi.fn(async () => ({ imported: 1 })),
  importCustomFonts: vi.fn(async () => ({ imported: 1 })),
  importReadingStats: vi.fn(async () => ({ daily: 1, byBook: 1 })),
}))

function createPayload(): WebdavBackupPayload {
  return {
    version: 2,
    createdAt: '2026-08-11T00:00:00.000Z',
    app: 'reader-rust-frontend',
    bookshelf: {
      books: [
        { name: '网络书', author: '作者', bookUrl: 'https://example.test/book', origin: 'source' },
        { name: '本地书', author: '', bookUrl: 'local-txt:test', origin: 'local-txt' },
      ],
      groups: [{ groupId: 2, groupName: '收藏', orderNo: 3 }],
    },
    bookSources: [],
    rssSources: [],
    bookmarks: [],
    replaceRules: [],
    localState: { theme: 'dark' },
    localBooks: [{ id: 'a'.repeat(32), files: [{ path: 'book.txt', base64: 'dGVzdA==' }] }],
    customFonts: [{ fileName: `${'c'.repeat(32)}__楷体.ttf`, base64: 'Zm9udA==' }],
    readingStats: {
      daily: [{ date: '2026-08-11', seconds: 90, characters: 12 }],
      byBook: [],
    },
  }
}

describe('compatible backup archives', () => {
  it('writes reader data and Legado-compatible common data together', () => {
    const files = createCompatibleBackupArchiveFiles(createPayload())
    const fileMap = Object.fromEntries(files.map((file) => [file.name, file.content]))
    const commonBooks = JSON.parse(fileMap['bookshelf.json'] || '[]') as unknown[]
    const groups = JSON.parse(fileMap['bookGroup.json'] || '[]') as Array<Record<string, unknown>>

    expect(fileMap['reader-rust.json']).toBeTruthy()
    expect(commonBooks).toHaveLength(1)
    expect(groups[0]?.order).toBe(3)
    // 本地书内容与字体嵌入 reader-rust.json
    const embedded = JSON.parse(fileMap['reader-rust.json'])
    expect(embedded.localBooks).toHaveLength(1)
    expect(embedded.customFonts).toHaveLength(1)
  })

  it('restores a Legado archive and skips Android-only local books', () => {
    const result = parseCompatibleBackupArchive({
      'bookshelf.json': JSON.stringify([
        {
          name: '网络书',
          author: '作者',
          bookUrl: 'https://example.test/book',
          origin: 'https://source.test',
          durChapterIndex: 12,
          durChapterPos: 34,
        },
        {
          name: '安卓本地书',
          author: '',
          bookUrl: '/storage/emulated/0/book.txt',
          origin: 'loc_book',
          type: 256,
        },
      ]),
      'bookGroup.json': '[{"groupId":2,"groupName":"收藏","order":4}]',
      'bookSource.json': '[{"bookSourceName":"测试源","bookSourceUrl":"https://source.test"}]',
      'bookmark.json': '[]',
      'rssSources.json': '[]',
      'replaceRule.json': '[]',
    })

    expect(result.format).toBe('legado')
    expect(result.skippedLocalBooks).toBe(1)
    expect(result.payload.bookshelf.books).toHaveLength(1)
    expect(result.payload.bookshelf.books[0]?.durChapterIndex).toBe(12)
    expect(result.payload.bookshelf.groups[0]?.orderNo).toBe(4)
    expect(result.payload.localState).toEqual({})
  })

  it('prefers the embedded reader backup when it is available', () => {
    const payload = createPayload()
    const result = parseCompatibleBackupArchive({
      'reader-rust.json': JSON.stringify(payload),
      'bookshelf.json': '[]',
    })

    expect(result.format).toBe('reader')
    expect(result.payload.bookshelf.books).toHaveLength(2)
    expect(result.payload.localState.theme).toBe('dark')
  })

  it('rejects malformed compatible data files', () => {
    expect(() => parseCompatibleBackupArchive({
      'bookshelf.json': '{}',
    })).toThrow('bookshelf.json 不是数据列表')
  })

  it('skips empty book sources instead of failing the whole restore', () => {
    const result = parseCompatibleBackupArchive({
      'bookSource.json': JSON.stringify([
        { bookSourceName: '有效源', bookSourceUrl: 'https://ok.test' },
        // 真实备份里的脏数据: 名称与地址均为空串。
        { bookSourceName: '', bookSourceUrl: '', bookSourceGroup: '搜索链接规则为空,天龙' },
        { bookSourceName: '另一个有效源', bookSourceUrl: 'https://ok2.test' },
      ]),
    })

    expect(result.payload.bookSources).toHaveLength(2)
    expect(result.skipped).toHaveLength(1)
    expect(result.skipped[0]?.file).toBe('bookSource.json')
    expect(result.skipped[0]?.count).toBe(1)
    expect(result.skipped[0]?.samples).toEqual(['(未命名)'])
    expect(result.skipped[0]?.allSkipped).toBe(false)
  })

  it('reports every file with invalid entries independently', () => {
    const result = parseCompatibleBackupArchive({
      'bookshelf.json': JSON.stringify([
        { name: '正常书', bookUrl: 'https://example.test/b', origin: 'https://s.test' },
        { name: '缺地址', bookUrl: '', origin: 'https://s.test' },
      ]),
      'bookSource.json': JSON.stringify([
        { bookSourceName: '', bookSourceUrl: '' },
      ]),
      'rssSources.json': JSON.stringify([
        { sourceName: '有效RSS', sourceUrl: 'https://rss.test' },
        { sourceName: '缺地址RSS', sourceUrl: '' },
      ]),
      'bookmark.json': JSON.stringify([
        { bookName: '', bookAuthor: '', chapterIndex: 1 },
      ]),
      'replaceRule.json': JSON.stringify([
        { name: '有效规则', pattern: 'a' },
        { name: '', pattern: 'b' },
      ]),
      'bookGroup.json': JSON.stringify([
        { groupId: 0, groupName: '' },
      ]),
    })

    expect(result.payload.bookshelf.books).toHaveLength(1)
    expect(result.payload.bookSources).toHaveLength(0)
    expect(result.payload.rssSources).toHaveLength(1)
    expect(result.payload.bookmarks).toHaveLength(0)
    expect(result.payload.replaceRules).toHaveLength(1)
    expect(result.payload.bookshelf.groups).toHaveLength(0)

    const files = result.skipped.map((entry) => entry.file).sort()
    expect(files).toEqual([
      'bookGroup.json',
      'bookSource.json',
      'bookmark.json',
      'bookshelf.json',
      'replaceRule.json',
      'rssSources.json',
    ])
  })

  it('still rejects structurally broken entries', () => {
    // 元素不是对象属于结构性损坏, 不能被静默跳过。
    expect(() => parseCompatibleBackupArchive({
      'bookSource.json': '[null]',
    })).toThrow('bookSource.json 包含无效数据')
  })

  it('keeps skipped empty when every entry is valid', () => {
    const result = parseCompatibleBackupArchive({
      'bookSource.json': '[{"bookSourceName":"a","bookSourceUrl":"https://a.test"}]',
    })

    expect(result.skipped).toEqual([])
    expect(result.payload.bookSources).toHaveLength(1)
  })

  it('flags a file whose entries are all invalid', () => {
    // 恢复是先清空再写入; 该类数据全无效意味着会被清空且不写回,
    // 调用方据此在动手前确认。
    const result = parseCompatibleBackupArchive({
      'bookSource.json': JSON.stringify([
        { bookSourceName: '', bookSourceUrl: '' },
        { bookSourceName: '有名无址', bookSourceUrl: '' },
      ]),
    })

    expect(result.payload.bookSources).toHaveLength(0)
    expect(result.skipped).toHaveLength(1)
    expect(result.skipped[0]?.allSkipped).toBe(true)
    expect(result.skipped[0]?.samples).toEqual(['(未命名)', '有名无址'])
  })

  it('does not flag partial skips as fully invalid', () => {
    const result = parseCompatibleBackupArchive({
      'bookSource.json': JSON.stringify([
        { bookSourceName: '好源', bookSourceUrl: 'https://ok.test' },
        { bookSourceName: '', bookSourceUrl: '' },
      ]),
    })

    expect(result.skipped[0]?.allSkipped).toBe(false)
  })

  it('does not flag an empty file as fully invalid', () => {
    // 备份里本来就是 0 条属于正常情况, 不是"全部无效"。
    const result = parseCompatibleBackupArchive({ 'bookSource.json': '[]' })

    expect(result.skipped).toEqual([])
    expect(result.payload.bookSources).toHaveLength(0)
  })

  it('accepts v1 backups without local books, fonts or stats', () => {
    const parsed = parseWebdavBackup(JSON.stringify({
      version: 1,
      createdAt: '2026-08-11T00:00:00.000Z',
      app: 'reader-rust-frontend',
      bookshelf: { books: [], groups: [] },
      localState: {},
    }))

    expect(parsed.localBooks).toEqual([])
    expect(parsed.customFonts).toEqual([])
    expect(parsed.readingStats).toEqual({ daily: [], byBook: [] })
  })
})

describe('backup payload v2', () => {
  it('includes local books, fonts and reading stats from the backend exports', async () => {
    vi.stubGlobal('localStorage', {
      getItem: () => null,
      setItem: () => undefined,
      removeItem: () => undefined,
    })
    try {
      const payload = await createWebdavBackupPayload()

      expect(payload.version).toBe(2)
      expect(payload.localBooks).toHaveLength(1)
      expect(payload.skippedLocalBooks).toEqual([{ id: 'b'.repeat(32), sizeBytes: 123 }])
      expect(payload.customFonts).toHaveLength(1)
      expect(payload.readingStats?.daily).toHaveLength(1)
    } finally {
      vi.unstubAllGlobals()
    }
  })

  it('backfills reader page background image and hotkeys', async () => {
    // 回归测试: 阅读页专属背景图与自定义快捷键都存在**独立的** localStorage key
    // 里(不随 readConfig 走)。它们漏在备份清单外时, 恢复备份后背景图与用户
    // 自定义的按键会一并丢失, 且不会有任何提示。
    const store: Record<string, string> = {
      'reader-page-background-image': 'data:image/png;base64,AAAA',
      'reader-hotkeys': JSON.stringify({ 'Control+f': ['search'] }),
    }
    vi.stubGlobal('localStorage', {
      getItem: (key: string) => store[key] ?? null,
      setItem: (key: string, value: string) => { store[key] = value },
      removeItem: (key: string) => { delete store[key] },
    })
    try {
      const payload = await createWebdavBackupPayload()

      expect(payload.localState['reader-page-background-image']).toBe('data:image/png;base64,AAAA')
      expect(payload.localState['reader-hotkeys']).toBe(JSON.stringify({ 'Control+f': ['search'] }))
    } finally {
      vi.unstubAllGlobals()
    }
  })

  it('round-trips those keys through restore', async () => {
    // 恢复侧: 备份里有值就写回; 备份里没有才清除。验证读-写闭环而不是只看单边。
    const store: Record<string, string> = {
      'reader-page-background-image': 'data:image/png;base64,OLD',
      'reader-hotkeys': '{"old":["back"]}',
    }
    vi.stubGlobal('localStorage', {
      getItem: (key: string) => store[key] ?? null,
      setItem: (key: string, value: string) => { store[key] = value },
      removeItem: (key: string) => { delete store[key] },
    })
    try {
      const payload = createPayload()
      payload.localState = {
        ...payload.localState,
        'reader-page-background-image': 'data:image/png;base64,NEW',
        'reader-hotkeys': '{"new":["catalog"]}',
      }

      await restoreWebdavBackup(payload)

      expect(store['reader-page-background-image']).toBe('data:image/png;base64,NEW')
      expect(store['reader-hotkeys']).toBe('{"new":["catalog"]}')
    } finally {
      vi.unstubAllGlobals()
    }
  })

  it('captures the app preference keys', async () => {
    const store: Record<string, string> = {
      'reader-close-to-tray': '1',
      'reader-boss-key': 'ctrl+alt+b',
      'reader-hidden-features': '["rss"]',
    }
    vi.stubGlobal('localStorage', {
      getItem: (key: string) => store[key] ?? null,
      setItem: (key: string, value: string) => { store[key] = value },
      removeItem: (key: string) => { delete store[key] },
    })
    try {
      const payload = await createWebdavBackupPayload()

      expect(payload.localState['reader-close-to-tray']).toBe('1')
      expect(payload.localState['reader-boss-key']).toBe('ctrl+alt+b')
      expect(payload.localState['reader-hidden-features']).toBe('["rss"]')
    } finally {
      vi.unstubAllGlobals()
    }
  })

  it('restores local book files before shelf records and imports fonts and stats', async () => {
    const store: Record<string, string> = {}
    vi.stubGlobal('localStorage', {
      getItem: (key: string) => store[key] ?? null,
      setItem: (key: string, value: string) => { store[key] = value },
      removeItem: (key: string) => { delete store[key] },
    })
    try {
      await restoreWebdavBackup(createPayload())

      expect(importLocalBooks).toHaveBeenCalledWith(createPayload().localBooks)
      expect(importCustomFonts).toHaveBeenCalledWith(createPayload().customFonts)
      expect(importReadingStats).toHaveBeenCalledWith(expect.objectContaining({
        daily: [{ date: '2026-08-11', seconds: 90, characters: 12 }],
      }))
    } finally {
      vi.unstubAllGlobals()
    }
  })

  it('clears existing groups with a bulk overwrite instead of upserting one by one', async () => {
    // 回归测试: 用户反馈「备份恢复后书架分组不会被删除」。
    // saveBookGroup 是 upsert —— groupId 为 0 时后端会另分配一个新 id 再 push,
    // 且逐个写入无法表达「以这份列表为准」; 一旦前置清理被跳过(读取失败被
    // catch 成空数组), 旧分组就会和新分组混在一起。恢复必须走整表覆盖。
    const store: Record<string, string> = {}
    vi.stubGlobal('localStorage', {
      getItem: (key: string) => store[key] ?? null,
      setItem: (key: string, value: string) => { store[key] = value },
      removeItem: (key: string) => { delete store[key] },
    })
    try {
      const { saveBookGroupOrder } = await import('../api/bookshelf')
      const { saveBookGroup } = await import('../api/bookshelf')
      vi.mocked(saveBookGroupOrder).mockClear()
      vi.mocked(saveBookGroup).mockClear()

      await restoreWebdavBackup(createPayload())

      // 备份里的分组必须整表覆盖写入一次
      expect(saveBookGroupOrder).toHaveBeenCalledTimes(1)
      expect(saveBookGroupOrder).toHaveBeenCalledWith(createPayload().bookshelf.groups)
      // 不能再用逐个 upsert 的路径
      expect(saveBookGroup).not.toHaveBeenCalled()
    } finally {
      vi.unstubAllGlobals()
    }
  })

  it('assigns ids to groups that arrive without one', async () => {
    // 防御路径: 正常解析会剔除 groupId 为 0 的分组(见 toBookGroup 的防呆), 所以
    // 这里直接构造 payload 绕过解析器, 验证 restore 侧不会把 0 原样写进存储 ——
    // 那样后端会另分配 id, 与书架的 group 位域失配。
    const store: Record<string, string> = {}
    vi.stubGlobal('localStorage', {
      getItem: (key: string) => store[key] ?? null,
      setItem: (key: string, value: string) => { store[key] = value },
      removeItem: (key: string) => { delete store[key] },
    })
    try {
      const { saveBookGroupOrder } = await import('../api/bookshelf')
      vi.mocked(saveBookGroupOrder).mockClear()

      const payload = createPayload()
      payload.bookshelf.groups = [
        { groupId: 0, groupName: '无 id', orderNo: 0 },
        { groupId: 5, groupName: '有 id', orderNo: 1 },
      ]
      await restoreWebdavBackup(payload)

      const written = vi.mocked(saveBookGroupOrder).mock.calls[0]?.[0]
      expect(written).toHaveLength(2)
      // 已有的 5 必须保持, 缺 id 的补一个正数且不与 5 冲突
      expect(written?.map((g) => g.groupId).sort()).toEqual([1, 5])
    } finally {
      vi.unstubAllGlobals()
    }
  })

  it('keeps Legado negative group ids through parse and restore', async () => {
    // 兼容性回归: Legado 用一批负值作保留/虚拟分组 ——
    //   IdRoot=-100, IdAll=-1, IdLocal=-2, IdAudio=-3, IdNetNone=-4,
    //   IdLocalNone=-5, IdVideo=-6, IdError=-11
    // 它们是合法且有语义的。若用 `groupId > 0` 当判据, 这些分组会被当成「缺 id」
    // 改写成正数, 破坏从 Legado 导入的分组语义。
    // 这里走**真实解析路径**(parseCompatibleBackupArchive), 而不是手搓 payload。
    const store: Record<string, string> = {}
    vi.stubGlobal('localStorage', {
      getItem: (key: string) => store[key] ?? null,
      setItem: (key: string, value: string) => { store[key] = value },
      removeItem: (key: string) => { delete store[key] },
    })
    try {
      const parsed = parseCompatibleBackupArchive({
        'bookshelf.json': JSON.stringify([]),
        'bookGroup.json': JSON.stringify([
          { groupId: -2, groupName: '本地', order: 0 },
          { groupId: -1, groupName: '全部', order: 1 },
          { groupId: 1, groupName: '玄幻', order: 2 },
        ]),
      })

      expect(parsed.format).toBe('legado')
      expect(parsed.payload.bookshelf.groups.map((g) => g.groupId)).toEqual([-2, -1, 1])

      const { saveBookGroupOrder } = await import('../api/bookshelf')
      vi.mocked(saveBookGroupOrder).mockClear()

      await restoreWebdavBackup(parsed.payload)

      const ids = vi.mocked(saveBookGroupOrder).mock.calls[0]?.[0]?.map((g) => g.groupId) ?? []
      expect(ids).toEqual([-2, -1, 1])
    } finally {
      vi.unstubAllGlobals()
    }
  })

  it('drops a Legado group that has no usable id, instead of writing id 0', async () => {
    // 既有防呆: groupId 为 0 / 缺失的分组无法被任何书引用, 解析阶段就剔除掉。
    const parsed = parseCompatibleBackupArchive({
      'bookshelf.json': JSON.stringify([]),
      'bookGroup.json': JSON.stringify([
        { groupId: 1, groupName: '有效', order: 0 },
        { groupName: '缺 id', order: 1 },
        { groupId: 0, groupName: '零 id', order: 2 },
      ]),
    })

    expect(parsed.payload.bookshelf.groups.map((g) => g.groupId)).toEqual([1])
    expect(parsed.skipped.some((s) => s.file === 'bookGroup.json')).toBe(true)
  })

  it('does not touch local state for a legado backup', async () => {
    // Legado 备份没有 localState/customFonts/readingStats —— 恢复时不能去动
    // 本应用的外观偏好(否则会用空对象把用户设置清掉)。
    const store: Record<string, string> = { readConfig: '{"fontSize":20}' }
    vi.stubGlobal('localStorage', {
      getItem: (key: string) => store[key] ?? null,
      setItem: (key: string, value: string) => { store[key] = value },
      removeItem: (key: string) => { delete store[key] },
    })
    try {
      const payload = createPayload()
      payload.app = 'legado'
      payload.localState = {}
      await restoreWebdavBackup(payload)

      expect(store.readConfig).toBe('{"fontSize":20}')
    } finally {
      vi.unstubAllGlobals()
    }
  })

  it('clears custom fonts before importing, so removed fonts do not linger', async () => {
    const store: Record<string, string> = {}
    vi.stubGlobal('localStorage', {
      getItem: (key: string) => store[key] ?? null,
      setItem: (key: string, value: string) => { store[key] = value },
      removeItem: (key: string) => { delete store[key] },
    })
    try {
      const { listCustomFonts, deleteCustomFont } = await import('../api/fonts')
      vi.mocked(listCustomFonts).mockClear()
      vi.mocked(listCustomFonts).mockResolvedValueOnce([
        { id: 'old-font-id', name: '旧字体', url: 'reader://files?path=default/fonts/x.ttf' },
      ])
      vi.mocked(deleteCustomFont).mockClear()

      await restoreWebdavBackup(createPayload())

      // 恢复前应把现有自定义字体清掉, 否则备份里没有的旧字体仍留在列表里
      expect(listCustomFonts).toHaveBeenCalled()
      expect(deleteCustomFont).toHaveBeenCalledWith('old-font-id')
    } finally {
      vi.unstubAllGlobals()
    }
  })

  it('deletes every existing entity before writing, not just the fonts', async () => {
    // 回归测试: 本次改动的核心命题是「恢复前把旧数据清干净」, 但此前所有 getter
    // 都被 mock 成返回空数组, 于是 clearCurrentData 里 6 条清理分支全部走了
    // `Promise.resolve()` 空分支 —— 一条删除都没被断言, 核心命题没有回归网。
    // 这里让每个 getter 返回非空数据, 断言对应的删除函数确实被调用。
    // 若哪天有人把 `.catch(() => [])` 之类"读失败就当空"的写法加回来,
    // 读取失败会退回空数组、清理被跳过, 这条测试就会失败。
    const store: Record<string, string> = {}
    vi.stubGlobal('localStorage', {
      getItem: (key: string) => store[key] ?? null,
      setItem: (key: string, value: string) => { store[key] = value },
      removeItem: (key: string) => { delete store[key] },
    })
    try {
      const shelf = await import('../api/bookshelf')
      const bookmark = await import('../api/bookmark')
      const replaceRule = await import('../api/replaceRule')
      const rss = await import('../api/rss')
      const source = await import('../api/source')

      // 现存数据: 含 Legado 的负数保留分组, 负数必须照样被删除。
      vi.mocked(shelf.getBookGroups).mockResolvedValueOnce([
        { groupId: 1, groupName: '玄幻', orderNo: 0 },
        { groupId: -2, groupName: '本地', orderNo: 1 },
      ])
      vi.mocked(shelf.getBookshelf).mockResolvedValueOnce([
        { name: '书A', author: '', bookUrl: 'u1', origin: 'o' },
      ] as never)
      vi.mocked(bookmark.getBookmarks).mockResolvedValueOnce([
        { time: 1, bookName: '书B', bookAuthor: '', chapterIndex: 0, chapterPos: 0, chapterName: '章' },
      ])
      vi.mocked(replaceRule.getReplaceRules).mockResolvedValueOnce([
        { name: '规则', pattern: 'a', replacement: 'b', isRegex: false, enabled: true, orderNo: 0 },
      ] as never)
      vi.mocked(rss.getRssSources).mockResolvedValueOnce([
        { sourceUrl: 'https://rss.test', sourceName: 'RSS', enabled: true, sortUrl: '', customOrder: 0 },
      ] as never)

      for (const fn of [
        shelf.deleteBookGroups, shelf.deleteBooks, bookmark.deleteBookmarks,
        replaceRule.deleteReplaceRules, rss.deleteRssSource, source.deleteAllBookSources,
      ]) (fn as unknown as { mockClear: () => void }).mockClear()

      await restoreWebdavBackup(createPayload())

      // 分组必须**一次批量删**: 逐个并发删在后端是「读列表→删→写回」, 读改写交错
      // 会让被删的分组复活(A 读[1,2,3]、B 读[1,2,3]、A 写[2,3]、B 写[1,3] →
      // 分组 1 回来)。负数保留 id 也要一起传进去。
      expect(shelf.deleteBookGroups).toHaveBeenCalledTimes(1)
      expect(shelf.deleteBookGroups).toHaveBeenCalledWith([1, -2])
      expect(shelf.deleteBooks).toHaveBeenCalled()
      expect(bookmark.deleteBookmarks).toHaveBeenCalledWith(
        expect.arrayContaining([expect.objectContaining({ bookName: '书B' })]),
      )
      expect(replaceRule.deleteReplaceRules).toHaveBeenCalledWith(
        expect.arrayContaining([expect.objectContaining({ name: '规则' })]),
      )
      expect(rss.deleteRssSource).toHaveBeenCalledWith(
        expect.objectContaining({ sourceUrl: 'https://rss.test' }),
      )
      // 书源没有「按列表删」的接口, 整表清空
      expect(source.deleteAllBookSources).toHaveBeenCalled()
    } finally {
      vi.unstubAllGlobals()
    }
  })

  it('does not clear anything when the existing data reads back empty', async () => {
    // 对侧: 本来就是空库时不该去调删除(避免无谓 IPC)。
    const store: Record<string, string> = {}
    vi.stubGlobal('localStorage', {
      getItem: (key: string) => store[key] ?? null,
      setItem: (key: string, value: string) => { store[key] = value },
      removeItem: (key: string) => { delete store[key] },
    })
    try {
      const shelf = await import('../api/bookshelf')
      const bookmark = await import('../api/bookmark')
      vi.mocked(shelf.deleteBookGroups).mockClear()
      vi.mocked(shelf.deleteBooks).mockClear()
      vi.mocked(bookmark.deleteBookmarks).mockClear()

      await restoreWebdavBackup(createPayload())

      expect(shelf.deleteBookGroups).not.toHaveBeenCalled()
      expect(shelf.deleteBooks).not.toHaveBeenCalled()
      expect(bookmark.deleteBookmarks).not.toHaveBeenCalled()
    } finally {
      vi.unstubAllGlobals()
    }
  })

  it('aborts the restore when reading existing data fails, instead of silently skipping the clear', async () => {
    // 关键回归: 早先写的是 `getBookGroups().catch(() => [])`, 读取失败会退回空数组,
    // `length` 守卫随即整块跳过清理, 而写入侧是 upsert 合并 —— 旧数据原地残留、
    // 和新数据混成并集, 且全程无报错。现在必须显式失败。
    const store: Record<string, string> = {}
    vi.stubGlobal('localStorage', {
      getItem: (key: string) => store[key] ?? null,
      setItem: (key: string, value: string) => { store[key] = value },
      removeItem: (key: string) => { delete store[key] },
    })
    try {
      const shelf = await import('../api/bookshelf')
      const { saveBookGroupOrder } = shelf
      vi.mocked(shelf.getBookGroups).mockRejectedValueOnce(new Error('IPC 读取失败'))
      vi.mocked(saveBookGroupOrder).mockClear()

      await expect(restoreWebdavBackup(createPayload())).rejects.toThrow('IPC 读取失败')

      // 清理阶段就失败 → 一本书、一个分组都不该被写入。
      expect(saveBookGroupOrder).not.toHaveBeenCalled()
    } finally {
      vi.unstubAllGlobals()
    }
  })
})
