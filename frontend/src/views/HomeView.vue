<template>
  <div class="home-view">
    <!-- Search Mode -->
    <SearchResults
      v-if="shelfStore.isSearchMode"
      @back="shelfStore.clearSearch()"
    />

    <!-- Normal Bookshelf View -->
    <div v-else class="shelf-content">
      <!-- Shelf Header -->
      <div class="shelf-header">
        <h1 class="shelf-title">
          书架
          <span class="book-count">({{ visibleShelfBooks.length }})</span>
        </h1>
        <div class="shelf-actions">
          <template v-if="shelfStore.editMode">
            <button class="shelf-btn" type="button" title="全选" aria-label="全选" @click="shelfStore.selectAll()">
              <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                <path d="M9 11l3 3L22 4" />
                <path d="M21 12v7a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h11" />
              </svg>
              <span class="shelf-btn-label">全选</span>
            </button>
            <button class="shelf-btn" type="button" title="取消全选" aria-label="取消全选" @click="shelfStore.clearSelection()">
              <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                <rect x="3" y="3" width="18" height="18" rx="2" />
                <path d="M8 12h8" />
              </svg>
              <span class="shelf-btn-label">取消全选</span>
            </button>
          </template>
          <button class="shelf-btn" type="button" title="导入本地书" aria-label="导入本地书" @click="triggerLocalBookUpload" :disabled="localBookUploading">
            <svg v-if="!localBookUploading" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
              <path d="M12 3v12" />
              <path d="m7 8 5-5 5 5" />
              <path d="M5 21h14" />
            </svg>
            <svg v-else class="spinning" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
              <path d="M21 12a9 9 0 0 0-15.55-6.2L3 8" />
              <path d="M3 3v5h5" />
            </svg>
            <span class="shelf-btn-label">{{ localBookUploading ? '导入中' : '导入本地书' }}</span>
          </button>
          <!-- 导出走桌面文件对话框(save dialog + 本地路径), 浏览器里没有等价能力。 -->
          <button
            v-if="!appStore.isWebMode"
            class="shelf-btn"
            type="button"
            title="导出书籍"
            aria-label="导出书籍"
            @click="showExportModal = true"
          >
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
              <path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4" />
              <path d="M7 10l5 5 5-5" />
              <path d="M12 15V3" />
            </svg>
            <span class="shelf-btn-label">导出书籍</span>
          </button>
          <!--
            分组管理 / 缓存管理 / 编辑(批量删除、移组)都是**写操作**, 而 Web 端
            只注册了只读接口(见 src/server/router.rs): `saveBookGroup`、
            `deleteBookGroups`、`saveBookGroupId`、`deleteBook(s)` 都没有 HTTP 路由。
            这些按钮留着不会有任何效果 —— 用户点"批量删除"只会收到一个
            `请求失败(404)` 的 toast, 比没有按钮更糟。与"导出书籍"同样处理。
          -->
          <button
            v-if="!appStore.isWebMode"
            class="shelf-btn"
            type="button"
            title="分组管理"
            aria-label="分组管理"
            @click="showGroupManager = true"
          >
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
              <path d="M4 7h6l2 2h8v8a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V9a2 2 0 0 1 2-2Z" />
              <path d="M8 13h8" />
            </svg>
            <span class="shelf-btn-label">分组管理</span>
          </button>
          <button
            v-if="!appStore.isWebMode"
            class="shelf-btn"
            type="button"
            title="缓存管理"
            aria-label="缓存管理"
            @click="showCacheManager = true"
          >
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
              <ellipse cx="12" cy="5" rx="8" ry="3" />
              <path d="M4 5v6c0 1.66 3.58 3 8 3s8-1.34 8-3V5" />
              <path d="M4 11v6c0 1.66 3.58 3 8 3s8-1.34 8-3v-6" />
            </svg>
            <span class="shelf-btn-label">缓存管理</span>
          </button>
          <button
            v-if="!appStore.isWebMode"
            class="shelf-btn"
            type="button"
            :class="{ active: shelfStore.editMode }"
            :title="shelfStore.editMode ? '完成' : '编辑'"
            :aria-label="shelfStore.editMode ? '完成' : '编辑'"
            @click="toggleEditMode"
          >
            <svg v-if="shelfStore.editMode" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
              <path d="M20 6 9 17l-5-5" />
            </svg>
            <svg v-else viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
              <path d="M12 20h9" />
              <path d="M16.5 3.5a2.12 2.12 0 0 1 3 3L7 19l-4 1 1-4Z" />
            </svg>
            <span class="shelf-btn-label">{{ shelfStore.editMode ? '完成' : '编辑' }}</span>
          </button>
        </div>
      </div>

      <input
        ref="localBookFileInputRef"
        type="file"
        accept=".txt,.epub,.pdf,text/plain,application/epub+zip,application/pdf"
        class="hidden-input"
        @change="handleLocalBookFileChange"
      />

      <!-- Group Tabs -->
      <div class="group-tabs">
        <div class="tabs-scroll">
          <button
            v-for="group in shelfStore.displayGroups"
            :key="group.groupId"
            class="tab-item"
            :class="{ active: shelfStore.activeGroupId === group.groupId }"
            @click="shelfStore.activeGroupId = group.groupId"
          >
            {{ group.groupName }}
          </button>
        </div>
        <!-- 编辑模式(全选/批量删除)作用于整个分组, 此时隐藏搜索框: 显示筛选
             子集却批量操作看不见的书, 对不可撤销的删除是危险的歧义。 -->
        <div v-if="!shelfStore.editMode" class="shelf-search">
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" aria-hidden="true">
            <circle cx="11" cy="11" r="7" />
            <path d="m20 20-3.5-3.5" />
          </svg>
          <input
            v-model.trim="bookSearchText"
            type="text"
            class="shelf-search-input"
            placeholder="搜索书架"
            aria-label="搜索书架"
          />
          <button
            v-if="bookSearchText"
            type="button"
            class="shelf-search-clear"
            aria-label="清空书架搜索"
            @click="bookSearchText = ''"
          >
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.4" aria-hidden="true">
              <path d="M6 6l12 12M18 6 6 18" />
            </svg>
          </button>
        </div>
        <div ref="sortMenuRef" class="sort-trigger-wrap">
          <button
            type="button"
            class="sort-trigger"
            :class="{ open: sortMenuOpen }"
            :aria-expanded="sortMenuOpen"
            aria-haspopup="listbox"
            :title="`书架排序方式:${bookSortLabel(shelfStore.effectiveSortMode)}`"
            :aria-label="`书架排序方式:${bookSortLabel(shelfStore.effectiveSortMode)}`"
            @click="toggleSortMenu"
          >
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" aria-hidden="true">
              <path d="M3 6h12M3 12h9M3 18h6" />
              <path d="m17 8 3-3 3 3M20 5v14" />
            </svg>
            <span>{{ bookSortLabel(shelfStore.effectiveSortMode) }}</span>
            <svg class="sort-caret" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" aria-hidden="true">
              <path d="m6 9 6 6 6-6" />
            </svg>
          </button>
          <Transition name="sort-pop">
            <div v-if="sortMenuOpen" class="sort-popover" role="listbox" aria-label="书架排序方式">
              <button
                v-for="option in BOOK_SORT_OPTIONS"
                :key="option.value"
                type="button"
                class="sort-option"
                :class="{ active: option.value === shelfStore.effectiveSortMode }"
                role="option"
                :aria-selected="option.value === shelfStore.effectiveSortMode"
                @click="chooseSort(option.value)"
              >
                <span class="sort-option-dot" aria-hidden="true">
                  <svg v-if="option.value === shelfStore.effectiveSortMode" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="3">
                    <path d="M20 6 9 17l-5-5" />
                  </svg>
                </span>
                <span class="sort-option-label">{{ option.label }}</span>
              </button>
            </div>
          </Transition>
        </div>
      </div>

      <!-- Book Grid -->
      <div class="shelf-grid-wrapper">
        <BookGrid
          :books="visibleShelfBooks"
          :edit-mode="shelfStore.editMode"
          :selected-urls="shelfStore.selectedBookUrls"
          :loading="shelfStore.loading"
          :sortable="sortable"
          :empty-text="shelfEmptyText"
        @click="handleBookClick"
        @info="handleBookInfo"
        @delete="handleDeleteBook"
        @select="shelfStore.toggleSelection($event.bookUrl)"
        @reorder="handleReorderBooks"
        @contextmenu="handleBookContextMenu"
        />
      </div>

      <!-- Batch Toolbar -->
      <Transition name="slide-up">
        <div v-if="shelfStore.editMode && shelfStore.selectedBookUrls.size > 0" class="batch-toolbar">
          <div class="batch-info">
            已选中 <span>{{ shelfStore.selectedBookUrls.size }}</span> 本书
          </div>
          <div class="batch-actions">
            <button class="batch-btn" @click="handleBulkMove">
              <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M12 5v14M5 12h14" /></svg>
              移动分组
            </button>
            <button class="batch-btn danger" @click="handleBulkDelete">
              <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M3 6h18M19 6v14a3 3 0 0 1-3-3H7c-1 0-2-1-2-2V6M8 6V4c0-1 1-2 2-2h4c1 0 2 1 2 2v2" /></svg>
              批量删除
            </button>
          </div>
        </div>
      </Transition>
    </div>

    <!-- Book Detail Modal -->
    <BookDetailModal
      v-model="showDetail"
      :book="selectedBook"
      :start-in-edit="openDetailInEdit"
    />

    <!-- Group Select Modal -->
    <GroupSelectModal
      v-model="showGroupSelect"
      :books="groupEditTargets"
      :book-count="groupEditTargets.length"
      :submitting="groupSaving"
      @confirm="handleGroupConfirm"
    />
    <GroupManagerModal v-model="showGroupManager" />

    <CacheLibraryModal v-model="showCacheManager" />
    <ExportBookModal v-model="showExportModal" />
  </div>
</template>

<script setup lang="ts">
import { ref, computed, onBeforeUnmount, onMounted, watch } from 'vue'
import { useRouter } from 'vue-router'
import { useBookshelfStore } from '../stores/bookshelf'
import { useReaderStore } from '../stores/reader'
import { useAppStore } from '../stores/app'
import { uploadEpubBook, uploadPdfBook, uploadTxtBook } from '../api/bookshelf'
import BookGrid from '../components/BookGrid.vue'
import BookDetailModal from '../components/BookDetailModal.vue'
import GroupSelectModal from '../components/bookshelf/GroupSelectModal.vue'
import GroupManagerModal from '../components/bookshelf/GroupManagerModal.vue'
import SearchResults from '../components/SearchResults.vue'
import CacheLibraryModal from '../components/CacheLibraryModal.vue'
import ExportBookModal from '../components/ExportBookModal.vue'
import { showContextMenu, type MenuItem } from '../composables/useContextMenu'
import { BOOK_SORT_KEYS, BOOK_SORT_OPTIONS, bookSortLabel } from '../utils/bookshelfSort'
import { filterShelfBooks } from '../utils/bookshelfFilter'
import type { Book, SearchBook } from '../types'

const router = useRouter()
const shelfStore = useBookshelfStore()
const readerStore = useReaderStore()
const appStore = useAppStore()

const showDetail = ref(false)
/** 书架搜索关键词: 只在已加载的书里筛, 不发起网络搜索。 */
const bookSearchText = ref('')
/** 书架网格实际展示的书: 当前分组按排序排好后, 再按关键词过滤。 */
const visibleShelfBooks = computed(() => (
  filterShelfBooks(shelfStore.sortedBooks, bookSearchText.value)
))
/** 空态文案要区分「书架没有书」和「关键词没命中」, 否则搜不到时像把书弄丢了。 */
const shelfEmptyText = computed(() => (
  bookSearchText.value.trim()
    ? '没有找到匹配的书籍'
    : '书架空空如也,搜索添加新书吧'
))
const showGroupSelect = ref(false)
/** 右键「调整分组」时记住目标书; 为空表示走批量(编辑模式多选)路径。 */
const groupTargetBookUrl = ref('')
/** 分组落盘进行中: 传给弹窗禁用保存按钮。 */
const groupSaving = ref(false)
const showGroupManager = ref(false)
const showCacheManager = ref(false)
const showExportModal = ref(false)
const selectedBook = ref<Book | SearchBook | null>(null)
/** 本次打开详情弹窗是否直接进入编辑态(书架右键「编辑信息」)。 */
const openDetailInEdit = ref(false)
// 关闭后清掉, 避免下次打开沿用上一次的「直接编辑」意图。
watch(showDetail, (visible) => {
  if (!visible) openDetailInEdit.value = false
})
const openingBookUrl = ref('')
const localBookFileInputRef = ref<HTMLInputElement | null>(null)
const localBookUploading = ref(false)

/** 分组编辑器的目标书: 右键单本时是那一本, 批量时是所有选中书。
 *
 *  用 computed 而不是打开时快照 —— 弹窗开着时用户没法改选中集, 但 store
 *  数据可能在后台刷新, 引用最新对象能保证勾选状态基于最新位域初始化。 */
const groupEditTargets = computed(() => {
  const target = groupTargetBookUrl.value
  if (target) {
    const book = shelfStore.books.find((b) => b.bookUrl === target)
    return book ? [book] : []
  }
  return shelfStore.books.filter((b) => shelfStore.selectedBookUrls.has(b.bookUrl))
})

onMounted(async () => {
  let skipOfflineRestore = false
  try {
    skipOfflineRestore = sessionStorage.getItem('reader-skip-offline-restore') === '1'
    sessionStorage.removeItem('reader-skip-offline-restore')
  } catch {
    // Ignore unavailable session storage.
  }
  await Promise.all([
    shelfStore.fetchBooks().catch(() => undefined),
    shelfStore.fetchGroups().catch(() => undefined),
  ])
  if (!skipOfflineRestore && !appStore.isOnline) {
    const restored = await readerStore.restorePersistedSession()
    if (restored) {
      appStore.showToast('已恢复最近阅读的离线章节', 'success')
      router.replace('/reader')
    }
  }
})

function triggerLocalBookUpload() {
  if (localBookUploading.value) return
  localBookFileInputRef.value?.click()
}

async function handleLocalBookFileChange(event: Event) {
  const input = event.target as HTMLInputElement
  const file = input.files?.[0]
  input.value = ''
  if (!file) return

  const lowerName = file.name.toLowerCase()
  if (!lowerName.endsWith('.txt') && !lowerName.endsWith('.epub') && !lowerName.endsWith('.pdf')) {
    appStore.showToast('只支持导入 .txt、.epub 或 .pdf 文件', 'warning')
    return
  }

  localBookUploading.value = true
  try {
    const book = lowerName.endsWith('.epub')
      ? await uploadEpubBook(file)
      : lowerName.endsWith('.pdf')
        ? await uploadPdfBook(file)
        : await uploadTxtBook(file)
    await shelfStore.fetchBooks()
    appStore.showToast(`已导入《${book.name}》`, 'success')
    await handleBookClick(book)
  } catch (e: unknown) {
    appStore.showToast((e as Error).message || '本地书导入失败', 'error')
  } finally {
    localBookUploading.value = false
  }
}

async function handleBookClick(book: Book | SearchBook) {
  const b = book as Book
  if (openingBookUrl.value === b.bookUrl) return

  openingBookUrl.value = b.bookUrl
  try {
    // 置顶只是书架排序，与"打开这本书"无关；await 它等于把一次书架整表写盘
    // 插在打开链路前面 —— 手机端同样是先开书、排序在后台做。并行发起，失败
    // 也不影响阅读（原来就是 catch 掉的）。
    void shelfStore.moveBookToFront(b.bookUrl).catch(() => undefined)
    const loadBookTask = readerStore.loadBook(b)
    await router.push('/reader')
    await loadBookTask
  } finally {
    openingBookUrl.value = ''
  }
}

function handleBookInfo(book: Book | SearchBook) {
  openDetail(book, false)
}

/** 打开详情弹窗。editNow 为真时直接落到编辑态(书架右键「编辑信息」)。 */
function openDetail(book: Book | SearchBook, editNow: boolean) {
  selectedBook.value = book
  openDetailInEdit.value = editNow
  showDetail.value = true
}

async function handleDeleteBook(book: Book | SearchBook) {
  const b = book as Book
  const ok = await appStore.confirmDialog(`确定从书架删除 "${b.name}"？`, { title: '删除书籍', danger: true })
  if (!ok) return
  try {
    await shelfStore.removeBook(b)
    appStore.showToast(`已删除 "${b.name}"`, 'success')
  } catch (e: unknown) {
    appStore.showToast((e as Error).message, 'error')
  }
}

function handleBookContextMenu({ book, event }: { book: Book | SearchBook; event: MouseEvent }) {
  // 批量编辑模式不弹单本菜单
  if (shelfStore.editMode) return
  const readItems: MenuItem[] = [
    { label: '开始阅读', action: () => handleBookClick(book) },
    { label: '查看详情', action: () => handleBookInfo(book) },
  ]
  // Web 端只保留上面这条纯读路径。其余项都依赖桌面专属能力:
  // 「编辑信息」要写回 saveBook + 封面上传, 「调整分组」是 saveBookGroupId,
  // 「缓存管理」靠 cache_book_sse(IPC 通道), 「删除」是 delete_book。
  // 这些都没有 HTTP 路由, 留着就是点一下 404 的按钮。
  const writeItems: MenuItem[] = appStore.isWebMode
    ? []
    : [
      { label: '编辑信息', action: () => openDetail(book, true) },
      { divider: true },
      // 单本分组调整: 记下这本书, 选中集此时必然是空的(编辑模式不弹本菜单)。
      { label: '调整分组', action: () => openGroupSelect(book.bookUrl) },
      { label: '缓存管理', action: () => { showCacheManager.value = true } },
      { divider: true },
      { label: '删除', danger: true, action: () => handleDeleteBook(book) },
    ]
  showContextMenu(event, [...readItems, ...writeItems], book)
}

function toggleEditMode() {
  shelfStore.editMode = !shelfStore.editMode
  if (!shelfStore.editMode) {
    shelfStore.clearSelection()
  }
}

/** 编辑模式的「全选」「批量删除」作用于整个分组, 而搜索只筛显示。
 *  进入编辑时清掉关键词, 否则用户看到的是筛选后的几本, 实际操作却包含
 *  看不见的书 —— 删除不可撤销, 这种歧义必须避免。 */
watch(() => shelfStore.editMode, (editing) => {
  if (editing) bookSearchText.value = ''
})

async function handleBulkDelete() {
  const count = shelfStore.selectedBookUrls.size
  const ok = await appStore.confirmDialog(`确定删除选中的 ${count} 本书？`, { title: '批量删除', danger: true })
  if (!ok) return
  try {
    await shelfStore.bulkDelete()
    appStore.showToast(`成功删除 ${count} 本书`, 'success')
  } catch (e: any) {
    appStore.showToast(e.message, 'error')
  }
}

async function handleBulkMove() {
  openGroupSelect('')
}

/** 打开分组编辑弹窗。`bookUrl` 非空表示单本调整, 空表示批量(编辑模式多选)。 */
function openGroupSelect(bookUrl: string) {
  groupTargetBookUrl.value = bookUrl
  showGroupSelect.value = true
}

/** 弹窗确认时按勾选位掩码落盘: 勾选即加入分组, 取消勾选即移出。 */
async function handleGroupConfirm(mask: number) {
  const target = groupTargetBookUrl.value
  groupSaving.value = true
  try {
    if (target) {
      await shelfStore.setBooksGroupMask([target], mask)
      appStore.showToast('分组已更新', 'success')
    } else {
      const urls = [...shelfStore.selectedBookUrls]
      await shelfStore.setBooksGroupMask(urls, mask)
      appStore.showToast(`已调整 ${urls.length} 本书的分组`, 'success')
      shelfStore.clearSelection()
    }
    showGroupSelect.value = false
  } catch (e: any) {
    // 失败时**保留** target 且不关弹窗: 用户在弹窗里重试仍是单本语义 ——
    // 若在 finally 里清掉, 重试会退化成「0 本书的批量」, 显示成功却没保存。
    appStore.showToast(e.message || '分组更新失败', 'error')
  } finally {
    groupSaving.value = false
  }
}
// 弹窗最终关闭(成功或用户放弃)时才清 target, 与保存失败的重试互不冲突。
watch(showGroupSelect, (visible) => {
  if (!visible) groupTargetBookUrl.value = ''
})
async function handleReorderBooks(payload: { draggedUrl: string; targetUrl: string }) {
  try {
    await shelfStore.reorderBooks(payload.draggedUrl, payload.targetUrl)
  } catch (e: any) {
    appStore.showToast(e.message || '排序失败', 'error')
  }
}

/** 书架拖拽排序。
 *
 *  「手动排序」已从排序菜单移除, 排序模式不再可能等于 3, 因此拖拽实际不可用
 *  —— 保留这个判断而不是写死 `false`: `reorderBooks` 与 BookGrid 的拖拽能力
 *  仍在, 将来若把该项加回菜单, 只需放开这一行即可。 */
const sortable = computed(() => (
  shelfStore.effectiveSortMode === BOOK_SORT_KEYS.manual
  && !shelfStore.editMode
  && !shelfStore.loading
  && !shelfStore.sorting
  && !shelfStore.isSearchMode
))

// ─── 排序下拉 ───
const sortMenuOpen = ref(false)
const sortMenuRef = ref<HTMLElement | null>(null)

/** 应用排序方式。选中具体分组时写该分组的 bookSort, 否则写全局偏好。 */
async function applySort(value: number) {
  if (shelfStore.activeGroupId > 0) {
    await shelfStore.setActiveGroupSort(value)
  } else {
    shelfStore.setSortMode(value)
  }
}

async function chooseSort(value: number) {
  sortMenuOpen.value = false
  try {
    await applySort(value)
  } catch (e: any) {
    appStore.showToast(e.message || '排序设置失败', 'error')
  }
}

function toggleSortMenu() {
  sortMenuOpen.value = !sortMenuOpen.value
}

function onSortMenuOutsideClick(event: MouseEvent) {
  const target = event.target as Node | null
  if (target && sortMenuRef.value?.contains(target)) return
  sortMenuOpen.value = false
}

function onSortMenuKeydown(event: KeyboardEvent) {
  if (event.key === 'Escape') sortMenuOpen.value = false
}

watch(sortMenuOpen, (open) => {
  if (open) {
    // 捕获阶段: 触发按钮自己的 click 已经在冒泡阶段处理完, 不会立刻自关。
    window.addEventListener('click', onSortMenuOutsideClick, true)
    window.addEventListener('keydown', onSortMenuKeydown)
  } else {
    window.removeEventListener('click', onSortMenuOutsideClick, true)
    window.removeEventListener('keydown', onSortMenuKeydown)
  }
})

onBeforeUnmount(() => {
  window.removeEventListener('click', onSortMenuOutsideClick, true)
  window.removeEventListener('keydown', onSortMenuKeydown)
})

</script>

<style scoped>
.home-view {
  height: 100%;
  min-height: 0;
  overflow: hidden;
  scrollbar-width: none;
  -ms-overflow-style: none;
}

.home-view::-webkit-scrollbar {
  display: none;
}

.shelf-content {
  height: 100%;
  max-width: var(--content-max-width);
  margin: 0 auto;
  padding: 0 var(--space-6);
  display: flex;
  flex-direction: column;
  min-height: 0;
}


.hidden-input {
  display: none;
}

.shelf-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: var(--space-6) 0 var(--space-3);
}

.shelf-title {
  font-size: var(--text-2xl);
  font-weight: 700;
  letter-spacing: 0;
}

.book-count {
  font-size: var(--text-base);
  font-weight: 400;
  color: var(--color-text-tertiary);
}

.shelf-actions {
  display: flex;
  gap: var(--space-2);
  align-items: center;
}

.shelf-btn {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  gap: var(--space-2);
  padding: var(--space-2) var(--space-4);
  border-radius: var(--radius-md);
  font-size: var(--text-sm);
  font-weight: 500;
  color: var(--color-text-secondary);
  border: 1px solid var(--color-border);
  transition: all var(--duration-fast);
  min-height: 38px;
  white-space: nowrap;
}

.shelf-btn:disabled {
  opacity: 0.55;
  cursor: wait;
}

.shelf-btn svg {
  width: 17px;
  height: 17px;
  flex-shrink: 0;
}

.shelf-btn:hover {
  background: var(--color-bg-hover);
  color: var(--color-text);
}

.shelf-btn.active {
  background: var(--color-primary-soft-bg);
  color: var(--color-primary-soft-text);
  border-color: var(--color-primary-soft-border);
}

.shelf-btn .spinning {
  animation: shelf-spin 0.9s linear infinite;
}

@keyframes shelf-spin {
  from { transform: rotate(0deg); }
  to { transform: rotate(360deg); }
}

.group-tabs {
  display: flex;
  align-items: center;
  gap: var(--space-3);
  border-bottom: 1px solid var(--color-border-light);
  margin-bottom: var(--space-2);
}

.tabs-scroll {
  flex: 1;
  min-width: 0;
  display: flex;
  gap: 0;
  overflow-x: auto;
  scrollbar-width: none;
  -ms-overflow-style: none;
}

/* 书架搜索: 与分组标签同一行, 排在排序入口左侧。 */
.shelf-search {
  flex: 0 1 190px;
  min-width: 0;
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 0 10px;
  border: 1px solid var(--color-border-light);
  border-radius: var(--radius-md);
  background: var(--color-bg-elevated);
  color: var(--color-text-tertiary);
  transition: border-color var(--duration-fast), color var(--duration-fast);
}

.shelf-search:focus-within {
  border-color: rgba(201, 127, 58, 0.32);
  color: var(--color-text-secondary);
}

.shelf-search > svg {
  width: 14px;
  height: 14px;
  flex-shrink: 0;
}

.shelf-search-input {
  flex: 1;
  min-width: 0;
  border: 0;
  background: transparent;
  padding: 7px 0;
  font-size: var(--text-xs);
  color: var(--color-text);
  outline: none;
}

.shelf-search-input::placeholder {
  color: var(--color-text-tertiary);
}

.shelf-search-clear {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 16px;
  height: 16px;
  flex-shrink: 0;
  border: 0;
  border-radius: 50%;
  background: transparent;
  color: var(--color-text-tertiary);
  cursor: pointer;
  transition: color var(--duration-fast);
}

.shelf-search-clear:hover {
  color: var(--color-text);
}

.shelf-search-clear svg {
  width: 10px;
  height: 10px;
}

.sort-trigger-wrap {
  flex-shrink: 0;
  position: relative;
}

.sort-trigger {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  padding: 5px 8px 5px 10px;
  border: 1px solid transparent;
  border-radius: var(--radius-md);
  background: transparent;
  color: var(--color-text-tertiary);
  font-size: var(--text-xs);
  font-weight: 500;
  line-height: 1.4;
  white-space: nowrap;
  transition: color var(--duration-fast), background var(--duration-fast), border-color var(--duration-fast);
}

.sort-trigger svg {
  width: 14px;
  height: 14px;
  flex-shrink: 0;
}

.sort-trigger .sort-caret {
  width: 12px;
  height: 12px;
  opacity: 0.65;
  transition: transform var(--duration-fast);
}

.sort-trigger:hover {
  color: var(--color-text-secondary);
  background: var(--color-bg-hover);
}

.sort-trigger.open {
  color: var(--color-text);
  background: var(--color-bg-hover);
  border-color: var(--color-border);
}

.sort-trigger.open .sort-caret {
  transform: rotate(180deg);
}

.sort-popover {
  position: absolute;
  top: calc(100% + 6px);
  right: 0;
  z-index: var(--z-dropdown, 60);
  min-width: 156px;
  padding: 5px;
  border: 1px solid color-mix(in srgb, var(--color-border) 80%, transparent);
  border-radius: var(--radius-lg);
  background: color-mix(in srgb, var(--color-bg-elevated) 94%, transparent);
  box-shadow: var(--shadow-xl);
  backdrop-filter: blur(20px) saturate(140%);
  -webkit-backdrop-filter: blur(20px) saturate(140%);
}

.sort-option {
  display: flex;
  align-items: center;
  gap: 8px;
  width: 100%;
  min-height: 32px;
  padding: 0 8px;
  border: 0;
  border-radius: var(--radius-sm);
  background: transparent;
  color: var(--color-text-secondary);
  font-size: var(--text-xs);
  text-align: left;
  transition: background var(--duration-fast), color var(--duration-fast);
}

.sort-option:hover {
  background: var(--color-bg-hover);
  color: var(--color-text);
}

.sort-option.active {
  color: var(--color-primary);
  font-weight: 600;
}

.sort-option-dot {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 14px;
  height: 14px;
  flex-shrink: 0;
}

.sort-option-dot svg {
  width: 13px;
  height: 13px;
}

.sort-option-label {
  flex: 1;
}

.sort-pop-enter-active,
.sort-pop-leave-active {
  transition: opacity var(--duration-fast), transform var(--duration-fast);
}

.sort-pop-enter-from,
.sort-pop-leave-to {
  opacity: 0;
  transform: translateY(-4px) scale(0.98);
}

.tabs-scroll::-webkit-scrollbar {
  display: none;
}

.tab-item {
  padding: var(--space-3) var(--space-5);
  font-size: var(--text-sm);
  font-weight: 500;
  color: var(--color-text-tertiary);
  white-space: nowrap;
  position: relative;
  transition: color var(--duration-fast);
  border-bottom: 2px solid transparent;
  margin-bottom: -1px;
}

.tab-item:hover {
  color: var(--color-text-secondary);
}

.tab-item.active {
  color: var(--color-primary);
  border-bottom-color: var(--color-primary);
}

.shelf-grid-wrapper {
  flex: 1;
  min-height: 0;
  overflow: auto;
  padding-bottom: calc(76px + var(--space-6));
  scrollbar-width: none;
  -ms-overflow-style: none;
}

.shelf-grid-wrapper::-webkit-scrollbar {
  display: none;
}

.batch-toolbar {
  position: fixed;
  bottom: calc(76px + var(--space-4));
  left: 50%;
  transform: translateX(-50%);
  background: var(--color-bg-elevated);
  border: 1px solid var(--color-border);
  box-shadow: var(--shadow-xl);
  border-radius: var(--radius-xl);
  padding: var(--space-3) var(--space-6);
  display: flex;
  align-items: center;
  gap: var(--space-8);
  z-index: calc(var(--z-sticky) + 5);
  backdrop-filter: blur(12px);
}

.batch-info {
  font-size: var(--text-sm);
  color: var(--color-text-secondary);
}

.batch-info span {
  font-weight: 700;
  color: var(--color-primary);
  margin: 0 4px;
}

.batch-actions {
  display: flex;
  gap: var(--space-2);
}

.batch-btn {
  display: flex;
  align-items: center;
  gap: var(--space-2);
  padding: var(--space-2) var(--space-4);
  border-radius: var(--radius-lg);
  font-size: var(--text-sm);
  font-weight: 600;
  background: var(--color-bg-hover);
  color: var(--color-text-secondary);
  transition: all var(--duration-fast);
}

.batch-btn:hover {
  background: var(--color-bg-sunken);
  color: var(--color-text);
}

.batch-btn.danger {
  color: var(--color-danger);
}

.batch-btn svg {
  width: 16px;
  height: 16px;
}

/* slide-up transition */
.slide-up-enter-active,
.slide-up-leave-active {
  transition: all var(--duration-normal) var(--ease-out);
}
.slide-up-enter-from,
.slide-up-leave-to {
  opacity: 0;
  transform: translate(-50%, 20px);
}

@media (max-width: 640px) {
  .shelf-content {
    padding: 0 var(--space-4);
  }

  /* 窄屏把搜索框压窄, 让分组标签仍有足够横向空间滚动。 */
  .shelf-search {
    flex: 0 1 116px;
  }

  .shelf-header {
    align-items: center;
    gap: var(--space-3);
    padding: var(--space-5) 0 var(--space-3);
  }

  .shelf-title {
    flex: 0 0 auto;
    font-size: var(--text-2xl);
  }

  .shelf-actions {
    flex: 1 1 auto;
    justify-content: flex-end;
    min-width: 0;
    overflow-x: auto;
    scrollbar-width: none;
    -ms-overflow-style: none;
  }

  .shelf-actions::-webkit-scrollbar {
    display: none;
  }

  .shelf-btn {
    width: 42px;
    min-width: 42px;
    height: 42px;
    min-height: 42px;
    padding: 0;
    border-radius: var(--radius-lg);
  }

  .shelf-btn svg {
    width: 19px;
    height: 19px;
  }

  .shelf-btn-label {
    position: absolute;
    width: 1px;
    height: 1px;
    padding: 0;
    margin: -1px;
    overflow: hidden;
    clip: rect(0, 0, 0, 0);
    white-space: nowrap;
    border: 0;
  }

  .batch-toolbar {
    width: calc(100% - var(--space-8));
    bottom: calc(76px + var(--space-3));
    gap: var(--space-4);
    justify-content: space-between;
  }
  .batch-info { display: none; }
}

@media (max-width: 420px) {
  /* 窄屏手机(小屏 Android 与 iPhone SE/标准款普遍在 375–414px): 排序入口只留
     图标, 把横向空间让给分组标签(标签栏本身仍可横向滚动)。
     断点取 420px 而不是更小: 行内空间是先满足搜索框与排序入口、剩下的才归
     tabs-scroll, 所以只有图标化这一步能改变分配 —— 若只在 360px 以下生效,
     361–420px 反而比 360px 更挤。按钮保留 aria-label 与 title, 当前排序方式
     仍能读出; min-width/height 保住触摸目标。 */
  .sort-trigger {
    gap: 0;
    justify-content: center;
    min-width: 40px;
    min-height: 40px;
    padding: 5px 6px;
  }

  .sort-trigger > span,
  .sort-trigger .sort-caret {
    display: none;
  }
}
</style>
