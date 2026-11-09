<template>
  <div class="stats-view">
    <div class="stats-content">
      <header class="stats-header">
        <div>
          <p class="stats-eyebrow">READING RECORD</p>
          <h1 class="stats-title">阅读数据统计</h1>
          <p class="stats-subtitle">持续记录每一次阅读，看看时间都去了哪里。</p>
        </div>
        <div class="stats-range" role="group" aria-label="统计时间范围">
          <button
            v-for="r in ranges"
            :key="r.days"
            class="range-chip"
            :class="{ active: activeRange === r.days }"
            @click="setRange(r.days)"
          >
            {{ r.label }}
          </button>
        </div>
      </header>

      <section class="overview-grid">
        <div class="overview-main">
          <span class="overview-label">累计阅读时长</span>
          <strong class="overview-value">{{ formatDuration(summary.totalSeconds) }}</strong>
          <span class="overview-note">{{ summary.readBooks }} 本书记录 · {{ summary.activeDays }} 个活跃日</span>
        </div>
        <div class="overview-mini">
          <span>今日</span>
          <strong>{{ formatDuration(summary.todaySeconds) }}</strong>
        </div>
        <div class="overview-mini">
          <span>本月</span>
          <strong>{{ formatDuration(summary.monthSeconds) }}</strong>
        </div>
        <div class="overview-mini">
          <span>{{ activeRangeLabel }}</span>
          <strong>{{ formatDuration(rangeSummary.totalSeconds) }}</strong>
        </div>
      </section>

      <section class="panel chart-panel">
        <header class="panel-head">
          <div>
            <h2>每日阅读时长</h2>
            <p>{{ activeRangeLabel }} · 单位：分钟</p>
          </div>
        </header>
        <div ref="chartRef" class="chart"></div>
      </section>

      <div class="detail-grid">
        <section class="panel daily-panel">
          <header class="panel-head">
            <div>
              <h2>每日明细</h2>
              <p>最近 14 个有记录的日子</p>
            </div>
          </header>
          <div v-if="dailyRows.length" class="daily-list">
            <div v-for="row in dailyRows" :key="row.date" class="daily-row">
              <div class="daily-date">
                <strong>{{ formatMonthDay(row.date) }}</strong>
                <span>{{ formatWeekday(row.date) }}</span>
              </div>
              <div class="daily-bar"><span :style="{ width: `${row.width}%` }"></span></div>
              <strong class="daily-value">{{ formatDuration(row.seconds) }}</strong>
            </div>
          </div>
          <div v-else class="empty-state">开始阅读后，这里会出现每日记录。</div>
        </section>

        <section class="panel rank-panel">
          <header class="panel-head">
            <div>
              <h2>阅读时长排行</h2>
              <p>{{ activeRangeLabel }}</p>
            </div>
            <span class="book-total">{{ bookStats.length }} 本</span>
          </header>
          <div v-if="bookStats.length" class="book-stats-list">
            <div
              v-for="(book, index) in bookStats"
              :key="book.bookUrl"
              class="book-stat-row"
              @contextmenu.prevent="handleBookStatsContextMenu($event, book)"
            >
              <span class="book-rank">{{ index + 1 }}</span>
              <div class="book-stat-main">
                <div class="book-stat-meta">
                  <strong :title="book.bookName">{{ book.bookName }}</strong>
                  <small>{{ bookShare(book.seconds) }}%</small>
                </div>
                <div class="book-progress" aria-hidden="true">
                  <span :style="{ width: `${bookProgressWidth(book.seconds)}%` }"></span>
                </div>
              </div>
              <div class="book-stat-value">
                <strong>{{ formatDuration(book.seconds) }}</strong>
                <small>{{ formatMonthDay(book.lastReadDate) }}</small>
              </div>
            </div>
          </div>
          <div v-else class="empty-state">当前范围内还没有阅读排行。</div>
        </section>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import * as echarts from 'echarts'
import {
  deleteReadingStatsByBook,
  getReadingStatsByBook,
  getReadingStatsDaily,
  getReadingStatsSummary,
  type BookReadingStats,
  type DailyReadingStats,
  type ReadingStatsSummary,
} from '../api/readingStats'
import { useAppStore } from '../stores/app'
import { showContextMenu } from '../composables/useContextMenu'

const ranges = [
  { days: 7, label: '近7天' },
  { days: 30, label: '近30天' },
  { days: 90, label: '近90天' },
] as const

const weekdays = ['一', '二', '三', '四', '五', '六', '日']
const activeRange = ref(30)
const appStore = useAppStore()
const chartRef = ref<HTMLElement | null>(null)
const summary = ref<ReadingStatsSummary>({
  totalSeconds: 0,
  todaySeconds: 0,
  monthSeconds: 0,
  activeDays: 0,
  readBooks: 0,
})
const dailyStats = ref<DailyReadingStats[]>([])
const bookStats = ref<BookReadingStats[]>([])
let chart: echarts.ECharts | null = null
let loadToken = 0

const activeRangeLabel = computed(() => ranges.find((range) => range.days === activeRange.value)?.label || '')
const rangeSummary = computed(() => ({
  totalSeconds: dailyStats.value.reduce((total, day) => total + day.seconds, 0),
  activeDays: dailyStats.value.filter((day) => day.seconds > 0).length,
}))
const maxBookSeconds = computed(() => Math.max(0, ...bookStats.value.map((book) => book.seconds)))
const trackedBookSeconds = computed(() => bookStats.value.reduce((total, book) => total + book.seconds, 0))
const maxDailySeconds = computed(() => Math.max(1, ...dailyStats.value.map((day) => day.seconds)))
const dailyRows = computed(() => dailyStats.value
  .filter((day) => day.seconds > 0)
  .slice(-14)
  .reverse()
  .map((day) => ({
    ...day,
    width: Math.max(4, Math.round(day.seconds / maxDailySeconds.value * 100)),
  })))
function formatDuration(seconds: number) {
  if (seconds <= 0) return '0分钟'
  if (seconds < 60) return '少于1分钟'
  const mins = Math.floor(seconds / 60)
  const hours = Math.floor(mins / 60)
  const remMins = mins % 60
  if (hours > 0) return `${hours}小时${remMins}分`
  return `${Math.max(1, mins)}分钟`
}

function bookProgressWidth(seconds: number) {
  if (!maxBookSeconds.value) return 0
  return Math.max(4, Math.round((seconds / maxBookSeconds.value) * 100))
}

function bookShare(seconds: number) {
  if (!trackedBookSeconds.value) return 0
  return Math.round((seconds / trackedBookSeconds.value) * 100)
}

function handleBookStatsContextMenu(event: MouseEvent, book: BookReadingStats) {
  const menuItems = [
    {
      label: '删除该书阅读记录',
      danger: true,
      action: () => handleDeleteBookStats(book),
    },
  ]
  showContextMenu(event, menuItems, book)
}

async function handleDeleteBookStats(book: BookReadingStats) {
  const ok = await appStore.confirmDialog(
    `确定删除「${book.bookName}」的阅读时长记录？此操作不可恢复。`,
    { title: '删除阅读记录', danger: true },
  )
  if (!ok) return
  try {
    await deleteReadingStatsByBook(book.bookUrl)
    appStore.showToast(`已删除「${book.bookName}」的阅读记录`, 'success')
    await loadStats()
  } catch (e: unknown) {
    appStore.showToast((e as Error).message || '删除失败', 'error')
  }
}

function formatMonthDay(value: string) {
  const parts = value.split('-')
  return parts.length === 3 ? `${Number(parts[1])}/${Number(parts[2])}` : value
}

function formatWeekday(value: string) {
  const date = new Date(`${value}T00:00:00`)
  return Number.isNaN(date.getTime()) ? '' : `周${weekdays[(date.getDay() + 6) % 7]}`
}

function dateStr(offsetDays: number) {
  const d = new Date()
  d.setDate(d.getDate() - offsetDays)
  return localDateKey(d)
}

/** 统计日期必须按用户本地日历算，不能用 UTC；否则东八区凌晨会落到前一天。 */
function localDateKey(date: Date) {
  const year = date.getFullYear()
  const month = String(date.getMonth() + 1).padStart(2, '0')
  const day = String(date.getDate()).padStart(2, '0')
  return `${year}-${month}-${day}`
}

async function loadStats() {
  const token = ++loadToken
  const end = dateStr(0)
  const rangeStart = dateStr(activeRange.value - 1)
  const start = dateStr(Math.max(activeRange.value - 1, 111))
  const [daily, books, overview] = await Promise.all([
    getReadingStatsDaily(start, end),
    getReadingStatsByBook(rangeStart, end),
    getReadingStatsSummary(),
  ])
  if (token !== loadToken) return
  summary.value = overview
  dailyStats.value = daily
  bookStats.value = books
  renderChart(daily.filter((day) => day.date >= rangeStart), rangeStart, end)
}

function renderChart(daily: DailyReadingStats[], start: string, end: string) {
  if (!chartRef.value) return
  if (!chart) chart = echarts.init(chartRef.value)

  const byDate = new Map(daily.map((day) => [day.date, day.seconds]))
  const dates: string[] = []
  const minutes: number[] = []
  const cursor = new Date(`${start}T00:00:00`)
  const last = new Date(`${end}T00:00:00`)
  while (cursor <= last) {
    const key = localDateKey(cursor)
    dates.push(key.slice(5))
    minutes.push(Math.round((byDate.get(key) || 0) / 60))
    cursor.setDate(cursor.getDate() + 1)
  }

  const styles = getComputedStyle(document.documentElement)
  const primary = styles.getPropertyValue('--color-primary').trim()
  const textTertiary = styles.getPropertyValue('--color-text-tertiary').trim()
  const divider = styles.getPropertyValue('--color-divider').trim()

  chart.setOption({
    grid: { left: 8, right: 8, top: 18, bottom: 8, containLabel: true },
    tooltip: { trigger: 'axis', valueFormatter: (value: number) => `${value} 分钟` },
    xAxis: {
      type: 'category',
      data: dates,
      axisLabel: { fontSize: 10, color: textTertiary },
      axisLine: { lineStyle: { color: divider } },
    },
    yAxis: {
      type: 'value',
      axisLabel: { fontSize: 10, color: textTertiary },
      splitLine: { lineStyle: { color: divider } },
    },
    series: [{
      type: 'bar',
      data: minutes,
      itemStyle: { color: primary, borderRadius: [3, 3, 0, 0] },
      barMaxWidth: 18,
    }],
  })
}

function setRange(days: number) {
  activeRange.value = days
  void loadStats()
}

onMounted(() => {
  void loadStats()
  window.addEventListener('resize', resizeChart)
})

function resizeChart() {
  chart?.resize()
}

onBeforeUnmount(() => {
  window.removeEventListener('resize', resizeChart)
  chart?.dispose()
  chart = null
})

watch(() => appStore.theme, () => {
  void loadStats()
})
</script>

<style scoped>
.stats-view {
  height: 100%;
  min-height: 0;
  overflow: hidden;
}

.stats-content {
  height: 100%;
  max-width: var(--content-max-width);
  margin: 0 auto;
  padding: 0 var(--space-6);
  display: flex;
  flex-direction: column;
  min-height: 0;
  overflow: auto;
}

.stats-header {
  padding: var(--space-6) 0 var(--space-2);
  display: flex;
  align-items: flex-end;
  justify-content: space-between;
  gap: var(--space-4);
  flex-shrink: 0;
}

.stats-eyebrow {
  margin: 0 0 6px;
  color: var(--color-primary);
  font-size: 10px;
  font-weight: 700;
  letter-spacing: 0.14em;
}

.stats-title {
  margin: 0;
  font-size: var(--text-2xl);
  font-weight: 700;
  letter-spacing: 0;
}

.stats-subtitle {
  margin: 8px 0 0;
  color: var(--color-text-tertiary);
  font-size: 13px;
}

.stats-range {
  display: flex;
  gap: 6px;
  flex-shrink: 0;
}

.range-chip {
  padding: 7px 12px;
  border: 1px solid var(--color-border-light);
  border-radius: var(--radius-full);
  background: var(--color-bg-elevated);
  color: var(--color-text-secondary);
  font-size: 12px;
  transition: all var(--duration-fast) var(--ease-out);
}

.range-chip.active {
  border-color: var(--color-primary);
  background: var(--color-primary-bg);
  color: var(--color-primary);
}

.overview-grid {
  display: grid;
  grid-template-columns: minmax(0, 1.7fr) repeat(3, minmax(0, 1fr));
  gap: 12px;
  flex-shrink: 0;
}

.overview-main,
.overview-mini,
.panel {
  border: 1px solid var(--color-border-light);
  border-radius: var(--radius-lg);
  background: var(--color-bg-elevated);
}

.overview-main {
  padding: 22px;
  display: flex;
  flex-direction: column;
  gap: 7px;
  background: linear-gradient(135deg, var(--color-primary-bg), var(--color-bg-elevated) 72%);
}

.overview-label,
.overview-mini span {
  color: var(--color-text-tertiary);
  font-size: 12px;
}

.overview-value {
  color: var(--color-primary);
  font-size: 30px;
  line-height: 1.15;
  font-variant-numeric: tabular-nums;
}

.overview-note {
  color: var(--color-text-secondary);
  font-size: 12px;
}

.overview-mini {
  padding: 16px;
  display: flex;
  flex-direction: column;
  justify-content: center;
  gap: 7px;
}

.overview-mini strong {
  color: var(--color-text);
  font-size: 18px;
  font-variant-numeric: tabular-nums;
}

.panel {
  padding: 18px;
  flex-shrink: 0;
}

.panel-head {
  display: flex;
  align-items: flex-start;
  justify-content: space-between;
  gap: 12px;
  margin-bottom: 14px;
}

.panel-head h2 {
  margin: 0;
  color: var(--color-text);
  font-size: 15px;
  font-weight: 650;
}

.panel-head p {
  margin: 4px 0 0;
  color: var(--color-text-tertiary);
  font-size: 12px;
}

.book-total {
  padding: 4px 9px;
  border-radius: var(--radius-md);
  background: var(--color-primary-bg);
  color: var(--color-primary);
  font-size: 11px;
  font-weight: 600;
  white-space: nowrap;
}

.chart {
  height: 220px;
  width: 100%;
}

.detail-grid {
  display: grid;
  grid-template-columns: minmax(0, 0.82fr) minmax(0, 1.18fr);
  gap: 16px;
  flex-shrink: 0;
}

.daily-list,
.book-stats-list {
  display: flex;
  flex-direction: column;
}

.daily-row {
  display: grid;
  grid-template-columns: 70px minmax(0, 1fr) 86px;
  align-items: center;
  gap: 10px;
  min-height: 46px;
  border-bottom: 1px solid var(--color-divider);
}

.daily-row:last-child,
.book-stat-row:last-child {
  border-bottom: 0;
}

.daily-date {
  display: flex;
  flex-direction: column;
  gap: 2px;
}

.daily-date strong {
  font-size: 12px;
}

.daily-date span,
.daily-value {
  color: var(--color-text-tertiary);
  font-size: 11px;
}

.daily-bar {
  height: 5px;
  overflow: hidden;
  border-radius: var(--radius-full);
  background: var(--color-bg-sunken);
}

.daily-bar span {
  display: block;
  height: 100%;
  border-radius: inherit;
  background: var(--color-primary);
}

.daily-value {
  color: var(--color-text-secondary);
  text-align: right;
  white-space: nowrap;
}

.book-stat-row {
  display: grid;
  grid-template-columns: 26px minmax(0, 1fr) auto;
  align-items: center;
  gap: 10px;
  min-height: 66px;
  padding: 8px 0;
  border-bottom: 1px solid var(--color-divider);
}

.book-rank {
  width: 22px;
  height: 22px;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  border-radius: var(--radius-md);
  background: var(--color-bg-sunken);
  color: var(--color-text-tertiary);
  font-size: 11px;
}

.book-stat-row:first-child .book-rank {
  color: var(--color-primary);
  background: var(--color-primary-bg);
}

.book-stat-main {
  min-width: 0;
}

.book-stat-meta {
  display: flex;
  align-items: baseline;
  justify-content: space-between;
  gap: 8px;
}

.book-stat-meta strong {
  overflow: hidden;
  color: var(--color-text);
  font-size: 13px;
  font-weight: 600;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.book-stat-meta small {
  flex-shrink: 0;
  color: var(--color-text-tertiary);
  font-size: 10px;
}

.book-progress {
  height: 4px;
  margin-top: 8px;
  overflow: hidden;
  border-radius: var(--radius-full);
  background: var(--color-bg-sunken);
}

.book-progress span {
  display: block;
  height: 100%;
  border-radius: inherit;
  background: var(--color-primary);
}

.book-stat-value {
  min-width: 72px;
  display: flex;
  flex-direction: column;
  align-items: flex-end;
  gap: 2px;
}

.book-stat-value strong {
  color: var(--color-text);
  font-size: 12px;
  font-variant-numeric: tabular-nums;
  white-space: nowrap;
}

.book-stat-value small {
  color: var(--color-text-tertiary);
  font-size: 10px;
}

.empty-state {
  padding: 34px 12px;
  color: var(--color-text-tertiary);
  font-size: 12px;
  text-align: center;
}

@media (max-width: 900px) {
  .overview-grid {
    grid-template-columns: repeat(3, minmax(0, 1fr));
  }

  .overview-main {
    grid-column: 1 / -1;
  }

  .detail-grid {
    grid-template-columns: 1fr;
  }
}

@media (max-width: 640px) {
  .stats-header {
    align-items: flex-start;
    flex-direction: column;
  }

  .overview-grid {
    grid-template-columns: 1fr;
  }

  .overview-main {
    grid-column: auto;
  }

  .stats-content {
    padding-inline: var(--space-4);
  }

  .daily-row {
    grid-template-columns: 58px minmax(0, 1fr) 74px;
  }
}
</style>
