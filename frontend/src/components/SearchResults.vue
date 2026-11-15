<template>
  <div ref="resultsContainerRef" class="search-results" @scroll="handleResultsScroll">
    <div class="search-header">
      <h2>
        搜索 "{{ searchKey }}"
        <span v-if="isSearching" class="searching-indicator">
          <span class="dot-pulse"></span>
          搜索中...
        </span>
        <span v-else class="result-count">({{ rankedResults.length }} 个结果)</span>
        <!-- 结果来自会话缓存时标一下；点它可重搜。
             原来是单独一行提示条（"已显示上次的搜索结果 / 重新搜索"），既多占
             一行又容易被重复渲染，改成一枚可点的角标。 -->
        <button
          v-if="resultsFromCache"
          type="button"
          class="cache-badge"
          title="结果来自上次搜索，点击重新搜索"
          @click="refreshSearch"
        >缓存 ⟳</button>
      </h2>
      <button class="back-btn" @click="backToShelf">
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
      :books="rankedResults"
      :is-search="true"
      :shelf-book-urls="shelfBookUrls"
      :shelf-book-keys="shelfBookKeys"
      :loading="isSearching && rankedResults.length === 0"
      :animate="false"
      empty-text="未找到相关书籍"
      @click="handleBookClick"
      @info="handleBookInfo"
      @addToShelf="handleAddToShelf"
      @contextmenu="handleBookContextMenu"
    />

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
import { createCoverCacheSession } from '../utils/coverCacheSession'
import { getSearchSessionCache, searchSessionKey } from '../utils/searchSessionCache'
import { shouldResearchForSourceChange } from '../utils/searchSourceChange'
import { isBookOnShelf } from '../utils/bookEdit'
import {
  initializeSearchResult,
  isSearchResultRelevant,
  mergeSearchResult,
  rankSearchResults,
  searchMergeKey,
  shelfMatchKey,
} from '../utils/searchRank'
import BookGrid from './BookGrid.vue'
import BookDetailModal from './BookDetailModal.vue'
import { showContextMenu } from '../composables/useContextMenu'
import type { Book, SearchBook } from '../types'

import { storeToRefs } from 'pinia'

const emit = defineEmits<{
  back: []
}>()

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
/**
 * 当前这轮搜索已经收到第几个书源（后端 `lastIndex`）。
 *
 * 续搜断点：搜索进行中进阅读页会关流，回来时从这里接着要后面的批次。
 * 每轮新搜索开始时置 -1。
 */
let searchLastIndex = -1
/**
 * 当前已经拉到的搜索页（每个书源各自的 page）。
 *
 * 多源搜索的“更多”有两层：书源清单还没走完（`lastIndex < 总数`），以及
 * 同一批书源还有下一页（legado 滚动到底 `searchPage++`）。第二层以前完全
 * 缺失，所以 JSON 接口站点（如搬山人）只能看到第 1 页。
 */
const searchPage = ref(1)
/** 本次搜索后端是否报告「这一页还有结果」。 */
const hasMorePages = ref(false)
/** 后端是否还有未扫描的书源（分批续扫时用）。 */
const hasMoreSources = ref(false)
/** 当前页（可能分几次扫完书源）是否已经收到过任意结果。 */
let pageHadResults = false
const coverCacheSession = createCoverCacheSession()
/**
 * 搜索结果的会话缓存。
 *
 * **注意 `<script setup>` 的顶层代码在 `setup()` 里执行，每个实例一份** ——
 * 这里写 `createSearchSessionCache()` 会是「每次挂载一个新的空缓存」，进阅读页
 * 卸载后就没了，返回时命中不了。所以必须从 utils 里取模块级单例。
 */
const searchSessionCache = getSearchSessionCache()
/** 本次结果是不是直接来自缓存（决定要不要提示「已显示上次结果」）。 */
const resultsFromCache = ref(false)
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
  // 用 shelfMatchKey 而不是 searchMergeKey: 后者会把本地《遮天》和网络《遮天》
  // 并成一本, 导致搜索卡片错误地显示「已加入」。
  return new Set(shelfStore.books.map((book) => shelfMatchKey(book)))
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

/** 全部命中结果(已完成按书源可用性与相关度过滤排序)。 */
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

/**
 * 拉下一批书源搜索结果。
 *
 * 先续完当前页剩余书源；全部书源都搜过且本页有结果时，再把每个书源的
 * `page` 加一并从第一个书源重新扫一遍 —— 与 legado `SearchModel.search()`
 * 的 `searchPage++` 语义一致。
 */
function loadMore() {
  if (isSearching.value) return
  const key = shelfStore.searchKey
  if (!key) return
  // 书源清单可能按分组/单源过滤，前端拿不到后端实际列表长度，
  // 因此只以后端的 `hasMore` 为准，不用 sourceOptions 猜。
  if (hasMoreSources.value) {
    continueSearchFrom(searchLastIndex + 1)
    return
  }
  if (!hasMorePages.value) return
  searchPage.value += 1
  // 新的一页必须从第一个书源重新扫: 沿用上一页的 lastIndex 会跳过
  // 新页前面的书源。
  searchLastIndex = -1
  pageHadResults = false
  continueSearchFrom(-1)
}

/** 从指定书源下标续拉当前搜索页（-1 = 从头开始）。 */
function continueSearchFrom(startIndex: number) {
  const key = shelfStore.searchKey
  if (!key) return
  const sessionKey = currentSearchSessionKey(key)
  shelfStore.isSearching = true
  streamSearch(key, sessionKey, startIndex, searchPage.value)
}

/** 滚动接近底部时自动续拉，行为对齐 legado 的滚动加载。 */
function handleResultsScroll() {
  const el = resultsContainerRef.value
  if (!el || isSearching.value) return
  const { scrollTop, scrollHeight, clientHeight } = el
  if (scrollTop + clientHeight >= scrollHeight - 160) loadMore()
}

function closeEventSource() {
  if (eventSource) {
    eventSource.close()
    eventSource = null
  }
}

/** 用户主动重搜当前条件（丢掉这份缓存）。 */
function refreshSearch() {
  const key = shelfStore.searchKey
  if (!key) return
  const sessionKey = currentSearchSessionKey(key)
  // 只丢当前这一组条件的结果；别把其它关键词的缓存也清掉。
  if (sessionKey) searchSessionCache.delete(sessionKey)
  doSearch(key, true)
}

/**
 * 「返回书架」= 主动结束这次搜索。
 *
 * 与「进阅读页」不同：那是临时离开，结果要留着（对齐 Legado 搜索页留在返回栈里）；
 * 用户点返回书架就是这次搜索看完了，缓存没有继续留着的意义 —— 下次再搜同一个词
 * 本来就该拿最新结果，而不是先给一份旧列表。
 *
 * 清「搜索会话缓存」统一由 `shelfStore.clearSearch()` 负责（顶栏的「清空搜索」
 * 也走同一条路径）；**封面缓存必须在这里释放** —— `onUnmounted` 为了支持
 * 「进阅读页再返回」故意不释放，如果这里也不放，离开搜索页后这批封面图就再也
 * 没有回收点了（后端 `public/cover` 磁盘缓存会一直留着）。
 */
function backToShelf() {
  resultsFromCache.value = false
  releaseSearchCovers()
  emit('back')
}

/** Release the previous search's covers without blocking the new search. */
function releaseSearchCovers() {
  coverCacheSession.release(shelfStore.books)
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

/** 当前搜索条件对应的缓存键。 */
function currentSearchSessionKey(key = shelfStore.searchKey): string {
  return searchSessionKey({
    keyword: key,
    scope: searchScope.value,
    group: selectedGroup.value,
    sourceUrl: selectedSourceUrl.value,
  })
}

/**
 * 填充搜索结果。
 *
 *  `force = false`（打开搜索页 / 回到搜索页）：命中会话缓存就直接铺出来，
 *  **不发任何请求** —— 从阅读页返回时不该重新搜一遍。
 *  `force = true`（改关键词/改范围/点刷新）：真正发请求，并覆盖缓存。
 */
function doSearch(key: string, force = false) {
  closeEventSource()
  // 这里的自增只是为了**作废在途回调**（关流前发出去的消息不该再写状态）。
  // 真正的 `generation` 在 `streamSearch()` 里再取一次，两处语义不同，别合并。
  searchGeneration += 1
  searchLastIndex = -1
  searchPage.value = 1
  hasMorePages.value = false
  hasMoreSources.value = false
  pageHadResults = false

  if (searchScope.value === 'group' && !selectedGroup.value) {
    releaseSearchCovers()
    shelfStore.searchResults = []
    shelfStore.isSearching = false
    return
  }

  if (searchScope.value === 'source' && !selectedSourceUrl.value) {
    releaseSearchCovers()
    shelfStore.searchResults = []
    shelfStore.isSearching = false
    return
  }

  const sessionKey = currentSearchSessionKey(key)
  if (!force && sessionKey) {
    const cached = searchSessionCache.get(sessionKey)
    if (cached) {
      // 直接复用离开前的结果：用户从阅读页退回时看到的还是同一批书，
      // 不会又等一次全量搜索、也不会因为批次没回来而少一半。
      // **不能在这里 releaseSearchCovers()** —— 那会把刚复用的封面也清掉，
      // 结果页会整屏重新下载图片。
      shelfStore.searchResults = cached.results.slice()
      resultsFromCache.value = true
      // 断点要在续搜前恢复：`streamSearch` 里每次落库都读它。
      searchLastIndex = cached.lastIndex
      // 旧缓存条目没有这些字段（热更新/老进程内条目），不能直接参与
      // Math.max —— `Math.max(1, undefined)` 是 NaN，会让请求页码变成 NaN。
      const cachedPage = Number.isFinite(cached.page) ? Math.floor(cached.page) : 1
      searchPage.value = Math.max(1, cachedPage)
      hasMorePages.value = Boolean(cached.morePages)
      hasMoreSources.value = Boolean(cached.moreSources)
      // 续搜只覆盖剩余书源: 之前批次已拿到的结果无法从缓存字段区分,
      // 按「有」处理, 避免剩余批次恰好为空时误判整页无结果。
      const resumedMidPage = !cached.complete && cached.lastIndex >= 0 && hasMoreSources.value
      pageHadResults = resumedMidPage || hasMorePages.value
      // 未完成 + 已过保质期：断点续搜没有意义了（那个 key 是对旧书源列表算的，
      // 隔太久书源可能已经增删过），直接按新搜索重跑，别拿过期结果垫底。
      const expiredAndIncomplete = !cached.complete && searchSessionCache.isStale(cached)
      if (cached.complete) {
        shelfStore.isSearching = false
      } else if (expiredAndIncomplete) {
        searchSessionCache.delete(sessionKey)
        resultsFromCache.value = false
        shelfStore.searchResults = []
        shelfStore.isSearching = true
        searchLastIndex = -1
        searchPage.value = 1
        hasMorePages.value = false
        hasMoreSources.value = false
        pageHadResults = false
        streamSearch(key, sessionKey, -1, 1)
      } else {
        // 上次还没搜完就离开了（典型：搜索进行中点书进阅读页）。
        // 手机端这时搜索仍在 SearchViewModel 里继续跑，回来接着看即可；
        // 本项目的搜索页组件会被卸载、流也随之关闭，所以这里**续搜**：
        // 先把已有结果摆出来，再从断点接着要后面的批次，而不是从头再来。
        shelfStore.isSearching = true
        streamSearch(key, sessionKey, cached.lastIndex, Math.max(1, cached.page))
      }
      nextTick(() => {
        resultsContainerRef.value?.scrollTo({ top: 0 })
        // 缓存里可能还有下一页且结果不足一屏: 没有分页按钮后, 只能靠
        // 这里补一次触底判断。
        handleResultsScroll()
      })
      return
    }
  }

  // 走到这里才是真正的重新搜索: 上一批结果的封面此时可以释放了。
  releaseSearchCovers()
  resultsFromCache.value = false

  shelfStore.searchResults = []
  shelfStore.isSearching = true
  streamSearch(key, sessionKey, -1, 1)
}

/**
 * 跑一轮书源搜索（从 `startIndex` 开始，-1 表示第一页）。
 *
 * 抽成独立函数是因为两个入口都要用：全新搜索，以及「上次没搜完」的续搜。
 *
 * 与 Legado 的对照：手机端搜索跑在 `SearchViewModel` + `SearchModel` 里，页面
 * 离开只是 `pause()`，回来 `resume()` 接着推。这里没有常驻 ViewModel，只能在
 * 重新挂载时根据缓存里的 `lastIndex` 续上 —— 用户看到的连续性是一样的。
 */
function streamSearch(key: string, sessionKey: string, startIndex: number, pageNo = searchPage.value) {
  const generation = ++searchGeneration

  const stream = searchBookMultiSSE({
    key,
    page: pageNo,
    concurrentCount: 24,
    bookSourceGroup: searchScope.value === 'group' ? selectedGroup.value : undefined,
    bookSourceUrl: searchScope.value === 'source' ? selectedSourceUrl.value : undefined,
    lastIndex: startIndex,
  })
  eventSource = stream

  const isCurrentSearch = () => (
    generation === searchGeneration
    && eventSource === stream
    && shelfStore.searchKey === key
  )

  /** 落缓存。
   *
   *  **每收到一批就写一次**（而不是等 end）：这样搜索进行中进阅读页、组件卸载，
   *  已经拿到的结果也不会丢 —— 这正是「搜索没完就离开、回来又要重搜」的根因。
   *  手机端是靠 ViewModel 常驻实现的，这里靠边收边存。
   *
   *  `complete` 为 false 时记下 `lastIndex`，回来可以续搜。
   *
   *  **不能复用 `isCurrentSearch()`**：它含 `eventSource === stream`，而调用点
   *  多在 `eventSource = null` 之后，一判就 false —— 缓存会静默地写不进去。 */
  const commitSession = (
    complete: boolean,
    lastIndex: number,
    page = searchPage.value,
    morePages = hasMorePages.value,
    moreSources = hasMoreSources.value,
  ) => {
    if (!sessionKey || generation !== searchGeneration) return
    if (shelfStore.searchKey !== key) return
    searchSessionCache.set(
      sessionKey,
      key,
      shelfStore.searchResults.slice(),
      complete,
      lastIndex,
      page,
      morePages,
      moreSources,
    )
  }

  stream.onmessage = (event) => {
    if (!isCurrentSearch()) return
    try {
      const data = event.data as { data?: SearchBook[]; lastIndex?: number }
      if (data.data && Array.isArray(data.data)) {
        if (data.data.length > 0) pageHadResults = true
        const byKey = new Map(shelfStore.searchResults.map((r) => [searchMergeKey(r), r]))
        coverCacheSession.collect(data.data)
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
        // 边收边存：中途离开也不至于什么都没有。
        const nextIndex = typeof data.lastIndex === 'number' ? data.lastIndex : searchLastIndex
        // 书源按完成顺序到达, 断点只能前进不能回退。
        searchLastIndex = Math.max(searchLastIndex, nextIndex)
        commitSession(false, searchLastIndex, searchPage.value, false)
      }
    } catch { /* skip */ }
  }

  stream.addEventListener('end', (event) => {
    if (!isCurrentSearch()) return
    const payload = event.data as {
      lastIndex?: number
      hasMore?: boolean
      hasMorePages?: boolean
    }
    if (typeof payload?.lastIndex === 'number') {
      searchLastIndex = Math.max(searchLastIndex, payload.lastIndex)
    }
    shelfStore.isSearching = false
    stream.close()
    eventSource = null
    // 单次续拉只覆盖部分书源时, `hasMorePages` 仅代表这一批; 同一页
    // 可能分多批扫完, 所以只要本页任一批有结果就算“这页还有下一页”。
    // 与 legado `SearchModel` 的 `hasMore = hasMore || items.isNotEmpty()`
    // 一致: 搜索结果不做“重复页”推断, 空页才停。
    hasMorePages.value = pageHadResults || payload?.hasMorePages === true
    hasMoreSources.value = payload?.hasMore === true
    // 还有更多（后端分页）时不能标 complete，否则回来不会再续。
    // 「这一页的书源已经全部扫完」就是一个可复用的完整快照；是否还有
    // 下一页由 hasMorePages 单独携带，不应让缓存一直处于 incomplete 而
    // 在从阅读页返回时多发一次空请求。
    commitSession(!hasMoreSources.value, searchLastIndex)
    // 结果不足一屏时滚动事件不会触发, 这里主动补一次加载, 否则用户
    // 看不到任何继续加载的入口。
    nextTick(() => handleResultsScroll())
  })

  stream.addEventListener('error', () => {
    if (!isCurrentSearch()) return
    shelfStore.isSearching = false
    stream.close()
    eventSource = null
    commitSession(false, searchLastIndex)
  })

  stream.onerror = () => {
    if (!isCurrentSearch()) return
    shelfStore.isSearching = false
    stream.close()
    eventSource = null
    commitSession(false, searchLastIndex)
  }
}

/**
 * 是否还允许读会话缓存。
 *
 * `immediate: true` 的首次触发代表「进入/返回搜索页」—— 这时优先复用上次结果，
 * 这是本次改动的核心（从阅读页退回不该重搜）。之后任何一次触发都源于用户改了
 * 关键词/范围，那本来就该重新搜。
 *
 * 每个组件实例初始为 true：从阅读页返回时组件是新建的，于是又会先读缓存。
 *
 * 注意不能一进 watch 就把它置 false：`ensureSearchSelection()` 会在书源列表
 * 到位后补一个 group/source 选项，那会再触发一次本 watch。若那时已经置 false，
 * 首次挂载就变成了"强制重搜"，缓存等于没用。所以只在**真正发起搜索**后才
 * 关闭，并且用 `lastSearchedSessionKey` 让同一条件不重复搜。
 */
let mayUseSessionCache = true
/** 上一次真正发起请求（或命中缓存）所对应的条件键，用于去重。 */
let lastSessionKey = ''

watch(
  [() => shelfStore.searchKey, searchScope, selectedGroup, selectedSourceUrl],
  ([key]) => {
    ensureSearchSelection()
    if (key) {
      const sessionKey = currentSearchSessionKey(key)
      // 同一条件被重复触发（ensureSearchSelection 补齐选项导致的重入）时直接
      // 跳过：结果已经在屏幕上了，没有任何理由再搜一次。
      if (sessionKey && sessionKey === lastSessionKey) return
      doSearch(key, !mayUseSessionCache)
      mayUseSessionCache = false
      lastSessionKey = sessionKey
    } else {
      lastSessionKey = ''
      mayUseSessionCache = true
      searchGeneration += 1
      closeEventSource()
      shelfStore.searchResults = []
      shelfStore.isSearching = false
      resultsFromCache.value = false
    }
  },
  { immediate: true }
)

watch([searchScope, sourceGroups, sourceOptions], () => {
  ensureSearchSelection()
}, { immediate: true })

/**
 * 是否需要因为「书源清单变化」而重搜。
 *
 * 两个坑都要避开：
 * 1. `onMounted` 里 `fetchSources(true)` 是**每次都拉**，首次挂载时签名会从
 *    空串变成完整清单 —— 若直接重搜，刚命中缓存的页面立刻又发起一次全量
 *    搜索，「返回不重搜」当场失效。
 * 2. 书源列表本来就还没加载完（签名是空）时也不能当成"变了"。
 *
 * 所以只在「两边都非空、且真的不同」时才重搜。
 */
let mountedSourceSignature = ''

watch(enabledSourceSignature, (next, previous) => {
  const baseline = mountedSourceSignature
  if (!shouldResearchForSourceChange({
    hasSearchKey: Boolean(searchKey.value),
    previous,
    next,
    baseline,
  })) {
    // 首次拿到清单时把基线补上，供后续比较。
    if (next && !previous) mountedSourceSignature = next
    return
  }
  mountedSourceSignature = next
  // 书源真的增删/停启了：缓存里可能有已删源的条目，新启用的源也可能搜出更多
  // 结果。这条不受会话缓存影响，必须重搜。
  doSearch(searchKey.value, true)
})

onMounted(async () => {
  // Always refresh the source list on mount so the group/source dropdowns
  // reflect any add/delete/edit done in the source manager since last visit.
  await sourceStore.fetchSources(true).catch(() => undefined)
  ensureSearchSelection()
  mountedSourceSignature = enabledSourceSignature.value
})

onUnmounted(() => {
  // 卸载前补一次落库：虽然每收到一批就写过了，但「最后一批刚 push 完就切走」
  // 这种时刻，响应式赋值与写缓存之间可能还没轮到执行 —— 补这一次能兜住。
  // 此刻仍然 `isSearching` 说明搜索没跑完，标 incomplete 让回来时续搜。
  const key = shelfStore.searchKey
  if (key) {
    const sessionKey = currentSearchSessionKey(key)
    if (sessionKey && shelfStore.searchResults.length) {
      searchSessionCache.set(
        sessionKey,
        key,
        shelfStore.searchResults.slice(),
        !shelfStore.isSearching
          && searchLastIndex >= 0
          && !hasMoreSources.value,
        searchLastIndex,
        searchPage.value,
        hasMorePages.value,
        hasMoreSources.value,
      )
    }
  }
  searchGeneration += 1
  closeEventSource()
  // 封面缓存要不要释放，看**这次卸载是不是搜索结束**：
  // - `searchKey` 已清空 = `clearSearch()`（返回书架 / 顶栏清空搜索）导致的卸载，
  //   这轮搜索结束了，封面没有留着的理由 → 释放，否则这批图再没有回收点。
  // - `searchKey` 还在 = 进阅读页/详情页的临时离开，组件会重建。此时释放会让
  //   返回后整屏几十上百张封面重新下载（看起来像"又搜了一遍"，还更卡）→ 保留。
  if (!shelfStore.searchKey) releaseSearchCovers()
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
    || shelfBookKeys.value.has(shelfMatchKey(book))
  const menuItems = [
    { label: '查看详情', action: () => handleBookInfo(book) },
    { divider: true },
    inShelf
      ? { label: '已在书架', disabled: true }
      : { label: '加入书架', action: () => handleAddToShelf(book) },
  ]
  showContextMenu(event, menuItems, book)
}
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

/* 「缓存」角标：标出结果来自上次搜索，不是刚搜的；点它重新搜索。
   原来是单独一行提示条，既多占一行又容易重复渲染，收成一枚角标。 */
.cache-badge {
  display: inline-block;
  margin-left: var(--space-2);
  padding: 1px 6px;
  border: 1px solid var(--color-border-light);
  border-radius: var(--radius-full);
  background: transparent;
  color: var(--color-text-tertiary);
  font-family: inherit;
  font-size: var(--text-xs);
  font-weight: 400;
  line-height: 1.6;
  vertical-align: middle;
  cursor: pointer;
  transition: color var(--duration-fast), border-color var(--duration-fast);
}

.cache-badge:hover {
  color: var(--color-primary);
  border-color: var(--color-primary-border);
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
  color: var(--color-primary-soft-text);
  background: var(--color-primary-soft-bg);
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
