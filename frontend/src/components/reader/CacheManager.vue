<template>
  <div class="cache-manager" :style="{ background: theme.popup, color: theme.fontColor }">
    <div class="cache-header">
      <div>
        <h3>缓存章节</h3>
        <p v-if="store.book" class="cache-subtitle">{{ store.book.name }}</p>
      </div>
      <button class="close-btn" @click="store.backPanel()">
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
          <path d="m15 18-6-6 6-6" />
        </svg>
      </button>
    </div>

    <div class="cache-body">
      <div v-if="!isLocalBookInServer" class="summary-grid">
        <div class="summary-card">
          <span class="summary-label">本地缓存</span>
          <strong>{{ serverCachedCount }}</strong>
          <small>已缓存章节</small>
        </div>
        <div class="summary-card">
          <span class="summary-label">浏览器缓存</span>
          <strong>{{ browserCachedCount }}</strong>
          <small>离线可读章节</small>
        </div>
      </div>

      <div v-if="working" class="caching-status">
        <div class="progress-circle">
          <svg viewBox="0 0 36 36">
            <path class="circle-bg" d="M18 2.0845 a 15.9155 15.9155 0 0 1 0 31.831 a 15.9155 15.9155 0 0 1 0 -31.831" />
            <path class="circle" :stroke-dasharray="`${progress}, 100`" d="M18 2.0845 a 15.9155 15.9155 0 0 1 0 31.831 a 15.9155 15.9155 0 0 1 0 -31.831" />
          </svg>
          <div class="percentage">{{ progress }}%</div>
        </div>
        <div class="status-text">
          <p class="main-status">{{ currentStatus }}</p>
          <p class="sub-status">{{ currentChapterName || '正在准备...' }}</p>
        </div>
        <button class="stop-btn" @click="stopWorking">停止</button>
      </div>

      <div v-else class="cache-sections">
        <div v-if="isLocalBookInServer" class="info-card">
          <p>本地书已存放在应用书架文件中，不需要额外缓存；阅读时会直接读取上传后的本地文件。</p>
        </div>

        <template v-else>
          <div class="info-card">
            <p>本地缓存保存在应用存储目录；浏览器缓存保存在当前设备的 IndexedDB。断网时阅读页会优先读取浏览器已缓存章节。</p>
          </div>

          <section class="cache-section">
            <div class="section-head">
              <h4>缓存到本地</h4>
              <button class="link-btn" @click="refreshStats">刷新</button>
            </div>
            <div class="option-list">
              <button class="cache-opt" @click="startServerCaching(50)">
                <span class="label">缓存后续 50 章</span>
                <span class="sub">适合当前追更</span>
              </button>
              <button class="cache-opt" @click="startServerCaching(100)">
                <span class="label">缓存后续 100 章</span>
                <span class="sub">中度离线阅读</span>
              </button>
              <button class="cache-opt primary" @click="startServerCaching(0)">
                <span class="label">全本缓存到本地</span>
                <span class="sub">保存到应用磁盘</span>
              </button>
              <button class="cache-opt danger" @click="clearServerCache">
                <span class="label">清除本地缓存</span>
                <span class="sub">删除当前书所有本地缓存</span>
              </button>
            </div>
          </section>

          <section class="cache-section">
            <div class="section-head">
              <h4>缓存到浏览器</h4>
              <button class="link-btn" @click="refreshStats">刷新</button>
            </div>
            <div class="option-list">
              <button class="cache-opt" @click="startBrowserCaching(50)">
                <span class="label">缓存后续 50 章</span>
                <span class="sub">只保留在当前浏览器</span>
              </button>
              <button class="cache-opt" @click="startBrowserCaching(100)">
                <span class="label">缓存后续 100 章</span>
                <span class="sub">适合本地离线使用</span>
              </button>
              <button class="cache-opt primary" @click="startBrowserCaching(0)">
                <span class="label">全本缓存到浏览器</span>
                <span class="sub">持久化到 IndexedDB</span>
              </button>
              <button class="cache-opt danger" @click="clearBrowserCache">
                <span class="label">清除浏览器缓存</span>
                <span class="sub">删除当前设备离线缓存</span>
              </button>
            </div>
          </section>
        </template>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { ref, computed, onMounted, onUnmounted } from 'vue'
import { useReaderStore } from '../../stores/reader'
import { useAppStore } from '../../stores/app'
import { cacheBookSSE } from '../../api/cache'
import type { SseLike } from '../../api/sse'
import { getBookshelfWithCacheInfo, deleteBookCache } from '../../api/bookshelf'
import { countBrowserBookCache, deleteBrowserBookCache } from '../../utils/browserCache'
import { cacheBookToBrowser, resolveBookChapters } from '../../utils/bookCache'
import { isLocalBook } from '../../utils/localBook'

const store = useReaderStore()
const appStore = useAppStore()
const theme = computed(() => store.currentTheme)

const working = ref(false)
const progress = ref(0)
const currentStatus = ref('准备中...')
const currentChapterName = ref('')
const serverCachedCount = ref(0)
const browserCachedCount = ref(0)
const isLocalBookInServer = computed(() => isLocalBook(store.book))
let sse: SseLike | null = null
let browserSignal = { cancelled: false }

onMounted(() => {
  refreshStats()
})

onUnmounted(() => {
  stopWorking()
})

async function refreshStats() {
  if (!store.book) return
  if (isLocalBookInServer.value) {
    serverCachedCount.value = 0
    browserCachedCount.value = 0
    return
  }
  const [serverList, browserCount] = await Promise.all([
    getBookshelfWithCacheInfo().catch(() => []),
    countBrowserBookCache(store.book.bookUrl).catch(() => 0),
  ])
  const matched = serverList.find((book) => book.bookUrl === store.book?.bookUrl)
  serverCachedCount.value = matched?.cachedChapterCount || 0
  browserCachedCount.value = browserCount
}

function startServerCaching(count: number) {
  if (!store.book || isLocalBookInServer.value) return
  stopWorking()
  working.value = true
  progress.value = 0
  currentStatus.value = '连接本地缓存任务...'
  currentChapterName.value = ''

  const total = count === 0
    ? Math.max(0, store.chapters.length - store.currentIndex)
    : Math.min(count, Math.max(0, store.chapters.length - store.currentIndex))

  sse = cacheBookSSE({
    bookUrl: store.book.bookUrl,
    tocUrl: store.currentChapter?.url,
    count,
    concurrentCount: 8,
  })

  sse.onmessage = (event) => {
    try {
      const data = event.data as { successCount?: number; cachedCount?: number; failedCount?: number }
      const completed = (data.successCount || 0) + (data.cachedCount || 0) - (data.failedCount || 0)
      if (total > 0) {
        progress.value = Math.min(100, Math.round((Math.max(0, completed) / total) * 100))
      }
      currentStatus.value = `本地缓存中 (${data.cachedCount || 0} 已缓存 / ${data.successCount || 0} 新增)`
    } catch {
      currentStatus.value = '本地缓存处理中...'
    }
  }

  sse.addEventListener('end', async (event) => {
    try {
      const data = event.data as { cachedCount?: number; successCount?: number }
      currentStatus.value = `本地缓存完成，累计 ${data.cachedCount || 0} 章`
      progress.value = 100
    } finally {
      closeSSE()
      await refreshStats()
      window.setTimeout(() => {
        working.value = false
      }, 800)
    }
  })

  sse.onerror = async () => {
    currentStatus.value = '本地缓存已中断'
    closeSSE()
    await refreshStats()
    window.setTimeout(() => {
      working.value = false
    }, 1200)
  }
}

async function startBrowserCaching(count: number) {
  if (!store.book || isLocalBookInServer.value) return
  stopWorking()
  browserSignal = { cancelled: false }
  working.value = true
  progress.value = 0
  currentStatus.value = '准备浏览器缓存...'
  currentChapterName.value = ''

  try {
    const chapters = store.chapters.length ? store.chapters : await resolveBookChapters(store.book)
    const total = count === 0
      ? Math.max(0, chapters.length - store.currentIndex)
      : Math.min(count, Math.max(0, chapters.length - store.currentIndex))
    await cacheBookToBrowser({
      book: store.book,
      chapters,
      startIndex: store.currentIndex,
      count: count || undefined,
      signal: browserSignal,
      onProgress: ({ completed, total, chapterTitle }) => {
        currentChapterName.value = chapterTitle
        currentStatus.value = `浏览器缓存中 (${completed}/${total})`
        progress.value = total > 0 ? Math.round((completed / total) * 100) : 100
      },
    })
    if (!browserSignal.cancelled) {
      currentStatus.value = `浏览器缓存完成，共 ${total} 章`
      progress.value = 100
      appStore.showToast('已缓存到浏览器', 'success')
    }
  } catch (error) {
    currentStatus.value = '浏览器缓存失败'
    appStore.showToast((error as Error).message || '浏览器缓存失败', 'error')
  } finally {
    await refreshStats()
    window.setTimeout(() => {
      working.value = false
    }, 800)
  }
}

async function clearServerCache() {
  if (!store.book || isLocalBookInServer.value) return
  await deleteBookCache(store.book.bookUrl)
  appStore.showToast('本地缓存已清除', 'success')
  await refreshStats()
}

async function clearBrowserCache() {
  if (!store.book || isLocalBookInServer.value) return
  await deleteBrowserBookCache(store.book.bookUrl)
  appStore.showToast('浏览器缓存已清除', 'success')
  await refreshStats()
}

function closeSSE() {
  if (sse) {
    sse.close()
    sse = null
  }
}

function stopWorking() {
  browserSignal.cancelled = true
  closeSSE()
  working.value = false
}
</script>

<style scoped>
.cache-manager {
  display: flex;
  flex-direction: column;
  height: 100%;
}

.cache-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 16px 20px;
  border-bottom: 1px solid rgba(0,0,0,0.06);
}

.cache-header h3 { margin: 0; font-size: 16px; }
.cache-subtitle { margin: 4px 0 0; font-size: 12px; opacity: 0.55; }

.close-btn {
  width: 32px; height: 32px;
  display: flex; align-items: center; justify-content: center;
  border-radius: 8px; color: inherit; opacity: 0.6;
  background: transparent; border: none; cursor: pointer;
}

.cache-body {
  flex: 1;
  padding: 24px;
  overflow-y: auto;
  -webkit-overflow-scrolling: touch;
  overscroll-behavior: contain;
}

.summary-grid {
  display: grid;
  grid-template-columns: repeat(2, minmax(0, 1fr));
  gap: 12px;
  margin-bottom: 20px;
}

.summary-card {
  border-radius: 16px;
  padding: 16px;
  background: rgba(201, 127, 58, 0.08);
  display: flex;
  flex-direction: column;
  gap: 6px;
}

.summary-label { font-size: 12px; opacity: 0.65; }
.summary-card strong { font-size: 28px; line-height: 1; }
.summary-card small { font-size: 12px; opacity: 0.5; }

.cache-sections {
  display: flex;
  flex-direction: column;
  gap: 24px;
}

.cache-section {
  display: flex;
  flex-direction: column;
  gap: 14px;
}

.section-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
}

.section-head h4 {
  margin: 0;
  font-size: 15px;
}

.link-btn {
  background: transparent;
  border: none;
  color: var(--color-primary, #c97f3a);
  cursor: pointer;
}

.info-card {
  background: rgba(201, 127, 58, 0.08);
  padding: 16px;
  border-radius: 12px;
  border-left: 4px solid var(--color-primary, #c97f3a);
}

.info-card p { margin: 0; font-size: 13px; line-height: 1.6; opacity: 0.8; }

.option-list { display: flex; flex-direction: column; gap: 12px; }

.cache-opt {
  display: flex;
  flex-direction: column;
  align-items: flex-start;
  padding: 16px 20px;
  border-radius: 16px;
  border: 1px solid rgba(0,0,0,0.1);
  background: transparent;
  color: inherit;
  cursor: pointer;
  text-align: left;
  transition: all 0.2s;
}

.cache-opt:hover {
  border-color: var(--color-primary, #c97f3a);
  background: rgba(201, 127, 58, 0.04);
}

.cache-opt.primary {
  background: var(--color-primary, #c97f3a);
  border-color: var(--color-primary, #c97f3a);
  color: white;
}

.cache-opt.danger {
  border-color: rgba(239, 68, 68, 0.2);
  color: #dc2626;
}

.cache-opt .label { font-weight: 600; font-size: 15px; margin-bottom: 4px; }
.cache-opt .sub { font-size: 11px; opacity: 0.6; }

.caching-status {
  min-height: 360px;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  padding-bottom: 40px;
}

.progress-circle {
  position: relative;
  width: 150px;
  height: 150px;
  margin-bottom: 24px;
}

.progress-circle svg { transform: rotate(-90deg); width: 100%; height: 100%; }
.circle-bg { fill: none; stroke: rgba(0,0,0,0.05); stroke-width: 2.8; }
.circle { fill: none; stroke: var(--color-primary, #c97f3a); stroke-width: 2.8; stroke-linecap: round; transition: stroke-dasharray 0.3s; }

.percentage {
  position: absolute;
  top: 50%; left: 50%;
  transform: translate(-50%, -50%);
  font-size: 24px; font-weight: 700;
}

.status-text { text-align: center; margin-bottom: 32px; }
.main-status { font-weight: 600; font-size: 16px; margin: 0 0 8px 0; }
.sub-status { font-size: 13px; opacity: 0.5; margin: 0; }

.stop-btn {
  padding: 8px 24px;
  border-radius: 20px;
  background: rgba(239, 68, 68, 0.1);
  color: #ef4444;
  border: 1px solid rgba(239, 68, 68, 0.2);
  cursor: pointer;
  font-size: 14px;
}
</style>
