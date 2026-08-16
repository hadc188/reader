<template>
  <div class="debugger">
    <div class="debug-toolbar">
      <div class="step-tabs">
        <button
          v-for="step in steps"
          :key="step.key"
          class="step-btn"
          :class="{ active: activeStep === step.key, loading: running && activeStep === step.key }"
          type="button"
          :disabled="running"
          @click="selectStep(step.key)"
        >
          {{ step.label }}
        </button>
      </div>

      <div class="input-row">
        <input
          :value="inputValue"
          class="debug-input"
          :placeholder="inputPlaceholder"
          spellcheck="false"
          @input="inputValue = ($event.target as HTMLInputElement).value"
          @keydown.enter="run"
        />
        <button class="mini-btn primary" type="button" :disabled="running" @click="run">
          {{ running ? '调试中...' : '运行' }}
        </button>
      </div>
    </div>

    <div v-if="error" class="debug-error">{{ error }}</div>

    <template v-if="trace">
      <div class="trace-meta">
        <span class="status-badge" :class="{ error: trace.status >= 400 }">HTTP {{ trace.status }}</span>
        <span class="trace-url">{{ trace.requestUrl }}</span>
      </div>

      <div v-if="trace.warnings?.length" class="trace-warnings">
        <div v-for="(w, i) in trace.warnings" :key="i" class="trace-warning">
          <span class="warn-flag">⚠</span>
          <span>{{ w }}</span>
        </div>
      </div>

      <div v-if="trace.headers?.length" class="trace-headers">
        <button class="headers-toggle" type="button" @click="headersOpen = !headersOpen">
          <span class="caret" :class="{ open: headersOpen }">▸</span>
          响应头（{{ trace.headers.length }}）
        </button>
        <div v-if="headersOpen" class="headers-list">
          <div v-for="([k, v], i) in trace.headers" :key="i" class="header-row">
            <span class="header-key">{{ k }}</span>
            <span class="header-val">{{ v }}</span>
          </div>
        </div>
      </div>

      <div class="trace-grid">
        <div class="trace-col">
          <h4>原始响应 <small>（截断 {{ bodySizeKB }} KB）</small></h4>
          <pre class="trace-body">{{ trace.body }}</pre>
        </div>
        <div class="trace-col">
          <h4>解析结果</h4>
          <pre class="trace-result">{{ prettyResult }}</pre>
        </div>
      </div>
    </template>

    <div v-else-if="!running && !error" class="debug-empty">
      选择步骤并输入参数，查看该步的原始响应与解析结果。
    </div>
  </div>
</template>

<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { useAppStore } from '../../stores/app'
import { debugSourceStep, type DebugStep } from '../../api/debug'
import type { BookSource, DebugTrace } from '../../types'

const props = defineProps<{
  source: BookSource | null
  editorText: string
}>()

const appStore = useAppStore()

const steps: { key: DebugStep; label: string }[] = [
  { key: 'search', label: '搜索' },
  { key: 'bookInfo', label: '详情' },
  { key: 'toc', label: '目录' },
  { key: 'content', label: '正文' },
]

const activeStep = ref<DebugStep>('search')
const inputValue = ref('')
const running = ref(false)
const error = ref('')
const trace = ref<DebugTrace | null>(null)
const headersOpen = ref(false)

const inputPlaceholder = computed(() => {
  switch (activeStep.value) {
    case 'search':
      return '搜索关键词，留空用规则 checkKeyWord'
    case 'bookInfo':
      return '书籍链接（bookUrl）或目录链接'
    case 'toc':
      return '目录链接（tocUrl）'
    case 'content':
      return '章节链接（chapterUrl）'
  }
})

const bodySizeKB = computed(() => (trace.value?.body.length || 0) / 1024)

const prettyResult = computed(() => {
  if (!trace.value) return ''
  try {
    return JSON.stringify(trace.value.result, null, 2)
  } catch {
    return String(trace.value.result)
  }
})

watch(
  () => [activeStep.value, props.source?.bookSourceUrl],
  () => {
    inputValue.value = ''
    error.value = ''
    trace.value = null
  },
)

function selectStep(step: DebugStep) {
  activeStep.value = step
}

function currentSourceUrl(): string | null {
  if (props.source?.bookSourceUrl?.trim()) return props.source.bookSourceUrl
  try {
    const parsed = JSON.parse(props.editorText) as BookSource
    return parsed.bookSourceUrl?.trim() || null
  } catch {
    return null
  }
}

async function run() {
  const bookSourceUrl = currentSourceUrl()
  if (!bookSourceUrl) {
    appStore.showToast('请先填写并保存书源 URL', 'warning')
    return
  }
  if (activeStep.value !== 'search' && !inputValue.value.trim()) {
    appStore.showToast(`请输入${inputPlaceholder.value}`, 'warning')
    return
  }

  running.value = true
  error.value = ''
  trace.value = null
  headersOpen.value = false
  try {
    trace.value = await debugSourceStep({
      bookSourceUrl,
      step: activeStep.value,
      keyword: activeStep.value === 'search' ? inputValue.value : undefined,
      bookUrl: activeStep.value === 'bookInfo' || activeStep.value === 'toc' ? inputValue.value : undefined,
      chapterUrl: activeStep.value === 'content' ? inputValue.value : undefined,
    })
  } catch (e: unknown) {
    error.value = (e as Error).message || '调试失败'
  } finally {
    running.value = false
  }
}
</script>

<style scoped>
.debugger {
  display: flex;
  flex-direction: column;
  gap: 10px;
  min-height: 0;
  flex: 1;
}

.debug-toolbar {
  display: flex;
  flex-direction: column;
  gap: 8px;
}

.step-tabs {
  display: flex;
  gap: 6px;
  flex-wrap: wrap;
}

.step-btn {
  min-height: 30px;
  padding: 0 12px;
  border-radius: var(--radius-full);
  background: transparent;
  color: var(--color-text-secondary);
  font-size: 13px;
  border: 1px solid var(--color-border);
  transition: all var(--duration-fast);
}

.step-btn.active {
  background: var(--color-primary);
  border-color: var(--color-primary);
  color: #fff;
}

.step-btn:disabled {
  cursor: not-allowed;
  opacity: 0.6;
}

.input-row {
  display: flex;
  gap: 8px;
}

.debug-input {
  flex: 1;
  min-height: 34px;
  padding: 0 12px;
  border: 1px solid var(--color-border);
  border-radius: var(--radius-md);
  background: var(--color-bg);
  color: var(--color-text);
  font-size: 13px;
  outline: none;
}

.mini-btn {
  min-height: 32px;
  padding: 0 12px;
  border: 1px solid var(--color-border);
  border-radius: var(--radius-md);
  background: transparent;
  font-size: 12px;
  white-space: nowrap;
}

.mini-btn.primary {
  background: var(--color-primary);
  border-color: var(--color-primary);
  color: #fff;
}

.mini-btn:disabled {
  cursor: not-allowed;
  opacity: 0.48;
}

.debug-error {
  padding: 10px 12px;
  border-radius: var(--radius-md);
  background: rgba(245, 34, 45, 0.08);
  color: var(--color-danger);
  font-size: 13px;
}

.trace-meta {
  display: flex;
  align-items: center;
  gap: 10px;
  font-size: 12px;
  color: var(--color-text-tertiary);
  word-break: break-all;
}

.status-badge {
  padding: 2px 8px;
  border-radius: var(--radius-full);
  background: var(--color-primary-bg);
  color: var(--color-primary-dark);
  flex-shrink: 0;
}

.status-badge.error {
  background: rgba(245, 34, 45, 0.12);
  color: var(--color-danger);
}

.trace-warnings {
  display: flex;
  flex-direction: column;
  gap: 6px;
}

.trace-warning {
  display: flex;
  align-items: flex-start;
  gap: 8px;
  padding: 8px 10px;
  border-radius: var(--radius-md);
  background: rgba(250, 173, 20, 0.1);
  border: 1px solid rgba(250, 173, 20, 0.28);
  color: var(--color-warning, #d48806);
  font-size: 12px;
  line-height: 1.5;
  word-break: break-all;
}

.warn-flag {
  flex-shrink: 0;
}

.trace-headers {
  display: flex;
  flex-direction: column;
  gap: 4px;
}

.headers-toggle {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  align-self: flex-start;
  padding: 4px 8px;
  border: none;
  background: transparent;
  color: var(--color-text-secondary);
  font-size: 12px;
  cursor: pointer;
}

.headers-toggle:hover {
  color: var(--color-primary);
}

.caret {
  display: inline-block;
  transition: transform var(--duration-fast);
}

.caret.open {
  transform: rotate(90deg);
}

.headers-list {
  display: flex;
  flex-direction: column;
  border: 1px solid var(--color-border-light);
  border-radius: var(--radius-md);
  overflow: hidden;
}

.header-row {
  display: flex;
  gap: 10px;
  padding: 5px 10px;
  font-family: ui-monospace, SFMono-Regular, Menlo, Consolas, monospace;
  font-size: 11px;
  line-height: 1.5;
}

.header-row:nth-child(odd) {
  background: var(--color-bg);
}

.header-key {
  flex-shrink: 0;
  min-width: 140px;
  color: var(--color-text-secondary);
  font-weight: 600;
}

.header-val {
  color: var(--color-text);
  word-break: break-all;
}

.trace-grid {
  display: grid;
  grid-template-columns: 1fr 1fr;
  gap: 10px;
  min-height: 0;
  flex: 1;
}

.trace-col {
  display: flex;
  flex-direction: column;
  gap: 6px;
  min-height: 0;
}

.trace-col h4 {
  margin: 0;
  font-size: 12px;
  color: var(--color-text-secondary);
}

.trace-col small {
  color: var(--color-text-tertiary);
}

.trace-body,
.trace-result {
  flex: 1;
  min-height: 0;
  overflow: auto;
  margin: 0;
  padding: 10px;
  border: 1px solid var(--color-border-light);
  border-radius: var(--radius-md);
  background: var(--color-bg);
  font-family: ui-monospace, SFMono-Regular, Menlo, Consolas, monospace;
  font-size: 11px;
  line-height: 1.5;
  color: var(--color-text);
  white-space: pre-wrap;
  word-break: break-all;
}

.trace-result {
  color: var(--color-primary-dark);
}

.debug-empty {
  display: flex;
  align-items: center;
  justify-content: center;
  flex: 1;
  color: var(--color-text-tertiary);
  font-size: 13px;
  text-align: center;
  padding: 24px;
}

@media (max-width: 760px) {
  .trace-grid {
    grid-template-columns: 1fr;
  }
}
</style>
