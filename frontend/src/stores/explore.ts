import { defineStore } from 'pinia'
import { ref, computed } from 'vue'
import {
  exploreBook,
  exploreKindChars,
  exploreKindType,
  getExploreKinds,
  runExploreAction,
  type ExploreActionOutcome,
  type ExploreKind,
} from '../api/explore'
import { useSourceStore } from './source'
import type { SearchBook, BookSource } from '../types'
import type { ShowBrowserRequest } from '../api/sourceClick'
import { getInitialExploreCategoryUrl, isJsExploreUrl, parseExploreCategories } from '../utils/exploreCategories'

/**
 * 发现页（对齐 legado ExploreFragment / ExploreAdapter）。
 *
 * 与旧实现的根本差异：`exploreUrl` 求值出的不只是一串"分类链接"，还包含
 * **交互控件**（搜索框 / 下拉筛选 / 按钮 / 开关）。这些控件的取值构成
 * `infoMap`，会被送回书源脚本参与求值 —— 少了这一环，聚合类书源的筛选栏
 * 与刷新按钮在桌面端根本无法工作。
 *
 * 所有 JS 都在后端沙盒执行：WebView 里没有 `java` 对象。
 */

/**
 * action 登记、等待视图执行的意图。
 *
 * 书源的 `java.searchBook(...)` 只表达"我要发起这次搜索"，搜索面板归上层视图
 * 所有，因此 store 把它排进队列，由视图取走后跳转。
 */
export type ExploreIntent =
  | { kind: 'search'; key: string; source: string }
  | { kind: 'open'; openType: string; openKind: string }
  | { kind: 'openBook'; url: string }

/**
 * 是否是"书源自带的搜索控件"。
 *
 * 判定依据是 **action 语义而非标题文字** —— 标题各家写法不一（"🔍搜索"、
 * "搜索"、带 emoji 的变体），而 `java.searchBook(...)` 是稳定的调用约定。
 * 另外把 `type: 'text'` 且标题含"关键词/搜索"的输入框也算进来：它是搜索的
 * 输入框，与按钮成对出现，留下输入框而滤掉按钮只会更费解。
 */
export function isSearchControl(kind: ExploreKind): boolean {
  const action = (kind.action ?? '').trim()
  if (/java\.searchBook\s*\(/.test(action)) return true
  if (/^exploreSearch\s*\(/.test(action)) return true
  // 搜索输入框：它的 action 只把值写进"搜索关键词"这类变量，供搜索按钮读取。
  // 搜索按钮已被滤掉，留着这个输入框只会让用户"输入了没处用"。
  if (/setVariable\s*\(\s*['"](搜索关键词|关键词|searchKey)['"]/.test(action)) return true
  if (!action && exploreKindType(kind) === 'text' && /搜索|关键词|书名/.test(kind.title)) {
    return true
  }
  return false
}

/**
 * 是否是"书源自带的登录控件"。
 *
 * `java.open('login', ...)` 是书源请求打开登录面板的约定写法；发现页改为提供
 * 统一的登录按钮（走 `login_book_source`，与书源管理里那个面板完全一致），
 * 因此把这类控件滤掉，避免同一功能出现两个入口而其中一个没反应。
 */
export function isLoginControl(kind: ExploreKind): boolean {
  const action = (kind.action ?? '').trim()
  return /java\.open\s*\(\s*['"]login['"]/.test(action)
}

/** 解析后端返回的原始分类（保留控件字段，不做结构裁剪）。 */
export function normalizeKinds(raw: unknown): ExploreKind[] {  if (!Array.isArray(raw)) return []
  return raw
    .map((item) => {
      const kind = item as ExploreKind
      return {
        title: String(kind?.title ?? '').trim(),
        url: kind?.url == null ? '' : String(kind.url).trim(),
        type: kind?.type ?? null,
        action: kind?.action ?? null,
        chars: kind?.chars ?? null,
        default: kind?.default ?? null,
        viewName: kind?.viewName ?? null,
        style: kind?.style,
      } satisfies ExploreKind
    })
    .filter((kind) => kind.title)
}

export const useExploreStore = defineStore('explore', () => {
  const sourceStore = useSourceStore()

  const activeSourceUrl = ref<string>('')
  const activeCategoryUrl = ref<string>('')
  const kinds = ref<ExploreKind[]>([])
  const categoriesLoading = ref(false)
  const categoriesError = ref<string | null>(null)
  /** 控件取值（legado `InfoMap`），提交给后端参与 exploreUrl / action 求值。 */
  const infoMap = ref<Record<string, string>>({})
  /** action 登记、等待视图消费的意图（搜索/登录面板）。 */
  const pendingIntents = ref<ExploreIntent[]>([])
  /**
   * action 弹出的面板（`java.startBrowser`）。
   *
   * 「更新书源」「书源设置」这类按钮的全部效果就是这个 HTML 面板；早先后端把
   * 请求丢掉，前端自然点了没反应。
   */
  const pendingBrowser = ref<ShowBrowserRequest | null>(null)
  /** action 产生的提示（`java.toast`），由视图弹 toast。 */
  const pendingMessages = ref<string[]>([])
  /**
   * 登录对话框开关（与「书源管理」里的登录面板是同一个组件）。
   *
   * 发现页提供统一的登录入口：书源自带的登录控件（`java.open('login')`）
   * 只能发一个无效果的意图，不如直接打开真正的登录面板。
   */
  const showLogin = ref(false)

  function openLogin() {
    if (!currentSource.value) return
    showLogin.value = true
  }

  function closeLogin() {
    showLogin.value = false
  }

  const books = ref<SearchBook[]>([])
  const loading = ref(false)
  const page = ref(1)
  const hasMore = ref(true)
  const error = ref<string | null>(null)

  // 停用的书源不出现在发现页：`enabled` 是「书源总开关」，停用后搜索、缓存、
  // 可读书源探测都已把它排除（后端 `list_enabled`），发现页若仍列出它，用户会在
  // 这里选到一个整体已停用的源。
  //
  // 判据用 `enabled !== false` 而非 `=== true`：`enabled` 是 `Option<bool>`，
  // 缺省（`None`/`undefined`）表示启用 —— 与后端 `is_enabled()`（`enabled !=
  // Some(false)`）和书源管理页的复选框（`:checked="source.enabled !== false"`）
  // 保持一致。用严格 `=== true` 会把所有没写过该字段的书源一并隐藏。
  const exploreSources = computed(() => {
    return sourceStore.sources.filter(
      (s: BookSource) => s.enabled !== false && s.enabledExplore && s.exploreUrl,
    )
  })

  const currentSource = computed(() => {
    return sourceStore.sources.find((s: BookSource) => s.bookSourceUrl === activeSourceUrl.value)
  })

  /** 可点击的分类（url 型且 url 非空）；控件与分组标题不在内。 */
  const categories = computed(() => {
    return kinds.value.filter(
      (kind) => exploreKindType(kind) === 'url' && (kind.url ?? '').trim().length > 0,
    )
  })

  /**
   * 交互控件（button/text/toggle/select），按后端给出的顺序渲染。
   *
   * 刻意滤掉两类书源自带控件：
   * - **搜索控件**：书源的 `java.searchBook(...)` 与桌面端「书架搜索」是两套
   *   互不相通的入口，点了要么报"请输入关键词"要么跳转错页，用户用不上。
   * - **登录控件**：发现页统一提供自己的登录按钮（见 `loginVisible`），
   *   书源那个只发 `java.open('login',...)` 意图，没有可见效果。
   */
  const controls = computed(() => {
    return kinds.value.filter((kind) => {
      if (exploreKindType(kind) === 'url') return false
      if (isSearchControl(kind)) return false
      if (isLoginControl(kind)) return false
      return true
    })
  })

  /** 分组标题等占位项（url 型但无 url）。 */
  const sections = computed(() => {
    return kinds.value.filter(
      (kind) => exploreKindType(kind) === 'url' && (kind.url ?? '').trim().length === 0,
    )
  })

  /**
   * 分类栏的完整渲染序列：**按后端给出的原顺序**保留分组标题与可点分类。
   *
   * 有的源几十项里绝大多数是「🏆 排行榜」「📚 分类」这样的分组标题，
   * 14 个是分组。只渲染可点分类会把它们压成一排，用户看不出分类属于哪一组。
   */
  const categoryItems = computed(() => {
    return kinds.value.filter((kind) => exploreKindType(kind) === 'url')
  })

  function optionsOf(kind: ExploreKind): string[] {
    return exploreKindChars(kind)
  }

  /** 控件当前显示值：优先 infoMap，其次 default，最后首个选项。 */
  function valueOf(kind: ExploreKind): string {
    const saved = infoMap.value[kind.title]
    if (saved !== undefined && saved !== '') return saved
    const fallback = kind.default ?? optionsOf(kind)[0] ?? ''
    return fallback
  }

  /** 竞态令牌：快速切源时丢弃过期结果。 */
  let categoriesToken = 0

  /**
   * 拉取分类与控件。
   *
   * 优先本地解析纯文本/JSON 型（没有 action/chars 概念，省一次 IPC）。但**本地
   * 解析不出任何分类时必须回退后端** —— 有一类规则没有 `@js:` 前缀却同样不是
   * 合法 JSON：
   *
   *   `[{'title':'我的书架','url':'..','style':{layout_flexGrow: 1}}, ...]`
   *
   * 单引号 + 无引号键名（这一类字符串）。`JSON.parse` 抛错后本地只会退回 `::`
   * 文本切分，结果 0 个分类，界面显示「该源暂无可用的分类」。legado 用 GSON 的
   * 宽松模式能读，所以手机端正常。后端会把它交给 rquickjs 求值成 JSON。
   *
   * 用「解析结果为空」而不是「像不像 JS 字面量」做判据：书源写法千奇百怪，
   * 启发式规则总会漏；结果为空才回退，既覆盖全部情况，也不给正常书源加 IPC。
   */
  async function loadKinds(source: BookSource): Promise<ExploreKind[]> {
    if (!isJsExploreUrl(source.exploreUrl)) {
      const local = parseExploreCategories(source.exploreUrl)
      if (local.length > 0) {
        return local.map(
          (category) => ({ title: category.title, url: category.url, type: 'url' }) satisfies ExploreKind,
        )
      }
    }

    // 只提交有值的项，避免把空串当作用户选择覆盖书源默认值。
    const payload: Record<string, string> = {}
    for (const [key, value] of Object.entries(infoMap.value)) {
      if (value !== '') payload[key] = value
    }
    const raw = await getExploreKinds({ bookSourceUrl: source.bookSourceUrl, infoMap: payload })
    return normalizeKinds(raw)
  }

  /**
   * 用后端回传的默认值补齐 infoMap。
   *
   * 书源的 exploreUrl 常把"发现页来源=源甲"这类默认值写进 infoMap，后续 action
   * 依赖这些值；只在本地为空时填，避免覆盖用户已做的选择。
   */
  function applyDefaults(next: ExploreKind[]) {
    const merged = { ...infoMap.value }
    for (const kind of next) {
      if (exploreKindType(kind) === 'url') continue
      const current = merged[kind.title]
      if (current === undefined || current === '') {
        const value = kind.default ?? optionsOf(kind)[0]
        if (value) merged[kind.title] = value
      }
    }
    infoMap.value = merged
  }

  async function refreshCategories(sourceChanged: boolean) {
    const source = currentSource.value
    if (!source) {
      categoriesToken += 1
      kinds.value = []
      categoriesError.value = null
      categoriesLoading.value = false
      activeCategoryUrl.value = ''
      books.value = []
      hasMore.value = false
      return
    }

    const token = ++categoriesToken
    categoriesLoading.value = true
    categoriesError.value = null

    let next: ExploreKind[] = []
    let failure: string | null = null
    try {
      next = await loadKinds(source)
    } catch (err: unknown) {
      failure = (err as Error)?.message || '发现分类加载失败'
    }
    // 期间用户又切了源：本轮结果（含错误）一并作废。
    if (token !== categoriesToken) return

    categoriesLoading.value = false
    categoriesError.value = failure
    kinds.value = next
    if (!failure) applyDefaults(next)

    const firstCategoryUrl = getInitialExploreCategoryUrl(
      next
        .filter((kind) => exploreKindType(kind) === 'url')
        .map((kind) => ({ title: kind.title, url: kind.url ?? '' })),
    )
    if (!firstCategoryUrl) {
      // 没有可点分类：失效在途的列表请求，否则它回来时会把书推进空列表。
      booksToken += 1
      loading.value = false
      activeCategoryUrl.value = ''
      books.value = []
      hasMore.value = false
      return
    }

    const activeCategoryStillValid = next.some(
      (kind) => exploreKindType(kind) === 'url' && (kind.url ?? '').trim() === activeCategoryUrl.value,
    )
    if (sourceChanged || !activeCategoryStillValid) {
      activeCategoryUrl.value = firstCategoryUrl
      await resetAndFetch()
    }
  }

  /**
   * 执行控件 action 并把副作用落到本 store。
   *
   * 后端只回传"意图"（刷新发现页 / 打开登录 / 发起搜索），真正的状态变更与
   * 路由跳转必须在前端做 —— 这也是 legado 里 `java.refreshExplore()` 等
   * 由宿主 UI 层响应的原因。
   */
  async function runAction(kind: ExploreKind, value?: string): Promise<ExploreActionOutcome | null> {
    const source = currentSource.value
    if (!source) return null

    if (value !== undefined) infoMap.value = { ...infoMap.value, [kind.title]: value }
    const action = (kind.action ?? '').trim()

    // action 可为空（legado 允许）：此时控件只负责写 infoMap，但**仍要刷新
    // 发现页** —— exploreUrl 会按新取值重新生成分类，否则用户改了下拉框却
    // 什么也没发生。书源自己配了 action 时由 action 决定是否刷新
    //（它通常以 java.refreshExplore() 结尾）。
    if (!action) {
      await refreshCategories(true)
      return null
    }

    categoriesError.value = null
    let outcome: ExploreActionOutcome | null = null
    try {
      const payload: Record<string, string> = {}
      for (const [key, entry] of Object.entries(infoMap.value)) {
        if (entry !== '') payload[key] = entry
      }
      outcome = await runExploreAction({
        bookSourceUrl: source.bookSourceUrl,
        action,
        infoMap: payload,
      })
    } catch (err: unknown) {
      categoriesError.value = (err as Error)?.message || '书源动作执行失败'
      return null
    }

    if (outcome.exploreRefresh) {
      // 刷新发现页：重算分类并回到第一个分类。
      await refreshCategories(true)
    }
    applyIntents(outcome)
    // 面板（更新书源 / 书源设置）与 toast 由视图消费：它们要渲染组件/弹提示，
    // store 只负责把结果放好。
    if (outcome.showBrowser) {
      pendingBrowser.value = outcome.showBrowser
    }
    if (outcome.messages?.length) {
      pendingMessages.value = outcome.messages
    }
    return outcome
  }

  /**
   * 落实 action 登记的意图。
   *
   * `java.searchBook(...)` / `java.open('login',...)` / `java.openBook(...)` 是
   * 书源给宿主的**指令** —— 它们本身不产生任何可见效果，必须由 UI 响应。
   * 少了这一步，书源三的「🔍搜索」与「⚙登录」按钮点了会静默无反应。
   *
   * 有意不做的事：搜索/登录面板由上层（HomeView 等）挂载，这里只把请求写进
   * `pendingIntent` 并返回，避免在 store 里耦合路由与 UI 状态。
   */
  function applyIntents(outcome: ExploreActionOutcome): ExploreIntent[] {
    const handled: ExploreIntent[] = []
    for (const raw of outcome.intents ?? []) {
      const payload = (raw?.payload ?? {}) as Record<string, unknown>
      switch (raw?.kind) {
        case 'search': {
          const key = String(payload.key ?? '').trim()
          if (!key) break
          handled.push({ kind: 'search', key, source: String(payload.source ?? '') })
          break
        }
        case 'open': {
          handled.push({
            kind: 'open',
            openType: String(payload.type ?? ''),
            openKind: String(payload.kind ?? ''),
          })
          break
        }
        case 'openBook': {
          const url = String((raw.payload as unknown) ?? '').trim()
          if (url) handled.push({ kind: 'openBook', url })
          break
        }
        default:
          break
      }
    }

    // 登录刷新（java.reLoginView / source.login / java.open('login')）统一收敛成
    // "打开登录面板"：发现页用的是与书源管理完全相同的那套登录对话框。
    if (outcome.loginRefresh || outcome.login || handled.some((i) => i.kind === 'open' && i.openType === 'login')) {
      showLogin.value = true
    }

    if (handled.length > 0) pendingIntents.value = handled
    return handled
  }

  /** 取出并清空待处理意图（由视图消费）。 */
  function takePendingIntents(): ExploreIntent[] {
    const items = pendingIntents.value
    pendingIntents.value = []
    return items
  }

  /** 取出并清空待弹出的提示。 */
  function takePendingMessages(): string[] {
    const items = pendingMessages.value
    pendingMessages.value = []
    return items
  }

  function closePendingBrowser() {
    pendingBrowser.value = null
  }

  /** 下拉选择：写入 infoMap 后触发 action。 */
  async function selectOption(kind: ExploreKind, value: string) {
    await runAction(kind, value)
  }

  /** 开关：在 chars 里循环切换后触发 action。 */
  async function toggleOption(kind: ExploreKind) {
    const options = optionsOf(kind)
    if (options.length === 0) return
    const current = valueOf(kind)
    const index = options.indexOf(current)
    const next = options[(index + 1) % options.length] ?? options[0]!
    await runAction(kind, next)
  }

  /** 文本框：写入 infoMap，并在书源配置了 action 时执行（书源自带防抖）。 */
  async function setText(kind: ExploreKind, value: string) {
    infoMap.value = { ...infoMap.value, [kind.title]: value }
    if ((kind.action ?? '').trim()) {
      await runAction(kind)
    }
  }

  /** 按钮：直接执行 action。 */
  async function pressButton(kind: ExploreKind) {
    return runAction(kind)
  }

  async function ensureActiveSource() {
    if (exploreSources.value.length === 0) {
      categoriesToken += 1
      activeSourceUrl.value = ''
      activeCategoryUrl.value = ''
      kinds.value = []
      categoriesError.value = null
      categoriesLoading.value = false
      books.value = []
      hasMore.value = false
      return
    }

    const activeSourceStillValid = exploreSources.value.some(
      (source) => source.bookSourceUrl === activeSourceUrl.value,
    )
    if (!activeSourceUrl.value || !activeSourceStillValid) {
      await setSource(exploreSources.value[0].bookSourceUrl)
      return
    }

    if (!kinds.value.length) {
      await refreshCategories(false)
    }
  }

  async function setSource(url: string) {
    const sourceChanged = activeSourceUrl.value !== url
    if (sourceChanged) {
      activeSourceUrl.value = url
      // 切源：先失效在途的列表请求，再清空上一个源的控件取值与列表。
      // 顺序很重要 —— 令牌自增必须发生在清空之前，否则旧响应可能在两步之间落地。
      booksToken += 1
      loading.value = false
      infoMap.value = {}
      books.value = []
      kinds.value = []
      hasMore.value = false
    }
    await refreshCategories(sourceChanged)
  }

  async function setCategory(url: string) {
    const nextUrl = url.trim()
    if (!nextUrl) return
    if (activeCategoryUrl.value !== nextUrl) {
      activeCategoryUrl.value = nextUrl
      await resetAndFetch()
    }
  }

  /**
   * 书籍列表请求令牌。
   *
   * 切源/切分类时自增，在途的响应回来发现令牌不匹配就丢弃。没有这层保护会出现
   * 「第二个源的发现页显示第一个源的书」：
   *   A 源列表在途（`loading = true`）→ 用户切到 B 源 → `resetAndFetch` 里的
   *   `fetchMore` 被 `loading` 挡掉直接返回 → A 源的响应后到，`books.push(...)`
   *   把 A 的书塞进了 B 源的列表。
   */
  let booksToken = 0

  async function resetAndFetch() {
    // 让在途请求失效；同时清掉 loading，否则新源的首次加载会被旧请求的
    // `loading = true` 挡住（这正是竞态的直接成因）。
    booksToken += 1
    loading.value = false
    books.value = []
    page.value = 1
    hasMore.value = true
    error.value = null
    await fetchMore()
  }

  async function fetchMore() {
    if (loading.value || !hasMore.value || !activeSourceUrl.value || !activeCategoryUrl.value) return

    // 快照本次请求的源与分类：响应回来时若已切走，一律丢弃。
    const token = booksToken
    const sourceUrl = activeSourceUrl.value
    const categoryUrl = activeCategoryUrl.value
    const requestPage = page.value

    loading.value = true
    error.value = null
    try {
      const result = await exploreBook({
        bookSourceUrl: sourceUrl,
        ruleFindUrl: categoryUrl,
        page: requestPage,
      })

      // 期间用户切了源/分类：本轮结果作废，且不能碰任何共享状态。
      if (token !== booksToken) return

      if (result && result.length > 0) {
        // 与 legado 的 `SearchBookMergeUtils.appendReplacing` 对齐：
        // 有些站点（含接口型分类）在超出末页后仍返回 200，但内容是
        // **最后一页的重复数据**，只按「有返回」判断会让滚动永远继续。
        // 这里按书源 + bookUrl 去重，只有确实新增了书才允许翻下一页。
        const existing = new Set(
          books.value.map(exploreBookKey).filter((key): key is string => key !== null),
        )
        const appended = result.filter((book) => {
          const key = exploreBookKey(book)
          // 信息不足的书不参与去重：宁可重复展示，也不能误删合法条目。
          if (key === null) return true
          if (existing.has(key)) return false
          existing.add(key)
          return true
        })
        if (appended.length > 0) {
          books.value.push(...appended)
          page.value++
        } else {
          hasMore.value = false
        }
      } else {
        hasMore.value = false
      }
    } catch (err: any) {
      if (token !== booksToken) return
      error.value = err.message || '加载失败'
      hasMore.value = false
    } finally {
      // 同理：过期请求不能把 loading 关掉，那会让新源的加载状态错乱。
      if (token === booksToken) loading.value = false
    }
  }

  async function init() {
    if (sourceStore.sources.length === 0) {
      await sourceStore.fetchSources()
    }
    await ensureActiveSource()
  }

  return {
    activeSourceUrl,
    activeCategoryUrl,
    kinds,
    categories,
    categoryItems,
    controls,
    sections,
    categoriesLoading,
    categoriesError,
    infoMap,
    pendingIntents,
    pendingBrowser,
    pendingMessages,
    takePendingIntents,
    takePendingMessages,
    closePendingBrowser,
    showLogin,
    openLogin,
    closeLogin,
    books,
    loading,
    page,
    hasMore,
    error,
    exploreSources,
    currentSource,
    valueOf,
    optionsOf,
    runAction,
    selectOption,
    toggleOption,
    setText,
    pressButton,
    init,
    setSource,
    setCategory,
    fetchMore,
    resetAndFetch,
  }
})

/** 发现页去重键，等价于 legado `stableSearchBookKey()` 的「书源 + 书籍地址」。 */
function exploreBookKey(book: SearchBook): string | null {
  const source = (book.origin || '').trim() || (book.originName || '').trim()
  const url = (book.bookUrl || '').trim()
  if (url) return `${source}|${url}`
  if ((book.name || '').trim() || (book.author || '').trim()) {
    return `${source}|${book.name || ''}|${book.author || ''}`
  }
  if ((book.coverUrl || '').trim()) return `${source}|${book.coverUrl}`
  return null
}
