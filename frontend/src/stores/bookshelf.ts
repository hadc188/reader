import { defineStore } from 'pinia'
import { ref, computed } from 'vue'
import {
  getBookshelfWithCacheInfo,
  getBookGroups,
  deleteBook as apiDeleteBook,
  deleteBooks as apiDeleteBooks,
  saveBook as apiSaveBook,
  saveBookGroupId as apiSaveBookGroupId,
  saveBookGroup as apiSaveBookGroup,
  deleteBookGroups as apiDeleteBookGroups,
  saveBooks as apiSaveBooks,
} from '../api/bookshelf'
import type { Book, BookGroup, SearchBook } from '../types'
import { clearGroupsFromBook } from '../utils/bookGroupBits'
import {
  DEFAULT_BOOK_SORT,
  applyManualOrder,
  isRetiredBookSort,
  normalizeBookSort,
  sortBooks,
  toAvailableBookSort,
} from '../utils/bookshelfSort'
import { clearRecentReadBooks, getRecentReadBookKey, loadRecentReadBooks, removeRecentReadBook } from '../utils/recentBooks'

/** 书架排序方式的存储键。与 Legado 的 `bookshelfSort` 偏好同名同义。 */
const BOOK_SORT_KEY = 'bookshelf-sort'

/** 读取持久化的排序方式; 存储不可用或值非法时回落到默认。
 *
 *  「手动排序」已从菜单下架, 存量的 3 在这里直接迁移到默认档并写回 —— 否则
 *  用户会卡在一个菜单里选不出来的状态(书架按 order 排, 却无法再选中它)。 */
function readStoredSort(): number {
  try {
    const raw = localStorage.getItem(BOOK_SORT_KEY)
    const sort = toAvailableBookSort(raw)
    if (raw !== null && normalizeBookSort(raw) !== sort) {
      writeStoredSort(sort)
    }
    return sort
  } catch {
    return DEFAULT_BOOK_SORT
  }
}

/** 写入排序方式; 存储不可用时只保留内存态, 不影响本次会话。 */
function writeStoredSort(value: number) {
  try {
    localStorage.setItem(BOOK_SORT_KEY, String(value))
  } catch {
    // Ignore unavailable storage.
  }
}

export const useBookshelfStore = defineStore('bookshelf', () => {
  // ─── Bookshelf ───
  const books = ref<Book[]>([])
  const recentBooks = ref<Book[]>([])
  const loading = ref(false)
  /** 是否成功拉过一次书架。
   *
   *  给阅读页的「加入书架」入口用：`books` 为空既可能是「书架真的空」，也可能
   *  是「还没加载」。只看 `books.length` 会把后者当成前者，对一本其实已在书架
   *  的书显示「加入书架」，点下去是一次覆盖写。 */
  const loaded = ref(false)
  const sorting = ref(false)

  async function refreshRecentBooks() {
    const shelfMap = new Map(books.value.map((book) => [getRecentReadBookKey(book), book]))
    recentBooks.value = loadRecentReadBooks().map((entry) => {
      const shelfBook = shelfMap.get(getRecentReadBookKey(entry))
      const merged = shelfBook
        ? {
            ...entry,
            ...shelfBook,
            recentReadAt: entry.recentReadAt,
            durChapterTime: entry.recentReadAt,
          }
        : entry
      return merged
    })
  }

  async function removeRecentBook(book: Pick<Book, 'bookUrl' | 'origin'>) {
    removeRecentReadBook(book)
    await refreshRecentBooks()
  }

  async function clearAllRecentBooks() {
    clearRecentReadBooks()
    await refreshRecentBooks()
  }

  async function fetchBooks() {
    loading.value = true
    try {
      await loadBooks()
    } finally {
      loading.value = false
    }
  }

  /// 共用的书架加载逻辑: 读取书架及应用本地缓存信息后刷新最近阅读。
  async function loadBooks() {
    books.value = await getBookshelfWithCacheInfo()
    // 只有真正拿到数据才算「已加载」: 抛错时保持 false, 阅读页便不会据此判定。
    loaded.value = true
    await refreshRecentBooks()
  }

  async function removeBook(book: Book) {
    await apiDeleteBook(book)
    books.value = books.value.filter((b) => b.bookUrl !== book.bookUrl)
    await refreshRecentBooks()
  }

  /** 加入书架并立刻刷新列表。
   *
   *  阅读页的「加入书架」用这个而不是裸调 `saveBook`：加完之后 `books` 必须
   *  同步更新，否则 `canAddToShelf` 仍会认为「未上架」，入口要等下次重新拉
   *  书架才消失，看起来像没生效。 */
  async function addBook(book: Book) {
    await apiSaveBook(book)
    await fetchBooks()
  }

  // ─── Groups ───
  const groups = ref<BookGroup[]>([])
  const activeGroupId = ref<number>(-1) // -1 = all

  /** 内置标签名: 同名分组再出现一次同样是重复项。 */
  const BUILTIN_GROUP_NAMES = new Set(['全部', '未分组'])

  const displayGroups = computed(() => {
    const all: BookGroup = { groupId: -1, groupName: '全部' }
    const ungrouped: BookGroup = { groupId: 0, groupName: '未分组' }
    // 只展示正 id 的真实分组: Legado 的保留分组(-1 全部 / -2 本地 / -3 音频 /
    // -4 网络未分组 / -5 本地未分组 / -6 视频 / -11 更新失败)是查询投影而非位域,
    // `filteredBooks` 的 `group & groupId` 对负数恒为 0, 点进去永远是空书架;
    // id 为 -1 的那条还会与上面内置的「全部」并排重复。
    //
    // 只在展示层过滤, 不从 store 里删数据 —— 分组本身仍要原样保留:
    // 备份恢复与写回要把 Legado 的完整分组表还给对方。
    return [all, ...visibleGroups.value, ungrouped]
  })

  /** 可被用户管理的真实分组: 正 id 且不与内置标签重名。
   *
   *  分组管理弹窗用的是这一份而不是 `groups` —— 否则用户在弹窗里会看到
   *  「全部 / 本地 / 音频」这些 Legado 虚拟分组, 还能把它们删掉, 删了就直接
   *  污染备份数据; 而标签栏里又看不到它们, 前后不一致。 */
  const visibleGroups = computed(() => (
    groups.value.filter((group) => (
      group.groupId > 0 && !BUILTIN_GROUP_NAMES.has(group.groupName)
    ))
  ))

  const filteredBooks = computed(() => {
    if (activeGroupId.value === -1) return books.value
    if (activeGroupId.value === 0) {
      return books.value.filter((b) => !b.group || b.group === 0)
    }
    return books.value.filter(
      (b) => b.group && (b.group & activeGroupId.value) !== 0
    )
  })

  // ─── 书架排序 ───
  /** 全局排序方式(持久化到 localStorage, 对齐 Legado 的 `AppConfig.bookshelfSort`)。 */
  const sortMode = ref(toAvailableBookSort(readStoredSort()))
  /** 当前分组自己的排序方式; 分组没有单独设置时返回 null, 由全局设置兜底。 */
  const activeGroupSort = computed<number | null>(() => {
    const id = activeGroupId.value
    if (id <= 0) return null
    const group = groups.value.find((g) => g.groupId === id)
    // Legado 用 -1 表示「跟随全局」, 缺失/null 同理。
    if (!group || typeof group.bookSort !== 'number' || group.bookSort < 0) return null
    // 分组上若还留着已下架的「手动排序」, 同样按「跟随全局」处理 —— 否则这个
    // 分组会永远按 order 排序, 而菜单里没有能改回来的选项。
    if (isRetiredBookSort(group.bookSort)) return null
    return group.bookSort
  })
  /** 当前实际生效的排序方式: 分组级设置优先于全局。 */
  const effectiveSortMode = computed(() => activeGroupSort.value ?? sortMode.value)

  /** 按当前排序方式排好序的书架(标签栏与书目网格都用它)。
   *
   *  `effectiveSortMode` 已不含下架值; 这里再过一次 `toAvailableBookSort` 是
   *  为了 defensive: 菜单只暴露可选项, 排序计算也只用可选项 —— 「手动排序」的
   *  比较分支保留在 `sortBooks` 里, 供仍需按 order 排列的调用方(如备份往返)使用。 */
  const sortedBooks = computed(() => sortBooks(filteredBooks.value, effectiveSortMode.value))

  function setSortMode(value: number) {
    sortMode.value = toAvailableBookSort(value)
    writeStoredSort(sortMode.value)
  }

  /** 设置当前分组的排序方式。`null` 表示跟随全局(写回 -1, 与 Legado 同义)。 */
  async function setActiveGroupSort(value: number | null) {
    const id = activeGroupId.value
    if (id <= 0) return
    const group = groups.value.find((g) => g.groupId === id)
    if (!group) return
    const nextSort = value === null ? -1 : toAvailableBookSort(value)
    await apiSaveBookGroup({
      groupId: group.groupId,
      groupName: group.groupName,
      orderNo: group.orderNo ?? 0,
      bookSort: nextSort,
    })
    await fetchGroups()
  }

  async function fetchGroups() {
    try {
      groups.value = await getBookGroups()
    } catch {
      groups.value = []
    }
    // 当前选中的分组若已不在标签栏(被删掉, 或本身就是不展示的 Legado 虚拟分组
    // -2 本地 / -3 音频 等), 筛选会指向一个看不到的空列表 —— 回到「全部」。
    if (
      activeGroupId.value !== -1
      && activeGroupId.value !== 0
      && !displayGroups.value.some((group) => group.groupId === activeGroupId.value)
    ) {
      activeGroupId.value = -1
    }
  }

  async function saveGroup(groupName: string, groupId = 0) {
    // 记住刷新前的 id 集 —— 新建后列表里**多出来的**那个才是本次分配的 id。
    // 不能按名字找: 后端不查重名, 同名时 find 命中的是旧分组, 勾选会挂错。
    const knownIds = new Set(groups.value.map((group) => group.groupId))
    await apiSaveBookGroup({
      groupId,
      groupName,
      orderNo: groups.value.length,
    })
    await fetchGroups()
    if (groupId !== 0) return groupId
    return groups.value.find((group) => !knownIds.has(group.groupId))?.groupId ?? 0
  }

  async function removeGroup(groupId: number) {
    await removeGroups([groupId])
  }

  /** 批量删除分组。
   *
   *  除了删分组本身, 还要把书架上各书的 `group` 位域里对应的位清掉并把**这批书
   *  写回后端** —— 原来的单删版本只改了内存, 下次 fetchBooks 会把旧位读回来,
   *  书就继续挂在已删除的分组上。 */
  async function removeGroups(groupIds: number[]) {
    if (!groupIds.length) return
    await apiDeleteBookGroups(groupIds)
    const removed = new Set(groupIds)
    groups.value = groups.value.filter((group) => !removed.has(group.groupId))
    if (removed.has(activeGroupId.value)) activeGroupId.value = -1

    // 位域清理: 只要有任何一本书的归属发生变化就整体写回一次。
    const snapshot = books.value.slice()
    let changed = false
    books.value = books.value.map((book) => {
      const next = clearGroupsFromBook(book, groupIds)
      if (next !== book) changed = true
      return next
    })
    if (!changed) return

    try {
      // save_books 是**整表覆盖**(不是合并), 所以写回的是内存快照。
      await apiSaveBooks(books.value)
    } catch (error) {
      // 必须回滚内存: 否则随后任何一次 reorderBooks / moveBookToFront 也会调
      // apiSaveBooks, 把这个没落盘的清位结果推给后端, 静默改掉书的归属。
      // 分组本身已经删掉了, 所以回滚的只是位域 —— 比留下脏快照安全。
      books.value = snapshot
      throw error
    }
  }

  // ─── Search ───
  const searchResults = ref<SearchBook[]>([])
  const isSearching = ref(false)
  const searchKey = ref('')
  const searchScope = ref<'all' | 'group' | 'source'>('all')
  const searchGroup = ref('')
  const searchSourceUrl = ref('')

  function startSearch(key: string, options: {
    scope?: 'all' | 'group' | 'source'
    group?: string
    sourceUrl?: string
  } = {}) {
    const nextKey = key.trim()
    if (!nextKey) {
      clearSearch()
      return
    }

    searchScope.value = options.scope || 'all'
    searchGroup.value = options.group || ''
    searchSourceUrl.value = options.sourceUrl || ''
    searchKey.value = nextKey
  }

  function clearSearch() {
    searchResults.value = []
    searchKey.value = ''
    isSearching.value = false
    searchScope.value = 'all'
    searchGroup.value = ''
    searchSourceUrl.value = ''
  }

  const isSearchMode = computed(() => searchKey.value.length > 0)

  // ─── Edit mode and Selection ───
  const editMode = ref(false)
  const selectedBookUrls = ref<Set<string>>(new Set())

  function toggleSelection(url: string) {
    // Vue 3 的 ref 对 Set 的 mutation(add/delete)不会触发响应式更新,
    // 必须创建新 Set 重新赋值才能让依赖 selectedBookUrls 的 UI 更新。
    const next = new Set(selectedBookUrls.value)
    if (next.has(url)) {
      next.delete(url)
    } else {
      next.add(url)
    }
    selectedBookUrls.value = next
  }

  function selectAll() {
    selectedBookUrls.value = new Set(filteredBooks.value.map(b => b.bookUrl))
  }

  function clearSelection() {
    selectedBookUrls.value = new Set()
  }

  async function bulkDelete() {
    const toDelete = books.value
      .filter(b => selectedBookUrls.value.has(b.bookUrl))
      .map(b => ({ bookUrl: b.bookUrl, origin: b.origin }))
    
    if (toDelete.length === 0) return
    await apiDeleteBooks(toDelete as Book[])
    books.value = books.value.filter(b => !selectedBookUrls.value.has(b.bookUrl))
    clearSelection()
  }

  /** 单本书移入分组(替换它原有的分组归属)。
   *
   *  与 `bulkSetGroup` 的区别: 批量走的是"编辑模式多选", 而右键菜单的单本操作
   *  发生在**非编辑模式**, 此时 selectedBookUrls 一定是空的 —— 早先两者共用
   *  批量路径, 导致右键「移入分组」什么也没移动, 还提示"成功将 0 本书移至新分组"。
   *  这里用 saveBookGroupId(后端是赋值语义), 只动这一本书。 */
  async function setBookGroup(bookUrl: string, groupId: number) {
    if (!bookUrl) return
    await apiSaveBookGroupId(bookUrl, groupId)
    // 本地先改, 免得等一次全量刷新 —— fetchBooks 之后仍会覆盖为权威值。
    books.value = books.value.map((book) => (
      book.bookUrl === bookUrl ? { ...book, group: groupId } : book
    ))
    await fetchBooks()
  }

  /** 把一组书的 `group` 位域整体设置为 `mask`(勾选式分组编辑器的落盘入口)。
   *
   *  mask 直接是「这些书最终属于哪些分组」的位域: 老位全部失效, 新位以它
   *  为准 —— 勾选即加入、取消勾选即移出(issue #9: 此前完全没有移出入口)。
   *  用 saveBooks 整表覆盖一次, 避免逐本 N 个 IPC 往返。
   *
   *  与 setBookGroup/bulkSetGroup 的「赋值语义」不同, 本方法支持一本书同时
   *  属于多个分组(Legado 位域原生语义), 也就是编辑器里勾选多个分组。 */
  async function setBooksGroupMask(bookUrls: string[], mask: number) {
    if (!bookUrls.length) return
    const snapshot = books.value.slice()
    const next = snapshot.map((book) => (
      bookUrls.includes(book.bookUrl) ? { ...book, group: mask } : book
    ))
    books.value = next
    try {
      await apiSaveBooks(next)
    } catch (error) {
      // 落盘失败必须回滚内存, 理由同 removeGroups 的注释。
      books.value = snapshot
      throw error
    }
    await refreshRecentBooks()
  }

  async function bulkSetGroup(groupId: number) {
    const urls = Array.from(selectedBookUrls.value)
    // 并发设置分组, 避免选中 50 本书时串行等 50 个往返
    await Promise.all(urls.map((url) => apiSaveBookGroupId(url, groupId)))
    // Refresh to get updated groups
    await fetchBooks()
    clearSelection()
  }

  /** 拖拽调整顺序。
   *
   *  以前是直接调换 `books` 数组元素的位置, 顺序只存在于 JSON 数组下标里 ——
   *  从 Legado 恢复的书架没有 order, 一刷新就回到导入顺序。
   *
   *  `order` 是**全局**序号: 必须在全体书上重排再统一编号, 不能只对当前可见
   *  分组编号 —— 那样 `0..n-1` 会与其他分组的序号重复, 「手动排序」下两组书
   *  交叠排列、顺序不稳定。
   *
   *  `filteredBooks` 是 `books` 的子序列且保持相对顺序, 所以「把 A 拖到 B 的
   *  位置」这个操作在全体列表上做与在可见列表上做等价。 */
  async function reorderBooks(draggedUrl: string, targetUrl: string) {
    if (!draggedUrl || !targetUrl || draggedUrl === targetUrl) return

    const snapshot = books.value.slice()
    const manualOrder = sortBooks(books.value, 3)
    const changed = applyManualOrder(manualOrder, draggedUrl, targetUrl, (book) => book.bookUrl)
    if (!changed.length) return

    const changedByUrl = new Map(changed.map((book) => [book.bookUrl, book.order as number]))
    const next = books.value.map((book) => (
      changedByUrl.has(book.bookUrl)
        ? { ...book, order: changedByUrl.get(book.bookUrl) }
        : book
    ))

    books.value = next
    sorting.value = true
    try {
      await apiSaveBooks(next)
    } catch (error) {
      books.value = snapshot
      throw error
    } finally {
      sorting.value = false
    }
  }

  /** 打开一本书时把它排到手动顺序的最前。
   *
   *  同样走「全体重排 + 统一编号」, 序号永远是干净的 0..n-1, 不会像
   *  「最小值减一」那样持续变负。只在「手动排序」下影响可见位置, 其余排序
   *  方式会立刻用各自规则覆盖展示顺序。 */
  async function moveBookToFront(bookUrl: string) {
    if (!bookUrl || books.value.length <= 1) return

    const snapshot = books.value.slice()
    const manualOrder = sortBooks(books.value, 3)
    const fromIndex = manualOrder.findIndex((book) => book.bookUrl === bookUrl)
    if (fromIndex <= 0) return

    const reordered = manualOrder.slice()
    const [moved] = reordered.splice(fromIndex, 1)
    reordered.unshift(moved!)

    // 按 bookUrl 定位新序号, 而不是 `indexOf(book)`: 后者依赖「sortBooks 返回
    // 的是同一批对象引用」这个隐式约定, 哪天比较器改成克隆就会静默失效。
    const indexByUrl = new Map(reordered.map((book, index) => [book.bookUrl, index]))
    const next = books.value.map((book) => {
      const index = indexByUrl.get(book.bookUrl)
      return index === undefined || book.order === index ? book : { ...book, order: index }
    })

    books.value = next
    sorting.value = true
    try {
      await apiSaveBooks(next)
    } catch (error) {
      books.value = snapshot
      throw error
    } finally {
      sorting.value = false
    }
  }

  return {
    books, recentBooks, loading, loaded, sorting,
    fetchBooks, addBook, removeBook,
    refreshRecentBooks, removeRecentBook, clearAllRecentBooks,
    groups, activeGroupId, displayGroups, visibleGroups, filteredBooks,
    sortMode, activeGroupSort, effectiveSortMode, sortedBooks,
    setSortMode, setActiveGroupSort,
    fetchGroups, saveGroup, removeGroup, removeGroups,
    searchResults, isSearching, searchKey,
    searchScope, searchGroup, searchSourceUrl, startSearch, clearSearch, isSearchMode,
    editMode,
    selectedBookUrls, toggleSelection, selectAll, clearSelection,
    bulkDelete, bulkSetGroup, setBookGroup, setBooksGroupMask, reorderBooks, moveBookToFront,
  }
})
