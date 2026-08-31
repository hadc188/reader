<template>
  <Teleport to="body">
    <Transition name="fade">
      <div v-if="modelValue" class="modal-overlay" @click="close"></div>
    </Transition>
    <Transition name="scale">
      <div v-if="modelValue && book" :key="detailKey" class="modal-container" @click.self="close">
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
                <span v-if="displayOriginName" class="tag origin">{{ displayOriginName }}</span>
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
          <div v-if="!isLocal" class="source-section">
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
                @click="selectSource(cand)"
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
            <button v-else class="action-btn" :disabled="removingFromShelf" @click="removeFromShelf">
              <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="18" height="18">
                <path d="M5 12h14" />
              </svg>
              {{ removingFromShelf ? '正在取消...' : '取消加入' }}
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
import { isLocalBook } from '../utils/localBook'
import { matchesSourceSwitchAuthor, searchMergeKey } from '../utils/searchRank'

const SOURCE_CANDIDATES_CACHE_LIMIT = 20
const sourceCandidatesCache = new Map<string, SearchBook[]>()

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
const sourceCatalog = ref(new Map<string, string>())
const removingFromShelf = ref(false)
let detailLoadId = 0
let chapterLoadId = 0
let sourceSSE: SseLike | null = null

const isLocal = computed(() => isLocalBook(props.book))

const detailKey = computed(() => {
  const book = props.book
  if (!book) return ''
  return `${book.name}::${book.author}::${book.bookUrl}::${book.origin}`
})

const coverSrc = computed(() => {
  if (coverFailed.value || !props.book) return ''
  const url = (props.book as Book).customCoverUrl || props.book.coverUrl
  return url ? getCoverUrl(url) : ''
})

const displayChapters = computed(() => {
  if (showAllChapters.value) return chapters.value
  return chapters.value.slice(0, 50)
})

const displayOriginName = computed(() => {
  if (!props.book) return ''
  return sourceCatalog.value.get(sourceKey(props.book.origin))
    || (props.book as Book).originName
    || props.book.origin
})

function sourceNameByOrigin(origin: string): string {
  return sourceCatalog.value.get(sourceKey(origin))
    || sourceStore.sources.find((source) => sourceKey(source.bookSourceUrl) === sourceKey(origin))?.bookSourceName
    || origin
}

function isCurrentDetail(loadId: number, bookKey: string): boolean {
  return loadId === detailLoadId
    && props.modelValue
    && detailKey.value === bookKey
}

function sourceKey(origin: string): string {
  const normalized = origin.trim()
  if (!normalized) return ''
  return normalized.replace(/\/+$/, '')
}

function candidateKey(candidate: SearchBook): string {
  return `${sourceKey(candidate.origin)}::${candidate.bookUrl}`
}

function sourceCatalogSignature(): string {
  return sourceStore.sources
    .filter((source) => source.enabled !== false)
    .map((source) => sourceKey(source.bookSourceUrl))
    .sort()
    .join('\n')
}

function sourceCandidatesCacheKey(bookKey: string): string {
  return `${bookKey}::${sourceCatalogSignature()}`
}

function storeSourceCandidatesCache(key: string, candidates: SearchBook[]) {
  sourceCandidatesCache.delete(key)
  sourceCandidatesCache.set(key, candidates)
  while (sourceCandidatesCache.size > SOURCE_CANDIDATES_CACHE_LIMIT) {
    const oldestKey = sourceCandidatesCache.keys().next().value
    if (oldestKey === undefined) break
    sourceCandidatesCache.delete(oldestKey)
  }
}

function canonicalSourceOrigin(origin: string): string {
  const found = sourceStore.sources.find((source) => (
    sourceKey(source.bookSourceUrl) === sourceKey(origin)
  ))
  return found?.bookSourceUrl || origin
}

function resetDetailState() {
  closeSourceSSE()
  coverFailed.value = false
  showAllChapters.value = false
  chapters.value = []
  sourceCandidates.value = []
  selectedSource.value = null
  sourceCatalog.value = new Map()
  sourcesLoading.value = false
}

function closeSourceSSE() {
  sourceSSE?.close()
  sourceSSE = null
}

function loadSourceCandidates(loadId: number, bookKey: string) {
  const b = props.book
  if (!b || !b.name) {
    return
  }
  closeSourceSSE()
  sourcesLoading.value = true
  const current = toSearchBook(b as Book)
  const storedCandidates = (b as Book).sourceCandidates || []
  const initialCandidates = [current, ...storedCandidates]
  const cachedCandidates = sourceCandidatesCache.get(sourceCandidatesCacheKey(bookKey))
  if (cachedCandidates) {
    if (!isCurrentDetail(loadId, bookKey)) return
    setSourceCandidates([...initialCandidates, ...cachedCandidates])
    sourcesLoading.value = false
    return
  }
  // 先显示当前书源和已经保存的候选，其他书源搜索完成后逐个追加。
  setSourceCandidates(initialCandidates)

  const stream = getAvailableBookSourceSSE({
    url: b.bookUrl,
    name: b.name,
    author: b.author,
    origin: b.origin,
    lastIndex: -1,
    resultLimit: 100,
    concurrentCount: 12,
  })
  sourceSSE = stream
  let completed = false

  const finish = (cacheResult: boolean) => {
    if (completed) return
    completed = true
    if (sourceSSE === stream) sourceSSE = null
    if (!isCurrentDetail(loadId, bookKey)) return
    sourcesLoading.value = false
    if (cacheResult) {
      storeSourceCandidatesCache(
        sourceCandidatesCacheKey(bookKey),
        sourceCandidates.value.slice(),
      )
    }
  }

  stream.onmessage = (event) => {
    if (sourceSSE !== stream || !isCurrentDetail(loadId, bookKey)) {
      stream.close()
      return
    }
    const data = event.data as { data?: SearchBook[] }
    if (Array.isArray(data.data) && data.data.length > 0) {
      setSourceCandidates([...sourceCandidates.value, ...data.data])
    }
  }

  stream.addEventListener('end', (event) => {
    const payload = event.data as { hasMore?: boolean }
    finish(payload.hasMore !== true)
  })
  stream.onerror = () => finish(false)
}

function toSearchBook(book: Book): SearchBook {
  return {
    name: book.name,
    author: book.author,
    bookUrl: book.bookUrl,
    origin: book.origin,
    coverUrl: book.coverUrl,
    intro: book.intro,
    kind: book.kind,
    lastChapter: book.latestChapterTitle,
  }
}

function setSourceCandidates(candidates: SearchBook[]) {
  const previousSelection = selectedSource.value ? candidateKey(selectedSource.value) : ''
  const currentOrigin = sourceKey(props.book?.origin || '')
  const currentAuthor = props.book?.author
  const seenSources = new Set<string>()
  sourceCandidates.value = candidates.map((candidate) => ({
    ...candidate,
    origin: canonicalSourceOrigin(candidate.origin),
  })).filter((candidate) => {
    if (!candidate.origin || !candidate.bookUrl) return false
    if (!matchesSourceSwitchAuthor(currentAuthor, candidate.author)) return false
    const originKey = sourceKey(candidate.origin)
    if (!sourceCatalog.value.has(originKey) || seenSources.has(originKey)) {
      return false
    }
    seenSources.add(originKey)
    return true
  })
  selectedSource.value = sourceCandidates.value.find((candidate) => (
    candidateKey(candidate) === previousSelection
  )) || sourceCandidates.value.find((candidate) => (
    sourceKey(candidate.origin) === currentOrigin
  )) || sourceCandidates.value[0] || null
}

function selectSource(candidate: SearchBook) {
  if (!props.modelValue || !props.book) return
  if (
    sourceKey(selectedSource.value?.origin || '') === sourceKey(candidate.origin)
    && selectedSource.value?.bookUrl === candidate.bookUrl
  ) return
  selectedSource.value = candidate
  void loadChaptersFor(candidate.bookUrl, candidate.origin, detailLoadId, detailKey.value)
}

async function loadChaptersFor(
  bookUrl: string,
  origin: string,
  loadId = detailLoadId,
  bookKey = detailKey.value,
) {
  const requestId = ++chapterLoadId
  chaptersLoading.value = true
  try {
    const nextChapters = await getChapterList({ bookUrl, bookSourceUrl: origin })
    if (!isCurrentDetail(loadId, bookKey) || requestId !== chapterLoadId) return
    chapters.value = nextChapters
  } catch {
    if (!isCurrentDetail(loadId, bookKey) || requestId !== chapterLoadId) return
    chapters.value = []
  } finally {
    if (requestId === chapterLoadId) chaptersLoading.value = false
  }
}

watch([() => props.modelValue, detailKey], async ([visible, bookKey]) => {
  const loadId = ++detailLoadId
  chapterLoadId += 1

  if (!visible || !props.book) {
    resetDetailState()
    return
  }

  resetDetailState()
  const b = props.book as Book
  if (isLocal.value) {
    await loadChaptersFor(b.bookUrl, b.origin, loadId, bookKey)
    return
  }

  sourcesLoading.value = true
  // The source manager force-refreshes this shared store after source changes.
  // A detail view only needs to initialize it when no source list exists yet.
  if (sourceStore.sources.length === 0) {
    await sourceStore.fetchSources().catch(() => undefined)
  }
  if (!isCurrentDetail(loadId, bookKey)) return

  sourceCatalog.value = new Map(
    sourceStore.sources
      .filter((source) => source.enabled !== false)
      .map((source) => [sourceKey(source.bookSourceUrl), source.bookSourceName]),
  )
  if (!sourceCatalog.value.has(sourceKey(b.origin))) {
    // 保留当前书籍自身的源，避免书源列表加载失败时详情页完全没有可选项。
    sourceCatalog.value.set(sourceKey(b.origin), (b as Book).originName || b.origin)
  }
  const sourceTask = loadSourceCandidates(loadId, bookKey)
  await Promise.all([
    sourceTask,
    loadChaptersFor(b.bookUrl, b.origin, loadId, bookKey),
  ])
}, { immediate: true })

watch(() => sourceStore.availabilityVersion, async () => {
  if (!props.modelValue || !props.book || isLocal.value) return
  sourceCandidatesCache.clear()
  const loadId = ++detailLoadId
  chapterLoadId += 1
  const bookKey = detailKey.value
  resetDetailState()
  sourcesLoading.value = true
  if (!isCurrentDetail(loadId, bookKey)) return
  sourceCatalog.value = new Map(
    sourceStore.sources
      .filter((source) => source.enabled !== false)
      .map((source) => [sourceKey(source.bookSourceUrl), source.bookSourceName]),
  )
  const b = props.book as Book
  if (!sourceCatalog.value.has(sourceKey(b.origin))) {
    sourceCatalog.value.set(sourceKey(b.origin), b.originName || b.origin)
  }
  await Promise.all([
    loadSourceCandidates(loadId, bookKey),
    loadChaptersFor(b.bookUrl, b.origin, loadId, bookKey),
  ])
}, { flush: 'post' })

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
      sourceCandidates: sourceCandidates.value.slice(),
    }
  }
  return base
}

/** True when the shown book already lives on the shelf (so a source switch should persist). */
function isShelfBook(): boolean {
  return Boolean(findShelfBook())
}

function findShelfBook(): Book | undefined {
  const b = props.book
  if (!b) return undefined
  const identity = searchMergeKey(b)
  return shelfStore.books.find((item) => (
    item.bookUrl === b.bookUrl || searchMergeKey(item) === identity
  ))
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
    await saveBook(b)
    await shelfStore.fetchBooks()
    appStore.showToast('成功加入书架', 'success')
  } catch (e: unknown) {
    appStore.showToast((e as Error).message || '加入书架失败', 'error')
  }
}

async function removeFromShelf() {
  const shelfBook = findShelfBook()
  if (!shelfBook || removingFromShelf.value) return
  removingFromShelf.value = true
  try {
    await shelfStore.removeBook(shelfBook)
    appStore.showToast('已取消加入书架', 'success')
  } catch (e: unknown) {
    appStore.showToast((e as Error).message || '取消加入失败', 'error')
  } finally {
    removingFromShelf.value = false
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
