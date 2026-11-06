import { beforeEach, describe, expect, it, vi } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'
import { useBookshelfStore } from './bookshelf'
import { getBookshelfWithCacheInfo, getBookGroups, saveBooks } from '../api/bookshelf'

vi.mock('../api/bookshelf', () => ({
  getBookshelfWithCacheInfo: vi.fn(),
  getBookGroups: vi.fn(),
  deleteBook: vi.fn(),
  deleteBooks: vi.fn(),
  saveBookGroupId: vi.fn(),
  saveBookGroup: vi.fn(),
  deleteBookGroup: vi.fn(),
  saveBooks: vi.fn(),
}))

vi.mock('../utils/recentBooks', () => ({
  clearRecentReadBooks: vi.fn(),
  getRecentReadBookKey: vi.fn((book) => `${book.origin || ''}::${book.bookUrl}`),
  loadRecentReadBooks: vi.fn(() => []),
  removeRecentReadBook: vi.fn(),
}))

describe('bookshelf search state', () => {
  beforeEach(() => {
    setActivePinia(createPinia())
    vi.mocked(getBookshelfWithCacheInfo).mockResolvedValue([])
    vi.mocked(saveBooks).mockResolvedValue([] as never)
  })

  it('starts searches in all-sources scope by default', () => {
    const store = useBookshelfStore()

    store.startSearch('星门')

    expect(store.searchKey).toBe('星门')
    expect(store.searchScope).toBe('all')
    expect(store.searchSourceUrl).toBe('')
    expect(store.searchGroup).toBe('')
  })


  it('keeps application cache counts from the backend', async () => {
    vi.mocked(getBookshelfWithCacheInfo).mockResolvedValue([
      {
        name: '本地书',
        author: '本地导入',
        origin: 'local-txt',
        bookUrl: 'local-txt:abc',
        cachedChapterCount: 12,
      },
      {
        name: '远程书',
        author: '作者',
        origin: 'https://source.example',
        bookUrl: 'https://book.example/1',
      },
    ] as never)
    const store = useBookshelfStore()

    await store.fetchBooks()

    expect(store.books.find((book) => book.bookUrl === 'local-txt:abc')?.cachedChapterCount).toBe(12)
    expect(store.books.find((book) => book.bookUrl === 'https://book.example/1')?.cachedChapterCount).toBeUndefined()
  })

  it('can start a search with the active explore source selected', () => {
    const store = useBookshelfStore()

    store.startSearch('星门', { scope: 'source', sourceUrl: 'https://m.epsilon.example.com' })

    expect(store.searchKey).toBe('星门')
    expect(store.searchScope).toBe('source')
    expect(store.searchSourceUrl).toBe('https://m.epsilon.example.com')
  })
})

describe('setBooksGroupMask (勾选式分组编辑器落盘)', () => {
  beforeEach(() => {
    setActivePinia(createPinia())
    vi.mocked(getBookshelfWithCacheInfo).mockResolvedValue([
      {
        name: '书A',
        author: '作者',
        origin: 'local-txt',
        bookUrl: 'book:a',
        group: 1,
      },
      {
        name: '书B',
        author: '作者',
        origin: 'local-txt',
        bookUrl: 'book:b',
        group: 2,
      },
    ] as never)
    vi.mocked(saveBooks).mockResolvedValue([] as never)
  })

  it('把目标书的位域整体替换为掩码(勾选=加入, 取消勾选=移出)', async () => {
    const store = useBookshelfStore()
    await store.fetchBooks()

    // 书A 从分组 1 移到分组 1|4(同时属于两个分组), 书B 不动。
    await store.setBooksGroupMask(['book:a'], 1 | 4)

    expect(saveBooks).toHaveBeenCalled()
    const saved = lastSavedBooks()
    expect(saved.find((b) => b.bookUrl === 'book:a')?.group).toBe(5)
    expect(saved.find((b) => b.bookUrl === 'book:b')?.group).toBe(2)
    // 内存同步更新, 不等全量刷新
    expect(store.books.find((b) => b.bookUrl === 'book:a')?.group).toBe(5)
  })

  it('掩码为 0 时即「移出所有分组」', async () => {
    const store = useBookshelfStore()
    await store.fetchBooks()

    await store.setBooksGroupMask(['book:a'], 0)

    const saved = lastSavedBooks()
    expect(saved.find((b) => b.bookUrl === 'book:a')?.group).toBe(0)
  })

  it('落盘失败时回滚内存, 不留脏快照', async () => {
    const store = useBookshelfStore()
    await store.fetchBooks()
    vi.mocked(saveBooks).mockRejectedValueOnce(new Error('boom'))

    await expect(store.setBooksGroupMask(['book:a'], 8)).rejects.toThrow('boom')
    // 回滚: 内存里书 A 仍在分组 1
    expect(store.books.find((b) => b.bookUrl === 'book:a')?.group).toBe(1)
  })
})

/** 取 saveBooks 最近一次收到的载荷(vi 的 mock 状态跨测试保留, 必须取最后)。 */
function lastSavedBooks() {
  const calls = vi.mocked(saveBooks).mock.calls
  return calls[calls.length - 1][0]
}

describe('displayGroups (书架分组标签)', () => {
  beforeEach(() => {
    setActivePinia(createPinia())
    vi.mocked(getBookshelfWithCacheInfo).mockResolvedValue([])
    vi.mocked(saveBooks).mockResolvedValue([] as never)
  })

  it('不重复展示内置「全部」, 也不展示 Legado 的虚拟分组', async () => {
    vi.mocked(getBookGroups).mockResolvedValue([
      // Legado 备份原样带进来的保留分组: 负数 id, 不是位域分组。
      { groupId: -1, groupName: '全部' },
      { groupId: -2, groupName: '本地' },
      { groupId: -3, groupName: '音频' },
      { groupId: -5, groupName: '本地未分组' },
      { groupId: -11, groupName: '更新失败' },
      { groupId: 1, groupName: '玄幻' },
      { groupId: 2, groupName: '未分组' },
    ])

    const store = useBookshelfStore()
    await store.fetchGroups()

    expect(store.displayGroups.map((g) => g.groupName)).toEqual(['全部', '玄幻', '未分组'])
    expect(store.displayGroups.map((g) => g.groupId)).toEqual([-1, 1, 0])
  })

  it('虚拟分组仍留在 store 里, 只是不展示(备份恢复要原样写回)', async () => {
    vi.mocked(getBookGroups).mockResolvedValue([
      { groupId: -1, groupName: '全部' },
      { groupId: 4, groupName: '仙侠' },
    ])

    const store = useBookshelfStore()
    await store.fetchGroups()

    expect(store.groups.map((g) => g.groupId)).toEqual([-1, 4])
  })

  it('刷新后停在不可见分组上时回到「全部」', async () => {
    vi.mocked(getBookGroups).mockResolvedValue([
      { groupId: -2, groupName: '本地' },
      { groupId: 4, groupName: '仙侠' },
    ])

    const store = useBookshelfStore()
    store.activeGroupId = -2
    await store.fetchGroups()

    expect(store.activeGroupId).toBe(-1)
  })
})

describe('书架排序', () => {
  beforeEach(() => {
    setActivePinia(createPinia())
    // 测试环境是 node, 没有 localStorage; store 里读写偏好都包了 try/catch,
    // 这里显式 stub 才能断言持久化键值。
    vi.stubGlobal('localStorage', {
      getItem: vi.fn(() => null),
      setItem: vi.fn(),
      removeItem: vi.fn(),
    })
    vi.mocked(saveBooks).mockResolvedValue([] as never)
    vi.mocked(getBookGroups).mockResolvedValue([])
    vi.mocked(getBookshelfWithCacheInfo).mockResolvedValue([
      { name: '遮天', author: '辰东', origin: 'o', bookUrl: 'b:1', durChapterTime: 100 },
      { name: '斗破苍穹', author: '天蚕土豆', origin: 'o', bookUrl: 'b:2', durChapterTime: 300 },
      { name: '红楼梦', author: '曹雪芹', origin: 'o', bookUrl: 'b:3', durChapterTime: 200 },
    ] as never)
  })

  it('默认按最近阅读排序', async () => {
    const store = useBookshelfStore()
    await store.fetchBooks()

    expect(store.sortedBooks.map((b) => b.name)).toEqual(['斗破苍穹', '红楼梦', '遮天'])
  })

  it('切换排序方式会持久化', async () => {
    const store = useBookshelfStore()
    await store.fetchBooks()

    store.setSortMode(2)

    expect(store.sortedBooks.map((b) => b.name)).toEqual(['斗破苍穹', '红楼梦', '遮天'])
    expect(localStorage.setItem).toHaveBeenCalledWith('bookshelf-sort', '2')
  })

  it('分组自带的排序优先于全局', async () => {
    vi.mocked(getBookGroups).mockResolvedValue([
      { groupId: 1, groupName: '玄幻', bookSort: 2 },
    ])
    const store = useBookshelfStore()
    await store.fetchBooks()
    await store.fetchGroups()

    // 还没有选中具体分组时用全局默认
    expect(store.activeGroupSort).toBeNull()
    expect(store.effectiveSortMode).toBe(0)

    store.activeGroupId = 1
    expect(store.activeGroupSort).toBe(2)
    expect(store.effectiveSortMode).toBe(2)
  })

  it('分组 bookSort 为 -1 时跟随全局', async () => {
    vi.mocked(getBookGroups).mockResolvedValue([
      { groupId: 1, groupName: '玄幻', bookSort: -1 },
    ])
    const store = useBookshelfStore()
    await store.fetchBooks()
    await store.fetchGroups()
    store.setSortMode(5)

    store.activeGroupId = 1
    expect(store.activeGroupSort).toBeNull()
    expect(store.effectiveSortMode).toBe(5)
  })

  it('拖拽写入 order 序号并落盘', async () => {
    const store = useBookshelfStore()
    await store.fetchBooks()

    // 把「遮天」拖到「红楼梦」处(数据原始顺序: 遮天, 斗破, 红楼梦)。
    // reorderBooks 按 order 语义重排, 与当前展示排序无关。
    await store.reorderBooks('b:1', 'b:3')

    expect(saveBooks).toHaveBeenCalled()
    const saved = lastSavedBooks()
    const byUrl = new Map(saved.map((b) => [b.bookUrl, b.order]))
    // 所有书都重编号为 0..n-1
    expect([...byUrl.values()].sort()).toEqual([0, 1, 2])
    // 遮天从第一位挪到末位
    expect(byUrl.get('b:1')).toBe(2)
    expect(byUrl.get('b:2')).toBe(0)
  })

  it('打开书时把它挪到手动顺序最前', async () => {
    const store = useBookshelfStore()
    await store.fetchBooks()

    await store.moveBookToFront('b:3')

    const saved = lastSavedBooks()
    const byUrl = new Map(saved.map((b) => [b.bookUrl, b.order]))
    expect(byUrl.get('b:3')).toBe(0)
    expect([...byUrl.values()].sort()).toEqual([0, 1, 2])
  })

  it('已下架的「手动排序」偏好迁移到最近阅读', async () => {
    vi.mocked(localStorage.getItem).mockReturnValue('3')

    const store = useBookshelfStore()
    await store.fetchBooks()

    expect(store.sortMode).toBe(0)
    // 迁移结果要写回, 否则每次启动都会重算一遍
    expect(localStorage.setItem).toHaveBeenCalledWith('bookshelf-sort', '0')
    expect(store.sortedBooks.map((b) => b.name)).toEqual(['斗破苍穹', '红楼梦', '遮天'])
  })

  it('分组上残留的「手动排序」按跟随全局处理', async () => {
    vi.mocked(getBookGroups).mockResolvedValue([
      { groupId: 1, groupName: '玄幻', bookSort: 3 },
    ])
    const store = useBookshelfStore()
    await store.fetchBooks()
    await store.fetchGroups()
    store.setSortMode(5)

    store.activeGroupId = 1
    expect(store.activeGroupSort).toBeNull()
    expect(store.effectiveSortMode).toBe(5)
  })

  it('设置排序时不会写回已下架的值', async () => {
    const store = useBookshelfStore()
    await store.fetchBooks()

    store.setSortMode(3)

    expect(store.sortMode).toBe(0)
    expect(localStorage.setItem).toHaveBeenCalledWith('bookshelf-sort', '0')
  })

  it('让后端跳过目录预抓取, 阅读页才用它', async () => {
    // 阅读页调用时必须带 prefetch: false —— 否则后端会把书架里每本缺目录缓存的
    // 书都联网抓一遍, 和在读章节的正文抢连接池。书架页则保持缺省(undefined),
    // 仍会顺带预热目录缓存。
    const store = useBookshelfStore()

    await store.fetchBooks({ prefetch: false })
    expect(getBookshelfWithCacheInfo).toHaveBeenLastCalledWith({ prefetch: false })

    await store.fetchBooks()
    expect(getBookshelfWithCacheInfo).toHaveBeenLastCalledWith({})
  })
})
