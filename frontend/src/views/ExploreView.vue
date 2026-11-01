<template>
  <div class="explore-view" :style="{ '--color-primary': '#c97f3a' }">
    <!-- 顶部工具区：书源切换 + 书源自己的控件。
         刻意做得很矮 —— 发现页的主角是书grid, 早先"大标题 + 独占一行的书源下拉 +
         换行的控件栏"要吃掉约 250px, 窗口模式下书籍区只剩一半。 -->
    <header class="explore-toolbar">
      <div class="toolbar-primary">
        <h1 class="view-title">发现书海</h1>

        <div class="source-selector" v-if="store.exploreSources.length > 0">
          <select
            class="field source-select"
            :value="store.activeSourceUrl"
            :disabled="store.categoriesLoading"
            @change="onSourceChange"
          >
            <option
              v-for="src in store.exploreSources"
              :key="src.bookSourceUrl"
              :value="src.bookSourceUrl"
            >
              {{ src.bookSourceName }}
            </option>
          </select>
        </div>
        <span v-else class="no-sources-text">无带有发现规则的书源</span>

        <span class="toolbar-spacer" />

        <span v-if="store.categoriesLoading" class="status-chip">
          <span class="status-dot loading" />分类加载中
        </span>
        <span v-else-if="store.books.length > 0" class="status-chip">
          共 {{ store.books.length }} 本
        </span>

        <!-- 登录：与「书源管理」里完全同一套登录面板。
             书源自己也有登录控件（java.open('login')），但那个只发意图、
             没有可见效果；统一收到这里，功能与位置都可预期。 -->
        <button
          v-if="store.currentSource"
          type="button"
          class="field login-button"
          :title="`登录 ${store.currentSource.bookSourceName}`"
          @click="store.openLogin()"
        >
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" width="14" height="14">
            <path d="M15 3h4a2 2 0 0 1 2 2v14a2 2 0 0 1-2 2h-4" />
            <path d="M10 17l5-5-5-5M15 12H3" />
          </svg>
          登录
        </button>

        <!-- 窄窗口下才出现：收起书源控件，把高度让给书籍网格 -->
        <button
          v-if="store.controls.length > 0"
          type="button"
          class="controls-toggle"
          :aria-expanded="!controlsCollapsed"
          @click="controlsCollapsed = !controlsCollapsed"
        >
          {{ controlsCollapsed ? `筛选 (${store.controls.length})` : '收起筛选' }}
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="12" height="12">
            <path :d="controlsCollapsed ? 'M6 9l6 6 6-6' : 'M18 15l-6-6-6 6'" />
          </svg>
        </button>
      </div>

      <!-- 书源控件栏：搜索框 / 下拉筛选 / 按钮 / 开关。
           对齐 legado ExploreAdapter —— 这些控件由 exploreUrl 的 JS 生成，
           其取值(infoMap)会回送给书源脚本参与后续求值。

           聚合类书源常有 6~10 个控件（一个源可有近十个），横排必然换行。
           窄窗口下把它们折进可折叠区域，默认收起 —— 否则工具栏会涨到 125px+，
           比旧布局还占地方。宽窗口控件一行放得下，则不需要折叠按钮。 -->
      <div class="toolbar-controls-wrap" v-if="store.controls.length > 0">
        <div class="toolbar-controls" :class="{ 'is-collapsed': controlsCollapsed }">
          <div
            v-for="(ctrl, index) in store.controls"
            :key="`${ctrl.title}:${index}`"
            class="control-item"
          >
            <!-- 按钮 -->
            <button
              v-if="exploreKindType(ctrl) === 'button'"
              class="field control-button"
              :disabled="store.categoriesLoading"
              @click="onPressButton(ctrl)"
            >
              {{ exploreKindLabel(ctrl) }}
            </button>

            <!-- 开关：点击在选项间循环 -->
            <button
              v-else-if="exploreKindType(ctrl) === 'toggle'"
              class="field control-toggle"
              :class="{ 'is-on': Boolean(store.valueOf(ctrl)) }"
              :disabled="store.categoriesLoading"
              @click="onToggle(ctrl)"
            >
              <span class="toggle-value">{{ store.valueOf(ctrl) }}</span>
              <span class="toggle-label">{{ exploreKindLabel(ctrl) }}</span>
            </button>

            <!-- 下拉选择 -->
            <label v-else-if="exploreKindType(ctrl) === 'select'" class="control-select">
              <span class="control-label">{{ exploreKindLabel(ctrl) }}</span>
              <select
                class="field"
                :value="store.valueOf(ctrl)"
                :disabled="store.categoriesLoading"
                @change="onSelect(ctrl, $event)"
              >
                <option v-for="opt in store.optionsOf(ctrl)" :key="opt" :value="opt">{{ opt }}</option>
              </select>
            </label>

            <!-- 文本输入 -->
            <label v-else-if="exploreKindType(ctrl) === 'text'" class="control-text">
              <input
                class="field"
                type="text"
                :placeholder="exploreKindLabel(ctrl)"
                :value="store.infoMap[ctrl.title] ?? ''"
                :disabled="store.categoriesLoading"
                @input="onTextInput(ctrl, $event)"
              />
            </label>
          </div>
        </div>
      </div>
    </header>

    <div class="explore-body">
      <!-- 桌面端侧边分类 / 移动端顶部分类滑动条 -->
      <aside class="categories-panel">
        <div class="categories-scroll" v-if="store.categoryItems.length > 0">
          <template v-for="(cat, index) in store.categoryItems" :key="`${cat.url}:${index}`">
            <!-- 分组标题（url 为空）：不可点击，用于分隔分类 -->
            <div v-if="!(cat.url ?? '').trim()" class="category-section">{{ cat.title }}</div>
            <button
              v-else
              type="button"
              class="category-tag"
              :class="{ active: store.activeCategoryUrl === cat.url }"
              @click="handleCategoryClick(cat)"
            >
              {{ cat.title }}
            </button>
          </template>
        </div>
        <div class="categories-scroll" v-else>
          <div class="category-hint">{{ emptyCategoryHint }}</div>
        </div>
      </aside>

      <!-- 书籍列表区 -->
      <main class="content-panel" ref="scrollContainer" @scroll="handleScroll">
        <div class="books-grid-wrapper" v-if="store.books.length > 0">
          <BookGrid
            :books="store.books"
            :is-search="true"
            :shelf-book-urls="shelfBookUrls"
            :shelf-book-keys="shelfBookKeys"
            empty-text="暂无数据"
            @click="handleBookInfo"
            @info="handleBookInfo"
            @addToShelf="handleAddToShelf"
            @contextmenu="handleBookContextMenu"
          />
        </div>

        <div class="loading-state" v-if="store.loading">
          <div class="spinner"></div>加载中...
        </div>

        <div class="end-state" v-else-if="!store.hasMore && store.books.length > 0">
          没有更多了
        </div>

        <div class="error-state" v-else-if="store.error">
          {{ store.error }}
        </div>
      </main>
    </div>

    <!-- 发现页的书只有聚合源认得，详情页不必去全网并发搜「可读书源」：
         既慢（12 路并发）又几乎搜不到，只会让用户看到一个源。 -->
    <BookDetailModal v-model="showDetail" :book="selectedBook" single-source-only />

    <!-- 登录面板：与书源管理里用的是同一个组件。
         它只暴露 v-model，关闭后重新求值一次分类 —— 登录态会决定有些分类
         （聚合源的「晴天书架」「会员榜」）能不能取到数据。 -->
    <SourceLoginDialog
      v-model="store.showLogin"
      v-if="store.currentSource"
      :source="store.currentSource"
    />

    <!-- 书源 action 弹出的 HTML 面板（「更新书源」「书源设置」） -->
    <SourceBrowserPanel
      v-if="store.pendingBrowser"
      :request="store.pendingBrowser"
      @close="store.closePendingBrowser()"
      @toast="onBrowserToast"
      @import-link="importFromLink"
    />
  </div>
</template>

<script setup lang="ts">
import { ref, onMounted, onBeforeUnmount, computed, watch } from 'vue'
import { useRouter } from 'vue-router'
import { useExploreStore } from '../stores/explore'
import { useReaderStore } from '../stores/reader'
import { saveBook } from '../api/bookshelf'
import { useAppStore } from '../stores/app'
import BookGrid from '../components/BookGrid.vue'
import BookDetailModal from '../components/BookDetailModal.vue'
import SourceBrowserPanel from '../components/SourceBrowserPanel.vue'
import SourceLoginDialog from '../components/source-manager/SourceLoginDialog.vue'
import { showContextMenu } from '../composables/useContextMenu'
import { useSourceImport } from '../composables/useSourceImport'
import type { Book, SearchBook } from '../types'
import { exploreKindLabel, exploreKindType, type ExploreKind } from '../api/explore'
import { shelfMatchKey } from '../utils/searchRank'
import { isBookOnShelf } from '../utils/bookEdit'
import { useBookshelfStore } from '../stores/bookshelf'
import { createCoverCacheSession } from '../utils/coverCacheSession'

const store = useExploreStore()
const readerStore = useReaderStore()
const appStore = useAppStore()
const shelfStore = useBookshelfStore()
const router = useRouter()

const scrollContainer = ref<HTMLElement>()
const openingBookUrl = ref('')
const selectedBook = ref<Book | SearchBook | null>(null)
const showDetail = ref(false)
const coverCacheSession = createCoverCacheSession()

/** 文本框输入防抖计时器（key 为控件标题）。 */
const textTimers = new Map<string, ReturnType<typeof setTimeout>>()

/**
 * 书源控件的折叠状态。
 *
 * 初值按窗口宽度定：窄窗口（<=900px）默认收起 —— 聚合类书源有 6~10 个控件，
 * 窄窗口下必然换成两三行，工具栏会从 85px 涨到 125px 以上，比不折叠更占地方；
 * 宽窗口一行放得下，保持展开，不牺牲可见性。
 *
 * 只在初始化时判定一次：用户手动折叠/展开后不再被 resize 覆盖，避免"刚点开
 * 又被自动收起"。窄窗口的 CSS 才让折叠生效（宽窗口忽略该类），所以宽窗口里
 * 这个 ref 取什么值都无副作用。
 */
const controlsCollapsed = ref(
  typeof window !== 'undefined' && window.innerWidth <= 900,
)

const emptyCategoryHint = computed(() => {
  if (store.categoriesLoading) return '分类加载中…'
  if (store.categoriesError) return store.categoriesError
  return '该源暂无可用分类'
})

// 已在书架的书集合, 用于发现页卡片显示「已加入」并禁用重复加入。
// 少了这两个集合, 「加入书架」在本页永不 disabled, 点到已在书架的书会走
// save_book 的整条覆盖分支, 把用户自定义的封面/简介等冲掉。
const shelfBookUrls = computed(() => new Set(shelfStore.books.map((book) => book.bookUrl)))
// shelfMatchKey 额外按「本地书 / 网络书」分档: 只按书名+作者会把这本本地导入的
// 《遮天》和发现页的网络《遮天》并成一本, 于是网络版被错误标成「已加入」。
const shelfBookKeys = computed(() => new Set(shelfStore.books.map((book) => shelfMatchKey(book))))

onMounted(async () => {
  // 书架为空时「已加入」标记与 join 判重全都失效(见 shelfBookUrls 注释),
  // 所以本页必须自己保证书架数据到手; 失败不阻断发现页浏览。
  void shelfStore.fetchBooks().catch(() => undefined)
  await store.init()
})

// 离开发现页时清掉未触发的输入防抖，避免计时器在别的路由里补齐执行。
onBeforeUnmount(clearTextTimers)
onBeforeUnmount(() => coverCacheSession.release(shelfStore.books))

watch(
  () => store.books.length,
  () => coverCacheSession.collect(store.books),
  { deep: false },
)

function onSourceChange(event: Event) {
  // 切源时丢弃未触发的输入防抖: 它们的 action 属于上一个源，落定后会被
  // runAction 打到新源上（旧 kind 对新源无意义）。
  clearTextTimers()
  coverCacheSession.release(shelfStore.books)
  void store.setSource((event.target as HTMLSelectElement).value)
}

/** 清空所有待触发的输入防抖计时器。 */
function clearTextTimers() {
  for (const timer of textTimers.values()) clearTimeout(timer)
  textTimers.clear()
}

function handleScroll() {
  const el = scrollContainer.value
  if (!el) return
  const { scrollTop, scrollHeight, clientHeight } = el
  // 触底 100px 触发加载
  if (scrollTop + clientHeight >= scrollHeight - 100) {
    void store.fetchMore()
  }
}

function handleCategoryClick(category: ExploreKind) {
  coverCacheSession.release(shelfStore.books)
  void store.setCategory(category.url ?? '')
}

function onSelect(ctrl: ExploreKind, event: Event) {
  void store.selectOption(ctrl, (event.target as HTMLSelectElement).value)
}

function onToggle(ctrl: ExploreKind) {
  void store.toggleOption(ctrl)
}

function onPressButton(ctrl: ExploreKind) {
  void store.pressButton(ctrl).then(handleIntents)
}

/**
 * 落实书源 action 登记的意图（如书源三的「🔍搜索」按钮）。
 *
 * `java.searchBook(key, source)` 只表达"发起这次搜索"，真正的搜索框与结果页
 * 归书架视图所有：这里把关键词交给 bookshelf store 并跳过去。
 *
 * 面板与提示由 store 暂存、模板直接渲染（`store.pendingBrowser` /
 * `store.pendingMessages`），这里只负责需要跳转的那一类。
 */
function handleIntents() {
  for (const message of store.takePendingMessages()) {
    appStore.showToast(message)
  }

  const intents = store.takePendingIntents()
  for (const intent of intents) {
    if (intent.kind === 'search') {
      shelfStore.startSearch(intent.key, {
        scope: 'source',
        sourceUrl: store.activeSourceUrl,
      })
      void router.push('/')
      return
    }
  }
}

function onBrowserToast(message: string) {
  appStore.showToast(message)
}

// 书源面板里点「一键导入」(yuedu:// / legado://) 时由这里完成入库。
const { importFromLink } = useSourceImport()

// 登录面板关闭后重新求值分类：登录态会决定部分分类能否取到数据
//（聚合源的「晴天书架」「会员榜」未登录时是空的）。
watch(
  () => store.showLogin,
  (visible, wasVisible) => {
    if (wasVisible && !visible && store.activeSourceUrl) {
      void store.setSource(store.activeSourceUrl)
    }
  },
)

/**
 * 文本框输入：写入 infoMap，并按书源配置的 action 防抖执行。
 *
 * legado 在 TextWatcher 里等 600ms 再跑 action（书源的搜索框靠它触发联想/
 * 刷新），这里保持一致，避免每敲一个字都打一次后端。
 */
function onTextInput(ctrl: ExploreKind, event: Event) {
  const value = (event.target as HTMLInputElement).value
  const key = ctrl.title
  const existing = textTimers.get(key)
  if (existing) clearTimeout(existing)

  if (!(ctrl.action ?? '').trim()) {
    void store.setText(ctrl, value)
    return
  }
  textTimers.set(
    key,
    setTimeout(() => {
      textTimers.delete(key)
      void store.setText(ctrl, value)
    }, 600),
  )
}

async function handleBookClick(book: Book | SearchBook) {
  const b = book as Book
  if (!b.origin || !b.bookUrl) return
  if (openingBookUrl.value === b.bookUrl) return

  openingBookUrl.value = b.bookUrl
  try {
    const loadBookTask = readerStore.loadBook(b)
    await router.push('/reader')
    await loadBookTask
  } finally {
    openingBookUrl.value = ''
  }
}

async function handleAddToShelf(book: Book | SearchBook) {
  // 已在书架就不能再走一遍 saveBook: 这里只提交 5 个字段, 而后端对同一本书是
  // 整条覆盖, 会把用户自定义过的封面/简介等清空。卡片按钮与右键菜单两条入口都要挡。
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
    })
    await shelfStore.fetchBooks()
    appStore.showToast(`"${book.name}" 已加入书架`, 'success')
  } catch (e: unknown) {
    appStore.showToast((e as Error).message, 'error')
  }
}

function handleBookContextMenu({ book, event }: { book: Book | SearchBook; event: MouseEvent }) {
  const menuItems = [
    { label: '开始阅读', action: () => handleBookClick(book) },
    { label: '查看详情', action: () => handleBookInfo(book) },
    { divider: true },
    { label: '加入书架', action: () => handleAddToShelf(book) },
  ]
  showContextMenu(event, menuItems, book)
}

function handleBookInfo(book: Book | SearchBook) {
  selectedBook.value = book
  showDetail.value = true
}
</script>

<style scoped>
/* ─────────────────────────────────────────────────────────────
   发现书海
   ─────────────────────────────────────────────────────────────
   布局要点（窗口模式下尤其重要）：
   顶部工具区必须矮。早先是 "大标题独占一行 + 书源下拉独占一行 + 换行的
   大控件"，在 1435x905 的窗口里吃掉约 250px，书籍区只剩一半，窗口再小
   就直接看不到书。现在标题与书源选择器并排一行、控件与它们同处一个容器，
   整体压在 90px 以内，并把省下的高度全部让给书籍网格。
   ───────────────────────────────────────────────────────────── */

.explore-view {
  height: 100%;
  min-height: 0;
  display: flex;
  flex-direction: column;
  background: var(--color-bg);
  overflow: hidden;
}

/* ── 顶部工具区 ── */
.explore-toolbar {
  flex-shrink: 0;
  display: flex;
  flex-direction: column;
  gap: var(--space-2);
  padding: var(--space-3) var(--space-5);
  border-bottom: 1px solid var(--color-border);
  background: var(--color-bg-elevated);
}

.toolbar-primary {
  display: flex;
  align-items: center;
  gap: var(--space-3);
  min-height: 32px;
}

.view-title {
  font-size: var(--text-lg);
  font-weight: 700;
  color: var(--color-text);
  margin: 0;
  white-space: nowrap;
  /* 标题是装饰性的：宽度不够时优先让给书源选择器与状态，不要挤成两行 */
  flex-shrink: 0;
}

.toolbar-spacer {
  flex: 1;
  min-width: var(--space-2);
}

.status-chip {
  display: inline-flex;
  align-items: center;
  gap: var(--space-2);
  font-size: var(--text-xs);
  color: var(--color-text-tertiary);
  white-space: nowrap;
  flex-shrink: 0;
}

.status-dot {
  width: 6px;
  height: 6px;
  border-radius: var(--radius-full);
  background: var(--color-text-tertiary);
}

.status-dot.loading {
  background: var(--color-primary);
  animation: status-pulse 1.1s ease-in-out infinite;
}

@keyframes status-pulse {
  0%, 100% { opacity: 0.35; }
  50% { opacity: 1; }
}

/* 统一的表单控件外观：书源选择器与书源控件共用一套尺寸，
   避免出现三种不同的高度把工具栏撑成锯齿状。 */
.field {
  height: 32px;
  padding: 0 var(--space-3);
  border-radius: var(--radius-sm);
  border: 1px solid var(--color-border);
  background: var(--color-bg);
  color: var(--color-text);
  font-size: var(--text-sm);
  font-family: inherit;
  outline: none;
  transition:
    border-color var(--duration-fast) var(--ease-out),
    background var(--duration-fast) var(--ease-out);
}

.field:hover:not(:disabled) {
  border-color: color-mix(in srgb, var(--color-primary) 38%, var(--color-border));
}

.field:focus-visible {
  border-color: var(--color-primary);
  box-shadow: var(--focus-ring);
}

.field:disabled {
  opacity: 0.55;
  cursor: default;
}

.source-selector {
  display: flex;
  align-items: center;
  min-width: 0;
  flex: 1;
  max-width: 420px;
}

/* 登录按钮：与分类同宽度的控件，靠主色区分出它是"动作"而非"筛选" */
.login-button {
  display: inline-flex;
  align-items: center;
  gap: var(--space-1);
  cursor: pointer;
  color: var(--color-text-secondary);
  flex-shrink: 0;
}

.login-button:hover {
  border-color: var(--color-primary);
  color: var(--color-primary);
  background: var(--color-primary-bg);
}

.source-select {
  width: 100%;
  cursor: pointer;
  font-weight: 500;
}

.no-sources-text {
  font-size: var(--text-sm);
  color: var(--color-text-tertiary);
}

/* ── 书源控件栏 ── */
.toolbar-controls-wrap {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: var(--space-2);
  min-width: 0;
}

.toolbar-controls {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: var(--space-2);
  min-width: 0;
  /* 占满余量，把折叠按钮挤到同一行末尾 */
  flex: 1;
}

/* 折叠按钮：宽窗口下控件一行放得下，不需要它。
   窄窗口出现时与"发现书海 + 书源选择器"同一行，**不额外占一行** ——
   早先把它放在控件下面自成一行，860px 宽时工具栏反而涨到 155px。 */
.controls-toggle {
  display: none;
  align-items: center;
  justify-content: center;
  gap: var(--space-1);
  height: 26px;
  padding: 0 var(--space-2);
  border: none;
  background: transparent;
  color: var(--color-primary);
  font-size: var(--text-xs);
  font-family: inherit;
  cursor: pointer;
  border-radius: var(--radius-sm);
  white-space: nowrap;
  flex-shrink: 0;
}

.controls-toggle:hover {
  background: var(--color-primary-bg);
}

.control-item {
  display: flex;
  align-items: center;
  min-width: 0;
}

.control-button {
  cursor: pointer;
  white-space: nowrap;
  color: var(--color-text-secondary);
}

.control-button:hover:not(:disabled) {
  border-color: var(--color-primary);
  color: var(--color-primary);
  background: var(--color-primary-bg);
}

/* 开关：值 + 名称分两段显示，值用主色强调当前状态 */
.control-toggle {
  display: inline-flex;
  align-items: center;
  gap: var(--space-2);
  cursor: pointer;
  white-space: nowrap;
}

.toggle-value {
  color: var(--color-text);
  font-weight: 600;
}

.toggle-label {
  color: var(--color-text-tertiary);
  font-size: var(--text-xs);
}

.control-toggle.is-on {
  border-color: color-mix(in srgb, var(--color-primary) 34%, var(--color-border));
  background: var(--color-primary-bg);
}

.control-select {
  display: flex;
  align-items: center;
  gap: var(--space-2);
  min-width: 0;
}

.control-label {
  font-size: var(--text-xs);
  color: var(--color-text-tertiary);
  white-space: nowrap;
  flex-shrink: 0;
}

.control-select select {
  cursor: pointer;
  max-width: 220px;
}

.control-text input {
  width: 200px;
  padding: 0 var(--space-3);
}

/* ── 主体：分类栏 + 书籍网格 ── */
.explore-body {
  flex: 1;
  min-height: 0;
  display: flex;
  overflow: hidden;
}

.categories-panel {
  flex-shrink: 0;
  width: 188px;
  border-right: 1px solid var(--color-border);
  background: var(--color-bg-elevated);
  overflow-y: auto;
  overflow-x: hidden;
  min-height: 0;
  padding: var(--space-2);
  scrollbar-width: thin;
}

.categories-scroll {
  display: flex;
  flex-direction: column;
  gap: 2px;
}

/* 分组标题（后端返回的 url 为空的项）：不可点击，用来分隔分类。 */
.category-section {
  padding: var(--space-3) var(--space-3) var(--space-1);
  font-size: var(--text-xs);
  font-weight: 600;
  color: var(--color-text-tertiary);
  letter-spacing: 0.02em;
}

.category-tag {
  display: block;
  width: 100%;
  text-align: left;
  padding: var(--space-2) var(--space-3);
  border-radius: var(--radius-sm);
  border: none;
  background: transparent;
  cursor: pointer;
  font-family: inherit;
  font-size: var(--text-sm);
  color: var(--color-text-secondary);
  transition:
    background var(--duration-fast) var(--ease-out),
    color var(--duration-fast) var(--ease-out);
}

/* 分类为空/加载中/出错时的占位文案 */
.category-hint {
  padding: var(--space-3);
  font-size: var(--text-sm);
  color: var(--color-text-tertiary);
  line-height: var(--leading-normal);
}

.category-tag:hover:not(.active) {
  background: var(--color-bg-hover);
  color: var(--color-text);
}

/* 选中态: 淡主色底 + 跟随主题的主色文字。
 * 这里**不能**用 --color-primary-soft-* 令牌: 本视图在根元素覆盖了
 * --color-primary(#c97f3a), 而自定义属性是在声明元素上完成替换后就继承的,
 * :root 里那份已按 #c56f35 固化, 不会重算 —— 会出现底与字两套色系。
 * 就地 color-mix 才能取到本视图的 --color-primary。 */
.category-tag.active {
  background: color-mix(in srgb, var(--color-primary) 14%, transparent);
  color: color-mix(in srgb, var(--color-primary) 55%, var(--color-text));
  font-weight: 600;
}

.content-panel {
  flex: 1;
  min-width: 0;
  min-height: 0;
  overflow-y: auto;
  padding: var(--space-4) var(--space-5);
  position: relative;
  scrollbar-width: thin;
}

.books-grid-wrapper {
  margin-bottom: var(--space-4);
}

/* ── 状态提示 ── */
.loading-state,
.end-state,
.error-state {
  text-align: center;
  padding: var(--space-5) 0;
  color: var(--color-text-tertiary);
  font-size: var(--text-sm);
  display: flex;
  justify-content: center;
  align-items: center;
  gap: var(--space-2);
}

.error-state {
  color: var(--color-danger);
}

.spinner {
  width: 15px;
  height: 15px;
  border: 2px solid var(--color-border);
  border-top-color: var(--color-primary);
  border-radius: var(--radius-full);
  animation: spin 1s linear infinite;
}

@keyframes spin {
  to { transform: rotate(360deg); }
}

/* ── 窄窗口 / 移动端 ──
   窗口被拉窄时（用户把桌面窗口缩小，或移动端），分类栏改为顶部的横向
   滚动条，把纵向空间全部让给书籍网格。 */
@media (max-width: 900px) {
  .explore-toolbar {
    gap: var(--space-1);
    padding: var(--space-2) var(--space-4);
  }

  .source-selector {
    max-width: 240px;
  }

  /* 窄窗口优先压缩输入框与下拉的宽度，而不是让控件换行 */
  .control-text input {
    width: 120px;
  }

  .control-select select {
    max-width: 130px;
  }

  /* 窄窗口才出现折叠按钮；收起后控件整块隐藏，工具栏立刻矮一截。
     不这么做时 860px 宽下 9 个控件必然换成两行，工具栏 125px，
     比旧布局的 100px 还占地方。 */
  .controls-toggle {
    display: inline-flex;
  }

  .toolbar-controls.is-collapsed {
    display: none;
  }

  .content-panel {
    padding: var(--space-3) var(--space-4);
  }
}

@media (max-width: 768px) {
  .explore-body {
    flex-direction: column;
  }

  .toolbar-primary {
    flex-wrap: wrap;
    gap: var(--space-2);
  }

  .view-title {
    font-size: var(--text-base);
  }

  .source-selector {
    max-width: none;
    order: 3;
    width: 100%;
  }

  .categories-panel {
    width: 100%;
    border-right: none;
    border-bottom: 1px solid var(--color-border);
    overflow-x: auto;
    overflow-y: hidden;
    flex-shrink: 0;
    padding: var(--space-2) var(--space-3);
  }

  .categories-scroll {
    flex-direction: row;
    gap: var(--space-2);
    align-items: center;
  }

  .category-section {
    padding: 0 var(--space-1);
    white-space: nowrap;
  }

  .category-tag {
    width: auto;
    white-space: nowrap;
    padding: var(--space-2) var(--space-3);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-full);
    background: var(--color-bg);
  }

  /* 窄屏沿用桌面版的就地写法: 同样不能换成 soft 令牌(原因见上方 .category-tag.active),
   * 且媒体查询不提升特异性, 若这里另起一套值, 桌面/窄屏会出现两种颜色。 */
  .category-tag.active {
    background: color-mix(in srgb, var(--color-primary) 14%, transparent);
    color: color-mix(in srgb, var(--color-primary) 55%, var(--color-text));
    border-color: color-mix(in srgb, var(--color-primary) 34%, transparent);
  }

  .content-panel {
    padding: var(--space-3) var(--space-4);
  }
}
</style>
