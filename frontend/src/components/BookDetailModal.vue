<template>
  <Teleport to="body">
    <Transition name="fade">
      <div v-if="modelValue" class="modal-overlay" @click="close"></div>
    </Transition>
    <Transition name="scale">
      <div v-if="modelValue && book" class="modal-container" @click.self="close">
        <div class="detail-modal">
          <button class="modal-close" @click="close">
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
              <path d="M18 6 6 18M6 6l12 12" />
            </svg>
          </button>

          <!-- Book Header -->
          <div class="book-header">
            <div class="book-cover-lg">
              <img
                v-if="coverSrc"
                :src="coverSrc"
                :alt="book.name"
                @error="coverFailed = true"
              />
              <div v-else class="cover-placeholder-lg">
                <span>{{ book.name }}</span>
              </div>
            </div>
            <div class="book-header-info">
              <h2>{{ book.name }}</h2>
              <p class="author">{{ book.author || '未知作者' }}</p>
              <div class="book-tags">
                <span v-if="book.kind" class="tag">{{ book.kind }}</span>
                <span v-if="(book as Book).totalChapterNum" class="tag">共{{ (book as Book).totalChapterNum }}章</span>
                <span v-if="(book as Book).originName" class="tag origin">{{ (book as Book).originName }}</span>
              </div>
              <p v-if="(book as Book).durChapterTitle" class="progress">
                已读至：{{ (book as Book).durChapterTitle }}
              </p>
            </div>
          </div>

          <!-- Intro -->
          <div v-if="book.intro" class="book-intro">
            <h3>简介</h3>
            <p>{{ book.intro }}</p>
          </div>

          <!-- Available Sources -->
          <div class="source-section">
            <h3>可读书源</h3>
            <div v-if="sourcesLoading && !sourceCandidates.length" class="source-loading">
              <div class="loading-spinner"></div>
              正在查找其他书源...
            </div>
            <div v-else-if="!sourceCandidates.length" class="source-empty">
              未找到其他书源
            </div>
            <div v-else class="source-list">
              <div
                v-for="cand in sourceCandidates"
                :key="`${cand.origin}::${cand.bookUrl}`"
                class="source-item"
                :class="{ selected: selectedSource?.origin === cand.origin && selectedSource?.bookUrl === cand.bookUrl }"
                @click="selectedSource = cand"
              >
                <span class="source-radio" :class="{ checked: selectedSource?.origin === cand.origin && selectedSource?.bookUrl === cand.bookUrl }" />
                <span class="source-name">{{ sourceNameByOrigin(cand.origin) }}</span>
                <span v-if="cand.lastChapter" class="source-latest">{{ cand.lastChapter }}</span>
              </div>
            </div>
          </div>

          <!-- Chapters -->
          <div class="chapter-section" v-if="chapters.length > 0">
            <h3>目录 ({{ chapters.length }})</h3>
            <div class="chapter-list">
              <div
                v-for="(chapter, i) in displayChapters"
                :key="chapter.url"
                class="chapter-item"
                :class="{ current: i === (book as Book).durChapterIndex }"
                @click="readChapter(i)"
              >
                <span class="chapter-index">{{ i + 1 }}</span>
                <span class="chapter-title">{{ chapter.title }}</span>
              </div>
            </div>
            <button
              v-if="chapters.length > 50 && !showAllChapters"
              class="show-more-btn"
              @click="showAllChapters = true"
            >
              显示全部 {{ chapters.length }} 章
            </button>
          </div>
          <div v-else-if="chaptersLoading" class="chapter-loading">
            <div class="loading-spinner"></div>
            加载目录中...
          </div>

          <!-- Actions -->
          <div class="modal-actions">
            <button class="action-btn primary" @click="startReading">
              <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="18" height="18">
                <path d="M2 3h6a4 4 0 0 1 4 4v14a3 3 0 0 0-3-3H2z" />
                <path d="M22 3h-6a4 4 0 0 0-4 4v14a3 3 0 0 1 3-3h7z" />
              </svg>
              {{ (book as Book).durChapterIndex ? '继续阅读' : '开始阅读' }}
            </button>
            <button v-if="!isShelfBook()" class="action-btn" @click="addToShelf">
              <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="18" height="18">
                <path d="M12 5v14M5 12h14" />
              </svg>
              加入书架
            </button>
            <button class="action-btn" @click="close">关闭</button>
          </div>
        </div>
      </div>
    </Transition>
  </Teleport>
</template>

<script setup lang="ts">
import { ref, watch, computed } from 'vue'
import { useRouter } from 'vue-router'
import { getCoverUrl, getChapterList, saveBook, setBookSource } from '../api/bookshelf'
import { getAvailableBookSourceSSE } from '../api/search'
import type { SseLike } from '../api/sse'
import { useSourceStore } from '../stores/source'
import { useBookshelfStore } from '../stores/bookshelf'
import { useReaderStore } from '../stores/reader'
import { useAppStore } from '../stores/app'
import type { Book, SearchBook, BookChapter } from '../types'

const props = defineProps<{
  modelValue: boolean
  book: Book | SearchBook | null
}>()

const emit = defineEmits<{
  'update:modelValue': [value: boolean]
}>()

const router = useRouter()
const readerStore = useReaderStore()
const shelfStore = useBookshelfStore()
const sourceStore = useSourceStore()
const appStore = useAppStore()

const coverFailed = ref(false)
const chapters = ref<BookChapter[]>([])
const chaptersLoading = ref(false)
const showAllChapters = ref(false)
const sourceCandidates = ref<SearchBook[]>([])
const sourcesLoading = ref(false)
const selectedSource = ref<SearchBook | null>(null)
let sourceSSE: SseLike | null = null

const coverSrc = computed(() => {
  if (coverFailed.value || !props.book) return ''
  const url = (props.book as Book).customCoverUrl || props.book.coverUrl
  return url ? getCoverUrl(url) : ''
})

const displayChapters = computed(() => {
  if (showAllChapters.value) return chapters.value
  return chapters.value.slice(0, 50)
})

function sourceNameByOrigin(origin: string): string {
  const found = sourceStore.sources.find((s) => s.bookSourceUrl === origin)
  return found?.bookSourceName || origin
}

function closeSourceSSE() {
  if (sourceSSE) {
    sourceSSE.close()
    sourceSSE = null
  }
}

function loadSourceCandidates() {
  closeSourceSSE()
  const b = props.book
  if (!b || !b.name) {
    sourceCandidates.value = []
    selectedSource.value = null
    return
  }
  sourcesLoading.value = true
  sourceCandidates.value = []
  selectedSource.value = null
  // Always include the current source as the first candidate, so the user can
  // see it and switch back to it. The backend may omit it (e.g. when serving a
  // stale cache saved before the current source was included), so we seed it
  // here and let the SSE stream dedup against it.
  const base = b as Book
  if (base.origin) {
    sourceCandidates.value.push({
      name: base.name,
      author: base.author,
      bookUrl: base.bookUrl,
      origin: base.origin,
      coverUrl: base.coverUrl,
      intro: base.intro,
      kind: base.kind,
      lastChapter: base.latestChapterTitle,
    } as SearchBook)
    selectedSource.value = sourceCandidates.value[0]
  }
  const stream = getAvailableBookSourceSSE({
    url: b.bookUrl,
    name: b.name,
    author: b.author,
    origin: b.origin,
    lastIndex: -1,
    concurrentCount: 12,
  })
  sourceSSE = stream
  stream.onmessage = (event) => {
    const data = event.data as { data?: SearchBook[] }
    if (data.data && Array.isArray(data.data)) {
      const seen = new Set(sourceCandidates.value.map((c) => `${c.origin}::${c.bookUrl}`))
      for (const cand of data.data) {
        const key = `${cand.origin}::${cand.bookUrl}`
        if (!seen.has(key)) {
          seen.add(key)
          sourceCandidates.value.push(cand)
        }
      }
      if (!selectedSource.value && sourceCandidates.value.length > 0) {
        selectedSource.value = sourceCandidates.value[0]
      }
    }
  }
  stream.addEventListener('end', () => {
    sourcesLoading.value = false
    closeSourceSSE()
  })
  stream.onerror = () => {
    sourcesLoading.value = false
    closeSourceSSE()
  }
}

async function loadChaptersFor(bookUrl: string, origin: string) {
  chaptersLoading.value = true
  try {
    chapters.value = await getChapterList({ bookUrl, bookSourceUrl: origin })
  } catch {
    chapters.value = []
  } finally {
    chaptersLoading.value = false
  }
}

watch(() => props.modelValue, async (visible) => {
  if (visible && props.book) {
    coverFailed.value = false
    showAllChapters.value = false
    chapters.value = []
    if (sourceStore.sources.length === 0) {
      sourceStore.fetchSources().catch(() => undefined)
    }
    loadSourceCandidates()
    // Load the current source's chapters; switching source below reloads them.
    const b = props.book as Book
    await loadChaptersFor(b.bookUrl, b.origin)
  } else {
    closeSourceSSE()
  }
})

// Reload the chapter list when the user switches source in the detail modal.
watch(selectedSource, async (sel) => {
  if (!sel) return
  await loadChaptersFor(sel.bookUrl, sel.origin)
})

function close() {
  emit('update:modelValue', false)
}

/** The book to open for reading, preferring the user-selected source. */
function activeBook(): Book {
  const base = props.book as Book
  const sel = selectedSource.value
  if (sel) {
    return {
      ...base,
      bookUrl: sel.bookUrl,
      origin: sel.origin,
      originName: sourceNameByOrigin(sel.origin),
      coverUrl: sel.coverUrl || base.coverUrl,
      intro: sel.intro || base.intro,
      kind: sel.kind || base.kind,
      latestChapterTitle: sel.lastChapter || base.latestChapterTitle,
      durChapterIndex: 0,
      durChapterTitle: undefined,
      // A different source has its own toc URL; clear the old one so the reader
      // fetches the new source's chapter list instead of failing on a stale one.
      tocUrl: undefined,
    }
  }
  return base
}

/** True when the shown book already lives on the shelf (so a source switch should persist). */
function isShelfBook(): boolean {
  const b = props.book
  if (!b) return false
  return shelfStore.books.some((item) => item.bookUrl === b.bookUrl)
}

/** If the user picked a different source for a shelved book, persist the switch. */
async function persistSourceSwitchIfNeeded() {
  const sel = selectedSource.value
  if (!sel || !isShelfBook()) return
  const base = props.book as Book
  if (sel.origin === base.origin && sel.bookUrl === base.bookUrl) return
  try {
    const updated = await setBookSource({
      bookUrl: base.bookUrl,
      newUrl: sel.bookUrl,
      bookSourceUrl: sel.origin,
      name: base.name,
      author: base.author,
      coverUrl: sel.coverUrl || base.coverUrl,
      intro: sel.intro || base.intro,
      kind: sel.kind || base.kind,
      latestChapterTitle: sel.lastChapter || base.latestChapterTitle,
      durChapterIndex: base.durChapterIndex,
      durChapterTitle: base.durChapterTitle,
      durChapterPos: base.durChapterPos,
      durChapterTime: base.durChapterTime,
    })
    if (updated) {
      await shelfStore.fetchBooks().catch(() => undefined)
    }
  } catch (e: unknown) {
    console.warn('切换书源失败', e)
  }
}

async function startReading() {
  await persistSourceSwitchIfNeeded()
  const b = activeBook()
  await shelfStore.moveBookToFront(b.bookUrl).catch(() => undefined)
  await readerStore.loadBook(b)
  await readerStore.loadChapter(b.durChapterIndex || 0)
  close()
  router.push('/reader')
}

async function readChapter(index: number) {
  await persistSourceSwitchIfNeeded()
  const b = activeBook()
  await shelfStore.moveBookToFront(b.bookUrl).catch(() => undefined)
  await readerStore.loadBook(b)
  await readerStore.loadChapter(index)
  close()
  router.push('/reader')
}

async function addToShelf() {
  const b = activeBook()
  try {
    await saveBook({
      name: b.name,
      author: b.author,
      bookUrl: b.bookUrl,
      origin: b.origin,
      coverUrl: b.coverUrl,
    })
    await shelfStore.fetchBooks()
    appStore.showToast('成功加入书架', 'success')
  } catch (e: unknown) {
    appStore.showToast((e as Error).message || '加入书架失败', 'error')
  }
}
</script>

<style scoped>
.modal-overlay {
  position: fixed;
  inset: 0;
  background: rgba(0, 0, 0, 0.5);
  z-index: var(--z-overlay);
  backdrop-filter: blur(4px);
}

.modal-container {
  position: fixed;
  inset: 0;
  z-index: var(--z-modal);
  display: flex;
  align-items: center;
  justify-content: center;
  padding:
    calc(var(--space-6) + var(--safe-area-top))
    calc(var(--space-6) + var(--safe-area-right))
    calc(var(--space-6) + var(--safe-area-bottom))
    calc(var(--space-6) + var(--safe-area-left));
  overflow-y: auto;
  -webkit-overflow-scrolling: touch;
}

.detail-modal {
  width: 100%;
  max-width: 600px;
  max-height: min(85vh, calc(var(--app-height, 100dvh) - var(--safe-area-top) - var(--safe-area-bottom) - 32px));
  overflow-y: auto;
  -webkit-overflow-scrolling: touch;
  background: var(--color-bg-elevated);
  border-radius: var(--radius-xl);
  padding: var(--space-8);
  position: relative;
  box-shadow: var(--shadow-xl);
}

.modal-close {
  position: absolute;
  top: max(var(--space-4), calc(var(--safe-area-top) * 0.35));
  right: var(--space-4);
  width: 32px;
  height: 32px;
  display: flex;
  align-items: center;
  justify-content: center;
  border-radius: var(--radius-md);
  color: var(--color-text-tertiary);
  transition: all var(--duration-fast);
  z-index: 1;
}

.modal-close:hover {
  background: var(--color-bg-hover);
  color: var(--color-text);
}

.modal-close svg {
  width: 18px;
  height: 18px;
}

.book-header {
  display: flex;
  gap: var(--space-5);
  margin-bottom: var(--space-6);
}

.book-cover-lg {
  width: 120px;
  height: 160px;
  flex-shrink: 0;
  border-radius: var(--radius-md);
  overflow: hidden;
  background: var(--color-bg-sunken);
  box-shadow: var(--shadow-md);
}

.book-cover-lg img {
  width: 100%;
  height: 100%;
  object-fit: cover;
}

.cover-placeholder-lg {
  width: 100%;
  height: 100%;
  display: flex;
  align-items: center;
  justify-content: center;
  background: linear-gradient(135deg, var(--color-primary-bg), var(--color-bg-sunken));
  padding: var(--space-3);
  text-align: center;
  font-size: var(--text-sm);
  font-weight: 600;
  color: var(--color-primary);
}

.book-header-info {
  flex: 1;
  min-width: 0;
}

.book-header-info h2 {
  font-size: var(--text-xl);
  font-weight: 700;
  margin-bottom: var(--space-2);
  line-height: var(--leading-tight);
}

.author {
  color: var(--color-text-secondary);
  font-size: var(--text-sm);
  margin-bottom: var(--space-3);
}

.book-tags {
  display: flex;
  flex-wrap: wrap;
  gap: var(--space-2);
  margin-bottom: var(--space-3);
}

.tag {
  padding: 2px var(--space-2);
  background: var(--color-bg-sunken);
  border-radius: var(--radius-sm);
  font-size: var(--text-xs);
  color: var(--color-text-secondary);
}

.tag.origin {
  background: var(--color-primary-bg);
  color: var(--color-primary);
}

.progress {
  font-size: var(--text-sm);
  color: var(--color-primary);
}

.book-intro {
  margin-bottom: var(--space-6);
}

.book-intro h3 {
  font-size: var(--text-base);
  font-weight: 600;
  margin-bottom: var(--space-2);
}

.book-intro p {
  font-size: var(--text-sm);
  color: var(--color-text-secondary);
  line-height: var(--leading-relaxed);
  white-space: pre-wrap;
}

.source-section {
  margin-bottom: var(--space-6);
}

.source-section h3 {
  font-size: var(--text-base);
  font-weight: 600;
  margin-bottom: var(--space-3);
}

.source-loading,
.source-empty {
  display: flex;
  align-items: center;
  justify-content: center;
  gap: var(--space-2);
  padding: var(--space-4);
  color: var(--color-text-tertiary);
  font-size: var(--text-sm);
}

.source-list {
  border: 1px solid var(--color-border-light);
  border-radius: var(--radius-md);
  overflow: hidden;
}

.source-item {
  display: flex;
  align-items: center;
  gap: var(--space-3);
  padding: var(--space-2) var(--space-3);
  cursor: pointer;
  transition: background var(--duration-fast);
  font-size: var(--text-sm);
  border-bottom: 1px solid var(--color-divider);
}

.source-item:last-child {
  border-bottom: none;
}

.source-item:hover {
  background: var(--color-bg-hover);
}

.source-item.selected {
  background: var(--color-primary-bg);
}

.source-radio {
  width: 16px;
  height: 16px;
  flex-shrink: 0;
  border-radius: 50%;
  border: 2px solid var(--color-border);
  display: inline-flex;
  align-items: center;
  justify-content: center;
  transition: all var(--duration-fast);
}

.source-radio.checked {
  border-color: var(--color-primary);
}

.source-radio.checked::after {
  content: '';
  width: 8px;
  height: 8px;
  border-radius: 50%;
  background: var(--color-primary);
}

.source-name {
  flex: 1;
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  font-weight: 500;
}

.source-item.selected .source-name {
  color: var(--color-primary);
}

.source-latest {
  flex-shrink: 0;
  max-width: 40%;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  font-size: var(--text-xs);
  color: var(--color-text-tertiary);
}

.chapter-section h3 {
  font-size: var(--text-base);
  font-weight: 600;
  margin-bottom: var(--space-3);
}

.chapter-list {
  max-height: 300px;
  overflow-y: auto;
  -webkit-overflow-scrolling: touch;
  border: 1px solid var(--color-border-light);
  border-radius: var(--radius-md);
}

@media (max-width: 768px) {
  .detail-modal {
    padding: var(--space-6);
    border-radius: 20px;
  }
}

.chapter-item {
  display: flex;
  align-items: center;
  gap: var(--space-3);
  padding: var(--space-2) var(--space-3);
  cursor: pointer;
  transition: background var(--duration-fast);
  font-size: var(--text-sm);
  border-bottom: 1px solid var(--color-divider);
}

.chapter-item:last-child {
  border-bottom: none;
}

.chapter-item:hover {
  background: var(--color-bg-hover);
}

.chapter-item.current {
  color: var(--color-primary);
  background: var(--color-primary-bg);
}

.chapter-index {
  color: var(--color-text-tertiary);
  font-size: var(--text-xs);
  min-width: 28px;
}

.chapter-title {
  flex: 1;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.show-more-btn {
  width: 100%;
  padding: var(--space-3);
  text-align: center;
  color: var(--color-primary);
  font-size: var(--text-sm);
  font-weight: 500;
  margin-top: var(--space-2);
  border-radius: var(--radius-md);
  transition: background var(--duration-fast);
}

.show-more-btn:hover {
  background: var(--color-primary-bg);
}

.chapter-loading {
  display: flex;
  align-items: center;
  justify-content: center;
  gap: var(--space-2);
  padding: var(--space-6);
  color: var(--color-text-tertiary);
  font-size: var(--text-sm);
}

.loading-spinner {
  width: 18px;
  height: 18px;
  border: 2px solid var(--color-border);
  border-top-color: var(--color-primary);
  border-radius: 50%;
  animation: spin 0.8s linear infinite;
}

@keyframes spin {
  to { transform: rotate(360deg); }
}

.modal-actions {
  display: flex;
  gap: var(--space-3);
  margin-top: var(--space-6);
  padding-top: var(--space-5);
  border-top: 1px solid var(--color-divider);
}

.action-btn {
  flex: 1;
  display: flex;
  align-items: center;
  justify-content: center;
  gap: var(--space-2);
  padding: var(--space-3);
  border-radius: var(--radius-md);
  font-size: var(--text-sm);
  font-weight: 600;
  border: 1px solid var(--color-border);
  background: var(--color-bg);
  transition: all var(--duration-fast);
}

.action-btn:hover {
  background: var(--color-bg-hover);
}

.action-btn.primary {
  background: var(--color-primary);
  color: white;
  border-color: var(--color-primary);
}

.action-btn.primary:hover {
  background: var(--color-primary-dark);
}
</style>
