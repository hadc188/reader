<template>
  <div class="reader-source" :style="{ background: theme.popup, color: theme.fontColor }">
    <div class="source-header">
      <div class="header-left">
        <h3>切换书源</h3>
        <span class="source-count" v-if="preparedResults.length">{{ preparedResults.length }} 个结果</span>
      </div>
      <div class="header-actions">
        <!-- 与手机端换源弹窗的 menu_start_stop 对齐: 搜索中显示「停止」,
             结束后显示「刷新」。此前只有搜完(或被判定失败)才能重新开始,
             某个源卡住时用户只能干等全部源超时。 -->
        <button
          v-if="searching"
          class="icon-btn"
          type="button"
          title="停止搜索"
          aria-label="停止搜索"
          @click="stopSearch"
        >
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round">
            <rect x="7" y="7" width="10" height="10" rx="1.5" />
          </svg>
        </button>
        <button
          v-else
          class="icon-btn"
          type="button"
          title="重新搜索书源"
          aria-label="重新搜索书源"
          :disabled="isLocal"
          @click="startSearch(true)"
        >
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round">
            <path d="M21 12a9 9 0 1 1-2.64-6.36M21 3v6h-6" />
          </svg>
        </button>
        <button class="close-btn" type="button" title="关闭" aria-label="关闭" @click="store.closePanel()">
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round">
            <path d="M6 6l12 12M18 6L6 18" />
          </svg>
        </button>
      </div>
    </div>
    
    <div class="source-list" ref="listRef">
      <!-- Current Source Header -->
      <div class="section-label" v-if="currentSource">当前书源</div>
      <div v-if="currentSource" class="source-item current">
        <div class="source-name">{{ currentSource.originName || currentSource.origin }}</div>
        <div class="source-url">{{ currentSource.origin }}</div>
      </div>

      <!--
        当前书籍概览：只留判断换源需要的最小信息（封面/书名/作者/读到哪/最新章）。
        书源脚本返回的 `intro` 是一整块信息（最新章、点击、评分、上次更新…），
        铺在这里既长又和上面几行重复，换源时没有决策价值 —— 直接不显示。
      -->
      <div v-if="store.book" class="book-brief">
        <div class="book-brief-cover">
          <img v-if="store.book.coverUrl" :src="getCoverUrl(store.book.coverUrl, store.book.origin)" :alt="store.book.name">
          <div v-else class="book-brief-placeholder">{{ store.book.name.slice(0, 1) }}</div>
        </div>
        <div class="book-brief-main">
          <div class="book-brief-title">{{ store.book.name }}</div>
          <div class="book-brief-meta">{{ store.book.author || '未知作者' }}</div>
          <div class="book-brief-meta" v-if="store.currentChapter?.title">当前章节：{{ store.currentChapter.title }}</div>
          <div class="book-brief-meta" v-if="store.book.latestChapterTitle">最新章节：{{ store.book.latestChapterTitle }}</div>
        </div>
      </div>

      <div v-if="isLocal" class="empty">本地书籍不支持换源</div>

      <template v-else>
      <div class="section-label">其他可用源</div>

      <div v-if="searching && !preparedResults.length" class="loading">
        <div class="spinner"></div>
        正在全网搜索同名书籍...
      </div>

      <div v-else-if="!preparedResults.length && !sourcesIncomplete" class="empty">未找到其他书源</div>
      
      <!--
        候选列表：紧凑的一行一源。
        旧版把简介正文、书名、作者、最新章、更新时间、三项匹配标签、独立的
        「切换」按钮全部堆进每一行，多源时列表被撑成几十屏，同一个书名还要
        重复几十遍 —— 那正是"看着不美观"的来源。这里按手机端
        `item_change_source.xml` 的信息密度收拢：主行只留书源名 + 关键状态，
        次行给「最新章节 · 更新时间」，其余细节放下方的选中详情里按需展示。
      -->
      <div class="candidate-list" v-else>
        <button
          v-for="item in preparedResults"
          :key="sourceCandidateKey(item.book)"
          type="button"
          class="candidate"
          :class="{ active: selectedCandidate?.book.bookUrl === item.book.bookUrl && selectedCandidate?.book.origin === item.book.origin }"
          @click="selectCandidate(item)"
        >
          <span class="candidate-rail" aria-hidden="true"></span>
          <span class="candidate-body">
            <span class="candidate-title-row">
              <span class="candidate-name">{{ sourceNameByOrigin(item.book.origin) }}</span>
              <span v-if="item.sameLatest" class="candidate-badge good">最新章一致</span>
              <span v-else-if="item.book.lastChapter" class="candidate-badge">章节有差异</span>
              <span v-if="!item.sameName" class="candidate-badge warn">书名不完全一致</span>
            </span>
            <span class="candidate-meta">
              <span v-if="item.book.author" class="candidate-author">{{ item.book.author }}</span>
              <span v-if="item.book.lastChapter" class="candidate-latest" :title="item.book.lastChapter">
                {{ item.book.lastChapter }}
              </span>
            </span>
          </span>
          <span class="candidate-go" aria-hidden="true">
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
              <path d="m9 6 6 6-6 6" />
            </svg>
          </span>
        </button>
      </div>

      <div v-if="searching && preparedResults.length" class="inline-loading">
        <div class="spinner small"></div>
        继续搜索其他书源...
      </div>
      </template>

      <!--
        部分书源失败：一条提示 + 一个明确动作。
        改版前是 `justify-content: space-between` 且没有左右内边距 —— 文字贴在
        列表左边缘、按钮贴到右边缘，两者之间空出一大段，看起来像两件无关的东西。
        现在收成一个带底色的小卡：图标 + 说明在左，重试按钮紧跟其后（不是顶到最右），
        左右边距与候选列表的 12px 对齐。
      -->
      <div v-if="sourcesIncomplete && !searching && !loadingMore" class="source-incomplete" role="status">
        <svg class="incomplete-icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round">
          <circle cx="12" cy="12" r="9" />
          <path d="M12 7.5v5.5M12 16.5h.01" />
        </svg>
        <span class="incomplete-text">部分书源访问失败</span>
        <button type="button" class="incomplete-retry" @click="startSearch(true)">重试</button>
      </div>
      <div v-if="hasMoreSources && !searching" class="load-more-wrap">
        <button class="load-more-btn" :disabled="loadingMore" @click="loadMoreSources">
          {{ loadingMore ? '加载中...' : '加载更多' }}
        </button>
      </div>
    </div>

    <!--
      选中书源的详情与「切换」动作：固定在底部，不随列表滚动。
      旧版把这块当普通内容放在列表末尾，选中任意一项都要滚到底才能点到
      「切换到此书源」；源多时等于把主操作藏起来。

      版式收成两行：列表里已经能看到书源名/作者/最新章，这里再铺三列事实
      加一段简介会把卡片撑到接近半屏，反而挡住用户正在挑的那几个源。
      只留「书源名（+作者/更新时间）」和「最新章节 · 衔接状态」+ 按钮。
    -->
    <Transition name="detail-slide">
      <div v-if="selectedCandidate && !isLocal" class="candidate-detail">
        <div class="detail-row detail-row-main">
          <span class="detail-name">{{ sourceNameByOrigin(selectedCandidate.book.origin) }}</span>
          <span class="detail-sub">{{ detailSubLine }}</span>
          <button class="detail-dismiss" type="button" title="收起" aria-label="收起" @click="clearSelection">
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
              <path d="M18 6 6 18M6 6l12 12" />
            </svg>
          </button>
        </div>

        <div class="detail-row detail-row-action">
          <span class="detail-latest" :title="detailLatestLine">{{ detailLatestLine }}</span>
          <button
            class="switch-btn primary"
            type="button"
            :disabled="store.loading"
            @click="handleSwitch(selectedCandidate.book)"
          >切换到此书源</button>
        </div>
      </div>
    </Transition>

    <!-- Switching Overlay -->
    <Transition name="fade">
      <div v-if="store.loading" class="switch-overlay" :style="{ background: theme.popup }">
        <div class="spinner"></div>
        <p>正在切换书源，请稍候...</p>
      </div>
    </Transition>
  </div>
</template>

<script setup lang="ts">
import { ref, computed, onMounted, onUnmounted, watch } from 'vue'
import { useReaderStore } from '../../stores/reader'
import { useAppStore } from '../../stores/app'
import { useSourceStore } from '../../stores/source'
import { getAvailableBookSourceSSE } from '../../api/search'
import type { SseLike } from '../../api/sse'
import { getBookInfo, getCoverUrl } from '../../api/bookshelf'
import { isLocalBook } from '../../utils/localBook'
import { matchesSourceSwitchTitle, mergeSourceCandidates, normalizeSourceSwitchAuthor, sourceCandidateKey } from '../../utils/searchRank'
import {
  getSourceCandidateCache,
  mergeForCommit,
  sourceCandidateBookKey,
  sourceCatalogSignatureOf,
} from '../../utils/sourceCandidateCache'
import type { Book, SearchBook } from '../../types'

type CandidateItem = {
  book: SearchBook
  sameName: boolean
  sameAuthor: boolean
  sameLatest: boolean
  chapterHint: string
  score: number
}

const store = useReaderStore()
const appStore = useAppStore()
const sourceStore = useSourceStore()
const theme = computed(() => store.chromeTheme)
const isLocal = computed(() => isLocalBook(store.book))
const searching = ref(false)
const sourcesIncomplete = ref(false)
const loadingMore = ref(false)
const results = ref<SearchBook[]>([])
const lastIndex = ref(-1)
const hasMoreSources = ref(true)
const selectedCandidate = ref<CandidateItem | null>(null)
const candidatePreview = ref<Book | null>(null)
const AVAILABLE_CONCURRENT_COUNT = 16
let availableSourceSSE: SseLike | null = null

/**
 * 与详情页共用同一个持久化缓存（issue #28 第 2 点）。
 *
 * 两处面板读的是同一份 localStorage：在详情页刷出来的结果，进阅读页换源时
 * 可以直接用，不必再并发搜一遍；反过来也成立。键里带书源清单签名，源库改了
 * 就自动失效重搜。
 */
const sourceCandidateCache = getSourceCandidateCache()

const currentSource = computed(() => {
  if (!store.book) return null
  return {
    origin: store.book.origin,
    originName: store.book.originName
  }
})

function sourceNameByOrigin(origin: string): string {
  const found = sourceStore.sources.find((s) => sourceKey(s.bookSourceUrl) === sourceKey(origin))
  return found?.bookSourceName || origin
}

/** 与 sourceNameByOrigin 相同的比较口径: 去掉首尾空白与末尾斜杠。 */
function sourceKey(origin: string): string {
  const normalized = (origin || '').trim()
  if (!normalized) return ''
  return normalized.replace(/\/+$/, '')
}

/** 已启用书源的 origin 集合; 书源列表还没拿到时返回 null(此时不做过滤)。 */
const enabledSourceOrigins = computed<Set<string> | null>(() => {
  if (!sourceStore.sources.length) return null
  return new Set(
    sourceStore.sources
      .filter((source) => source.enabled !== false)
      .map((source) => sourceKey(source.bookSourceUrl)),
  )
})

function normalizeText(value?: string) {
  return (value || '')
    .replace(/\s+/g, '')
    .replace(/[：:,.，。！？!?\-—_()（）【】\[\]<>《》'"“”‘’]/g, '')
    .toLowerCase()
}

function normalizeAuthorText(value?: string) {
  return normalizeSourceSwitchAuthor(value)
}

const preparedResults = computed<CandidateItem[]>(() => {
  if (!store.book) return []
  const currentName = normalizeText(store.book.name)
  const currentAuthor = normalizeAuthorText(store.book.author)
  const currentLatest = normalizeText(store.book.latestChapterTitle || store.currentChapter?.title)

  return results.value
    .map((book) => {
      const sameName = normalizeText(book.name) === currentName
      const sameAuthor = normalizeAuthorText(book.author) === currentAuthor
      const sameLatest = !!currentLatest && normalizeText(book.lastChapter) === currentLatest
      const chapterHint = sameLatest
        ? '可无缝续读'
        : (book.lastChapter ? `目标源最新：${book.lastChapter}` : '')
      const score = (sameName ? 3 : 0) + (sameAuthor ? 3 : 0) + (sameLatest ? 4 : 0)
      return { book, sameName, sameAuthor, sameLatest, chapterHint, score }
    })
    .sort((a, b) => b.score - a.score)
})

onMounted(async () => {
  // 先把缓存/已有候选**同步**铺出来，再刷新书源清单。
  // 早先是 await 完才 hydrate，IPC 那段时间面板是空的 —— 与详情页同样的
  // 「内容闪一下才出现」观感。
  hydrateFromCache()
  // 每次打开都刷新一遍书源清单：候选会被 `enabledSourceOrigins` 过滤，
  // 沿用过期清单会把刚启用的源误剔除。
  await sourceStore.fetchSources().catch(() => undefined)
  // `watch(enabledSourceOrigins)` 会在清单到位后重新过滤并补选首项，
  // 这里不用再做一遍。
})

// 首次进入应用直接打开阅读页时, 书源列表往往晚于本面板就绪: 那一次合并因为
// 还不知道启用集合而放行了全部候选。等列表到位后重做一次, 把已经删除或禁用
// 的书源从列表中剔除, 并保住仍然有效的选择。
watch(enabledSourceOrigins, (enabled) => {
  if (!enabled) return
  results.value = results.value.filter((item) => enabled.has(sourceKey(item.origin)))
  const selected = selectedCandidate.value
  if (selected && enabled.has(sourceKey(selected.book.origin))) return
  selectedCandidate.value = null
  candidatePreview.value = null
  // 搜索已经结束时不会再收到 payload, 这里自己补选首项, 免得对照面板就此消失。
  if (!searching.value && preparedResults.value.length) {
    void selectCandidate(preparedResults.value[0])
  }
  if (store.book) mergeCandidates(store.book.sourceCandidates || [])
})

onUnmounted(() => {
  closeAvailableSourceSSE()
})

/** 打开面板 / 搜索书源。
 *
 *  `refresh = false`（打开面板）：先读缓存，不发请求；书源清单变过才自动重搜。
 *  `refresh = true`（点刷新 / 重试）：强制并发搜索，并保留旧结果垫底。
 */
function startSearch(refresh = false) {
  if (!store.book || isLocal.value) return
  closeAvailableSourceSSE()
  // 与详情页同一口径：打开就搜，旧缓存只作为垫底先显示出来。
  // 之前靠「签名是否变化」判断要不要搜，但签名只描述当时那份书源清单，
  // 缓存里若只含部分源、而清单恰好没变过，就会永远显示残缺结果。
  const cached = sourceCandidateCache.get(sourceCandidateCacheKey())

  searching.value = true
  sourcesIncomplete.value = false
  loadingMore.value = false
  results.value = []
  lastIndex.value = -1
  hasMoreSources.value = true
  selectedCandidate.value = null
  candidatePreview.value = null
  mergeCandidates(store.book.sourceCandidates || [])
  // 旧缓存垫在下面：一边搜一边还在显示上次的结果，不会瞬间清空。
  if (cached) mergeCandidates(cached.books)

  openAvailableSourceSSE('initial', refresh)
}

/** 当前书籍的缓存键。与详情页共用 `sourceCandidateBookKey`，口径不会漂移。 */
function sourceCandidateCacheKey(): string {
  const book = store.book
  if (!book) return ''
  return sourceCandidateBookKey({
    name: book.name,
    author: book.author,
    bookUrl: book.bookUrl,
    origin: book.origin,
  })
}

/** 当前启用书源的签名。与详情页共用同一实现。 */
function sourceCatalogSignature(): string {
  return sourceCatalogSignatureOf(sourceStore.sources)
}

/** 与详情页一致的落库规则：完整搜完就替换；中断/部分失败与旧缓存合并。 */
function commitResults(complete: boolean, previous: SearchBook[]) {
  const key = sourceCandidateCacheKey()
  if (!key) return
  const merged = mergeForCommit(previous, results.value, complete, mergeSourceCandidates)
  if (merged.length <= 1) return
  sourceCandidateCache.set(key, sourceCatalogSignature(), merged, complete)
}

/** 打开面板时的入口：就是「不刷新地 startSearch()」。 */
function hydrateFromCache() {
  startSearch()
}

function mergeCandidates(candidates: SearchBook[]) {
  if (!store.book || !candidates.length) return
  const currentBook = store.book
  const currentKey = sourceCandidateKey(currentBook)
  // 书架保存的候选是换源当时的快照, 其中可能有后来已被删除或禁用的书源;
  // 只展示仍在启用列表里的源, 与详情页换源的口径一致。
  const enabled = enabledSourceOrigins.value
  results.value = mergeSourceCandidates(results.value, candidates.filter((item) => (
    sourceCandidateKey(item) !== currentKey
    && matchesSourceSwitchTitle(currentBook.name, item.name)
    && (!enabled || enabled.has(sourceKey(item.origin)))
  )))
}

type AvailableSourceMode = 'initial' | 'loadMore'
type AvailableSourceSSEPayload = {
  data?: SearchBook[]
  books?: SearchBook[]
  lastIndex?: number
  hasMore?: boolean
  hasErrors?: boolean
}

function closeAvailableSourceSSE() {
  if (!availableSourceSSE) return
  availableSourceSSE.close()
  availableSourceSSE = null
}

/** 停止当前搜索(对齐手机端 `ChangeBookSourceViewModel.stopSearch`)。
 *
 *  只关流不够: `searching` 必须复位, 否则"停止"后按钮还停在停止态、
 *  「继续搜索其他书源...」的转圈也永远不会消失。已拿到的结果保留 ——
 *  用户在搜索中途点停止, 想要的正是"就这些, 别再等了"。 */
function stopSearch() {
  closeAvailableSourceSSE()
  searching.value = false
  loadingMore.value = false
  hasMoreSources.value = false
  // 中断也要落缓存（与详情页一致）：用户刷到 6 个源后点停止，下次打开面板
  // 必须还是这 6 个，不能退回书架里那 3 个旧候选。`complete = false` 让
  // 「重试」入口继续可见。
  const cached = sourceCandidateCache.get(sourceCandidateCacheKey())
  commitResults(false, cached?.books || [])
  if (!selectedCandidate.value && preparedResults.value.length) {
    void selectCandidate(preparedResults.value[0])
  }
}

function parseAvailableSourcePayload(event: { data: unknown }): AvailableSourceSSEPayload | null {
  try {
    return event.data as AvailableSourceSSEPayload
  } catch (error) {
    console.error('parse getAvailableBookSourceSSE payload failed', error)
    return null
  }
}

function applyAvailableSourcePayload(payload: AvailableSourceSSEPayload | null) {
  if (!payload) return
  sourcesIncomplete.value ||= payload.hasErrors === true
  const incoming = Array.isArray(payload.data)
    ? payload.data
    : (Array.isArray(payload.books) ? payload.books : [])

  if (typeof payload.lastIndex === 'number') {
    lastIndex.value = payload.lastIndex
  }
  if (typeof payload.hasMore === 'boolean') {
    hasMoreSources.value = payload.hasMore
  }
  mergeCandidates(incoming)

  if (!selectedCandidate.value && preparedResults.value.length) {
    void selectCandidate(preparedResults.value[0])
  }
}

function openAvailableSourceSSE(mode: AvailableSourceMode, refresh = false) {
  if (!store.book) return

  const beforeCount = results.value.length
  // 这一轮开始前已有的候选（可能是旧缓存）。中断/失败时用它兜底合并。
  const previousResults = results.value.slice()
  const stream = getAvailableBookSourceSSE({
    url: store.book.bookUrl,
    name: store.book.name,
    author: store.book.author,
    origin: store.book.origin,
    lastIndex: mode === 'loadMore' ? lastIndex.value : -1,
    concurrentCount: AVAILABLE_CONCURRENT_COUNT,
    refresh: refresh ? 1 : undefined,
  })
  availableSourceSSE = stream

  stream.onmessage = (event) => {
    if (availableSourceSSE !== stream) return
    applyAvailableSourcePayload(parseAvailableSourcePayload(event))
    // 每搜到一批就落一次缓存（标记未完成）：用户搜到一半直接关面板/退出应用时
    // 磁盘上不会还停留在搜索前那份旧缓存。跑完后 `finishAvailableSourceSSE`
    // 会用权威结果覆盖成 `complete = true`。
    commitResults(false, previousResults)
  }

  stream.addEventListener('end', (event) => {
    if (availableSourceSSE !== stream) return
    applyAvailableSourcePayload(parseAvailableSourcePayload(event))
    finishAvailableSourceSSE(stream, mode, beforeCount, false, previousResults)
  })

  stream.onerror = (event) => {
    if (availableSourceSSE !== stream) return
    console.error('getAvailableBookSourceSSE failed', event)
    finishAvailableSourceSSE(stream, mode, beforeCount, true, previousResults)
  }
}

function finishAvailableSourceSSE(
  stream: SseLike,
  mode: AvailableSourceMode,
  beforeCount: number,
  failed = false,
  previousResults: SearchBook[] = [],
) {
  if (availableSourceSSE !== stream) return
  stream.close()
  availableSourceSSE = null

  if (mode === 'initial') {
    searching.value = false
    // 完整结束且没有源访问失败 → 用这一轮结果替换缓存；否则与旧缓存合并，
    // 保住用户已经看到的那几个源（与详情页同一套规则）。
    commitResults(!failed && !sourcesIncomplete.value, previousResults)
  } else {
    loadingMore.value = false
    // 加载更多只是往同一份结果里追加，成功后顺带更新缓存。
    if (!failed) commitResults(true, [])
  }

  if (!selectedCandidate.value && preparedResults.value.length) {
    void selectCandidate(preparedResults.value[0])
  }

  if (failed) {
    sourcesIncomplete.value = true
    hasMoreSources.value = false
    if (mode === 'loadMore') {
      appStore.showToast('加载更多书源失败', 'error')
    }
    return
  }

  if (mode === 'loadMore') {
    const addedCount = results.value.length - beforeCount
    if (addedCount > 0) {
      appStore.showToast(`已新增 ${addedCount} 个书源`, 'success')
    } else if (sourcesIncomplete.value) {
      return
    } else if (!hasMoreSources.value) {
      appStore.showToast('没有更多书源了', 'warning')
    } else {
      appStore.showToast('本批次未找到更多匹配书源', 'warning')
    }
  }
}

async function selectCandidate(item: CandidateItem) {
  selectedCandidate.value = item
  candidatePreview.value = null
  try {
    candidatePreview.value = await getBookInfo(item.book.bookUrl, item.book.origin)
  } catch {
    candidatePreview.value = null
  }
}

/** 收起底部详情，回到纯列表。选中态一并清掉，避免列表里留着高亮却看不到对应详情。 */
function clearSelection() {
  selectedCandidate.value = null
  candidatePreview.value = null
}

/**
 * 底部详情的副标题：作者 +（有的话）更新时间。
 * 只放一行，省略号截断 —— 卡片要尽量矮，正文信息在列表行里已经有了。
 */
const detailSubLine = computed(() => {
  const book = selectedCandidate.value?.book
  if (!book) return ''
  const author = (book.author || '未知作者').trim()
  const updated = (book.updateTime || '').trim()
  return updated ? `${author} · ${updated}` : author
})

/**
 * 底部详情的关键信息行：「最新章节 + 衔接状态」。
 *
 * 换源时用户真正关心的只有两件事 —— 目标源有没有最新章节、切过去会不会
 * 断章。这两件事合成一行就够，不必再摆成三列带 dt 标签的表格。
 */
const detailLatestLine = computed(() => {
  const candidate = selectedCandidate.value
  if (!candidate) return ''
  const latest = (candidatePreview.value?.latestChapterTitle || candidate.book.lastChapter || '').trim()
  const hint = candidate.sameLatest
    ? '可无缝续读'
    : (candidate.chapterHint || '')
  if (latest && hint) return `${latest} · ${hint}`
  return latest || hint || '最新章节未知'
})

function loadMoreSources() {
  if (!store.book || loadingMore.value || !hasMoreSources.value) return

  closeAvailableSourceSSE()
  loadingMore.value = true
  openAvailableSourceSSE('loadMore')
}

async function handleSwitch(res: SearchBook) {
  if (store.loading) return
  try {
    const nextBook = await store.switchSource(res.bookUrl, res.origin)
    store.closePanel()
    appStore.showToast(`已切换到 ${nextBook?.originName || nextBook?.origin || res.origin}`, 'success')
  } catch (e: any) {
    appStore.showToast(`切换失败: ${e?.message || '未知错误'}`, 'error')
  }
}
</script>

<style scoped>
.reader-source {
  display: flex;
  flex-direction: column;
  height: 100%;
  position: relative;
}

.source-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 16px 20px;
  border-bottom: 1px solid rgba(0,0,0,0.06);
  flex-shrink: 0;
}

.header-left { display: flex; align-items: baseline; gap: 8px; }
.source-header h3 { font-size: 16px; margin: 0; }
.source-count { font-size: 11px; opacity: 0.5; }

.header-actions { display: flex; align-items: center; gap: 2px; }

/* ─── 头部图标按钮 ───
   统一规格：30px 热区、16px 图标、1.8 描边、圆头线帽。
   之前是 32px + 17px + stroke-width 2，图标顶到边缘、线条又粗又长，
   放在「切换书源」这种细字标题旁边显得笨重。 */
.icon-btn,
.close-btn {
  width: 30px;
  height: 30px;
  display: flex;
  align-items: center;
  justify-content: center;
  padding: 0;
  border: none;
  border-radius: 8px;
  background: transparent;
  color: inherit;
  opacity: 0.55;
  cursor: pointer;
  transition: opacity 0.16s ease, background 0.16s ease;
}

.icon-btn svg,
.close-btn svg {
  width: 16px;
  height: 16px;
}

.icon-btn:hover:not(:disabled),
.close-btn:hover {
  opacity: 1;
  background: rgba(0, 0, 0, 0.05);
}

.icon-btn:active:not(:disabled),
.close-btn:active {
  background: rgba(0, 0, 0, 0.09);
}

.icon-btn:disabled {
  opacity: 0.25;
  cursor: not-allowed;
}

.icon-btn:focus-visible,
.close-btn:focus-visible {
  outline: 2px solid color-mix(in srgb, var(--color-primary, #c97f3a) 55%, transparent);
  outline-offset: 1px;
}

/* ─── 部分书源失败提示 ───
 * 一条轻量提示，不是列表项也不是两级页脚。左右边距与 .candidate-list 的 12px
 * 对齐，内部 12px 与候选卡的内容缩进一致；重试按钮跟在文字后面，不顶到最右 ——
 * 顶到最右会和上面的列表行抢视觉重心，看起来像一个孤立的次要操作。 */
.source-incomplete {
  display: flex;
  align-items: center;
  gap: 8px;
  margin: 4px 12px 10px;
  padding: 9px 10px 9px 12px;
  border-radius: 12px;
  background: color-mix(in srgb, currentColor 4%, transparent);
  border: 1px solid color-mix(in srgb, currentColor 8%, transparent);
  font-size: 12px;
  line-height: 1.4;
  color: color-mix(in srgb, currentColor 62%, transparent);
}

.incomplete-icon {
  flex: 0 0 auto;
  width: 14px;
  height: 14px;
  color: var(--color-primary, #c97f3a);
  opacity: 0.85;
}

.incomplete-text {
  min-width: 0;
  flex: 1;
}

.incomplete-retry {
  flex: 0 0 auto;
  padding: 4px 12px;
  border-radius: 8px;
  border: 1px solid color-mix(in srgb, var(--color-primary, #c97f3a) 34%, transparent);
  background: color-mix(in srgb, var(--color-primary, #c97f3a) 12%, transparent);
  color: inherit;
  font-size: 12px;
  font-weight: 500;
  cursor: pointer;
  transition: background 0.16s ease, border-color 0.16s ease;
}

.incomplete-retry:hover {
  background: color-mix(in srgb, var(--color-primary, #c97f3a) 20%, transparent);
  border-color: var(--color-primary, #c97f3a);
}

.incomplete-retry:active {
  transform: scale(0.97);
}

.source-list {
  flex: 1;
  /* 必须有 min-height: 0：flex 子项默认 min-height:auto，内容一多就会把
     滚动区撑开、连同下方固定的详情一起顶出可视区，详情里的「切换」按钮
     反而看不见了。 */
  min-height: 0;
  overflow-y: auto;
  padding: 8px 0;
}

.book-brief {
  display: flex;
  gap: 12px;
  margin: 8px 16px 14px;
  padding: 12px;
  border-radius: 14px;
  background: rgba(201, 127, 58, 0.08);
  border: 1px solid rgba(201, 127, 58, 0.14);
}

/* 封面按四行文本的高度收：不再是简介撑高的长卡片，56×76 会留出多余
   上下空白，显得松散。 */
.book-brief-cover {
  width: 52px;
  height: 70px;
  flex-shrink: 0;
  border-radius: 9px;
  overflow: hidden;
  background: rgba(0, 0, 0, 0.06);
}

.book-brief-cover img,
.book-brief-placeholder {
  width: 100%;
  height: 100%;
}

.book-brief-cover img {
  object-fit: cover;
}

.book-brief-placeholder {
  display: flex;
  align-items: center;
  justify-content: center;
  font-size: 20px;
  font-weight: 700;
}

.book-brief-main {
  min-width: 0;
  display: flex;
  flex-direction: column;
  gap: 4px;
}

.book-brief-title {
  font-size: 15px;
  font-weight: 700;
}

.book-brief-meta {
  font-size: 12px;
  opacity: 0.68;
  line-height: 1.4;
}

.section-label {
  padding: 12px 20px 4px;
  font-size: 11px;
  text-transform: uppercase;
  letter-spacing: 0.05em;
  opacity: 0.4;
  font-weight: 600;
}

.source-item {
  display: flex;
  justify-content: space-between;
  align-items: center;
  padding: 12px 20px;
  border-bottom: 1px solid rgba(0,0,0,0.02);
  cursor: default;
  transition: background 0.2s;
}

.source-item.current { background: rgba(201, 127, 58, 0.04); }

/* ─── 候选书源列表 ───
   一行一源，信息密度对齐手机端 item_change_source.xml：
   主行「书源名 + 状态徽标」，次行「作者 · 最新章节」。 */
.candidate-list {
  display: flex;
  flex-direction: column;
  gap: 2px;
  padding: 2px 12px 8px;
}

.candidate {
  position: relative;
  display: flex;
  align-items: center;
  gap: 10px;
  width: 100%;
  padding: 10px 12px 10px 14px;
  border: 1px solid transparent;
  border-radius: 12px;
  background: transparent;
  color: inherit;
  font: inherit;
  text-align: left;
  cursor: pointer;
  transition: background 0.18s ease, border-color 0.18s ease;
}

.candidate:hover { background: rgba(0, 0, 0, 0.035); }

.candidate:active { transform: scale(0.995); }

.candidate:focus-visible {
  outline: 2px solid color-mix(in srgb, var(--color-primary, #c97f3a) 55%, transparent);
  outline-offset: 1px;
}

/* 选中指示用左侧色条，避免整行套一个高饱和底色把列表压得发闷。 */
.candidate-rail {
  position: absolute;
  left: 4px;
  top: 50%;
  width: 3px;
  height: 0;
  border-radius: 999px;
  background: var(--color-primary, #c97f3a);
  transform: translateY(-50%);
  transition: height 0.18s ease;
}

.candidate.active {
  background: color-mix(in srgb, var(--color-primary, #c97f3a) 9%, transparent);
  border-color: color-mix(in srgb, var(--color-primary, #c97f3a) 26%, transparent);
}

.candidate.active .candidate-rail { height: 60%; }

.candidate-body {
  display: flex;
  flex: 1;
  min-width: 0;
  flex-direction: column;
  gap: 3px;
}

.candidate-title-row {
  display: flex;
  align-items: center;
  gap: 6px;
  min-width: 0;
}

.candidate-name {
  min-width: 0;
  overflow: hidden;
  font-size: 14px;
  font-weight: 600;
  line-height: 1.3;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.candidate-badge {
  flex-shrink: 0;
  padding: 1px 6px;
  border-radius: 5px;
  background: rgba(0, 0, 0, 0.055);
  font-size: 10px;
  line-height: 1.5;
  opacity: 0.72;
  white-space: nowrap;
}

.candidate-badge.good {
  background: rgba(82, 196, 26, 0.15);
  color: #3f8f16;
  opacity: 1;
}

.candidate-badge.warn {
  background: rgba(201, 127, 58, 0.16);
  color: #a4631f;
  opacity: 1;
}

.candidate-meta {
  display: flex;
  align-items: baseline;
  gap: 8px;
  min-width: 0;
  font-size: 11.5px;
  line-height: 1.4;
  opacity: 0.58;
}

.candidate-author { flex-shrink: 0; }

.candidate-latest {
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.candidate-go {
  flex-shrink: 0;
  display: flex;
  align-items: center;
  opacity: 0.22;
  transition: opacity 0.18s ease, transform 0.18s ease;
}

.candidate-go svg { width: 15px; height: 15px; }

.candidate:hover .candidate-go,
.candidate.active .candidate-go {
  opacity: 0.55;
  transform: translateX(1px);
}

/* ─── 底部选中详情 ───
   固定在列表下方（flex-shrink: 0），主操作始终可见，不用滚到列表末尾。
   两行、约 88px 高：列表本身已经显示书名/作者/最新章，这里只补「切过去
   是否安全」这一个决定性问题，不再重复列表里已有的信息。 */
.candidate-detail {
  flex-shrink: 0;
  margin: 0 12px 12px;
  padding: 10px 12px;
  border-radius: 14px;
  background: color-mix(in srgb, var(--color-primary, #c97f3a) 8%, transparent);
  border: 1px solid color-mix(in srgb, var(--color-primary, #c97f3a) 20%, transparent);
}

.detail-row {
  display: flex;
  align-items: center;
  gap: 8px;
  min-width: 0;
}

.detail-row-main { margin-bottom: 8px; }

.detail-name {
  min-width: 0;
  overflow: hidden;
  font-size: 13px;
  font-weight: 600;
  line-height: 1.3;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.detail-sub {
  min-width: 0;
  overflow: hidden;
  font-size: 11px;
  line-height: 1.3;
  opacity: 0.5;
  text-overflow: ellipsis;
  white-space: nowrap;
}

/* 撑开中间空隙，把关闭按钮推到最右。 */
.detail-row-main .detail-sub { flex: 1; }

.detail-dismiss {
  flex-shrink: 0;
  display: flex;
  align-items: center;
  justify-content: center;
  width: 20px;
  height: 20px;
  margin: -2px -2px -2px 0;
  padding: 0;
  border: none;
  border-radius: 5px;
  background: transparent;
  color: inherit;
  cursor: pointer;
  opacity: 0.35;
  transition: opacity 0.18s ease, background 0.18s ease;
}

.detail-dismiss svg { width: 12px; height: 12px; }

.detail-dismiss:hover {
  opacity: 0.85;
  background: rgba(0, 0, 0, 0.06);
}

.detail-latest {
  flex: 1;
  min-width: 0;
  overflow: hidden;
  font-size: 11.5px;
  line-height: 1.35;
  opacity: 0.66;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.detail-slide-enter-active,
.detail-slide-leave-active {
  transition: opacity 0.18s ease, transform 0.18s ease;
}

.detail-slide-enter-from,
.detail-slide-leave-to {
  opacity: 0;
  transform: translateY(6px);
}

.source-url { font-size: 11px; opacity: 0.3; }

/* 主操作：实心，底部详情里唯一的强调色元素，视觉焦点不会被列表抢走。 */
.switch-btn {
  flex-shrink: 0;
  padding: 7px 16px;
  border: 1px solid transparent;
  border-radius: 999px;
  background: var(--color-primary, #c97f3a);
  color: #fff;
  font-size: 12.5px;
  font-weight: 600;
  line-height: 1.2;
  cursor: pointer;
  transition: filter 0.18s ease, transform 0.12s ease;
}

.switch-btn:hover:not(:disabled) { filter: brightness(1.07); }

.switch-btn:active:not(:disabled) { transform: scale(0.97); }

.switch-btn:focus-visible {
  outline: 2px solid color-mix(in srgb, var(--color-primary, #c97f3a) 60%, transparent);
  outline-offset: 2px;
}

.switch-btn:disabled {
  cursor: not-allowed;
  opacity: 0.5;
}

.loading, .empty {
  padding: 40px 20px;
  text-align: center;
  opacity: 0.5;
  font-size: 14px;
}

.inline-loading {
  display: flex;
  align-items: center;
  justify-content: center;
  gap: 8px;
  padding: 12px 20px;
  font-size: 12px;
  opacity: 0.55;
}

.load-more-wrap {
  padding: 16px 20px 24px;
  display: flex;
  justify-content: center;
}

.load-more-btn {
  min-width: 120px;
  padding: 10px 18px;
  border-radius: 999px;
  border: 1px solid rgba(0,0,0,0.08);
  background: transparent;
  color: inherit;
  cursor: pointer;
}

.load-more-btn:hover:not(:disabled) {
  background: rgba(201, 127, 58, 0.08);
  border-color: var(--color-primary, #c97f3a);
  color: var(--color-primary, #c97f3a);
}

.load-more-btn:disabled {
  opacity: 0.5;
  cursor: wait;
}

.spinner {
  width: 24px;
  height: 24px;
  border: 2px solid rgba(0,0,0,0.1);
  border-top-color: var(--color-primary, #c97f3a);
  border-radius: 50%;
  animation: spin 1s linear infinite;
  margin: 0 auto 12px;
}

.inline-loading .spinner,
.spinner.small {
  width: 14px;
  height: 14px;
  margin: 0;
  border-width: 2px;
}

@keyframes spin { to { transform: rotate(360deg); } }

.switch-overlay {
  position: absolute;
  top: 0; left: 0; right: 0; bottom: 0;
  display: flex; flex-direction: column;
  align-items: center; justify-content: center;
  z-index: 20;
}

.switch-overlay p { margin-top: 16px; font-size: 14px; opacity: 0.8; }

@media (max-width: 640px) {
  /* 窄屏下让按钮独占第二行右侧，信息行仍然保持单行不换行。 */
  .detail-latest {
    font-size: 11px;
  }

  .switch-btn {
    padding: 7px 14px;
  }
}
</style>
