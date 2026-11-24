<template>
  <Teleport to="body">
    <Transition name="cl-fade">
      <div v-if="modelValue" class="cl-overlay" @click="close"></div>
    </Transition>
    <Transition name="cl-pop">
      <div v-if="modelValue" class="cl-layer" @click.self="close">
        <section class="cl-modal" role="dialog" aria-modal="true" aria-label="缓存管理">
          <header class="cl-head">
            <div class="cl-head-text">
              <h2>缓存管理</h2>
              <p>{{ summaryText }}</p>
            </div>
            <div class="cl-head-actions">
              <button class="cl-icon-btn" type="button" title="刷新" aria-label="刷新" :disabled="loading" @click="refreshData(true)">
                <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8">
                  <path d="M20 11a8 8 0 1 0-2.3 5.7" />
                  <path d="M20 4v7h-7" />
                </svg>
              </button>
              <button class="cl-icon-btn" type="button" title="关闭" aria-label="关闭" @click="close">
                <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8">
                  <path d="M18 6 6 18M6 6l12 12" />
                </svg>
              </button>
            </div>
          </header>

          <div v-if="loading && !overview.length" class="cl-loading">
            <span class="cl-spinner"></span>
            <p>正在读取缓存信息</p>
          </div>

          <div v-else class="cl-body" :class="{ 'is-detail': mobileDetail && !!selected }">
            <aside class="cl-side">
              <div class="cl-search">
                <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8">
                  <circle cx="11" cy="11" r="7" />
                  <path d="m20 20-3.5-3.5" />
                </svg>
                <input v-model="query" type="search" placeholder="搜索书名或作者" />
              </div>

              <div class="cl-filters">
                <button
                  v-for="item in filters"
                  :key="item.key"
                  class="cl-filter"
                  :class="{ active: filterKey === item.key }"
                  type="button"
                  @click="filterKey = item.key"
                >
                  {{ item.label }}
                  <span>{{ item.count }}</span>
                </button>
              </div>

              <div class="cl-sort">
                <label for="cl-sort-select">排序</label>
                <select id="cl-sort-select" v-model="sortKey">
                  <option value="smart">智能排序</option>
                  <option value="recent">最近缓存</option>
                  <option value="missing">缺失最多</option>
                  <option value="name">书名</option>
                  <option value="shelf">书架顺序</option>
                </select>
              </div>

              <div ref="listRef" class="cl-book-list">
                <button
                  v-for="item in visibleBooks"
                  :key="item.bookUrl"
                  class="cl-book"
                  :class="{ active: item.bookUrl === selectedUrl }"
                  type="button"
                  @click="selectBook(item.bookUrl)"
                >
                  <img v-if="coverOf(item)" class="cl-cover" :src="coverOf(item)" alt="" loading="lazy" />
                  <span v-else class="cl-cover cl-cover-empty">{{ item.name.slice(0, 1) }}</span>
                  <span class="cl-book-main">
                    <span class="cl-book-top">
                      <span class="cl-book-name">{{ item.name }}</span>
                      <span class="cl-chip" :class="chipClass(item)">{{ chipText(item) }}</span>
                    </span>
                    <span class="cl-book-sub">
                      <span>{{ item.author || '未知作者' }}</span>
                      <span class="cl-book-origin">{{ item.originName || item.origin }}</span>
                    </span>
                    <span class="cl-book-track">
                      <span class="cl-book-fill" :style="{ width: `${percentOf(item)}%` }"></span>
                    </span>
                    <span class="cl-book-meta">
                      <span>{{ displayCached(item) }} / {{ item.totalChapterCount }} 章</span>
                      <span v-if="item.totalChapterCount === 0">待同步目录</span>
                      <span v-else-if="missingOf(item) > 0">还缺 {{ missingOf(item) }} 章</span>
                      <span v-else>全部缓存</span>
                      <span v-if="item.failedCount > 0" class="cl-fail-text">{{ item.failedCount }} 失败</span>
                    </span>
                  </span>
                </button>
                <div v-if="!visibleBooks.length" class="cl-empty">没有匹配的书籍</div>
              </div>
            </aside>

            <main class="cl-detail">
              <button v-if="mobileDetail && !!selected" class="cl-back" type="button" @click="backToList">
                <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8"><path d="m15 18-6-6 6-6" /></svg>
                返回书籍列表
              </button>

              <template v-if="selected">
                <div class="cl-book-head">
                  <img v-if="coverOf(selected)" class="cl-detail-cover" :src="coverOf(selected)" alt="" />
                  <span v-else class="cl-detail-cover cl-cover-empty">{{ selected.name.slice(0, 1) }}</span>
                  <div class="cl-book-head-text">
                    <h3>{{ selected.name }}</h3>
                    <p>
                      <span>{{ selected.author || '未知作者' }}</span>
                      <span class="cl-muted">{{ selected.originName || selected.origin }}</span>
                    </p>
                  </div>
                </div>

                <section class="cl-summary">
                  <div class="cl-hero">
                    <span class="cl-hero-percent">{{ detailPercentText }}</span>
                    <div class="cl-hero-text">
                      <strong>{{ detailCached }} / {{ detailTotal }} 章</strong>
                      <span>{{ detailSub }}</span>
                    </div>
                    <div class="cl-hero-side">
                      <span>{{ formatSize(selected.cacheSizeBytes) }}</span>
                      <span>{{ formatRelative(selected.updatedAt) }}</span>
                      <span v-if="detailFailed > 0" class="cl-fail-text">失败 {{ detailFailed }} 章</span>
                    </div>
                  </div>
                  <div class="cl-stack">
                    <span class="cl-stack-cached" :style="{ width: `${detailCachedPercent}%` }"></span>
                    <span class="cl-stack-failed" :style="{ width: `${detailFailedPercent}%` }"></span>
                  </div>
                </section>

                <section v-if="selectedTask" class="cl-task">
                  <div class="cl-task-top">
                    <span>{{ taskLabel }}</span>
                    <span>{{ taskPercent(selectedTask) }}%</span>
                  </div>
                  <div class="cl-task-track">
                    <span class="cl-task-fill" :style="{ width: `${taskPercent(selectedTask)}%` }"></span>
                  </div>
                  <div class="cl-task-meta">
                    <span>{{ selectedTask.cached }} / {{ selectedTask.total || '…' }} 章</span>
                    <span v-if="selectedTask.failed > 0" class="cl-fail-text">失败 {{ selectedTask.failed }}</span>
                    <span v-if="selectedTask.success > 0">本轮新增 {{ selectedTask.success }}</span>
                    <span v-if="selectedTask.round > 0">自动重试 {{ selectedTask.round }} / 2</span>
                    <button class="cl-link cl-danger" type="button" :disabled="abortingUrl === selected.bookUrl" @click="abortTask(selected.bookUrl)">{{ abortingUrl === selected.bookUrl ? '中断中' : '中断' }}</button>
                  </div>
                </section>

                <section ref="chapterSectionRef" class="cl-block cl-chapter-block">
                  <div class="cl-block-head">
                    <h4>章节</h4>
                  </div>
                  <div class="cl-chapter-tabs">
                    <button type="button" :class="{ active: chapterTab === 'missing' }" @click="chapterTab = 'missing'">
                      未缓存 <span>{{ chapterMissingCount }}</span>
                    </button>
                    <button type="button" :class="{ active: chapterTab === 'cached' }" @click="chapterTab = 'cached'">
                      已缓存 <span>{{ chapterCachedCount }}</span>
                    </button>
                    <button type="button" :class="{ active: chapterTab === 'all' }" @click="chapterTab = 'all'">
                      全部 <span>{{ chaptersOfBook.length }}</span>
                    </button>
                  </div>
                  <div class="cl-chapter-toolbar">
                    <div class="cl-search cl-chapter-search">
                      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8">
                        <circle cx="11" cy="11" r="7" />
                        <path d="m20 20-3.5-3.5" />
                      </svg>
                      <input v-model="chapterQuery" type="search" placeholder="搜索章节" />
                    </div>
                  </div>
                  <div class="cl-chapter-actions">
                    <div class="cl-chapter-links">
                      <button class="cl-link" type="button" :disabled="detailBusy" @click="selectMissingChapters">全选未缓存</button>
                      <button class="cl-link" type="button" :disabled="detailBusy" @click="selectAllChapters">全选本书</button>
                      <button class="cl-link" type="button" :disabled="!selectedChapterUrls.size" @click="clearChapterSelection">清除选择</button>
                    </div>
                    <label v-if="chapterTab === 'cached' || chapterTab === 'all'" class="cl-overwrite" title="已缓存章节也重新抓取">
                      <input v-model="overwriteCached" type="checkbox" :disabled="detailBusy" />
                      <span>覆盖已缓存</span>
                    </label>
                  </div>
                  <div class="cl-chapter-list">
                    <label
                      v-for="chapter in visibleChapters"
                      :key="chapter.url"
                      class="cl-chapter-row"
                      :class="{ 'is-cached': chapter.cached, 'is-picked': selectedChapterUrls.has(chapter.url) }"
                    >
                      <input
                        type="checkbox"
                        :checked="selectedChapterUrls.has(chapter.url)"
                        @change="toggleChapter(chapter.url)"
                      />
                      <span class="cl-chapter-index">{{ chapterSequenceOf(chapter) }}</span>
                      <span class="cl-chapter-title">{{ chapter.title }}</span>
                      <span v-if="chapterTab !== 'missing'" class="cl-chapter-state" :class="{ 'is-cached': chapter.cached }">
                        {{ chapter.cached ? '已缓存' : '未缓存' }}
                      </span>
                    </label>
                    <div v-if="manifestLoading" class="cl-empty-inline">正在读取章节</div>
                    <div v-else-if="!visibleChapters.length" class="cl-empty-inline">{{ chapterEmptyText }}</div>
                  </div>
                  <button
                    v-if="filteredChapters.length > chapterLimit"
                    class="cl-load-more"
                    type="button"
                    @click="chapterLimit += 120"
                  >加载更多</button>
                </section>
                <section v-if="selected.failedCount > 0" class="cl-block">
                  <div class="cl-block-head">
                    <h4>失败章节</h4>
                    <button
                      class="cl-link"
                      type="button"
                      :disabled="detailBusy"
                      @click="retryAllFailed(selected)"
                    >全部重试</button>
                  </div>
                  <div class="cl-fail-list">
                    <div v-for="entry in selected.failedChapters" :key="entry.url" class="cl-fail-row">
                      <div class="cl-fail-main">
                        <span class="cl-fail-title">{{ entry.title }}</span>
                        <span class="cl-fail-reason" :title="entry.reason">{{ entry.reason || '未知原因' }}</span>
                      </div>
                      <span class="cl-fail-attempts">{{ entry.attempts }} 次</span>
                      <button class="cl-mini-btn" type="button" :disabled="detailBusy" @click="retryChapter(entry)">重试</button>
                    </div>
                  </div>
                </section>
                <div class="cl-actions">
                  <template v-if="selectedChapterUrls.size">
                    <button class="cl-primary cl-primary-wide" type="button" :disabled="detailBusy" @click="cacheSelected">
                      {{ selectedCacheActionText }}
                    </button>
                    <button class="cl-ghost" type="button" @click="clearChapterSelection">取消选择</button>
                  </template>
                  <template v-else>
                    <div class="cl-split">
                      <button class="cl-primary" type="button" :disabled="detailBusy || (detailTotal > 0 && detailMissing === 0)" @click="cacheMissing(0)">
                        {{ detailTotal === 0 ? '同步目录' : detailMissing > 0 ? '补全缺失 ' + detailMissing + ' 章' : '已全部缓存' }}
                      </button>
                      <button class="cl-primary cl-split-toggle" type="button" :disabled="detailBusy" aria-label="更多缓存范围" @click="menuOpen = !menuOpen">▾</button>
                      <div v-if="menuOpen" class="cl-menu">
                        <button type="button" @click="cacheMissing(50)">补 50 章缺失</button>
                        <button type="button" @click="cacheMissing(100)">补 100 章缺失</button>
                        <button type="button" @click="cacheMissing(0)">补全所有缺失章</button>
                        <button type="button" @click="openChapterPicker">选择章节</button>
                      </div>
                    </div>
                  </template>
                  <div class="cl-more">
                    <button class="cl-ghost" type="button" @click="moreOpen = !moreOpen">更多</button>
                    <div v-if="moreOpen" class="cl-menu is-right">
                      <button type="button" :disabled="selected.failedCount === 0 || detailBusy" @click="retryAllFailed(selected)">重试失败章节</button>
                      <button class="cl-danger" type="button" :disabled="detailBusy" @click="clearCache(selected)">清空本地缓存</button>
                    </div>
                  </div>
                </div>
              </template>

              <div v-else class="cl-placeholder">
                <div class="cl-placeholder-icon">
                  <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5">
                    <path d="M4 5.5A2.5 2.5 0 0 1 6.5 3H20v15H6.5A2.5 2.5 0 0 0 4 20.5z" />
                    <path d="M4 20.5A2.5 2.5 0 0 1 6.5 18H20" />
                  </svg>
                </div>
                <p>选择一本书查看缓存详情</p>
              </div>
            </main>
          </div>
        </section>
      </div>
    </Transition>
  </Teleport>

</template>

<script setup lang="ts">
/* eslint-disable @typescript-eslint/no-unused-vars */
import { computed, nextTick, onUnmounted, ref, watch } from 'vue'
import { useAppStore } from '../stores/app'
import { useBookshelfStore } from '../stores/bookshelf'
import { deleteBookCache, getCoverUrl } from '../api/bookshelf'
import { cacheBookSSE, cancelCacheBook, getCacheManifest, getCacheOverview } from '../api/cache'
import type { CacheBookOverview, CacheChapterEntry, CacheFailedEntry, CacheManifest } from '../types'
import type { SseLike } from '../api/sse'

const props = defineProps<{ modelValue: boolean }>()
const emit = defineEmits<{ 'update:modelValue': [value: boolean] }>()

const appStore = useAppStore()
const shelfStore = useBookshelfStore()

type FilterKey = 'all' | 'active' | 'working' | 'done'
type SortKey = 'smart' | 'recent' | 'missing' | 'name' | 'shelf'
type ChapterTab = 'missing' | 'cached' | 'all'

interface TaskState {
  total: number
  cached: number
  success: number
  failed: number
  round: number
}

interface TaskOptions {
  bookUrl: string
  mode: 'range' | 'missing' | 'failed' | 'selected'
  count: number
  chapterUrls?: string[]
  overwrite?: boolean
  /** 单章重试等临时任务结束后保留用户在章节列表里的勾选。 */
  keepSelection?: boolean
}

interface TaskStream {
  stream: SseLike
  round: number
  timer: number | null
  options: TaskOptions
}

const overview = ref<CacheBookOverview[]>([])
const loading = ref(false)
const query = ref('')
const filterKey = ref<FilterKey>('all')
const sortKey = ref<SortKey>('smart')
const selectedUrl = ref<string | null>(null)
const mobileDetail = ref(false)
const menuOpen = ref(false)
const moreOpen = ref(false)
const listRef = ref<HTMLElement | null>(null)
const chapterSectionRef = ref<HTMLElement | null>(null)
const abortingUrl = ref<string | null>(null)
const overwriteCached = ref(false)

const manifest = ref<CacheManifest | null>(null)
const manifestLoading = ref(false)
let manifestToken = 0
let cacheOverviewToken = 0

const chapterTab = ref<ChapterTab>('missing')
const chapterQuery = ref('')
const chapterLimit = ref(120)
const selectedChapterUrls = ref<Set<string>>(new Set())

const tasks = ref<Record<string, TaskState>>({})
const streams = new Map<string, TaskStream>()

const selected = computed(() => overview.value.find((book) => book.bookUrl === selectedUrl.value) ?? null)
const selectedTask = computed(() => (selectedUrl.value ? tasks.value[selectedUrl.value] ?? null : null))
const detailBusy = computed(() => !!selectedTask.value)

const summaryText = computed(() => {
  const books = overview.value.length
  const chapters = overview.value.reduce((sum, book) => sum + displayCached(book), 0)
  const running = Object.keys(tasks.value).length
  return `${books} 本书  ${chapters} 章已缓存  ${running} 个任务进行中`
})

const filters = computed<Array<{ key: FilterKey; label: string; count: number }>>(() => {
  const all = overview.value
  return [
    { key: 'all', label: '全部', count: all.length },
    { key: 'active', label: '未完成', count: all.filter((book) => missingOf(book) > 0 || book.failedCount > 0).length },
    { key: 'working', label: '缓存中', count: all.filter((book) => !!tasks.value[book.bookUrl]).length },
    { key: 'done', label: '已完成', count: all.filter(isComplete).length },
  ]
})

const visibleBooks = computed(() => {
  const keyword = query.value.trim().toLowerCase()
  let list = overview.value.filter((book) => {
    if (keyword && !`${book.name} ${book.author}`.toLowerCase().includes(keyword)) return false
    if (filterKey.value === 'active') return missingOf(book) > 0 || book.failedCount > 0
    if (filterKey.value === 'working') return !!tasks.value[book.bookUrl]
    if (filterKey.value === 'done') return isComplete(book)
    return true
  })

  list = [...list]
  if (sortKey.value === 'recent') {
    list.sort((a, b) => b.updatedAt - a.updatedAt)
  } else if (sortKey.value === 'missing') {
    list.sort((a, b) => missingOf(b) - missingOf(a))
  } else if (sortKey.value === 'name') {
    list.sort((a, b) => a.name.localeCompare(b.name, 'zh-Hans-CN'))
  } else if (sortKey.value === 'shelf') {
    const order = new Map(overview.value.map((book, index) => [book.bookUrl, index]))
    list.sort((a, b) => (order.get(a.bookUrl) ?? 0) - (order.get(b.bookUrl) ?? 0))
  } else {
    list.sort((a, b) => {
      const aRunning = tasks.value[a.bookUrl] ? 1 : 0
      const bRunning = tasks.value[b.bookUrl] ? 1 : 0
      if (aRunning !== bRunning) return bRunning - aRunning
      if (a.failedCount !== b.failedCount) return b.failedCount - a.failedCount
      return b.updatedAt - a.updatedAt
    })
  }
  return list
})

const chaptersOfBook = computed<CacheChapterEntry[]>(() => {
  return (manifest.value?.chapters ?? []).filter((chapter) => !chapter.isVolume)
})

const chapterSequences = computed(() => {
  const map = new Map<string, number>()
  chaptersOfBook.value.forEach((chapter, index) => map.set(chapter.url, index + 1))
  return map
})

function chapterSequenceOf(chapter: CacheChapterEntry) {
  return chapterSequences.value.get(chapter.url) ?? chapter.index + 1
}

const chapterCachedCount = computed(() => chaptersOfBook.value.filter((chapter) => chapter.cached).length)
const chapterMissingCount = computed(() => chaptersOfBook.value.length - chapterCachedCount.value)

const filteredChapters = computed<CacheChapterEntry[]>(() => {
  const keyword = chapterQuery.value.trim().toLowerCase()
  return chaptersOfBook.value.filter((chapter) => {
    if (chapterTab.value === 'missing' && chapter.cached) return false
    if (chapterTab.value === 'cached' && !chapter.cached) return false
    if (keyword && !chapter.title.toLowerCase().includes(keyword)) return false
    return true
  })
})

const visibleChapters = computed(() => filteredChapters.value.slice(0, chapterLimit.value))
const chapterEmptyText = computed(() => {
  if (!manifest.value) return '章节目录读取失败'
  if (!chaptersOfBook.value.length) return '目录尚未缓存'
  if (chapterTab.value === 'missing') return '没有未缓存章节'
  if (chapterTab.value === 'cached') return '还没有已缓存章节'
  return '没有匹配的章节'
})
const selectedCacheActionText = computed(() => `缓存所选 ${selectedChapterUrls.value.size} 章`)

const detailCached = computed(() => (selected.value ? displayCached(selected.value) : 0))
const detailTotal = computed(() => selected.value?.totalChapterCount ?? 0)
const detailFailed = computed(() => selected.value?.failedCount ?? 0)
const detailMissing = computed(() => missingOf(selected.value))
const detailCachedPercent = computed(() => (detailTotal.value ? Math.min(100, Math.round((detailCached.value / detailTotal.value) * 100)) : 0))
const detailFailedPercent = computed(() => (detailTotal.value ? Math.min(100 - detailCachedPercent.value, Math.round((detailFailed.value / detailTotal.value) * 100)) : 0))
const detailPercentText = computed(() => (detailTotal.value ? `${detailCachedPercent.value}%` : '0%'))
const detailSub = computed(() => {
  if (!detailTotal.value) return '目录尚未同步'
  return detailMissing.value > 0 ? `还缺 ${detailMissing.value} 章` : '已全部缓存'
})
const taskLabel = computed(() => (selectedTask.value?.round ? `缓存任务 第 ${selectedTask.value.round} 轮重试` : '缓存任务进行中'))

function displayCached(book: CacheBookOverview) {
  return Math.min(book.cachedChapterCount, book.totalChapterCount)
}

function missingOf(book: CacheBookOverview | null) {
  if (!book) return 0
  return Math.max(0, book.totalChapterCount - displayCached(book))
}

function percentOf(book: CacheBookOverview) {
  if (!book.totalChapterCount) return 0
  return Math.min(100, Math.round((displayCached(book) / book.totalChapterCount) * 100))
}

function isComplete(book: CacheBookOverview) {
  // 目录还没落到本机时总章数为 0，不能当成「已全部缓存」。
  return book.totalChapterCount > 0 && missingOf(book) === 0 && book.failedCount === 0
}

function chipText(book: CacheBookOverview) {
  if (tasks.value[book.bookUrl]) return '缓存中'
  if (book.totalChapterCount === 0) return '待同步'
  if (isComplete(book)) return '已完成'
  if (book.failedCount > 0) return '有失败'
  return '未完成'
}

function chipClass(book: CacheBookOverview) {
  if (tasks.value[book.bookUrl]) return 'is-running'
  if (book.totalChapterCount === 0) return 'is-idle'
  if (isComplete(book)) return 'is-done'
  if (book.failedCount > 0) return 'is-fail'
  return 'is-idle'
}

function coverOf(book: CacheBookOverview) {
  return book.coverUrl ? getCoverUrl(book.coverUrl, book.origin) : ''
}

function formatSize(bytes: number) {
  if (!bytes) return '0 B'
  const units = ['B', 'KB', 'MB', 'GB']
  let value = bytes
  let index = 0
  while (value >= 1024 && index < units.length - 1) {
    value /= 1024
    index += 1
  }
  return `${value >= 10 || index === 0 ? Math.round(value) : value.toFixed(1)} ${units[index]}`
}

function formatRelative(timestamp: number) {
  if (!timestamp) return '暂无'
  const diff = Date.now() - timestamp
  if (diff < 60_000) return '刚刚'
  if (diff < 3_600_000) return `${Math.floor(diff / 60_000)} 分钟前`
  if (diff < 86_400_000) return `${Math.floor(diff / 3_600_000)} 小时前`
  if (diff < 2_592_000_000) return `${Math.floor(diff / 86_400_000)} 天前`
  return new Date(timestamp).toLocaleDateString('zh-CN')
}

function taskPercent(task: TaskState) {
  if (!task.total) return 0
  return Math.min(100, Math.round(((task.cached + task.failed) / task.total) * 100))
}

function resetChapterView() {
  chapterTab.value = 'missing'
  chapterQuery.value = ''
  chapterLimit.value = 120
  overwriteCached.value = false
  selectedChapterUrls.value = new Set()
}

async function loadManifest(bookUrl: string) {
  const token = ++manifestToken
  manifestLoading.value = true
  try {
    const data = await getCacheManifest(bookUrl)
    if (token !== manifestToken || selectedUrl.value !== bookUrl) return
    manifest.value = data
    if (data) {
      // 概览可能因缺目录只拿到占位总章数。详情补抓目录后同步回列表，
      // 避免左侧仍显示 0 / 0。
      const book = overview.value.find((item) => item.bookUrl === bookUrl)
      if (book) {
        const chapters = data.chapters.filter((chapter) => !chapter.isVolume)
        if (chapters.length) book.totalChapterCount = chapters.length
        book.cachedChapterCount = chapters.filter((chapter) => chapter.cached).length
        book.failedCount = data.failed.length
        book.updatedAt = data.updatedAt
      }
    }
  } catch {
    if (token === manifestToken && selectedUrl.value === bookUrl) manifest.value = null
  } finally {
    if (token === manifestToken) manifestLoading.value = false
  }
}

function selectBook(bookUrl: string) {
  selectedUrl.value = bookUrl
  if (window.innerWidth <= 860) mobileDetail.value = true
  menuOpen.value = false
  moreOpen.value = false
}

function backToList() {
  mobileDetail.value = false
  menuOpen.value = false
  moreOpen.value = false
}

function close() {
  emit('update:modelValue', false)
}

async function refreshData(showSpinner = false) {
  const token = ++cacheOverviewToken
  if (showSpinner) loading.value = true
  try {
    const list = await getCacheOverview()
    if (token !== cacheOverviewToken) return
    mergeOverview(list)
    shelfStore.setCachedChapterCounts(new Map(
      list.map((book) => [book.bookUrl, Math.min(book.cachedChapterCount, book.totalChapterCount)]),
    ))
  } catch {
    appStore.showToast('缓存信息读取失败', 'error')
  } finally {
    if (token === cacheOverviewToken) loading.value = false
  }
}

function mergeOverview(list: CacheBookOverview[]) {
  const scrollTop = listRef.value?.scrollTop ?? 0
  const previous = new Map(overview.value.map((book) => [book.bookUrl, book]))
  overview.value = list.map((book) => {
    const old = previous.get(book.bookUrl)
    if (!old) return book
    Object.assign(old, book)
    return old
  })
  if (!selectedUrl.value && overview.value.length) {
    selectedUrl.value = overview.value[0].bookUrl
  } else if (selectedUrl.value && !overview.value.some((book) => book.bookUrl === selectedUrl.value)) {
    selectedUrl.value = overview.value[0]?.bookUrl ?? null
  }
  requestAnimationFrame(() => {
    if (listRef.value) listRef.value.scrollTop = scrollTop
  })
}

function startTask(options: TaskOptions, round = 0) {
  const bookUrl = options.bookUrl
  closeTask(bookUrl)
  tasks.value = { ...tasks.value, [bookUrl]: { total: 0, cached: 0, success: 0, failed: 0, round } }

  const stream = cacheBookSSE({
    bookUrl,
    count: options.count,
    concurrentCount: 8,
    mode: options.mode,
    chapterUrls: options.chapterUrls,
    overwrite: options.overwrite,
  })

  const record: TaskStream = { stream, round, timer: null, options }

  stream.onmessage = (event) => {
    const data = event.data as { totalChapters?: number; cachedCount?: number; successCount?: number; failedCount?: number }
    tasks.value = {
      ...tasks.value,
      [bookUrl]: {
        total: data.totalChapters ?? 0,
        cached: data.cachedCount ?? 0,
        success: data.successCount ?? 0,
        failed: data.failedCount ?? 0,
        round,
      },
    }
  }

  stream.addEventListener('end', (event) => {
    const data = event.data as { failedCount?: number; failedUrls?: string[]; aborted?: boolean }
    const failed = data.failedCount ?? 0
    if (!data.aborted && failed > 0 && round < 2) {
      // selected 只重试本轮真正失败的章节，不能把整批再抓一遍。
      const retry = options.mode === 'selected'
        ? (data.failedUrls?.length ? { ...options, chapterUrls: data.failedUrls } : null)
        : { ...options, mode: 'failed' as const, count: 0 }
      if (retry) {
        record.timer = window.setTimeout(() => startTask(retry, round + 1), 1200)
        return
      }
    }
    finishTask(bookUrl)
  })

  stream.onerror = (event) => {
    const payload = (event as { data?: unknown } | null)?.data
    const reason = typeof payload === 'string'
      ? payload
      : (payload as { errorMsg?: string } | undefined)?.errorMsg
    appStore.showToast(reason ? `缓存任务中断 ${reason}` : '缓存任务中断', 'error')
    // 异常时保留已选章节，用户可以直接重试。
    finishTask(bookUrl, true)
  }
  streams.set(bookUrl, record)
}

function finishTask(bookUrl: string, keepSelection = false) {
  const record: TaskStream | undefined = streams.get(bookUrl)
  closeTask(bookUrl)
  delete tasks.value[bookUrl]
  tasks.value = { ...tasks.value }
  if (!keepSelection && !record?.options.keepSelection && record?.options.mode === 'selected' && selectedUrl.value === bookUrl) {
    selectedChapterUrls.value = new Set()
  }
  if (abortingUrl.value === bookUrl) abortingUrl.value = null
  void refreshData().then(() => {
    if (bookUrl === selectedUrl.value) void loadManifest(bookUrl)
  })
}

function closeTask(bookUrl: string) {
  const record = streams.get(bookUrl)
  if (!record) return
  if (record.timer) window.clearTimeout(record.timer)
  record.stream.close()
  streams.delete(bookUrl)
}

async function abortTask(bookUrl: string) {
  const record = streams.get(bookUrl)
  if (!record || abortingUrl.value === bookUrl) return
  abortingUrl.value = bookUrl
  // 正处于自动重试等待时后端已无任务，取消标志会返回 false。
  if (record.timer) {
    window.clearTimeout(record.timer)
    record.timer = null
  }
  const result = await cancelCacheBook(bookUrl).catch(() => null)
  if (!streams.has(bookUrl)) return
  if (!result?.cancelled) {
    // 后端还没登记任务时取消会返回 false；提示用户稍后再试，不要假装已中断。
    appStore.showToast('任务尚未开始，请稍后再试', 'warning')
    abortingUrl.value = null
    return
  }
  // 等后端 end 事件把已抓章节落账后再刷新，避免读到半截状态。
  record.timer = window.setTimeout(() => finishTask(bookUrl), 15000)
}

function cacheMissing(count: number) {
  if (!selected.value || detailBusy.value) return
  menuOpen.value = false
  startTask({ bookUrl: selected.value.bookUrl, mode: 'missing', count })
}

function retryAllFailed(book: CacheBookOverview) {
  moreOpen.value = false
  if (!book.failedCount || tasks.value[book.bookUrl]) return
  startTask({ bookUrl: book.bookUrl, mode: 'failed', count: 0 })
}

function retryChapter(entry: CacheFailedEntry) {
  if (!selected.value || tasks.value[selected.value.bookUrl]) return
  // 单章重试复用当前书源，按选中模式只缓存这一章并覆盖失败时留下的空状态。
  startTask({
    bookUrl: selected.value.bookUrl,
    mode: 'selected',
    count: 0,
    chapterUrls: [entry.url],
    overwrite: true,
    keepSelection: true,
  })
}

function toggleChapter(url: string) {
  const next = new Set(selectedChapterUrls.value)
  if (next.has(url)) next.delete(url)
  else next.add(url)
  selectedChapterUrls.value = next
}

function selectMissingChapters() {
  const missing = chaptersOfBook.value.filter((chapter) => !chapter.cached).map((chapter) => chapter.url)
  selectedChapterUrls.value = new Set(missing)
}

function clearChapterSelection() {
  selectedChapterUrls.value = new Set()
}

function selectAllChapters() {
  selectedChapterUrls.value = new Set(chaptersOfBook.value.map((chapter) => chapter.url))
}

function openChapterPicker() {
  menuOpen.value = false
  chapterTab.value = 'missing'
  chapterQuery.value = ''
  chapterLimit.value = 120
  nextTick(() => {
    chapterSectionRef.value?.scrollIntoView({ behavior: 'smooth', block: 'start' })
  })
}

function cacheSelected() {
  if (!selected.value || detailBusy.value || !selectedChapterUrls.value.size) return
  // 当前源且未勾选覆盖时，已缓存章节会被后端过滤掉；整批都命中就没必要起任务。
  if (!overwriteCached.value) {
    const cached = new Set(chaptersOfBook.value.filter((chapter) => chapter.cached).map((chapter) => chapter.url))
    if ([...selectedChapterUrls.value].every((url) => cached.has(url))) {
      appStore.showToast('所选章节均已缓存', 'warning')
      return
    }
  }
  startTask({
    bookUrl: selected.value.bookUrl,
    mode: 'selected',
    count: 0,
    chapterUrls: [...selectedChapterUrls.value],
    overwrite: overwriteCached.value,
  })
}

async function clearCache(book: CacheBookOverview) {
  moreOpen.value = false
  if (tasks.value[book.bookUrl]) return
  const ok = await appStore.confirmDialog(`清空《${book.name}》的全部本地缓存？只删本机文件，不影响书架。`, {
    title: '清空本地缓存',
    danger: true,
  })
  if (!ok) return
  await cancelCacheBook(book.bookUrl).catch(() => null)
  closeTask(book.bookUrl)
  if (tasks.value[book.bookUrl]) {
    delete tasks.value[book.bookUrl]
    tasks.value = { ...tasks.value }
  }
  try {
    await deleteBookCache(book.bookUrl)
    appStore.showToast(`《${book.name}》缓存已清空`, 'success')
  } catch {
    appStore.showToast('缓存清空失败', 'error')
    return
  }
  await refreshData()
  if (selectedUrl.value) await loadManifest(selectedUrl.value)
}

watch(
  selectedUrl,
  (bookUrl, previous) => {
    if (bookUrl === previous) return
    manifestToken += 1
    manifest.value = null
    resetChapterView()
    if (bookUrl) void loadManifest(bookUrl)
  },
)

watch([chapterTab, chapterQuery], () => {
  chapterLimit.value = 120
})

watch(
  () => props.modelValue,
  (visible) => {
    if (!visible) {
      menuOpen.value = false
      moreOpen.value = false
      mobileDetail.value = false
      return
    }
    // 概览会顺带按磁盘重建清单，等它完成再读章节，避免拿到旧的缓存状态。
    void refreshData(!overview.value.length).then(() => {
      if (!selectedUrl.value && overview.value.length) selectedUrl.value = overview.value[0].bookUrl
      else if (selectedUrl.value) void loadManifest(selectedUrl.value)
    })
  },
)

onUnmounted(() => {
  // 弹窗销毁只断开事件通道，后端任务继续跑；本次进度不保留，重开后按磁盘清单刷新。
  for (const record of streams.values()) {
    if (record.timer) window.clearTimeout(record.timer)
    record.stream.close()
  }
  streams.clear()
  manifestToken += 1
})
</script>

<style scoped>
.cl-overlay {
  position: fixed;
  inset: 0;
  z-index: var(--z-overlay);
  background: rgba(12, 14, 16, 0.52);
  backdrop-filter: blur(4px);
}

.cl-layer {
  position: fixed;
  inset: 0;
  z-index: var(--z-modal);
  display: flex;
  align-items: center;
  justify-content: center;
  padding: 24px;
}

.cl-modal {
  width: min(1080px, 100%);
  height: min(720px, calc(100vh - 48px));
  display: flex;
  flex-direction: column;
  overflow: hidden;
  border: 1px solid var(--color-border-light);
  border-radius: 18px;
  background: var(--color-bg-elevated);
  box-shadow: var(--shadow-xl);
}

.cl-head {
  display: flex;
  align-items: flex-start;
  justify-content: space-between;
  gap: 16px;
  padding: 20px 22px 16px;
  border-bottom: 1px solid var(--color-border-light);
}

.cl-head h2 {
  margin: 0;
  font-size: 19px;
  font-weight: 600;
  letter-spacing: 0.01em;
}

.cl-head-text {
  min-width: 0;
}

.cl-head p {
  margin: 5px 0 0;
  font-size: 12.5px;
  color: var(--color-text-tertiary);
  font-variant-numeric: tabular-nums;
}

.cl-head-actions {
  display: flex;
  gap: 6px;
}

.cl-icon-btn {
  display: grid;
  place-items: center;
  width: 32px;
  height: 32px;
  padding: 0;
  border: 1px solid transparent;
  border-radius: 9px;
  background: transparent;
  color: var(--color-text-secondary);
  cursor: pointer;
  transition: background 0.15s, color 0.15s, border-color 0.15s;
}

.cl-icon-btn svg {
  width: 17px;
  height: 17px;
}

.cl-icon-btn:hover:not(:disabled) {
  background: var(--color-bg-hover);
  color: var(--color-text);
}

.cl-icon-btn:disabled {
  opacity: 0.4;
  cursor: default;
}

.cl-body {
  flex: 1;
  min-height: 0;
  display: grid;
  grid-template-columns: 360px minmax(0, 1fr);
}

.cl-side {
  display: flex;
  flex-direction: column;
  min-height: 0;
  border-right: 1px solid var(--color-border-light);
  background: var(--color-bg-sunken);
}

.cl-search {
  position: relative;
  padding: 14px 16px 0;
}

.cl-search svg {
  position: absolute;
  top: 26px;
  left: 28px;
  width: 15px;
  height: 15px;
  color: var(--color-text-tertiary);
  pointer-events: none;
}

.cl-search input {
  width: 100%;
  height: 34px;
  padding: 0 12px 0 34px;
  border: 1px solid var(--color-border-light);
  border-radius: 9px;
  background: var(--color-bg-elevated);
  color: var(--color-text);
  font-size: 13px;
  outline: none;
  transition: border-color 0.15s, box-shadow 0.15s;
}

.cl-search input::placeholder {
  color: var(--color-text-tertiary);
}

.cl-search input:focus {
  border-color: var(--color-primary-border);
  box-shadow: 0 0 0 3px var(--color-primary-bg);
}

.cl-filters {
  display: flex;
  gap: 6px;
  padding: 12px 16px 0;
  overflow-x: auto;
  scrollbar-width: none;
}

.cl-filters::-webkit-scrollbar {
  display: none;
}

.cl-filter {
  flex: 0 0 auto;
  display: inline-flex;
  align-items: center;
  gap: 5px;
  height: 27px;
  padding: 0 10px;
  border: 1px solid var(--color-border-light);
  border-radius: 999px;
  background: transparent;
  color: var(--color-text-secondary);
  font-size: 12px;
  cursor: pointer;
  transition: background 0.15s, color 0.15s, border-color 0.15s;
}

.cl-filter span {
  font-size: 11px;
  color: var(--color-text-tertiary);
  font-variant-numeric: tabular-nums;
}

.cl-filter:hover {
  color: var(--color-text);
  border-color: var(--color-border);
}

.cl-filter.active {
  color: var(--color-primary-soft-text);
  border-color: var(--color-primary-soft-border);
  background: var(--color-primary-soft-bg);
}

.cl-filter.active span {
  color: inherit;
  opacity: 0.7;
}

.cl-sort {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 10px 16px 8px;
}

.cl-sort label {
  font-size: 11.5px;
  color: var(--color-text-tertiary);
}

.cl-sort select {
  flex: 1;
  height: 28px;
  padding: 0 8px;
  border: 1px solid var(--color-border-light);
  border-radius: 7px;
  background: var(--color-bg-elevated);
  color: var(--color-text-secondary);
  font-size: 12px;
  outline: none;
  cursor: pointer;
}

.cl-book-list {
  flex: 1;
  min-height: 0;
  overflow-y: auto;
  padding: 4px 10px 12px;
}

.cl-book {
  display: flex;
  gap: 11px;
  width: 100%;
  padding: 10px;
  border: 1px solid transparent;
  border-radius: 11px;
  background: transparent;
  color: inherit;
  text-align: left;
  cursor: pointer;
  transition: background 0.15s, border-color 0.15s;
}

.cl-book:hover {
  background: var(--color-bg-hover);
}

.cl-book.active {
  border-color: var(--color-primary-soft-border);
  background: var(--color-primary-soft-bg);
}

.cl-cover {
  flex: 0 0 auto;
  width: 44px;
  height: 60px;
  border-radius: 6px;
  object-fit: cover;
  background: var(--color-bg-active);
}

.cl-cover-empty {
  display: grid;
  place-items: center;
  font-size: 16px;
  color: var(--color-text-tertiary);
}

.cl-book-main {
  flex: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
  gap: 4px;
  padding-top: 1px;
}

.cl-book-top {
  display: flex;
  align-items: center;
  gap: 7px;
  min-width: 0;
}

.cl-book-name {
  flex: 1;
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  font-size: 13.5px;
  font-weight: 550;
}

.cl-chip {
  flex: 0 0 auto;
  padding: 1px 6px;
  border-radius: 999px;
  font-size: 10.5px;
  line-height: 16px;
  border: 1px solid transparent;
}

.cl-chip.is-running {
  color: var(--color-primary-soft-text);
  background: var(--color-primary-soft-bg);
  border-color: var(--color-primary-soft-border);
}

.cl-chip.is-done {
  color: var(--color-success);
  background: color-mix(in srgb, var(--color-success) 12%, transparent);
  border-color: color-mix(in srgb, var(--color-success) 26%, transparent);
}

.cl-chip.is-fail {
  color: var(--color-danger);
  background: color-mix(in srgb, var(--color-danger) 11%, transparent);
  border-color: color-mix(in srgb, var(--color-danger) 24%, transparent);
}

.cl-chip.is-idle {
  color: var(--color-text-tertiary);
  background: var(--color-bg-active);
  border-color: var(--color-border-light);
}

.cl-book-sub {
  display: flex;
  gap: 8px;
  min-width: 0;
  font-size: 11.5px;
  color: var(--color-text-tertiary);
}

.cl-book-sub > span:first-child {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.cl-book-origin {
  flex: 0 0 auto;
  max-width: 38%;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  opacity: 0.8;
}

.cl-book-track {
  height: 3px;
  margin-top: 2px;
  border-radius: 999px;
  background: var(--color-border-light);
  overflow: hidden;
}

.cl-book-fill {
  display: block;
  height: 100%;
  border-radius: inherit;
  background: var(--color-primary);
  transition: width 0.25s ease;
}

.cl-book-meta {
  display: flex;
  gap: 8px;
  font-size: 11px;
  color: var(--color-text-tertiary);
  font-variant-numeric: tabular-nums;
}

.cl-fail-text {
  color: var(--color-danger);
}

.cl-empty,
.cl-empty-inline {
  padding: 22px 10px;
  text-align: center;
  font-size: 12.5px;
  color: var(--color-text-tertiary);
}

.cl-empty-inline {
  padding: 12px 0;
  text-align: left;
}

.cl-loading {
  flex: 1;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 14px;
  color: var(--color-text-tertiary);
  font-size: 13px;
}

.cl-spinner {
  width: 26px;
  height: 26px;
  border: 2.5px solid var(--color-border-light);
  border-top-color: var(--color-primary);
  border-radius: 50%;
  animation: cl-spin 0.8s linear infinite;
}

@keyframes cl-spin {
  to { transform: rotate(360deg); }
}.cl-detail {
  position: relative;
  display: flex;
  flex-direction: column;
  gap: 16px;
  min-height: 0;
  overflow-y: auto;
  padding: 18px 22px 20px;
}

.cl-back {
  display: inline-flex;
  align-items: center;
  gap: 5px;
  align-self: flex-start;
  padding: 4px 8px 4px 4px;
  border: none;
  border-radius: 7px;
  background: transparent;
  color: var(--color-text-secondary);
  font-size: 12.5px;
  cursor: pointer;
}

.cl-back svg {
  width: 15px;
  height: 15px;
}

.cl-back:hover {
  background: var(--color-bg-hover);
  color: var(--color-text);
}

.cl-book-head {
  display: flex;
  gap: 13px;
  align-items: center;
}

.cl-detail-cover {
  flex: 0 0 auto;
  width: 54px;
  height: 74px;
  border-radius: 7px;
  object-fit: cover;
  background: var(--color-bg-active);
}

.cl-book-head-text {
  min-width: 0;
}

.cl-book-head-text h3 {
  margin: 0;
  font-size: 17px;
  font-weight: 600;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.cl-book-head-text p {
  display: flex;
  gap: 8px;
  margin: 5px 0 0;
  font-size: 12.5px;
  color: var(--color-text-secondary);
}

.cl-muted {
  color: var(--color-text-tertiary);
}

.cl-summary {
  display: flex;
  flex-direction: column;
  gap: 10px;
  padding: 16px 18px;
  border: 1px solid var(--color-border-light);
  border-radius: 13px;
  background: var(--color-bg-sunken);
}

.cl-hero {
  display: flex;
  align-items: baseline;
  gap: 12px;
}

.cl-hero-percent {
  font-size: 34px;
  font-weight: 600;
  line-height: 1;
  letter-spacing: -0.02em;
  font-variant-numeric: tabular-nums;
  color: var(--color-primary);
}

.cl-hero-text {
  display: flex;
  flex-direction: column;
  gap: 2px;
}

.cl-hero-text strong {
  font-size: 13px;
  font-weight: 550;
  font-variant-numeric: tabular-nums;
}

.cl-hero-text span {
  font-size: 11.5px;
  color: var(--color-text-tertiary);
}

.cl-hero-side {
  display: flex;
  align-items: center;
  gap: 10px;
  margin-left: auto;
  font-size: 11.5px;
  color: var(--color-text-tertiary);
  font-variant-numeric: tabular-nums;
}

.cl-stack {
  display: flex;
  height: 7px;
  border-radius: 999px;
  background: var(--color-border-light);
  overflow: hidden;
}

.cl-stack-cached {
  height: 100%;
  background: var(--color-primary);
  transition: width 0.3s ease;
}

.cl-stack-failed {
  height: 100%;
  background: var(--color-danger);
  opacity: 0.75;
  transition: width 0.3s ease;
}

.cl-task {
  display: flex;
  flex-direction: column;
  gap: 7px;
  padding: 13px 15px;
  border: 1px solid var(--color-primary-soft-border);
  border-radius: 12px;
  background: var(--color-primary-soft-bg);
}

.cl-task-top {
  display: flex;
  justify-content: space-between;
  gap: 10px;
  font-size: 12.5px;
  font-weight: 550;
  color: var(--color-primary-soft-text);
}

.cl-task-top span:last-child {
  font-variant-numeric: tabular-nums;
}

.cl-task-track {
  height: 5px;
  border-radius: 999px;
  background: color-mix(in srgb, var(--color-primary) 18%, transparent);
  overflow: hidden;
}

.cl-task-fill {
  display: block;
  height: 100%;
  border-radius: inherit;
  background: var(--color-primary);
  transition: width 0.25s ease;
}

.cl-task-meta {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 6px 12px;
  font-size: 11.5px;
  color: var(--color-text-secondary);
  font-variant-numeric: tabular-nums;
}

.cl-task-meta .cl-link {
  margin-left: auto;
}

.cl-link {
  padding: 2px 5px;
  border: none;
  border-radius: 5px;
  background: transparent;
  color: var(--color-primary-soft-text);
  font-size: 12px;
  cursor: pointer;
}

.cl-link:hover {
  background: var(--color-primary-soft-bg-hover);
}

.cl-link.cl-danger {
  color: var(--color-danger);
}

.cl-link:disabled {
  opacity: 0.45;
  cursor: default;
}

.cl-block {
  display: flex;
  flex-direction: column;
  gap: 9px;
}

.cl-block-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 10px;
}

.cl-block-head h4 {
  margin: 0;
  font-size: 13px;
  font-weight: 600;
}

.cl-chapter-block {
  padding-top: 2px;
}

.cl-chapter-tabs {
  display: flex;
  gap: 2px;
  padding: 3px;
  border-radius: 9px;
  background: var(--color-bg-sunken);
}

.cl-chapter-tabs button {
  flex: 1;
  height: 28px;
  border: none;
  border-radius: 7px;
  background: transparent;
  color: var(--color-text-tertiary);
  font-size: 12px;
  cursor: pointer;
}

.cl-chapter-tabs button span {
  margin-left: 3px;
  font-variant-numeric: tabular-nums;
}

.cl-chapter-tabs button.active {
  background: var(--color-bg-elevated);
  color: var(--color-text);
  box-shadow: 0 1px 3px rgba(0, 0, 0, 0.08);
}

.cl-chapter-toolbar {
  display: flex;
  align-items: center;
  gap: 10px;
}

.cl-chapter-search {
  flex: 1;
  min-width: 0;
  padding: 0;
}

/* 列表搜索框没有外层 padding 放大镜需按输入框自身高度定位 */
.cl-chapter-search svg {
  top: 7px;
  left: 10px;
}

.cl-chapter-search input {
  width: 100%;
  height: 30px;
  padding: 0 10px 0 30px;
  border: 1px solid var(--color-border-light);
  border-radius: 8px;
  background: var(--color-bg-sunken);
  color: var(--color-text);
  font-size: 12px;
}

.cl-chapter-actions {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 6px 8px;
  padding: 8px 10px;
  border: 1px solid var(--color-border-light);
  border-radius: 9px;
  background: var(--color-bg-sunken);
}

.cl-chapter-links {
  display: flex;
  align-items: center;
  gap: 4px;
}

.cl-overwrite {
  display: inline-flex;
  align-items: center;
  gap: 4px;
  font-size: 11.5px;
  color: var(--color-text-tertiary);
  cursor: pointer;
  user-select: none;
}

.cl-overwrite input {
  width: 13px;
  height: 13px;
  accent-color: var(--color-primary);
}

.cl-chapter-list {
  display: flex;
  flex-direction: column;
  max-height: 320px;
  overflow-y: auto;
  border: 1px solid var(--color-border-light);
  border-radius: 10px;
}

.cl-chapter-row {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 7px 10px;
  font-size: 12.5px;
  cursor: pointer;
}

.cl-chapter-row + .cl-chapter-row {
  border-top: 1px solid var(--color-border-light);
}

.cl-chapter-row:hover {
  background: var(--color-bg-hover);
}

.cl-chapter-row.is-picked {
  background: var(--color-primary-soft-bg);
}

.cl-chapter-row input {
  flex: 0 0 auto;
  width: 14px;
  height: 14px;
  accent-color: var(--color-primary);
}

.cl-chapter-index {
  flex: 0 0 34px;
  color: var(--color-text-tertiary);
  font-size: 11px;
  font-variant-numeric: tabular-nums;
}

.cl-chapter-title {
  flex: 1;
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.cl-chapter-state {
  flex: 0 0 auto;
  padding: 0 6px;
  border-radius: 4px;
  background: var(--color-bg-active);
  color: var(--color-text-tertiary);
  font-size: 10.5px;
  line-height: 17px;
}

.cl-chapter-state.is-cached {
  background: color-mix(in srgb, var(--color-success) 12%, transparent);
  color: var(--color-success);
}

.cl-load-more {
  align-self: center;
  padding: 4px 10px;
  border: none;
  background: transparent;
  color: var(--color-primary-soft-text);
  font-size: 12px;
  cursor: pointer;
}

.cl-fail-list {
  display: flex;
  flex-direction: column;
  gap: 1px;
}

.cl-fail-row {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 6px 0;
}

.cl-fail-row + .cl-fail-row {
  border-top: 1px solid var(--color-border-light);
}

.cl-fail-main {
  flex: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
  gap: 2px;
}

.cl-fail-title {
  font-size: 12.5px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.cl-fail-reason {
  font-size: 11px;
  color: var(--color-text-tertiary);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.cl-fail-attempts {
  flex: 0 0 auto;
  font-size: 11px;
  color: var(--color-text-tertiary);
  font-variant-numeric: tabular-nums;
}

.cl-mini-btn {
  flex: 0 0 auto;
  height: 24px;
  padding: 0 8px;
  border: 1px solid var(--color-border-light);
  border-radius: 6px;
  background: transparent;
  color: var(--color-text-secondary);
  font-size: 11.5px;
  cursor: pointer;
  transition: background 0.15s, color 0.15s, border-color 0.15s;
}

.cl-mini-btn:hover:not(:disabled) {
  border-color: var(--color-primary-border);
  background: var(--color-primary-soft-bg);
  color: var(--color-primary-soft-text);
}

.cl-mini-btn:disabled {
  opacity: 0.5;
  cursor: default;
}

.cl-actions {
  position: sticky;
  bottom: -20px;
  display: flex;
  gap: 8px;
  margin: auto -22px -20px;
  padding: 12px 22px 16px;
  border-top: 1px solid var(--color-border-light);
  background: var(--color-bg-elevated);
}

.cl-split {
  position: relative;
  display: flex;
}

.cl-primary.cl-primary-wide {
  flex: 0 0 auto;
  border-radius: 9px;
}

.cl-actions-note {
  align-self: center;
  font-size: 11.5px;
  color: var(--color-text-tertiary);
  font-variant-numeric: tabular-nums;
}

.cl-primary {
  height: 34px;
  padding: 0 16px;
  border: 1px solid var(--color-primary);
  border-radius: 9px 0 0 9px;
  background: var(--color-primary);
  color: var(--color-text-inverse);
  font-size: 12.5px;
  font-weight: 550;
  cursor: pointer;
  transition: filter 0.15s;
}

.cl-primary:hover:not(:disabled) {
  filter: brightness(1.06);
}

.cl-primary:disabled {
  opacity: 0.55;
  cursor: default;
}

.cl-split-toggle {
  width: 30px;
  padding: 0;
  border-left: none;
  border-radius: 0 9px 9px 0;
}

.cl-ghost {
  height: 34px;
  padding: 0 14px;
  border: 1px solid var(--color-border);
  border-radius: 9px;
  background: transparent;
  color: var(--color-text-secondary);
  font-size: 12.5px;
  cursor: pointer;
  transition: background 0.15s, color 0.15s;
}

.cl-ghost:hover {
  background: var(--color-bg-hover);
  color: var(--color-text);
}

.cl-more {
  position: relative;
}

.cl-menu {
  position: absolute;
  left: 0;
  bottom: calc(100% + 6px);
  z-index: 5;
  min-width: 172px;
  padding: 5px;
  border: 1px solid var(--color-border);
  border-radius: 10px;
  background: var(--color-bg-elevated);
  box-shadow: var(--shadow-lg);
}

.cl-menu.is-right {
  left: auto;
  right: 0;
}

.cl-menu button {
  display: block;
  width: 100%;
  padding: 8px 10px;
  border: none;
  border-radius: 6px;
  background: transparent;
  color: var(--color-text-secondary);
  font-size: 12.5px;
  text-align: left;
  cursor: pointer;
}

.cl-menu button:hover:not(:disabled) {
  background: var(--color-bg-hover);
  color: var(--color-text);
}

.cl-menu button.cl-danger {
  color: var(--color-danger);
}

.cl-menu button:disabled {
  opacity: 0.45;
  cursor: default;
}

.cl-placeholder {
  flex: 1;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 12px;
  color: var(--color-text-tertiary);
  font-size: 13px;
}

.cl-placeholder-icon {
  display: grid;
  place-items: center;
  width: 44px;
  height: 44px;
  border-radius: 50%;
  background: var(--color-bg-sunken);
  color: var(--color-text-tertiary);
}

.cl-placeholder-icon svg {
  width: 22px;
  height: 22px;
}

.cl-fade-enter-active,
.cl-fade-leave-active {
  transition: opacity 0.18s ease;
}

.cl-fade-enter-from,
.cl-fade-leave-to {
  opacity: 0;
}

.cl-pop-enter-active,
.cl-pop-leave-active {
  transition: opacity 0.18s ease, transform 0.18s ease;
}

.cl-pop-enter-from,
.cl-pop-leave-to {
  opacity: 0;
  transform: translateY(8px) scale(0.99);
}

@media (max-width: 860px) {
  .cl-layer {
    padding: 12px;
  }

  .cl-modal {
    height: min(720px, calc(100vh - 24px));
  }

  .cl-body {
    grid-template-columns: 1fr;
  }

  .cl-side {
    border-right: none;
  }

  .cl-body.is-detail .cl-side {
    display: none;
  }

  .cl-body:not(.is-detail) .cl-detail {
    display: none;
  }

  .cl-filters {
    padding-bottom: 2px;
  }
}

@media (max-width: 560px) {
  .cl-head {
    padding: 16px 16px 12px;
  }

  .cl-detail {
    padding: 14px 16px 16px;
  }

  /* 详情内边距在这里缩到 16px，底部操作栏的负边距必须同步，否则会横向溢出。 */
  .cl-actions {
    margin: auto -16px -16px;
    padding: 12px 16px 14px;
    flex-wrap: wrap;
  }

  .cl-hero-side {
    width: 100%;
    justify-content: flex-start;
    margin-left: 0;
  }

  .cl-hero {
    flex-wrap: wrap;
  }

  .cl-hero-percent {
    font-size: 28px;
  }
}
</style>
