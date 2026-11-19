<template>
  <div
    class="cache-manager"
    :style="{
      background: theme.popup,
      color: theme.fontColor,
      '--cm-font': theme.fontColor,
      '--cm-popup': theme.popup,
    }"
  >
    <header class="panel-head">
      <div class="panel-head-main">
        <h3>离线缓存</h3>
        <p v-if="store.book" class="panel-sub">{{ store.book.name }}</p>
      </div>
      <button class="icon-btn" type="button" aria-label="返回" title="返回" @click="store.backPanel()">
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="m15 18-6-6 6-6" /></svg>
      </button>
    </header>

    <div class="panel-body">
      <!-- 本地书不需要缓存：只说明情况，不摆一堆无效操作。 -->
      <div v-if="isLocalBookInServer" class="note">
        本地书已存放在应用书架中，不需要额外缓存。阅读时会直接读取上传后的本地文件。
      </div>

      <template v-else>
        <!-- ── 本机缓存 ── -->
        <section class="group">
          <div class="group-head">
            <h4>本机缓存</h4>
            <button class="link-btn" type="button" @click="refreshStats">刷新</button>
          </div>
          <div class="card">
            <div class="stat-row">
              <div class="stat">
                <span class="stat-value">{{ localCachedCount }}</span>
                <span class="stat-unit">章</span>
              </div>
              <span class="stat-caption">已缓存到本机</span>
            </div>

            <div v-if="working" class="progress-block">
              <div class="progress-line">
                <div class="progress-track">
                  <div class="progress-fill" :style="{ width: progress + '%' }"></div>
                </div>
                <span class="progress-num">{{ progress }}%</span>
              </div>
              <p class="progress-status">{{ currentStatus }}</p>
              <p class="progress-chapter">{{ currentChapterName || '正在准备…' }}</p>
              <button class="btn btn-quiet" type="button" @click="stopWorking">停止缓存</button>
            </div>
          </div>
        </section>

        <!-- ── 缓存操作 ── -->
        <section class="group">
          <div class="group-head"><h4>缓存到本地</h4></div>
          <div class="card card-rows">
            <button class="row-btn" type="button" @click="startServerCaching(50)">
              <span class="row-text">
                <span class="row-label">后续 50 章</span>
                <span class="row-sub">适合正在追更的书</span>
              </span>
            </button>
            <button class="row-btn" type="button" @click="startServerCaching(100)">
              <span class="row-text">
                <span class="row-label">后续 100 章</span>
                <span class="row-sub">中度离线阅读</span>
              </span>
            </button>
            <button class="row-btn" type="button" @click="startServerCaching(0)">
              <span class="row-text">
                <span class="row-label">全本缓存</span>
                <span class="row-sub">从当前章缓存到最后一章</span>
              </span>
            </button>
            <button
              class="row-btn row-danger"
              type="button"
              :disabled="!localCachedCount"
              :title="localCachedCount ? '删除当前书的全部本地缓存' : '这本书还没有本地缓存'"
              @click="clearServerCache"
            >
              <span class="row-text">
                <span class="row-label">清除本地缓存</span>
                <span class="row-sub">只删本机文件，不影响书架里的书</span>
              </span>
            </button>
          </div>
          <p class="hint">已缓存的章节直接从本机读取，不再联网，断网也能照常翻页。</p>
        </section>
      </template>
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
import { isLocalBook } from '../../utils/localBook'

const store = useReaderStore()
const appStore = useAppStore()
const theme = computed(() => store.chromeTheme)

const working = ref(false)
const progress = ref(0)
const currentStatus = ref('准备中...')
const currentChapterName = ref('')
const localCachedCount = ref(0)
const isLocalBookInServer = computed(() => isLocalBook(store.book))
let sse: SseLike | null = null

onMounted(() => {
  refreshStats()
})

onUnmounted(() => {
  stopWorking()
})

async function refreshStats() {
  if (!store.book) return
  if (isLocalBookInServer.value) {
    localCachedCount.value = 0
    return
  }
  const localBooks = await getBookshelfWithCacheInfo().catch(() => [])
  const matched = localBooks.find((book) => book.bookUrl === store.book?.bookUrl)
  localCachedCount.value = matched?.cachedChapterCount || 0
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
      const data = event.data as { totalChapters?: number; successCount?: number; cachedCount?: number; failedCount?: number }
      const totalLocal = data.totalChapters || total
      const completed = (data.cachedCount || 0) + (data.failedCount || 0)
      if (totalLocal > 0) {
        progress.value = Math.min(100, Math.round((Math.max(0, completed) / totalLocal) * 100))
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

async function clearServerCache() {
  if (!store.book || isLocalBookInServer.value) return
  await deleteBookCache(store.book.bookUrl)
  appStore.showToast('本地缓存已清除', 'success')
  await refreshStats()
}

function closeSSE() {
  if (sse) {
    sse.close()
    sse = null
  }
}

function stopWorking() {
  closeSSE()
  working.value = false
}
</script>

<style scoped>
/* 颜色从 --cm-font（阅读主题文字色）派生：本面板跟随 9 套阅读主题，
 * 写死 rgba(0,0,0,x) 在暗色主题下会整块消失。 */
.cache-manager {
  --cm-line: color-mix(in srgb, var(--cm-font, currentColor) 9%, transparent);
  --cm-line-strong: color-mix(in srgb, var(--cm-font, currentColor) 18%, transparent);
  --cm-label: color-mix(in srgb, var(--cm-font, currentColor) 70%, transparent);
  --cm-muted: color-mix(in srgb, var(--cm-font, currentColor) 46%, transparent);
  --cm-card: color-mix(in srgb, var(--cm-font, currentColor) 3.2%, transparent);
  --cm-card-border: color-mix(in srgb, var(--cm-font, currentColor) 7%, transparent);
  --cm-accent: var(--color-primary, #c97f3a);
  --cm-accent-fill: color-mix(in srgb, var(--cm-accent) 13%, transparent);
  --cm-danger: #c43f3a;

  display: flex;
  flex-direction: column;
  height: 100%;
  /* 组件内统一 border-box：本面板有一层 padding 容器 + 一层 padding 卡片，
   * 任何一层按 content-box 计算都会让内容比抽屉宽出内边距，右侧被裁掉。 */
  box-sizing: border-box;
}

.cache-manager *,
.cache-manager *::before,
.cache-manager *::after {
  box-sizing: border-box;
}

/* ─── 顶栏：与阅读设置同一套尺寸与留白 ─── */
.panel-head {
  display: flex;
  align-items: flex-start;
  justify-content: space-between;
  gap: 12px;
  padding: 22px 20px 14px;
  flex-shrink: 0;
}

.panel-head-main {
  min-width: 0;
}

.panel-head h3 {
  margin: 0;
  font-size: 16px;
  font-weight: 600;
}

.panel-sub {
  margin: 4px 0 0;
  font-size: 12px;
  color: var(--cm-muted);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.icon-btn {
  flex: 0 0 auto;
  width: 28px;
  height: 28px;
  display: flex;
  align-items: center;
  justify-content: center;
  border-radius: 8px;
  border: 1px solid transparent;
  background: transparent;
  color: var(--cm-label);
  cursor: pointer;
  transition: background 0.15s, color 0.15s, border-color 0.15s;
}

.icon-btn svg {
  width: 16px;
  height: 16px;
}

.icon-btn:hover {
  background: color-mix(in srgb, var(--cm-font, currentColor) 6%, transparent);
  color: var(--cm-font, currentColor);
}

.panel-body {
  flex: 1;
  min-height: 0;
  overflow-y: auto;
  padding: 0 20px 24px;
  display: flex;
  flex-direction: column;
  gap: 20px;
  -webkit-overflow-scrolling: touch;
  overscroll-behavior: contain;
}

/* ─── 分组 ─── */
.group {
  display: flex;
  flex-direction: column;
  gap: 8px;
}

.group-head {
  display: flex;
  align-items: baseline;
  justify-content: space-between;
  gap: 10px;
  padding: 0 3px;
}

.group-head h4 {
  margin: 0;
  font-size: 11.5px;
  font-weight: 600;
  letter-spacing: 0.1em;
  color: var(--cm-muted);
}

.card {
  border-radius: 14px;
  border: 1px solid var(--cm-card-border);
  background: var(--cm-card);
  padding: 4px 14px;
  /* 同理：卡片本身也要 border-box，避免内容区被内边距撑出面板。 */
  box-sizing: border-box;
}

.card-rows {
  display: flex;
  flex-direction: column;
  /* 行自己带左右内边距（见 .row-btn），卡片这里只留上下空隙，
     否则内边距会叠加成两层。 */
  padding: 2px 0;
  overflow: hidden;
  box-sizing: border-box;
}

/* ─── 统计 ─── */
.stat-row {
  display: flex;
  align-items: baseline;
  gap: 8px;
  padding: 12px 0;
}

.stat {
  display: flex;
  align-items: baseline;
  gap: 3px;
}

.stat-value {
  font-size: 26px;
  font-weight: 600;
  line-height: 1;
  font-variant-numeric: tabular-nums;
}

.stat-unit {
  font-size: 12px;
  color: var(--cm-muted);
}

.stat-caption {
  font-size: 12px;
  color: var(--cm-muted);
}

/* ─── 进度 ─── */
.progress-block {
  display: flex;
  flex-direction: column;
  gap: 6px;
  padding: 4px 0 12px;
  border-top: 1px solid var(--cm-line);
}

.progress-line {
  display: flex;
  align-items: center;
  gap: 10px;
  margin-top: 10px;
}

.progress-track {
  flex: 1;
  height: 4px;
  border-radius: 999px;
  background: color-mix(in srgb, var(--cm-font, currentColor) 15%, transparent);
  overflow: hidden;
}

.progress-fill {
  height: 100%;
  border-radius: 999px;
  background: var(--cm-accent);
  transition: width 0.28s ease;
}

.progress-num {
  flex: 0 0 auto;
  font-size: 12px;
  font-variant-numeric: tabular-nums;
  color: var(--cm-label);
}

.progress-status {
  margin: 4px 0 0;
  font-size: 12.5px;
  color: var(--cm-label);
}

.progress-chapter {
  margin: 0;
  font-size: 11.5px;
  color: var(--cm-muted);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.btn {
  align-self: flex-start;
  margin-top: 6px;
  min-height: 30px;
  padding: 5px 12px;
  border-radius: 8px;
  border: 1px solid var(--cm-line-strong);
  background: transparent;
  color: var(--cm-label);
  font-size: 12.5px;
  cursor: pointer;
  transition: background 0.15s, color 0.15s, border-color 0.15s;
}

.btn-quiet:hover {
  background: color-mix(in srgb, var(--cm-font, currentColor) 6%, transparent);
  color: var(--cm-font, currentColor);
}

/* ─── 操作行 ─── */
.row-btn {
  position: relative;
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
  /* 行自己带左右内边距（卡片在这里不留内边距），
     这样文字与上方标题对齐，悬停底色又能铺到卡片边缘。
     不要用负外边距抵内边距：那样行会比卡片宽 28px，右侧连同卡片圆角一起被裁掉。 */
  width: 100%;
  /* 显式 border-box：按钮若按 content-box 计算，width:100% 再加左右内边距
   * 会比卡片宽 28px，右侧连同圆角与箭头一起被裁掉。 */
  box-sizing: border-box;
  margin: 0;
  padding: 11px 14px;
  border: none;
  border-radius: 8px;
  background: transparent;
  color: inherit;
  text-align: left;
  cursor: pointer;
  transition: background 0.15s;
}

.row-btn + .row-btn {
  /* 分隔线用伪元素而不是 border-top：行有圆角，边框会顺着圆角拐弯，
   * 而伪元素可以只画一条两端内缩的直线。 */
  border-top: none;
}

.row-btn + .row-btn::before {
  content: "";
  position: absolute;
  top: 0;
  left: 14px;
  right: 14px;
  height: 1px;
  background: var(--cm-line);
}

.row-btn:hover:not(:disabled) {
  background: color-mix(in srgb, var(--cm-font, currentColor) 5%, transparent);
}

.row-btn:hover:not(:disabled) .row-label {
  color: var(--cm-accent);
}

/* 右侧箭头：这些行整行可点，没有它就只有文字变色这一点提示。 */
.row-btn::after {
  content: "";
  flex: 0 0 auto;
  width: 7px;
  height: 7px;
  margin-left: 2px;
  border-right: 1.5px solid var(--cm-muted);
  border-bottom: 1.5px solid var(--cm-muted);
  transform: rotate(-45deg);
  transition: border-color 0.15s, transform 0.15s;
}

.row-btn:hover:not(:disabled)::after {
  border-color: var(--cm-accent);
  transform: rotate(-45deg) translate(2px, 0);
}

/* 禁用态只弱化文字与箭头，不整体降透明度：
 * 整体变淡会让红色「清除本地缓存」变成浑浊的暗红，看起来像样式错了。 */
.row-btn:disabled {
  cursor: not-allowed;
}

.row-btn:disabled .row-label {
  color: var(--cm-muted);
}

.row-btn:disabled .row-sub {
  opacity: 0.7;
}

.row-btn:disabled::after {
  opacity: 0.35;
}

.row-btn:active:not(:disabled) {
  background: color-mix(in srgb, var(--cm-font, currentColor) 8%, transparent);
}

.row-text {
  display: flex;
  flex-direction: column;
  gap: 3px;
  min-width: 0;
}

.row-label {
  font-size: 13.5px;
  font-weight: 500;
  transition: color 0.15s;
}

.row-sub {
  font-size: 11.5px;
  color: var(--cm-muted);
  line-height: 1.4;
}

.row-danger .row-label {
  color: var(--cm-danger);
}

.row-danger:hover:not(:disabled) .row-label {
  color: var(--cm-danger);
  text-decoration: underline;
}

/* ─── 说明 ─── */
.hint {
  margin: 0;
  padding: 0 3px;
  font-size: 11.5px;
  line-height: 1.6;
  color: var(--cm-muted);
}

.note {
  margin-top: 4px;
  padding: 12px 14px;
  border-radius: 12px;
  border: 1px solid var(--cm-card-border);
  background: var(--cm-card);
  font-size: 12.5px;
  line-height: 1.65;
  color: var(--cm-label);
}

.link-btn {
  padding: 2px 6px;
  border: none;
  border-radius: 6px;
  background: transparent;
  color: var(--cm-accent);
  font-size: 12px;
  cursor: pointer;
  transition: background 0.15s;
}

.link-btn:hover {
  background: var(--cm-accent-fill);
}

@media (prefers-reduced-motion: reduce) {
  .cache-manager * {
    transition-duration: 0.01ms !important;
  }
}
</style>
