<template>
  <Teleport to="body">
    <Transition name="fade">
      <div v-if="modelValue" class="modal-overlay" @click="close"></div>
    </Transition>
    <Transition name="scale">
      <div v-if="modelValue" class="modal-container" @click.self="close">
        <section
          class="xmnote-modal"
          role="dialog"
          aria-modal="true"
          aria-labelledby="xmnote-manager-title"
          @keydown.esc.stop.prevent="close"
        >
          <header class="modal-header">
            <div>
              <h2 id="xmnote-manager-title">纸间书摘</h2>
              <p class="subtitle">把书架阅读进度与书签摘录发送到纸间书摘</p>
            </div>
            <button class="icon-btn" type="button" aria-label="关闭" @click="close">
              <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                <path d="M18 6 6 18M6 6l12 12" />
              </svg>
            </button>
          </header>

          <div class="xmnote-body">
            <section class="settings-panel">
              <div class="panel-heading">
                <div>
                  <h3>接收地址</h3>
                  <p>在纸间书摘中打开「通过 API 导入」，填写页面显示的局域网地址。</p>
                </div>
              </div>
              <label class="field-label" for="xmnote-endpoint">API 地址</label>
              <input
                id="xmnote-endpoint"
                v-model.trim="endpointDraft"
                class="text-input"
                type="text"
                inputmode="url"
                autocomplete="off"
                placeholder="例如 http://192.168.1.8:8080/send"
                :disabled="sending"
                @keydown.enter="saveEndpoint()"
              >
              <div class="endpoint-actions">
                <button class="action-btn" type="button" :disabled="savingEndpoint" @click="saveEndpoint()">
                  {{ savingEndpoint ? '保存中...' : '保存地址' }}
                </button>
                <span class="save-hint">{{ endpointSaved ? '已保存' : '留空时无法发送' }}</span>
              </div>
            </section>

            <section class="settings-panel">
              <div class="panel-heading">
                <div>
                  <h3>同步范围</h3>
                  <p>按书籍逐本发送，单本失败不会中断其余书籍。</p>
                </div>
              </div>
              <div class="option-grid">
                <label class="check-option">
                  <input v-model="includeProgress" type="checkbox" :disabled="sending">
                  <span>阅读进度</span>
                </label>
                <label class="check-option">
                  <input v-model="includeEntries" type="checkbox" :disabled="sending">
                  <span>书签摘录</span>
                </label>
              </div>
              <p v-if="!includeProgress && !includeEntries" class="field-warning">
                至少选择一项内容。
              </p>
            </section>

            <section class="settings-panel books-panel">
              <div class="panel-heading">
                <div>
                  <h3>选择书籍</h3>
                  <p>{{ selectedBooks.length }} / {{ books.length }} 本已选</p>
                </div>
                <div class="selection-actions">
                  <button class="mini-btn" type="button" @click="selectAll">全选</button>
                  <button class="mini-btn" type="button" @click="clearSelection">清空</button>
                </div>
              </div>
              <div class="book-list">
                <label v-for="book in books" :key="book.bookUrl" class="book-option">
                  <input v-model="selectedBookUrls" type="checkbox" :value="book.bookUrl" :disabled="sending">
                  <span class="book-option-main">
                    <strong>{{ book.name }}</strong>
                    <small>{{ book.author || '未知作者' }} · {{ bookmarkCount(book) }} 条书签</small>
                  </span>
                </label>
                <div v-if="!books.length && !loading" class="empty-state compact-empty">
                  书架中暂无可同步书籍
                </div>
                <div v-if="loading" class="empty-state compact-empty">正在读取书架...</div>
              </div>
            </section>

            <section
              v-if="result"
              class="result-panel"
              :class="{ failed: result.failed > 0 }"
              role="status"
              aria-live="polite"
            >
              <strong>{{ resultSummary }}</strong>
              <ul v-if="result.errors.length">
                <li v-for="error in result.errors" :key="error">{{ error }}</li>
              </ul>
            </section>
          </div>

          <footer class="modal-footer">
            <span class="sync-summary">{{ syncSummary }}</span>
            <button
              class="action-btn primary"
              type="button"
              :disabled="sending || !canSend"
              @click="sendToXmnote"
            >
              <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                <path d="M12 19V5M5 12l7-7 7 7" />
              </svg>
              {{ sending ? `发送中 ${sentProgress}/${sendTotal}` : '发送到纸间书摘' }}
            </button>
          </footer>
        </section>
      </div>
    </Transition>
  </Teleport>
</template>

<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { getBookmarks } from '../api/bookmark'
import { getChapterList } from '../api/bookshelf'
import { sendXmnoteBooks, type XmnoteSendResult } from '../api/xmnote'
import { useAppStore } from '../stores/app'
import { useBookshelfStore } from '../stores/bookshelf'
import type { Book, Bookmark } from '../types'
import {
  buildXmnoteBookPayload,
  groupXmnoteBookDrafts,
  normalizeXmnoteEndpoint,
} from '../utils/xmnote'

const ENDPOINT_KEY = 'xmnote-endpoint'

const props = defineProps<{
  modelValue: boolean
  bookmarks?: Bookmark[]
}>()

// 父组件传入的是当前搜索/筛选后的书签副本。子组件只读使用，不能把这个
// 数组当成持久数据源；这样关闭搜索后重新打开纸间窗口也不会读到旧筛选结果。
const sourceBookmarks = computed(() => props.bookmarks ?? [])

const emit = defineEmits<{
  'update:modelValue': [value: boolean]
}>()

const appStore = useAppStore()
const shelfStore = useBookshelfStore()
const endpointDraft = ref(readStoredEndpoint())
const endpointSaved = ref(Boolean(endpointDraft.value))
const savingEndpoint = ref(false)
const includeProgress = ref(true)
const includeEntries = ref(true)
const books = ref<Book[]>([])
const bookmarks = ref<Bookmark[]>([])
const chapterTitles = ref<Record<string, string[]>>({})
const selectedBookUrls = ref<string[]>([])
const loading = ref(false)
const sending = ref(false)
const sentProgress = ref(0)
const sendTotal = ref(0)
const result = ref<XmnoteSendResult | null>(null)

const selectedBooks = computed(() => books.value.filter((book) => (
  selectedBookUrls.value.includes(book.bookUrl)
)))
const canSend = computed(() => (
  Boolean(endpointDraft.value.trim())
  && selectedBooks.value.length > 0
  && (includeProgress.value || includeEntries.value)
))
const syncSummary = computed(() => {
  if (!selectedBooks.value.length) return '尚未选择书籍'
  const entryCount = selectedBooks.value.reduce((total, book) => (
    total + (bookmarkCount(book) || 0)
  ), 0)
  const parts: string[] = []
  if (includeProgress.value) parts.push(`${selectedBooks.value.length} 本书的进度`)
  if (includeEntries.value) parts.push(`${entryCount} 条书签`)
  return parts.join('，') || '未选择同步内容'
})
const resultSummary = computed(() => {
  if (!result.value) return ''
  const { sent, failed } = result.value
  if (!failed) return `已成功发送 ${sent} 本书`
  return `发送完成：成功 ${sent} 本，失败 ${failed} 本`
})

watch(() => props.modelValue, (visible) => {
  if (visible) void loadData()
})

function close() {
  emit('update:modelValue', false)
}

function readStoredEndpoint() {
  try {
    return localStorage.getItem(ENDPOINT_KEY) || ''
  } catch {
    return ''
  }
}

function saveEndpoint(options: { notify?: boolean } = {}) {
  savingEndpoint.value = true
  try {
    endpointDraft.value = normalizeXmnoteEndpoint(endpointDraft.value)
    localStorage.setItem(ENDPOINT_KEY, endpointDraft.value)
    endpointSaved.value = true
    if (options.notify !== false) {
      appStore.showToast('纸间书摘地址已保存', 'success')
    }
  } catch (error) {
    endpointSaved.value = false
    if (options.notify !== false) {
      appStore.showToast((error as Error).message || '地址格式不正确', 'error')
    }
    throw error
  } finally {
    savingEndpoint.value = false
  }
}

async function loadData() {
  if (loading.value) return
  loading.value = true
  result.value = null
  try {
    const [shelfBooks, allBookmarks] = await Promise.all([
      shelfStore.loaded ? Promise.resolve(shelfStore.books) : shelfStore.fetchBooks({ prefetch: false }).then(() => shelfStore.books),
      props.bookmarks ? Promise.resolve(sourceBookmarks.value) : getBookmarks(),
    ])
    books.value = shelfBooks
    bookmarks.value = allBookmarks
    if (!selectedBookUrls.value.length) {
      selectedBookUrls.value = shelfBooks.map((book) => book.bookUrl)
    }
  } catch (error) {
    appStore.showToast((error as Error).message || '加载同步数据失败', 'error')
  } finally {
    loading.value = false
  }
}

function bookmarkCount(book: Book) {
  return bookmarks.value.filter((bookmark) => (
    bookmark.bookName === book.name && bookmark.bookAuthor === book.author
  )).length
}

function selectAll() {
  selectedBookUrls.value = books.value.map((book) => book.bookUrl)
}

function clearSelection() {
  selectedBookUrls.value = []
}

async function sendToXmnote() {
  if (sending.value || !canSend.value) return
  let endpoint: string
  try {
    endpoint = normalizeXmnoteEndpoint(endpointDraft.value)
  } catch (error) {
    appStore.showToast((error as Error).message || '地址格式不正确', 'error')
    return
  }

  const includeProgressSnapshot = includeProgress.value
  const includeEntriesSnapshot = includeEntries.value
  const drafts = groupXmnoteBookDrafts(selectedBooks.value, bookmarks.value)
    .filter((draft) => (
      includeProgressSnapshot
      || draft.bookmarks.some((bookmark) => bookmark.bookText?.trim() || bookmark.content?.trim())
    ))
  if (!drafts.length) {
    appStore.showToast('所选书籍没有可发送的书签摘录', 'warning')
    return
  }
  sending.value = true
  sentProgress.value = 0
  sendTotal.value = drafts.length
  result.value = null
  try {
    try {
      saveEndpoint({ notify: false })
    } catch (error) {
      appStore.showToast((error as Error).message || '地址格式不正确', 'error')
      return
    }
    const combined: XmnoteSendResult = { sent: 0, failed: 0, errors: [] }
    for (const draft of drafts) {
      try {
        let chapterNames = chapterTitles.value[draft.book.bookUrl] || []
        if (includeEntriesSnapshot && draft.bookmarks.length && !chapterNames.length) {
          const chapters = await getChapterList({
            bookUrl: draft.book.bookUrl,
            bookSourceUrl: draft.book.origin,
            tocUrl: draft.book.tocUrl || undefined,
          }).catch(() => [])
          chapterNames = chapters.map((chapter) => chapter.title).filter(Boolean)
          chapterTitles.value[draft.book.bookUrl] = chapterNames
        }
        const payload = buildXmnoteBookPayload(draft.book, draft.bookmarks, chapterNames, {
          includeProgress: includeProgressSnapshot,
          includeEntries: includeEntriesSnapshot,
        })
        const response = await sendXmnoteBooks(endpoint, [payload])
        combined.sent += response.sent
        combined.failed += response.failed
        combined.errors.push(...response.errors)
      } catch (error) {
        combined.failed += 1
        combined.errors.push(`${draft.book.name}：${(error as Error).message || '发送失败'}`)
      }
      sentProgress.value += 1
    }
    result.value = combined
    appStore.showToast(
      combined.failed ? '部分书籍发送失败，请查看结果' : '已发送到纸间书摘',
      combined.failed ? 'warning' : 'success',
    )
  } catch (error) {
    appStore.showToast((error as Error).message || '发送失败', 'error')
  } finally {
    sending.value = false
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

.xmnote-modal {
  display: flex;
  flex-direction: column;
  width: min(720px, 100%);
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

.xmnote-body {
  flex: 1;
  min-height: 0;
  overflow-y: auto;
  padding: var(--space-4) var(--space-6);
}

.settings-panel {
  padding: var(--space-4);
  border: 1px solid var(--color-border-light);
  border-radius: var(--radius-lg);
  background: var(--color-bg-sunken);
}

.settings-panel + .settings-panel {
  margin-top: var(--space-3);
}

.panel-heading {
  display: flex;
  align-items: flex-start;
  justify-content: space-between;
  gap: var(--space-3);
}

.panel-heading h3 {
  font-size: var(--text-md);
  font-weight: 700;
}

.panel-heading p {
  margin-top: 4px;
  color: var(--color-text-secondary);
  font-size: var(--text-xs);
  line-height: 1.5;
}

.field-label {
  display: block;
  margin-top: var(--space-3);
  color: var(--color-text-secondary);
  font-size: var(--text-xs);
  font-weight: 600;
}

.text-input {
  width: 100%;
  min-height: 38px;
  margin-top: var(--space-2);
  padding: 0 var(--space-3);
  border: 1px solid var(--color-border-light);
  border-radius: var(--radius-md);
  outline: none;
  background: var(--color-bg-elevated);
  color: var(--color-text);
  font: inherit;
  font-size: var(--text-sm);
}

.text-input:focus {
  border-color: var(--color-primary);
  box-shadow: 0 0 0 2px var(--color-primary-soft-bg);
}

.endpoint-actions {
  display: flex;
  align-items: center;
  gap: var(--space-3);
  margin-top: var(--space-3);
}

.save-hint {
  color: var(--color-text-tertiary);
  font-size: var(--text-xs);
}

.option-grid {
  display: flex;
  gap: var(--space-5);
  margin-top: var(--space-3);
}

.check-option {
  display: inline-flex;
  align-items: center;
  gap: var(--space-2);
  color: var(--color-text);
  font-size: var(--text-sm);
}

.check-option input,
.book-option input {
  width: 16px;
  height: 16px;
  accent-color: var(--color-primary);
}

.field-warning {
  margin-top: var(--space-2);
  color: var(--color-danger);
  font-size: var(--text-xs);
}

.selection-actions {
  display: flex;
  gap: var(--space-2);
}

.mini-btn {
  min-height: 28px;
  padding: 0 var(--space-2);
  border: 1px solid var(--color-border-light);
  border-radius: var(--radius-md);
  background: transparent;
  color: var(--color-text-secondary);
  font: inherit;
  font-size: var(--text-xs);
}

.mini-btn:hover {
  background: var(--color-bg-hover);
}

.book-list {
  max-height: 220px;
  margin-top: var(--space-3);
  overflow-y: auto;
  border: 1px solid var(--color-border-light);
  border-radius: var(--radius-md);
  background: var(--color-bg-elevated);
}

.book-option {
  display: flex;
  align-items: center;
  gap: var(--space-3);
  padding: var(--space-2) var(--space-3);
  border-bottom: 1px solid var(--color-divider);
}

.book-option:last-of-type {
  border-bottom: 0;
}

.book-option:hover {
  background: var(--color-bg-hover);
}

.book-option-main {
  display: grid;
  min-width: 0;
  gap: 2px;
}

.book-option-main strong {
  overflow: hidden;
  color: var(--color-text);
  font-size: var(--text-sm);
  text-overflow: ellipsis;
  white-space: nowrap;
}

.book-option-main small {
  color: var(--color-text-tertiary);
  font-size: var(--text-xs);
}

.result-panel {
  margin-top: var(--space-3);
  padding: var(--space-3);
  border: 1px solid var(--color-primary-soft-border);
  border-radius: var(--radius-md);
  background: var(--color-primary-soft-bg);
  color: var(--color-primary-soft-text);
  font-size: var(--text-sm);
}

.result-panel.failed {
  border-color: rgba(220, 38, 38, 0.25);
  background: rgba(220, 38, 38, 0.06);
  color: var(--color-danger);
}

.result-panel ul {
  max-height: 110px;
  margin: var(--space-2) 0 0;
  padding-left: 1.2em;
  overflow-y: auto;
  font-size: var(--text-xs);
  line-height: 1.5;
}

.modal-footer {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: var(--space-4);
  padding: var(--space-4) var(--space-6);
  border-top: 1px solid var(--color-divider);
}

.sync-summary {
  color: var(--color-text-tertiary);
  font-size: var(--text-xs);
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

.action-btn.primary {
  border-color: var(--color-primary-soft-border);
  background: var(--color-primary-soft-bg);
  color: var(--color-primary-soft-text);
}

.action-btn:hover:not(:disabled) {
  border-color: var(--color-border);
  background: var(--color-bg-hover);
}

.action-btn.primary:hover:not(:disabled) {
  border-color: var(--color-primary);
  background: var(--color-primary-soft-bg-hover);
}

.action-btn:disabled {
  opacity: 0.45;
  cursor: not-allowed;
}

.action-btn svg {
  width: 15px;
  height: 15px;
}

.empty-state {
  display: flex;
  min-height: 220px;
  align-items: center;
  justify-content: center;
  color: var(--color-text-tertiary);
  font-size: var(--text-sm);
}

.compact-empty {
  min-height: 80px;
}

@media (max-width: 700px) {
  .modal-container {
    padding: 0;
  }

  .xmnote-modal {
    width: 100%;
    max-height: 100dvh;
    height: 100dvh;
    border: 0;
    border-radius: 0;
  }

  .modal-header,
  .xmnote-body,
  .modal-footer {
    padding-inline: var(--space-4);
  }

  .option-grid {
    flex-direction: column;
    gap: var(--space-2);
  }

  .modal-footer {
    align-items: stretch;
    flex-direction: column;
  }
}
</style>
