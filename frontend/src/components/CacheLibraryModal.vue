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
              <p>查看并清理所有书籍的服务端缓存与浏览器缓存</p>
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
                <span>浏览器缓存章节 {{ totalBrowserCachedCount }} 章</span>
              </div>
            </div>

            <div v-for="item in mergedBooks" :key="item.bookUrl" class="cache-item">
              <div class="cache-main">
                <h3>{{ item.name }}</h3>
                <p>{{ item.author || '未知作者' }}</p>
                <div class="cache-stats">
                  <span>服务端 {{ item.serverCachedCount }} 章</span>
                  <span>浏览器 {{ item.browserCachedCount }} 章</span>
                </div>
              </div>
              <div class="cache-actions">
                <button @click="cacheServer(item.book)">{{ cacheActionLabel('服务器') }}</button>
                <button @click="cacheBrowser(item.book)">{{ cacheActionLabel('浏览器') }}</button>
                <button @click="clearServer(item.book)">清服务端</button>
                <button @click="clearBrowser(item.book)">清浏览器</button>
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
import { deleteBrowserBookCache, listBrowserCacheSummary } from '../utils/browserCache'
import { cacheBookToBrowser } from '../utils/bookCache'
import { cacheBookSSE } from '../api/cache'
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
const browserSummaries = ref<Array<{ bookUrl: string; cachedChapterCount: number }>>([])

const mergedBooks = computed(() => {
  const serverMap = new Map(serverBooks.value.map((book) => [book.bookUrl, book.cachedChapterCount || 0]))
  const browserMap = new Map(browserSummaries.value.map((item) => [item.bookUrl, item.cachedChapterCount]))

  return shelfStore.books
    .filter((book) => !isLocalBook(book))
    .map((book) => ({
      book,
      bookUrl: book.bookUrl,
      name: book.name,
      author: book.author,
      serverCachedCount: serverMap.get(book.bookUrl) || 0,
      browserCachedCount: browserMap.get(book.bookUrl) || 0,
    }))
})

const offlineReadyCount = computed(() => mergedBooks.value.filter((item) => item.browserCachedCount > 0).length)
const totalBrowserCachedCount = computed(() => mergedBooks.value.reduce((sum, item) => sum + item.browserCachedCount, 0))

watch(() => props.modelValue, (visible) => {
  if (visible) {
    refreshData()
  }
})

function close() {
  emit('update:modelValue', false)
}

function cacheActionLabel(target: '服务器' | '浏览器') {
  return cacheCount.value === 0 ? `缓存全本到${target}` : `缓存后续${cacheCount.value}章到${target}`
}

async function awaitSafeBrowserSummary() {
  return listBrowserCacheSummary().catch(() => [])
}

async function refreshData() {
  loading.value = true
  try {
    const [server, browser] = await Promise.all([
      getBookshelfWithCacheInfo().catch(() => []),
      awaitSafeBrowserSummary(),
    ])
    serverBooks.value = server
    browserSummaries.value = browser
  } finally {
    loading.value = false
  }
}

function cacheServer(book: Book) {
  const sse = cacheBookSSE({ bookUrl: book.bookUrl, count: cacheCount.value, concurrentCount: 8 })
  sse.addEventListener('end', async () => {
    sse.close()
    appStore.showToast(`"${book.name}" 已缓存到服务器`, 'success')
    await refreshData()
  })
  sse.onerror = () => {
    sse.close()
    appStore.showToast(`"${book.name}" 服务端缓存失败`, 'error')
  }
}

async function cacheBrowser(book: Book) {
  try {
    await cacheBookToBrowser({ book, startIndex: 0, count: cacheCount.value || undefined })
    appStore.showToast(`"${book.name}" 已缓存到浏览器`, 'success')
    await refreshData()
  } catch (error) {
    appStore.showToast((error as Error).message || '浏览器缓存失败', 'error')
  }
}

async function clearServer(book: Book) {
  await deleteBookCache(book.bookUrl)
  appStore.showToast(`"${book.name}" 服务端缓存已清除`, 'success')
  await refreshData()
}

async function clearBrowser(book: Book) {
  await deleteBrowserBookCache(book.bookUrl)
  appStore.showToast(`"${book.name}" 浏览器缓存已清除`, 'success')
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
  border-radius: 24px;
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
  border-radius: 12px;
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
  border-radius: 18px;
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
  border-radius: 18px;
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
