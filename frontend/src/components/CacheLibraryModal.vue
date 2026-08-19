<template>
  <Teleport to="body">
    <Transition name="fade">
      <div v-if="modelValue" class="modal-overlay" @click="close"></div>
    </Transition>
    <Transition name="scale">
      <div v-if="modelValue" class="modal-container" @click.self="close">
        <div class="cache-modal">
          <div class="modal-head">
            <div>
              <h2>缓存管理</h2>
              <p>查看并清理所有书籍的本地缓存</p>
            </div>
            <div class="head-actions">
              <button class="ghost-btn" @click="refreshData">刷新</button>
              <button class="close-btn" @click="close">
                <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                  <path d="M18 6 6 18M6 6l12 12" />
                </svg>
              </button>
            </div>
          </div>

          <div v-if="loading" class="loading-state">
            <div class="loading-spinner"></div>
            <p>缓存信息加载中...</p>
          </div>

          <div v-else class="cache-list">
            <div class="cache-toolbar">
              <div class="cache-scope">
                <span class="scope-label">默认缓存范围</span>
                <div class="scope-options">
                  <button class="scope-btn" :class="{ active: cacheCount === 50 }" @click="cacheCount = 50">50章</button>
                  <button class="scope-btn" :class="{ active: cacheCount === 100 }" @click="cacheCount = 100">100章</button>
                  <button class="scope-btn" :class="{ active: cacheCount === 0 }" @click="cacheCount = 0">全本</button>
                </div>
              </div>
              <div class="cache-overview">
                <span>可离线书籍 {{ offlineReadyCount }} 本</span>
                <span>本地缓存章节 {{ totalServerCachedCount }} 章</span>
              </div>
            </div>

            <div v-for="item in mergedBooks" :key="item.bookUrl" class="cache-item">
              <div class="cache-main">
                <h3>{{ item.name }}</h3>
                <p>{{ item.author || '未知作者' }}</p>
                <div class="cache-stats">
                  <span>本地 {{ item.serverCachedCount }} 章</span>
                </div>
                <div v-if="cacheProgress[item.bookUrl]" class="cache-progress">
                  <div class="progress-text">
                    <span>正在缓存 {{ cacheProgress[item.bookUrl].cached }}/{{ cacheProgress[item.bookUrl].total || '…' }} 章</span>
                    <span v-if="cacheProgress[item.bookUrl].failed > 0" class="progress-failed">失败 {{ cacheProgress[item.bookUrl].failed }}</span>
                    <span class="progress-percent">{{ progressPercent(item.bookUrl) }}%</span>
                  </div>
                  <div class="progress-bar">
                    <div class="progress-fill" :style="{ width: `${progressPercent(item.bookUrl)}%` }"></div>
                  </div>
                </div>
              </div>
              <div class="cache-actions">
                <button v-if="cacheProgress[item.bookUrl]" class="abort-btn" @click="abortCache(item.book)">中断缓存</button>
                <button v-else @click="cacheServer(item.book)">{{ cacheActionLabel }}</button>
                <button @click="clearServer(item.book)">清本地</button>
              </div>
            </div>
          </div>
        </div>
      </div>
    </Transition>
  </Teleport>
</template>

<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { useBookshelfStore } from '../stores/bookshelf'
import { useAppStore } from '../stores/app'
import { getBookshelfWithCacheInfo, deleteBookCache } from '../api/bookshelf'
import type { Book } from '../types'
import { cacheBookSSE, cancelCacheBook } from '../api/cache'
import type { CacheBookProgressPayload } from '../api/cache'
import { isLocalBook } from '../utils/localBook'

const props = defineProps<{
  modelValue: boolean
}>()

const emit = defineEmits<{
  'update:modelValue': [value: boolean]
}>()

const shelfStore = useBookshelfStore()
const appStore = useAppStore()
const loading = ref(false)
const cacheCount = ref(50)
const serverBooks = ref<Book[]>([])

interface CacheProgress {
  cached: number
  total: number
  failed: number
}

const cacheProgress = ref<Record<string, CacheProgress>>({})

const mergedBooks = computed(() => {
  const serverMap = new Map(serverBooks.value.map((book) => [book.bookUrl, book.cachedChapterCount || 0]))

  return shelfStore.books
    .filter((book) => !isLocalBook(book))
    .map((book) => ({
      book,
      bookUrl: book.bookUrl,
      name: book.name,
      author: book.author,
      serverCachedCount: serverMap.get(book.bookUrl) || 0,
    }))
})

const offlineReadyCount = computed(() => mergedBooks.value.filter((item) => item.serverCachedCount > 0).length)
const totalServerCachedCount = computed(() => mergedBooks.value.reduce((sum, item) => sum + item.serverCachedCount, 0))
const cacheActionLabel = computed(() => cacheCount.value === 0 ? '缓存全本到本地' : `缓存后续${cacheCount.value}章到本地`)

watch(() => props.modelValue, (visible) => {
  if (visible) {
    refreshData()
  }
})

function close() {
  emit('update:modelValue', false)
}

async function refreshData() {
  loading.value = true
  try {
    serverBooks.value = await getBookshelfWithCacheInfo().catch(() => [])
  } finally {
    loading.value = false
  }
}

function progressPercent(bookUrl: string) {
  const progress = cacheProgress.value[bookUrl]
  if (!progress || progress.total <= 0) return 0
  return Math.min(100, Math.round((progress.cached / progress.total) * 100))
}

function cacheServer(book: Book) {
  if (cacheProgress.value[book.bookUrl]) return
  cacheProgress.value = { ...cacheProgress.value, [book.bookUrl]: { cached: 0, total: 0, failed: 0 } }

  const sse = cacheBookSSE({ bookUrl: book.bookUrl, count: cacheCount.value, concurrentCount: 8 })
  sse.addEventListener('message', (event) => {
    const payload = event.data as CacheBookProgressPayload
    cacheProgress.value = {
      ...cacheProgress.value,
      [book.bookUrl]: {
        cached: payload.cachedCount ?? 0,
        total: payload.totalChapters ?? 0,
        failed: payload.failedCount ?? 0,
      },
    }
  })
  sse.addEventListener('end', async (event) => {
    sse.close()
    const aborted = !!(event.data as CacheBookProgressPayload).aborted
    delete cacheProgress.value[book.bookUrl]
    cacheProgress.value = { ...cacheProgress.value }
    appStore.showToast(
      aborted ? `已中断"${book.name}"的缓存` : `"${book.name}" 已缓存到本地`,
      aborted ? 'warning' : 'success',
    )
    await refreshData()
  })
  sse.onerror = () => {
    sse.close()
    delete cacheProgress.value[book.bookUrl]
    cacheProgress.value = { ...cacheProgress.value }
    appStore.showToast(`"${book.name}" 本地缓存失败`, 'error')
  }
}

async function abortCache(book: Book) {
  const res = await cancelCacheBook(book.bookUrl).catch(() => null)
  if (!res?.cancelled) {
    // 后端已无该任务（刚结束或失败），本地状态直接清理。
    delete cacheProgress.value[book.bookUrl]
    cacheProgress.value = { ...cacheProgress.value }
  }
}

async function clearServer(book: Book) {
  await deleteBookCache(book.bookUrl)
  appStore.showToast(`"${book.name}" 本地缓存已清除`, 'success')
  await refreshData()
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
  padding: 24px;
}

.cache-modal {
  width: min(960px, 100%);
  max-height: 82vh;
  overflow: auto;
  background: var(--color-bg-elevated);
  border-radius: var(--radius-xl);
  padding: 24px;
  box-shadow: var(--shadow-xl);
}

.modal-head {
  display: flex;
  justify-content: space-between;
  gap: 16px;
  margin-bottom: 20px;
}

.modal-head h2 {
  margin: 0;
}

.modal-head p {
  margin: 6px 0 0;
  color: var(--color-text-tertiary);
}

.head-actions {
  display: flex;
  gap: 8px;
  align-items: flex-start;
}

.ghost-btn,
.close-btn,
.cache-actions button {
  border: 1px solid var(--color-border);
  background: transparent;
  border-radius: var(--radius-md);
  padding: 8px 12px;
  cursor: pointer;
}

.close-btn svg {
  width: 16px;
  height: 16px;
}

.loading-state {
  min-height: 240px;
  display: flex;
  flex-direction: column;
  justify-content: center;
  align-items: center;
  gap: 16px;
}

.loading-spinner {
  width: 32px;
  height: 32px;
  border: 3px solid var(--color-border);
  border-top-color: var(--color-primary);
  border-radius: 50%;
  animation: spin 0.8s linear infinite;
}

.cache-list {
  display: flex;
  flex-direction: column;
  gap: 12px;
}

.cache-toolbar {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 16px;
  padding: 14px 16px;
  border: 1px solid var(--color-border-light);
  border-radius: var(--radius-lg);
  background: var(--color-bg-sunken);
}

.cache-scope {
  display: flex;
  align-items: center;
  gap: 12px;
  flex-wrap: wrap;
}

.scope-label {
  font-size: 13px;
  color: var(--color-text-secondary);
}

.scope-options {
  display: flex;
  gap: 8px;
  flex-wrap: wrap;
}

.scope-btn {
  border: 1px solid var(--color-border);
  background: transparent;
  border-radius: 999px;
  padding: 6px 12px;
  cursor: pointer;
}

.scope-btn.active {
  background: var(--color-primary);
  border-color: var(--color-primary);
  color: #fff;
}

.cache-overview {
  display: flex;
  gap: 14px;
  flex-wrap: wrap;
  font-size: 13px;
  color: var(--color-text-secondary);
}

.cache-item {
  display: flex;
  justify-content: space-between;
  gap: 16px;
  border: 1px solid var(--color-border-light);
  border-radius: var(--radius-lg);
  padding: 18px;
}

.cache-main h3 {
  margin: 0;
}

.cache-main p {
  margin: 6px 0 10px;
  color: var(--color-text-tertiary);
}

.cache-stats {
  display: flex;
  gap: 12px;
  flex-wrap: wrap;
  font-size: 13px;
}

.cache-actions {
  display: flex;
  gap: 8px;
  flex-wrap: wrap;
  justify-content: flex-end;
  align-content: flex-start;
}

.abort-btn {
  color: var(--color-danger);
  border-color: var(--color-danger);
}

.cache-progress {
  margin-top: 10px;
  display: flex;
  flex-direction: column;
  gap: 6px;
}

.progress-text {
  display: flex;
  gap: 10px;
  font-size: 13px;
  color: var(--color-text-secondary);
}

.progress-failed {
  color: var(--color-danger);
}

.progress-percent {
  margin-left: auto;
  font-variant-numeric: tabular-nums;
}

.progress-bar {
  height: 6px;
  border-radius: 999px;
  background: var(--color-border-light);
  overflow: hidden;
}

.progress-fill {
  height: 100%;
  border-radius: inherit;
  background: var(--color-primary);
  transition: width 0.2s ease;
}

@keyframes spin {
  to { transform: rotate(360deg); }
}

@media (max-width: 768px) {
  .cache-toolbar {
    flex-direction: column;
    align-items: flex-start;
  }

  .cache-item {
    flex-direction: column;
  }

  .cache-actions {
    justify-content: flex-start;
  }
}
</style>
