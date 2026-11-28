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
          <button
            class="shelf-btn"
            type="button"
            title="导入本地"
            aria-label="导入本地"
            @click="openLocalBookImport"
            :disabled="localBookUploading"
          >
            <svg v-if="!localBookUploading" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
              <path d="M12 3v12" />
              <path d="m7 8 5-5 5 5" />
              <path d="M5 21h14" />
            </svg>
            <svg v-else class="spinning" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
              <path d="M21 12a9 9 0 0 0-15.55-6.2L3 8" />
              <path d="M3 3v5h5" />
            </svg>
            <span class="shelf-btn-label">{{ localBookUploading ? '导入中' : '导入本地' }}</span>
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
        multiple
        class="hidden-input"
        @change="handleLocalBookFileChange"
      />
      <!-- Group Tabs -->
      <div class="group-tabs">
        <button
          v-if="groupScrollLeft"
          class="tabs-scroll-btn is-left"
          type="button"
          title="向前查看分组"
          aria-label="向前查看分组"
          @click="scrollGroupTabs(-1)"
        >
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" aria-hidden="true">
            <path d="m15 18-6-6 6-6" />
          </svg>
        </button>
        <div
          :ref="setGroupTabsRef"
          class="tabs-scroll"
          :class="{ 'is-group-dragging': groupTabsDrag.dragActive.value }"
          @scroll="updateGroupScrollState"
          @wheel="handleGroupTabsWheel"
        >
          <div
            v-if="groupTabsDrag.dropMarker.value"
            class="group-drop-marker"
            :style="groupTabsDrag.dropMarkerStyle.value"
          />
          <button
            v-for="group in shelfStore.displayGroups"
            :key="group.groupId"
            :data-group-id="group.groupId"
            class="tab-item"
            :class="{
              active: shelfStore.activeGroupId === group.groupId,
              'is-group-dragging': groupTabsDrag.draggingGroupId.value === group.groupId && groupTabsDrag.dragActive.value,
            }"
            :aria-pressed="shelfStore.activeGroupId === group.groupId"
            @click="groupTabsDrag.handleClick(group.groupId)"
            @pointerdown="group.groupId > 0 && handleGroupTabPointerDown($event, group)"
            @pointermove="groupTabsDrag.handlePointerMove($event)"
            @touchmove="groupTabsDrag.handleTouchMove($event)"
            @pointerup="groupTabsDrag.handlePointerUp($event)"
            @pointercancel="groupTabsDrag.handlePointerCancel($event)"
            @touchend="groupTabsDrag.handleTouchEnd($event)"
          >
            {{ group.groupName }}
          </button>
        </div>
        <button
          v-if="groupScrollRight"
          class="tabs-scroll-btn is-right"
          type="button"
          title="向后查看分组"
          aria-label="向后查看分组"
          @click="scrollGroupTabs(1)"
        >
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" aria-hidden="true">
            <path d="m9 18 6-6-6-6" />
          </svg>
        </button>
        <Teleport to="body">
          <div
            v-if="groupTabsDrag.dragActive.value"
            class="group-drag-ghost"
            :style="{
              left: `${groupTabsDrag.ghost.value.x}px`,
              top: `${groupTabsDrag.ghost.value.y}px`,
              width: `${groupTabsDrag.ghost.value.width}px`,
              height: `${groupTabsDrag.ghost.value.height}px`,
            }"
          >
            {{ groupTabsDrag.draggedName.value }}
          </div>
        </Teleport>
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
        <div v-if="shelfStore.editMode && (shelfStore.selectedBookUrls.size > 0 || bulkDeleteProgress !== null)" class="batch-toolbar">
          <div class="batch-info">
            已选中 <span>{{ shelfStore.selectedBookUrls.size || bulkDeleteProgress?.total }}</span> 本书
          </div>
          <div class="batch-actions">
            <button class="batch-btn" :disabled="bulkDeleteProgress !== null" @click="handleBulkMove">
              <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M12 5v14M5 12h14" /></svg>
              移动分组
            </button>
            <button class="batch-btn danger" :disabled="bulkDeleteProgress !== null" @click="handleBulkDelete">
              <svg v-if="bulkDeleteProgress === null" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M3 6h18M19 6v14a3 3 0 0 1-3-3H7c-1 0-2-1-2-2V6M8 6V4c0-1 1-2 2-2h4c1 0 2 1 2 2v2" /></svg>
              <svg v-else class="spinning" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                <path d="M21 12a9 9 0 0 0-15.55-6.2L3 8" />
                <path d="M3 3v5h5" />
              </svg>
              {{ bulkDeleteProgress === null ? '批量删除' : '删除中' }}
            </button>
          </div>
        </div>
      </Transition>
      <div
        v-if="bulkDeleteProgress"
        class="bulk-delete-progress"
        role="status"
        aria-live="polite"
      >
        <div class="bulk-delete-progress-head">
          <span>{{ bulkDeleteProgress.text }}</span>
          <strong>{{ bulkDeleteProgress.percent }}%</strong>
        </div>
        <div class="progress-track">
          <div class="progress-fill" :style="{ width: `${bulkDeleteProgress.percent}%` }"></div>
        </div>
      </div>
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
    <div v-if="showLocalBookImport" class="modal-overlay" @click.self="closeLocalBookImport">
      <div class="modal-content local-import-modal">
        <div class="modal-header">
          <h3>导入本地书</h3>
          <button
            class="close-btn"
            type="button"
            aria-label="关闭"
            :disabled="localBookUploading || creatingLocalBookGroup"
            @click="closeLocalBookImport"
          >
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
              <path d="M18 6 6 18M6 6l12 12" />
            </svg>
          </button>
        </div>
        <div class="modal-body">
          <div class="import-choice">
            <button
              class="import-choice-btn"
              type="button"
              :disabled="localBookUploading || creatingLocalBookGroup"
              @click="chooseLocalBookFiles"
            >
              <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                <path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8Z" />
                <path d="M14 2v6h6" />
              </svg>
              <span>{{ appStore.isWebMode ? '选择文件' : '多选文件' }}</span>
            </button>
            <button
              v-if="isTauriRuntime()"
              class="import-choice-btn"
              type="button"
              :disabled="localBookUploading || creatingLocalBookGroup"
              @click="chooseLocalBookFolder"
            >
              <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                <path d="M3 7a2 2 0 0 1 2-2h5l2 2h9a2 2 0 0 1 2 2v9a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2Z" />
                <path d="M12 11v5M9 14h6" />
              </svg>
              <span>选择文件夹</span>
            </button>
          </div>
          <div v-if="!appStore.isWebMode" class="import-group">
            <span class="import-label">分组</span>
            <div class="import-group-list">
              <label
                v-for="group in shelfStore.visibleGroups"
                :key="group.groupId"
                class="import-group-item"
                :class="{ disabled: localBookUploading || creatingLocalBookGroup }"
              >
                <input
                  v-model="localBookGroupIds"
                  type="checkbox"
                  :value="group.groupId"
                  :disabled="localBookUploading || creatingLocalBookGroup"
                />
                <span>{{ group.groupName }}</span>
              </label>
              <p v-if="!shelfStore.visibleGroups.length" class="empty-group-hint">暂无分组</p>
            </div>
            <button
              v-if="!showNewLocalBookGroup"
              class="new-group-trigger"
              type="button"
              :disabled="localBookUploading || creatingLocalBookGroup"
              @click="showNewLocalBookGroup = true"
            >
              <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                <path d="M12 5v14M5 12h14" />
              </svg>
              新建分组
            </button>
            <div v-else class="new-group-row">
              <input
                v-model.trim="newLocalBookGroupName"
                class="new-group-input"
                placeholder="分组名称"
                :disabled="localBookUploading || creatingLocalBookGroup"
                @keyup.enter="createLocalBookGroup"
              />
              <button
                class="new-group-save"
                type="button"
                :disabled="!newLocalBookGroupName || creatingLocalBookGroup || localBookUploading"
                @click="createLocalBookGroup"
              >
                {{ creatingLocalBookGroup ? '创建中' : '创建' }}
              </button>
            </div>
          </div>
          <div v-if="localBookImportProgress" class="import-progress" role="status" aria-live="polite">
            <div class="progress-track">
              <div class="progress-fill" :style="{ width: `${localBookImportProgress.percent}%` }"></div>
            </div>
            <span>{{ localBookImportProgress.text }}</span>
          </div>
          <p v-if="localBookImportSummary" class="import-summary">{{ localBookImportSummary }}</p>
        </div>
        <div class="modal-footer">
          <span class="import-hint">{{ appStore.isWebMode ? '支持 TXT、EPUB 和 PDF' : '支持 TXT、EPUB、PDF 及子文件夹' }}</span>
          <button class="cancel-btn" type="button" :disabled="localBookUploading || creatingLocalBookGroup" @click="closeLocalBookImport">
            {{ localBookImportSummary ? '关闭' : '取消' }}
          </button>
        </div>
      </div>
    </div>
    <GroupManagerModal v-model="showGroupManager" />

    <CacheLibraryModal v-model="showCacheManager" />
    <ExportBookModal v-model="showExportModal" />
  </div>
</template>

<script setup lang="ts">
import { ref, computed, nextTick, onBeforeUnmount, onMounted, watch } from 'vue'
import { useRouter } from 'vue-router'
import { open as openDialog } from '@tauri-apps/plugin-dialog'
import { useBookshelfStore } from '../stores/bookshelf'
import { useReaderStore } from '../stores/reader'
import { useAppStore } from '../stores/app'
import {
  importLocalBookFolder,
  uploadEpubBook,
  uploadPdfBook,
  uploadTxtBook,
  type LocalBookFolderImportFile,
} from '../api/bookshelf'
import { isTauriRuntime } from '../api/transport'
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
import { groupMask as groupMaskFromIds } from '../utils/bookGroupBits'
import { useGroupDragSort } from '../composables/useGroupDragSort'
import type { Book, BookGroup, SearchBook } from '../types'

const router = useRouter()
const shelfStore = useBookshelfStore()
const readerStore = useReaderStore()
const appStore = useAppStore()

const showDetail = ref(false)
/** 书架搜索关键词: 只在已加载的书里筛, 不发起网络搜索。 */
const bookSearchText = ref('')
const groupTabsRef = ref<HTMLElement | null>(null)
const groupScrollLeft = ref(false)
const groupScrollRight = ref(false)
let groupTabsResizeObserver: ResizeObserver | null = null
let groupTabsScrollTimer: number | null = null
const groupTabsDrag = useGroupDragSort({
  items: () => shelfStore.displayGroups.map((group) => ({
    ...group,
    draggable: group.groupId > 0,
  })),
  direction: 'horizontal',
  disabled: () => shelfStore.editMode,
  longPressOnTouch: true,
  onTap: (groupId) => {
    shelfStore.activeGroupId = groupId
  },
  onReorder: async (draggedId, targetId, placement) => {
    await shelfStore.reorderGroups(draggedId, targetId, placement)
  },
})
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

function updateGroupScrollState() {
  const tabs = groupTabsRef.value
  if (!tabs) {
    groupScrollLeft.value = false
    groupScrollRight.value = false
    return
  }
  groupScrollLeft.value = tabs.scrollLeft > 1
  groupScrollRight.value = tabs.scrollLeft + tabs.clientWidth < tabs.scrollWidth - 1
}

function setGroupTabsRef(el: Element | { $el?: Element } | null) {
  const tabs = el instanceof HTMLElement
    ? el
    : el && '$el' in el && el.$el instanceof HTMLElement
      ? el.$el
      : null
  groupTabsRef.value = tabs
  groupTabsDrag.containerRef.value = tabs
}

function scrollGroupTabs(direction: -1 | 1) {
  const tabs = groupTabsRef.value
  if (!tabs) return
  tabs.scrollBy({
    left: direction * Math.max(160, tabs.clientWidth * 0.7),
    behavior: 'smooth',
  })
  if (groupTabsScrollTimer !== null) window.clearTimeout(groupTabsScrollTimer)
  groupTabsScrollTimer = window.setTimeout(() => {
    groupTabsScrollTimer = null
    updateGroupScrollState()
  }, 300)
}

function handleGroupTabsWheel(event: WheelEvent) {
  const tabs = groupTabsRef.value
  if (!tabs || tabs.scrollWidth <= tabs.clientWidth + 1) return
  const delta = Math.abs(event.deltaX) > Math.abs(event.deltaY) ? event.deltaX : event.deltaY
  if (!delta) return
  event.preventDefault()
  tabs.scrollLeft += delta
}

function handleGroupTabPointerDown(event: PointerEvent, group: BookGroup) {
  groupTabsDrag.handlePointerDown(event, group.groupId, group.groupName)
}

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
const showLocalBookImport = ref(false)
const localBookGroupIds = ref<number[]>([])
const showNewLocalBookGroup = ref(false)
const newLocalBookGroupName = ref('')
const creatingLocalBookGroup = ref(false)
const localBookImportProgress = ref<{
  current: number
  total: number
  percent: number
  text: string
} | null>(null)
const localBookImportSummary = ref('')
const bulkDeleteProgress = ref<{
  current: number
  total: number
  percent: number
  text: string
} | null>(null)

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

function closeLocalBookImport() {
  if (localBookUploading.value || creatingLocalBookGroup.value) return
  closeLocalBookImportAfterUpload()
}

function chooseLocalBookFiles() {
  if (localBookUploading.value || creatingLocalBookGroup.value) return
  localBookFileInputRef.value?.click()
}

function chooseLocalBookFolder() {
  if (localBookUploading.value || creatingLocalBookGroup.value) return
  if (!isTauriRuntime()) {
    appStore.showToast('文件夹导入仅支持桌面应用', 'warning')
    return
  }
  void chooseLocalBookFolderDesktop()
}

async function chooseLocalBookFolderDesktop() {
  if (localBookUploading.value || creatingLocalBookGroup.value) return
  let selected: string | string[] | null
  try {
    selected = await openDialog({
      directory: true,
      multiple: true,
      title: '选择本地书文件夹',
    })
  } catch (e: unknown) {
    appStore.showToast((e as Error).message || '打开文件夹失败', 'error')
    return
  }
  const folders = Array.isArray(selected) ? selected : selected ? [selected] : []
  if (!folders.length) return
  await importLocalBookFolders(folders)
}

async function createLocalBookGroup() {
  if (appStore.isWebMode) return
  const name = newLocalBookGroupName.value.trim()
  if (!name || creatingLocalBookGroup.value || localBookUploading.value) return
  creatingLocalBookGroup.value = true
  try {
    const groupId = await shelfStore.saveGroup(name)
    if (!showLocalBookImport.value) {
      appStore.showToast('分组已在后台创建', 'success')
      return
    }
    if (!groupId) {
      appStore.showToast('分组已创建，但未能自动勾选', 'warning')
      return
    }
    if (!localBookGroupIds.value.includes(groupId)) {
      localBookGroupIds.value = [...localBookGroupIds.value, groupId]
    }
    newLocalBookGroupName.value = ''
    showNewLocalBookGroup.value = false
    appStore.showToast('分组已创建', 'success')
  } catch (e: unknown) {
    appStore.showToast((e as Error).message || '创建分组失败', 'error')
  } finally {
    creatingLocalBookGroup.value = false
  }
}

async function handleLocalBookFileChange(event: Event) {
  const input = event.target as HTMLInputElement
  const selectedFiles = Array.from(input.files ?? [])
  input.value = ''
  if (!selectedFiles.length) return
  const supported = selectedFiles.filter((file) => isSupportedLocalBookName(file.name))
  if (!supported.length) {
    appStore.showToast('所选内容中没有 TXT、EPUB 或 PDF 文件', 'warning')
    return
  }
  await runLocalBookImport(
    supported.map((file) => ({
      name: file.name,
      load: () => uploadLocalBookFile(file),
    })),
    selectedFiles.length - supported.length,
  )
}

function isSupportedLocalBookName(name: string) {
  const lowerName = name.toLowerCase()
  return lowerName.endsWith('.txt') || lowerName.endsWith('.epub') || lowerName.endsWith('.pdf')
}

function uploadLocalBookFile(file: File) {
  const lowerName = file.name.toLowerCase()
  return lowerName.endsWith('.epub')
    ? uploadEpubBook(file)
    : lowerName.endsWith('.pdf')
      ? uploadPdfBook(file)
      : uploadTxtBook(file)
}

async function importLocalBookFolders(folders: string[]) {
  localBookImportSummary.value = ''
  const groupMask = groupMaskFromIds(localBookGroupIds.value)
  const groupSelectionLocked = !appStore.isWebMode && groupMask > 0
  const failed: Array<{ name: string; reason: string }> = []
  const importedUrls: string[] = []
  let importedCount = 0
  let groupError = ''
  let refreshError = false
  localBookUploading.value = true
  localBookImportProgress.value = {
    current: 0,
    total: folders.length,
    percent: 0,
    text: '正在扫描文件夹',
  }
  try {
    const results: LocalBookFolderImportFile[] = []
    for (const [index, folder] of folders.entries()) {
      try {
        results.push(...await importLocalBookFolder(folder))
      } catch (e: unknown) {
        failed.push({ name: folder, reason: (e as Error).message || '导入失败' })
      }
      localBookImportProgress.value = {
        current: index + 1,
        total: folders.length,
        percent: Math.round(((index + 1) / folders.length) * 100),
        text: `已处理 ${index + 1}/${folders.length} 个文件夹`,
      }
    }
    let duplicateResults = 0
    const seen = new Set<string>()
    for (const result of results) {
      if (result.book && !seen.has(result.book.bookUrl)) {
        seen.add(result.book.bookUrl)
        importedUrls.push(result.book.bookUrl)
        importedCount += 1
      } else if (result.book) {
        duplicateResults += 1
      }
      if (result.error) failed.push({ name: result.path, reason: result.error })
      if (!result.book && !result.error) {
        failed.push({ name: result.path, reason: '导入结果缺失' })
      }
    }
    if ((failed.length || duplicateResults) && !importedUrls.length) {
      localBookImportSummary.value = [
        `导入 0 本`,
        failed.length ? `失败 ${failed.length} 本` : '',
        failed.slice(0, 3).map((item) => `${item.name}: ${item.reason}`).join('，'),
        duplicateResults ? `重复 ${duplicateResults} 本` : '',
      ].filter(Boolean).join('，')
      appStore.showToast(localBookImportSummary.value, 'warning')
      return
    }
    if (importedUrls.length) {
      const finished = await finishLocalBookImport(
        importedUrls,
        groupMask,
        duplicateResults,
        groupSelectionLocked,
      )
      if (finished.outcome === 'refresh-error') refreshError = true
      else if (finished.outcome === 'missing') groupError = `NEW_BOOKS_MISSING:${importedUrls.length}`
      else if (finished.outcome === 'duplicate') groupError = `DUPLICATE_RESULTS:${duplicateResults}`
      else if (finished.outcome === 'group-error') groupError = finished.failure || '未知错误'
    }
    completeLocalBookImport(
      importedCount,
      failed,
      0,
      groupError,
      refreshError,
      groupSelectionLocked,
    )
  } finally {
    localBookUploading.value = false
    localBookImportProgress.value = null
  }
}

async function runLocalBookImport(
  files: Array<{ name: string; load: () => Promise<Book> }>,
  ignoredFiles: number,
) {
  if (!files.length) return
  localBookImportSummary.value = ''
  const groupMask = groupMaskFromIds(localBookGroupIds.value)
  const groupSelectionLocked = !appStore.isWebMode && groupMask > 0
  const failed: Array<{ name: string; reason: string }> = []
  let importedCount = 0
  const importedUrls: string[] = []
  let groupError = ''
  let refreshError = false
  localBookUploading.value = true
  localBookImportProgress.value = {
    current: 0,
    total: files.length,
    percent: 0,
    text: `准备导入 ${files.length} 本`,
  }
  try {
    for (const [index, file] of files.entries()) {
      localBookImportProgress.value = {
        current: index + 1,
        total: files.length,
        percent: Math.round((index / files.length) * 100),
        text: `正在导入 ${index + 1}/${files.length} ${file.name}`,
      }
      try {
        const book = await file.load()
        importedCount += 1
        importedUrls.push(book.bookUrl)
      } catch (e: unknown) {
        failed.push({ name: file.name, reason: (e as Error).message || '导入失败' })
      }
      localBookImportProgress.value = {
        current: index + 1,
        total: files.length,
        percent: Math.round(((index + 1) / files.length) * 100),
        text: `已处理 ${index + 1}/${files.length}`,
      }
    }

    if (importedUrls.length) {
      const finished = await finishLocalBookImport(
        importedUrls,
        groupMask,
        0,
        groupSelectionLocked,
      )
      if (finished.outcome === 'refresh-error') refreshError = true
      else if (finished.outcome === 'missing') groupError = `NEW_BOOKS_MISSING:${importedUrls.length}`
      else if (finished.outcome === 'group-error') groupError = finished.failure || '未知错误'
    }
    completeLocalBookImport(
      importedCount,
      failed,
      ignoredFiles,
      groupError,
      refreshError,
      groupSelectionLocked,
    )
  } finally {
    localBookUploading.value = false
    localBookImportProgress.value = null
  }
}

type LocalBookImportOutcome = 'applied' | 'refresh-error' | 'missing' | 'duplicate' | 'group-error'

async function finishLocalBookImport(
  importedUrls: string[],
  groupMask: number,
  duplicateCount: number,
  requiresGroup: boolean,
): Promise<{ outcome: LocalBookImportOutcome; failure?: string }> {
  let outcome: LocalBookImportOutcome = 'applied'
  let failure = ''
  try {
    await shelfStore.fetchBooks()
  } catch {
    return { outcome: 'refresh-error' }
  }
  const missing = importedUrls.filter((url) => (
    !shelfStore.books.some((book) => book.bookUrl === url)
  )).length
  if (missing) outcome = 'missing'
  else if (duplicateCount > 0) outcome = 'duplicate'
  if (!missing && requiresGroup && !appStore.isWebMode) {
    try {
      await shelfStore.setBooksGroupMask(importedUrls, groupMask)
    } catch (e: unknown) {
      outcome = 'group-error'
      failure = (e as Error).message || '未知错误'
    }
  }
  return { outcome, failure }
}

function completeLocalBookImport(
  importedCount: number,
  failed: Array<{ name: string; reason: string }>,
  ignoredFiles: number,
  groupError: string,
  refreshError: boolean,
  requiresGroups: boolean,
) {
  const failedFiles = failed.filter((item) => item.name !== '分组')
  const allApplied = !failedFiles.length && !refreshError && !groupError
  if (allApplied) {
    appStore.showToast(
      ignoredFiles
        ? `已导入 ${importedCount} 本，跳过 ${ignoredFiles} 个不支持的文件`
        : `已导入 ${importedCount} 本`,
      ignoredFiles ? 'warning' : 'success',
    )
    closeLocalBookImportAfterUpload()
    return
  }

  const missingMatch = groupError.match(/^NEW_BOOKS_MISSING:(\d+)$/)
  const duplicateMatch = groupError.match(/^DUPLICATE_RESULTS:(\d+)$/)
  const extra = missingMatch
    ? `书架中缺少 ${missingMatch[1]} 本新书，分组未应用`
    : duplicateMatch
      ? `跳过 ${duplicateMatch[1]} 本重复结果，分组已应用`
    : refreshError
      ? requiresGroups
        ? '书架刷新失败，所选分组未应用'
        : '书架刷新失败'
      : ''
  const failureDetails = failedFiles
    .slice(0, 3)
    .map((item) => `${item.name}: ${item.reason}`)
    .join('，')
  const moreFailures = failedFiles.length > 3 ? `另有 ${failedFiles.length - 3} 本失败` : ''
  localBookImportSummary.value = [
    `已导入 ${importedCount} 本`,
    failedFiles.length
      ? `失败 ${failedFiles.length} 本`
      : '',
    failureDetails,
    moreFailures,
    groupError && !missingMatch && !duplicateMatch ? `分组失败 ${groupError}` : '',
    ignoredFiles ? `跳过 ${ignoredFiles} 个不支持的文件` : '',
    extra,
  ].filter(Boolean).join('，')
  appStore.showToast(localBookImportSummary.value, 'warning')
}

function closeLocalBookImportAfterUpload() {
  showLocalBookImport.value = false
  localBookImportSummary.value = ''
  localBookGroupIds.value = []
  showNewLocalBookGroup.value = false
  newLocalBookGroupName.value = ''
}

async function openLocalBookImport() {
  if (localBookUploading.value || creatingLocalBookGroup.value) return
  if (!appStore.isWebMode) {
    try {
      await shelfStore.fetchGroups({ preserveOnError: true })
    } catch {
      appStore.showToast('分组加载失败，请重试', 'error')
      return
    }
  }
  localBookGroupIds.value = []
  localBookImportSummary.value = ''
  showNewLocalBookGroup.value = false
  newLocalBookGroupName.value = ''
  showLocalBookImport.value = true
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

function observeGroupTabs() {
  groupTabsResizeObserver?.disconnect()
  groupTabsResizeObserver = null
  const tabs = groupTabsRef.value
  if (!tabs || typeof ResizeObserver === 'undefined') {
    updateGroupScrollState()
    return
  }
  groupTabsResizeObserver = new ResizeObserver(updateGroupScrollState)
  groupTabsResizeObserver.observe(tabs)
  updateGroupScrollState()
}

watch(groupTabsRef, observeGroupTabs)

watch(() => shelfStore.displayGroups, async () => {
  await nextTick()
  updateGroupScrollState()
}, { immediate: true })

watch(() => shelfStore.activeGroupId, async (groupId) => {
  await nextTick()
  const tabs = groupTabsRef.value
  const active = tabs?.querySelector<HTMLElement>(`[data-group-id="${groupId}"]`)
  if (!tabs || !active) return
  const left = active.offsetLeft
  const right = left + active.offsetWidth
  if (left < tabs.scrollLeft) tabs.scrollLeft = left
  else if (right > tabs.scrollLeft + tabs.clientWidth) {
    tabs.scrollLeft = right - tabs.clientWidth
  }
  updateGroupScrollState()
})

async function handleBulkDelete() {
  if (bulkDeleteProgress.value !== null) return
  const count = shelfStore.selectedBookUrls.size
  if (count === 0) return
  const ok = await appStore.confirmDialog(`确定删除选中的 ${count} 本书？`, { title: '批量删除', danger: true })
  if (!ok) return
  const selectedCount = shelfStore.selectedBookUrls.size
  if (selectedCount === 0) return
  let settled = false
  bulkDeleteProgress.value = {
    current: 0,
    total: selectedCount,
    percent: 0,
    text: '正在移除书架记录',
  }
  try {
    const result = await shelfStore.bulkDelete((progress) => {
      const percent = progress.total > 0
        ? Math.round((progress.current / progress.total) * 100)
        : 0
      bulkDeleteProgress.value = {
        current: progress.current,
        total: selectedCount,
        percent: (
          progress.stage === 'deleting'
            ? 0
            : progress.stage === 'removed'
              ? 0
              : progress.stage === 'cleanup'
              ? Math.round(percent * 0.7)
              : progress.stage === 'files'
                ? 70 + Math.round((progress.current / Math.max(1, progress.total)) * 20)
                : 95
        ),
        text: progress.stage === 'deleting'
          ? '正在移除书架记录'
          : progress.stage === 'removed'
            ? '正在清理缓存'
          : progress.stage === 'done'
            ? '正在整理书架'
            : progress.stage === 'files'
              ? `正在清理本地文件 ${progress.current}/${progress.total}`
              : '删除中',
      }
    })
    if (result.removedUrls.length === 0) {
      bulkDeleteProgress.value = {
        ...bulkDeleteProgress.value!,
        current: 0,
        total: selectedCount,
        percent: 100,
        text: '未删除书籍',
      }
      appStore.showToast('选中的书已不在书架', 'warning')
    } else if (
      result.cacheCleanupFailed > 0
      || result.fileCleanupFailed > 0
      || result.removedUrls.length < selectedCount
    ) {
      bulkDeleteProgress.value = {
        ...bulkDeleteProgress.value!,
        current: result.deleted,
        total: selectedCount,
        percent: 100,
        text: `已删除 ${result.deleted} 本`,
      }
      const failedParts = [
        result.cacheCleanupFailed > 0 ? `${result.cacheCleanupFailed} 本缓存` : '',
        result.fileCleanupFailed > 0 ? `${result.fileCleanupFailed} 本文件` : '',
      ].filter(Boolean)
      const unmatched = selectedCount - result.removedUrls.length
      if (unmatched > 0) failedParts.push(`${unmatched} 本未匹配`)
      appStore.showToast(
        `已删除 ${result.deleted} 本书${failedParts.length ? `，${failedParts.join('、')}` : ''}`,
        'warning',
      )
    } else {
      bulkDeleteProgress.value = {
        ...bulkDeleteProgress.value!,
        current: result.deleted,
        total: selectedCount,
        percent: 100,
        text: '删除完成',
      }
      appStore.showToast(`成功删除 ${result.deleted} 本书`, 'success')
    }
    settled = true
  } catch (e: any) {
    appStore.showToast(e.message || '批量删除失败', 'error')
  } finally {
    if (settled) {
      window.setTimeout(() => {
        bulkDeleteProgress.value = null
      }, 900)
    } else {
      bulkDeleteProgress.value = null
    }
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
  groupTabsResizeObserver?.disconnect()
  groupTabsResizeObserver = null
  if (groupTabsScrollTimer !== null) {
    window.clearTimeout(groupTabsScrollTimer)
    groupTabsScrollTimer = null
  }
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

.modal-overlay {
  position: fixed;
  inset: 0;
  z-index: var(--z-overlay);
  display: flex;
  align-items: center;
  justify-content: center;
  padding:
    calc(16px + var(--safe-area-top))
    calc(16px + var(--safe-area-right))
    calc(16px + var(--safe-area-bottom))
    calc(16px + var(--safe-area-left));
  background: rgba(0, 0, 0, 0.42);
  backdrop-filter: blur(4px);
}

.modal-content {
  width: min(460px, 100%);
  max-height: calc(var(--app-height, 100dvh) - var(--safe-area-top) - var(--safe-area-bottom) - 32px);
  display: flex;
  flex-direction: column;
  overflow: hidden;
  border: 1px solid var(--color-border-light);
  border-radius: var(--radius-xl);
  background: var(--color-bg-elevated);
  box-shadow: var(--shadow-xl);
}

.modal-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: var(--space-4);
  padding: var(--space-4) var(--space-5);
  border-bottom: 1px solid var(--color-border-light);
}

.modal-header h3 {
  margin: 0;
  font-size: var(--text-lg);
  font-weight: 600;
}

.close-btn {
  width: 30px;
  height: 30px;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  flex-shrink: 0;
  border-radius: var(--radius-md);
  background: transparent;
  color: var(--color-text-tertiary);
}

.close-btn:hover {
  background: var(--color-bg-hover);
  color: var(--color-text);
}

.close-btn:disabled {
  opacity: 0.45;
  cursor: not-allowed;
}

.close-btn svg {
  width: 17px;
  height: 17px;
}

.modal-body {
  padding: var(--space-5);
  display: flex;
  flex-direction: column;
  gap: var(--space-5);
  overflow-y: auto;
}

.modal-footer {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: var(--space-3);
  padding: var(--space-3) var(--space-5);
  border-top: 1px solid var(--color-border-light);
}

.import-choice {
  display: grid;
  grid-template-columns: repeat(2, minmax(0, 1fr));
  gap: var(--space-3);
}

.import-choice-btn {
  min-height: 82px;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: var(--space-2);
  border: 1px solid var(--color-border);
  border-radius: var(--radius-lg);
  background: var(--color-bg-sunken);
  color: var(--color-text-secondary);
  font-size: var(--text-sm);
  font-weight: 600;
}

.import-choice-btn:hover:not(:disabled) {
  border-color: var(--color-primary-soft-border);
  background: var(--color-primary-soft-bg);
  color: var(--color-primary-soft-text);
}

.import-choice-btn:disabled {
  opacity: 0.55;
  cursor: wait;
}

.import-choice-btn svg {
  width: 22px;
  height: 22px;
}

.import-group {
  display: flex;
  flex-direction: column;
  gap: var(--space-2);
}

.import-label {
  color: var(--color-text-secondary);
  font-size: var(--text-sm);
  font-weight: 600;
}

.import-group-list {
  max-height: 190px;
  display: flex;
  flex-direction: column;
  gap: 2px;
  overflow-y: auto;
}

.import-group-item {
  min-height: 36px;
  display: flex;
  align-items: center;
  gap: var(--space-3);
  padding: 0 var(--space-3);
  border-radius: var(--radius-md);
  color: var(--color-text-secondary);
  font-size: var(--text-sm);
  cursor: pointer;
}

.import-group-item:hover {
  background: var(--color-bg-hover);
}

.local-import-modal .import-group-item.disabled {
  opacity: 0.55;
  cursor: wait;
}

.import-group-item input {
  width: 17px;
  height: 17px;
  flex-shrink: 0;
  accent-color: var(--color-primary);
}

.empty-group-hint {
  margin: 0;
  padding: var(--space-2) var(--space-3);
  color: var(--color-text-tertiary);
  font-size: var(--text-sm);
}

.new-group-trigger {
  align-self: flex-start;
  display: inline-flex;
  align-items: center;
  gap: 5px;
  min-height: 30px;
  padding: 0 var(--space-2);
  border-radius: var(--radius-sm);
  background: transparent;
  color: var(--color-primary);
  font-size: var(--text-sm);
}

.new-group-trigger:hover {
  background: var(--color-primary-soft-bg);
}

.new-group-trigger svg {
  width: 15px;
  height: 15px;
}

.new-group-row {
  display: flex;
  gap: var(--space-2);
}

.new-group-input {
  flex: 1;
  min-width: 0;
  padding: 8px var(--space-3);
  border: 1px solid var(--color-border);
  border-radius: var(--radius-md);
  background: var(--color-bg-sunken);
  color: var(--color-text);
}

.new-group-input:focus {
  outline: none;
  border-color: var(--color-primary);
}

.new-group-save,
.cancel-btn {
  min-height: 34px;
  padding: 0 var(--space-4);
  border: 1px solid var(--color-border);
  border-radius: var(--radius-md);
  background: transparent;
  color: var(--color-text-secondary);
  font-size: var(--text-sm);
}

.new-group-save {
  border-color: var(--color-primary-soft-border);
  background: var(--color-primary-soft-bg);
  color: var(--color-primary-soft-text);
}

.new-group-save:disabled,
.cancel-btn:disabled {
  opacity: 0.5;
  cursor: not-allowed;
}

.import-progress {
  display: flex;
  flex-direction: column;
  gap: 6px;
  color: var(--color-text-tertiary);
  font-size: var(--text-xs);
}

.progress-track {
  height: 5px;
  overflow: hidden;
  border-radius: 999px;
  background: var(--color-bg-sunken);
}

.progress-fill {
  height: 100%;
  border-radius: inherit;
  background: var(--color-primary);
  transition: width var(--duration-fast);
}

@media (max-width: 480px) {
  .import-choice {
    grid-template-columns: 1fr;
  }

  .import-choice-btn {
    min-height: 58px;
    flex-direction: row;
  }
}

.import-hint {
  color: var(--color-text-tertiary);
  font-size: var(--text-xs);
}

.import-summary {
  margin: 0;
  color: var(--color-text-secondary);
  font-size: var(--text-sm);
  line-height: 1.55;
  overflow-wrap: anywhere;
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

.shelf-btn .spinning,
.batch-btn .spinning {
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
  position: relative;
  overflow-x: auto;
  scrollbar-width: none;
  -ms-overflow-style: none;
}

.tabs-scroll.is-group-dragging {
  cursor: grabbing;
}

.group-drop-marker {
  position: absolute;
  z-index: 2;
  width: 2px;
  pointer-events: none;
  background: var(--color-primary);
  border-radius: 999px;
  box-shadow: 0 0 0 2px color-mix(in srgb, var(--color-primary) 16%, transparent);
}

.group-drag-ghost {
  position: fixed;
  z-index: calc(var(--z-sticky) + 10);
  pointer-events: none;
  display: flex;
  align-items: center;
  justify-content: center;
  padding: 0 var(--space-3);
  overflow: hidden;
  border: 1px solid var(--color-primary);
  border-radius: var(--radius-md);
  background: color-mix(in srgb, var(--color-bg-elevated) 96%, transparent);
  box-shadow: var(--shadow-xl);
  color: var(--color-primary);
  font-size: var(--text-sm);
  font-weight: 500;
  white-space: nowrap;
}

.tabs-scroll-btn {
  width: 28px;
  height: 28px;
  flex-shrink: 0;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  border: 1px solid var(--color-border-light);
  border-radius: var(--radius-md);
  background: var(--color-bg-elevated);
  color: var(--color-text-tertiary);
  cursor: pointer;
  transition: color var(--duration-fast), background var(--duration-fast), border-color var(--duration-fast);
}

.tabs-scroll-btn:hover {
  color: var(--color-text);
  background: var(--color-bg-hover);
  border-color: var(--color-border);
}

.tabs-scroll-btn svg {
  width: 15px;
  height: 15px;
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
  cursor: pointer;
  touch-action: none;
  user-select: none;
}

.tab-item:hover {
  color: var(--color-text-secondary);
}

.tab-item.active {
  color: var(--color-primary);
  border-bottom-color: var(--color-primary);
}

.tab-item.is-group-dragging {
  opacity: 0.35;
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

.batch-btn:disabled {
  opacity: 0.6;
  cursor: wait;
}

.batch-btn svg {
  width: 16px;
  height: 16px;
}

.bulk-delete-progress {
  position: fixed;
  left: 50%;
  bottom: calc(76px + 88px);
  z-index: calc(var(--z-sticky) + 4);
  width: min(420px, calc(100% - var(--space-8)));
  padding: var(--space-3) var(--space-4);
  transform: translateX(-50%);
  border: 1px solid var(--color-border);
  border-radius: var(--radius-lg);
  background: var(--color-bg-elevated);
  box-shadow: var(--shadow-lg);
}

.bulk-delete-progress-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: var(--space-4);
  margin-bottom: var(--space-2);
  color: var(--color-text-secondary);
  font-size: var(--text-xs);
}

.bulk-delete-progress-head strong {
  flex-shrink: 0;
  color: var(--color-text);
  font-variant-numeric: tabular-nums;
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

  .bulk-delete-progress {
    bottom: calc(76px + 84px);
  }
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
