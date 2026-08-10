<template>
  <div class="stats-view">
    <div class="stats-content">
      <header class="stats-header">
        <h1 class="stats-title">阅读数据统计</h1>
        <div class="stats-range">
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

      <div class="stats-cards">
        <div class="stat-card">
          <span class="stat-value">{{ formatDuration(summary.totalSeconds) }}</span>
          <small>累计阅读时长</small>
        </div>
        <div class="stat-card">
          <span class="stat-value">{{ formatNumber(summary.totalCharacters) }}</span>
          <small>累计阅读字数</small>
        </div>
        <div class="stat-card">
          <span class="stat-value">{{ summary.activeDays }}</span>
          <small>活跃天数</small>
        </div>
      </div>

      <div class="chart-card">
        <h3 class="chart-title">每日阅读时长（分钟）</h3>
        <div ref="chartRef" class="chart"></div>
      </div>

      <section class="book-stats-card">
        <header class="book-stats-head">
          <div>
            <h2>分书阅读时长</h2>
            <p>{{ activeRangeLabel }}</p>
          </div>
          <span class="book-total">{{ bookStats.length }} 本</span>
        </header>

        <div v-if="bookStats.length" class="book-stats-list">
          <div v-for="(book, index) in bookStats" :key="book.bookUrl" class="book-stat-row">
            <span class="book-rank">{{ index + 1 }}</span>
            <div class="book-stat-main">
              <div class="book-stat-meta">
                <strong :title="book.bookName">{{ book.bookName }}</strong>
                <small>最近阅读 {{ formatBookDate(book.lastReadDate) }}</small>
              </div>
              <div class="book-progress" aria-hidden="true">
                <span :style="{ width: `${bookProgressWidth(book.seconds)}%` }"></span>
              </div>
            </div>
            <div class="book-stat-value">
              <strong>{{ formatDuration(book.seconds) }}</strong>
              <small>占本期 {{ bookShare(book.seconds) }}%</small>
            </div>
          </div>
        </div>
        <div v-else class="book-stats-empty">当前时间范围内暂无按书阅读记录</div>
      </section>
    </div>
  </div>
</template>

<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import * as echarts from 'echarts'
import {
  getReadingStatsByBook,
  getReadingStatsDaily,
  getReadingStatsSummary,
  type BookReadingStats,
  type ReadingStatsSummary,
} from '../api/readingStats'
import { useAppStore } from '../stores/app'

const ranges = [
  { days: 7, label: '近7天' },
  { days: 30, label: '近30天' },
  { days: 90, label: '近90天' },
]

const activeRange = ref(30)
const appStore = useAppStore()
const chartRef = ref<HTMLElement | null>(null)
const summary = ref<ReadingStatsSummary>({ totalSeconds: 0, totalCharacters: 0, activeDays: 0 })
const bookStats = ref<BookReadingStats[]>([])
let chart: echarts.ECharts | null = null

const activeRangeLabel = computed(() => ranges.find((range) => range.days === activeRange.value)?.label || '')
const maxBookSeconds = computed(() => Math.max(0, ...bookStats.value.map((book) => book.seconds)))
const trackedBookSeconds = computed(() => bookStats.value.reduce((total, book) => total + book.seconds, 0))

function formatDuration(seconds: number) {
  if (seconds <= 0) return '0分钟'
  if (seconds < 60) return '少于1分钟'
  const mins = Math.floor(seconds / 60)
  const hours = Math.floor(mins / 60)
  const remMins = mins % 60
  if (hours > 0) return `${hours}小时${remMins}分`
  return `${Math.max(1, mins)}分钟`
}

function formatBookDate(value: string) {
  const parts = value.split('-')
  return parts.length === 3 ? `${parts[1]}/${parts[2]}` : value
}

function bookProgressWidth(seconds: number) {
  if (!maxBookSeconds.value) return 0
  return Math.max(4, Math.round((seconds / maxBookSeconds.value) * 100))
}

function bookShare(seconds: number) {
  if (!trackedBookSeconds.value) return 0
  return Math.round((seconds / trackedBookSeconds.value) * 100)
}

function formatNumber(n: number) {
  if (n >= 10000) return `${(n / 10000).toFixed(1)}万`
  return String(n)
}

function dateStr(offsetDays: number) {
  const d = new Date()
  d.setDate(d.getDate() - offsetDays)
  return d.toISOString().slice(0, 10)
}

async function loadStats() {
  const end = dateStr(0)
  const start = dateStr(activeRange.value - 1)
  const [daily, sum, books] = await Promise.all([
    getReadingStatsDaily(start, end),
    getReadingStatsSummary(),
    getReadingStatsByBook(start, end),
  ])
  summary.value = sum
  bookStats.value = books
  renderChart(daily, start, end)
}

function renderChart(daily: { date: string; seconds: number }[], start: string, end: string) {
  if (!chartRef.value) return
  if (!chart) chart = echarts.init(chartRef.value)

  // Fill missing days with 0 so the axis is continuous.
  const byDate = new Map(daily.map((d) => [d.date, d]))
  const dates: string[] = []
  const minutes: number[] = []
  const cur = new Date(start)
  const last = new Date(end)
  while (cur <= last) {
    const key = cur.toISOString().slice(0, 10)
    dates.push(key.slice(5))
    minutes.push(Math.round((byDate.get(key)?.seconds || 0) / 60))
    cur.setDate(cur.getDate() + 1)
  }

  const styles = getComputedStyle(document.documentElement)
  const primary = styles.getPropertyValue('--color-primary').trim()
  const textTertiary = styles.getPropertyValue('--color-text-tertiary').trim()
  const divider = styles.getPropertyValue('--color-divider').trim()

  chart.setOption({
    grid: { left: 8, right: 8, top: 20, bottom: 8, containLabel: true },
    tooltip: { trigger: 'axis' },
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
  window.addEventListener('resize', () => chart?.resize())
})

onBeforeUnmount(() => {
  window.removeEventListener('resize', () => chart?.resize())
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
  padding: var(--space-6) 0 var(--space-3);
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: var(--space-4);
  flex-shrink: 0;
}

.stats-title {
  font-size: var(--text-2xl);
  font-weight: 700;
  letter-spacing: 0;
}

.stats-range {
  display: flex;
  gap: 8px;
}

.range-chip {
  padding: 8px 14px;
  border-radius: var(--radius-full);
  border: 1px solid var(--color-border-light);
  background: var(--color-bg-elevated);
  color: var(--color-text-secondary);
  font-size: var(--text-sm);
  transition: all var(--duration-fast) var(--ease-out);
}

.range-chip.active {
  border-color: rgba(201, 127, 58, 0.26);
  background: rgba(201, 127, 58, 0.1);
  color: var(--color-primary);
}

.stats-cards {
  display: grid;
  grid-template-columns: repeat(3, 1fr);
  gap: 12px;
  padding-bottom: var(--space-4);
  flex-shrink: 0;
}

.stat-card {
  border: 1px solid var(--color-border-light);
  border-radius: var(--radius-lg);
  background: var(--color-bg-elevated);
  padding: 16px;
  display: flex;
  flex-direction: column;
  gap: 6px;
}

.stat-value {
  font-size: 22px;
  font-weight: 700;
  color: var(--color-primary);
}

.stat-card small {
  color: var(--color-text-tertiary);
  font-size: 12px;
}

.chart-card {
  border: 1px solid var(--color-border-light);
  border-radius: var(--radius-lg);
  background: var(--color-bg-elevated);
  padding: 16px;
  margin-bottom: var(--space-4);
  flex-shrink: 0;
}

.book-stats-card {
  border: 1px solid var(--color-border-light);
  border-radius: var(--radius-lg);
  background: var(--color-bg-elevated);
  margin-bottom: var(--space-4);
  flex-shrink: 0;
  overflow: hidden;
}

.book-stats-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: var(--space-4);
  padding: 16px;
  border-bottom: 1px solid var(--color-divider);
}

.book-stats-head h2 {
  margin: 0;
  font-size: 15px;
  font-weight: 650;
}

.book-stats-head p {
  margin: 3px 0 0;
  color: var(--color-text-tertiary);
  font-size: 12px;
}

.book-total {
  flex-shrink: 0;
  padding: 4px 9px;
  border-radius: var(--radius-md);
  background: var(--color-primary-bg);
  color: var(--color-primary);
  font-size: 12px;
  font-weight: 600;
}

.book-stats-list {
  display: flex;
  flex-direction: column;
}

.book-stat-row {
  display: grid;
  grid-template-columns: 28px minmax(0, 1fr) auto;
  align-items: center;
  gap: 12px;
  min-height: 72px;
  padding: 12px 16px;
  border-bottom: 1px solid var(--color-divider);
}

.book-stat-row:last-child {
  border-bottom: none;
}

.book-rank {
  width: 24px;
  height: 24px;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  border-radius: var(--radius-md);
  background: var(--color-bg-sunken);
  color: var(--color-text-tertiary);
  font-size: 12px;
  font-variant-numeric: tabular-nums;
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
  gap: 12px;
  min-width: 0;
}

.book-stat-meta strong {
  overflow: hidden;
  color: var(--color-text);
  font-size: 14px;
  font-weight: 600;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.book-stat-meta small {
  flex-shrink: 0;
  color: var(--color-text-tertiary);
  font-size: 11px;
}

.book-progress {
  height: 4px;
  margin-top: 9px;
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
  min-width: 86px;
  display: flex;
  flex-direction: column;
  align-items: flex-end;
  gap: 2px;
}

.book-stat-value strong {
  color: var(--color-text);
  font-size: 14px;
  font-variant-numeric: tabular-nums;
}

.book-stat-value small {
  color: var(--color-text-tertiary);
  font-size: 11px;
}

.book-stats-empty {
  padding: 44px 20px;
  color: var(--color-text-tertiary);
  font-size: 13px;
  text-align: center;
}

.chart-title {
  margin: 0 0 12px;
  font-size: 14px;
  font-weight: 600;
  color: var(--color-text-secondary);
}

.chart {
  height: 220px;
  width: 100%;
}

@media (max-width: 640px) {
  .stats-header {
    align-items: flex-start;
    flex-direction: column;
  }

  .stats-cards {
    grid-template-columns: 1fr;
  }

  .book-stat-row {
    grid-template-columns: 24px minmax(0, 1fr) auto;
    gap: 9px;
    padding: 12px;
  }

  .book-stat-meta {
    align-items: flex-start;
    flex-direction: column;
    gap: 2px;
  }

  .book-stat-value {
    min-width: 74px;
  }
}
</style>
