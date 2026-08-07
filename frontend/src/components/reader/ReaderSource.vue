<template>
  <div class="reader-source" :style="{ background: theme.popup, color: theme.fontColor }">
    <div class="source-header">
      <div class="header-left">
        <h3>切换书源</h3>
        <span class="source-count" v-if="preparedResults.length">{{ preparedResults.length }} 个结果</span>
      </div>
      <button class="close-btn" @click="store.closePanel()">
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M18 6 6 18M6 6l12 12" /></svg>
      </button>
    </div>
    
    <div class="source-list" ref="listRef">
      <!-- Current Source Header -->
      <div class="section-label" v-if="currentSource">当前书源</div>
      <div v-if="currentSource" class="source-item current">
        <div class="source-name">{{ currentSource.originName || currentSource.origin }}</div>
        <div class="source-url">{{ currentSource.origin }}</div>
      </div>

      <div v-if="store.book" class="book-brief">
        <div class="book-brief-cover">
          <img v-if="store.book.coverUrl" :src="store.book.coverUrl" :alt="store.book.name">
          <div v-else class="book-brief-placeholder">{{ store.book.name.slice(0, 1) }}</div>
        </div>
        <div class="book-brief-main">
          <div class="book-brief-title">{{ store.book.name }}</div>
          <div class="book-brief-meta">{{ store.book.author || '未知作者' }}</div>
          <div class="book-brief-meta" v-if="store.currentChapter?.title">当前章节：{{ store.currentChapter.title }}</div>
          <div class="book-brief-meta" v-if="store.book.latestChapterTitle">最新章节：{{ store.book.latestChapterTitle }}</div>
          <div class="book-brief-intro" v-if="store.book.intro">{{ store.book.intro }}</div>
        </div>
      </div>

      <div class="section-label">其他可用源</div>
      
      <div v-if="searching && !preparedResults.length" class="loading">
        <div class="spinner"></div>
        正在全网搜索同名书籍...
      </div>
      
      <div v-else-if="!preparedResults.length" class="empty">未找到其他书源</div>
      
      <div
        v-else
        v-for="item in preparedResults"
        :key="item.book.bookUrl + item.book.origin"
        class="source-item"
        :class="{ selected: selectedCandidate?.book.bookUrl === item.book.bookUrl && selectedCandidate?.book.origin === item.book.origin }"
        @click="selectCandidate(item)"
      >
        <div class="source-main">
          <div class="source-name-row">
            <span class="source-name">{{ sourceNameByOrigin(item.book.origin) }}</span>
            <span class="source-tag" v-if="item.book.kind">{{ item.book.kind }}</span>
            <span v-if="item.sameLatest" class="compare-badge good">最新章节一致</span>
            <span v-else-if="item.book.lastChapter" class="compare-badge">章节不同</span>
          </div>
          <div class="source-book-name" v-if="item.book.name">{{ item.book.name }}</div>
          <div class="source-author">{{ item.book.author }}</div>
          <div class="source-intro" v-if="item.book.intro">{{ item.book.intro }}</div>
          <div class="source-chapter" v-if="item.book.lastChapter">最新: {{ item.book.lastChapter }}</div>
          <div class="source-update" v-if="item.book.updateTime">更新时间: {{ item.book.updateTime }}</div>
          <div class="source-compare-line">
            <span v-if="item.sameName" class="compare-text">书名匹配</span>
            <span v-if="item.sameAuthor" class="compare-text">作者匹配</span>
            <span v-if="item.chapterHint" class="compare-text strong">{{ item.chapterHint }}</span>
          </div>
        </div>
        <div class="source-action">
          <button class="switch-btn" @click.stop="handleSwitch(item.book)">切换</button>
        </div>
      </div>

      <div v-if="searching && preparedResults.length" class="inline-loading">
        <div class="spinner small"></div>
        继续搜索其他书源...
      </div>

      <div v-if="selectedCandidate" class="compare-panel">
        <div class="compare-header">
          <h4>书源对照</h4>
          <button class="switch-btn primary" :disabled="store.loading" @click="handleSwitch(selectedCandidate.book)">切换到此书源</button>
        </div>
        <div class="compare-grid">
          <div class="compare-card">
            <div class="compare-title">当前</div>
            <div class="compare-name">{{ store.book?.name }}</div>
            <div class="compare-meta">{{ store.book?.author || '未知作者' }}</div>
            <div class="compare-line">书源：{{ store.book?.originName || store.book?.origin }}</div>
            <div class="compare-line">当前章节：{{ store.currentChapter?.title || '未知' }}</div>
            <div class="compare-line">最新章节：{{ store.book?.latestChapterTitle || '未知' }}</div>
          </div>
          <div class="compare-card highlight">
            <div class="compare-title">目标</div>
            <div class="compare-name">{{ candidatePreview?.name || selectedCandidate.book.name }}</div>
            <div class="compare-meta">{{ candidatePreview?.author || selectedCandidate.book.author || '未知作者' }}</div>
            <div class="compare-line">书源：{{ candidatePreview?.originName || selectedCandidate.book.origin }}</div>
            <div class="compare-line">最新章节：{{ candidatePreview?.latestChapterTitle || selectedCandidate.book.lastChapter || '未知' }}</div>
            <div class="compare-line compare-intro" v-if="candidatePreview?.intro || selectedCandidate.book.intro">{{ candidatePreview?.intro || selectedCandidate.book.intro }}</div>
          </div>
        </div>
      </div>

      <div v-if="hasMoreSources && !searching" class="load-more-wrap">
        <button class="load-more-btn" :disabled="loadingMore" @click="loadMoreSources">
          {{ loadingMore ? '加载中...' : '加载更多' }}
        </button>
      </div>
    </div>

    <!-- Switching Overlay -->
    <Transition name="fade">
      <div v-if="store.loading" class="switch-overlay" :style="{ background: theme.popup }">
        <div class="spinner"></div>
        <p>正在切换书源，请稍候...</p>
      </div>
    </Transition>
  </div>
</template>

<script setup lang="ts">
import { ref, computed, onMounted, onUnmounted } from 'vue'
import { useReaderStore } from '../../stores/reader'
import { useAppStore } from '../../stores/app'
import { useSourceStore } from '../../stores/source'
import { getAvailableBookSourceSSE } from '../../api/search'
import type { SseLike } from '../../api/sse'
import { getBookInfo } from '../../api/bookshelf'
import type { Book, SearchBook } from '../../types'

type CandidateItem = {
  book: SearchBook
  sameName: boolean
  sameAuthor: boolean
  sameLatest: boolean
  chapterHint: string
  score: number
}

const store = useReaderStore()
const appStore = useAppStore()
const sourceStore = useSourceStore()
const theme = computed(() => store.currentTheme)
const searching = ref(false)
const loadingMore = ref(false)
const results = ref<SearchBook[]>([])
const lastIndex = ref(-1)
const hasMoreSources = ref(true)
const selectedCandidate = ref<CandidateItem | null>(null)
const candidatePreview = ref<Book | null>(null)
const AVAILABLE_CONCURRENT_COUNT = 16
let availableSourceSSE: SseLike | null = null

const currentSource = computed(() => {
  if (!store.book) return null
  return {
    origin: store.book.origin,
    originName: store.book.originName
  }
})

function sourceNameByOrigin(origin: string): string {
  const found = sourceStore.sources.find((s) => s.bookSourceUrl === origin)
  return found?.bookSourceName || origin
}

function normalizeText(value?: string) {
  return (value || '')
    .replace(/\s+/g, '')
    .replace(/[：:,.，。！？!?\-—_()（）【】\[\]<>《》'"“”‘’]/g, '')
    .toLowerCase()
}

function normalizeAuthorText(value?: string) {
  return normalizeText(value).replace(/^作者/, '')
}

const preparedResults = computed<CandidateItem[]>(() => {
  if (!store.book) return []
  const currentName = normalizeText(store.book.name)
  const currentAuthor = normalizeAuthorText(store.book.author)
  const currentLatest = normalizeText(store.book.latestChapterTitle || store.currentChapter?.title)

  return results.value
    .map((book) => {
      const sameName = normalizeText(book.name) === currentName
      const sameAuthor = normalizeAuthorText(book.author) === currentAuthor
      const sameLatest = !!currentLatest && normalizeText(book.lastChapter) === currentLatest
      const chapterHint = sameLatest
        ? '可无缝续读'
        : (book.lastChapter ? `目标源最新：${book.lastChapter}` : '')
      const score = (sameName ? 3 : 0) + (sameAuthor ? 3 : 0) + (sameLatest ? 4 : 0)
      return { book, sameName, sameAuthor, sameLatest, chapterHint, score }
    })
    .sort((a, b) => b.score - a.score)
})

onMounted(() => {
  if (sourceStore.sources.length === 0) {
    sourceStore.fetchSources().catch(() => undefined)
  }
  startSearch()
})

onUnmounted(() => {
  closeAvailableSourceSSE()
})

function startSearch() {
  if (!store.book) return
  closeAvailableSourceSSE()
  searching.value = true
  loadingMore.value = false
  results.value = []
  lastIndex.value = -1
  hasMoreSources.value = true
  selectedCandidate.value = null
  candidatePreview.value = null

  openAvailableSourceSSE('initial')
}

function mergeCandidates(candidates: SearchBook[]) {
  if (!store.book || !candidates.length) return
  const currentBook = store.book
  const currentAuthor = normalizeAuthorText(currentBook.author)
  candidates.forEach((item) => {
    if (item.origin === currentBook.origin) return
    if (currentAuthor && item.author && normalizeAuthorText(item.author) !== currentAuthor) return
    const existed = results.value.some((candidate) =>
      candidate.origin === item.origin || (candidate.bookUrl === item.bookUrl && candidate.origin === item.origin),
    )
    if (!existed) {
      results.value.push(item)
    }
  })
}

type AvailableSourceMode = 'initial' | 'loadMore'
type AvailableSourceSSEPayload = {
  data?: SearchBook[]
  books?: SearchBook[]
  lastIndex?: number
  hasMore?: boolean
}

function closeAvailableSourceSSE() {
  if (!availableSourceSSE) return
  availableSourceSSE.close()
  availableSourceSSE = null
}

function parseAvailableSourcePayload(event: { data: unknown }): AvailableSourceSSEPayload | null {
  try {
    return event.data as AvailableSourceSSEPayload
  } catch (error) {
    console.error('parse getAvailableBookSourceSSE payload failed', error)
    return null
  }
}

function applyAvailableSourcePayload(payload: AvailableSourceSSEPayload | null) {
  if (!payload) return
  const incoming = Array.isArray(payload.data)
    ? payload.data
    : (Array.isArray(payload.books) ? payload.books : [])

  if (typeof payload.lastIndex === 'number') {
    lastIndex.value = payload.lastIndex
  }
  if (typeof payload.hasMore === 'boolean') {
    hasMoreSources.value = payload.hasMore
  }
  mergeCandidates(incoming)

  if (!selectedCandidate.value && preparedResults.value.length) {
    void selectCandidate(preparedResults.value[0])
  }
}

function openAvailableSourceSSE(mode: AvailableSourceMode) {
  if (!store.book) return

  const beforeCount = results.value.length
  const stream = getAvailableBookSourceSSE({
    url: store.book.bookUrl,
    name: store.book.name,
    author: store.book.author,
    origin: store.book.origin,
    lastIndex: mode === 'loadMore' ? lastIndex.value : -1,
    concurrentCount: AVAILABLE_CONCURRENT_COUNT,
  })
  availableSourceSSE = stream

  stream.onmessage = (event) => {
    if (availableSourceSSE !== stream) return
    applyAvailableSourcePayload(parseAvailableSourcePayload(event))
  }

  stream.addEventListener('end', (event) => {
    if (availableSourceSSE !== stream) return
    applyAvailableSourcePayload(parseAvailableSourcePayload(event))
    finishAvailableSourceSSE(stream, mode, beforeCount)
  })

  stream.onerror = (event) => {
    if (availableSourceSSE !== stream) return
    console.error('getAvailableBookSourceSSE failed', event)
    finishAvailableSourceSSE(stream, mode, beforeCount, true)
  }
}

function finishAvailableSourceSSE(
  stream: SseLike,
  mode: AvailableSourceMode,
  beforeCount: number,
  failed = false,
) {
  if (availableSourceSSE !== stream) return
  stream.close()
  availableSourceSSE = null

  if (mode === 'initial') {
    searching.value = false
  } else {
    loadingMore.value = false
  }

  if (!selectedCandidate.value && preparedResults.value.length) {
    void selectCandidate(preparedResults.value[0])
  }

  if (failed) {
    if (mode === 'loadMore') {
      appStore.showToast('加载更多书源失败', 'error')
    }
    return
  }

  if (mode === 'loadMore') {
    const addedCount = results.value.length - beforeCount
    if (addedCount > 0) {
      appStore.showToast(`已新增 ${addedCount} 个书源`, 'success')
    } else if (!hasMoreSources.value) {
      appStore.showToast('没有更多书源了', 'warning')
    } else {
      appStore.showToast('本批次未找到更多匹配书源', 'warning')
    }
  }
}

async function selectCandidate(item: CandidateItem) {
  selectedCandidate.value = item
  candidatePreview.value = null
  try {
    candidatePreview.value = await getBookInfo(item.book.bookUrl, item.book.origin)
  } catch {
    candidatePreview.value = null
  }
}

function loadMoreSources() {
  if (!store.book || loadingMore.value || !hasMoreSources.value) return

  closeAvailableSourceSSE()
  loadingMore.value = true
  openAvailableSourceSSE('loadMore')
}

async function handleSwitch(res: SearchBook) {
  if (store.loading) return
  try {
    const nextBook = await store.switchSource(res.bookUrl, res.origin)
    store.closePanel()
    appStore.showToast(`已切换到 ${nextBook?.originName || nextBook?.origin || res.origin}`, 'success')
  } catch (e: any) {
    appStore.showToast(`切换失败: ${e?.message || '未知错误'}`, 'error')
  }
}
</script>

<style scoped>
.reader-source {
  display: flex;
  flex-direction: column;
  height: 100%;
  position: relative;
}

.source-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 16px 20px;
  border-bottom: 1px solid rgba(0,0,0,0.06);
  flex-shrink: 0;
}

.header-left { display: flex; align-items: baseline; gap: 8px; }
.source-header h3 { font-size: 16px; margin: 0; }
.source-count { font-size: 11px; opacity: 0.5; }

.close-btn {
  width: 32px; height: 32px;
  display: flex; align-items: center; justify-content: center;
  border-radius: 8px; color: inherit; opacity: 0.6;
  background: transparent; border: none; cursor: pointer;
}

.source-list {
  flex: 1;
  overflow-y: auto;
  padding: 8px 0;
}

.book-brief {
  display: flex;
  gap: 12px;
  margin: 8px 16px 14px;
  padding: 12px;
  border-radius: 14px;
  background: rgba(201, 127, 58, 0.08);
  border: 1px solid rgba(201, 127, 58, 0.14);
}

.book-brief-cover {
  width: 56px;
  height: 76px;
  flex-shrink: 0;
  border-radius: 10px;
  overflow: hidden;
  background: rgba(0, 0, 0, 0.06);
}

.book-brief-cover img,
.book-brief-placeholder {
  width: 100%;
  height: 100%;
}

.book-brief-cover img {
  object-fit: cover;
}

.book-brief-placeholder {
  display: flex;
  align-items: center;
  justify-content: center;
  font-size: 20px;
  font-weight: 700;
}

.book-brief-main {
  min-width: 0;
  display: flex;
  flex-direction: column;
  gap: 4px;
}

.book-brief-title {
  font-size: 15px;
  font-weight: 700;
}

.book-brief-meta {
  font-size: 12px;
  opacity: 0.68;
  line-height: 1.4;
}

.book-brief-intro {
  font-size: 12px;
  line-height: 1.45;
  opacity: 0.76;
  display: -webkit-box;
  -webkit-line-clamp: 3;
  -webkit-box-orient: vertical;
  overflow: hidden;
}

.section-label {
  padding: 12px 20px 4px;
  font-size: 11px;
  text-transform: uppercase;
  letter-spacing: 0.05em;
  opacity: 0.4;
  font-weight: 600;
}

.source-item {
  display: flex;
  justify-content: space-between;
  align-items: center;
  padding: 12px 20px;
  border-bottom: 1px solid rgba(0,0,0,0.02);
  cursor: pointer;
  transition: background 0.2s;
}

.source-item:hover { background: rgba(0,0,0,0.03); }
.source-item.current { background: rgba(201, 127, 58, 0.04); cursor: default; }
.source-item.selected {
  background: rgba(201, 127, 58, 0.08);
  box-shadow: inset 3px 0 0 var(--color-primary, #c97f3a);
}

.source-main { flex: 1; min-width: 0; }
.source-name-row { display: flex; align-items: center; gap: 8px; margin-bottom: 2px; }
.source-name { font-weight: 600; font-size: 14px; }
.source-tag { font-size: 10px; opacity: 0.5; border: 1px solid currentColor; padding: 0 3px; border-radius: 3px; }
.compare-badge {
  font-size: 10px;
  padding: 1px 6px;
  border-radius: 999px;
  background: rgba(0, 0, 0, 0.06);
  opacity: 0.72;
}

.compare-badge.good {
  background: rgba(82, 196, 26, 0.14);
  color: #3f8f16;
  opacity: 1;
}

.source-book-name {
  font-size: 13px;
  margin-bottom: 2px;
  opacity: 0.86;
}

.source-author { font-size: 11px; opacity: 0.5; margin-bottom: 4px; }
.source-chapter { font-size: 11px; opacity: 0.7; color: var(--color-primary, #c97f3a); }
.source-update { font-size: 11px; opacity: 0.48; margin-top: 2px; }
.source-compare-line {
  display: flex;
  flex-wrap: wrap;
  gap: 6px;
  margin-top: 6px;
}

.compare-text {
  font-size: 11px;
  opacity: 0.62;
}

.compare-text.strong {
  color: var(--color-primary, #c97f3a);
  opacity: 0.92;
}

.source-intro {
  font-size: 12px;
  opacity: 0.68;
  line-height: 1.45;
  margin-bottom: 4px;
  display: -webkit-box;
  -webkit-line-clamp: 2;
  -webkit-box-orient: vertical;
  overflow: hidden;
}

.source-url { font-size: 11px; opacity: 0.3; }

.switch-btn {
  padding: 4px 12px;
  font-size: 11px;
  border-radius: 12px;
  border: 1px solid var(--color-border);
  background: transparent;
  color: inherit;
  cursor: pointer;
  opacity: 0.7;
}

.source-item:hover .switch-btn {
  background: var(--color-primary, #c97f3a);
  color: white;
  border-color: var(--color-primary, #c97f3a);
  opacity: 1;
}

.compare-panel {
  margin: 8px 16px 18px;
  padding: 14px;
  border-radius: 16px;
  background: rgba(201, 127, 58, 0.08);
  border: 1px solid rgba(201, 127, 58, 0.14);
}

.compare-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
  margin-bottom: 12px;
}

.compare-header h4 {
  margin: 0;
  font-size: 14px;
  font-weight: 700;
}

.switch-btn.primary {
  background: var(--color-primary, #c97f3a);
  border-color: var(--color-primary, #c97f3a);
  color: #fff;
  opacity: 1;
}

.switch-btn:disabled {
  cursor: not-allowed;
  opacity: 0.45;
}

.compare-grid {
  display: grid;
  grid-template-columns: repeat(2, minmax(0, 1fr));
  gap: 12px;
}

.compare-card {
  padding: 12px;
  border-radius: 14px;
  background: rgba(255, 255, 255, 0.6);
  border: 1px solid rgba(0, 0, 0, 0.06);
}

.compare-card.highlight {
  border-color: rgba(201, 127, 58, 0.28);
  background: rgba(201, 127, 58, 0.12);
}

.compare-title {
  margin-bottom: 8px;
  font-size: 11px;
  font-weight: 700;
  letter-spacing: 0.05em;
  text-transform: uppercase;
  opacity: 0.58;
}

.compare-name {
  font-size: 14px;
  font-weight: 700;
  line-height: 1.4;
  margin-bottom: 4px;
}

.compare-meta {
  font-size: 12px;
  opacity: 0.68;
  margin-bottom: 10px;
}

.compare-line {
  font-size: 12px;
  line-height: 1.5;
  opacity: 0.78;
  margin-top: 4px;
}

.compare-intro {
  display: -webkit-box;
  -webkit-line-clamp: 4;
  -webkit-box-orient: vertical;
  overflow: hidden;
}

.loading, .empty {
  padding: 40px 20px;
  text-align: center;
  opacity: 0.5;
  font-size: 14px;
}

.inline-loading {
  display: flex;
  align-items: center;
  justify-content: center;
  gap: 8px;
  padding: 12px 20px;
  font-size: 12px;
  opacity: 0.55;
}

.load-more-wrap {
  padding: 16px 20px 24px;
  display: flex;
  justify-content: center;
}

.load-more-btn {
  min-width: 120px;
  padding: 10px 18px;
  border-radius: 999px;
  border: 1px solid rgba(0,0,0,0.08);
  background: transparent;
  color: inherit;
  cursor: pointer;
}

.load-more-btn:hover:not(:disabled) {
  background: rgba(201, 127, 58, 0.08);
  border-color: var(--color-primary, #c97f3a);
  color: var(--color-primary, #c97f3a);
}

.load-more-btn:disabled {
  opacity: 0.5;
  cursor: wait;
}

.spinner {
  width: 24px;
  height: 24px;
  border: 2px solid rgba(0,0,0,0.1);
  border-top-color: var(--color-primary, #c97f3a);
  border-radius: 50%;
  animation: spin 1s linear infinite;
  margin: 0 auto 12px;
}

.inline-loading .spinner,
.spinner.small {
  width: 14px;
  height: 14px;
  margin: 0;
  border-width: 2px;
}

@keyframes spin { to { transform: rotate(360deg); } }

.switch-overlay {
  position: absolute;
  top: 0; left: 0; right: 0; bottom: 0;
  display: flex; flex-direction: column;
  align-items: center; justify-content: center;
  z-index: 20;
}

.switch-overlay p { margin-top: 16px; font-size: 14px; opacity: 0.8; }

@media (max-width: 640px) {
  .source-item {
    padding: 12px 16px;
    align-items: flex-start;
  }

  .source-action {
    padding-top: 2px;
  }

  .compare-panel {
    margin: 8px 12px 16px;
    padding: 12px;
  }

  .compare-header {
    flex-direction: column;
    align-items: stretch;
  }

  .compare-grid {
    grid-template-columns: 1fr;
  }
}
</style>
