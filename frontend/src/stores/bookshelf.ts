import { defineStore } from 'pinia'
import { ref, computed } from 'vue'
import {
  getBookshelfWithCacheInfo,
  getBookGroups,
  deleteBook as apiDeleteBook,
  deleteBooks as apiDeleteBooks,
  saveBookGroupId as apiSaveBookGroupId,
  saveBookGroup as apiSaveBookGroup,
  deleteBookGroups as apiDeleteBookGroups,
  saveBooks as apiSaveBooks,
} from '../api/bookshelf'
import type { Book, BookGroup, SearchBook } from '../types'
import { clearGroupsFromBook } from '../utils/bookGroupBits'
import { clearRecentReadBooks, getRecentReadBookKey, loadRecentReadBooks, removeRecentReadBook } from '../utils/recentBooks'

export const useBookshelfStore = defineStore('bookshelf', () => {
  // ─── Bookshelf ───
  const books = ref<Book[]>([])
  const recentBooks = ref<Book[]>([])
  const loading = ref(false)
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
    await refreshRecentBooks()
  }

  async function removeBook(book: Book) {
    await apiDeleteBook(book)
    books.value = books.value.filter((b) => b.bookUrl !== book.bookUrl)
    await refreshRecentBooks()
  }

  // ─── Groups ───
  const groups = ref<BookGroup[]>([])
  const activeGroupId = ref<number>(-1) // -1 = all

  const displayGroups = computed(() => {
    const all: BookGroup = { groupId: -1, groupName: '全部' }
    const ungrouped: BookGroup = { groupId: 0, groupName: '未分组' }
    return [all, ...groups.value, ungrouped]
  })

  const filteredBooks = computed(() => {
    if (activeGroupId.value === -1) return books.value
    if (activeGroupId.value === 0) {
      return books.value.filter((b) => !b.group || b.group === 0)
    }
    return books.value.filter(
      (b) => b.group && (b.group & activeGroupId.value) !== 0
    )
  })

  async function fetchGroups() {
    try {
      groups.value = await getBookGroups()
    } catch {
      groups.value = []
    }
    if (
      activeGroupId.value > 0
      && !groups.value.some((group) => group.groupId === activeGroupId.value)
    ) {
      activeGroupId.value = -1
    }
  }

  async function saveGroup(groupName: string, groupId = 0) {
    await apiSaveBookGroup({
      groupId,
      groupName,
      orderNo: groups.value.length,
    })
    await fetchGroups()
    return groups.value.find((group) => group.groupName === groupName)?.groupId || groupId
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

  async function bulkSetGroup(groupId: number) {
    const urls = Array.from(selectedBookUrls.value)
    // 并发设置分组, 避免选中 50 本书时串行等 50 个往返
    await Promise.all(urls.map((url) => apiSaveBookGroupId(url, groupId)))
    // Refresh to get updated groups
    await fetchBooks()
    clearSelection()
  }

  async function reorderBooks(draggedUrl: string, targetUrl: string) {
    if (!draggedUrl || !targetUrl || draggedUrl === targetUrl) return

    const snapshot = books.value.slice()
    const fromIndex = snapshot.findIndex((book) => book.bookUrl === draggedUrl)
    const toIndex = snapshot.findIndex((book) => book.bookUrl === targetUrl)
    if (fromIndex === -1 || toIndex === -1 || fromIndex === toIndex) return

    const next = snapshot.slice()
    const [moved] = next.splice(fromIndex, 1)
    next.splice(toIndex, 0, moved)

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

  async function moveBookToFront(bookUrl: string) {
    if (!bookUrl || books.value.length <= 1) return

    const snapshot = books.value.slice()
    const fromIndex = snapshot.findIndex((book) => book.bookUrl === bookUrl)
    if (fromIndex <= 0) return

    const next = snapshot.slice()
    const [moved] = next.splice(fromIndex, 1)
    next.unshift(moved)

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
    books, recentBooks, loading, sorting,
    fetchBooks, removeBook,
    refreshRecentBooks, removeRecentBook, clearAllRecentBooks,
    groups, activeGroupId, displayGroups, filteredBooks,
    fetchGroups, saveGroup, removeGroup, removeGroups,
    searchResults, isSearching, searchKey,
    searchScope, searchGroup, searchSourceUrl, startSearch, clearSearch, isSearchMode,
    editMode,
    selectedBookUrls, toggleSelection, selectAll, clearSelection,
    bulkDelete, bulkSetGroup, setBookGroup, reorderBooks, moveBookToFront,
  }
})
