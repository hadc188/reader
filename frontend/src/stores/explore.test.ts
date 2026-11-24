import { beforeEach, describe, expect, it, vi } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'
import { normalizeKinds, useExploreStore } from './explore'
import { useSourceStore } from './source'
import { getBookSources } from '../api/source'
import { exploreBook, exploreKindLabel, getExploreKinds, runExploreAction } from '../api/explore'
import type { BookSource, SearchBook } from '../types'
import type { ExploreActionOutcome } from '../api/explore'

vi.mock('../api/source', () => ({
  getBookSources: vi.fn(),
}))

vi.mock('../api/explore', async () => {
  const actual = await vi.importActual<typeof import('../api/explore')>('../api/explore')
  return {
    // 保留纯函数（exploreKindType / exploreKindChars / exploreKindLabel），只 mock IPC 调用。
    exploreKindType: actual.exploreKindType,
    exploreKindChars: actual.exploreKindChars,
    exploreKindLabel: actual.exploreKindLabel,
    exploreBook: vi.fn(),
    getExploreKinds: vi.fn(),
    runExploreAction: vi.fn(),
  }
})

const getBookSourcesMock = vi.mocked(getBookSources)
const exploreBookMock = vi.mocked(exploreBook)
const getExploreKindsMock = vi.mocked(getExploreKinds)
const runExploreActionMock = vi.mocked(runExploreAction)

function idleOutcome(overrides: Partial<ExploreActionOutcome> = {}): ExploreActionOutcome {
  return {
    value: '',
    exploreRefresh: false,
    loginRefresh: false,
    login: false,
    intents: [],
    values: {},
    logs: [],
    ...overrides,
  }
}

describe('explore store', () => {
  beforeEach(() => {
    setActivePinia(createPinia())
    getBookSourcesMock.mockReset()
    exploreBookMock.mockReset()
    getExploreKindsMock.mockReset()
    runExploreActionMock.mockReset()
    exploreBookMock.mockResolvedValue([])
    getExploreKindsMock.mockResolvedValue([])
    runExploreActionMock.mockResolvedValue(idleOutcome())
  })

  it('repairs a stale active source when explore sources are already loaded', async () => {
    const sourceStore = useSourceStore()
    sourceStore.sources = [sourceWithExplore()]
    const store = useExploreStore()
    store.activeSourceUrl = 'https://missing.example'

    await store.init()

    expect(store.activeSourceUrl).toBe('https://m.epsilon.example.com')
    expect(store.categories.map((category) => category.title)).toEqual(['书 库', '排 行'])
    expect(store.activeCategoryUrl).toBe('/book/category/catalog.html')
  })

  // 纯文本/JSON 型规则前端自己解析即可，不该为它多跑一次 IPC。
  it('parses plain-text rules locally without calling the backend', async () => {
    const sourceStore = useSourceStore()
    sourceStore.sources = [sourceWithExplore()]
    const store = useExploreStore()

    await store.init()

    expect(getExploreKindsMock).not.toHaveBeenCalled()
    expect(store.activeCategoryUrl).toBe('/book/category/catalog.html')
  })

  // 裸 JS 对象字面量：单引号 + 无引号键名，不是合法 JSON。
  // 本地 JSON.parse 抛错后只会退回 `::` 文本切分，得到 0 个分类，界面显示
  // 「该源暂无可用的分类」。必须在本地解析为空时回退后端求值。
  it('falls back to the backend when local parsing yields no categories', async () => {
    const literal = "[{'title':'我的书架','url':'https://books.example.com/j/index/bookshelf',"
      + "'style':{layout_flexGrow: 1,layout_flexBasisPercent:0.4}},"
      + "{'title':'今日限免','url':'https://books.example.com/j/index/featured/?start=6'}]"
    const sourceStore = useSourceStore()
    sourceStore.sources = [{
      bookSourceName: '示例书源',
      bookSourceUrl: 'https://books.example.com',
      enabledExplore: true,
      exploreUrl: literal,
    }]
    getExploreKindsMock.mockResolvedValue([
      { title: '我的书架', url: 'https://books.example.com/j/index/bookshelf' },
      { title: '今日限免', url: 'https://books.example.com/j/index/featured/?start=6' },
    ])
    const store = useExploreStore()

    await store.init()

    expect(getExploreKindsMock).toHaveBeenCalled()
    expect(store.categories.map((c) => c.title)).toEqual(['我的书架', '今日限免'])
    expect(store.activeCategoryUrl).toBe('https://books.example.com/j/index/bookshelf')
    expect(store.categoriesError).toBeNull()
  })

  // 本地能解析出分类时不该多跑一次 IPC（保持原有优化）。
  it('does not call the backend when local parsing succeeds', async () => {
    const sourceStore = useSourceStore()
    sourceStore.sources = [sourceWithExplore()]
    const store = useExploreStore()

    await store.init()

    expect(getExploreKindsMock).not.toHaveBeenCalled()
  })

  it('asks the backend for js explore rules and uses its categories', async () => {
    getExploreKindsMock.mockResolvedValue([
      { title: '线路', url: '' },
      { title: '小说', url: 'https://api.example.com/list?page={{page}}' },
    ])
    const sourceStore = useSourceStore()
    sourceStore.sources = [jsExploreSource()]
    const store = useExploreStore()

    await store.init()

    expect(getExploreKindsMock).toHaveBeenCalledWith({
      bookSourceUrl: 'https://api.example.com',
      infoMap: {},
    })
    expect(store.categories.map((category) => category.title)).toEqual(['小说'])
    expect(store.activeCategoryUrl).toBe('https://api.example.com/list?page={{page}}')
  })

  it('reports an error instead of a blank page when the js rule fails', async () => {
    getExploreKindsMock.mockRejectedValue(new Error('书源解析失败: boom'))
    const sourceStore = useSourceStore()
    sourceStore.sources = [jsExploreSource()]
    const store = useExploreStore()

    await store.init()

    expect(store.categories).toEqual([])
    expect(store.categoriesError).toContain('boom')
    expect(store.activeCategoryUrl).toBe('')
  })

  // 刻意不做长期缓存：发现页 JS 依赖源变量（线路/发现页来源），前端拿不到，
  // 缓存无法正确失效，宁可每次重新求值。这里锁住该语义，防止后人加回缓存。
  it('re-evaluates js categories on every load instead of caching them', async () => {
    getExploreKindsMock.mockResolvedValue([{ title: '旧分类', url: '/old' }])
    const sourceStore = useSourceStore()
    sourceStore.sources = [jsExploreSource()]
    const store = useExploreStore()

    await store.init()
    expect(store.categories.map((c) => c.title)).toEqual(['旧分类'])

    getExploreKindsMock.mockResolvedValue([{ title: '新分类', url: '/new' }])
    await store.setSource('https://api.example.com')

    expect(store.categories.map((c) => c.title)).toEqual(['新分类'])
    expect(store.activeCategoryUrl).toBe('/new')
    expect(getExploreKindsMock).toHaveBeenCalledTimes(2)
  })
})

// ── 交互控件（本轮重构的核心） ──

describe('explore controls', () => {
  beforeEach(() => {
    setActivePinia(createPinia())
    getBookSourcesMock.mockReset()
    exploreBookMock.mockReset()
    getExploreKindsMock.mockReset()
    runExploreActionMock.mockReset()
    exploreBookMock.mockResolvedValue([])
    runExploreActionMock.mockResolvedValue(idleOutcome())
  })

  it('separates controls, categories and section headers', async () => {
    getExploreKindsMock.mockResolvedValue([
      { title: '源站', type: 'select', chars: ['源甲', '源乙'], default: '源甲', url: '' },
      { title: '📖模式', type: 'toggle', chars: ['小说', '听书'], default: '小说', url: '' },
      { title: '排行榜', type: 'url', url: '' },
      { title: '总榜', type: 'url', url: '/rank' },
    ])
    const sourceStore = useSourceStore()
    sourceStore.sources = [jsExploreSource()]
    const store = useExploreStore()

    await store.init()

    expect(store.controls.map((c) => c.title)).toEqual(['源站', '📖模式'])
    expect(store.sections.map((c) => c.title)).toEqual(['排行榜'])
    expect(store.categories.map((c) => c.title)).toEqual(['总榜'])
  })

  // 书源的 exploreUrl 会把默认值写进 infoMap，后续 action 依赖它们。
  it('applies control defaults into infoMap', async () => {
    getExploreKindsMock.mockResolvedValue([
      { title: '源站', type: 'select', chars: ['源甲', '源乙'], default: '源甲', url: '' },
      { title: '频道', type: 'select', chars: ['男频', '女频'], url: '' },
      { title: '总榜', type: 'url', url: '/rank' },
    ])
    const sourceStore = useSourceStore()
    sourceStore.sources = [jsExploreSource()]
    const store = useExploreStore()

    await store.init()

    expect(store.infoMap['源站']).toBe('源甲')
    // 无 default 时取首个选项
    expect(store.infoMap['频道']).toBe('男频')
    // 初次求值不该把默认值回传（避免覆盖书源自身逻辑）
    expect(getExploreKindsMock).toHaveBeenCalledWith({
      bookSourceUrl: 'https://api.example.com',
      infoMap: {},
    })
  })

  it('sends the selected value and existing infoMap when running an action', async () => {
    getExploreKindsMock.mockResolvedValue([
      { title: '源站', type: 'select', chars: ['源甲', '源乙'], default: '源甲', action: 'onPick()', url: '' },
      { title: '总榜', type: 'url', url: '/rank' },
    ])
    const sourceStore = useSourceStore()
    sourceStore.sources = [jsExploreSource()]
    const store = useExploreStore()
    await store.init()

    const ctrl = store.controls[0]!
    await store.selectOption(ctrl, '源乙')

    expect(store.infoMap['源站']).toBe('源乙')
    expect(runExploreActionMock).toHaveBeenCalledWith({
      bookSourceUrl: 'https://api.example.com',
      action: 'onPick()',
      infoMap: { 源站: '源乙' },
    })
  })

  // 没配 action 的下拉框只写 infoMap，但必须重算分类 —— 否则用户改了下拉
  // 什么都不发生（exploreUrl 才是按取值生成分类的地方）。
  it('reloads categories when a control has no action', async () => {
    getExploreKindsMock.mockResolvedValue([
      { title: '源站', type: 'select', chars: ['源甲', '源乙'], default: '源甲', url: '' },
      { title: '总榜', type: 'url', url: '/rank' },
    ])
    const sourceStore = useSourceStore()
    sourceStore.sources = [jsExploreSource()]
    const store = useExploreStore()
    await store.init()

    getExploreKindsMock.mockResolvedValue([{ title: '聚合榜', type: 'url', url: '/qm' }])
    await store.selectOption(store.controls[0]!, '源乙')

    expect(runExploreActionMock).not.toHaveBeenCalled()
    // 第二次求值要带上用户选中的值
    expect(getExploreKindsMock).toHaveBeenLastCalledWith({
      bookSourceUrl: 'https://api.example.com',
      infoMap: { 源站: '源乙' },
    })
    expect(store.categories.map((c) => c.title)).toEqual(['聚合榜'])
  })

  // java.refreshExplore() → 重新拉分类（带当前 infoMap）
  it('refreshes categories when the action reports exploreRefresh', async () => {
    getExploreKindsMock.mockResolvedValue([
      { title: '总榜', type: 'url', url: '/rank' },
      { title: '🔄刷新', type: 'button', action: 'java.refreshExplore()' },
    ])
    const sourceStore = useSourceStore()
    sourceStore.sources = [jsExploreSource()]
    const store = useExploreStore()
    await store.init()

    runExploreActionMock.mockResolvedValue(idleOutcome({ exploreRefresh: true }))
    getExploreKindsMock.mockResolvedValue([{ title: '新榜', type: 'url', url: '/new' }])

    await store.pressButton(store.controls[0]!)

    expect(getExploreKindsMock).toHaveBeenCalledTimes(2)
    expect(store.categories.map((c) => c.title)).toEqual(['新榜'])
    expect(store.activeCategoryUrl).toBe('/new')
  })

  it('cycles toggle options through chars', async () => {
    getExploreKindsMock.mockResolvedValue([
      { title: '男频', type: 'toggle', chars: ['男频', '女频'], default: '男频', action: 'onToggle()' },
      { title: '总榜', type: 'url', url: '/rank' },
    ])
    const sourceStore = useSourceStore()
    sourceStore.sources = [jsExploreSource()]
    const store = useExploreStore()
    await store.init()

    const ctrl = store.controls[0]!
    expect(store.valueOf(ctrl)).toBe('男频')
    await store.toggleOption(ctrl)
    expect(store.infoMap['男频']).toBe('女频')
    await store.toggleOption(ctrl)
    expect(store.infoMap['男频']).toBe('男频')
  })

  // 书源的搜索控件与桌面端「书架搜索」是两套不相通的入口，用户明确反馈
  // "用不上而且有点bug"，因此不再渲染。判定依据是 action 语义而非标题文字。
  it('hides source-provided search controls', async () => {
    getExploreKindsMock.mockResolvedValue([
      { title: '☃关键词', type: 'text', url: '' },
      { title: '🔍搜索', type: 'button', action: "java.searchBook(infoMap['☃关键词'] || '')" },
      { title: '搜索', type: 'button', action: 'exploreSearch()' },
      // 聚合源的输入框：action 只把值写进"搜索关键词"变量，供搜索按钮读取。
      // 按钮已被滤掉，输入框必须一起滤掉，否则"输入了没处用"。
      { title: '搜索关键词', type: 'text', action: "setVariable('搜索关键词',(infoMap['搜索关键词'] || ''))" },
      { title: '频道', type: 'select', chars: ['男频', '女频'], default: '男频', url: '' },
      { title: '总榜', type: 'url', url: '/rank' },
    ])
    const sourceStore = useSourceStore()
    sourceStore.sources = [jsExploreSource()]
    const store = useExploreStore()
    await store.init()

    // 只留下真正的筛选控件；搜索输入框与两个搜索按钮都不渲染
    expect(store.controls.map((c) => c.title)).toEqual(['频道'])
    expect(store.categories.map((c) => c.title)).toEqual(['总榜'])
  })

  // 书源自带的登录控件只发一个无效果的意图；发现页改为提供统一的登录按钮，
  // 因此把它滤掉，避免同一功能两个入口、其中一个没反应。
  it('hides source-provided login controls', async () => {
    getExploreKindsMock.mockResolvedValue([
      { title: '⚙登录', type: 'button', action: "java.open('login','http',null)" },
      { title: '频道', type: 'select', chars: ['男频'], default: '男频', url: '' },
    ])
    const sourceStore = useSourceStore()
    sourceStore.sources = [jsExploreSource()]
    const store = useExploreStore()
    await store.init()

    expect(store.controls.map((c) => c.title)).toEqual(['频道'])
  })

  // 非搜索/登录的按钮（如「🔄刷新」）必须保留 —— 过滤不能误伤。
  it('keeps unrelated buttons', async () => {
    getExploreKindsMock.mockResolvedValue([
      { title: '🔄刷新', type: 'button', action: 'java.refreshExplore()' },
      { title: '更新书源', type: 'button', action: 'renderVersionPage()' },
    ])
    const sourceStore = useSourceStore()
    sourceStore.sources = [jsExploreSource()]
    const store = useExploreStore()
    await store.init()

    expect(store.controls.map((c) => c.title)).toEqual(['🔄刷新', '更新书源'])
  })

  // 登录意图（java.open('login') / java.reLoginView）统一打开登录面板。
  it('opens the login panel when an action asks to log in', async () => {
    getExploreKindsMock.mockResolvedValue([
      { title: '重登', type: 'button', action: 'java.reLoginView()' },
      { title: '总榜', type: 'url', url: '/rank' },
    ])
    const sourceStore = useSourceStore()
    sourceStore.sources = [jsExploreSource()]
    const store = useExploreStore()
    await store.init()

    expect(store.showLogin).toBe(false)
    runExploreActionMock.mockResolvedValue(idleOutcome({ loginRefresh: true }))
    await store.pressButton(store.controls[0]!)

    expect(store.showLogin).toBe(true)
  })

  // 「更新书源」「书源设置」的全部效果就是弹一个 HTML 面板。
  // 后端早先把 showBrowser 丢掉了，前端因此点了没反应。
  it('exposes the panel produced by the action', async () => {
    getExploreKindsMock.mockResolvedValue([
      { title: '更新书源', type: 'button', action: 'renderVersionPage()' },
      { title: '总榜', type: 'url', url: '/rank' },
    ])
    const sourceStore = useSourceStore()
    sourceStore.sources = [jsExploreSource()]
    const store = useExploreStore()
    await store.init()

    runExploreActionMock.mockResolvedValue(
      idleOutcome({
        showBrowser: { url: '', panelId: 'panel-1' } as never,
      }),
    )
    await store.pressButton(store.controls[0]!)

    expect(store.pendingBrowser).toMatchObject({ panelId: 'panel-1' })
    store.closePendingBrowser()
    expect(store.pendingBrowser).toBeNull()
  })

  // java.toast 的文案要能传出来（"已是最新配置"这类反馈全靠它）。
  it('exposes toast messages produced by the action', async () => {
    getExploreKindsMock.mockResolvedValue([
      { title: '更新配置', type: 'button', action: 'getCloudSettings(true)' },
      { title: '总榜', type: 'url', url: '/rank' },
    ])
    const sourceStore = useSourceStore()
    sourceStore.sources = [jsExploreSource()]
    const store = useExploreStore()
    await store.init()

    runExploreActionMock.mockResolvedValue(
      idleOutcome({ messages: ['正在更新最新配置', '已是最新配置：20261010'] }),
    )
    await store.pressButton(store.controls[0]!)

    expect(store.takePendingMessages()).toEqual(['正在更新最新配置', '已是最新配置：20261010'])
    expect(store.pendingMessages).toEqual([])
  })


  // 分组标题必须留在分类序列里（有的源几十项里绝大多数是分组标题），
  // 只渲染可点分类会把它们压平，用户看不出分类归属。
  it('keeps section headers in the category render list, in backend order', async () => {
    getExploreKindsMock.mockResolvedValue([
      { title: '🏆 排行榜', type: 'url', url: '' },
      { title: '点击榜', type: 'url', url: '/rank/allvisit' },
      { title: '📚 分类', type: 'url', url: '' },
      { title: '科幻', type: 'url', url: '/c/41' },
      { title: '源站', type: 'select', chars: ['源甲'], url: '' },
    ])
    const sourceStore = useSourceStore()
    sourceStore.sources = [jsExploreSource()]
    const store = useExploreStore()
    await store.init()

    expect(store.categoryItems.map((c) => c.title)).toEqual([
      '🏆 排行榜',
      '点击榜',
      '📚 分类',
      '科幻',
    ])
    // 可点分类与分组各自成集
    expect(store.categories.map((c) => c.title)).toEqual(['点击榜', '科幻'])
    expect(store.sections.map((c) => c.title)).toEqual(['🏆 排行榜', '📚 分类'])
  })

  it('surfaces action failures without breaking the page', async () => {
    getExploreKindsMock.mockResolvedValue([
      { title: '🔄刷新', type: 'button', action: 'boom()' },
      { title: '总榜', type: 'url', url: '/rank' },
    ])
    const sourceStore = useSourceStore()
    sourceStore.sources = [jsExploreSource()]
    const store = useExploreStore()
    await store.init()

    runExploreActionMock.mockRejectedValue(new Error('书源动作执行失败: boom'))
    await store.pressButton(store.controls[0]!)

    expect(store.categoriesError).toContain('boom')
    // 列表仍可用
    expect(store.categories.map((c) => c.title)).toEqual(['总榜'])
  })

  // 切源必须清掉 A 源的筛选条件，否则会把 A 的 infoMap 带进 B 源求值。
  it('clears infoMap when switching sources', async () => {
    getExploreKindsMock.mockResolvedValue([
      { title: '源站', type: 'select', chars: ['源甲', '源乙'], default: '源甲', url: '' },
      { title: '总榜', type: 'url', url: '/rank' },
    ])
    const sourceStore = useSourceStore()
    sourceStore.sources = [jsExploreSource(), otherJsSource()]
    const store = useExploreStore()
    await store.init()
    expect(store.infoMap['源站']).toBe('源甲')

    getExploreKindsMock.mockResolvedValue([{ title: '另一源', type: 'url', url: '/x' }])
    await store.setSource('https://other.example')

    expect(store.infoMap['源站']).toBeUndefined()
    expect(store.activeCategoryUrl).toBe('/x')
  })

})
describe('exploreKindLabel', () => {
  it('prefers the backend-resolved viewName over title', () => {
    // 聚合源把筛选控件显示名写成 `'请选择类型'`
    expect(exploreKindLabel({ title: '平台', viewName: '请选择类型' })).toBe('请选择类型')
  })

  it('falls back to title when viewName is missing or blank', () => {
    expect(exploreKindLabel({ title: '平台' })).toBe('平台')
    expect(exploreKindLabel({ title: '平台', viewName: null })).toBe('平台')
    expect(exploreKindLabel({ title: '平台', viewName: '   ' })).toBe('平台')
  })
})

describe('normalizeKinds', () => {
  it('keeps interactive fields and drops blank titles', () => {
    expect(normalizeKinds([
      { title: '源站', type: 'select', chars: ['源甲'], default: '源甲', action: 'a()', url: '' },
      { title: ' 小说 ', url: ' https://a.example/list ' },
      { title: '', url: '/ignored' },
    ])).toEqual([
      {
        title: '源站',
        url: '',
        type: 'select',
        chars: ['源甲'],
        default: '源甲',
        action: 'a()',
        viewName: null,
        style: undefined,
      },
      {
        title: '小说',
        url: 'https://a.example/list',
        type: null,
        chars: null,
        default: null,
        action: null,
        viewName: null,
        style: undefined,
      },
    ])
  })

  it('tolerates non-array payloads', () => {
    expect(normalizeKinds(null)).toEqual([])
    expect(normalizeKinds(undefined)).toEqual([])
    expect(normalizeKinds({ title: 'not-an-array' })).toEqual([])
  })
})

// ── 切源竞态（本轮修复） ──

describe('explore source switching race', () => {
  beforeEach(() => {
    setActivePinia(createPinia())
    getBookSourcesMock.mockReset()
    exploreBookMock.mockReset()
    getExploreKindsMock.mockReset()
    runExploreActionMock.mockReset()
    exploreBookMock.mockResolvedValue([])
    getExploreKindsMock.mockResolvedValue([])
    runExploreActionMock.mockResolvedValue(idleOutcome())
  })

  /**
   * 用户在 A 源列表还在加载时切到 B 源，最终必须显示 B 源的书。
   *
   * 回归的 bug：`resetAndFetch` 里的 `fetchMore` 会被旧请求设的 `loading = true`
   * 挡掉直接返回，随后 A 源的响应抵达并 `books.push(...)`，于是 B 源的发现页
   * 显示的是 A 源的书。
   */
  it('discards the previous source response when switching mid-flight', async () => {
    let resolveA: (books: SearchBook[]) => void = () => {}
    const sourceA = { name: 'A源的书', author: 'a', bookUrl: '/a/1', origin: 'https://a.example' }
    const bookB = { name: 'B源的书', author: 'b', bookUrl: '/b/1', origin: 'https://b.example' }

    const sourceStore = useSourceStore()
    sourceStore.sources = [
      { bookSourceName: 'A源', bookSourceUrl: 'https://a.example', enabledExplore: true, exploreUrl: JSON.stringify([{ title: 'A列表', url: '/a-list' }]) },
      { bookSourceName: 'B源', bookSourceUrl: 'https://b.example', enabledExplore: true, exploreUrl: JSON.stringify([{ title: 'B列表', url: '/b-list' }]) },
    ]
    const store = useExploreStore()

    // init 会为第一个源发一次请求并等待它，所以首次 A 源必须立即返回；
    // 之后的 A 源请求才挂起，用来模拟"列表加载中"。
    let aCalls = 0
    exploreBookMock.mockImplementation((params) => {
      if (params.bookSourceUrl === 'https://a.example') {
        aCalls += 1
        if (aCalls === 1) return Promise.resolve([])
        return new Promise<SearchBook[]>((resolve) => { resolveA = resolve })
      }
      return Promise.resolve([bookB])
    })

    await store.init()

    // 先切到 B 源（立即成功），再切回 A 源 —— 这样 A 源会重新发请求并挂住。
    await store.setSource('https://b.example')
    void store.setSource('https://a.example')
    await new Promise((resolve) => setTimeout(resolve, 0))

    expect(aCalls).toBe(2) // A 源的第二个请求确实挂着

    // A 源在途时切到 B 源。
    void store.setSource('https://b.example')
    await new Promise((resolve) => setTimeout(resolve, 0))

    // A 源的响应姗姗来迟：必须被丢弃，不能进 B 源的列表。
    resolveA([sourceA])
    await new Promise((resolve) => setTimeout(resolve, 0))

    expect(store.books.map((book) => book.name)).toEqual(['B源的书'])
    expect(store.books.some((book) => book.name === 'A源的书')).toBe(false)
  })

  /**
   * 切源后必须能重新发起列表请求 —— 旧请求的 `loading = true` 不能把它挡住。
   */
  it('starts a fresh request for the new source instead of staying blocked', async () => {
    let resolveA: (books: SearchBook[]) => void = () => {}
    const bookB = { name: 'B源的书', author: 'b', bookUrl: '/b/1', origin: 'https://b.example' }

    const sourceStore = useSourceStore()
    sourceStore.sources = [
      { bookSourceName: 'A源', bookSourceUrl: 'https://a.example', enabledExplore: true, exploreUrl: JSON.stringify([{ title: 'A列表', url: '/a-list' }]) },
      { bookSourceName: 'B源', bookSourceUrl: 'https://b.example', enabledExplore: true, exploreUrl: JSON.stringify([{ title: 'B列表', url: '/b-list' }]) },
    ]
    const store = useExploreStore()

    let aCalls = 0
    exploreBookMock.mockImplementation((params) => {
      if (params.bookSourceUrl === 'https://a.example') {
        aCalls += 1
        if (aCalls === 1) return Promise.resolve([])
        return new Promise<SearchBook[]>((resolve) => { resolveA = resolve })
      }
      return Promise.resolve([bookB])
    })

    await store.init()

    // 制造「A 源列表在途」：先到 B 源，再切回 A 源并让请求挂住。
    await store.setSource('https://b.example')
    void store.setSource('https://a.example')
    await new Promise((resolve) => setTimeout(resolve, 0))
    expect(aCalls).toBe(2) // A 源确实有一个请求挂着

    // 在途时切到 B 源：新源必须能发出自己的请求。
    await store.setSource('https://b.example')

    const bCalls = exploreBookMock.mock.calls.filter(
      (call) => call[0].bookSourceUrl === 'https://b.example',
    )
    // B 源至少被请求过两次（初次进入 + 从 A 源切回），说明没有被旧请求的
    // loading 挡住。
    expect(bCalls.length).toBeGreaterThanOrEqual(2)
    expect(bCalls[bCalls.length - 1][0]).toMatchObject({
      bookSourceUrl: 'https://b.example',
      ruleFindUrl: '/b-list',
    })
    expect(store.books.map((book) => book.name)).toEqual(['B源的书'])

    resolveA([])
    await new Promise((resolve) => setTimeout(resolve, 0))
  })

  /**
   * 过期请求的 `finally` 不能把新源的 `loading` 关掉。
   *
   * 单独锁定这条不变量（不依赖其它清理逻辑）：A 源请求挂起 → 切到 B 源并让 B
   * 的请求也挂起 → 放行 A 的响应 → B 的 `loading` 必须还是 true。
   * 若 `finally` 无条件执行，A 的过期响应会把 B 的加载指示器提前关掉。
   */
  it('does not let a stale response clear the new loading state', async () => {
    let resolveA: (books: SearchBook[]) => void = () => {}

    const sourceStore = useSourceStore()
    sourceStore.sources = [
      { bookSourceName: 'A源', bookSourceUrl: 'https://a.example', enabledExplore: true, exploreUrl: JSON.stringify([{ title: 'A列表', url: '/a-list' }]) },
      { bookSourceName: 'B源', bookSourceUrl: 'https://b.example', enabledExplore: true, exploreUrl: JSON.stringify([{ title: 'B列表', url: '/b-list' }]) },
    ]
    const store = useExploreStore()

    let aCalls = 0
    exploreBookMock.mockImplementation((params) => {
      if (params.bookSourceUrl === 'https://a.example') {
        aCalls += 1
        if (aCalls === 1) return Promise.resolve([])
        return new Promise<SearchBook[]>((resolve) => { resolveA = resolve })
      }
      // B 源永远挂起：它的 loading 必须一直保持到自己的响应回来。
      return new Promise<SearchBook[]>(() => {})
    })

    await store.init()

    // 让 A 源列表在途（B 源同样挂起，所以这两步都不能 await）。
    void store.setSource('https://b.example')
    await new Promise((resolve) => setTimeout(resolve, 0))
    void store.setSource('https://a.example')
    await new Promise((resolve) => setTimeout(resolve, 0))
    void store.setSource('https://b.example')
    await new Promise((resolve) => setTimeout(resolve, 0))

    expect(store.loading).toBe(true) // B 源正在加载

    // A 源的过期响应回来了。
    resolveA([])
    await new Promise((resolve) => setTimeout(resolve, 0))
    await new Promise((resolve) => setTimeout(resolve, 0))

    // B 源仍在加载：过期响应不能改它的状态。
    expect(store.loading).toBe(true)
    expect(store.books).toEqual([])
  })
})

// ── 停用书源不进入发现页 ──

describe('explore source enabled filter', () => {
  beforeEach(() => {
    setActivePinia(createPinia())
    getBookSourcesMock.mockReset()
    exploreBookMock.mockReset()
    getExploreKindsMock.mockReset()
    runExploreActionMock.mockReset()
    exploreBookMock.mockResolvedValue([])
    getExploreKindsMock.mockResolvedValue([])
    runExploreActionMock.mockResolvedValue(idleOutcome())
  })

  it('hides disabled sources from the explore source list', async () => {
    const sourceStore = useSourceStore()
    sourceStore.sources = [
      { bookSourceName: '停用源', bookSourceUrl: 'https://off.example', enabled: false, enabledExplore: true, exploreUrl: '/off-list' },
      { bookSourceName: '启用源', bookSourceUrl: 'https://on.example', enabled: true, enabledExplore: true, exploreUrl: '/on-list' },
    ]
    const store = useExploreStore()

    expect(store.exploreSources.map((s) => s.bookSourceName)).toEqual(['启用源'])
  })

  // `enabled` 是 Option<bool>：没写过该字段（undefined）表示启用。
  // 判据必须是 `!== false`，用严格 `=== true` 会把这类书源一并隐藏。
  it('treats a missing enabled flag as enabled', async () => {
    const sourceStore = useSourceStore()
    sourceStore.sources = [
      { bookSourceName: '未设置', bookSourceUrl: 'https://unset.example', enabledExplore: true, exploreUrl: '/unset-list' },
    ]
    const store = useExploreStore()

    expect(store.exploreSources.map((s) => s.bookSourceName)).toEqual(['未设置'])
  })

  // 停用仍然要看 `enabledExplore`：只停用总开关的源不出现，但停用发现页的源
  // 同样不出现 —— 两个条件都要满足。
  it('still requires enabledExplore', async () => {
    const sourceStore = useSourceStore()
    sourceStore.sources = [
      { bookSourceName: '启用但无发现', bookSourceUrl: 'https://noexp.example', enabled: true, enabledExplore: false, exploreUrl: '/x' },
      { bookSourceName: '正常', bookSourceUrl: 'https://ok.example', enabled: true, enabledExplore: true, exploreUrl: '/ok' },
    ]
    const store = useExploreStore()

    expect(store.exploreSources.map((s) => s.bookSourceName)).toEqual(['正常'])
  })

  // 当前源被停用后自动切到列表里首个可用源，否则界面会停在一个已经不可用的源上。
  it('switches away from a source that becomes disabled', async () => {
    const sourceStore = useSourceStore()
    sourceStore.sources = [
      { bookSourceName: '可用源', bookSourceUrl: 'https://on.example', enabled: true, enabledExplore: true, exploreUrl: JSON.stringify([{ title: '列表', url: '/on-list' }]) },
    ]
    const store = useExploreStore()
    await store.init()
    expect(store.activeSourceUrl).toBe('https://on.example')

    // 该源被停用：再次 init 应把它换掉（列表已无可用源时清空）。
    sourceStore.sources = [
      { bookSourceName: '停用源', bookSourceUrl: 'https://on.example', enabled: false, enabledExplore: true, exploreUrl: JSON.stringify([{ title: '列表', url: '/on-list' }]) },
    ]
    await store.init()

    expect(store.exploreSources).toEqual([])
    expect(store.activeSourceUrl).toBe('')
    expect(store.categories).toEqual([])
  })
})

// ── 分类翻页终止 ──

describe('explore pagination', () => {
  beforeEach(() => {
    setActivePinia(createPinia())
    getBookSourcesMock.mockReset()
    exploreBookMock.mockReset()
    getExploreKindsMock.mockReset()
    runExploreActionMock.mockReset()
    exploreBookMock.mockResolvedValue([])
    getExploreKindsMock.mockResolvedValue([])
    runExploreActionMock.mockResolvedValue(idleOutcome())
  })

  /**
   * 有些接口型分类在越过末页后仍返回 200 和**末页的重复数据**。
   * 只按「返回非空」判断 hasMore 会让无限滚动永远继续、列表里堆满重复项；
   * 与 legado `SearchBookMergeUtils.appendReplacing` 一样，按书源 + 书籍地址
   * 去重后没有新增就必须停。
   */
  it('stops when a page only repeats books already loaded', async () => {
    const sourceStore = useSourceStore()
    sourceStore.sources = [{
      bookSourceName: '分页源',
      bookSourceUrl: 'https://paged.example',
      enabledExplore: true,
      exploreUrl: JSON.stringify([{ title: '列表', url: '/list?page={{page}}' }]),
    }]
    const first = { name: '第一本', author: '作者', bookUrl: '/book/1', origin: 'https://paged.example' }
    const second = { name: '第二本', author: '作者', bookUrl: '/book/2', origin: 'https://paged.example' }
    exploreBookMock
      .mockResolvedValueOnce([first, second])
      .mockResolvedValueOnce([first, second])

    const store = useExploreStore()
    await store.init()
    expect(store.books).toHaveLength(2)
    expect(store.hasMore).toBe(true)

    await store.fetchMore()

    expect(store.books).toHaveLength(2)
    expect(store.hasMore).toBe(false)
  })

  it('keeps loading while pages introduce new books', async () => {
    const sourceStore = useSourceStore()
    sourceStore.sources = [{
      bookSourceName: '分页源',
      bookSourceUrl: 'https://paged.example',
      enabledExplore: true,
      exploreUrl: JSON.stringify([{ title: '列表', url: '/list?page={{page}}' }]),
    }]
    const first = { name: '第一本', author: '作者', bookUrl: '/book/1', origin: 'https://paged.example' }
    const second = { name: '第二本', author: '作者', bookUrl: '/book/2', origin: 'https://paged.example' }
    exploreBookMock
      .mockResolvedValueOnce([first])
      .mockResolvedValueOnce([first, second])

    const store = useExploreStore()
    await store.init()

    await store.fetchMore()

    expect(store.books.map((book) => book.name)).toEqual(['第一本', '第二本'])
    expect(store.hasMore).toBe(true)
  })

  it('requests the next page with the category page template intact', async () => {
    const sourceStore = useSourceStore()
    sourceStore.sources = [{
      bookSourceName: '分页测试源',
      bookSourceUrl: 'https://paged.example.test',
      enabledExplore: true,
      exploreUrl: JSON.stringify([{ title: '分类', url: '/category/{{page}}/' }]),
    }]
    const first = { name: '第一本', author: '作者', bookUrl: '/book/1.html', origin: 'https://paged.example.test' }
    const second = { name: '第二本', author: '作者', bookUrl: '/book/2.html', origin: 'https://paged.example.test' }
    exploreBookMock
      .mockResolvedValueOnce([first])
      .mockResolvedValueOnce([second])

    const store = useExploreStore()
    await store.init()
    await store.fetchMore()

    expect(exploreBookMock).toHaveBeenNthCalledWith(1, {
      bookSourceUrl: 'https://paged.example.test',
      ruleFindUrl: '/category/{{page}}/',
      page: 1,
    })
    expect(exploreBookMock).toHaveBeenNthCalledWith(2, {
      bookSourceUrl: 'https://paged.example.test',
      ruleFindUrl: '/category/{{page}}/',
      page: 2,
    })
  })

  it('reloads the first page and can still load the next page after re-entry', async () => {
    const sourceStore = useSourceStore()
    sourceStore.sources = [{
      bookSourceName: '分页测试源',
      bookSourceUrl: 'https://paged.example.test',
      enabledExplore: true,
      exploreUrl: JSON.stringify([{ title: '分类', url: '/category/{{page}}/' }]),
    }]
    const first = { name: '第一本', author: '作者', bookUrl: '/book/1.html', origin: 'https://paged.example.test' }
    const second = { name: '第二本', author: '作者', bookUrl: '/book/2.html', origin: 'https://paged.example.test' }
    exploreBookMock
      .mockResolvedValueOnce([first])
      .mockResolvedValueOnce([second])
      .mockResolvedValueOnce([first])
      .mockResolvedValueOnce([second])

    const store = useExploreStore()
    await store.init()
    await store.fetchMore()
    expect(store.books.map((book) => book.name)).toEqual(['第一本', '第二本'])

    // ExploreView 重新挂载时会再次执行 init；此时不能再初始化成空列表，
    // 也不能继承上次的末页状态。
    await store.init()
    await store.fetchMore()
    expect(store.loading).toBe(false)
    expect(store.hasMore).toBe(true)
    expect(store.books.map((book) => book.name)).toEqual(['第一本', '第二本'])
  })

  it('keeps retrying after a transient page failure', async () => {
    const sourceStore = useSourceStore()
    sourceStore.sources = [{
      bookSourceName: '分页测试源',
      bookSourceUrl: 'https://paged.example.test',
      enabledExplore: true,
      exploreUrl: JSON.stringify([{ title: '分类', url: '/category/{{page}}/' }]),
    }]
    const first = { name: '第一本', author: '作者', bookUrl: '/book/1.html', origin: 'https://paged.example.test' }
    const second = { name: '第二本', author: '作者', bookUrl: '/book/2.html', origin: 'https://paged.example.test' }
    exploreBookMock
      .mockResolvedValueOnce([first])
      .mockRejectedValueOnce(new Error('temporary network error'))
      .mockResolvedValueOnce([second])

    const store = useExploreStore()
    await store.init()
    await store.fetchMore()
    expect(store.error).toContain('temporary network error')

    await store.fetchMore()
    expect(store.books.map((book) => book.name)).toEqual(['第一本', '第二本'])
  })

  it('does not keep loading blocked after an invalidated first-page request', async () => {
    const sourceStore = useSourceStore()
    sourceStore.sources = [{
      bookSourceName: '分页测试源',
      bookSourceUrl: 'https://paged.example.test',
      enabledExplore: true,
      exploreUrl: JSON.stringify([{ title: '分类', url: '/category/{{page}}/' }]),
    }]
    sourceStore.sources.push({
      bookSourceName: '另一个源',
      bookSourceUrl: 'https://other.example',
      enabledExplore: true,
      exploreUrl: JSON.stringify([{ title: '列表', url: '/other/{{page}}/' }]),
    })
    let resolveFirst!: (books: unknown[]) => void
    exploreBookMock
      .mockImplementationOnce(() => new Promise((resolve) => { resolveFirst = resolve as (books: unknown[]) => void }))
      .mockResolvedValueOnce([
        { name: '第二本', author: '作者', bookUrl: '/book/2.html', origin: 'https://paged.example.test' },
      ])

    const store = useExploreStore()
    const pending = store.init()
    await Promise.resolve()
    await store.setSource('https://other.example')
    resolveFirst([])
    await pending

    expect(store.loading).toBe(false)
    expect(store.hasMore).toBe(true)
    expect(exploreBookMock.mock.calls.length).toBeGreaterThanOrEqual(2)
  })

  it('clears a stale loading flag when the view is initialized again', async () => {
    const sourceStore = useSourceStore()
    sourceStore.sources = [{
      bookSourceName: '分页测试源',
      bookSourceUrl: 'https://paged.example.test',
      enabledExplore: true,
      exploreUrl: JSON.stringify([{ title: '分类', url: '/category/{{page}}/' }]),
    }]
    exploreBookMock.mockResolvedValue([
      { name: '第一本', author: '作者', bookUrl: '/book/1.html', origin: 'https://paged.example.test' },
    ])

    const store = useExploreStore()
    await store.init()
    store.loading = true

    await store.init()

    expect(store.loading).toBe(false)
    expect(exploreBookMock.mock.calls.length).toBeGreaterThanOrEqual(2)
  })
})

function sourceWithExplore(): BookSource {
  return {
    bookSourceName: 'm.epsilon.example.com',
    bookSourceUrl: 'https://m.epsilon.example.com',
    enabledExplore: true,
    exploreUrl: JSON.stringify([
      { style: { layout_flexBasisPercent: 1.0 }, title: '书 库', url: '/book/category/catalog.html' },
      { style: { layout_flexBasisPercent: 0.25 }, title: '排 行', url: '/book/ranking.html' },
    ]),
  }
}

function jsExploreSource(): BookSource {
  return {
    bookSourceName: '聚合源乙',
    bookSourceUrl: 'https://api.example.com',
    enabledExplore: true,
    exploreUrl: '@js:QMA_DISC.explore.call(this)',
  }
}

function otherJsSource(): BookSource {
  return {
    bookSourceName: '聚合源丙',
    bookSourceUrl: 'https://other.example',
    enabledExplore: true,
    exploreUrl: '<js>\nsomeRule()\n</js>',
  }
}
