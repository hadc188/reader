<template>
  <div ref="resultsContainerRef" class="search-results">
    <div class="search-header">
      <h2>
        搜索 "{{ searchKey }}"
        <span v-if="isSearching" class="searching-indicator">
          <span class="dot-pulse"></span>
          搜索中...
        </span>
        <span v-else class="result-count">({{ rankedResults.length }} 个结果)</span>
      </h2>
      <button class="back-btn" @click="$emit('back')">
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="18" height="18">
          <path d="M18 6 6 18M6 6l12 12" />
        </svg>
        返回书架
      </button>
    </div>

    <div class="search-filters">
      <div class="filter-tabs" role="tablist" aria-label="搜索范围">
        <button
          type="button"
          class="filter-tab"
          :class="{ active: searchScope === 'all' }"
          @click="searchScope = 'all'"
        >
          全部书源
        </button>
        <button
          type="button"
          class="filter-tab"
          :class="{ active: searchScope === 'group' }"
          @click="searchScope = 'group'"
        >
          按分组
        </button>
        <button
          type="button"
          class="filter-tab"
          :class="{ active: searchScope === 'source' }"
          @click="searchScope = 'source'"
        >
          单个书源
        </button>
      </div>

      <div v-if="searchScope === 'group'" class="filter-select-wrap">
        <select v-model="selectedGroup" class="filter-select">
          <option v-for="group in sourceGroups" :key="group" :value="group">
            {{ group }}
          </option>
        </select>
      </div>

      <div v-else-if="searchScope === 'source'" class="filter-select-wrap">
        <select v-model="selectedSourceUrl" class="filter-select">
          <option v-for="source in sourceOptions" :key="source.bookSourceUrl" :value="source.bookSourceUrl">
            {{ source.bookSourceName }}
          </option>
        </select>
      </div>
    </div>

    <BookGrid
      :books="displayResults"
      :is-search="true"
      :shelf-book-urls="shelfBookUrls"
      :shelf-book-keys="shelfBookKeys"
      :loading="isSearching && displayResults.length === 0"
      :animate="false"
      empty-text="未找到相关书籍"
      @click="handleBookClick"
      @info="handleBookInfo"
      @addToShelf="handleAddToShelf"
      @contextmenu="handleBookContextMenu"
    />

    <div v-if="totalPages > 1" class="pagination">
      <div class="pagination-count">{{ pageRangeText }}</div>
      <div class="pagination-controls">
        <button
          type="button"
          class="page-btn"
          :disabled="currentPage <= 1"
          @click="goToPage(currentPage - 1)"
        >
          上一页
        </button>
        <template v-for="(item, index) in pageItems" :key="`${item}-${index}`">
          <span v-if="item === 'ellipsis'" class="page-ellipsis">…</span>
          <button
            v-else
            type="button"
            class="page-btn page-number"
            :class="{ active: item === currentPage }"
            :aria-current="item === currentPage ? 'page' : undefined"
            @click="goToPage(item)"
          >
            {{ item }}
          </button>
        </template>
        <button
          type="button"
          class="page-btn"
          :disabled="currentPage >= totalPages"
          @click="goToPage(currentPage + 1)"
        >
          下一页
        </button>
      </div>
    </div>

    <BookDetailModal
      v-model="showBookDetail"
      :book="selectedBook"
    />
  </div>
</template>

<script setup lang="ts">
import { computed, nextTick, onMounted, onUnmounted, ref, watch } from 'vue'
import { useBookshelfStore } from '../stores/bookshelf'
import { useAppStore } from '../stores/app'
import { useSourceStore } from '../stores/source'
import { searchBookMultiSSE } from '../api/search'
import type { SseLike } from '../api/sse'
import { saveBook } from '../api/bookshelf'
import { isBookOnShelf } from '../utils/bookEdit'
import {
  initializeSearchResult,
  isSearchResultRelevant,
  mergeSearchResult,
  rankSearchResults,
  searchMergeKey,
} from '../utils/searchRank'
import BookGrid from './BookGrid.vue'
import BookDetailModal from './BookDetailModal.vue'
import { showContextMenu } from '../composables/useContextMenu'
import type { Book, SearchBook } from '../types'

import { storeToRefs } from 'pinia'

const shelfStore = useBookshelfStore()
const appStore = useAppStore()
const sourceStore = useSourceStore()

const {
  searchKey,
  searchResults: results,
  isSearching,
  searchScope,
  searchGroup: selectedGroup,
  searchSourceUrl: selectedSourceUrl,
} = storeToRefs(shelfStore)

let eventSource: SseLike | null = null
let searchGeneration = 0
const resultsContainerRef = ref<HTMLElement | null>(null)
const showBookDetail = ref(false)
const selectedBook = ref<Book | SearchBook | null>(null)

const sourceByUrl = computed(() => {
  return new Map(sourceStore.sources.map((source) => [source.bookSourceUrl, source]))
})

// 已在书架的书 URL 集合，用于搜索卡片显示「已加入」。
const shelfBookUrls = computed(() => {
  return new Set(shelfStore.books.map((book) => book.bookUrl))
})

const shelfBookKeys = computed(() => {
  return new Set(shelfStore.books.map((book) => searchMergeKey(book)))
})

const sourceGroups = computed(() => {
  const groups = new Set<string>()
  for (const source of sourceStore.sources.filter((item) => item.enabled !== false)) {
    const parts = (source.bookSourceGroup || '')
      .split(/[;,，；、|/]/)
      .map((item) => item.trim())
      .filter(Boolean)
    for (const group of parts) {
      groups.add(group)
    }
  }
  return Array.from(groups).sort((a, b) => a.localeCompare(b, 'zh-Hans-CN'))
})

const sourceOptions = computed(() => {
  return [...sourceStore.sources]
    .filter((source) => source.enabled !== false)
    .sort((a, b) => {
      const orderDiff = (a.customOrder ?? 0) - (b.customOrder ?? 0)
      if (orderDiff !== 0) return orderDiff
      return a.bookSourceName.localeCompare(b.bookSourceName, 'zh-Hans-CN')
    })
})

const enabledSourceUrls = computed(() => {
  return new Set(sourceOptions.value.map((source) => source.bookSourceUrl))
})

const enabledSourceSignature = computed(() => {
  return sourceOptions.value.map((source) => source.bookSourceUrl).join('\n')
})

/**
 * 搜索结果每页条数。
 *
 * 多源搜索动辄上千条(实测 1964 条), 而 BookGrid 是 `v-for` 全量渲染 + TransitionGroup,
 * 全部铺进 DOM 会让布局/样式重算成本随条数线性增长, 滚动直接掉帧。
 * 因此按页渲染, 只把当前页交给 BookGrid。
 *
 * **已知局限**: 分页只降低**渲染**量, 没有改动下面 `rankedResults` 的全量
 * filter/map/sort —— 搜索**进行中**每收到一批 SSE 消息仍会整体重算一遍
 * (见 onmessage 里整体替换 `searchResults`)。用户反馈的"搜索完成后卡顿"
 * 主因是 DOM 数量, 所以这里已足够; 若之后要优化搜索过程本身, 应改
 * `shallowRef` + 分帧提交, 而不是继续加大分页。
 */
const SEARCH_RESULT_PAGE_SIZE = 100

const currentPage = ref(1)

/** 页码按钮项: 数字直接跳页, `'ellipsis'` 渲染成省略号。 */
type PageItem = number | 'ellipsis'

/** 全部命中结果(已完成按书源可用性与相关度过滤排序), 用于展示总数与分页。 */
const rankedResults = computed<SearchBook[]>(() => {
  const enriched = results.value
    .filter((book) => enabledSourceUrls.value.has(book.origin))
    .map((book) => {
      const source = sourceByUrl.value.get(book.origin)
      return {
        ...book,
        bookSourceUrls: book.bookSourceUrls?.filter((url) => enabledSourceUrls.value.has(url)),
        sourceCandidates: book.sourceCandidates?.filter((candidate) => (
          enabledSourceUrls.value.has(candidate.origin)
        )),
        originName: book.originName || source?.bookSourceName || book.origin,
        originGroup: book.originGroup || source?.bookSourceGroup,
      }
    })
  return rankSearchResults(enriched, searchKey.value)
})

/** 真正交给 BookGrid 渲染的当前页切片。 */
const displayResults = computed<SearchBook[]>(() => {
  const start = (currentPage.value - 1) * SEARCH_RESULT_PAGE_SIZE
  return rankedResults.value.slice(start, start + SEARCH_RESULT_PAGE_SIZE)
})

const totalPages = computed(() =>
  Math.max(1, Math.ceil(rankedResults.value.length / SEARCH_RESULT_PAGE_SIZE))
)

/** 当前页第一条/最后一条在全部结果中的序号(1-based), 用于"第 X-Y 条"提示。 */
const pageRangeText = computed(() => {
  const total = rankedResults.value.length
  if (total === 0) return ''
  const start = (currentPage.value - 1) * SEARCH_RESULT_PAGE_SIZE + 1
  const end = Math.min(total, currentPage.value * SEARCH_RESULT_PAGE_SIZE)
  return `第 ${start}-${end} 条，共 ${total} 条`
})

/**
 * 计算要渲染的页码按钮。
 *
 * 结果多时页数可达几十页, 全列出来会把分页条撑成一条长带; 只保留首页、末页
 * 与当前页附近的若干页, 中间用省略号。`siblingCount` 是当前页两侧各保留几个。
 */
const pageItems = computed<PageItem[]>(() => {
  const total = totalPages.value
  const current = currentPage.value
  const siblingCount = 1
  // 页数少时直接全列(不留省略号, 免得比页码还占地方)。
  const maxWithoutEllipsis = 7
  if (total <= maxWithoutEllipsis) {
    return Array.from({ length: total }, (_, index) => index + 1)
  }

  const first = 1
  const last = total
  const left = Math.max(first + 1, current - siblingCount)
  const right = Math.min(last - 1, current + siblingCount)
  const items: PageItem[] = [first]
  if (left > first + 1) items.push('ellipsis')

  for (let page = left; page <= right; page += 1) {
    items.push(page)
  }

  if (right < last - 1) items.push('ellipsis')
  items.push(last)
  return items
})

/**
 * 跳页。
 *
 * 页码变化后把滚动容器回到顶部: 停在底部再翻页会看到新一页的末尾,
 * 用户会以为"没翻动"。用 nextTick 等 DOM 更新完再滚, 否则滚的是旧高度。
 */
function goToPage(page: number) {
  const target = Math.min(Math.max(1, page), totalPages.value)
  if (target === currentPage.value) return
  currentPage.value = target
  nextTick(() => {
    resultsContainerRef.value?.scrollTo({ top: 0 })
  })
}

function closeEventSource() {
  if (eventSource) {
    eventSource.close()
    eventSource = null
  }
}

function ensureSearchSelection() {
  if (searchScope.value === 'group') {
    const selectedGroupStillValid = selectedGroup.value && sourceGroups.value.includes(selectedGroup.value)
    if (!selectedGroupStillValid && sourceGroups.value.length > 0) {
      selectedGroup.value = sourceGroups.value[0]
    }
  }
  if (searchScope.value === 'source') {
    const selectedSourceStillValid = sourceOptions.value.some((source) => source.bookSourceUrl === selectedSourceUrl.value)
    if (!selectedSourceStillValid && sourceOptions.value.length > 0) {
      selectedSourceUrl.value = sourceOptions.value[0].bookSourceUrl
    }
  }
}

function doSearch(key: string) {
  closeEventSource()
  const generation = ++searchGeneration
  // 新搜索从第一页开始: 沿用上一次的页码会落在空页上(新结果更少时)。
  currentPage.value = 1

  if (searchScope.value === 'group' && !selectedGroup.value) {
    shelfStore.searchResults = []
    shelfStore.isSearching = false
    return
  }

  if (searchScope.value === 'source' && !selectedSourceUrl.value) {
    shelfStore.searchResults = []
    shelfStore.isSearching = false
    return
  }

  shelfStore.searchResults = []
  shelfStore.isSearching = true

  const stream = searchBookMultiSSE({
    key,
    concurrentCount: 24,
    bookSourceGroup: searchScope.value === 'group' ? selectedGroup.value : undefined,
    bookSourceUrl: searchScope.value === 'source' ? selectedSourceUrl.value : undefined,
  })
  eventSource = stream

  const isCurrentSearch = () => (
    generation === searchGeneration
    && eventSource === stream
    && shelfStore.searchKey === key
  )

  stream.onmessage = (event) => {
    if (!isCurrentSearch()) return
    try {
      const data = event.data as { data?: SearchBook[] }
      if (data.data && Array.isArray(data.data)) {
        const byKey = new Map(shelfStore.searchResults.map((r) => [searchMergeKey(r), r]))
        for (const b of data.data) {
          if (!isSearchResultRelevant(b, key)) continue
          const mergeKey = searchMergeKey(b)
          const existing = byKey.get(mergeKey)
          if (existing) {
            byKey.set(mergeKey, mergeSearchResult(existing, b))
          } else {
            byKey.set(mergeKey, initializeSearchResult(b))
          }
        }
        shelfStore.searchResults = Array.from(byKey.values())
      }
    } catch { /* skip */ }
  }

  stream.addEventListener('end', () => {
    if (!isCurrentSearch()) return
    shelfStore.isSearching = false
    stream.close()
    eventSource = null
  })

  stream.addEventListener('error', () => {
    if (!isCurrentSearch()) return
    shelfStore.isSearching = false
    stream.close()
    eventSource = null
  })

  stream.onerror = () => {
    if (!isCurrentSearch()) return
    shelfStore.isSearching = false
    stream.close()
    eventSource = null
  }
}

watch(
  [() => shelfStore.searchKey, searchScope, selectedGroup, selectedSourceUrl],
  ([key]) => {
    ensureSearchSelection()
    if (key) {
      doSearch(key)
    } else {
      searchGeneration += 1
      closeEventSource()
      shelfStore.searchResults = []
      shelfStore.isSearching = false
      currentPage.value = 1
    }
  },
  { immediate: true }
)

watch([searchScope, sourceGroups, sourceOptions], () => {
  ensureSearchSelection()
}, { immediate: true })

watch(enabledSourceSignature, (next, previous) => {
  if (next === previous || !searchKey.value) return
  doSearch(searchKey.value)
})

onMounted(async () => {
  // Always refresh the source list on mount so the group/source dropdowns
  // reflect any add/delete/edit done in the source manager since last visit.
  await sourceStore.fetchSources(true).catch(() => undefined)
  ensureSearchSelection()
})

onUnmounted(() => {
  searchGeneration += 1
  closeEventSource()
})

// Clicking a search result (card body or cover) always opens the detail modal.
// Reading only happens via the "开始阅读" button in the modal.
function handleBookClick(book: Book | SearchBook) {
  selectedBook.value = book
  showBookDetail.value = true
}

function handleBookInfo(book: Book | SearchBook) {
  selectedBook.value = book
  showBookDetail.value = true
}

async function handleAddToShelf(book: Book | SearchBook) {
  // 函数级判重, 不能只靠 UI 置灰: 卡片禁用与右键菜单都依赖 shelfStore.books
  // 已加载, 一旦它还是空(启动竞态/加载失败被静默吞掉), 两层拦截都会失效,
  // 点下去就会走 save_book 的整条覆盖分支 —— 这里只提交 6 个字段, 会把用户
  // 自定义的封面、简介、original_* 等一并清空。
  if (isBookOnShelf(book, shelfStore.books)) {
    appStore.showToast(`"${book.name}" 已在书架中`, 'warning')
    return
  }
  try {
    await saveBook({
      name: book.name,
      author: book.author,
      bookUrl: book.bookUrl,
      origin: book.origin,
      coverUrl: book.coverUrl,
      sourceCandidates: (book as SearchBook).sourceCandidates,
    })
    await shelfStore.fetchBooks()
    appStore.showToast('成功加入书架', 'success')
  } catch (e: unknown) {
    appStore.showToast((e as Error).message, 'error')
  }
}

function handleBookContextMenu({ book, event }: { book: Book | SearchBook; event: MouseEvent }) {
  const inShelf = shelfBookUrls.value.has(book.bookUrl)
    || shelfBookKeys.value.has(searchMergeKey(book))
  const menuItems = [
    { label: '查看详情', action: () => handleBookInfo(book) },
    { divider: true },
    inShelf
      ? { label: '已在书架', disabled: true }
      : { label: '加入书架', action: () => handleAddToShelf(book) },
  ]
  showContextMenu(event, menuItems, book)
}

defineEmits<{
  back: []
}>()
</script>

<style scoped>
.search-results {
  height: 100%;
  min-height: 0;
  overflow: auto;
  padding: 0 var(--space-6);
}

.search-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: var(--space-4) 0;
  gap: var(--space-4);
}

.search-header h2 {
  font-size: var(--text-xl);
  font-weight: 700;
  display: flex;
  align-items: center;
  gap: var(--space-3);
}

.result-count {
  font-size: var(--text-sm);
  font-weight: 400;
  color: var(--color-text-tertiary);
}

.searching-indicator {
  display: inline-flex;
  align-items: center;
  gap: var(--space-2);
  font-size: var(--text-sm);
  font-weight: 400;
  color: var(--color-primary);
}

.dot-pulse {
  display: inline-block;
  width: 8px;
  height: 8px;
  border-radius: 50%;
  background: var(--color-primary);
  animation: pulse 1.2s infinite ease-in-out;
}

@keyframes pulse {
  0%, 80%, 100% {
    transform: scale(0.6);
    opacity: 0.5;
  }
  40% {
    transform: scale(1);
    opacity: 1;
  }
}

.back-btn {
  display: flex;
  align-items: center;
  gap: var(--space-2);
  padding: var(--space-2) var(--space-4);
  border-radius: var(--radius-md);
  font-size: var(--text-sm);
  font-weight: 500;
  color: var(--color-text-secondary);
  border: 1px solid var(--color-border);
  transition: all var(--duration-fast);
}

.back-btn:hover {
  background: var(--color-bg-hover);
  color: var(--color-text);
}

.search-filters {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: var(--space-3);
  margin-bottom: var(--space-5);
}

.filter-tabs {
  display: inline-flex;
  align-items: center;
  gap: var(--space-2);
  padding: 4px;
  border-radius: var(--radius-full);
  background: var(--color-bg-elevated);
  border: 1px solid var(--color-border-light);
}

.filter-tab {
  min-height: 34px;
  padding: 0 var(--space-4);
  border-radius: var(--radius-full);
  font-size: var(--text-sm);
  font-weight: 500;
  color: var(--color-text-secondary);
  transition: all var(--duration-fast);
}

.filter-tab:hover {
  color: var(--color-text);
  background: var(--color-bg-hover);
}

.filter-tab.active {
  color: white;
  background: var(--color-primary);
}

.filter-select-wrap {
  min-width: min(100%, 280px);
}

.filter-select {
  width: 100%;
  min-height: 40px;
  padding: 0 var(--space-4);
  border-radius: var(--radius-lg);
  border: 1px solid var(--color-border);
  background: var(--color-bg-elevated);
  color: var(--color-text);
  font-size: var(--text-sm);
}

.pagination {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: var(--space-3);
  padding: var(--space-4) 0 var(--space-8);
}

.pagination-count {
  font-size: var(--text-sm);
  color: var(--color-text-tertiary);
}

.pagination-controls {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  justify-content: center;
  gap: var(--space-2);
}

.page-btn {
  min-width: 36px;
  min-height: 36px;
  padding: 0 var(--space-3);
  border-radius: var(--radius-md);
  border: 1px solid var(--color-border);
  background: transparent;
  color: var(--color-text-secondary);
  font-size: var(--text-sm);
  cursor: pointer;
  transition: all var(--duration-fast);
}

.page-btn:hover:not(:disabled):not(.active) {
  color: var(--color-primary);
  border-color: var(--color-primary);
  background: var(--color-bg-hover);
}

.page-btn:disabled {
  opacity: 0.45;
  cursor: not-allowed;
}

.page-number.active {
  color: #fff;
  background: var(--color-primary);
  border-color: var(--color-primary);
  font-weight: 600;
}

.page-ellipsis {
  min-width: 24px;
  text-align: center;
  color: var(--color-text-tertiary);
  user-select: none;
}

@media (max-width: 720px) {
  .search-results {
    padding: 0 var(--space-4);
  }

  .search-header {
    flex-direction: column;
    align-items: stretch;
  }

  .search-header h2 {
    flex-wrap: wrap;
  }

  .back-btn {
    justify-content: center;
  }

  .filter-tabs {
    width: 100%;
    justify-content: space-between;
  }

  .filter-tab {
    flex: 1;
    padding: 0 var(--space-2);
  }

  .filter-select-wrap {
    width: 100%;
    min-width: 0;
  }
}
</style>
