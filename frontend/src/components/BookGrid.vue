<template>
  <div class="book-grid" :class="{ 'touch-sorting': touchDragState.started }">
    <TransitionGroup name="card" tag="div" class="book-grid-list">
      <div
        v-for="item in displayItems"
        :key="getDisplayKey(item)"
        :ref="(el) => setItemRef(el, getDisplayKey(item))"
        class="book-grid-item"
        :data-book-url="isPlaceholder(item) ? '' : item.bookUrl"
        :class="{
          'drop-target': sortable && !isPlaceholder(item) && dropTargetUrl === item.bookUrl,
          'is-dragging': sortable && !isPlaceholder(item) && draggedUrl === item.bookUrl,
          'is-touch-gap': sortable && touchDragState.started && isPlaceholder(item),
          'is-touch-sibling': sortable && touchDragState.started && !isPlaceholder(item),
        }"
        :style="getItemStyle()"
        :draggable="sortable"
        @dragstart="!isPlaceholder(item) && handleDragStart(item)"
        @dragenter.prevent="!isPlaceholder(item) && handleDragEnter(item)"
        @dragover.prevent="!isPlaceholder(item) && handleDragOver(item)"
        @drop.prevent="!isPlaceholder(item) && handleDrop(item)"
        @dragend="handleDragEnd"
        @touchstart.passive="!isPlaceholder(item) && handleTouchStart($event, item)"
        @touchmove="handleTouchMove"
        @touchend="handleTouchEnd"
        @touchcancel="handleTouchCancel"
      >
        <div v-if="isPlaceholder(item)" class="touch-gap-slot" />
        <BookCard
          v-else
          :book="item"
          :edit-mode="editMode"
          :selected="selectedUrls?.has(item.bookUrl)"
          :is-search="isSearch"
          :in-shelf="isBookInShelf(item)"
          :dragging="sortable && draggedUrl === item.bookUrl"
          :show-delete-action="showDeleteAction"
          @click="$emit('click', $event)"
          @info="$emit('info', $event)"
          @delete="$emit('delete', $event)"
          @select="$emit('select', $event)"
          @addToShelf="$emit('addToShelf', $event)"
        />
      </div>
    </TransitionGroup>
    <div
      v-if="touchDragState.started && draggedBook"
      class="touch-drag-ghost"
      :style="ghostStyle"
    >
      <BookCard
        :book="draggedBook"
        :edit-mode="editMode"
        :selected="selectedUrls?.has(draggedBook.bookUrl)"
        :is-search="isSearch"
        :dragging="true"
        :show-delete-action="false"
      />
    </div>
    <div v-if="books.length === 0 && !loading" class="empty-state">
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5">
        <path d="M2 3h6a4 4 0 0 1 4 4v14a3 3 0 0 0-3-3H2z" />
        <path d="M22 3h-6a4 4 0 0 0-4 4v14a3 3 0 0 1 3-3h7z" />
      </svg>
      <p>{{ emptyText }}</p>
    </div>
    <div v-if="loading" class="loading-state">
      <div class="loading-spinner"></div>
      <p>加载中...</p>
    </div>
  </div>
</template>

<script setup lang="ts">
import { computed, nextTick, ref } from 'vue'
import BookCard from './BookCard.vue'
import type { Book, SearchBook } from '../types'
import { searchMergeKey } from '../utils/searchRank'

type DisplayItem = (Book | SearchBook) | { __placeholder: true }
const PLACEHOLDER_KEY = '__drag-placeholder__'

const props = defineProps<{
  books: (Book | SearchBook)[]
  editMode?: boolean
  selectedUrls?: Set<string>
  isSearch?: boolean
  loading?: boolean
  emptyText?: string
  sortable?: boolean
  showDeleteAction?: boolean
  /** 已在书架的书 URL 集合，用于搜索模式下显示「已加入」。 */
  shelfBookUrls?: Set<string>
  /** 已在书架的书名与作者标识，跨书源判断同一本书。 */
  shelfBookKeys?: Set<string>
}>()

const emit = defineEmits<{
  click: [book: Book | SearchBook]
  info: [book: Book | SearchBook]
  delete: [book: Book | SearchBook]
  select: [book: Book | SearchBook]
  addToShelf: [book: Book | SearchBook]
  reorder: [payload: { draggedUrl: string; targetUrl: string }]
}>()

function isBookInShelf(book: Book | SearchBook): boolean {
  return props.shelfBookUrls?.has(book.bookUrl)
    || props.shelfBookKeys?.has(searchMergeKey(book))
    || false
}

const draggedUrl = ref('')
const dropTargetUrl = ref('')
const touchDropIndex = ref(-1)
const itemRefs = ref<Record<string, HTMLElement>>({})
const touchDragState = ref({
  active: false,
  started: false,
  startX: 0,
  startY: 0,
  offsetX: 0,
  offsetY: 0,
})
const dragGhostRect = ref({
  left: 0,
  top: 0,
  width: 0,
  height: 0,
})
const dragPointerOffset = ref({
  x: 0,
  y: 0,
})
let longPressTimer: number | null = null
let autoScrollFrame: number | null = null
const touchDragMetrics = ref({
  itemHeight: 0,
  gap: 16,
})
const flipAnimationDurationMs = 680
const edgeScrollThresholdPx = 140
const maxAutoScrollStepPx = 24

function getBookUrl(book: Book | SearchBook) {
  return book.bookUrl
}

function isPlaceholder(item: DisplayItem): item is { __placeholder: true } {
  return '__placeholder' in item
}

function getDisplayKey(item: DisplayItem) {
  if (isPlaceholder(item)) return PLACEHOLDER_KEY
  return props.isSearch ? `${item.origin}::${item.bookUrl}` : item.bookUrl
}

function getRemainingBooks() {
  return props.books.filter((book) => getBookUrl(book) !== draggedUrl.value)
}

function handleDragStart(book: Book | SearchBook) {
  if (!props.sortable) return
  draggedUrl.value = getBookUrl(book)
  dropTargetUrl.value = getBookUrl(book)
  touchDropIndex.value = -1
}

function handleDragEnter(book: Book | SearchBook) {
  if (!props.sortable || !draggedUrl.value) return
  dropTargetUrl.value = getBookUrl(book)
}

function handleDragOver(book: Book | SearchBook) {
  if (!props.sortable || !draggedUrl.value) return
  dropTargetUrl.value = getBookUrl(book)
}

function handleDrop(book: Book | SearchBook) {
  if (!props.sortable || !draggedUrl.value) return
  const sourceUrl = draggedUrl.value
  const targetUrl = getBookUrl(book)
  draggedUrl.value = ''
  dropTargetUrl.value = ''
  touchDropIndex.value = -1
  if (sourceUrl !== targetUrl) {
    emit('reorder', { draggedUrl: sourceUrl, targetUrl })
  }
}

function handleDragEnd() {
  draggedUrl.value = ''
  dropTargetUrl.value = ''
  touchDropIndex.value = -1
  stopAutoScroll()
}

function setItemRef(el: Element | { $el?: Element } | null, bookUrl: string) {
  if (!bookUrl) return
  const resolved = el instanceof HTMLElement
    ? el
    : el && '$el' in el && el.$el instanceof HTMLElement
      ? el.$el
      : null
  if (resolved) {
    itemRefs.value[bookUrl] = resolved
  } else {
    delete itemRefs.value[bookUrl]
  }
}

function captureItemRects() {
  const rects = new Map<string, DOMRect>()
  for (const item of displayItems.value) {
    const key = getDisplayKey(item)
    if (!key) continue
    const rect = itemRefs.value[key]?.getBoundingClientRect()
    if (rect) {
      rects.set(key, rect)
    }
  }
  return rects
}

function playFlipAnimation(previousRects: Map<string, DOMRect>) {
  for (const [bookUrl, prevRect] of previousRects.entries()) {
    const el = itemRefs.value[bookUrl]
    if (!el) continue
    const nextRect = el.getBoundingClientRect()
    const deltaX = prevRect.left - nextRect.left
    const deltaY = prevRect.top - nextRect.top
    if (Math.abs(deltaX) < 0.5 && Math.abs(deltaY) < 0.5) continue

    el.style.transition = 'none'
    el.style.transform = `translate3d(${deltaX}px, ${deltaY}px, 0)`
    void el.offsetWidth
    el.style.transition = `transform ${flipAnimationDurationMs}ms cubic-bezier(0.22, 0.8, 0.22, 1)`
    el.style.transform = ''
    window.setTimeout(() => {
      if (el.style.transform === '') {
        el.style.transition = ''
      }
    }, flipAnimationDurationMs)
  }
}

function handleTouchStart(event: TouchEvent, book: Book | SearchBook) {
  if (!props.sortable || event.touches.length !== 1) return
  const touch = event.touches[0]
  if (!touch) return
  clearLongPressTimer()
  touchDragState.value = {
    active: true,
    started: false,
    startX: touch.clientX,
    startY: touch.clientY,
    offsetX: 0,
    offsetY: 0,
  }
  draggedUrl.value = getBookUrl(book)
  dropTargetUrl.value = getBookUrl(book)
  touchDropIndex.value = props.books.findIndex((item) => getBookUrl(item) === draggedUrl.value)
  longPressTimer = window.setTimeout(() => {
    if (!touchDragState.value.active || !draggedUrl.value) return
    touchDragState.value.started = true
    measureTouchDragMetrics(draggedUrl.value, touch.clientX, touch.clientY)
  }, 220)
}

function resolveTouchDropIndex(clientX: number, clientY: number) {
  const remainingBooks = getRemainingBooks()
  const entries = remainingBooks
    .map((book, index) => {
      const rect = itemRefs.value[getBookUrl(book)]?.getBoundingClientRect()
      return rect ? { index, rect, bookUrl: getBookUrl(book) } : null
    })
    .filter((entry): entry is { index: number; rect: DOMRect; bookUrl: string } => Boolean(entry))

  const containing = entries.find(({ rect }) => (
    clientX >= rect.left &&
    clientX <= rect.right &&
    clientY >= rect.top &&
    clientY <= rect.bottom
  ))
  if (containing) {
    const rect = containing.rect
    const beforeByAxis = clientX < rect.left + rect.width * 0.56
    return beforeByAxis ? containing.index : containing.index + 1
  }

  const sortedByTop = entries.slice().sort((a, b) => a.rect.top - b.rect.top || a.rect.left - b.rect.left)
  const rowGroups: Array<Array<{ index: number; rect: DOMRect; bookUrl: string }>> = []
  for (const entry of sortedByTop) {
    const lastRow = rowGroups[rowGroups.length - 1]
    if (!lastRow || Math.abs(lastRow[0]!.rect.top - entry.rect.top) > entry.rect.height * 0.5) {
      rowGroups.push([entry])
    } else {
      lastRow.push(entry)
    }
  }

  for (const row of rowGroups) {
    const top = Math.min(...row.map((entry) => entry.rect.top))
    const bottom = Math.max(...row.map((entry) => entry.rect.bottom))
    const rowPadding = (bottom - top) * 0.5
    if (clientY >= top - rowPadding && clientY <= bottom + rowPadding) {
      const sortedRow = row.slice().sort((a, b) => a.rect.left - b.rect.left)
      for (const entry of sortedRow) {
        const centerX = entry.rect.left + entry.rect.width * 0.56
        if (clientX < centerX) return entry.index
      }
      return sortedRow[sortedRow.length - 1]!.index + 1
    }
  }

  const first = sortedByTop[0]
  const last = sortedByTop[sortedByTop.length - 1]
  if (!first || !last) return -1
  if (clientY < first.rect.top) return 0
  if (clientY > last.rect.bottom) return remainingBooks.length

  let closestIndex = 0
  let closestDistance = Number.POSITIVE_INFINITY
  for (const entry of entries) {
    const centerX = entry.rect.left + entry.rect.width / 2
    const centerY = entry.rect.top + entry.rect.height / 2
    const distance = Math.hypot(clientX - centerX, clientY - centerY)
    if (distance < closestDistance) {
      closestDistance = distance
      closestIndex = entry.index
    }
  }
  return closestIndex
}

function handleTouchMove(event: TouchEvent) {
  if (!props.sortable || !touchDragState.value.active || event.touches.length !== 1) return
  const touch = event.touches[0]
  if (!touch) return
  const deltaX = touch.clientX - touchDragState.value.startX
  const deltaY = touch.clientY - touchDragState.value.startY
  touchDragState.value.offsetX = deltaX
  touchDragState.value.offsetY = deltaY
  if (!touchDragState.value.started) {
    if (Math.abs(deltaX) > 10 || Math.abs(deltaY) > 10) {
      handleTouchCancel()
    }
    return
  }
  event.preventDefault()
  updateAutoScroll(touch.clientY)
  const targetIndex = resolveTouchDropIndex(touch.clientX, touch.clientY)
  if (targetIndex !== -1 && targetIndex !== touchDropIndex.value) {
    const previousRects = captureItemRects()
    touchDropIndex.value = targetIndex
    nextTick(() => playFlipAnimation(previousRects))
  }
}

function handleTouchEnd() {
  if (!props.sortable || !touchDragState.value.active) {
    handleTouchCancel()
    return
  }
  const sourceUrl = draggedUrl.value
  const fromIndex = props.books.findIndex((item) => getBookUrl(item) === sourceUrl)
  const insertIndex = touchDropIndex.value
  const remainingBooks = getRemainingBooks()
  const targetUrl = (
    insertIndex !== -1 &&
    remainingBooks.length > 0 &&
    insertIndex !== fromIndex
  )
    ? (insertIndex > fromIndex
      ? getBookUrl(remainingBooks[Math.min(insertIndex - 1, remainingBooks.length - 1)]!)
      : getBookUrl(remainingBooks[Math.min(insertIndex, remainingBooks.length - 1)]!))
    : ''
  const shouldEmit = touchDragState.value.started && sourceUrl && targetUrl && sourceUrl !== targetUrl
  handleTouchCancel()
  if (shouldEmit) {
    emit('reorder', { draggedUrl: sourceUrl, targetUrl })
  }
}

function handleTouchCancel() {
  clearLongPressTimer()
  stopAutoScroll()
  touchDragState.value.active = false
  touchDragState.value.started = false
  touchDragState.value.offsetX = 0
  touchDragState.value.offsetY = 0
  draggedUrl.value = ''
  dropTargetUrl.value = ''
  touchDropIndex.value = -1
}

function clearLongPressTimer() {
  if (longPressTimer) {
    clearTimeout(longPressTimer)
    longPressTimer = null
  }
}

function getScrollContainer() {
  return document.scrollingElement || document.documentElement
}

function updateAutoScroll(clientY: number) {
  const viewportHeight = window.innerHeight
  let delta = 0
  if (clientY < edgeScrollThresholdPx) {
    const intensity = 1 - clientY / edgeScrollThresholdPx
    delta = -Math.max(6, maxAutoScrollStepPx * intensity)
  } else if (clientY > viewportHeight - edgeScrollThresholdPx) {
    const distanceFromBottom = viewportHeight - clientY
    const intensity = 1 - distanceFromBottom / edgeScrollThresholdPx
    delta = Math.max(6, maxAutoScrollStepPx * intensity)
  }

  if (Math.abs(delta) < 0.5) {
    stopAutoScroll()
    return
  }

  const step = () => {
    const container = getScrollContainer()
    container.scrollTop += delta
    autoScrollFrame = window.requestAnimationFrame(step)
  }

  if (autoScrollFrame === null) {
    autoScrollFrame = window.requestAnimationFrame(step)
  }
}

function stopAutoScroll() {
  if (autoScrollFrame !== null) {
    window.cancelAnimationFrame(autoScrollFrame)
    autoScrollFrame = null
  }
}

function measureTouchDragMetrics(bookUrl: string, clientX?: number, clientY?: number) {
  const item = itemRefs.value[bookUrl]
  const itemHeight = item?.getBoundingClientRect().height || 0
  const rect = item?.getBoundingClientRect()
  const grid = item?.parentElement
  const style = grid ? window.getComputedStyle(grid) : null
  const gap = style ? parseFloat(style.rowGap || style.gap || '16') : 16
  if (rect) {
    dragGhostRect.value = {
      left: rect.left,
      top: rect.top,
      width: rect.width,
      height: rect.height,
    }
    if (typeof clientX === 'number' && typeof clientY === 'number') {
      dragPointerOffset.value = {
        x: Math.min(Math.max(clientX - rect.left, rect.width * 0.22), rect.width * 0.78),
        y: Math.min(Math.max(clientY - rect.top, rect.height * 0.18), rect.height * 0.82),
      }
      touchDragState.value.offsetX = clientX - touchDragState.value.startX
      touchDragState.value.offsetY = clientY - touchDragState.value.startY
    }
  }
  touchDragMetrics.value = {
    itemHeight,
    gap: Number.isFinite(gap) ? gap : 16,
  }
}

function getItemStyle() {
  if (!(props.sortable && touchDragState.value.started)) return undefined
  return undefined
}

const draggedBook = computed(() => {
  if (!draggedUrl.value) return null
  return props.books.find((book) => getBookUrl(book) === draggedUrl.value) || null
})

const displayItems = computed<DisplayItem[]>(() => {
  if (!(props.sortable && touchDragState.value.started && draggedUrl.value && touchDropIndex.value !== -1)) {
    return props.books
  }
  const remainingBooks = getRemainingBooks()
  const insertIndex = Math.max(0, Math.min(touchDropIndex.value, remainingBooks.length))
  if (insertIndex === -1) {
    return props.books
  }
  const next: DisplayItem[] = remainingBooks.slice()
  next.splice(insertIndex, 0, { __placeholder: true })
  return next
})

const ghostStyle = computed(() => {
  const dragged = draggedBook.value
  if (!dragged || !touchDragState.value.started) return undefined
  const currentPointerX = touchDragState.value.startX + touchDragState.value.offsetX
  const currentPointerY = touchDragState.value.startY + touchDragState.value.offsetY
  const x = currentPointerX - dragPointerOffset.value.x
  const y = currentPointerY - dragPointerOffset.value.y
  return {
    left: `${x}px`,
    top: `${y}px`,
    width: `${dragGhostRect.value.width}px`,
    height: `${dragGhostRect.value.height}px`,
  }
})
</script>

<style scoped>
.book-grid {
  position: relative;
}

.book-grid-list {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(320px, 420px));
  justify-content: start;
  gap: var(--space-4);
  padding: var(--space-4) 0;
}

.book-grid-item {
  min-width: 0;
  width: 100%;
  max-width: 420px;
  height: 100%;
  transition:
    transform 820ms cubic-bezier(0.16, 0.84, 0.2, 1),
    filter 700ms ease,
    opacity 700ms ease;
  transform-origin: center center;
}

.book-grid-item.drop-target :deep(.book-card) {
  border-color: var(--color-primary);
  box-shadow:
    0 0 0 2px rgba(var(--color-primary-rgb), 0.16),
    0 12px 24px rgba(var(--color-primary-rgb), 0.12);
}

.book-grid-item.is-dragging {
  cursor: grabbing;
}

.book-grid-item.is-touch-dragging {
  position: relative;
  pointer-events: none;
}

.book-grid-item.is-touch-gap :deep(.book-card) {
  border-style: dashed;
  border-color: rgba(var(--color-primary-rgb), 0.7);
  background:
    linear-gradient(135deg, rgba(var(--color-primary-rgb), 0.08), rgba(var(--color-primary-rgb), 0.02)),
    var(--color-surface);
  box-shadow:
    inset 0 0 0 2px rgba(var(--color-primary-rgb), 0.16),
    0 0 0 2px rgba(var(--color-primary-rgb), 0.08);
}

.book-grid.touch-sorting .book-grid-item.is-touch-sibling {
  animation: shelf-jiggle 1.08s ease-in-out infinite alternate;
}

.book-grid.touch-sorting .book-grid-item.is-touch-sibling:nth-child(2n) {
  animation-delay: 0.06s;
}

.book-grid.touch-sorting .book-grid-item.is-touch-sibling:nth-child(3n) {
  animation-delay: 0.12s;
}

.empty-state {
  grid-column: 1 / -1;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  padding: var(--space-16) var(--space-6);
  color: var(--color-text-tertiary);
  gap: var(--space-4);
}

.empty-state svg {
  width: 64px;
  height: 64px;
  opacity: 0.3;
}

.empty-state p {
  font-size: var(--text-base);
}

.loading-state {
  grid-column: 1 / -1;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  padding: var(--space-16) var(--space-6);
  gap: var(--space-4);
  color: var(--color-text-tertiary);
}

.loading-spinner {
  width: 32px;
  height: 32px;
  border: 3px solid var(--color-border);
  border-top-color: var(--color-primary);
  border-radius: 50%;
  animation: spin 0.8s linear infinite;
}

@keyframes spin {
  to { transform: rotate(360deg); }
}

/* Card transition group */
.card-enter-active {
  transition: all 820ms cubic-bezier(0.16, 0.84, 0.2, 1);
}
.card-leave-active {
  transition: all 360ms ease-out;
}
.card-enter-from {
  opacity: 0;
  transform: scale(0.95) translateY(8px);
}
.card-leave-to {
  opacity: 0;
  transform: scale(0.95);
}
.card-move {
  transition: transform 820ms cubic-bezier(0.16, 0.84, 0.2, 1);
}

.touch-drag-ghost {
  position: fixed;
  z-index: 30;
  pointer-events: none;
  transform: scale(1.04);
  filter: drop-shadow(0 18px 24px rgba(0, 0, 0, 0.18));
}

.touch-drag-ghost :deep(.book-card) {
  border-color: var(--color-primary);
  box-shadow:
    0 16px 30px rgba(0, 0, 0, 0.18),
    0 0 0 2px rgba(var(--color-primary-rgb), 0.18);
}

@keyframes shelf-jiggle {
  from {
    transform: rotate(-0.45deg);
  }
  to {
    transform: rotate(0.45deg);
  }
}

@media (max-width: 640px) {
  .book-grid-list {
    grid-template-columns: 1fr;
  }

  .book-grid-item {
    max-width: none;
  }
}
</style>
