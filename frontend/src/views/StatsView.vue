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

      <div class="ticket-card">
        <h3 class="chart-title">阅读书票</h3>
        <div ref="ticketRef" class="ticket">
          <div class="ticket-inner">
            <div class="ticket-title">我的阅读书票</div>
            <div class="ticket-stats">
              <div class="ticket-stat">
                <span class="ticket-num">{{ formatDuration(summary.totalSeconds) }}</span>
                <small>累计阅读</small>
              </div>
              <div class="ticket-stat">
                <span class="ticket-num">{{ formatNumber(summary.totalCharacters) }}</span>
                <small>阅读字数</small>
              </div>
              <div class="ticket-stat">
                <span class="ticket-num">{{ summary.activeDays }}</span>
                <small>活跃天数</small>
              </div>
            </div>
            <div class="ticket-footer">阅读 · 让世界更辽阔</div>
          </div>
        </div>
        <button class="share-btn" @click="shareTicket">保存书票图片</button>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { onBeforeUnmount, onMounted, ref } from 'vue'
import * as echarts from 'echarts'
import { getReadingStatsDaily, getReadingStatsSummary, type ReadingStatsSummary } from '../api/readingStats'

const ranges = [
  { days: 7, label: '近7天' },
  { days: 30, label: '近30天' },
  { days: 90, label: '近90天' },
]

const activeRange = ref(30)
const chartRef = ref<HTMLElement | null>(null)
const ticketRef = ref<HTMLElement | null>(null)
const summary = ref<ReadingStatsSummary>({ totalSeconds: 0, totalCharacters: 0, activeDays: 0 })
let chart: echarts.ECharts | null = null

function formatDuration(seconds: number) {
  const mins = Math.floor(seconds / 60)
  const hours = Math.floor(mins / 60)
  const remMins = mins % 60
  if (hours > 0) return `${hours}小时${remMins}分`
  return `${Math.max(1, mins)}分钟`
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
  const [daily, sum] = await Promise.all([
    getReadingStatsDaily(start, end),
    getReadingStatsSummary(),
  ])
  summary.value = sum
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

  chart.setOption({
    grid: { left: 8, right: 8, top: 20, bottom: 8, containLabel: true },
    tooltip: { trigger: 'axis' },
    xAxis: {
      type: 'category',
      data: dates,
      axisLabel: { fontSize: 10, color: '#999' },
      axisLine: { lineStyle: { color: '#e5e5e5' } },
    },
    yAxis: {
      type: 'value',
      axisLabel: { fontSize: 10, color: '#999' },
      splitLine: { lineStyle: { color: '#f0f0f0' } },
    },
    series: [{
      type: 'bar',
      data: minutes,
      itemStyle: { color: '#179a57', borderRadius: [3, 3, 0, 0] },
      barMaxWidth: 18,
    }],
  })
}

function setRange(days: number) {
  activeRange.value = days
  void loadStats()
}

function shareTicket() {
  const el = ticketRef.value
  if (!el) return
  // Render the ticket to a canvas and download as PNG.
  const canvas = document.createElement('canvas')
  const scale = 2
  canvas.width = el.offsetWidth * scale
  canvas.height = el.offsetHeight * scale
  const ctx = canvas.getContext('2d')
  if (!ctx) return
  ctx.scale(scale, scale)
  ctx.fillStyle = '#fff'
  ctx.fillRect(0, 0, el.offsetWidth, el.offsetHeight)
  // Simple text-based ticket (kept dependency-free).
  ctx.fillStyle = '#179a57'
  ctx.font = 'bold 20px sans-serif'
  ctx.fillText('我的阅读书票', 20, 40)
  ctx.fillStyle = '#333'
  ctx.font = '14px sans-serif'
  ctx.fillText(`累计阅读：${formatDuration(summary.value.totalSeconds)}`, 20, 80)
  ctx.fillText(`阅读字数：${formatNumber(summary.value.totalCharacters)}`, 20, 110)
  ctx.fillText(`活跃天数：${summary.value.activeDays}`, 20, 140)
  ctx.fillStyle = '#999'
  ctx.font = '12px sans-serif'
  ctx.fillText('阅读 · 让世界更辽阔', 20, 180)

  const a = document.createElement('a')
  a.download = `阅读书票-${dateStr(0)}.png`
  a.href = canvas.toDataURL('image/png')
  a.click()
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
  letter-spacing: -0.02em;
}

.stats-range {
  display: flex;
  gap: 8px;
}

.range-chip {
  padding: 8px 14px;
  border-radius: 999px;
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
  border-radius: 16px;
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

.chart-card,
.ticket-card {
  border: 1px solid var(--color-border-light);
  border-radius: 16px;
  background: var(--color-bg-elevated);
  padding: 16px;
  margin-bottom: var(--space-4);
  flex-shrink: 0;
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

.ticket {
  border-radius: 12px;
  background: linear-gradient(135deg, #179a57, #0e6b3c);
  color: #fff;
  padding: 20px;
}

.ticket-inner {
  display: flex;
  flex-direction: column;
  gap: 16px;
}

.ticket-title {
  font-size: 18px;
  font-weight: 700;
}

.ticket-stats {
  display: flex;
  gap: 24px;
}

.ticket-stat {
  display: flex;
  flex-direction: column;
  gap: 4px;
}

.ticket-num {
  font-size: 20px;
  font-weight: 700;
}

.ticket-stat small {
  font-size: 12px;
  opacity: 0.85;
}

.ticket-footer {
  font-size: 12px;
  opacity: 0.8;
}

.share-btn {
  margin-top: 12px;
  padding: 10px 16px;
  border-radius: 999px;
  border: 1px solid var(--color-border-light);
  background: var(--color-bg);
  color: var(--color-primary);
  font-size: var(--text-sm);
  cursor: pointer;
}

.share-btn:hover {
  background: var(--color-bg-hover);
}

@media (max-width: 640px) {
  .stats-header {
    align-items: flex-start;
    flex-direction: column;
  }

  .stats-cards {
    grid-template-columns: 1fr;
  }
}
</style>
