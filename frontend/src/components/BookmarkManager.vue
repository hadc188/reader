<template>
  <Teleport to="body">
    <Transition name="fade">
      <div v-if="modelValue" class="modal-overlay" @click="close"></div>
    </Transition>
    <Transition name="scale">
      <div v-if="modelValue" class="modal-container" @click.self="close">
        <section
          class="bookmark-modal"
          role="dialog"
          aria-modal="true"
          aria-labelledby="bookmark-manager-title"
          @keydown.esc.stop.prevent="close"
        >
          <header class="modal-header">
            <div>
              <h2 id="bookmark-manager-title">书签管理</h2>
              <p class="subtitle">查看全部标记书签，点击可回到原书章节</p>
            </div>
            <button class="icon-btn" type="button" aria-label="关闭" @click="close">
              <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                <path d="M18 6 6 18M6 6l12 12" />
              </svg>
            </button>
          </header>

          <div class="toolbar">
            <div class="bookmark-summary">
              <strong>{{ searchQuery ? filteredBookmarks.length : bookmarks.length }}</strong>
              <span>{{ searchQuery ? '条匹配结果' : `条书签 · ${groups.length} 本书` }}</span>
            </div>
            <label class="bookmark-search">
              <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" aria-hidden="true">
                <circle cx="11" cy="11" r="8" />
                <path d="m21 21-4.3-4.3" />
              </svg>
              <input
                v-model.trim="searchQuery"
                type="search"
                placeholder="搜索书名、章节或摘录"
                aria-label="搜索书签"
              >
              <button
                v-if="searchQuery"
                type="button"
                aria-label="清除搜索"
                @click="searchQuery = ''"
              >
                <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                  <path d="M18 6 6 18M6 6l12 12" />
                </svg>
              </button>
            </label>
            <div class="toolbar-actions">
              <label class="format-select-wrap">
                <select
                  v-model="exportFormat"
                  class="format-select"
                  :disabled="!bookmarks.length || exporting"
                  aria-label="选择书签导出格式"
                  @change="handleFormatChange"
                >
                  <option value="" disabled hidden>选择格式</option>
                  <option value="json">JSON</option>
                  <option value="md">Markdown</option>
                  <option value="xmnote">纸间书摘</option>
                </select>
              </label>
              <button
                class="action-btn icon-action"
                type="button"
                :disabled="loading"
                aria-label="刷新书签"
                title="刷新书签"
                @click="loadBookmarks"
              >
                <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                  <path d="M21 12a9 9 0 0 0-15.55-6.2L3 8" />
                  <path d="M3 3v5h5M3 12a9 9 0 0 0 15.55 6.2L21 16" />
                  <path d="M21 21v-5h-5" />
                </svg>
              </button>
              <button
                v-if="!manageMode"
                class="action-btn"
                type="button"
                :disabled="!filteredGroups.length"
                @click="toggleManageMode"
              >
                <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                  <path d="M9 11l3 3L22 4" />
                  <path d="M21 12v7a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h11" />
                </svg>
                管理
              </button>
              <template v-else>
                <button
                  class="action-btn"
                  type="button"
                  :disabled="deleting || !selectedKeys.size"
                  :class="{ danger: selectedKeys.size > 0 }"
                  @click="removeSelected"
                >
                  {{ deleting ? '删除中...' : `删除选中 (${selectedKeys.size})` }}
                </button>
                <button class="action-btn" type="button" @click="exitManageMode">完成</button>
              </template>
            </div>
          </div>

          <div class="bookmark-body">
            <div v-if="loading && !bookmarks.length" class="empty-state">正在加载书签...</div>
            <div v-else-if="errorMessage" class="empty-state error-state">
              <span>{{ errorMessage }}</span>
              <button class="action-btn" type="button" @click="loadBookmarks">重新加载</button>
            </div>
            <div v-else-if="!bookmarks.length" class="empty-state">暂无书签</div>
            <div v-else-if="!filteredGroups.length" class="empty-state">
              没有匹配的书签
            </div>
            <template v-else>
              <section v-for="group in filteredGroups" :key="group.key" class="bookmark-group">
                <header class="group-header">
                  <div class="book-identity">
                    <img v-if="group.coverUrl" :src="group.coverUrl" alt="" class="book-cover">
                    <button
                      v-if="manageMode"
                      class="group-check"
                      :class="{ checked: groupAllSelected(group) }"
                      type="button"
                      :aria-label="groupAllSelected(group) ? `取消选择 ${group.bookName}` : `选择 ${group.bookName}`"
                      @click="toggleGroupSelection(group)"
                    >
                      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5">
                        <path d="m5 12 4 4L19 6" />
                      </svg>
                    </button>
                    <div class="book-title">
                      <strong>{{ group.bookName }}</strong>
                      <span>{{ group.bookAuthor || '未知作者' }}</span>
                    </div>
                  </div>
                  <span class="group-count">{{ group.items.length }} 条</span>
                </header>

                <div
                  v-for="bookmark in group.items"
                  :key="bookmarkKey(bookmark)"
                  class="bookmark-row"
                  :class="{ selected: selectedKeys.has(bookmarkKey(bookmark)) }"
                >
                  <button
                    v-if="manageMode"
                    class="bookmark-check"
                    :class="{ checked: selectedKeys.has(bookmarkKey(bookmark)) }"
                    type="button"
                    :aria-label="selectedKeys.has(bookmarkKey(bookmark)) ? '取消选择书签' : '选择书签'"
                    @click="toggleBookmarkSelection(bookmark)"
                  >
                    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5">
                      <path d="m5 12 4 4L19 6" />
                    </svg>
                  </button>
                  <button
                    class="bookmark-main"
                    type="button"
                    :title="group.inShelf ? '回到阅读位置' : '原书不在书架，无法打开'"
                    :disabled="manageMode || !group.inShelf || navigatingKey === bookmarkKey(bookmark)"
                    @click="openBookmark(group, bookmark)"
                  >
                    <span class="chapter-line">
                      <strong>{{ bookmark.chapterName || '未命名章节' }}</strong>
                      <time>{{ formatDate(bookmark.time) }}</time>
                    </span>
                    <span v-if="bookmark.bookText" class="excerpt">{{ bookmark.bookText }}</span>
                    <span v-if="bookmark.content" class="note">{{ bookmark.content }}</span>
                    <span v-if="!group.inShelf" class="missing-book">原书不在书架</span>
                  </button>
                  <button
                    v-if="!manageMode"
                    class="bookmark-delete"
                    type="button"
                    :disabled="deleting"
                    aria-label="删除书签"
                    title="删除书签"
                    @click="removeOne(bookmark)"
                  >
                    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                      <path d="M3 6h18M8 6V4h8v2M19 6l-1 15H6L5 6M10 11v6M14 11v6" />
                    </svg>
                  </button>
                  <span v-else class="row-action" aria-hidden="true">
                    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                      <path d="m9 18 6-6-6-6" />
                    </svg>
                  </span>
                </div>
              </section>
            </template>
          </div>
        </section>
      </div>
    </Transition>
    <XmnoteManager
      v-model="showXmnoteManager"
      :bookmarks="filteredBookmarks"
    />
  </Teleport>
</template>

<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { useRouter } from 'vue-router'
import {
  deleteBookmark,
  deleteBookmarks,
  getBookmarks,
  saveBookmarkExportAs,
} from '../api/bookmark'
import { getCoverUrl } from '../api/bookshelf'
import { useAppStore } from '../stores/app'
import { useBookshelfStore } from '../stores/bookshelf'
import { useReaderStore } from '../stores/reader'
import type { Book, Bookmark } from '../types'
import XmnoteManager from './XmnoteManager.vue'
import { normalizeBookmarkPosition } from './bookmarkNavigation'
import {
  bookmarkExportFilename,
  buildBookmarkExportJson,
  buildBookmarkExportMarkdown,
} from '../utils/bookmarkExport'

const props = defineProps<{
  modelValue: boolean
}>()

const emit = defineEmits<{
  'update:modelValue': [value: boolean]
}>()

interface BookmarkGroup {
  key: string
  bookName: string
  bookAuthor: string
  coverUrl: string
  inShelf: boolean
  book?: Book
  items: Bookmark[]
}

const router = useRouter()
const appStore = useAppStore()
const shelfStore = useBookshelfStore()
const readerStore = useReaderStore()
const bookmarks = ref<Bookmark[]>([])
const loading = ref(false)
const errorMessage = ref('')
const navigatingKey = ref('')
const searchQuery = ref('')
const manageMode = ref(false)
const selectedKeys = ref<Set<string>>(new Set())
const deleting = ref(false)
type ExportFormat = '' | 'json' | 'md' | 'xmnote'
const exportFormat = ref<ExportFormat>('')
const showXmnoteManager = ref(false)
const exporting = ref(false)
let navigationRequest = 0

const filteredBookmarks = computed(() => {
  const query = searchQuery.value.trim().toLocaleLowerCase('zh-CN')
  if (!query) return bookmarks.value
  return bookmarks.value.filter((bookmark) => [
    bookmark.bookName,
    bookmark.bookAuthor,
    bookmark.chapterName,
    bookmark.bookText,
    bookmark.content,
  ].some((value) => (
    typeof value === 'string' && value.toLocaleLowerCase('zh-CN').includes(query)
  )))
})

const groups = computed<BookmarkGroup[]>(() => {
  const shelfByKey = new Map(
    shelfStore.books.map((book) => [`${book.name}\u0000${book.author}`, book]),
  )
  const grouped = new Map<string, Bookmark[]>()
  for (const bookmark of filteredBookmarks.value) {
    const key = `${bookmark.bookName}\u0000${bookmark.bookAuthor}`
    const list = grouped.get(key)
    if (list) list.push(bookmark)
    else grouped.set(key, [bookmark])
  }
  return [...grouped.entries()].map(([key, items]) => {
    const book = shelfByKey.get(key)
    return {
      key,
      bookName: items[0]?.bookName || '未知书籍',
      bookAuthor: items[0]?.bookAuthor || '',
      coverUrl: getCoverUrl(book?.customCoverUrl || book?.coverUrl, book?.origin),
      inShelf: Boolean(book),
      book,
      items,
    }
  })
})

const filteredGroups = computed(() => groups.value.filter((group) => group.items.length))

watch(() => props.modelValue, (visible) => {
  if (visible) void loadBookmarks()
})

watch(searchQuery, () => {
  selectedKeys.value = new Set(
    [...selectedKeys.value].filter((key) => (
      filteredBookmarks.value.some((bookmark) => bookmarkKey(bookmark) === key)
    )),
  )
})

function close() {
  emit('update:modelValue', false)
}

async function loadBookmarks() {
  if (loading.value) return
  loading.value = true
  errorMessage.value = ''
  try {
    const [, nextBookmarks] = await Promise.all([
      shelfStore.fetchBooks({ prefetch: false }),
      getBookmarks(),
    ])
    bookmarks.value = nextBookmarks
  } catch (error) {
    errorMessage.value = (error as Error).message || '加载书签失败'
  } finally {
    loading.value = false
  }
}

async function openBookmark(group: BookmarkGroup, bookmark: Bookmark) {
  if (!group.book) return
  const request = ++navigationRequest
  const key = bookmarkKey(bookmark)
  navigatingKey.value = key
  try {
    close()
    const targetIndex = bookmark.chapterIndex ?? 0
    const targetProgress = normalizeBookmarkPosition(bookmark.chapterPos)
    await readerStore.loadBook(group.book, false, {
      initialChapterIndex: targetIndex,
      initialProgressOverride: targetProgress,
    })
    if (request !== navigationRequest) return
    await router.push('/reader')
  } catch (error) {
    appStore.showToast((error as Error).message || '打开书签失败', 'error')
  } finally {
    if (request === navigationRequest) navigatingKey.value = ''
  }
}

function bookmarkKey(bookmark: Bookmark) {
  return [
    bookmark.bookName,
    bookmark.bookAuthor,
    bookmark.chapterIndex,
    bookmark.chapterPos,
    bookmark.time,
    bookmark.bookText,
  ].join('|')
}

function formatDate(value?: number) {
  if (!value) return ''
  const date = new Date(value)
  if (Number.isNaN(date.getTime())) return ''
  return date.toLocaleString('zh-CN', {
    year: 'numeric',
    month: '2-digit',
    day: '2-digit',
    hour: '2-digit',
    minute: '2-digit',
  })
}

function groupAllSelected(group: BookmarkGroup) {
  return group.items.length > 0
    && group.items.every((bookmark) => selectedKeys.value.has(bookmarkKey(bookmark)))
}

function toggleBookmarkSelection(bookmark: Bookmark) {
  const next = new Set(selectedKeys.value)
  const key = bookmarkKey(bookmark)
  if (next.has(key)) next.delete(key)
  else next.add(key)
  selectedKeys.value = next
}

function toggleGroupSelection(group: BookmarkGroup) {
  const next = new Set(selectedKeys.value)
  const selected = groupAllSelected(group)
  for (const bookmark of group.items) {
    const key = bookmarkKey(bookmark)
    if (selected) next.delete(key)
    else next.add(key)
  }
  selectedKeys.value = next
}

function toggleManageMode() {
  manageMode.value = true
}

function exitManageMode() {
  manageMode.value = false
  selectedKeys.value = new Set()
}

async function removeOne(bookmark: Bookmark) {
  const ok = await appStore.confirmDialog(
    `确定删除「${bookmark.chapterName || '未命名章节'}」的书签吗？`,
    { title: '删除书签', danger: true },
  )
  if (!ok || deleting.value) return
  deleting.value = true
  try {
    await deleteBookmark(bookmark)
    const next = new Set(selectedKeys.value)
    next.delete(bookmarkKey(bookmark))
    selectedKeys.value = next
    appStore.showToast('书签已删除', 'success')
    await reloadAfterMutation()
  } catch (error) {
    appStore.showToast((error as Error).message || '删除书签失败', 'error')
  } finally {
    deleting.value = false
  }
}

async function removeSelected() {
  const items = bookmarks.value.filter((bookmark) => selectedKeys.value.has(bookmarkKey(bookmark)))
  if (!items.length || deleting.value) return
  const ok = await appStore.confirmDialog(
    `确定删除选中的 ${items.length} 条书签吗？`,
    { title: '批量删除书签', danger: true },
  )
  if (!ok) return
  deleting.value = true
  try {
    await deleteBookmarks(items)
    selectedKeys.value = new Set()
    manageMode.value = false
    appStore.showToast(`已删除 ${items.length} 条书签`, 'success')
    await reloadAfterMutation()
  } catch (error) {
    appStore.showToast((error as Error).message || '删除书签失败', 'error')
  } finally {
    deleting.value = false
  }
}

async function reloadAfterMutation() {
  const [, nextBookmarks] = await Promise.all([
    shelfStore.fetchBooks({ prefetch: false }),
    getBookmarks(),
  ])
  bookmarks.value = nextBookmarks
}

async function handleFormatChange() {
  const format = exportFormat.value
  if (!format) return
  if (format === 'xmnote') {
    exportFormat.value = ''
    showXmnoteManager.value = true
    return
  }
  exportFormat.value = ''
  await downloadExport(format)
}

async function downloadExport(format: 'json' | 'md') {
  if (exporting.value) return
  exporting.value = true
  const sorted = [...bookmarks.value].sort((a, b) => (
    `${a.bookName}\u0000${a.bookAuthor}`.localeCompare(`${b.bookName}\u0000${b.bookAuthor}`)
    || (a.chapterIndex || 0) - (b.chapterIndex || 0)
    || (a.time || 0) - (b.time || 0)
  ))
  try {
    const content = format === 'json'
      ? buildBookmarkExportJson(sorted)
      : buildBookmarkExportMarkdown(sorted)
    const result = await saveBookmarkExportAs(
      bookmarkExportFilename(format),
      content,
      format,
    )
    if (result.saved) {
      appStore.showToast(`书签已导出为 ${format === 'json' ? 'JSON' : 'Markdown'}`, 'success')
    }
  } catch (error) {
    appStore.showToast((error as Error).message || '导出书签失败', 'error')
  } finally {
    exporting.value = false
  }
}
</script>

<style scoped>
.modal-overlay {
  position: fixed;
  inset: 0;
  z-index: var(--z-overlay);
  background: rgba(15, 23, 42, 0.46);
  backdrop-filter: blur(6px);
}

.modal-container {
  position: fixed;
  inset: 0;
  z-index: var(--z-modal);
  display: flex;
  align-items: center;
  justify-content: center;
  padding: var(--space-6);
}

.bookmark-modal {
  display: flex;
  flex-direction: column;
  width: min(860px, 100%);
  max-height: min(88vh, 880px);
  overflow: hidden;
  border: 1px solid var(--color-border-light);
  border-radius: var(--radius-xl);
  background: var(--color-bg-elevated);
  box-shadow: var(--shadow-xl);
}

.modal-header {
  display: flex;
  align-items: flex-start;
  justify-content: space-between;
  gap: var(--space-4);
  padding: var(--space-5) var(--space-6);
  border-bottom: 1px solid var(--color-divider);
}

.modal-header h2 {
  font-size: var(--text-xl);
  font-weight: 700;
}

.subtitle {
  margin-top: var(--space-1);
  color: var(--color-text-secondary);
  font-size: var(--text-sm);
}

.icon-btn {
  display: flex;
  flex: 0 0 auto;
  align-items: center;
  justify-content: center;
  width: 38px;
  height: 38px;
  border: 0;
  border-radius: var(--radius-md);
  color: var(--color-text-secondary);
  background: none;
}

.icon-btn:hover {
  background: var(--color-bg-hover);
}

.icon-btn svg {
  width: 18px;
  height: 18px;
}

.toolbar {
  display: grid;
  grid-template-columns: auto minmax(220px, 1fr) auto;
  align-items: center;
  gap: var(--space-3);
  padding: var(--space-3) var(--space-6);
  border-bottom: 1px solid var(--color-divider);
}

.bookmark-summary {
  display: flex;
  align-items: baseline;
  gap: 5px;
  color: var(--color-text-tertiary);
  font-size: var(--text-xs);
}

.bookmark-summary strong {
  color: var(--color-text);
  font-size: var(--text-lg);
}

.toolbar-actions {
  display: flex;
  align-items: center;
  flex-wrap: nowrap;
  justify-content: flex-end;
  gap: var(--space-2);
}

.bookmark-search {
  display: flex;
  width: min(360px, 100%);
  justify-self: center;
  min-height: 34px;
  align-items: center;
  gap: 6px;
  padding: 0 var(--space-2);
  border: 1px solid var(--color-border-light);
  border-radius: var(--radius-md);
  background: var(--color-bg-sunken);
}

.bookmark-search:focus-within {
  border-color: var(--color-primary);
  box-shadow: 0 0 0 2px var(--color-primary-soft-bg);
}

.bookmark-search > svg {
  width: 15px;
  height: 15px;
  flex: 0 0 auto;
  color: var(--color-text-tertiary);
}

.bookmark-search input {
  width: 100%;
  min-width: 0;
  border: 0;
  outline: 0;
  background: transparent;
  color: var(--color-text);
  font: inherit;
  font-size: var(--text-xs);
}

.bookmark-search > button {
  display: flex;
  flex: 0 0 auto;
  padding: 2px;
  border: 0;
  border-radius: 4px;
  background: transparent;
  color: var(--color-text-tertiary);
}

.bookmark-search > button:hover {
  background: var(--color-bg-hover);
  color: var(--color-text);
}

.bookmark-search > button svg {
  width: 13px;
  height: 13px;
}

.format-select-wrap {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  color: var(--color-text-tertiary);
  font-size: var(--text-xs);
}

.format-select {
  min-height: 34px;
  padding: 0 var(--space-2);
  border: 1px solid var(--color-border-light);
  border-radius: var(--radius-md);
  background: var(--color-bg-elevated);
  color: var(--color-text);
  font: inherit;
  font-size: var(--text-xs);
  font-weight: 600;
}

.action-btn {
  display: inline-flex;
  min-height: 34px;
  align-items: center;
  justify-content: center;
  gap: 6px;
  padding: 0 var(--space-3);
  border: 1px solid var(--color-border-light);
  border-radius: var(--radius-md);
  background: var(--color-bg-sunken);
  color: var(--color-text);
  font: inherit;
  font-size: var(--text-xs);
  font-weight: 600;
}

.action-btn svg {
  width: 15px;
  height: 15px;
}

.icon-action {
  width: 34px;
  padding: 0;
}

.action-btn.primary {
  border-color: var(--color-primary-soft-border);
  background: var(--color-primary-soft-bg);
  color: var(--color-primary-soft-text);
}

.action-btn.primary:hover:not(:disabled) {
  border-color: var(--color-primary);
  background: var(--color-primary-soft-bg-hover);
}

.action-btn:hover:not(:disabled) {
  border-color: var(--color-border);
  background: var(--color-bg-hover);
}

.action-btn.danger {
  border-color: rgba(220, 38, 38, 0.3);
  color: var(--color-danger);
}

.action-btn:disabled {
  opacity: 0.45;
}

.bookmark-body {
  flex: 1;
  min-height: 0;
  overflow-y: auto;
  padding: var(--space-3) var(--space-6) var(--space-6);
}

.bookmark-group {
  margin-top: var(--space-3);
  border: 1px solid var(--color-border-light);
  border-radius: var(--radius-lg);
  background: var(--color-bg-sunken);
  overflow: hidden;
}

.group-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: var(--space-3);
  padding: var(--space-3) var(--space-4);
  border-bottom: 1px solid var(--color-divider);
}

.group-check,
.bookmark-check {
  display: flex;
  flex: 0 0 auto;
  align-items: center;
  justify-content: center;
  width: 22px;
  height: 22px;
  padding: 0;
  border: 1px solid var(--color-border);
  border-radius: 5px;
  background: var(--color-bg-elevated);
  color: transparent;
}

.group-check.checked,
.bookmark-check.checked {
  border-color: var(--color-primary-soft-border);
  background: var(--color-primary-soft-bg);
  color: var(--color-primary-soft-text);
}

.group-check svg,
.bookmark-check svg {
  width: 14px;
  height: 14px;
}

.book-identity {
  display: flex;
  align-items: center;
  gap: var(--space-3);
  min-width: 0;
}

.book-cover {
  width: 30px;
  height: 40px;
  flex: 0 0 auto;
  border-radius: 4px;
  object-fit: cover;
  background: var(--color-bg-hover);
}

.book-title {
  display: grid;
  min-width: 0;
  gap: 2px;
}

.book-title strong {
  overflow: hidden;
  color: var(--color-text);
  font-size: var(--text-sm);
  text-overflow: ellipsis;
  white-space: nowrap;
}

.book-title span,
.group-count {
  color: var(--color-text-tertiary);
  font-size: var(--text-xs);
}

.group-count {
  flex: 0 0 auto;
}

.bookmark-row {
  display: flex;
  width: 100%;
  align-items: center;
  justify-content: space-between;
  gap: var(--space-4);
  padding: var(--space-2) var(--space-3);
  border: 0;
  border-bottom: 1px solid var(--color-divider);
  background: transparent;
  color: inherit;
  text-align: left;
}

.bookmark-row:last-child {
  border-bottom: 0;
}

.bookmark-row:hover {
  background: var(--color-bg-hover);
}

.bookmark-row.selected {
  background: var(--color-primary-soft-bg);
}

.bookmark-check {
  margin-right: var(--space-1);
}

.bookmark-main {
  display: grid;
  flex: 1;
  width: auto;
  min-width: 0;
  padding: 0;
  border: 0;
  background: transparent;
  color: inherit;
  text-align: left;
  gap: 5px;
}

.bookmark-main:hover:not(:disabled) .chapter-line strong {
  color: var(--color-primary);
}

.bookmark-main:disabled {
  cursor: default;
}

.chapter-line {
  display: flex;
  align-items: baseline;
  gap: var(--space-3);
}

.chapter-line strong {
  overflow: hidden;
  color: var(--color-text);
  font-size: var(--text-sm);
  text-overflow: ellipsis;
  white-space: nowrap;
}

.chapter-line time {
  flex: 0 0 auto;
  color: var(--color-text-tertiary);
  font-size: var(--text-xs);
}

.excerpt,
.note {
  display: -webkit-box;
  overflow: hidden;
  -webkit-box-orient: vertical;
  -webkit-line-clamp: 2;
  color: var(--color-text-secondary);
  font-size: var(--text-xs);
  line-height: 1.5;
}

.note {
  padding-left: var(--space-2);
  border-left: 2px solid var(--color-primary-soft-border);
  color: var(--color-text);
}

.missing-book {
  color: var(--color-text-tertiary);
  font-size: var(--text-xs);
}

.row-action {
  display: flex;
  flex: 0 0 auto;
  color: var(--color-text-tertiary);
}

.row-action svg {
  width: 16px;
  height: 16px;
}

.bookmark-delete {
  display: flex;
  flex: 0 0 auto;
  align-items: center;
  justify-content: center;
  width: 30px;
  height: 30px;
  padding: 0;
  border: 0;
  border-radius: var(--radius-md);
  background: transparent;
  color: var(--color-text-tertiary);
  opacity: 0;
}

.bookmark-row:hover .bookmark-delete,
.bookmark-delete:focus-visible {
  opacity: 1;
}

.bookmark-delete:hover:not(:disabled) {
  background: rgba(220, 38, 38, 0.08);
  color: var(--color-danger);
}

.bookmark-delete:disabled {
  opacity: 0.35;
}

.bookmark-delete svg {
  width: 16px;
  height: 16px;
}

.empty-state {
  display: flex;
  min-height: 260px;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: var(--space-3);
  color: var(--color-text-tertiary);
  font-size: var(--text-sm);
}

.error-state {
  color: var(--color-danger);
}

@media (max-width: 700px) {
  .modal-container {
    padding: 0;
  }

  .bookmark-modal {
    width: 100%;
    max-height: 100dvh;
    height: 100dvh;
    border: 0;
    border-radius: 0;
  }

  .modal-header,
  .toolbar,
  .bookmark-body {
    padding-inline: var(--space-4);
  }

  .toolbar {
    align-items: stretch;
    grid-template-columns: minmax(0, 1fr) auto;
  }

  .toolbar-actions {
    display: grid;
    grid-auto-flow: column;
    grid-auto-columns: max-content;
    justify-content: end;
    width: auto;
  }

  .bookmark-search {
    width: 100%;
    grid-column: 1 / -1;
    grid-row: 2;
  }

  .bookmark-summary {
    grid-column: 1;
    grid-row: 1;
  }

  .toolbar-actions {
    grid-column: 2;
    grid-row: 1;
  }

  .format-select {
    max-width: 150px;
  }

  .bookmark-delete {
    opacity: 1;
  }

  .chapter-line {
    align-items: flex-start;
    flex-direction: column;
    gap: 2px;
  }
}
</style>
