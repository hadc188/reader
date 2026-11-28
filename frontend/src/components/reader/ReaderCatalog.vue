<template>
  <div
    class="reader-catalog"
    :class="{ mini }"
    :style="{ background: theme.popup, color: theme.fontColor }"
  >
    <div class="catalog-header">
      <div class="tabs">
        <div 
          class="tab" 
          :class="{ active: activeTab === 'chapters' }" 
          @click="activeTab = 'chapters'"
        >
          目录
        </div>
        <div 
          class="tab" 
          :class="{ active: activeTab === 'bookmarks' }" 
          @click="activeTab = 'bookmarks'"
        >
          书签
        </div>
      </div>
      <div class="header-actions">
        <button
          v-if="activeTab === 'chapters'"
          class="icon-btn"
          type="button"
          title="刷新目录"
          aria-label="刷新目录"
          :disabled="store.chaptersLoading"
          @click="refreshCatalog"
        >
          <svg
            viewBox="0 0 24 24"
            fill="none"
            stroke="currentColor"
            stroke-width="1.8"
            stroke-linecap="round"
            stroke-linejoin="round"
            :class="{ spinning: store.chaptersLoading }"
          ><path d="M21 12a9 9 0 1 1-2.64-6.36M21 3v6h-6" /></svg>
        </button>
        <button class="close-btn" type="button" title="关闭" aria-label="关闭" @click="store.closePanel()">
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round">
            <path d="M6 6l12 12M18 6L6 18" />
          </svg>
        </button>
      </div>
    </div>

    <div v-if="activeTab === 'bookmarks'" class="bookmark-toolbar">
      <button class="bookmark-action primary" @click="addCurrentBookmark">添加当前页书签</button>
      <button
        class="bookmark-action"
        :class="{ danger: bookmarkEditMode && selectedBookmarkKeys.size > 0 }"
        @click="handleBatchAction"
      >
        {{ bookmarkEditMode ? (selectedBookmarkKeys.size ? `删除选中(${selectedBookmarkKeys.size})` : '完成') : '批量管理' }}
      </button>
    </div>

    <div v-show="activeTab === 'chapters'" class="chapter-toolbar">
      <div class="search-box">
        <input
          v-model="chapterSearch"
          type="text"
          placeholder="搜索章节..."
          class="search-input"
        />
        <button v-if="chapterSearch" class="search-clear" @click="chapterSearch = ''">
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M18 6 6 18M6 6l12 12" /></svg>
        </button>
      </div>
      <div class="chapter-jump-actions">
        <button
          class="jump-btn"
          title="跳到目录顶部"
          :disabled="!filteredChapters.length"
          @click="scrollCatalogTo('top')"
        >
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
            <path d="m18 15-6-6-6 6" />
            <path d="M5 5h14" />
          </svg>
        </button>
        <button
          class="jump-btn"
          title="跳到目录底部"
          :disabled="!filteredChapters.length"
          @click="scrollCatalogTo('bottom')"
        >
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
            <path d="m6 9 6 6 6-6" />
            <path d="M5 19h14" />
          </svg>
        </button>
      </div>
    </div>

    <!-- Chapters List -->
    <div
      v-show="activeTab === 'chapters'"
      class="list-container"
      ref="listRef"
      @scroll="hideTitleTip"
    >
      <div v-if="store.chaptersLoading" class="loading">加载目录中...</div>
      <div v-else-if="filteredChapters.length === 0" class="empty">未找到匹配的章节</div>
      <div
        v-else
        v-for="chapter in filteredChapters"
        :key="chapter.index"
        class="list-item"
        :class="{ active: chapter.index === store.currentIndex, read: store.isChapterRead(chapter.index) }"
        @click="goToChapter(chapter.index)"
      >
        <span class="item-index">
          <span class="index-num">{{ chapter.index + 1 }}</span>
          <span
            v-if="isChapterCached(chapter.url)"
            class="cached-dot"
            role="img"
            aria-label="已缓存"
            title="已缓存"
          ></span>
        </span>
        <span
          class="item-title"
          @mouseenter="handleTitleEnter"
          @mouseleave="handleTitleLeave"
        ><span class="item-title-text">{{ chapter.title }}</span></span>
        <span v-if="chapterWordCountText(chapter.url)" class="word-count">{{ chapterWordCountText(chapter.url) }}</span>
        <div
          v-if="chapter.index === store.currentIndex || store.isChapterRead(chapter.index)"
          class="item-status"
        >
          <span v-if="chapter.index === store.currentIndex" class="status-badge current">当前</span>
          <span v-else class="status-badge read">已读</span>
        </div>
      </div>
    </div>

    <!-- Bookmarks List -->
    <div v-show="activeTab === 'bookmarks'" class="list-container">
      <div v-if="!store.bookmarks.length" class="empty">暂无书签</div>
      <div
        v-else
        v-for="(bm, idx) in store.bookmarks"
        :key="idx"
        class="list-item bookmark-item"
        :class="{ selected: isBookmarkSelected(bm) }"
        @click="goToBookmark(bm)"
      >
        <button
          v-if="bookmarkEditMode"
          class="bookmark-check"
          :class="{ checked: isBookmarkSelected(bm) }"
          @click.stop="toggleBookmarkSelection(bm)"
        >
          鉁?
        </button>
        <div class="bm-header">
          <span class="bm-chapter">{{ bm.chapterName }}</span>
          <span class="bm-time">{{ formatDate(bm.time) }}</span>
        </div>
        <div class="bm-snippet">{{ bm.bookText }}</div>
        <button v-if="!bookmarkEditMode" class="bm-delete" @click.stop="store.removeBookmark(bm)">
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M3 6h18M19 6v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6m3 0V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2" /></svg>
        </button>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { ref, computed, onMounted, nextTick, watch } from 'vue'
import { useReaderStore } from '../../stores/reader'
import { useAppStore } from '../../stores/app'
import { getCachedChapterUrls, getChapterWordCounts } from '../../api/bookshelf'
import type { Bookmark } from '../../types'

const props = withDefaults(defineProps<{
  initialTab?: 'chapters' | 'bookmarks'
  /**
   * 迷你模式: 压缩头部/工具栏/列表项的尺寸。
   * 小窗只有 240px 高, 常规尺寸下头部+工具栏会吃掉一半以上, 列表只剩两行。
   */
  mini?: boolean
}>(), {
  initialTab: 'chapters',
  mini: false,
})
const emit = defineEmits<{
  jumpChapter: [index: number]
}>()

const store = useReaderStore()
const appStore = useAppStore()
const theme = computed(() => store.chromeTheme)
const activeTab = ref<'chapters' | 'bookmarks'>(props.initialTab)
const listRef = ref<HTMLElement>()
const bookmarkEditMode = ref(false)
const selectedBookmarkKeys = ref<Set<string>>(new Set())
const cachedChapterUrls = ref<Set<string>>(new Set())
/** 已缓存章节的可见字数（去标签后统计），key 为章节地址。 */
const chapterWordCounts = ref<Map<string, number>>(new Map())
const chapterSearch = ref('')

// Filtered chapters based on search
const filteredChapters = computed(() => {
  if (!chapterSearch.value.trim()) {
    return store.chapters.map((chapter, index) => ({ ...chapter, index }))
  }
  const searchTerm = chapterSearch.value.toLowerCase().trim()
  return store.chapters
    .map((chapter, index) => ({ ...chapter, index }))
    .filter(chapter => chapter.title.toLowerCase().includes(searchTerm))
})

onMounted(() => {
  activeTab.value = props.initialTab
  scrollToCurrent()
  store.fetchBookmarks()
  void refreshCachedChapterState()
})

watch(() => props.initialTab, (tab) => {
  activeTab.value = tab
  if (tab !== 'bookmarks') {
    bookmarkEditMode.value = false
    selectedBookmarkKeys.value.clear()
  }
  if (tab === 'chapters') {
    void refreshCachedChapterState()
  }
})

watch(() => store.book?.bookUrl, () => {
  void refreshCachedChapterState()
})

watch(() => store.chapters, () => {
  void refreshCachedChapterState()
}, { deep: true })

function scrollToCurrent() {
  nextTick(() => {
    const activeEl = listRef.value?.querySelector('.list-item.active')
    if (activeEl) {
      activeEl.scrollIntoView({ block: 'center' })
    }
  })
}

function scrollCatalogTo(position: 'top' | 'bottom') {
  const el = listRef.value
  if (!el) return
  el.scrollTo({
    top: position === 'top' ? 0 : el.scrollHeight,
    behavior: 'smooth',
  })
}

async function goToChapter(index: number) {
  emit('jumpChapter', index)
}

async function refreshCatalog() {
  try {
    const refreshed = await store.refreshChapters()
    if (!refreshed) return
  } catch {
    // 失败原因由 store 统一 toast（含「目录为空」这类后端不回错的场景）;
    // 这里只要不继续滚动列表即可 —— 抛出去会变成未处理的 Promise 拒绝。
    return
  }
  await refreshCachedChapterState()
  scrollToCurrent()
}

async function refreshCachedChapterState() {
  if (!store.book || !store.chapters.length) {
    cachedChapterUrls.value = new Set()
    chapterWordCounts.value = new Map()
    return
  }
  const bookUrl = store.book.bookUrl
  const urls = store.chapters.map((chapter) => chapter.url).filter(Boolean)
  cachedChapterUrls.value = new Set(
    await getCachedChapterUrls(bookUrl, urls).catch(() => []),
  )
  const counts = await getChapterWordCounts(bookUrl, urls).catch(() => [])
  chapterWordCounts.value = new Map(counts)
}

function isChapterCached(chapterUrl?: string) {
  return !!chapterUrl && cachedChapterUrls.value.has(chapterUrl)
}

/** 章节字数文案：与阅读端一致，过万字显示为 x.x 万字。 */
function chapterWordCountText(chapterUrl?: string): string {
  const count = chapterUrl ? chapterWordCounts.value.get(chapterUrl) : undefined
  if (!count) return ''
  if (count > 10000) return `${(count / 10000).toFixed(1)}万字`
  return `${count}字`
}

/** 标题是否被省略号截断。
 *
 *  不看 outer 的 scrollWidth: 外层是固定宽度的裁剪窗口, 它的 scrollWidth 包含
 *  内层溢出部分, 但直接拿内层宽度更直观 —— 内层是 inline-block 且 nowrap,
 *  其 offsetWidth 就是文字的真实宽度。
 *  退化路径: 内层还没渲染(不可能, 但防御)时用离屏探针量文本宽度。 */
function isTitleTruncated(outer: HTMLElement, text: string): boolean {
  if (outer.clientWidth <= 0) return false
  const inner = outer.querySelector<HTMLElement>('.item-title-text')
  if (inner) return inner.scrollWidth > outer.clientWidth + 1
  const probe = document.createElement('span')
  const style = window.getComputedStyle(outer)
  probe.style.cssText = [
    'position:absolute',
    'visibility:hidden',
    'white-space:nowrap',
    'pointer-events:none',
    `font:${style.font}`,
    `letter-spacing:${style.letterSpacing}`,
  ].join(';')
  probe.textContent = text
  document.body.appendChild(probe)
  const textWidth = probe.getBoundingClientRect().width
  document.body.removeChild(probe)
  return textWidth > outer.clientWidth + 1
}

/** 当前正在滚动的标题元素与动画。同一时刻只保留一个:
 *  鼠标从上一条移到下一条时, 上一条的动画要显式取消, 否则它会在后台继续跑,
 *  并且把 transform 留在元素上, 让那条标题错位。 */
let titleAnimation: Animation | null = null
/** 记录被动画的标题元素, 停止时用来还原省略号(见 startTitleScroll)。 */
let titleScrolledEl: HTMLElement | null = null

/** 停止滚动并把标题恢复到原位。 */
function stopTitleScroll() {
  if (titleAnimation) {
    try {
      // cancel() 会撤销 fill: forwards 留下的 transform, 元素回到原始位置。
      titleAnimation.cancel()
    } catch {
      /* 动画可能已自然结束, 忽略 */
    }
    titleAnimation = null
  }
  if (titleScrolledEl) {
    titleScrolledEl.style.textOverflow = ''
    titleScrolledEl = null
  }
}

/** 标题横向滚动, 露出被省略号截断的尾部。
 *
 *  用 Web Animations API 而不是 CSS 动画 / class: 这个组件是按需加载的分包,
 *  给它写的 CSS 规则在产物里曾整条缺失(浮层就是这么没显示出来的), 同样的坑
 *  不想在滚动上再踩一次。Element.animate 由脚本直接驱动, 不经过样式表。
 *
 *  transform 只作用在标题自身, 不影响列表布局; 滚动结束停在末尾, 让用户看清
 *  被截掉的部分。 */
/** 标题横向滚动, 露出被省略号截断的尾部。
 *
 *  两层结构是关键: outer(.item-title) 只用 overflow: hidden 当裁剪窗口,
 *  真正被 translate 的是 inner(.item-title-text)。若直接位移 outer, 裁剪框会
 *  跟着元素一起移动, 文字相对可见区毫无变化 —— 那正是"不会滚动"的原因。
 *
 *  用 Web Animations API 而不是 CSS 动画 / class: 这个组件是按需加载的分包,
 *  给它写的 CSS 规则在产物里曾整条缺失(浮层就是这么没显示出来的), 同样的坑
 *  不想在滚动上再踩一次。Element.animate 由脚本直接驱动, 不经过样式表。
 *
 *  滚动结束停在末尾, 让用户看清被截掉的部分。 */
function startTitleScroll(outer: HTMLElement) {
  stopTitleScroll()
  const inner = outer.querySelector<HTMLElement>('.item-title-text')
  if (!inner) return
  // 用内层的实际文字宽度减外层可见宽度, 得到需要移动的距离。
  const overflow = inner.scrollWidth - outer.clientWidth
  if (overflow <= 1) return
  // 约 45px/秒, 夹在 0.8~5 秒: 太快的长标题看不清, 太慢的短溢出又拖沓。
  const duration = Math.min(5000, Math.max(800, (overflow / 45) * 1000))
  // 滚动期间去掉省略号: 否则它跟着内容一起左移, 看着像文字末尾粘了个「…」。
  outer.style.textOverflow = 'clip'
  titleScrolledEl = outer
  titleAnimation = inner.animate(
    [
      { transform: 'translateX(0)' },
      { transform: `translateX(-${overflow}px)` },
    ],
    {
      duration,
      // 先停顿 0.35s 再动: 鼠标扫过列表时不会到处闪动。
      delay: 350,
      easing: 'linear',
      fill: 'forwards',
    },
  )
}

function handleTitleEnter(event: MouseEvent) {
  const el = event.currentTarget as HTMLElement | null
  if (!el) return
  const text = el.textContent || ''
  if (!text.trim()) return
  if (!isTitleTruncated(el, text)) return
  startTitleScroll(el)
}

function handleTitleLeave(event: MouseEvent) {
  const el = event.currentTarget as HTMLElement | null
  if (!el) return
  stopTitleScroll()
  // cancel() 会把元素恢复到未动画时的状态, 因此下一次悬停仍从开头滚起。
}

/** 列表滚动时停掉标题动画: 动画锚定的是元素自身, 列表一滚画面会跟着错位。 */
function hideTitleTip() {
  stopTitleScroll()
}

async function goToBookmark(bm: Bookmark) {
  if (bookmarkEditMode.value) {
    toggleBookmarkSelection(bm)
    return
  }
  if (bm.chapterIndex !== undefined) {
    await store.loadChapter(bm.chapterIndex)
    // Position scrolling could be added here if needed
    store.closePanel()
  }
}

function getBookmarkKey(bm: Bookmark) {
  return `${bm.bookName}|${bm.bookAuthor}|${bm.chapterIndex}|${bm.chapterPos}|${bm.time}|${bm.bookText}`
}

function isBookmarkSelected(bm: Bookmark) {
  return selectedBookmarkKeys.value.has(getBookmarkKey(bm))
}

function toggleBookmarkSelection(bm: Bookmark) {
  const key = getBookmarkKey(bm)
  if (selectedBookmarkKeys.value.has(key)) {
    selectedBookmarkKeys.value.delete(key)
  } else {
    selectedBookmarkKeys.value.add(key)
  }
}

async function addCurrentBookmark() {
  await store.addBookmark()
  appStore.showToast('已添加当前页书签', 'success')
}

async function handleBatchAction() {
  if (!bookmarkEditMode.value) {
    bookmarkEditMode.value = true
    return
  }
  if (!selectedBookmarkKeys.value.size) {
    bookmarkEditMode.value = false
    return
  }
  const items = store.bookmarks.filter((bookmark) => selectedBookmarkKeys.value.has(getBookmarkKey(bookmark)))
  await store.removeBookmarks(items)
  selectedBookmarkKeys.value.clear()
  bookmarkEditMode.value = false
  appStore.showToast(`&#x5DF2;&#x5220;&#x9664; ${items.length} &#x6761;&#x4E66;&#x7B7E;`, 'success')
}

function formatDate(ts?: number) {
  if (!ts) return ''
  const d = new Date(ts)
  return `${d.getMonth() + 1}-${d.getDate()} ${d.getHours()}:${String(d.getMinutes()).padStart(2, '0')}`
}
</script>

<style scoped>
.reader-catalog {
  display: flex;
  flex-direction: column;
  height: 100%;
}

.catalog-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 0 12px;
  border-bottom: 1px solid rgba(0,0,0,0.06);
  flex-shrink: 0;
  height: 56px;
}

.tabs {
  display: flex;
  gap: 20px;
  height: 100%;
}

.tab {
  height: 100%;
  display: flex;
  align-items: center;
  font-size: 15px;
  font-weight: 500;
  cursor: pointer;
  opacity: 0.6;
  border-bottom: 2px solid transparent;
  padding: 0 4px;
}

.tab.active {
  opacity: 1;
  color: var(--color-primary, #c97f3a);
  border-bottom-color: var(--color-primary, #c97f3a);
}

/* ─── 头部图标按钮 ───
   与换源面板保持同一规格：30px 热区、16px 图标、1.8 描边、圆头线帽。
   原来的 32px + 17px + stroke-width 2 让图标顶满按钮，线条偏粗，
   和「目录 / 书签」这种细字标签摆在一起很抢眼。 */
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
  opacity: 0.35;
  cursor: default;
}

.icon-btn:focus-visible,
.close-btn:focus-visible {
  outline: 2px solid color-mix(in srgb, var(--color-primary, #c97f3a) 55%, transparent);
  outline-offset: 1px;
}

/* 刷新期间让图标自转：目录刷新可能要走几十页，没有反馈会让人以为没点到。 */
.icon-btn .spinning {
  animation: catalog-refresh-spin 0.9s linear infinite;
}

@keyframes catalog-refresh-spin {
  to { transform: rotate(360deg); }
}

.header-actions {
  display: flex;
  align-items: center;
  gap: 2px;
}

.list-container {
  flex: 1;
  overflow-y: auto;
  padding: 4px 0 8px;
  -webkit-overflow-scrolling: touch;
  overscroll-behavior: contain;
  /* 目录列表的滚动条常显且加宽, 方便直接抓住拖动定位。 */
  scrollbar-width: auto;
  scrollbar-color: color-mix(in srgb, currentColor 42%, transparent) transparent;
  scrollbar-gutter: stable;
}

.list-container::-webkit-scrollbar {
  width: 14px;
  height: 14px;
}

.list-container::-webkit-scrollbar-track {
  background: color-mix(in srgb, currentColor 8%, transparent);
  border-radius: 999px;
}

/* 用 3px 透明边框把视觉宽度收成 8px, 同时保留 14px 的抓取区域。 */
.list-container::-webkit-scrollbar-thumb {
  min-height: 48px;
  border: 3px solid transparent;
  border-radius: 999px;
  background: color-mix(in srgb, currentColor 42%, transparent);
  background-clip: content-box;
}

.list-container::-webkit-scrollbar-thumb:hover {
  background: color-mix(in srgb, currentColor 62%, transparent);
  background-clip: content-box;
}

.chapter-toolbar {
  flex-shrink: 0;
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 12px 16px;
  border-bottom: 1px solid rgba(0,0,0,0.06);
}

.search-box {
  flex: 1;
  min-width: 0;
  position: relative;
}

.search-input {
  width: 100%;
  padding: 8px 32px 8px 12px;
  border: 1px solid rgba(0,0,0,0.1);
  border-radius: 8px;
  background: rgba(0,0,0,0.03);
  color: inherit;
  font-size: 14px;
  outline: none;
  transition: all 0.2s;
}

.search-input:focus {
  border-color: var(--color-primary, #c97f3a);
  background: rgba(0,0,0,0.02);
}

.search-clear {
  position: absolute;
  right: 8px;
  top: 50%;
  transform: translateY(-50%);
  width: 20px;
  height: 20px;
  display: flex;
  align-items: center;
  justify-content: center;
  border-radius: 50%;
  background: rgba(0,0,0,0.1);
  color: inherit;
  border: none;
  cursor: pointer;
  opacity: 0.6;
  transition: opacity 0.2s;
}

.search-clear:hover {
  opacity: 1;
}

.search-clear svg {
  width: 12px;
  height: 12px;
}

.chapter-jump-actions {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  flex: 0 0 auto;
}

.jump-btn {
  width: 36px;
  height: 36px;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  border-radius: 8px;
  border: 1px solid rgba(0,0,0,0.08);
  background: rgba(0,0,0,0.025);
  color: inherit;
  opacity: 0.7;
  cursor: pointer;
  transition: all 0.2s;
}

.jump-btn:hover:not(:disabled) {
  opacity: 1;
  border-color: rgba(201, 127, 58, 0.35);
  background: rgba(201, 127, 58, 0.08);
  color: var(--color-primary, #c97f3a);
}

.jump-btn:disabled {
  opacity: 0.28;
  cursor: default;
}

.jump-btn svg {
  width: 17px;
  height: 17px;
}

.bookmark-toolbar {
  display: flex;
  gap: 10px;
  padding: 12px 16px 0;
  flex-wrap: wrap;
}

.bookmark-action {
  border: 1px solid rgba(0,0,0,0.08);
  border-radius: 999px;
  background: transparent;
  color: inherit;
  padding: 8px 12px;
  font-size: 12px;
  cursor: pointer;
}

.bookmark-action.primary {
  border-color: var(--color-primary, #c97f3a);
  color: var(--color-primary, #c97f3a);
}

.bookmark-action.danger {
  border-color: #ef4444;
  color: #ef4444;
}

.loading, .empty {
  padding: 40px;
  text-align: center;
  opacity: 0.5;
  font-size: 14px;
}

.list-item {
  display: flex;
  align-items: center;
  gap: 12px;
  padding: 12px 20px;
  cursor: pointer;
  transition: all 0.2s;
  border-bottom: 1px solid rgba(0,0,0,0.02);
}

.list-item:hover {
  background: rgba(0,0,0,0.03);
}

.list-item.active {
  color: var(--color-primary, #c97f3a);
  background: rgba(201, 127, 58, 0.05);
}

.list-item.read:not(.active) .item-title {
  opacity: 0.74;
}

/* 序号列: 数字贴左起排, 已缓存圆点跟在其右侧。
   用 flex-start 而非 flex-end: 靠右对齐时, 圆点占走的宽度会把有缓存的那行
   数字顶向左, 无缓存的行数字却停在右边, 同一列序号就对不齐。
   min-width 而非固定 width: 三位数章节号(100+)进来时不会被挤扁或换行。 */
.item-index {
  display: inline-flex;
  align-items: center;
  justify-content: flex-start;
  gap: 5px;
  min-width: 30px;
  flex-shrink: 0;
}

.index-num {
  font-size: 11px;
  opacity: 0.4;
  font-variant-numeric: tabular-nums;
}

/* 已缓存: 一个实心小圆点, 贴在序号右侧。用主色但压低不透明度, 只是"这章
   本地有货"的旁注, 不与「当前」「已读」抢注意力。 */
.cached-dot {
  width: 5px;
  height: 5px;
  border-radius: 50%;
  flex-shrink: 0;
  background: var(--color-primary, #c97f3a);
  opacity: 0.55;
}

/* 悬停看全名: 外层是裁剪窗口, 内层 .item-title-text 由 startTitleScroll 用
   Web Animations API 横向位移, 露出被省略号截断的尾部。
   必须是两层: 对 overflow: hidden 的元素本身做 translate 时, 裁剪框会跟着
   一起移动, 文字相对可见区没有任何位移 —— 看起来就是"不会滚动"。
   静止时仍是省略号, 列表密度与阅读位置都不动。 */
.item-title {
  font-size: 14px;
  line-height: 1.4;
  flex: 1;
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  cursor: default;
}

/* 内层承载文字, 也是被动画的那个元素。
   它必须自己也 nowrap 且不换行, 否则移动到边界时文字会折行。 */
.item-title-text {
  display: inline-block;
  white-space: nowrap;
  vertical-align: top;
}

/* 章节字数: 贴在最右侧的小字, 与标题之间留出弹性空档。
   只做轻量提示, 不与「当前」「已读」抢视觉焦点, 也不额外占一行高度。 */
.word-count {
  flex-shrink: 0;
  font-size: 11px;
  line-height: 1.4;
  white-space: nowrap;
  opacity: 0.42;
  font-variant-numeric: tabular-nums;
  letter-spacing: 0.02em;
}

.item-status {
  display: flex;
  align-items: center;
  gap: 6px;
  flex-shrink: 0;
}

.status-badge {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  padding: 2px 8px;
  border-radius: 999px;
  font-size: 11px;
  line-height: 1.4;
  white-space: nowrap;
  border: 1px solid transparent;
}

/* 「当前」用淡色底 + 主色字, 「已缓存」用同色系更弱的底: 靠强度分层,
   不用实心色块 —— 实心「当前」会在滚动时变成整列最重的一块。 */
.status-badge.current {
  color: var(--color-primary, #c97f3a);
  background: rgba(201, 127, 58, 0.12);
  border-color: rgba(201, 127, 58, 0.2);
}

.status-badge.read {
  color: inherit;
  background: color-mix(in srgb, currentColor 10%, transparent);
  border-color: color-mix(in srgb, currentColor 14%, transparent);
  opacity: 0.72;
}

/* Bookmark items */
.bookmark-item {
  flex-direction: column;
  gap: 6px;
  position: relative;
  padding-left: 48px;
}

.bookmark-item.selected {
  background: rgba(201, 127, 58, 0.08);
}

.bookmark-check {
  position: absolute;
  left: 16px;
  top: 16px;
  width: 22px;
  height: 22px;
  border-radius: 50%;
  border: 1px solid rgba(0,0,0,0.15);
  background: transparent;
  color: transparent;
  cursor: pointer;
}

.bookmark-check.checked {
  background: var(--color-primary, #c97f3a);
  border-color: var(--color-primary, #c97f3a);
  color: #fff;
}

.bm-header {
  width: 100%;
  display: flex;
  justify-content: space-between;
  align-items: center;
}

.bm-chapter {
  font-size: 13px;
  font-weight: 600;
  opacity: 0.9;
}

.bm-time {
  font-size: 11px;
  opacity: 0.4;
}

.bm-snippet {
  font-size: 12px;
  opacity: 0.6;
  display: -webkit-box;
  -webkit-line-clamp: 2;
  -webkit-box-orient: vertical;
  overflow: hidden;
  line-height: 1.5;
}

.bm-delete {
  position: absolute;
  right: 12px;
  bottom: 12px;
  width: 28px;
  height: 28px;
  display: flex;
  align-items: center;
  justify-content: center;
  opacity: 0;
  background: rgba(255,0,0,0.05);
  color: #ef4444;
  border: none;
  border-radius: 4px;
  cursor: pointer;
  transition: 0.2s;
}

.bookmark-item:hover .bm-delete {
  opacity: 1;
}

.bm-delete svg {
  width: 14px;
  height: 14px;
}

/*
 * 迷你模式的紧凑尺寸。
 *
 * 小窗(默认 320×240)里, 常规尺寸的头部(56px)加工具栏(61px)会占掉抽屉一半以上,
 * 列表只剩约 92px —— 只能看到两条章节。这里把这两块压到 43+42px, 列表可用高度
 * 提升约 36%(实测 92px → 125px, 可见章节 2 条 → 3 条)。
 *
 * 只收尺寸, 不动配色与交互: 分类在 .mini 作用域内, 大窗口完全不受影响。
 */
.reader-catalog.mini .catalog-header {
  height: 42px;
}

.reader-catalog.mini .tabs {
  gap: 14px;
}

.reader-catalog.mini .tab {
  font-size: 13px;
}

.reader-catalog.mini .icon-btn,
.reader-catalog.mini .close-btn {
  width: 26px;
  height: 26px;
}

/* 图标也要跟着缩：26px 热区里放 16px 图标只剩 5px 留白，看着顶手。 */
.reader-catalog.mini .icon-btn svg,
.reader-catalog.mini .close-btn svg {
  width: 14px;
  height: 14px;
}

.reader-catalog.mini .chapter-toolbar {
  padding: 7px 10px;
  gap: 6px;
}

.reader-catalog.mini .search-input {
  padding: 6px 26px 6px 10px;
  font-size: 12px;
}

.reader-catalog.mini .jump-btn {
  width: 26px;
  height: 26px;
}

.reader-catalog.mini .list-container {
  padding: 2px 0 6px;
  /* 小窗里滚动条收窄一档, 但仍保留可抓取的宽度。 */
  scrollbar-width: thin;
}

.reader-catalog.mini .list-container::-webkit-scrollbar {
  width: 11px;
}

.reader-catalog.mini .list-item {
  padding: 7px 10px;
  gap: 8px;
}

.reader-catalog.mini .item-index {
  width: 28px;
  gap: 4px;
}

.reader-catalog.mini .index-num {
  font-size: 10px;
}

.reader-catalog.mini .cached-dot {
  width: 4px;
  height: 4px;
}

.reader-catalog.mini .item-title {
  font-size: 12px;
}

.reader-catalog.mini .status-badge {
  padding: 1px 5px;
  font-size: 10px;
}

.reader-catalog.mini .word-count {
  font-size: 10px;
}

.reader-catalog.mini .item-status {
  gap: 4px;
}

.reader-catalog.mini .loading,
.reader-catalog.mini .empty {
  padding: 20px;
  font-size: 12px;
}
</style>
