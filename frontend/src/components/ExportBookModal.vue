<template>
  <Teleport to="body">
    <Transition name="fade">
      <div v-if="modelValue" class="modal-overlay"></div>
    </Transition>
    <Transition name="scale">
      <div v-if="modelValue" class="modal-container" @click.self="close">
        <section class="export-modal" :class="{ exporting }" aria-labelledby="export-title">
          <header class="modal-head">
            <div>
              <p class="eyebrow">本地备份</p>
              <h2 id="export-title">导出书籍</h2>
            </div>
            <button v-if="!exporting" class="icon-btn" aria-label="关闭导出" @click="close">
              <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                <path d="M18 6 6 18M6 6l12 12" />
              </svg>
            </button>
          </header>

          <div v-if="!exporting" class="layout">
            <aside class="book-pane" aria-label="选择书籍">
              <div class="pane-head">
                <span>书架</span>
                <strong>{{ selectedBooks.length }}/{{ books.length }}</strong>
              </div>
              <div class="book-list">
                <label
                  v-for="book in books"
                  :key="book.bookUrl"
                  class="book-item"
                  :class="{ selected: selectedUrls.has(book.bookUrl) }"
                >
                  <input type="checkbox" :checked="selectedUrls.has(book.bookUrl)" @change="toggle(book.bookUrl)" />
                  <span class="book-name">{{ book.name }}</span>
                  <span class="book-author">{{ book.author || '未知作者' }}</span>
                </label>
                <div v-if="books.length === 0" class="empty-hint">书架还没有书籍</div>
              </div>
              <div class="pane-actions">
                <button type="button" @click="selectAll">全选</button>
                <button type="button" :disabled="selectedBooks.length === 0" @click="clearSelection">清空</button>
              </div>
            </aside>

            <div class="settings-pane">
              <section class="settings-panel">
                <h3>导出格式</h3>
                <div class="format-options">
                  <button type="button" :class="{ active: format === 'txt' }" @click="format = 'txt'">
                    <strong>TXT</strong>
                    <span>通用纯文本</span>
                  </button>
                  <button type="button" :class="{ active: format === 'epub' }" @click="format = 'epub'">
                    <strong>EPUB</strong>
                    <span>保留目录</span>
                  </button>
                </div>
              </section>

              <section class="settings-panel wide">
                <div class="panel-head">
                  <h3>章节范围</h3>
                  <button type="button" class="link-btn" :disabled="selectedBooks.length !== 1" @click="useCurrentProgress">
                    从当前进度起
                  </button>
                </div>
                <p class="panel-desc">每本书可单独设置；留空表示导出全部章节。</p>
                <div v-if="selectedBooks.length === 0" class="chapter-empty">先选择要导出的书</div>
                <div v-else class="range-list">
                  <div v-for="book in selectedBooks" :key="book.bookUrl" class="range-row">
                    <span class="range-book">{{ book.name }}</span>
                    <input v-model="chapterRanges[book.bookUrl]" type="text" placeholder="全部章节" spellcheck="false" autocomplete="off" />
                  </div>
                </div>
                <p v-if="rangeError" class="range-error">{{ rangeError }}</p>
                <p class="range-help">例：1-50,80 表示导出第 1 到 50 章和第 80 章。</p>
              </section>
            </div>
          </div>

          <div v-else class="progress-body">
            <div class="progress-title">
              <div>
                <p>正在导出</p>
                <h3>{{ currentBookName || selectedBooks[0]?.name }}</h3>
              </div>
              <strong>{{ overallPercent }}%</strong>
            </div>
            <div class="progress-track">
              <div class="progress-fill" :style="{ width: `${overallPercent}%` }"></div>
            </div>
            <div class="progress-meta">
              <span v-if="chapterTotal > 0">{{ doneCount }}/{{ chapterTotal }} 章</span>
              <span v-if="chapterTitle">{{ chapterTitle }}</span>
            </div>
            <div v-if="doneItems.length" class="result-list">
              <div v-for="item in doneItems" :key="item.path" class="done-item">
                <strong>{{ item.name }}</strong>
                <span>{{ item.path }}</span>
              </div>
            </div>
            <div v-if="failedItems.length" class="result-list failed">
              <div v-for="item in failedItems" :key="item.name" class="done-item">
                <strong>{{ item.name }}</strong>
                <span>{{ item.error }}</span>
              </div>
            </div>
          </div>

          <footer class="modal-foot">
            <template v-if="!exporting">
              <span class="foot-summary">{{ exportSummary }}</span>
              <button class="ghost-btn" @click="close">关闭</button>
              <button class="primary-btn" :disabled="!canExport" @click="startExport">开始导出</button>
            </template>
            <template v-else>
              <span class="foot-summary">{{ doneItems.length + failedItems.length }}/{{ exportBookTotal }} 本完成</span>
              <button class="ghost-btn danger" @click="cancelExport">取消导出</button>
            </template>
          </footer>
        </section>
      </div>
    </Transition>
  </Teleport>
</template>

<script setup lang="ts">
import { computed, reactive, ref, watch } from 'vue'
import { useBookshelfStore } from '../stores/bookshelf'
import { useAppStore } from '../stores/app'
import { openSse, type SseLike } from '../api/sse'
import type { Book } from '../types'

const props = defineProps<{ modelValue: boolean }>()
const emit = defineEmits<{ (e: 'update:modelValue', v: boolean): void }>()
const shelfStore = useBookshelfStore()
const appStore = useAppStore()
const books = computed<Book[]>(() => shelfStore.books)
const selectedUrls = ref<Set<string>>(new Set())
const chapterRanges = reactive<Record<string, string>>({})
const format = ref<'txt' | 'epub'>('txt')
const rangeError = ref('')
const exporting = ref(false)
const currentBookName = ref('')
const doneCount = ref(0)
const chapterTotal = ref(0)
const exportBookTotal = ref(0)
const chapterTitle = ref('')
const doneItems = ref<{ name: string; path: string }[]>([])
const failedItems = ref<{ name: string; error: string }[]>([])
let stream: SseLike | null = null

const selectedBooks = computed(() => books.value.filter((book) => selectedUrls.value.has(book.bookUrl)))
const validRanges = computed(() => {
  const result: Record<string, string> = {}
  for (const book of selectedBooks.value) {
    const value = (chapterRanges[book.bookUrl] || '').trim()
    if (!value) continue
    if (!isValidChapterRange(value)) return null
    result[book.bookUrl] = value
  }
  return result
})
const canExport = computed(() => selectedBooks.value.length > 0 && validRanges.value !== null)
const selectedChapterCount = computed(() => selectedBooks.value.reduce((sum, book) => (
  sum + Number(countSelectedChapters((chapterRanges[book.bookUrl] || '').trim()) || 0)
), 0))
const exportSummary = computed(() => {
  if (selectedBooks.value.length === 0) return '未选择书籍'
  if (validRanges.value === null) return '章节范围有误'
  return `${selectedBooks.value.length} 本书 · 约 ${selectedChapterCount.value} 章`
})
const overallPercent = computed(() => {
  const total = exportBookTotal.value || selectedBooks.value.length || 1
  const finished = doneItems.value.length + failedItems.value.length
  const chapterPart = chapterTotal.value > 0 ? doneCount.value / chapterTotal.value : 0
  return Math.min(100, Math.round(((finished + chapterPart) / total) * 100))
})

function syncPreset() {
  selectedUrls.value = new Set(
    shelfStore.editMode && shelfStore.selectedBookUrls.size > 0 ? shelfStore.selectedBookUrls : [],
  )
  for (const key of Object.keys(chapterRanges)) {
    if (!selectedUrls.value.has(key)) delete chapterRanges[key]
  }
}

function toggle(url: string) {
  const next = new Set(selectedUrls.value)
  if (next.has(url)) {
    next.delete(url)
    delete chapterRanges[url]
  } else {
    next.add(url)
  }
  selectedUrls.value = next
}

function selectAll() {
  selectedUrls.value = new Set(books.value.map((book) => book.bookUrl))
}

function clearSelection() {
  selectedUrls.value = new Set()
  for (const key of Object.keys(chapterRanges)) delete chapterRanges[key]
}

function useCurrentProgress() {
  const book = selectedBooks.value[0]
  if (!book) return
  chapterRanges[book.bookUrl] = `${Math.max(1, (book.durChapterIndex ?? 0) + 1)}-`
}

function isValidChapterRange(value: string) {
  return value.split(/[,，\s]+/).filter(Boolean).every((part) => /^\d+([-~]\d*)?$/.test(part))
}

function countSelectedChapters(value: string) {
  if (!value) return '全部'
  let count = 0
  let hasOpenRange = false
  for (const part of value.split(/[,，\s]+/).filter(Boolean)) {
    if (!/^\d+([-~]\d*)?$/.test(part)) continue
    if (part.includes('-') || part.includes('~')) {
      const [start, end] = part.split(/[-~]/) as [string, string | undefined]
      if (!end) hasOpenRange = true
      else count += Math.max(0, Number(end) - Number(start) + 1)
    } else {
      count += 1
    }
  }
  return hasOpenRange && count === 0 ? '剩余全部' : count
}

function close() {
  if (exporting.value) return
  emit('update:modelValue', false)
}

function startExport() {
  const ranges = validRanges.value
  if (!ranges) return
  rangeError.value = ''
  doneItems.value = []
  failedItems.value = []
  chapterTotal.value = 0
  doneCount.value = 0
  chapterTitle.value = ''
  currentBookName.value = selectedBooks.value[0]?.name ?? ''
  exportBookTotal.value = selectedBooks.value.length
  exporting.value = true
  stream = openSse('export_books_sse', {
    books: selectedBooks.value,
    format: format.value,
    chapterRanges: ranges,
  })
  stream.onmessage = (event) => {
    const payload = event.data as {
      event?: string
      bookName?: string
      done?: number
      total?: number
      chapterTitle?: string
      path?: string
      error?: string
    }
    if (payload.event === 'progress') {
      currentBookName.value = payload.bookName ?? currentBookName.value
      doneCount.value = payload.done ?? doneCount.value
      chapterTotal.value = payload.total ?? chapterTotal.value
      chapterTitle.value = payload.chapterTitle ?? ''
    } else if (payload.event === 'book_done') {
      doneItems.value.push({ name: payload.bookName ?? '', path: payload.path ?? '' })
      chapterTotal.value = 0
      doneCount.value = 0
      chapterTitle.value = ''
    } else if (payload.event === 'book_failed') {
      failedItems.value.push({ name: payload.bookName ?? '', error: payload.error ?? '导出失败' })
      chapterTotal.value = 0
      chapterTitle.value = ''
    }
  }
  stream.addEventListener('end', (event) => {
    const payload = event.data as { succeeded?: number; failed?: number; folder?: string; cancelled?: boolean }
    exporting.value = false
    stream?.close()
    stream = null
    if (payload.cancelled) {
      appStore.showToast('已取消导出', 'warning')
      return
    }
    const ok = payload.succeeded ?? 0
    const fail = payload.failed ?? 0
    if (fail === 0) appStore.showToast(`已导出 ${ok} 本到: ${payload.folder ?? ''}`, 'success')
    else appStore.showToast(`成功 ${ok} 本, 失败 ${fail} 本`, 'warning')
  })
  stream.onerror = (event) => {
    exporting.value = false
    stream?.close()
    stream = null
    const msg = typeof event === 'object' && event !== null
      ? String((event as { data?: unknown }).data ?? '导出失败')
      : '导出失败'
    rangeError.value = msg
    appStore.showToast(msg, 'error')
  }
}

function cancelExport() {
  stream?.close()
  stream = null
  exporting.value = false
  appStore.showToast('已取消导出', 'warning')
}

watch(() => props.modelValue, (visible) => {
  if (visible) syncPreset()
})
watch(chapterRanges, () => {
  rangeError.value = validRanges.value === null ? '章节范围格式不正确，请使用如 1-50,80' : ''
}, { deep: true })
</script>

<style scoped>
.modal-overlay { position: fixed; inset: 0; z-index: var(--z-overlay); background: rgba(24,28,32,.52); backdrop-filter: blur(5px); }
.modal-container { position: fixed; inset: 0; z-index: var(--z-modal); display: grid; place-items: center; padding: clamp(14px,3vw,32px); }
.export-modal { width: min(880px,100%); max-height: min(82vh,760px); display: flex; flex-direction: column; overflow: hidden; background: var(--color-bg-elevated); border: 1px solid var(--color-border); border-radius: var(--radius-xl); box-shadow: var(--shadow-xl); }
.export-modal.exporting { width: min(620px,100%); }
.modal-head { display: flex; align-items: flex-start; justify-content: space-between; padding: 24px 26px 18px; border-bottom: 1px solid var(--color-border); }
.eyebrow { margin: 0 0 5px; font-size: 11px; letter-spacing: .1em; color: var(--color-primary); }
.modal-head h2 { margin: 0; font-size: 24px; line-height: 1.2; letter-spacing: -.02em; }
.icon-btn { display: grid; place-items: center; width: 34px; height: 34px; border: none; border-radius: var(--radius-md); background: transparent; color: var(--color-text-tertiary); cursor: pointer; }
.icon-btn:hover { background: var(--color-bg-sunken); color: var(--color-text); }
.icon-btn svg { width: 19px; height: 19px; }
.layout { display: grid; grid-template-columns: minmax(240px,34%) minmax(0,1fr); min-height: 0; flex: 1; }
.book-pane { display: flex; flex-direction: column; min-height: 0; border-right: 1px solid var(--color-border); background: color-mix(in srgb, var(--color-bg-sunken) 58%, transparent); }
.pane-head, .panel-head, .progress-title { display: flex; align-items: center; justify-content: space-between; }
.pane-head { padding: 16px 18px 12px; font-size: 12px; color: var(--color-text-tertiary); }
.pane-head strong { font-variant-numeric: tabular-nums; color: var(--color-text); }
.book-list { flex: 1; overflow-y: auto; padding: 0 10px 10px 14px; }
.book-item { display: flex; align-items: center; gap: 9px; min-height: 40px; padding: 8px 10px; border-radius: var(--radius-md); cursor: pointer; }
.book-item:hover { background: var(--color-bg-elevated); }
.book-item.selected { background: var(--color-primary-bg); }
.book-item input { width: 15px; height: 15px; accent-color: var(--color-primary); }
.book-name { flex: 1; overflow: hidden; font-size: 14px; text-overflow: ellipsis; white-space: nowrap; }
.book-author { max-width: 30%; overflow: hidden; font-size: 11px; color: var(--color-text-tertiary); text-overflow: ellipsis; white-space: nowrap; }
.pane-actions { display: flex; gap: 8px; padding: 12px 14px; border-top: 1px solid var(--color-border); }
.pane-actions button { flex: 1; min-height: 32px; border: 1px solid var(--color-border); border-radius: var(--radius-md); background: transparent; color: var(--color-text-secondary); cursor: pointer; }
.pane-actions button:hover:not(:disabled) { border-color: var(--color-primary-border); color: var(--color-primary); }
.pane-actions button:disabled { opacity: .45; cursor: not-allowed; }
.empty-hint, .chapter-empty { display: grid; place-items: center; min-height: 120px; color: var(--color-text-tertiary); }
.settings-pane { min-height: 0; overflow-y: auto; padding: 18px; }
.settings-panel { padding: 18px; border: 1px solid var(--color-border); border-radius: var(--radius-lg); background: var(--color-bg-elevated); box-shadow: 0 10px 28px rgba(27,34,40,.05); }
.settings-panel.wide { margin-top: 14px; }
.settings-panel h3 { margin: 0; font-size: 15px; letter-spacing: -.01em; }
.format-options { display: grid; grid-template-columns: repeat(2,minmax(0,1fr)); gap: 10px; margin-top: 14px; }
.format-options button { display: flex; flex-direction: column; gap: 3px; padding: 12px; border: 1px solid var(--color-border); border-radius: var(--radius-md); background: transparent; cursor: pointer; text-align: left; }
.format-options button strong { font-size: 15px; }
.format-options button span, .panel-desc, .range-help { font-size: 12px; color: var(--color-text-tertiary); }
.format-options button.active { border-color: var(--color-primary-border); background: var(--color-primary-bg); box-shadow: inset 0 0 0 1px var(--color-primary-border); }
.panel-desc { margin: 6px 0 12px; }
.range-list { display: flex; flex-direction: column; gap: 8px; max-height: 260px; overflow-y: auto; }
.range-row { display: grid; grid-template-columns: minmax(90px,34%) minmax(0,1fr); align-items: center; gap: 10px; }
.range-book { overflow: hidden; font-size: 13px; text-overflow: ellipsis; white-space: nowrap; }
.range-row input { width: 100%; min-height: 34px; padding: 0 10px; border: 1px solid var(--color-border); border-radius: var(--radius-md); background: var(--color-bg); color: var(--color-text); font-variant-numeric: tabular-nums; }
.range-row input:focus { border-color: var(--color-primary); outline: none; }
.range-error { margin: 8px 0 0; font-size: 12px; color: #dc2626; }
.range-help { margin: 9px 0 0; }
.link-btn { border: none; background: transparent; color: var(--color-primary); cursor: pointer; font-size: 12px; }
.link-btn:disabled { color: var(--color-text-tertiary); cursor: not-allowed; }
.progress-body { flex: 1; padding: 26px; overflow-y: auto; }
.progress-title h3 { margin: 3px 0 0; font-size: 18px; }
.progress-title p { margin: 0; font-size: 12px; color: var(--color-text-tertiary); }
.progress-title strong { font-size: 24px; font-variant-numeric: tabular-nums; }
.progress-track { height: 9px; margin-top: 18px; overflow: hidden; border-radius: 999px; background: var(--color-bg-sunken); }
.progress-fill { height: 100%; border-radius: inherit; background: linear-gradient(90deg,var(--color-primary),var(--color-primary-light)); transition: width .2s ease; }
.progress-meta { display: flex; justify-content: space-between; gap: 12px; margin-top: 9px; font-size: 12px; color: var(--color-text-tertiary); }
.progress-meta span:last-child { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.result-list { margin-top: 18px; display: flex; flex-direction: column; gap: 8px; }
.done-item { display: flex; flex-direction: column; gap: 2px; padding: 10px 12px; border-radius: var(--radius-md); background: var(--color-bg-sunken); font-size: 12px; }
.done-item strong { font-size: 13px; }
.done-item span { color: var(--color-text-tertiary); word-break: break-all; }
.result-list.failed .done-item { background: rgba(220,38,38,.08); }
.result-list.failed .done-item span { color: #dc2626; }
.modal-foot { display: flex; align-items: center; justify-content: flex-end; gap: 10px; padding: 15px 20px; border-top: 1px solid var(--color-border); }
.foot-summary { margin-right: auto; font-size: 12px; color: var(--color-text-tertiary); }
.primary-btn, .ghost-btn { min-width: 96px; min-height: 38px; border-radius: var(--radius-md); cursor: pointer; font-size: 14px; }
.primary-btn { border: none; background: var(--color-primary); color: #fff; }
.primary-btn:hover:not(:disabled) { background: var(--color-primary-dark); }
.primary-btn:disabled { opacity: .48; cursor: not-allowed; }
.ghost-btn { border: 1px solid var(--color-border); background: transparent; color: var(--color-text-secondary); }
.ghost-btn:hover:not(:disabled) { border-color: var(--color-primary-border); color: var(--color-primary); }
.ghost-btn.danger:hover { border-color: #dc2626; color: #dc2626; }
@media (max-width: 760px) {
  .layout { grid-template-columns: 1fr; overflow-y: auto; }
  .book-pane { max-height: 260px; border-right: none; border-bottom: 1px solid var(--color-border); }
  .range-list { max-height: none; }
  .foot-summary { margin-right: 0; width: 100%; }
  .modal-foot { flex-wrap: wrap; }
}
</style>
