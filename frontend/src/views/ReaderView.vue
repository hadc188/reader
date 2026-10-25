<template>
  <div
    class="reader-view"
    :class="{ 'disable-system-callout': disableSystemCallout }"
    :style="{
      background: shellPaintsBackground ? 'transparent' : theme.body,
      color: theme.fontColor,
      colorScheme: readerColorScheme,
      fontFamily: currentFontFamily,
      '--color-primary': '#c97f3a',
      '--reader-selection-bg': theme.fontColor,
      '--reader-selection-color': theme.content,
    }"
    @click="handleBackgroundClick"
    @pointermove="handleReaderPointerMove"
    @pointerleave="handleReaderPointerLeave"
    @contextmenu.prevent="handleContextMenu"
  >

    <!-- 章节加载阶段：Teleport 到 body，避免被阅读容器的层叠/定位影响 -->
    <Teleport to="body">
      <Transition name="fade">
        <div
          v-if="loadStageText"
          class="chapter-loading-stage"
          :style="{ background: chromeTheme.popup, color: chromeTheme.fontColor }"
        >
          <span class="chapter-loading-spinner" aria-hidden="true"></span>
          <span>{{ loadStageText }}</span>
        </div>
      </Transition>
    </Teleport>

    <!-- Left Drawer Panels -->
    <Teleport to="body">
      <Transition name="fade">
        <div v-if="store.activePanel" class="reader-overlay" @click="store.closePanel()"></div>
      </Transition>
      <Transition name="slide-left">
        <div
          v-if="store.activePanel"
          class="reader-drawer"
          :class="{
            'with-custom-background': hasReaderBackground,
            'drawer-translucent': hasTranslucentPanel,
            'drawer-wide': store.activePanel === 'settings',
          }"
          :style="{ background: chromeTheme.popup, color: chromeTheme.fontColor, colorScheme: readerColorScheme }"
        >
          <ReaderCatalog
            v-if="store.activePanel === 'catalog' || store.activePanel === 'bookmark'"
            :initial-tab="store.activePanel === 'bookmark' ? 'bookmarks' : 'chapters'"
            :mini="appStore.miniMode"
            @jump-chapter="jumpFromCatalog"
          />
          <ReadSettings v-else-if="store.activePanel === 'settings'" />
          <ReaderBookshelf v-else-if="store.activePanel === 'bookshelf'" />
          <ReaderSource v-else-if="store.activePanel === 'source'" />
          <ReplaceRuleManager v-else-if="store.activePanel === 'rule'" />
          <CacheManager v-else-if="store.activePanel === 'cache'" />
        </div>
      </Transition>
    </Teleport>

    <!--
      阅读页控件统一用 ReaderMobileControls(点击正文呼出上/下栏 + 左右悬浮钮),
      宽屏与迷你模式共用同一套交互与外观。
      历史: 宽屏曾走"鼠标移到左右边缘滑出竖栏"的独立组件, 已随本次统一删除
      (见 backups/2026-10-05-review-fixes/)。
    -->
    <ReaderMobileControls
      :show="showControls || !!store.activePanel"
      :mini="appStore.miniMode"
      @goHome="goHome"
      @scrollTop="scrollToTop"
      @scrollBottom="scrollToBottom"
      @prev="prevChapter"
      @next="nextChapter"
      @bookmark="toggleBookmark"
      @search="openSearch"
      @info="openInfo"
      @tts="handleTTS"
      @progress="openCachePanel"
      @seekChapter="jumpFromCatalog"
    />

    <Transition name="auto-reading-status">
      <div
        v-if="store.isAutoScrolling"
        class="auto-reading-status"
        role="status"
        aria-live="polite"
        :style="{
          '--auto-status-bg': chromeTheme.popup,
          '--auto-status-color': chromeTheme.fontColor,
        }"
        @click.stop
      >
        <span class="auto-reading-indicator" aria-hidden="true"></span>
        <span class="auto-reading-label">自动翻页中</span>
        <span class="auto-reading-mode">平滑</span>
        <button type="button" title="停止自动翻页" aria-label="停止自动翻页" @click="stopAutoReadingFromStatus">
          <span aria-hidden="true"></span>
        </button>
      </div>
    </Transition>

    <ReaderTtsPanel
      :show="showTTSPanel"
      :state="ttsPanelDisplayState"
      :mini="appStore.miniMode"
      :theme="chromeTheme"
      :chapter-title="store.currentChapter?.title"
      :provider="store.speechConfig.provider"
      :provider-label="store.speechProviderLabel"
      :is-speaking="store.isSpeaking"
      :is-loading="store.isSpeechLoading"
      :is-paused="store.isPaused"
      :progress="store.speechProgress"
      :voices="store.voiceList"
      :voice-name="store.speechConfig.voiceName"
      :rate="store.speechConfig.speechRate"
      :pitch="store.speechConfig.speechPitch"
      :supports-pitch="store.speechConfig.provider === 'system'"
      :openai-model="store.speechConfig.openaiModel"
      :openai-voice="store.speechConfig.openaiVoice"
      :stop-after-minutes="store.speechConfig.stopAfterMinutes"
      :timer-text="speechTimerText"
      @close="closeTTSPanel"
      @prev="speechPrev"
      @toggle-play="toggleSpeechFromPanel"
      @stop="handleStopTTS"
      @next="speechNext"
      @voice-change="changeVoice"
      @openai-voice-change="changeOpenAIVoice"
      @rate-change="adjustSpeechRate"
      @pitch-change="adjustSpeechPitch"
      @timer-change="setSpeechTimer"
      @update:state="setTtsPanelState"
    />

    <!-- Main Content Area -->
    <div
      class="reader-scroll-container"
      :class="{
        'horizontal-page-mode': isHorizontalPageMode,
        'tts-panel-open': ttsPanelOpen,
      }"
      :style="{ '--tts-panel-inset': `${ttsPanelInset}px` }"
      ref="scrollContainerRef"
      @scroll="handleScroll"
      @load.capture="handleReaderImageLoad"
      @mousedown="handleManualReadingInput"
      @wheel="handleManualReadingInput"
      @touchstart="handleTouchStart"
      @touchmove="handleTouchMove"
      @touchend="handleTouchEnd"
      @click="handleGlobalClick"
    >
      <div v-if="showChapterLoading" class="content-loading">
        <div class="loading-spinner"></div>
      </div>

      <!-- 打开书失败：不能只留一个转圈。目录抓取失败(书源地址不匹配、tocUrl 为空、
           站点超时)在旧实现里会永久停在加载态，用户只能反复换源摸索。 -->
      <div v-else-if="store.bookLoadError" class="book-load-error">
        <p class="book-load-error-title">加载失败</p>
        <p class="book-load-error-message">{{ store.bookLoadError }}</p>
        <div class="book-load-error-actions">
          <button type="button" class="book-load-error-btn primary" @click="retryOpenBook">重试</button>
          <!-- 换源面板靠 IPC 流探测可用书源, Web 端没有对应能力(见 ReaderSource) -->
          <button
            v-if="!appStore.isWebMode"
            type="button"
            class="book-load-error-btn"
            @click="openSourcePanelFromError"
          >换源</button>
          <button type="button" class="book-load-error-btn" @click="goHome">返回书架</button>
        </div>
      </div>

      <div v-else-if="offlineBannerText" class="offline-banner">
        {{ offlineBannerText }}
      </div>

      <article
        v-if="!showChapterLoading && !store.bookLoadError && !isContinuousMode"
        class="chapter-content"
        :class="{ 'horizontal-page-article': isHorizontalPageMode }"
        :style="{
          maxWidth: isHorizontalPageMode ? 'none' : (config.pageWidth + 'px'),
          fontFamily: currentFontFamily,
          fontSize: config.fontSize + 'px',
          fontWeight: config.fontWeight,
          lineHeight: config.lineHeight,
          '--reader-page-width': config.pageWidth + 'px',
          '--reader-side-padding': '24px',
          '--reader-page-vertical-padding': config.pageVerticalPadding + 'px',
          '--reader-page-step': horizontalPageStepStyle,
          '--reader-text-width': horizontalTextWidth + 'px',
        }"
      >
        <!-- 章节加载阶段：段评解析可能耗时数秒。做成浮层，三种阅读模式都能看到。 -->
        <div v-if="isHorizontalPageMode" class="horizontal-page-layout">
          <section class="horizontal-content-page">
            <div ref="chapterTextRef" class="horizontal-pages">
              <section
                v-for="(page, idx) in horizontalPages"
                :key="`h-page-${idx}`"
                class="horizontal-page"
                :style="{
                  transform: horizontalPageTransform(idx),
                  transitionDuration: horizontalPageTransitionDuration,
                  willChange: horizontalPageWillChange(idx),
                }"
              >
                <div
                  class="chapter-text horizontal-page-content"
                  :style="{
                    '--p-spacing': config.paragraphSpacing + 'em',
                  }"
                  v-html="page"
                ></div>
              </section>
            </div>
          </section>
        </div>

        <div v-else>
          <div class="chapter-title">{{ store.currentChapter?.title || '加载中...' }}</div>

          <!-- 脚本规则没跑成时正文会回落到「不含脚本」的版本，与脚本本来就没启用
               看起来完全一样。必须显式说明，否则用户只会以为规则没效果。 -->
          <div v-if="store.replaceRulesDegraded" class="replace-rule-degraded">
            本章有 {{ store.failedScriptRuleCount }} 条净化脚本未能执行，正文按未执行脚本处理。
            <button type="button" class="replace-rule-degraded-link" @click="store.openPanel('rule')">
              查看规则
            </button>
          </div>

          <div
            ref="chapterTextRef"
            class="chapter-text"
            :style="{
              '--p-spacing': config.paragraphSpacing + 'em',
            }"
            v-html="formattedContent"
          ></div>

          <div class="chapter-footer">
            <button class="next-btn" :disabled="!store.hasNext" @click="nextChapter">
              {{ store.hasNext ? '下一章' : '没有更多了' }}
            </button>
          </div>
        </div>
      </article>

      <div
        v-if="!showChapterLoading && !store.bookLoadError && isContinuousMode"
        class="continuous-reading"
        :style="{
          maxWidth: config.pageWidth + 'px',
          fontFamily: currentFontFamily,
          fontSize: config.fontSize + 'px',
          fontWeight: config.fontWeight,
          lineHeight: config.lineHeight,
        }"
      >
        <div v-if="continuousLoadingPrev" class="continuous-loading-inline">正在加载上一章...</div>

        <section
          v-for="chapter in continuousChapters"
          :key="chapter.index"
          class="chapter-content continuous-chapter"
          :data-chapter-index="chapter.index"
        >
          <div class="chapter-title">{{ chapter.title }}</div>

          <div
            class="chapter-text"
            data-role="continuous"
            :data-chapter-index="chapter.index"
            :style="{
              '--p-spacing': config.paragraphSpacing + 'em',
            }"
            v-html="chapter.html"
          ></div>
        </section>

        <div v-if="continuousLoadingNext" class="continuous-loading-inline">正在加载下一章...</div>
      </div>
    </div>



    <ReaderSearchPanel
      :show="showSearch"
      :theme="chromeTheme"
      :query="searchQuery"
      :results="searchResults"
      :active-index="searchIndex"
      :count="searchCount"
      :status="bookSearchStatus"
      @close="closeSearch"
      @search="runSearch"
      @next="nextSearchResult"
      @prev="prevSearchResult"
      @update:query="searchQuery = $event"
      @jump="jumpToSearchResult"
    />

    <Transition name="fade">
      <div
        v-if="selectionMenu.visible"
        class="selection-menu"
        :class="{ 'with-custom-background': hasReaderBackground }"
        @click.stop
        :style="{
          top: selectionMenu.top + 'px',
          left: selectionMenu.left + 'px',
          background: chromeTheme.popup,
          color: chromeTheme.fontColor,
        }"
      >
        <div class="selection-menu-text">{{ selectionMenu.text }}</div>
        <div class="selection-menu-actions">
          <button @click="() => addSelectionBookmark()">加入书签</button>
          <button @click="addSelectionReplaceRule('book')">按本书替换</button>
          <button @click="addSelectionReplaceRule('source')">按书源替换</button>
        </div>
      </div>
    </Transition>

    <Transition name="fade">
      <div
        v-if="readerContextMenu.visible"
        class="reader-context-menu"
        :style="{ top: `${readerContextMenu.top}px`, left: `${readerContextMenu.left}px`, background: chromeTheme.popup, color: chromeTheme.fontColor }"
        @click.stop
      >
        <button v-if="readerContextMenu.text" @click="searchContextSelection">搜索选中内容</button>
        <button v-if="readerContextMenu.text" @click="replaceContextSelection">替换</button>
        <button @click="bookmarkContextSelection">加入书签</button>
        <button :disabled="store.currentIndex <= 0 || store.loading" @click="prevChapterFromContextMenu">上一章</button>
        <button :disabled="!store.hasNext || store.loading" @click="nextChapterFromContextMenu">下一章</button>
        <button @click="openCatalogFromContextMenu">目录</button>
        <button
          v-if="!isLocalBook(store.book) && !appStore.isWebMode"
          @click="openSourceFromContextMenu"
        >换源</button>
        <button @click="toggleAutoReadingFromContextMenu">{{ store.isAutoScrolling ? '停止自动翻页' : '开始自动翻页' }}</button>
        <button @click="refreshFromContextMenu">刷新</button>
      </div>
    </Transition>

    <Teleport to="body">
      <Transition name="fade">
        <div
          v-if="replaceDialog.visible"
          class="reader-replace-overlay"
          @click.self="closeReplaceDialog"
        >
          <div
            class="reader-replace-dialog"
            role="dialog"
            aria-modal="true"
            aria-labelledby="reader-replace-title"
            :style="{ background: chromeTheme.popup, color: chromeTheme.fontColor }"
          >
            <h3 id="reader-replace-title">替换选中内容</h3>
            <label class="reader-replace-field">
              <span>匹配内容</span>
              <textarea v-model="replaceDialog.pattern" rows="3"></textarea>
            </label>
            <label class="reader-replace-field">
              <span>替换为</span>
              <textarea v-model="replaceDialog.replacement" rows="3" placeholder="留空则删除"></textarea>
            </label>
            <label class="reader-replace-check">
              <input v-model="replaceDialog.isRegex" type="checkbox">
              <span>使用正则表达式</span>
            </label>
            <div class="reader-replace-field">
              <span>生效范围</span>
              <div class="reader-replace-scopes">
                <button type="button" :class="{ active: replaceDialog.scope === 'book' }" @click="replaceDialog.scope = 'book'">本书</button>
                <button type="button" :class="{ active: replaceDialog.scope === 'source' }" @click="replaceDialog.scope = 'source'">当前书源</button>
                <button type="button" :class="{ active: replaceDialog.scope === 'global' }" @click="replaceDialog.scope = 'global'">全部书籍</button>
              </div>
            </div>
            <div class="reader-replace-actions">
              <button type="button" @click="closeReplaceDialog">取消</button>
              <button type="button" class="primary" :disabled="replaceDialog.saving || !replaceDialog.pattern.trim()" @click="saveReplaceDialog">
                {{ replaceDialog.saving ? '保存中...' : '保存' }}
              </button>
            </div>
          </div>
        </div>
      </Transition>
    </Teleport>

    <BookDetailModal
      v-model="showBookInfo"
      :book="bookInfoBook"
    />

    <SourceBrowserPanel :request="sourceBrowser" :background="chromeTheme.popup" :color="chromeTheme.fontColor" @close="sourceBrowser = null" @toast="(message: string) => appStore.showToast(message)" @import-link="importFromLink" />
  </div>
</template>

<script setup lang="ts">
import { ref, computed, watch, onMounted, onUnmounted, nextTick, defineAsyncComponent } from 'vue'
import { onBeforeRouteLeave, useRouter } from 'vue-router'
import { useReaderStore, fontPresets, TOC_END_CHECK_INTERVAL_MS } from '../stores/reader'
import { useAppStore } from '../stores/app'
import { isTauriRuntime } from '../api/transport'
import { getBookInfo, getBookshelfWithCacheInfo } from '../api/bookshelf'
import { evalSourceClick, type ShowBrowserRequest } from '../api/sourceClick'
import { applySystemTheme } from '../utils/systemUi'
import { APP_VIEWPORT_CHANGE_EVENT, syncViewportSize } from '../utils/viewport'
import { isReaderInteractiveClickTarget } from '../utils/readerClick'
import {
  READER_HOVER_HIDE_DELAY_MS,
  isReaderHoverPointInZone,
  shouldRevealReaderControls,
} from '../utils/readerHoverControls'
import { resolveReaderBackTarget } from '../utils/readerBack'
import { shouldPreferServerReadingPosition, toMillis, type ReadingPositionSnapshot } from '../utils/readingPosition'
import { isLocalBook, isLocalEpubBook } from '../utils/localBook'
import { PDF_PAGE_ATTR, isPdfBook, releasePdfBook, renderPdfChapterPages } from '../utils/pdfPages'
import { rewriteCssAssetUrls, toReaderAssetUrl } from '../utils/localEpubAsset'
import { processSourceImageOptions, reserveImageSpace } from '../utils/sourceImageOptions'
import { repairSourceImageHtml } from '../utils/sourceImageHtml'
import { handleReaderFontSizeWheel } from '../utils/readerFontSize'
import { createReaderProgressAutoSaveScheduler, createReaderProgressExitSaver } from '../utils/readerProgressAutoSave'
import {
  animatePageScrollBy,
  animatePageScrollTo,
  cancelPageScrollAnimation,
  getReaderPageStep,
} from '../utils/readerPaging'
import {
  clampReaderPageIndex,
  readerLandingPageIndexAfterPrevChapter,
  readerPageIndexFromProgress,
} from '../utils/readerPageIndex'
import { splitParagraphsAroundExisting } from '../utils/readerParagraph'
import { resolveReaderBackground } from '../utils/readerColor'
import {
  hotkeyIdFromEvent,
  hotkeyIdFromMouseEvent,
  hotkeyIdFromWheelEvent,
  isActionAllowedWhilePanelOpen,
  isHotkeyCaptureActive,
  isMouseButtonHotkeyBlocked,
  isPointerHotkeyBlocked,
  mouseButtonHotkeyAction as resolveMouseButtonHotkeyAction,
  resolveHotkeyAction,
  type HotkeyActionId,
} from '../utils/readerHotkeys'
import type { Book } from '../types'

import ReaderMobileControls from '../components/reader/ReaderMobileControls.vue'
import SourceBrowserPanel from '../components/SourceBrowserPanel.vue'
import { useReaderSearch, type SearchLocateOptions } from '../composables/useReaderSearch'
import { useReaderSelection } from '../composables/useReaderSelection'
import { useHorizontalPaging, rememberDecodedImageSize } from '../composables/useHorizontalPaging'
import { useContinuousReading } from '../composables/useContinuousReading'
import { useReaderAutoPlayback } from '../composables/useReaderAutoPlayback'
import { useSourceImport } from '../composables/useSourceImport'

const ReaderCatalog = defineAsyncComponent(() => import('../components/reader/ReaderCatalog.vue'))
const ReadSettings = defineAsyncComponent(() => import('../components/reader/ReadSettings.vue'))
const ReaderBookshelf = defineAsyncComponent(() => import('../components/reader/ReaderBookshelf.vue'))
const ReaderSource = defineAsyncComponent(() => import('../components/reader/ReaderSource.vue'))
const ReplaceRuleManager = defineAsyncComponent(() => import('../components/reader/ReplaceRuleManager.vue'))
const CacheManager = defineAsyncComponent(() => import('../components/reader/CacheManager.vue'))
const BookDetailModal = defineAsyncComponent(() => import('../components/BookDetailModal.vue'))
const ReaderTtsPanel = defineAsyncComponent(() => import('../components/reader/ReaderTtsPanel.vue'))
/** 面板三态的运行时类型只用于本地状态标注, 与组件里的定义保持字面量一致即可。 */
type TtsPanelState = 'expanded' | 'bar' | 'bubble'
const ReaderSearchPanel = defineAsyncComponent(() => import('../components/reader/ReaderSearchPanel.vue'))

const router = useRouter()
const store = useReaderStore()
const appStore = useAppStore()
// 书源面板里点「一键导入」(yuedu:// / legado://) 时由这里完成入库。
const { importFromLink } = useSourceImport()
const READER_POSITION_PREFIX = 'reader-position:'
const SERVER_PROGRESS_AUTOSAVE_MS = 10000

type SavedReadingPosition = ReadingPositionSnapshot

const CONTINUOUS_POSITION_ANCHOR_RATIO = 0.12

function debugPositionLog(message: string, payload?: unknown) {
  console.log('[pos]', message, payload ?? '')
}

/** 书源脚本(java.log / java.toast)日志转发开关：排查书源面板时置 true。 */
const SOURCE_CLICK_LOG_ENABLED = false
function sourceClickLog(message: string, payload?: unknown) {
  if (!SOURCE_CLICK_LOG_ENABLED) return
  console.log('[source-click]', message, payload ?? '')
}

const config = computed(() => store.config)
/**
 * 章节加载阶段文案。
 *
 * 后端只在**真正发起网络请求**时才推阶段，缓存命中不会有任何阶段，
 * 所以这里为空即表示「没在加载」，不需要额外判断 `store.loading`。
 * 「解析」一步通常最慢（书源脚本 + 段评请求）。
 */
const loadStageText = computed(() => {
  switch (store.loadStage) {
    case 'fetching':
      return '正在获取章节…'
    case 'parsing':
      return '正在解析正文与段评…'
    case 'caching':
      return '正在保存到本地…'
    default:
      return ''
  }
})
const theme = computed(() => store.readerTheme)
const chromeTheme = computed(() => store.chromeTheme)
/** 阅读页背景来源: 专属图 > 自定义色 > 桌面图 > 主题。 */
const readerBackgroundSource = computed(() => resolveReaderBackground({
  customBackgroundColor: store.readerBackgroundColor,
  readerBackgroundImage: config.value.readerBackgroundImage,
  backgroundImage: config.value.backgroundImage,
  applyBackgroundToReader: config.value.applyBackgroundToReader,
}))
/** 背景图由 App 外壳在最底层统一铺(标题栏也在其内), 这里只要让开位置。
 *  阅读页自己再铺一层会导致同一张图叠两次、透明度相乘。 */
const shellPaintsBackground = computed(() => (
  readerBackgroundSource.value === 'readerImage' || readerBackgroundSource.value === 'desktopImage'
))
/** 工具栏等界面元素的「有背景图」判断, 供毛玻璃样式使用。 */
const hasReaderBackground = computed(() => shellPaintsBackground.value)
/** 浮层底板是否半透明(用户调低了「面板不透明度」)。半透明时才上毛玻璃:
 *  实色底板再叠 blur 只是白耗 GPU, 而模糊层在实色下也完全看不出来。 */
const hasTranslucentPanel = computed(() => store.config.panelOpacity < 1)
const readerColorScheme = computed(() => store.isNight || theme.value.name === '暗灰' ? 'dark' : 'light')

const scrollContainerRef = ref<HTMLElement>()
const chapterTextRef = ref<HTMLElement>()
/** 上/下工具栏是否呼出。代理到 store, 让标题栏(在 App 顶层渲染)也能读到,
 *  以便在呼出工具栏时从透明转为实色。 */
const showControls = computed({
  get: () => store.readerControlsVisible,
  set: (value: boolean) => { store.readerControlsVisible = value },
})
const isMobile = ref(false)

/* ─── 鼠标靠近边缘自动展开菜单栏 ───
 *  桌面环境(有真实指针、支持悬停)下, 指针进入阅读区 上/下/左/右 边缘的热区就展开
 *  上/下菜单栏与两侧悬浮钮, 离开热区即收回(READER_HOVER_HIDE_DELAY_MS)。
 *  触摸设备不启用(触摸没有「悬停」这一状态); 迷你窗口也不启用 —— 小窗只有
 *  240px 高, 上下各 60px 的热区会占掉一半高度, 鼠标随便一停就把菜单栏顶出来。
 *  热区边界与展开规则见 utils/readerHoverControls(已单测覆盖)。 */
const supportsHoverPointer = ref(false)
/** 指针是否停在边缘热区内(含热区里的正文)。 */
let hoverPointerInZone = false
/** 指针是否停在菜单栏自身上。菜单栏比热区高(底栏两行), 指针停在其上视为"没走开"。 */
let hoverPointerOnControls = false
/** 点击收起后, 直到指针离开热区前都不再自动展开 —— 否则贴着边缘点正文会立刻弹回。 */
let hoverRevealSuppressed = false
let hoverHideTimer: number | null = null
let hoverMediaQuery: MediaQueryList | null = null
let hoverSupportStop: (() => void) | null = null

/** 热区以**阅读区**为基准, 而不是整个窗口: 阅读区顶边在标题栏之下,
 *  若按 window 从顶边算, 顶部 60px 热区会被标题栏吃掉大半。
 *  坐标一并换算成相对阅读区的, 与 rect 尺寸配成一对。 */
function readerHoverFrame(element: EventTarget | null) {
  const rect = (element as HTMLElement | null)?.getBoundingClientRect?.()
  if (!rect || rect.width <= 0 || rect.height <= 0) return null
  return {
    rect,
    viewport: { width: rect.width, height: rect.height },
  }
}

function cancelHoverHide() {
  if (hoverHideTimer !== null) {
    clearTimeout(hoverHideTimer)
    hoverHideTimer = null
  }
}

/** 收起菜单栏。延迟为 0 时直接收, 不留异步尾巴(与 readest 的 mouseleave 立即隐藏一致)。 */
function hideControlsNow() {
  // 面板占用界面期间不收起。光标若一动不动, 面板关闭后单靠 pointermove 无法
  // 再纠正, 所以由下面 watch(store.activePanel) 补一次判定。
  if (store.activePanel) return
  showControls.value = false
}

/** 指针离开热区后收起: 延迟见 READER_HOVER_HIDE_DELAY_MS(当前为 0, 即立即收起)。 */
function scheduleHoverHide() {
  cancelHoverHide()
  if (READER_HOVER_HIDE_DELAY_MS <= 0) {
    hideControlsNow()
    return
  }
  hoverHideTimer = window.setTimeout(() => {
    hoverHideTimer = null
    hideControlsNow()
  }, READER_HOVER_HIDE_DELAY_MS)
}

/** 搜索/朗读/书详情/书源面板等浮层打开时不自动展开。 */
function readerOverlayOpen() {
  return showSearch.value || showTTSPanel.value || showBookInfo.value || !!sourceBrowser.value
}

function handleReaderPointerMove(event: PointerEvent) {
  // 触摸不产生「悬停」语义, 仍走点击呼出。这里要先把指针状态清掉: 触摸屏笔记本上
  // 鼠标可能刚把状态留成"停在热区里", 不清就会与点击后的静默叠加成死角。
  if (!supportsHoverPointer.value || event.pointerType === 'touch') {
    hoverPointerInZone = false
    hoverPointerOnControls = false
    hoverRevealSuppressed = false
    return
  }
  const target = event.target as Element | null
  hoverPointerOnControls = !!target?.closest?.('.mobile-controls')
  const frame = readerHoverFrame(event.currentTarget)
  const inZone = !!frame && isReaderHoverPointInZone(
    { x: event.clientX - frame.rect.left, y: event.clientY - frame.rect.top },
    frame.viewport,
  )
  hoverPointerInZone = inZone

  if (!inZone && !hoverPointerOnControls) {
    // 指针走开即解除点击后的静默。
    hoverRevealSuppressed = false
    // 只按"指针停在菜单栏上"豁免收起: 热区里还有正文, 停在那儿滚动不该让菜单栏常驻。
    if (showControls.value) scheduleHoverHide()
    return
  }

  cancelHoverHide()
  if (hoverRevealSuppressed) return
  if (!frame) return
  if (shouldRevealReaderControls(
    { x: event.clientX - frame.rect.left, y: event.clientY - frame.rect.top },
    frame.viewport,
    { panelOpen: !!store.activePanel, overlayOpen: readerOverlayOpen() },
  )) {
    showControls.value = true
  }
}

/** 指针离开整个阅读区(移出窗口/移到标题栏等外层区域)时按"离开热区"处理。 */
function handleReaderPointerLeave(event: PointerEvent) {
  if (!supportsHoverPointer.value || event.pointerType === 'touch') {
    hoverPointerInZone = false
    hoverPointerOnControls = false
    hoverRevealSuppressed = false
    return
  }
  hoverPointerInZone = false
  hoverPointerOnControls = false
  hoverRevealSuppressed = false
  if (showControls.value) scheduleHoverHide()
}

/** 点击收起菜单栏(正文中部、正文空白处共用)。 */
function collapseControlsByClick() {
  cancelHoverHide()
  showControls.value = false
  // 悬停模式下指针多半还贴着边缘, 静默到它离开热区为止。
  if (supportsHoverPointer.value && (hoverPointerInZone || hoverPointerOnControls)) {
    hoverRevealSuppressed = true
  }
}

/** 是否支持悬停展开: 桌面指针 + 非迷你窗口 + 用户没有关掉这个功能。 */
function syncHoverPointerSupport() {
  supportsHoverPointer.value = !!hoverMediaQuery?.matches
    && !appStore.miniMode
    && config.value.hoverRevealControls
  if (!supportsHoverPointer.value) {
    hoverPointerInZone = false
    hoverPointerOnControls = false
    hoverRevealSuppressed = false
    cancelHoverHide()
  }
}

/** 面板打开时不自动展开(面板已占满视线), 关闭后补一次收起判定。 */
watch(() => store.activePanel, (open) => {
  if (open) {
    cancelHoverHide()
    return
  }
  if (showControls.value && !hoverPointerInZone && !hoverPointerOnControls) scheduleHoverHide()
})

let speechTimerTicker: number | null = null
let readerViewUnmounted = false
/** 挂载流程是否已跑完。resize 的重新分页要等它结束: 挂载自己会重建一次分页,
 *  提前触发会和它撞在一起(见 checkMedia 的注释)。 */
let mounted = false
let suppressNextTapUntil = 0
const readerContextMenu = ref({ visible: false, top: 0, left: 0, text: '' })
const replaceDialog = ref({
  visible: false,
  pattern: '',
  replacement: '',
  scope: 'book' as 'book' | 'source' | 'global',
  isRegex: false,
  saving: false,
})
const sourceBrowser = ref<ShowBrowserRequest | null>(null)

function openSourceBrowserPanel(request: ShowBrowserRequest) {
  if (!request.panelId) {
    console.warn('[source-click] showBrowser 缺少 panelId')
    return
  }
  sourceBrowser.value = request
}
let restorePositionTimer: number | null = null
let persistPositionTimer: number | null = null
let legadoPositionRestoreTimer: number | null = null
let legadoPositionRestoreAttempts = 0
let legadoPositionRestoreSettling = false
let legadoPositionRestoreTargetTop = 0
let legadoPositionRestoreTargetMaxScroll = 0
const pendingRestorePosition = ref<SavedReadingPosition | null>(null)
const pendingLegadoPageRestore = ref<{
  index: number
  position: number
  progress: number
  token: number
} | null>(null)
let pendingRestoreAttempts = 0
let skipPositionRestoreIndex: number | null = null

/** 「往前翻跨章」的落位意图: 目标章(上一章)的索引, 期望落在它的**末页**。
 *  分页重建完成后由 applyPendingPrevChapterLanding 消费(见那边的说明)。 */
let pendingPrevChapterLandingIndex: number | null = null

/** 标记某章节的进入来自滚动/主动跳转, 无需恢复上次阅读位置。
 *
 *  这里**不能**用"带过期的乐观标记"来实现: 跨章翻页要等书源正文返回才真正切
 *  过去, 网络慢或未命中缓存时可能远超 1 秒。旧实现的 1 秒过期会在这段时间里
 *  静默失效, 随后的位置恢复便读到该章历史位置并把页码拽走 —— 表现就是"从上一
 *  章末页翻进来, 却落在上次读到的第 3 页"(issue #14 第 9 条的变体)。
 *
 *  标记因此改为**计数消费**: 只在"下一次进入索引 index 的那一次"生效, 由
 *  consumeSkipPositionRestore 取走并立即作废, 与耗时无关。残留也不会误吞后续
 *  打开书籍的位置恢复 —— 换书时 bookUrl watcher 会清掉它。 */
function markSkipPositionRestore(index: number) {
  skipPositionRestoreIndex = index
}

/** 消费一次跳过标记: 命中才作废, 保证一次进入最多被豁免一次。 */
function shouldSkipPositionRestore(index: number) {
  if (skipPositionRestoreIndex == null) return false
  if (skipPositionRestoreIndex !== index) return false
  skipPositionRestoreIndex = null
  return true
}
let suppressPositionSaveUntil = 0
let suppressContinuousScrollSyncUntil = 0
let suppressContinuousAutoLoadUntil = 0
const restoreStabilizeTimers: number[] = []
const serverProgressAutoSaveScheduler = createReaderProgressAutoSaveScheduler({
  intervalMs: SERVER_PROGRESS_AUTOSAVE_MS,
  flush: () => store.flushProgressToServer(),
})
const readerProgressExitSaver = createReaderProgressExitSaver({
  disposeAutoSave: () => serverProgressAutoSaveScheduler.dispose(),
  savePosition: () => saveReadingPosition({ force: true }),
  flushToServerKeepalive: () => store.flushProgressToServerKeepalive(true),
  flushToLegado: async () => {
    await store.syncCurrentBookProgressFromLegadoOnExit()
  },
})
const isPdfBookMode = computed(() => isPdfBook(store.book))
/**
 * 本地 PDF 走「逐页 canvas」渲染, 既不参与按字号重排的左右翻页 (那会把一张
 * 整页图切成几块), 也不进连续阅读: 那里的正文分散在多章块里, 而页渲染与
 * 释放都是按单章做的 —— 一章一屏才能保证换章时把上一章的 canvas 全部回收。
 */
const isContinuousMode = computed(
  () =>
    !isPdfBookMode.value &&
    (config.value.readMethod === '上下滚动' || config.value.readMethod === '上下滚动2'),
)
const hideReadChaptersMode = computed(
  () => !isPdfBookMode.value && config.value.readMethod === '上下滚动2',
)
const isHorizontalPageMode = computed(
  () => config.value.readMethod === '左右翻页' && !isPdfBookMode.value,
)
const isIosWebkit = computed(() => {
  const ua = typeof navigator !== 'undefined' ? navigator.userAgent : ''
  return /iPhone|iPad|iPod/i.test(ua) || (/Macintosh/i.test(ua) && typeof navigator !== 'undefined' && navigator.maxTouchPoints > 1)
})
const disableSystemCallout = computed(() => {
  return isIosWebkit.value && isMobile.value && config.value.selectAction === 'popup'
})
/** 连续阅读模式的「原地刷新」不显示全屏加载态: 章节列表必须留在 DOM 里。
 *  一旦卸载, `.content-loading` 的 height:100% 会让滚动容器内容塌缩、
 *  scrollTop 被夹到 0, 重新挂载后 handleScroll 会把第一项(上一章)判为当前章。 */
const showChapterLoading = computed(() => (
  store.loading && !(isContinuousMode.value && store.refreshing)
))
const touchState = ref({
  startX: 0,
  startY: 0,
  startAt: 0,
  moving: false,
  horizontalLocked: false,
})
const showBookInfo = ref(false)
const bookInfoBook = ref<Book | null>(null)
const showTTSPanel = ref(false)
const ttsPanelDismissed = ref(false)
/**
 * 听书面板形态：展开设置 / 单行控制条 / 圆形气泡。
 *
 * 默认单行: 展开态含音色、语速、语调、定时停止等设置, 高度约为视口三分之一,
 * 而它是 fixed 浮层、不参与滚动, 听书时想看对应正文就会被它挡住。
 * 单行只占一条, 播放/切段这些高频操作仍然随手可用; 圆形气泡是给"只想听、不看屏"用的。
 */
const TTS_PANEL_STATE_KEY = 'reader-ttsPanelState'
const ttsPanelState = ref<TtsPanelState>(readTtsPanelState())

function readTtsPanelState(): TtsPanelState {
  try {
    const saved = localStorage.getItem(TTS_PANEL_STATE_KEY)
    // 缺省单行: 首次打开听书不该先挡掉正文。
    return saved === 'expanded' || saved === 'bubble' ? saved : 'bar'
  } catch {
    return 'bar'
  }
}

function setTtsPanelState(value: TtsPanelState) {
  ttsPanelState.value = value
  try {
    localStorage.setItem(TTS_PANEL_STATE_KEY, value)
  } catch { /* 存储不可用时仅本次生效 */ }
}

/**
 * 听书面板实际渲染的形态：迷你窗口里一律用圆形气泡（现有最小形态）。
 * 只是显示层覆盖，不动用户保存的偏好（ttsPanelState）—— 退出迷你模式后
 * 用户原来选的展开/单行会自动回来。
 */
const ttsPanelDisplayState = computed<TtsPanelState>(() =>
  appStore.miniMode ? 'bubble' : ttsPanelState.value,
)

/**
 * 听书面板浮层给正文让出的底部空间。
 *
 * 面板是 fixed 浮层、不参与滚动: 分页模式下页高恒等于容器高, 被它盖住的行永远
 * 翻不出来; 滚动模式下滚到底也仍有最后几行藏在它后面。两种模式统一按面板高度
 * 收缩容器。
 *
 * 高度取实测值而不是写死: 收起/展开态差着好几百像素, 写死会让收起态白白浪费
 * 一大片正文空间。
 */
const ttsPanelOpen = ref(false)
const ttsPanelInset = ref(0)
const TTS_PANEL_GAP = 12
/** 面板刚打开时可能还没渲染完: 用 rAF 重试, 但要设上限, 否则面板永久不出现时会空转。 */
let ttsPanelMeasureAttempts = 0

function measureTtsPanelInset() {
  // 圆形气泡几乎不挡正文, 让空间反而白亏一块; 只有条形/展开态才收缩。
  // 这里必须看实际渲染的形态: 迷你模式把面板压成了气泡, 若仍按用户偏好
  // (可能是 expanded) 判断, 正文会白白少一条。
  if (!showTTSPanel.value || ttsPanelDisplayState.value === 'bubble') {
    ttsPanelMeasureAttempts = 0
    ttsPanelOpen.value = false
    ttsPanelInset.value = 0
    return
  }
  const panel = document.querySelector('.tts-controls') as HTMLElement | null
  if (!panel) {
    // 还没渲染出来: 下一帧再量。这一帧先按"已打开"处理, 但不改高度,
    // 免得用 0 当高度放行一次、把底部内容露出去。
    ttsPanelOpen.value = true
    if (ttsPanelMeasureAttempts >= 8) return
    ttsPanelMeasureAttempts += 1
    requestAnimationFrame(measureTtsPanelInset)
    return
  }
  ttsPanelMeasureAttempts = 0
  // 距视口底部的距离 = 面板底部留白; 让出 面板高 + 该留白 + 一点间距。
  const panelRect = panel.getBoundingClientRect()
  const inset = Math.round(window.innerHeight - panelRect.top + TTS_PANEL_GAP)
  ttsPanelOpen.value = true
  ttsPanelInset.value = Math.max(0, inset)
}
const offlineCachedCount = ref(0)
const speechTimerNow = ref(Date.now())
const speechTimerText = computed(() => {
  if (!store.speechStopAt) return ''
  const remainMs = store.speechStopAt - speechTimerNow.value
  if (remainMs <= 0) return ''
  const totalMinutes = Math.ceil(remainMs / 60000)
  if (totalMinutes >= 60) {
    const hours = Math.floor(totalMinutes / 60)
    const minutes = totalMinutes % 60
    return minutes ? `${hours}小时${minutes}分钟后停止` : `${hours}小时后停止`
  }
  return `${totalMinutes}分钟后停止`
})
const {
  showSearch,
  searchQuery,
  searchResults,
  searchIndex,
  searchCount,
  bookSearchStatus,
  openSearch,
  closeSearch,
  runSearch,
  nextSearchResult,
  prevSearchResult,
  jumpToSearchResult,
  handleContentUpdated,
  handlePresentationUpdated,
} = useReaderSearch(store, locateSearchMatch)
const {
  selectionMenu,
  suppressSelectionCloseUntil,
  hideSelectionMenu,
  scheduleSelectionMenuUpdate,
  handleMouseUpSelection,
  handleTouchEndSelection,
  handleSelectionChange,
  addSelectionBookmark,
  addSelectionReplaceRule,
  clearSelectionState,
  disposeSelection,
} = useReaderSelection(
  store,
  appStore,
  computed(() => ({ selectAction: config.value.selectAction })),
  scrollContainerRef,
)

const offlineBannerText = computed(() => {
  if (appStore.isOnline) return ''
  if (offlineCachedCount.value > 0) {
    return `离线模式：当前书已缓存 ${offlineCachedCount.value} 章，可继续阅读已缓存章节`
  }
  return '离线模式：当前书没有本地缓存，未缓存章节将无法打开'
})

async function refreshOfflineCacheState() {
  if (!store.book) {
    offlineCachedCount.value = 0
    return
  }
  const books = await getBookshelfWithCacheInfo().catch(() => [])
  offlineCachedCount.value = books.find((book) => book.bookUrl === store.book?.bookUrl)?.cachedChapterCount || 0
}

let refreshOfflineCacheStateTimer: number | null = null

function scheduleRefreshOfflineCacheState() {
  if (refreshOfflineCacheStateTimer) clearTimeout(refreshOfflineCacheStateTimer)
  refreshOfflineCacheStateTimer = window.setTimeout(() => {
    void refreshOfflineCacheState()
  }, 120)
}

/** resize 后的重新分页要防抖: 拖拽窗口会连续触发 resize, 每次重建都是重量级
 *  (逐段二分测量)。不防抖会有多个重建并发, 先完成的那个可能读到后一次写入的
 *  pages, 却拿自己那份已过期的 progress 去映射索引, 结果是位置不准 + 抖动。 */
let horizontalRebuildTimer: number | null = null
const loadedReaderImageSizes = new Map<string, string>()

function checkMedia() {
  isMobile.value = window.innerWidth <= 768
  // 挂载流程自己会重建(见 onMounted 的 rebuildHorizontalPages), 这里若也排一次
  // 重建就会双重重建 —— 而且后一次的 progress 是拿前一次刚写好的索引算的, 等于
  // 白白多做一轮重量级测量, 还可能把位置恢复的结果覆盖掉。
  if (!mounted) return
  scheduleHorizontalRebuild()
}

function scheduleHorizontalRebuild() {
  if (horizontalRebuildTimer) clearTimeout(horizontalRebuildTimer)
  horizontalRebuildTimer = window.setTimeout(() => {
    horizontalRebuildTimer = null
    if (!isHorizontalPageMode.value) return
    // 这里**不**提前调 updateHorizontalMetrics(): 它会立刻改写 --reader-page-step,
    // 而 horizontalPages 还是按旧尺寸分的页, 重建完成前的那一两帧会错位闪烁。
    // rebuildHorizontalPages 内部本来就会重算 metrics。
    //
    // 窗口尺寸变化会改变行宽与每页容量, 页边界必须重算 —— 沿用旧分页会出现半页
    // 错位。与排版参数变化一致: 按**阅读进度比例**映射位置, 而不是跳回章首。
    const previousMax = Math.max(0, horizontalPages.value.length - 1)
    const progress = previousMax > 0 ? horizontalPageIndex.value / previousMax : 0
    void rebuildHorizontalPages().then(() => {
      if (!isHorizontalPageMode.value) return
      // 与排版参数 watcher 同源: 未兑现的跨章回退落位优先(它才是用户此刻的意图),
      // 但只有真兑现了才让路, 否则照常按比例映射 —— 悬空的意图不该让 resize 失去定位。
      if (applyPendingPrevChapterLanding()) return
      if (pendingLegadoPageRestore.value) return
      horizontalPageIndex.value = readerPageIndexFromProgress(
        progress,
        horizontalPages.value.length,
      )
      syncHorizontalPageState()
    })
  }, 150)
}

function queueLoadedImagePagination(image: unknown) {
  if (!mounted || !isHorizontalPageMode.value || !(image instanceof HTMLImageElement) || !image.naturalWidth) return
  const src = image.currentSrc || image.src
  const size = `${image.naturalWidth}x${image.naturalHeight}`
  // 把解码后的真实尺寸登记给分页器: 重建分页时 DOM 是全新的, 新 img 还没解码,
  // naturalWidth 读不到, 没有这份缓存就只能按"没有尺寸"约束。
  rememberDecodedImageSize(src, image.naturalWidth, image.naturalHeight)
  if (loadedReaderImageSizes.get(src) === size) return
  // New page DOM fires load again even when its image is cached. Rebuild only
  // once per intrinsic size in this chapter, after the browser knows its width.
  loadedReaderImageSizes.set(src, size)
  scheduleHorizontalRebuild()
}

function handleReaderImageLoad(event: Event) {
  queueLoadedImagePagination(event.target)
}

function handleViewportChange() {
  syncViewportSize()
  checkMedia()
  scheduleRestoreReadingPosition()
}

const currentFontFamily = computed(() => {
  const preset = fontPresets.find(p => p.value === config.value.fontFamily)
  if (preset) return preset.family
  if (config.value.fontFamily.startsWith('custom:')) {
    return `"${store.customFontFamily(config.value.fontFamily.slice('custom:'.length))}"`
  }
  return ''
})

/**
 * 当前章节是否来自本地 EPUB 的自带排版。
 *
 * 判据是后端 `local_epub_book.rs` 的 `CHAPTER_SCOPE_CLASS` 包裹类(两处必须一致):
 * 书源正文里也可能出现 `<style>`, 那些仍要走既有的书源清洗流程。
 */
const EPUB_SCOPE_CLASS = 'epub-chapter-content'

function isEpubChapterHtml(html: string) {
  return isLocalEpubBook(store.book) && html.includes(EPUB_SCOPE_CLASS)
}

/** 承载本地 EPUB 原书样式的 `<style>` 元素(整本书共用一份)。 */
const EPUB_STYLE_ELEMENT_ID = 'reader-epub-book-style'

/**
 * 把章节里的 `<style>` 取出并合并注入文档 head, 同时从正文中移除。
 *
 * 原书样式在导入时已按 `.epub-chapter-content` 作用域化, 因此放到 head 也只
 * 影响本章容器, 不会波及阅读器界面; 而留在正文里会被横向分页的 `inline` 分支
 * 包进 `<p>`, 有被 `Range.cloneContents` 切成半截 CSS 的风险。
 * 每章的内容相同, 按内容比对去重, 避免重复注入。
 */
function moveEpubStylesToHead(root: HTMLElement) {
  const styles = Array.from(root.querySelectorAll('style'))
  if (!styles.length) return
  const css = styles
    .map((style) => rewriteCssAssetUrls(style.textContent || ''))
    .filter(Boolean)
    .join('\n')
  styles.forEach((style) => style.remove())
  if (!css) return

  let element = document.getElementById(EPUB_STYLE_ELEMENT_ID) as HTMLStyleElement | null
  if (!element) {
    element = document.createElement('style')
    element.id = EPUB_STYLE_ELEMENT_ID
    document.head.appendChild(element)
  }
  if (element.textContent !== css) element.textContent = css
}

function formatChapterHtml(rawText: string) {
  if (!rawText) return ''
  const text = rawText
  // 本地 EPUB 自带排版: 章节 HTML 里内联了原书(已作用域化的)CSS, 图片也按原书
  // 比例排布。这里不能再走下面的书源清洗流程 —— 它会删掉 <style>、剥掉内联
  // 字号字体, 还会按阅读器设置重写每个 <p> 的 margin, 恰好把这些样式全部抵消。
  if (isEpubChapterHtml(text)) {
    const wrapper = document.createElement('div')
    wrapper.innerHTML = text
    // 原书样式从正文流里摘出来注入文档 head: 样式已按 `.epub-chapter-content`
    // 作用域化, 放在 head 同样只作用于本章容器; 留在正文里则会被分页逻辑当成
    // 内联内容包进 <p>, 一旦该页被切分, `Range.cloneContents` 会切出半截 CSS。
    moveEpubStylesToHead(wrapper)
    rewriteLocalEpubAssetSrcs(wrapper)
    applyEpubFirstLineIndent(wrapper)
    highlightSearchText(wrapper)
    return wrapper.innerHTML
  }
  const stripLeadingIndent = (line: string) => line.replace(/^[\u3000\u00A0 \t]+/, '')
  const wrapper = document.createElement('div')

  if (/<[a-z][\s\S]*>/i.test(text)) {
    wrapper.innerHTML = repairSourceImageHtml(text)
    normalizeSourceTypography(wrapper)
    const paragraphs = Array.from(wrapper.querySelectorAll('p')) as HTMLParagraphElement[]
    paragraphs.forEach((paragraph) => {
      const plainText = (paragraph.textContent || '').replace(/^[\u3000\u00A0 \t]+/, '').trim()
      const hasRenderableChildren = Boolean(paragraph.querySelector('img, br, ruby, table, ul, ol'))
      if (!plainText && !hasRenderableChildren) {
        paragraph.remove()
        return
      }
      paragraph.innerHTML = paragraph.innerHTML.replace(/^[\u3000\u00A0 \t]+/, '')
      paragraph.style.marginTop = '0'
      paragraph.style.marginBottom = `${config.value.paragraphSpacing}em`
      paragraph.classList.toggle('reader-indent', config.value.firstLineIndent)
    })
    // 章节里「有 <p>」不代表整章都成段: 番茄系书源只在插图下方给署名加一个
    // `<p>`, 正文主体仍是裸文本行。旧实现以 paragraphs.length 作二选一条件,
    // 于是只要出现任何一个 <p>, 剩余几十上百行裸文本就完全跳过分段处理,
    // 整章塌成一大段(issue #17 的截图正是这种章节)。
    // 现在按原顺序切段: 已有 <p> 原样保留, 其余片段各自重组, 见
    // utils/readerParagraph.splitParagraphsAroundExisting(已单测覆盖)。
    wrapper.innerHTML = splitParagraphsAroundExisting(wrapper.innerHTML, {
      spacingEm: config.value.paragraphSpacing,
      firstLineIndent: config.value.firstLineIndent,
    })
  } else {
    wrapper.innerHTML = text
      .split(/\n/)
      .filter((line: string) => line.trim())
      .map((line: string) => {
        const shouldIndent = config.value.firstLineIndent
        const content = escapeHtmlText(stripLeadingIndent(line.trimEnd()))
        return `<p${shouldIndent ? ' class="reader-indent"' : ''} style="margin-top: 0; margin-bottom: ${config.value.paragraphSpacing}em;">${content}</p>`
      })
      .join('')
  }

  rewriteLocalEpubAssetSrcs(wrapper)
  processSourceImageOptions(wrapper)
  reserveImageSpace(wrapper)
  highlightSearchText(wrapper)
  return wrapper.innerHTML
}

// `,{json}` 解析与气泡处理见 utils/sourceImageOptions(已单测覆盖)
/**
 * 点击正文里的书源图片气泡(段评图标等): 执行 `,{json}` 里的 `click` 脚本。
 * 脚本在书源 jsLib 作用域运行(后端 `eval_source_click`), 因此插件函数可用;
 * 插件通常返回面板 HTML 或调用 `java.showBrowser` 弹面板。
 */
async function handleSourceImageClick(event: MouseEvent) {
  const target = event.target as HTMLElement | null
  const image = target?.closest?.('img[data-source-click]') as HTMLImageElement | null
  if (!image) return
  const script = image.dataset.sourceClick || ''
  const sourceUrl = store.book?.origin || ''
  if (!script || !sourceUrl) return

  // 阻止这次点击被阅读器的翻页/菜单逻辑继续处理
  event.preventDefault()
  event.stopPropagation()
  suppressNextTapUntil = Date.now() + 400

  const rawSrc = image.dataset.sourceRawSrc || image.getAttribute('src') || ''
  try {
    const outcome = await evalSourceClick({
      bookSourceUrl: sourceUrl,
      script,
      result: rawSrc,
      bookUrl: store.book?.bookUrl || '',
      chapterUrl: store.currentChapter?.url || '',
    })
    outcome.logs?.forEach((line) => sourceClickLog('脚本日志', line))
    if (outcome.showBrowser) {
      openSourceBrowserPanel(outcome.showBrowser)
    }
  } catch (error) {
    console.warn('[source-click] 执行失败', error)
    appStore.showToast(error instanceof Error ? error.message : '执行点击脚本失败', 'error')
  }
}

/**
 * 把章节里 `/reader3/localEpubAsset?...` 形式的资源地址改写成自定义协议地址
 * (`${readerOrigin}/epub?...`)。
 *
 * 三处都要改写: `<img src>`、封面页用的 SVG `<image xlink:href>`、以及内联
 * `<style>` 里 `url()` 引用的背景图。只处理 `<img>` 会让封面页和带背景图的
 * 页面整块 404。
 */
function rewriteLocalEpubAssetSrcs(root: HTMLElement) {
  root.querySelectorAll('img').forEach((image) => {
    const src = image.getAttribute('src') || ''
    const next = toReaderAssetUrl(src)
    if (next !== src) image.setAttribute('src', next)
  })

  // SVG `<image>`: `xlink:href` 在 HTML 解析下会被放进 xlink 命名空间,
  // 直接读 `getAttribute('xlink:href')` 未必命中, 两种读法都试一遍。
  root.querySelectorAll('image').forEach((element) => {
    const xlinkNs = 'http://www.w3.org/1999/xlink'
    const xlinkHref = element.getAttributeNS(xlinkNs, 'href')
      || element.getAttribute('xlink:href')
    if (xlinkHref) {
      const next = toReaderAssetUrl(xlinkHref)
      if (next !== xlinkHref) {
        element.setAttributeNS(xlinkNs, 'xlink:href', next)
        element.setAttribute('xlink:href', next)
      }
    }
    const href = element.getAttribute('href')
    if (href) {
      const next = toReaderAssetUrl(href)
      if (next !== href) element.setAttribute('href', next)
    }
  })

  root.querySelectorAll('style').forEach((style) => {
    const css = style.textContent || ''
    const next = rewriteCssAssetUrls(css)
    if (next !== css) style.textContent = next
  })
}

function highlightSearchText(root: HTMLElement) {
  if (!showSearch.value || !searchQuery.value) return
  const regex = new RegExp(escapeRegExp(searchQuery.value), 'gi')
  const walker = document.createTreeWalker(root, NodeFilter.SHOW_TEXT, {
    acceptNode(node) {
      const parent = node.parentElement
      if (!parent || ['MARK', 'SCRIPT', 'STYLE'].includes(parent.tagName)) {
        return NodeFilter.FILTER_REJECT
      }
      regex.lastIndex = 0
      return regex.test(node.textContent || '') ? NodeFilter.FILTER_ACCEPT : NodeFilter.FILTER_REJECT
    },
  })
  const nodes: Text[] = []
  while (walker.nextNode()) {
    nodes.push(walker.currentNode as Text)
  }
  nodes.forEach((node) => {
    const value = node.textContent || ''
    regex.lastIndex = 0
    const fragment = document.createDocumentFragment()
    let cursor = 0
    for (const match of value.matchAll(regex)) {
      const index = match.index ?? 0
      if (index > cursor) {
        fragment.appendChild(document.createTextNode(value.slice(cursor, index)))
      }
      const mark = document.createElement('mark')
      mark.className = 'search-highlight'
      mark.textContent = match[0]
      fragment.appendChild(mark)
      cursor = index + match[0].length
    }
    if (cursor < value.length) {
      fragment.appendChild(document.createTextNode(value.slice(cursor)))
    }
    node.parentNode?.replaceChild(fragment, node)
  })
}

function escapeRegExp(value: string) {
  return value.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')
}

function escapeHtmlText(value: string) {
  return value
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
}

function renderChapterHtml(rawText: string) {
  return formatChapterHtml(store.processContentForDisplay(rawText || ''))
}

/** 本地 PDF 的章节正文是逐页占位元素, 原样注入后交给 pdf.js 绘制。 */
const isPdfPageChapter = computed(
  () => isPdfBook(store.book) && (store.displayContent || '').includes(PDF_PAGE_ATTR),
)

const formattedContent = computed(() => {
  if (isPdfPageChapter.value) return store.displayContent || ''
  return formatChapterHtml(store.displayContent || '')
})

/** 上一章 PDF 页渲染的释放函数: 换章/卸载时必须调用, 否则 canvas 与文档常驻 */
let disposePdfPages: (() => void) | null = null

/** 每次发起渲染自增: 只有最后一次调用的结果才允许落到 DOM 上 */
let pdfRenderToken = 0

/**
 * 把当前章的 PDF 页占位元素绘制成 canvas。
 *
 * 必须等 DOM 把 `formattedContent` 渲染完才有占位元素可画, 所以走 nextTick。
 * 换章时先释放上一章的渲染结果, 否则 canvas 与整本 PDF 的文档会一直常驻
 * (大书整本几十兆字节)。
 */
async function renderPdfPagesIfNeeded() {
  const token = ++pdfRenderToken
  disposePdfPages?.()
  disposePdfPages = null
  if (!isPdfPageChapter.value || !store.book) return
  await nextTick()
  // 这一拍里可能已经换过章 (正文监听是异步派发的): 过期的调用不能再画,
  // 否则旧章的页会画进新章的容器。
  if (token !== pdfRenderToken) return
  const container = chapterTextRef.value
  if (!container) return
  const book = store.book
  const contentSnapshot = store.displayContent
  try {
    const dispose = await renderPdfChapterPages(
      container,
      book,
      () => token !== pdfRenderToken || store.displayContent !== contentSnapshot,
    )
    if (token !== pdfRenderToken) {
      // 渲染期间又发起了新的渲染: 这一份结果作废, 连同已画的 canvas 一起释放
      dispose()
      return
    }
    disposePdfPages = dispose
  } catch {
    // PDF 读取或解析失败: 占位框会留下空白页, 不阻塞换章
    if (token === pdfRenderToken) disposePdfPages = null
  }
}

const {
  horizontalPageIndex,
  horizontalPageStep,
  horizontalPageStepStyle,
  horizontalTextWidth,
  horizontalPages,
  horizontalLastPageStart,
  isHorizontalPagesStale,
  rebuildHorizontalPages,
  updateHorizontalMetrics,
  alignHorizontalToNearestPage,
  resetHorizontalPagePosition,
} = useHorizontalPaging(
  store,
  computed(() => ({
    fontSize: config.value.fontSize,
    fontWeight: config.value.fontWeight,
    lineHeight: config.value.lineHeight,
    pageWidth: config.value.pageWidth,
    pageVerticalPadding: config.value.pageVerticalPadding,
    paragraphSpacing: config.value.paragraphSpacing,
    firstLineIndent: config.value.firstLineIndent,
  })),
  currentFontFamily,
  formattedContent,
  isHorizontalPageMode,
  scrollContainerRef,
)

/** 只有即将参与滑动的那几页才提升为独立合成层。
 *
 *  `will-change: transform` 会让元素**常驻**一个自己的光栅层。给每一页都加上它,
 *  一章几十页时就是「视口大小 × 页数」的显存/纹理开销(1470×893×4B ≈ 5MB/页),
 *  长章节下远超预算。而一次翻页真正要动起来的只有当前页与它的邻居, 其余页都
 *  在裁剪区外, 交给浏览器按普通内容处理即可。 */
function horizontalPageWillChange(pageIndex: number) {
  const distance = Math.abs(pageIndex - horizontalPageIndex.value)
  return distance <= 1 ? 'transform' : 'auto'
}

/** 第 `pageIndex` 页在当前页索引下的位移。
 *
 *  这里**每页各自平移**, 而不是把整章所有页排成一条超宽横带、再平移那条带:
 *  后者会让 `.horizontal-pages` 变成一个「页数 × 屏宽」的巨型合成层(实测 10 页
 *  在 1174 宽窗口下是 14675×897 设备像素)。Chromium 对这种尺寸只能分块光栅,
 *  且**换章时不会可靠地让旧块失效** —— 屏幕上会残留上一章同一页码位置的文字
 *  位图, 它不属于 DOM(无法选中), 改窗口大小或重进阅读页重建层后才消失。
 *  现在布局宽度恒为一页, 每页各自是一个独立的小层, 不存在需要分块的超宽层。 */
function horizontalPageTransform(pageIndex: number) {
  // 一屏恰好一页: 让第 pageIndex 页落到视口, 它要移动 (当前页 - 自己) 个屏宽。
  const step = Math.max(1, horizontalPageStep.value)
  const offset = (pageIndex - horizontalPageIndex.value) * step
  return `translate3d(${offset}px, 0, 0)`
}
/** 一次跨过很多页时, 给新画面的淡入时长。跨章不做长距离滑动, 只留一个短促分界。 */
const HORIZONTAL_JUMP_FADE_MS = 150

/** 置位期间 transform 不吃过渡, 大跨度跳变靠它瞬时定位(见 beginHorizontalJump)。 */
const horizontalJumpInProgress = ref(false)
/** 连续跳变(如快速连翻两章)的令牌: 只有最后一次置位有权解除瞬时状态。 */
let horizontalJumpToken = 0
/** 进行中的淡入。连续跳变时先取消上一个: 两个 opacity 动画叠在同一元素上, 后者接管时
 *  会从 0 重新开始, 画面先闪一下黑再淡入。 */
let horizontalJumpFade: Animation | null = null

/** 跨页跳变时给画面一个短促淡入。
 *
 *  用 Web Animations 而不是 class + CSS 动画: 每次调用都是独立动画, 连续跨章时天然
 *  可重入, 不必管理"移除 class → 强制重排 → 加回 class"的重触发时序。
 *  淡入**不长于**「动画时长」: 该设置是用户对动效快慢的整体意愿, 把它调到 60ms 的人
 *  不该在这里又收到 150ms。设为关闭(0)时不做任何动效; 动效偏好为"减少动态效果"时
 *  同样跳过(global.css 已把 CSS 过渡压到近 0, 这里保持一致)。 */
function playPageJumpFade() {
  const duration = Number(config.value.animateDuration) || 0
  if (duration <= 0) return
  const element = chapterTextRef.value
  if (!element) return
  if (window.matchMedia('(prefers-reduced-motion: reduce)').matches) return
  horizontalJumpFade?.cancel()
  horizontalJumpFade = element.animate(
    [{ opacity: 0 }, { opacity: 1 }],
    { duration: Math.min(HORIZONTAL_JUMP_FADE_MS, duration), easing: 'ease-out' },
  )
}

/** 开始一次"一次性跨过很多页"的跳变: 瞬时定位, 跨章时再补一个短促淡入。
 *
 *  `fade` 只对**换章**为真。其它大跨度跳变(搜索跳转、位置恢复、改排版或窗口尺寸后按
 *  进度重新映射、图片加载完成后的重分页)同样要瞬时定位 —— 滑过去只是同一片飞速闪动;
 *  但它们不是"翻到了另一章", 不该有分界感。尤其图片加载完成与窗口 resize 这类用户
 *  没有主动触发、甚至未必察觉的重建, 每次都淡入会变成莫名其妙的闪烁。 */
function beginHorizontalJump(fade: boolean) {
  const token = ++horizontalJumpToken
  horizontalJumpInProgress.value = true
  // 位移必须先用 0ms 真正绘制一次, 之后才能恢复时长: 若同帧就恢复, 浏览器会拿恢复后的
  // 时长去补间这次尚未绘制的位移, 又滑成一片。两帧足以覆盖一次绘制。
  window.requestAnimationFrame(() => {
    // 先注册解除链再启动淡入: 淡入万一抛出, 置位也不会永久留在 true 上 —— 那会让
    // 之后**所有**翻页都失去滑动过渡。
    window.requestAnimationFrame(() => {
      // 期间又发生一次跳变时, 由那次(更新的 token)负责解除, 这里让路。
      if (token === horizontalJumpToken) horizontalJumpInProgress.value = false
    })
    // 淡入推迟到这一拍: 换章的分页是异步重建的, 若在索引变化的那一刻就开动画, 等浏览器
    // 真正绘出首帧时它已经跑掉一段, 短促的淡入只剩一个看不见的尾巴。放在这里, 首帧
    // 绘制时 opacity 恰好从 0 开始, 而新章的页数组也已就位(重建在微任务里完成)。
    if (fade) playPageJumpFade()
  })
}

/** 索引**一次跨过很多页**时不做长距离滑动。
 *
 *  滑动过渡的时长取自「动画时长」, 与这次跨了多少页无关: 步长 ±1 是正常翻页, 而一次
 *  跨过很多页时, 同一个时长里就会**飞速滑过一整章** —— 用户看到的正是"快速翻过了很多页"。
 *  这类跳变一律瞬时定位, 逐页翻(±1)不受影响。
 *
 *  注意**换章不能只靠这里的差值判断**: 本章只有 2 页时末页索引是 1, 清 0 的差值恰好
 *  为 1 会落进下面的 `<= 1` 盲区, 所以换章由 store.currentIndex 的 watcher 显式置位,
 *  淡入也只在那边给。 */
watch(horizontalPageIndex, (next, previous) => {
  if (!isHorizontalPageMode.value) return
  if (Math.abs(next - previous) <= 1) return
  beginHorizontalJump(false)
  // flush 必须是 sync: 置位要早于任何一次渲染读取 horizontalPageTransitionDuration,
  // 否则这次跳变已经按旧时长(«动画时长», 默认 300ms)绘制出去, 大跨度滑动照样发生。
  // pre(默认) 与渲染 effect 的执行先后取决于两边 id, 不足以依赖。
}, { flush: 'sync' })

const horizontalPageTransitionDuration = computed(() => {
  // 大跨度跳变(跨章等)瞬时到位, 不做滑动 —— 见上方 watch。
  if (horizontalJumpInProgress.value) return '0ms'
  const duration = Number(config.value.animateDuration) || 0
  if (duration <= 0) return '0ms'
  return `${duration}ms`
})

/** 把某个搜索命中的高亮定位到视野内 —— 左右分页下就是"换到它所在的那一页"。
 *
 *  为什么不能沿用 scrollIntoView: 分页画面由 `.horizontal-pages` 的 transform 加
 *  页索引驱动, 滚动容器是 overflow: hidden、没有可滚动空间, scrollIntoView 在这里
 *  是空操作 —— 这就是"左右分页下搜索结果点了不动"的根因。定位必须落到页索引上,
 *  与位置恢复(Legado 进度)走同一条路径。
 *
 *  只有**用户明确跳转**(点结果项/上一条/下一条/回车)才允许改页索引: 换章、改排版
 *  与逐字输入也会重算匹配, 那些场景改索引会把读者从正在读的位置拽走(关键词在章首
 *  出现时就是"跳回本章第一页")。见 SearchLocateOptions。
 *
 *  换章跳转时 `horizontalPages` 还是上一章的切片, 直接按它定位会把读者送到别章的
 *  某一页; 所以分页过期就先重建, 再在正确的分页上找页。`target` 为 null 是常规
 *  情况之一(刚输入关键词时 DOM 里还没有高亮, 高亮要等重建后才出现), 此时只按
 *  序号定位。 */
async function locateSearchMatch(
  target: HTMLElement | null,
  matchIndex: number,
  options: SearchLocateOptions,
) {
  if (!isHorizontalPageMode.value) {
    if (options.explicit) target?.scrollIntoView({ block: 'center', behavior: 'smooth' })
    return
  }
  // 分页渲染的是快照, 高亮变了就得重建, 否则新关键词在正文里根本没有标记。这一步
  // 与是否显式跳转无关; 但它只替换页内容, 不改页索引。
  if (isHorizontalPagesStale()) {
    await rebuildHorizontalPages()
    // 重建只换了 horizontalPages 数据, v-for 渲染要等这一拍。
    await nextTick()
  }
  if (!options.explicit) return
  const container = scrollContainerRef.value
  if (!container) return
  // 分页重建会把正文节点整个换掉, 传进来的元素可能已脱离文档; 那种情况下按序号
  // 取当前 DOM 里同一次命中。分页只做切片, 高亮的先后顺序与搜索结果一致。
  const marks = container.querySelectorAll<HTMLElement>('.search-highlight')
  const anchor = target?.isConnected ? target : marks[matchIndex]
  const page = anchor?.closest('.horizontal-page') as HTMLElement | null
  if (!page) return
  const pages = Array.from(container.querySelectorAll('.horizontal-page')) as HTMLElement[]
  const pageIndex = pages.indexOf(page)
  if (pageIndex < 0) return
  horizontalPageIndex.value = clampReaderPageIndex(pageIndex, pages.length)
  container.scrollTo({ left: 0, behavior: 'auto' })
  syncHorizontalPageState()
}
const {
  continuousChapters,
  continuousLoadingNext,
  continuousLoadingPrev,
  previousAutoLoadArmed,
  suppressContinuousSync,
  syncContinuousChapterHtml,
  getContinuousChapter,
  setContinuousActiveChapter,
  initializeContinuousChapters,
  syncContinuousToStoreState,
  loadContinuousNext,
  loadContinuousPrev,
  preloadContinuousAhead,
  armPreviousAutoLoad,
  getContinuousSections,
  scrollToContinuousChapter,
  pruneReadChapters,
  clearContinuousChapters,
  disposeContinuousReading,
} = useContinuousReading(
  store,
  renderChapterHtml,
  isContinuousMode,
  hideReadChaptersMode,
  scrollContainerRef,
)

/** 是否该预加载下一章: 到达倒数第二页就算到(与纵向滚动模式的「提前一屏」一致)。 */
function shouldPreloadNextChapter() {
  if (!config.value.enablePreload) return false
  return horizontalPageIndex.value >= horizontalLastPageStart.value - 1
}

function syncHorizontalPageState() {
  const maxPage = Math.max(0, horizontalPages.value.length - 1)
  const progress = maxPage <= 0 ? 1 : horizontalPageIndex.value / maxPage
  store.setChapterScrollProgress(progress)
  if (shouldPreloadNextChapter()) {
    store.preloadAroundChapter(store.currentIndex)
  }
  scheduleSaveReadingPosition()
  serverProgressAutoSaveScheduler.schedule()
}

function pageForward() {
  const container = scrollContainerRef.value
  if (!container) return
  if (isHorizontalPageMode.value) {
    // 一屏恰好一页, 所以步进量是 1 页。
    const lastPage = horizontalLastPageStart.value
    if (horizontalPageIndex.value >= lastPage) {
      nextChapter()
      return
    }
    horizontalPageIndex.value = Math.min(lastPage, horizontalPageIndex.value + 1)
    container.scrollTo({ left: 0, behavior: 'auto' })
    syncHorizontalPageState()
    return
  }
  const step = getPageStep(container)
  if (container.scrollTop + container.clientHeight >= container.scrollHeight - 10) {
    nextChapter()
    return
  }
  scrollPageBy(container, step)
}

function pageBackward() {
  const container = scrollContainerRef.value
  if (!container) return
  if (isHorizontalPageMode.value) {
    if (horizontalPageIndex.value <= 0) {
      // 跨到上一章时要把落点定在**上一章末页**。不标记的话 prevChapter 会走
      // scrollToTop 把索引清 0 —— 新章第 0 页是标题页, 于是往前翻看起来像没动
      // (用户反馈的正是这个: 翻上一页还是标题页), 再翻一次又进了更上一章。
      markLandOnPrevChapterLastPage()
      prevChapter()
      return
    }
    horizontalPageIndex.value = Math.max(0, horizontalPageIndex.value - 1)
    container.scrollTo({ left: 0, behavior: 'auto' })
    syncHorizontalPageState()
    return
  }
  const step = getPageStep(container)
  if (container.scrollTop <= 10) {
    prevChapter()
    return
  }
  scrollPageBy(container, -step)
}

/** 声明"这次进上一章要落在末页"。只在左右分页下的跨章回退调用。 */
function markLandOnPrevChapterLastPage() {
  const targetIndex = store.currentIndex - 1
  if (targetIndex < 0) return
  pendingPrevChapterLandingIndex = targetIndex
}

function clearPendingPrevChapterLanding() {
  pendingPrevChapterLandingIndex = null
}

/** 分页重建完成后，把"跨章回退"的落位意图兑现。
 *
 *  为什么必须异步兑现: 换章后 `horizontalPages` 还是**上一章**的分页结果, 索引
 *  此刻写任何值都会被 `store.currentIndex` watcher 的 resetHorizontalPagePosition()
 *  清成 0, 而真正的分页要等正文到达后由 rebuildHorizontalPages() 重建。所以这里只在
 *  每次重建完成后判定一次, 命中就把索引放到末页。
 *
 *  返回值 = **本次是否真的兑现了落位**。调用方据此决定要不要让路: 只有返回 true
 *  才说明位置已经由意图决定, 否则必须继续走各自的默认逻辑 —— 否则一次加载失败
 *  留下的悬空意图会让排版/resize 的比例映射永久失效(见调用点)。
 *
 *  位置恢复的干扰已在标记处排除: 跨章回退调用的是 prevChapter() 而非 jumpFromCatalog,
 *  没有 markSkipPositionRestore, 但 loadSavedReadingPosition 只在章节切到**已存进度
 *  所在章**时才产出 pending, 回退到上一章时它的 chapterIndex 与 currentIndex 不符,
 *  恢复逻辑自己会放弃。 */
function applyPendingPrevChapterLanding(): boolean {
  if (pendingPrevChapterLandingIndex === null) return false
  // 意图只在"等目标章就绪"的窗口内有效: 目标章本身, 或还没切过去的上一章(加载中)。
  // 其它索引说明用户已经去了别处(又跳章了/回退失败), 立刻作废 —— 留着它会在之后
  // 某次无关的重建(调字号、resize)里被兑现, 把用户送到别章的末页。
  if (store.currentIndex !== pendingPrevChapterLandingIndex
    && store.currentIndex !== pendingPrevChapterLandingIndex + 1) {
    clearPendingPrevChapterLanding()
    return false
  }
  const landing = readerLandingPageIndexAfterPrevChapter({
    intentIndex: pendingPrevChapterLandingIndex,
    currentIndex: store.currentIndex,
    loading: store.loading,
    totalPages: horizontalPages.value.length,
  })
  // 还不能兑现(正文没到/页数还是旧章的): 保留意图, 让调用方继续走默认逻辑。
  if (landing === null) return false
  clearPendingPrevChapterLanding()
  // 落位也是换章, 而且页数少的章节会落进按差值判断的盲区(上一章只有 2 页时索引 0→1,
  // 差值恰好为 1)。这里显式置位, 保证这次跳变同样瞬时到位。
  // 不重复淡入: 本次换章已经在 currentIndex 的 watcher 里起过一次淡入, 再起一次会让
  // 画面淡两次。
  beginHorizontalJump(false)
  horizontalPageIndex.value = landing
  syncHorizontalPageState()
  return true
}

// Navigation
function goHome() {
  captureProgressForSync()
  try {
    sessionStorage.setItem('reader-skip-offline-restore', '1')
  } catch {
    // Ignore unavailable session storage; navigation should still work.
  }
  router.replace('/')
}

/**
 * 本地 EPUB 的首行缩进。
 *
 * 问题: 阅读器有一条兜底 `.chapter-text p { text-indent: 0 }`, 特异性(含 scoped
 * 属性选择器)高于原书样式 `.epub-chapter-content p { text-indent: 2em }`, 于是原书
 * 写的段首缩进被静默清零 —— 这就是"EPUB 里首行缩进不生效"。
 *
 * 分两种阅读模式, 因为两者的 DOM 形态不同:
 *
 * 1. **上下滑动**: 章节按原样渲染, `.epub-chapter-content` 容器还在, 原书样式能
 *    命中。此时唯一要做的就是把阅读器兜底的特异性让开(见样式表 `:where(...)`),
 *    原书排版自然恢复 —— 包括 `p.foot`/`.sign` 这些刻意 `text-indent:0` 的例外段。
 *
 * 2. **左右翻页**: 分页器必须把容器展开成文本块才能测量切页(见
 *    useHorizontalPaging 的 buildHorizontalBlocks), 容器随之消失, 原书选择器一条
 *    都匹配不上。这时只能由阅读器补缩进 —— 复用既有的 `reader-indent` 类:
 *    分页器的 normalizeParagraphHtml 会把它转成内联 `text-indent:2em`, 而拆分
 *    跨页段落时又会自动去掉续接段的缩进, 全程不需要改分页代码。
 *
 *    代价: 分页模式下无法区分原书的例外段(脚注/题记), 它们也会被缩进。这是
 *    已知取舍 —— 分页模式下原书的其它排版本来就已经失效, 统一缩进至少让正文
 *    可读, 不会比现状更糟。
 *
 * 用户关闭开关时, 两种模式都只需给容器打上 `reader-no-indent`, 由样式表统一清零。
 */
const EPUB_NO_INDENT_CLASS = 'reader-no-indent'

/** 分页模式下用来请求"首行缩进"的类(与书源正文共用, 分页器已认识它)。 */
const READER_INDENT_CLASS = 'reader-indent'

function applyEpubFirstLineIndent(root: HTMLElement) {
  const container = root.classList.contains(EPUB_SCOPE_CLASS)
    ? root
    : root.querySelector<HTMLElement>(`.${EPUB_SCOPE_CLASS}`)
  if (!container) return
  // 关闭: 打标记, 由样式表统一强制清零(用户明确要求, 优先于原书)。
  container.classList.toggle(EPUB_NO_INDENT_CLASS, !config.value.firstLineIndent)
  // 开启: 上下滑动模式不必干预(原书 CSS 自然生效); 左右翻页模式下容器会被
  // 分页器展开, 必须靠段落上的类把缩进"带"到分页输出里。
  if (config.value.firstLineIndent && isHorizontalPageMode.value) {
    container.querySelectorAll<HTMLElement>('p').forEach((paragraph) => {
      paragraph.classList.add(READER_INDENT_CLASS)
    })
  }
}

function normalizeSourceTypography(root: HTMLElement) {
  root.querySelectorAll('style, link[rel="stylesheet"]').forEach((element) => element.remove())
  const typographyProperties = [
    'font',
    'font-size',
    'font-family',
    'line-height',
  ]

  root.querySelectorAll<HTMLElement>('*').forEach((element) => {
    if (element.tagName === 'FONT') {
      element.removeAttribute('size')
      element.removeAttribute('face')
    }
    typographyProperties.forEach((property) => element.style.removeProperty(property))
    if (!element.getAttribute('style')?.trim()) {
      element.removeAttribute('style')
    }
  })
}

function handlePageHide() {
  persistReadingProgressKeepalive()
}

function handleBeforeUnload() {
  persistReadingProgressKeepalive()
}

function handleVisibilityChange() {
  if (document.visibilityState !== 'hidden') return
  persistReadingProgressTemporaryKeepalive()
}

function persistReadingProgressKeepalive() {
  captureProgressForSync()
  readerProgressExitSaver.flushKeepalive()
}

function persistReadingProgressTemporaryKeepalive() {
  captureProgressForSync()
  readerProgressExitSaver.flushTemporaryKeepalive()
}

/** 页面隐藏或离开前，先把实际视口位置写回阅读状态。
 *
 *  左右分页模式**跳过**: 那里的进度由 `syncHorizontalPageState()` 在每次翻页时
 *  写进 store(进度 = 页索引 / 末页索引), 已经是权威值。而 `handleScroll` 要用
 *  `updateHorizontalMetrics()` + `clampReaderPageIndex()` 重新推一遍 —— 退出瞬间
 *  容器尺寸/分页可能正在变化, 推出来的页索引会比实际小一页, 于是把用户刚读到的
 *  位置改小; 下次打开就落在前一页(症状: "退出一页、重开少读一页")。 */
function captureProgressForSync() {
  if (!scrollContainerRef.value || !store.book || store.loading) return
  if (isHorizontalPageMode.value) return
  handleScroll()
}

async function jumpToRenderedContinuousChapter(targetIndex: number) {
  const chapter = getContinuousChapter(targetIndex)
  if (!chapter) return false

  // Adjacent chapter already on screen: scroll to its top instead of
  // refetching and rebuilding the whole continuous list.
  suppressContinuousScrollSyncUntil = Date.now() + 500
  suppressContinuousAutoLoadUntil = Date.now() + 500
  previousAutoLoadArmed.value = false
  // 主动跳章落在章节开头, 不恢复上次位置
  markSkipPositionRestore(targetIndex)
  setContinuousActiveChapter(targetIndex, chapter.content, 0)
  // Marking the chapter read queues pruneReadChapters (hide-read mode) via
  // the currentIndex watcher. Wait for that prune and its scroll compensation
  // to settle before measuring offsetTop — scrolling against the pre-prune
  // layout lands near the chapter tail.
  await nextTick()
  if (typeof window !== 'undefined') {
    await new Promise<void>((resolve) => window.requestAnimationFrame(() => resolve()))
  }
  scrollToContinuousChapter(targetIndex, false)
  return true
}

async function prevChapter() {
  const targetIndex = store.currentIndex - 1
  if (targetIndex < 0) {
    clearPendingPrevChapterLanding()
    return
  }

  if (!isContinuousMode.value) {
    // 跨章回退(在章首再往前翻)已经声明"要落在上一章末页"。这一条必须在 await
    // 之前取: 加载完成后索引清零的时机取决于 watcher 与微任务的先后, 事后再判断
    // 已经不可靠。
    const landingPending = isHorizontalPageMode.value
      && pendingPrevChapterLandingIndex === targetIndex
    // 跨章翻页是「进入目标章某一端」的明确意图: 向前落到新章首页, 向后由
    // applyPendingPrevChapterLanding 落到上一章末页。该章上次留下的阅读位置
    // 不属于这次进入, 必须挡在恢复之外 —— 否则位置恢复会把刚落好的页码拽走
    // (向后翻就会从末页跳到倒数第二页)。与目录跳转同一个标记、同一时机:
    // 在 await 之前置位, 因为索引变化触发的 watcher 可能早于本函数返回。
    markSkipPositionRestore(targetIndex)
    await store.prevChapter()
    if (landingPending) {
      // 落位改由 applyPendingPrevChapterLanding 在分页重建后兑现。这里**不能**走
      // scrollToTop —— 它把索引清 0, 而新章第 0 页是标题页, 于是"往前翻"停在标题页
      // (issue #7 的问题 2), 而且刚兑现的落位会被它当场覆盖。
      // 章节没切成功(书源报错)时意图作废, 免得之后落到无关章节的末页。
      if (store.currentIndex !== targetIndex) clearPendingPrevChapterLanding()
      return
    }
    scrollToTop('instant')
    return
  }

  if (await jumpToRenderedContinuousChapter(targetIndex)) return
  await rebuildContinuousAtChapter(targetIndex)
}

async function nextChapter() {
  let targetIndex = store.currentIndex + 1
  if (targetIndex >= store.chapters.length) {
    // 已到目录末尾: 静默检查书源新章节(60 秒节流), 有更新则接着往下读
    const added = await store.refreshTocFromSource(TOC_END_CHECK_INTERVAL_MS)
    if (added <= 0) {
      appStore.showToast('已经是最新章节了', 'success')
      return
    }
    targetIndex = store.currentIndex + 1
    if (targetIndex >= store.chapters.length) return
  }

  if (!isContinuousMode.value) {
    // 与 prevChapter 同理: 跨章前进是「去新章开头」的明确意图, 目标章的历史
    // 位置不能参与这次恢复。置位必须在 await 之前(索引 watcher 可能先跑)。
    markSkipPositionRestore(targetIndex)
    await store.nextChapter()
    scrollToTop('instant')
    return
  }

  if (await jumpToRenderedContinuousChapter(targetIndex)) return
  await rebuildContinuousAtChapter(targetIndex)
}

async function jumpFromCatalog(targetIndex: number) {
  if (targetIndex < 0 || targetIndex >= store.chapters.length) return

  // 主动跳章是明确的"去这一章开头"意图, 与"跨章回退落末页"互斥: 残留的意图会在
  // 分页重建后被兑现, 把用户送到目标章的末页。
  clearPendingPrevChapterLanding()
  markSkipPositionRestore(targetIndex)
  pendingRestorePosition.value = null
  pendingRestoreAttempts = 0
  clearRestoreStabilizers()
  if (restorePositionTimer) {
    clearTimeout(restorePositionTimer)
    restorePositionTimer = null
  }
  if (persistPositionTimer) {
    clearTimeout(persistPositionTimer)
    persistPositionTimer = null
  }
  suppressPositionSaveUntil = Date.now() + 1200

  try {
    if (!isContinuousMode.value) {
      // 先关目录再加载：否则弹层一直遮着正文，用户看不到加载阶段提示
      store.closePanel()
      await store.loadChapter(targetIndex)
      await nextTick()
      scrollToTop('instant')
      return
    }

    // 连续模式同理：先关目录才能看到阶段提示
    store.closePanel()
    await rebuildContinuousAtChapter(targetIndex)
    await nextTick()
  } finally {
    if (skipPositionRestoreIndex === targetIndex) {
      skipPositionRestoreIndex = null
    }
    pendingRestorePosition.value = null
    pendingRestoreAttempts = 0
    clearRestoreStabilizers()
  }
}

async function rebuildContinuousAtChapter(targetIndex: number) {
  suppressContinuousScrollSyncUntil = Date.now() + 500
  suppressContinuousAutoLoadUntil = Date.now() + 500
  // 重建后落在目标章节开头, 不恢复上次位置
  markSkipPositionRestore(targetIndex)
  await initializeContinuousChapters(targetIndex, false, true)
}

/** 跳到章节顶部。'instant' 用于切章(必须瞬时, 否则新章内容会滚动着出现),
 *  默认的平滑分支走「动画时长」设置。 */
function scrollToTop(behavior: ScrollBehavior = 'smooth') {
  const container = scrollContainerRef.value
  if (!container) return
  if (isHorizontalPageMode.value) {
    // 分页模式的内容位移由 horizontalPageIndex 驱动(见 horizontalPageTransform),
    // 容器并不靠 scrollLeft 定位 —— 只调 scrollTo({left:0}) 视觉上会滑回首页,
    // 但索引没变, 再翻一页就会跳回原处, 进度与预加载判断也还是旧值。
    // 这里不做「已在首页就返回」的短路: 换章后分页是异步重建的, 此刻索引可能还是
    // 上一章的旧值, 短路会漏掉这次状态同步。
    horizontalPageIndex.value = 0
    syncHorizontalPageState()
    return
  }
  if (behavior === 'smooth') {
    animatePageScrollTo(container, 0, config.value.animateDuration)
    return
  }
  container.scrollTo({ top: 0, behavior })
}

function scrollToBottom() {
  const container = scrollContainerRef.value
  if (!container) return
  if (isHorizontalPageMode.value) {
    // 尾页 = 末屏的起始栏(不是最后一栏): 多栏模式下索引必须落在屏边界上,
    // 否则会显示一个从半屏处开始的错位画面。
    horizontalPageIndex.value = horizontalLastPageStart.value
    syncHorizontalPageState()
    return
  }
  animatePageScrollTo(container, container.scrollHeight, config.value.animateDuration)
}

function getPositionStorageKey() {
  return store.book?.bookUrl ? `${READER_POSITION_PREFIX}${store.book.bookUrl}` : ''
}

/** 从打开书时的入口快照构造位置来源(书架/会话记录的进度)。
 *  不读活状态(durChapterPos/durChapterTime): 初始化滚动到章节顶部等过程
 *  会把它们清零, 用清零后的假数据会压过 localStorage 里的真实位置。 */
function buildServerSavedPosition(): SavedReadingPosition | null {
  const snapshot = store.openPosition
  if (!snapshot || snapshot.index !== store.currentIndex) return null
  const progress = snapshot.position > 1 ? snapshot.position / 10000 : snapshot.position
  return {
    chapterIndex: snapshot.index,
    progress: Math.max(0, Math.min(1, progress || 0)),
    // `durChapterTime` 是 Unix 秒, 本地快照是毫秒 —— 统一成毫秒, 免得
    // 任何基于时间戳的取舍把书架记录当成 1970 年的数据。
    updatedAt: toMillis(snapshot.time),
  }
}

function loadSavedReadingPosition() {
  // 远端位置已经完成精确恢复时，不能再用本地旧快照覆盖它；否则
  // 章节虽然正确，视口会又被恢复到章节开头。
  if (store.legadoPositionRestoreCompleted?.index === store.currentIndex) {
    pendingRestorePosition.value = null
    pendingRestoreAttempts = 0
    clearRestoreStabilizers()
    debugPositionLog('skip saved position after Legado restore', {
      currentIndex: store.currentIndex,
    })
    return
  }
  if (store.legadoPositionRestore?.index === store.currentIndex) {
    pendingRestorePosition.value = null
    pendingRestoreAttempts = 0
    clearRestoreStabilizers()
    debugPositionLog('skip saved position while restoring Legado position', {
      currentIndex: store.currentIndex,
    })
    return
  }
  if (pendingLegadoPageRestore.value?.index === store.currentIndex) {
    pendingRestorePosition.value = null
    pendingRestoreAttempts = 0
    clearRestoreStabilizers()
    return
  }
  if (shouldSkipPositionRestore(store.currentIndex)) {
    skipPositionRestoreIndex = null
    pendingRestorePosition.value = null
    pendingRestoreAttempts = 0
    clearRestoreStabilizers()
    debugPositionLog('skip saved position for chapter jump', {
      currentIndex: store.currentIndex,
    })
    return
  }
  const key = getPositionStorageKey()
  if (!key) {
    pendingRestorePosition.value = null
    pendingRestoreAttempts = 0
    debugPositionLog('skip load: no storage key')
    return
  }
  try {
    const raw = localStorage.getItem(key)
    const localSaved = raw ? JSON.parse(raw) as SavedReadingPosition : null
    const serverSaved = buildServerSavedPosition()

    let selected: SavedReadingPosition | null = null
    let source: 'local' | 'server' | 'none' = 'none'

    if (localSaved && localSaved.chapterIndex === store.currentIndex) {
      selected = localSaved
      source = 'local'
    }

    if (serverSaved && serverSaved.chapterIndex === store.currentIndex) {
      // 两个来源章号相同时**永远保留本地**: 本地是逐页写下的, 还带段落级锚点
      // (`paragraphIndex`), 恢复能精确到段; 书架那条由 10 秒自动保存与换章瞬间
      // 写入, 既粗糙又可能滞后。
      //
      // 早先这里用「进度差 ≥ 2%」决定要不要换成书架 —— 不看方向, 于是无论用户
      // 往后读还是往回读, 退出时那条更早的书架记录都会盖掉刚写下的位置:
      // 往后读会退回上一次的段落, 往回读会被拉回之前读得更远的地方。
      // 这里只在本地缺失时才会走到 server, 见下方 `if (!selected)`。
      if (!selected && shouldPreferServerReadingPosition(selected, serverSaved)) {
        selected = serverSaved
        source = 'server'
      }
    }

    if (!selected) {
      pendingRestorePosition.value = null
      pendingRestoreAttempts = 0
      clearRestoreStabilizers()
      debugPositionLog(raw ? 'ignored saved position' : 'no saved position', {
        key,
        currentIndex: store.currentIndex,
        localSaved,
        serverSaved,
      })
      return
    }

    pendingRestorePosition.value = selected
    pendingRestoreAttempts = 0
    clearRestoreStabilizers()
    debugPositionLog('loaded saved position', {
      key,
      saved: selected,
      source,
      localSaved,
      serverSaved,
      currentIndex: store.currentIndex,
      accepted: !!pendingRestorePosition.value,
    })
    if (pendingRestorePosition.value) {
      suppressPositionSaveUntil = Date.now() + 2500
    }
  } catch {
    pendingRestorePosition.value = null
    pendingRestoreAttempts = 0
    clearRestoreStabilizers()
    debugPositionLog('failed to parse saved position', { key })
  }
}

function saveReadingPosition(options: { force?: boolean } = {}) {
  const key = getPositionStorageKey()
  const container = scrollContainerRef.value
  const suppressed = !options.force && Date.now() < suppressPositionSaveUntil
  // 退出阅读页时组件已经卸载、容器 ref 已置空, 但这次写入恰恰最要紧:
  // 它是「下次打开落在原处」的唯一保证。此时用 store 里最后已知的进度兜底,
  // 只是拿不到段落锚点(段落位置需要 DOM), 章节 + 进度比例已经足够恢复。
  if (options.force && key && !container && store.book && !store.loading) {
    const fallback: SavedReadingPosition = {
      chapterIndex: store.currentIndex,
      progress: Math.max(0, Math.min(1, store.chapterScrollProgress || 0)),
      updatedAt: Date.now(),
    }
    try {
      localStorage.setItem(key, JSON.stringify(fallback))
      debugPositionLog('saved position (force, no container)', { key, position: fallback })
    } catch {
      debugPositionLog('failed to save position (force, no container)', { key })
    }
    return
  }
  if (!key || !container || store.loading || !store.book || suppressed) {
    debugPositionLog('skip save', {
      key,
      hasContainer: !!container,
      loading: store.loading,
      hasBook: !!store.book,
      suppressed,
      currentIndex: store.currentIndex,
    })
    return
  }

  const basePosition: SavedReadingPosition = {
    chapterIndex: store.currentIndex,
    progress: store.chapterScrollProgress,
    updatedAt: Date.now(),
  }

  const anchorRatio = isContinuousMode.value ? CONTINUOUS_POSITION_ANCHOR_RATIO : 0.3
  const anchorViewportY = container.getBoundingClientRect().top + container.clientHeight * anchorRatio
  if (isContinuousMode.value && continuousChapters.value.length) {
    const section = container.querySelector(`.continuous-chapter[data-chapter-index="${store.currentIndex}"]`) as HTMLElement | null
    const paragraphs = Array.from(section?.querySelectorAll('.chapter-text p') || []) as HTMLElement[]
    if (paragraphs.length) {
      let activeParagraph = paragraphs[0]
      let paragraphIndex = 0
      paragraphs.forEach((paragraph, index) => {
        if (paragraph.getBoundingClientRect().top <= anchorViewportY) {
          activeParagraph = paragraph
          paragraphIndex = index
        }
      })
      const rect = activeParagraph.getBoundingClientRect()
      const paragraphProgress = rect.height > 0 ? Math.max(0, Math.min(1, (anchorViewportY - rect.top) / rect.height)) : 0
      basePosition.paragraphIndex = paragraphIndex
      basePosition.paragraphProgress = paragraphProgress
    }
  } else if (!isHorizontalPageMode.value) {
    const paragraphs = Array.from(chapterTextRef.value?.querySelectorAll('p') || []) as HTMLElement[]
    if (paragraphs.length) {
      let activeParagraph = paragraphs[0]
      let paragraphIndex = 0
      paragraphs.forEach((paragraph, index) => {
        if (paragraph.getBoundingClientRect().top <= anchorViewportY) {
          activeParagraph = paragraph
          paragraphIndex = index
        }
      })
      const rect = activeParagraph.getBoundingClientRect()
      const paragraphProgress = rect.height > 0 ? Math.max(0, Math.min(1, (anchorViewportY - rect.top) / rect.height)) : 0
      basePosition.paragraphIndex = paragraphIndex
      basePosition.paragraphProgress = paragraphProgress
    }
  }

  localStorage.setItem(key, JSON.stringify(basePosition))
  debugPositionLog('saved position', { key, position: basePosition })
}

function scheduleSaveReadingPosition() {
  if (persistPositionTimer) clearTimeout(persistPositionTimer)
  persistPositionTimer = window.setTimeout(() => {
    saveReadingPosition()
  }, 120)
}

function restoreReadingPosition() {
  return restoreReadingPositionInternal(pendingRestorePosition.value, true)
}

async function restoreLegadoReadingPosition() {
  const saved = pendingLegadoPageRestore.value
  const container = scrollContainerRef.value
  if (!saved || saved.index !== store.currentIndex || !store.legadoPositionRestore
    || !store.legadoPositionRestore.ready || store.loading || !container) return false

  const root = isContinuousMode.value
    ? container.querySelector(`.continuous-chapter[data-chapter-index="${saved.index}"] .chapter-text`)
    : chapterTextRef.value
  if (!root) return false
  const contentLength = root.textContent?.length || 0
  if (!contentLength) return false

  // 手机端的位置按清洗后的章节文本计算，网页端 DOM 会去掉标签和部分
  // 换行，两个长度不能直接等同。先映射到当前实际显示的文本长度。
  const sourceContentLength = Math.max(
    1,
    (store.content || '').replace(/<[^>]+>/g, '').length,
  )
  const domPosition = Math.max(0, Math.min(
    contentLength,
    Math.round(Math.max(0, saved.position) * contentLength / sourceContentLength),
  ))

  const walker = document.createTreeWalker(root, NodeFilter.SHOW_TEXT)
  let remaining = domPosition
  let node: Text | null = null
  let offset = 0
  while (walker.nextNode()) {
    const candidate = walker.currentNode as Text
    const length = candidate.textContent?.length || 0
    if (length === 0) continue
    if (remaining <= length) {
      node = candidate
      offset = remaining
      break
    }
    remaining -= length
  }
  if (!node) {
    const fallbackWalker = document.createTreeWalker(root, NodeFilter.SHOW_TEXT)
    while (fallbackWalker.nextNode()) {
      const candidate = fallbackWalker.currentNode as Text
      if ((candidate.textContent?.length || 0) > 0) node = candidate
    }
    if (!node) return false
    offset = node.textContent?.length || 0
  }

  const range = document.createRange()
  const nodeLength = node.textContent?.length || 0
  const safeOffset = Math.max(0, Math.min(offset, nodeLength))
  // 折叠 Range 在部分 WebView 中没有尺寸，用一个字符范围可以稳定取得
  // 当前位置的行坐标；章节末尾则取最后一个字符。
  if (nodeLength > 0 && safeOffset >= nodeLength) {
    range.setStart(node, nodeLength - 1)
    range.setEnd(node, nodeLength)
  } else {
    range.setStart(node, safeOffset)
    range.setEnd(node, Math.min(nodeLength, safeOffset + 1))
  }
  const rect = range.getClientRects()[0] || range.getBoundingClientRect()
  const containerRect = container.getBoundingClientRect()
  const maxScroll = Math.max(0, container.scrollHeight - container.clientHeight)
  const rootRect = root.getBoundingClientRect()
  if (rootRect.height <= 0 || container.clientHeight <= 0) return false

  if (isHorizontalPageMode.value) {
    await rebuildHorizontalPages()
    const page = node.parentElement?.closest('.horizontal-page') as HTMLElement | null
    const pages = Array.from(container.querySelectorAll('.horizontal-page')) as HTMLElement[]
    const pageIndex = page ? pages.indexOf(page) : -1
    if (pageIndex < 0 || !pages.length) return false
    // 一屏恰好一页: 目标页索引直接就是它在页面列表里的下标。
    horizontalPageIndex.value = clampReaderPageIndex(pageIndex, pages.length)
    pendingLegadoPageRestore.value = null
    legadoPositionRestoreAttempts = 0
    legadoPositionRestoreSettling = false
    if (legadoPositionRestoreTimer) {
      clearTimeout(legadoPositionRestoreTimer)
      legadoPositionRestoreTimer = null
    }
    pendingRestorePosition.value = null
    pendingRestoreAttempts = 0
    clearRestoreStabilizers()
    suppressPositionSaveUntil = Date.now() + 700
    store.setChapterScrollProgress(saved.progress)
    store.clearLegadoPositionRestore(saved.token)
    debugPositionLog('restored Legado horizontal position', { saved, pageIndex })
    return true
  }

  // 手机端保存的是章节内字符偏移。网页端排版可能不同，因此字符范围
  // 用于精确定位，范围坐标不可用时按当前正文长度换算比例。
  // store 在目标章节正文加载完成时已经用同一份正文计算过一次进度。
  // 优先使用这个结果，避免 DOM 清洗差异让字符偏移重新映射到错误位置。
  const renderedProgress = Math.max(0, Math.min(1,
    saved.progress > 0 ? saved.progress : domPosition / contentLength,
  ))
  const ratioTargetTop = maxScroll * renderedProgress
  const hasRangeRect = Boolean(rect.height || rect.width)
  const rangeTargetTop = hasRangeRect
    ? container.scrollTop + rect.top - containerRect.top - container.clientHeight * 0.12
    : ratioTargetTop
  // 比例位置是稳定结果，字符范围只在与比例位置接近时做精细校正。
  // 某些 WebView 的 Range 坐标会错误地返回章节顶部，不能直接采用。
  const rangeTolerance = Math.max(container.clientHeight * 1.5, maxScroll * 0.12)
  let targetTop = Math.abs(rangeTargetTop - ratioTargetTop) <= rangeTolerance
    ? rangeTargetTop
    : ratioTargetTop
  if (isContinuousMode.value) {
    const section = root.closest('.continuous-chapter') as HTMLElement | null
    if (!section) return false
    const nextSection = section.nextElementSibling as HTMLElement | null
    const sectionHeight = Math.max(
      1,
      (nextSection ? nextSection.offsetTop : container.scrollHeight) - section.offsetTop,
    )
    const sectionRatioTarget = Math.max(
      section.offsetTop,
      section.offsetTop + sectionHeight * renderedProgress - container.clientHeight * 0.12,
    )
    const sectionRangeTolerance = Math.max(container.clientHeight * 1.5, sectionHeight * 0.12)
    if (!hasRangeRect || Math.abs(rangeTargetTop - sectionRatioTarget) > sectionRangeTolerance) {
      targetTop = sectionRatioTarget
    }
  } else if (!hasRangeRect || (renderedProgress > 0.02 && rangeTargetTop < maxScroll * 0.05)) {
    targetTop = ratioTargetTop
  }
  if (renderedProgress > 0.02 && maxScroll > 4 && targetTop < maxScroll * 0.05) {
    targetTop = ratioTargetTop
  }
  targetTop = Math.max(0, Math.min(maxScroll, targetTop))
  // 远端位置不在章节开头时，长章节必须已经产生可滚动高度。
  // 页面尚未完成布局时先等待，不能把 scrollTop=0 当成恢复成功。
  if (renderedProgress > 0.02 && maxScroll <= 4 && contentLength > container.clientHeight / 2) {
    return false
  }
  suppressContinuousScrollSyncUntil = Date.now() + 700
  suppressPositionSaveUntil = Date.now() + 700
  const previousScrollBehavior = container.style.scrollBehavior
  container.style.scrollBehavior = 'auto'
  container.scrollTop = targetTop
  if (Math.abs(container.scrollTop - targetTop) > 2) {
    container.scrollTo({ top: targetTop, behavior: 'auto' })
  }
  container.style.scrollBehavior = previousScrollBehavior
  legadoPositionRestoreTargetTop = targetTop
  legadoPositionRestoreTargetMaxScroll = maxScroll
  legadoPositionRestoreSettling = true
  store.setChapterScrollProgress(renderedProgress)
  // 正文转换、高亮和排版可能在滚动后再次改变 DOM 高度。延迟确认，
  // 避免位置刚恢复就被后续更新弹回章节开头。
  window.requestAnimationFrame(() => {
    window.requestAnimationFrame(() => {
      window.setTimeout(() => {
        legadoPositionRestoreSettling = false
        if (store.legadoPositionRestore?.token !== saved.token) return
        const currentMaxScroll = Math.max(0, container.scrollHeight - container.clientHeight)
        const layoutStillChanging = Math.abs(currentMaxScroll - legadoPositionRestoreTargetMaxScroll) > 24
        if (Math.abs(container.scrollTop - legadoPositionRestoreTargetTop) > 12 || layoutStillChanging) {
          legadoPositionRestoreSettling = false
          scheduleLegadoPositionRestore()
          return
        }
        pendingLegadoPageRestore.value = null
        legadoPositionRestoreAttempts = 0
        if (legadoPositionRestoreTimer) {
          clearTimeout(legadoPositionRestoreTimer)
          legadoPositionRestoreTimer = null
        }
        pendingRestorePosition.value = null
        pendingRestoreAttempts = 0
        clearRestoreStabilizers()
        store.clearLegadoPositionRestore(saved.token)
        saveReadingPosition({ force: true })
      }, 80)
    })
  })
  debugPositionLog('restored Legado character position', { saved, targetTop })
  return true
}

function scheduleLegadoPositionRestore() {
  if (legadoPositionRestoreSettling) return
  if (legadoPositionRestoreTimer) return
  void nextTick().then(async () => {
    if (await restoreLegadoReadingPosition()) return
    if (!pendingLegadoPageRestore.value || legadoPositionRestoreAttempts >= 100) return
    legadoPositionRestoreAttempts += 1
    legadoPositionRestoreTimer = window.setTimeout(() => {
      legadoPositionRestoreTimer = null
      scheduleLegadoPositionRestore()
    }, 80)
  })
}

function clearRestoreStabilizers() {
  while (restoreStabilizeTimers.length) {
    const timer = restoreStabilizeTimers.pop()
    if (typeof timer === 'number') clearTimeout(timer)
  }
}

function scheduleRestoreStabilization(saved: SavedReadingPosition) {
  clearRestoreStabilizers()
  if (!isIosWebkit.value || isHorizontalPageMode.value) return
  ;[140, 320, 680].forEach((delay) => {
    const timer = window.setTimeout(() => {
      if (store.loading || !scrollContainerRef.value || saved.chapterIndex !== store.currentIndex) return
      void nextTick(() => {
        restoreReadingPositionInternal(saved, false)
      })
    }, delay)
    restoreStabilizeTimers.push(timer)
  })
}

function restoreReadingPositionInternal(saved: SavedReadingPosition | null, finalize: boolean) {
  const container = scrollContainerRef.value
  if (!saved || !container || saved.chapterIndex !== store.currentIndex) {
    debugPositionLog('restore aborted', {
      hasSaved: !!saved,
      hasContainer: !!container,
      savedChapterIndex: saved?.chapterIndex,
      currentIndex: store.currentIndex,
    })
    return false
  }

  if (isHorizontalPageMode.value) {
    // 左右分页的画面位置**只由 `horizontalPageIndex` 的 transform 驱动**,
    // 滚动容器是 overflow: hidden、没有可滚动空间(见 .horizontal-page-mode
    // 与 horizontalPageTransform)。所以这里绝不能像纵向/连续模式那样按
    // `scrollLeft` 定位: 那个轴上 maxScroll 恒为 0, scrollTo 是空操作 ——
    // 恢复"成功"了却停在第一页, 正是「重进回到章节开头」的根因。
    //
    // 位置必须落回页索引。`syncHorizontalPageState` 保存的进度就是
    // `horizontalPageIndex / 末页索引`, 这里按同一个公式反向映射即可,
    // 与窗口缩放、改排版后的比例映射(readerPageIndexFromProgress)同源。
    if (store.loading || !horizontalPages.value.length) {
      debugPositionLog('restore waiting: horizontal content not ready', {
        saved,
        loading: store.loading,
        pages: horizontalPages.value.length,
      })
      return false
    }
    // 分页还是上一章的产物时, 页数是旧章的, 按它算索引会落错页。
    // 等 rebuildHorizontalPages 用新章正文重建完再恢复。
    if (isHorizontalPagesStale()) {
      debugPositionLog('restore waiting: horizontal pages are stale', {
        saved,
        pages: horizontalPages.value.length,
      })
      return false
    }
    const pageIndex = readerPageIndexFromProgress(
      Math.max(0, Math.min(1, saved.progress || 0)),
      horizontalPages.value.length,
    )
    horizontalPageIndex.value = pageIndex
    // 索引与容器的 scrollLeft 无关, 但仍归零: 旧实现留下的非零 scrollLeft
    // 会被 handleScroll 当成翻页信号。
    if (container.scrollLeft !== 0) {
      container.scrollTo({ left: 0, behavior: 'auto' })
    }
    if (finalize) {
      pendingRestorePosition.value = null
      pendingRestoreAttempts = 0
      // 恢复本身会改变索引, 这里同步一次让本章进度与页索引对齐; 期间禁止
      // 保存, 否则 handleScroll 会拿恢复中途的索引覆盖掉刚读到位置。
      suppressPositionSaveUntil = Date.now() + 400
      syncHorizontalPageState()
    }
    debugPositionLog('restored horizontal position', { saved, pageIndex })
    return true
  }

  const anchorOffset = container.clientHeight * (isContinuousMode.value ? CONTINUOUS_POSITION_ANCHOR_RATIO : 0.3)
  let targetTop = 0

  if (isContinuousMode.value) {
    if (store.loading || !continuousChapters.value.length) {
      debugPositionLog('restore waiting: continuous content not ready', {
        saved,
        loading: store.loading,
        continuousCount: continuousChapters.value.length,
      })
      return false
    }
    const section = container.querySelector(`.continuous-chapter[data-chapter-index="${saved.chapterIndex}"]`) as HTMLElement | null
    if (!section) {
      debugPositionLog('restore failed: section not found', {
        saved,
        availableSections: Array.from(container.querySelectorAll('.continuous-chapter')).map((el) => (el as HTMLElement).dataset.chapterIndex),
      })
      return false
    }
    const paragraphs = Array.from(section.querySelectorAll('.chapter-text p')) as HTMLElement[]
    if (typeof saved.paragraphIndex === 'number' && !paragraphs.length) {
      debugPositionLog('restore waiting: continuous paragraphs not ready', {
        saved,
        sectionIndex: saved.chapterIndex,
      })
      return false
    }
    if (paragraphs.length && typeof saved.paragraphIndex === 'number') {
      const paragraph = paragraphs[Math.max(0, Math.min(paragraphs.length - 1, saved.paragraphIndex))]
      const top = paragraph.getBoundingClientRect().top - container.getBoundingClientRect().top + container.scrollTop
      const paragraphProgress = Math.max(0, Math.min(1, saved.paragraphProgress || 0))
      targetTop = Math.max(section.offsetTop, top + paragraph.offsetHeight * paragraphProgress - anchorOffset)
    } else {
      const nextSection = section.nextElementSibling as HTMLElement | null
      const sectionHeight = Math.max(1, (nextSection ? nextSection.offsetTop : container.scrollHeight) - section.offsetTop)
      if ((saved.progress || 0) > 0 && sectionHeight <= Math.max(1, container.clientHeight * 0.25)) {
        debugPositionLog('restore waiting: continuous section height not ready', {
          saved,
          sectionHeight,
          clientHeight: container.clientHeight,
        })
        return false
      }
      targetTop = Math.max(
        section.offsetTop,
        section.offsetTop + sectionHeight * Math.max(0, Math.min(1, saved.progress || 0)),
      )
    }
  } else {
    const paragraphs = Array.from(chapterTextRef.value?.querySelectorAll('p') || []) as HTMLElement[]
    if (store.loading || !chapterTextRef.value) {
      debugPositionLog('restore waiting: chapter content not ready', {
        saved,
        loading: store.loading,
        hasChapterText: !!chapterTextRef.value,
      })
      return false
    }
    if (typeof saved.paragraphIndex === 'number' && !paragraphs.length) {
      debugPositionLog('restore waiting: chapter paragraphs not ready', {
        saved,
      })
      return false
    }
    if (paragraphs.length && typeof saved.paragraphIndex === 'number') {
      const paragraph = paragraphs[Math.max(0, Math.min(paragraphs.length - 1, saved.paragraphIndex))]
      const top = paragraph.getBoundingClientRect().top - container.getBoundingClientRect().top + container.scrollTop
      const paragraphProgress = Math.max(0, Math.min(1, saved.paragraphProgress || 0))
      targetTop = top + paragraph.offsetHeight * paragraphProgress - anchorOffset
    } else {
      const maxScroll = Math.max(0, container.scrollHeight - container.clientHeight)
      if ((saved.progress || 0) > 0 && maxScroll <= 4) {
        debugPositionLog('restore waiting: max scroll not ready', {
          saved,
          scrollHeight: container.scrollHeight,
          clientHeight: container.clientHeight,
          maxScroll,
        })
        return false
      }
      targetTop = maxScroll * Math.max(0, Math.min(1, saved.progress || 0))
    }
  }

  // Position restore must never animate — 'auto' would follow the
  // container's CSS scroll-behavior: smooth and visibly roll the page.
  container.scrollTo({ top: Math.max(0, targetTop), behavior: 'instant' })
  if (finalize) {
    pendingRestorePosition.value = null
    pendingRestoreAttempts = 0
    const suppressMs = isContinuousMode.value && isIosWebkit.value ? 1600 : 500
    suppressContinuousScrollSyncUntil = Date.now() + suppressMs
    suppressContinuousAutoLoadUntil = Date.now() + suppressMs
    scheduleRestoreStabilization(saved)
  }
  suppressPositionSaveUntil = Date.now() + 400
  debugPositionLog('restored vertical position', {
    saved,
    targetTop,
    isContinuous: isContinuousMode.value,
    finalize,
  })
  return true
}

function scheduleRestoreReadingPosition() {
  if (pendingLegadoPageRestore.value) {
    scheduleLegadoPositionRestore()
    return
  }
  if (store.restoringLegadoProgress) {
    debugPositionLog('defer restore: Legado progress is restoring', {
      currentIndex: store.currentIndex,
    })
    return
  }
  if (restorePositionTimer) clearTimeout(restorePositionTimer)
  if (legadoPositionRestoreTimer) clearTimeout(legadoPositionRestoreTimer)
  debugPositionLog('schedule restore', {
    attempts: pendingRestoreAttempts,
    hasPending: !!pendingRestorePosition.value,
    currentIndex: store.currentIndex,
  })
  restorePositionTimer = window.setTimeout(() => {
    void nextTick(() => {
      const restored = restoreReadingPosition()
      if (!restored && pendingRestorePosition.value && pendingRestoreAttempts < 12) {
        pendingRestoreAttempts += 1
        debugPositionLog('restore retry', {
          attempts: pendingRestoreAttempts,
          pending: pendingRestorePosition.value,
          currentIndex: store.currentIndex,
        })
        scheduleRestoreReadingPosition()
      } else if (!restored) {
        debugPositionLog('restore gave up', {
          attempts: pendingRestoreAttempts,
          pending: pendingRestorePosition.value,
          currentIndex: store.currentIndex,
        })
        pendingRestorePosition.value = null
        pendingRestoreAttempts = 0
      }
    })
  }, pendingRestoreAttempts === 0 ? 0 : 80)
}

const {
  clearReadingClass,
  startAutoScroll,
  stopAutoScroll,
  pauseAutoScrollForManualInput,
  startSpeech,
  speechPrev,
  speechNext,
  restartSpeechFromCurrentParagraph,
  cancelSpeechTransition,
  cancelPendingChapterFollow,
  markChapterChangeFromScroll,
  markManualReadingInput,
  disposeAutoPlayback,
} = useReaderAutoPlayback(
  store,
  computed(() => ({
    clickAction: config.value.clickAction,
    autoScrollSpeed: config.value.autoScrollSpeed,
  })),
  isContinuousMode,
  scrollContainerRef,
  chapterTextRef,
  nextChapter,
  prevChapter,
  isHorizontalPageMode,
  horizontalPageIndex,
)

/** 手动输入(滚轮/触摸/点击/按键)时中断翻页补间并暂停自动滚动。
 *  补间每帧直接写 scrollTop, 不中断会与用户自己的滚动互相抢位置。 */
function handleManualReadingInput() {
  cancelPageScrollAnimation(scrollContainerRef.value)
  pauseAutoScrollForManualInput()
  // 让朗读的「自滚保护窗口」知道这是用户在滚：此后反推出的索引变化按用户跳章处理。
  markManualReadingInput()
}

/** 翻页步长: 按正文行高取整, 翻页后视口顶部落在行首。
 *  原先固定用视口高度的 0.88/0.8, 每页都会把上一页末尾几行重复显示一遍。
 *  滚动模式下「下一页」即整页滚动 —— 不再有半页动作。 */
function getPageStep(container: HTMLElement) {
  return getReaderPageStep(container.clientHeight, config.value.fontSize * config.value.lineHeight)
}

/** 按「动画时长」设置滚动: 0 为瞬时, 其余走补间。
 *  容器 CSS scroll-behavior 已改为 auto, 平滑只能由此处显式提供。 */
function scrollPageBy(container: HTMLElement, delta: number) {
  animatePageScrollBy(container, delta, config.value.animateDuration)
}

// Click behavior
function handleBackgroundClick(e: Event) {
  // If clicked directly on the reader-view wrapper, toggle controls
  if ((e.target as HTMLElement).classList.contains('reader-view')) {
    collapseControlsByClick()
  }
}

function handleContextMenu(event: MouseEvent) {
  event.preventDefault()
  const text = window.getSelection?.()?.toString().trim() || ''
  const width = 190
  const height = 342
  readerContextMenu.value = {
    visible: true,
    top: Math.min(event.clientY, Math.max(8, window.innerHeight - height - 8)),
    left: Math.min(event.clientX, Math.max(8, window.innerWidth - width - 8)),
    text,
  }
}

async function prevChapterFromContextMenu() {
  closeReaderContextMenu()
  await prevChapter()
}

async function nextChapterFromContextMenu() {
  closeReaderContextMenu()
  await nextChapter()
}

function closeReaderContextMenu() {
  readerContextMenu.value.visible = false
}

function searchContextSelection() {
  const text = readerContextMenu.value.text
  closeReaderContextMenu()
  if (text) {
    searchQuery.value = text.slice(0, 80)
    openSearch()
  }
}

function replaceContextSelection() {
  const text = readerContextMenu.value.text
  closeReaderContextMenu()
  if (text) {
    replaceDialog.value = {
      visible: true,
      pattern: text,
      replacement: '',
      scope: 'book',
      isRegex: false,
      saving: false,
    }
  }
}

function closeReplaceDialog() {
  if (replaceDialog.value.saving) return
  replaceDialog.value.visible = false
}

async function saveReplaceDialog() {
  const pattern = replaceDialog.value.pattern.trim()
  if (!pattern || replaceDialog.value.saving) return
  replaceDialog.value.saving = true
  const saved = await addSelectionReplaceRule(replaceDialog.value.scope, pattern, {
    replacement: replaceDialog.value.replacement,
    isRegex: replaceDialog.value.isRegex,
  })
  replaceDialog.value.saving = false
  if (saved) {
    replaceDialog.value.visible = false
    if (isContinuousMode.value) {
      syncContinuousChapterHtml()
    } else if (isHorizontalPageMode.value) {
      await rebuildHorizontalPages()
    } else {
      await nextTick()
    }
  }
}

async function bookmarkContextSelection() {
  const text = readerContextMenu.value.text
  closeReaderContextMenu()
  if (text) {
    await addSelectionBookmark(text)
    return
  }
  try {
    await store.addBookmark(scrollContainerRef.value?.scrollTop || 0)
    appStore.showToast('已添加当前页书签', 'success')
  } catch {
    appStore.showToast('加入书签失败', 'error')
  }
}

function openCatalogFromContextMenu() {
  closeReaderContextMenu()
  store.openPanel('catalog')
}

function openSourceFromContextMenu() {
  closeReaderContextMenu()
  store.openPanel('source')
}

/** 打开书失败后的「重试」：重新走一遍 loadBook（会重取详情与目录）。
 *
 *  必须在用户点击时重来，而不是自动重试 —— 失败原因常常是书源站点临时
 *  不可达或需要登录，自动循环重试只会把站点打成限流。 */
async function retryOpenBook() {
  const current = store.book
  if (!current) {
    goHome()
    return
  }
  try {
    await store.loadBook(current, false)
  } catch (error) {
    appStore.showToast((error as Error).message || '重试失败', 'error')
  }
}

function openSourcePanelFromError() {
  store.openPanel('source')
}

function toggleAutoReadingFromContextMenu() {
  closeReaderContextMenu()
  store.toggleAutoReading()
  appStore.showToast(store.isAutoScrolling ? '已开始自动翻页' : '已停止自动翻页', 'success')
}

function stopAutoReadingFromStatus() {
  store.stopAutoReading()
  appStore.showToast('已停止自动翻页', 'success')
}

async function refreshFromContextMenu() {
  closeReaderContextMenu()
  try {
    await store.refreshContent()
    appStore.showToast('当前章节已刷新', 'success')
  } catch (error) {
    appStore.showToast((error as Error).message || '刷新失败', 'error')
  }
}

function handleGlobalClick(e: MouseEvent) {
  if (store.activePanel) return
  closeReaderContextMenu()
  if (Date.now() < suppressNextTapUntil) return
  if (Date.now() < suppressSelectionCloseUntil.value) return
  if (selectionMenu.value.visible) {
    hideSelectionMenu()
    return
  }
  if (window.getSelection?.()?.toString().trim()) return

  const target = e.target as HTMLElement | null
  if (isReaderInteractiveClickTarget(target)) return
  if (showControls.value && !store.activePanel) {
    collapseControlsByClick()
    return
  }
  if (store.isAutoScrolling) return
  
  // 左右分页模式: 全平台按横向分区(左 30% 上一页, 右 30% 下一页),
  // 竖向分区只适用于滚动模式。
  if (isHorizontalPageMode.value) {
    const x = e.clientX / window.innerWidth
    if (x < 0.3) {
      clickZoneAction('prev')
    } else if (x > 0.7) {
      clickZoneAction('next')
    } else {
      clickZoneAction('menu')
    }
  } else {
    const y = e.clientY / window.innerHeight
    if (y < 0.3) {
      clickZoneAction('prev')
    } else if (y > 0.7) {
      clickZoneAction('next')
    } else {
      clickZoneAction('menu')
    }
  }
}

function clickZoneAction(zone: 'prev' | 'menu' | 'next') {
  if (zone === 'menu') {
    // 宽屏也走"点击正文中部呼出控件": 阅读页控件已统一为 ReaderMobileControls,
    // 不再有"鼠标移到边缘滑出竖栏"这条宽屏专属路径, 这里不能再按 isMobile 拦截,
    // 否则大窗口下点了中部什么都不会发生。
    // 悬停模式下指针常常还贴着边缘, 收起要一并静默, 否则会立刻被悬停弹回来。
    if (showControls.value) collapseControlsByClick()
    else showControls.value = true
    return
  }

  if (config.value.clickAction === 'none') return

  const container = scrollContainerRef.value
  if (!container) return

  // 自动滚动时点击翻页区: 先中断补间并暂停自动滚动, 静止后自动恢复。
  handleManualReadingInput()

  if (isHorizontalPageMode.value) {
    if (zone === 'next') pageForward()
    else pageBackward()
    return
  }

  const h = container.clientHeight
  const delta = getPageStep(container) // Page scroll amount

  if (config.value.clickAction === 'next') {
    pageForward()
    return
  }
  
  if (zone === 'next') {
    if (container.scrollTop + h >= container.scrollHeight - 10) {
      if (config.value.clickAction === 'auto') nextChapter()
    } else {
      scrollPageBy(container, delta)
    }
  } else {
    if (container.scrollTop === 0) {
      if (config.value.clickAction === 'auto') prevChapter()
    } else {
      scrollPageBy(container, -delta)
    }
  }
}

function handleScroll() {
  hideSelectionMenu()
  const container = scrollContainerRef.value
  if (container && isContinuousMode.value && continuousChapters.value.length) {
    if (Date.now() < suppressContinuousScrollSyncUntil) {
      scheduleSaveReadingPosition()
      return
    }
    // 原地刷新期间索引与进度都不变, 重排造成的滚动事件不能反推当前章 ——
    // 刷新瞬间 offsetTop 尚未稳定, 反推会把上一章判成当前章并改写 currentIndex。
    if (store.refreshing) {
      scheduleSaveReadingPosition()
      return
    }
    const sections = getContinuousSections()
    if (sections.length) {
      const anchorLine = container.scrollTop + container.clientHeight * CONTINUOUS_POSITION_ANCHOR_RATIO
      let activeSection = sections[0]
      for (const section of sections) {
        if (section.offsetTop <= anchorLine) {
          activeSection = section
        } else {
          break
        }
      }

      const activeIndex = Number(activeSection.dataset.chapterIndex || 0)
      const activeChapter = getContinuousChapter(activeIndex)
      const firstIndex = Number(sections[0].dataset.chapterIndex || 0)
      if (activeIndex === firstIndex && container.scrollTop > 12) {
        armPreviousAutoLoad()
      }
      const nextSection = sections[sections.indexOf(activeSection) + 1] || null
      const sectionRange = Math.max(
        1,
        (nextSection ? nextSection.offsetTop : container.scrollHeight) - activeSection.offsetTop,
      )
      const progress = Math.max(0, Math.min(1, (container.scrollTop - activeSection.offsetTop) / sectionRange))
      if (activeChapter) {
        if (store.currentIndex !== activeIndex || store.content !== activeChapter.content) {
          // 自然滚动进入新章节: 视口本身就是正确的阅读位置, 跳过位置恢复。
          // 否则短章节下(进度接近 1)恢复会与滚动/裁剪补偿拉扯, 把视口拽到章节尾部。
          // 仅在索引变化时标记 —— 内容差异但索引相同的分支不触发 currentIndex
          // watcher, 标记无人消费会成为残留。
          if (store.currentIndex !== activeIndex) {
            markSkipPositionRestore(activeIndex)
            // 告诉朗读：这次索引变化是滚动反推出来的（可能是 TTS 自己的自动滚动，
            // 也可能是用户滚动 —— 由上手输入时刻区分，见 isSpeechSelfScroll）。
            markChapterChangeFromScroll()
          }
          setContinuousActiveChapter(activeIndex, activeChapter.content, progress)
        } else {
          store.setChapterScrollProgress(progress)
        }
      }
    }

    if (Date.now() >= suppressContinuousAutoLoadUntil) {
      if (previousAutoLoadArmed.value && container.scrollTop <= 12) {
        loadContinuousPrev()
      }
      const continuousRemaining = container.scrollHeight - (container.scrollTop + container.clientHeight)
      // 提前两屏就把下一章拉进缓存: 只按"距底不足 480px"触发时, 用户往往已经
      // 滚到列表末尾, 那时再取正文就只能显示「正在加载下一章…」。
      if (config.value.enablePreload && continuousRemaining < container.clientHeight * 2) {
        preloadContinuousAhead()
      }
      if (continuousRemaining < 480) {
        loadContinuousNext()
      }
    }
  } else if (container) {
    const maxScroll = Math.max(1, container.scrollHeight - container.clientHeight)
    const progress = isHorizontalPageMode.value
      ? (() => {
          const maxPage = Math.max(0, horizontalPages.value.length - 1)
          return maxPage <= 0 ? 1 : horizontalPageIndex.value / maxPage
        })()
      : (container.scrollHeight <= container.clientHeight ? 1 : container.scrollTop / maxScroll)
    store.setChapterScrollProgress(progress)
    if (isHorizontalPageMode.value) {
      updateHorizontalMetrics()
      horizontalPageIndex.value = clampReaderPageIndex(
        horizontalPageIndex.value,
        horizontalPages.value.length,
      )
      if (container.scrollLeft !== 0) {
        container.scrollTo({ left: 0, behavior: 'auto' })
      }
      if (shouldPreloadNextChapter()) {
        store.preloadAroundChapter(store.currentIndex)
      }
    } else if (config.value.enablePreload && container.scrollHeight - (container.scrollTop + container.clientHeight) < container.clientHeight * 1.5) {
      store.preloadAroundChapter(store.currentIndex)
    }
  }
  // 滚动时按原行为收起控件。悬停模式下只在指针停在菜单栏上(顶栏滚轮翻页)时才留 ——
  // 热区里的正文不算, 否则鼠标停在边缘看正文时菜单栏会一直常驻遮挡。
  if (showControls.value && !store.activePanel && !hoverPointerOnControls) {
    showControls.value = false
  }
  scheduleSaveReadingPosition()
  serverProgressAutoSaveScheduler.schedule()
}

function handleTouchStart(event: TouchEvent) {
  handleManualReadingInput()
  hideSelectionMenu()
  const touch = event.touches[0]
  if (!touch) return
  touchState.value = {
    startX: touch.clientX,
    startY: touch.clientY,
    startAt: Date.now(),
    moving: true,
    horizontalLocked: false,
  }
}

function handleTouchMove(event: TouchEvent) {
  if (!isMobile.value || config.value.readMethod !== '左右翻页' || !touchState.value.moving) return
  const selectedText = window.getSelection?.()?.toString().trim()
  if (selectedText) return
  // Keep long-press text selection gestures available on mobile.
  if (Date.now() - touchState.value.startAt > 220) return
  const touch = event.touches[0]
  if (!touch) return
  const deltaX = touch.clientX - touchState.value.startX
  const deltaY = touch.clientY - touchState.value.startY
  if (Math.abs(deltaX) > 12 && Math.abs(deltaX) > Math.abs(deltaY)) {
    touchState.value.horizontalLocked = true
    event.preventDefault()
  }
}

function handleTouchEnd(event: TouchEvent) {
  if (!isMobile.value || config.value.readMethod !== '左右翻页' || !touchState.value.moving) {
    touchState.value.moving = false
    return
  }
  const target = event.target as HTMLElement | null
  if (isReaderInteractiveClickTarget(target)) {
    touchState.value.moving = false
    return
  }
  const touchDuration = Date.now() - touchState.value.startAt
  const selectedText = window.getSelection?.()?.toString().trim()
  if (selectedText) {
    suppressNextTapUntil = Date.now() + 900
    touchState.value.moving = false
    scheduleSelectionMenuUpdate(260)
    return
  }
  const touch = event.changedTouches[0]
  if (!touch) {
    touchState.value.moving = false
    return
  }
  const deltaX = touch.clientX - touchState.value.startX
  const deltaY = touch.clientY - touchState.value.startY
  let didPageTurn = false
  if (Math.abs(deltaX) > 18 && Math.abs(deltaX) > Math.abs(deltaY)) {
    suppressNextTapUntil = Date.now() + 350
    if (deltaX < 0) {
      pageForward()
    } else {
      pageBackward()
    }
    didPageTurn = true
  }
  touchState.value.moving = false
  if (!didPageTurn && touchDuration > 260) {
    // Long-press should be reserved for native text selection, not page action.
    suppressNextTapUntil = Date.now() + 900
    scheduleSelectionMenuUpdate(260)
    return
  }
  if (!didPageTurn) {
    const moved = Math.hypot(deltaX, deltaY)
    if (touchDuration <= 260 && moved < 10) {
      suppressNextTapUntil = Date.now() + 350
      if (showControls.value && !store.activePanel) {
        collapseControlsByClick()
      } else {
        const x = touch.clientX / window.innerWidth
        if (x < 0.3) {
          clickZoneAction('prev')
        } else if (x > 0.7) {
          clickZoneAction('next')
        } else {
          clickZoneAction('menu')
        }
      }
    } else {
      window.setTimeout(() => {
        alignHorizontalToNearestPage(touchState.value.moving)
      }, 120)
    }
  }
  scheduleSelectionMenuUpdate(260)
}

function openCachePanel() {
  store.togglePanel('cache')
}

// Keyboard shortcuts
/** 当前阅读模式大类: 左右分页 vs 上下滚动(含连续阅读)。 */
const readingModeKind = computed<'paged' | 'scroll'>(() => (
  isHorizontalPageMode.value ? 'paged' : 'scroll'
))

/* ─── 全屏 ───
 *  必须走 Tauri 的**窗口全屏**(setFullscreen), 而不是 HTML5 的
 *  document.documentElement.requestFullscreen()。窗口是无边框的
 *  (decorations: false), 自定义标题栏由前端绘制: HTML5 全屏只把 webview
 *  铺满屏幕, 窗口本身仍是普通窗口, --app-height 与真实视口在切换期间不一致,
 *  顶部标题栏与底部就会露出错位/黑条(用户反馈的 F11 BUG)。
 *
 *  浏览器里(开发调试)没有 Tauri API, 回退到 HTML5 全屏。
 *
 *  「最大化状态下进全屏会露出任务栏高度的黑边」由 tao 修好(tauri-apps/tao#1353,
 *  tao 0.37.2 起包含; 早期用 vendor 副本, 现已改回官方版本)。
 *
 *  这里刻意**不再**用「进全屏前先 unmaximize、退出后再 maximize」的变通。那虽
 *  也能绕开黑边, 但 unmaximize/maximize 各对应一次 ShowWindow(SW_RESTORE) /
 *  SW_MAXIMIZE, 且都由 tao 异步派发到事件循环线程 —— 退出全屏时必然先看到一次
 *  窗口化动画、再来一次最大化动画, 观感很差。修好 tao 之后窗口在全屏前后始终
 *  保持最大化状态, tao 退出全屏时用 SetWindowPlacement 一次性恢复, 一步到位。 */

/** 全屏切换后布局会重排: 立刻同步一次视口变量, 并让位置恢复重新对齐。
 *  Tauri 的全屏事件也会触发 window resize, 这里只是把首次同步提前,
 *  避免中间帧按旧高度渲染。 */
function afterFullscreenToggle() {
  syncViewportSize()
  window.requestAnimationFrame(() => {
    syncViewportSize()
    scheduleRestoreReadingPosition()
  })
}

/** 本端追踪的全屏期望状态, 仅用于连续触发时不错乱。
 *
 *  不能只依赖 `setFullscreen(!(await isFullscreen()))`: 两次 IPC 往返之间用户
 *  再按一次 F11, 第二次会读到**过期**的 isFullscreen, 于是把同一个值又设一遍 ——
 *  用户连按两下期望「进→出」, 实际停在「进」。本地记一份期望值, 并在切换后再用
 *  真实状态校准, 就能让连按变成两次真切换。 */
let fullscreenToggleInFlight: Promise<void> | null = null
let desiredFullscreen: boolean | null = null

async function toggleTauriFullscreen() {
  const { getCurrentWindow } = await import('@tauri-apps/api/window')
  const appWindow = getCurrentWindow()
  // 上一次切换还没落地就再按: 基于**期望值**取反, 而不是重新去问(可能过期的)实际状态。
  const base = desiredFullscreen ?? (await appWindow.isFullscreen())
  desiredFullscreen = !base
  await appWindow.setFullscreen(desiredFullscreen)
  // 用真实状态校准: 万一设置没生效(被系统拒、窗口已销毁), 下一次仍能正确取反。
  try {
    desiredFullscreen = await appWindow.isFullscreen()
  } catch {
    desiredFullscreen = null
  }
  afterFullscreenToggle()
}

function toggleFullscreen() {
  if (isTauriRuntime()) {
    // 串行化: 快速连按 F11 时不并发发 IPC, 否则两次 setFullscreen 的到达顺序
    // 无法保证, 结果可能是用户不想要的那个。
    const run = (fullscreenToggleInFlight ?? Promise.resolve()).then(toggleTauriFullscreen)
    fullscreenToggleInFlight = run.catch(() => undefined).finally(() => {
      if (fullscreenToggleInFlight === run) fullscreenToggleInFlight = null
    })
    void fullscreenToggleInFlight.catch((error) => {
      // 原来这里是 `.catch(() => undefined)` —— 全屏失败时用户按 F11 毫无反应也
      // 没有任何提示, 只能反复按。至少给一条 toast, 并清除期望值以免后续取反错乱。
      desiredFullscreen = null
      appStore.showToast((error as Error)?.message || '切换全屏失败', 'warning')
    })
    return
  }
  if (document.fullscreenElement) {
    void document.exitFullscreen().then(afterFullscreenToggle).catch(() => undefined)
  } else {
    void document.documentElement.requestFullscreen().then(afterFullscreenToggle).catch(() => undefined)
  }
}

/** 「返回」: 按弹层 → 选区菜单 → 搜索 → 朗读面板 → 书详情的顺序逐层退出,
 *  都没有时回书架。
 *
 *  **工具栏不参与这一串层级**(判定见 utils/readerBack, 已单测覆盖)。旧实现把
 *  "收起工具栏"排在"退出"之前, 于是用户按第一次 Esc 只是把菜单栏收起来、
 *  第二次才回书架 —— 也就是 issue #19 的「第一次会先隐藏顶层菜单栏」。工具栏
 *  本来就会随滚动/点击正文/指针离开自己收起, 不需要靠「返回」来关。 */
function handleReaderBack() {
  switch (resolveReaderBackTarget({
    panelOpen: !!store.activePanel,
    selectionMenuVisible: selectionMenu.value.visible,
    searchOpen: showSearch.value,
    ttsOpen: showTTSPanel.value,
    bookInfoOpen: showBookInfo.value,
  })) {
    case 'panel':
      store.closePanel()
      return
    case 'selectionMenu':
      hideSelectionMenu()
      return
    case 'search':
      closeSearch()
      return
    case 'tts':
      closeTTSPanel()
      return
    case 'bookInfo':
      showBookInfo.value = false
      return
    default:
      goHome()
  }
}

/** 执行快捷键动作。返回 true 表示已处理(调用方据此决定是否 preventDefault)。
 *
 *  不依赖滚动容器的动作先处理: 章节加载的短暂窗口内容器为空, 「返回」这类
 *  动作必须仍然可用(旧实现里 Esc / F11 / Ctrl+F 也都位于容器检查之前)。 */
function runHotkeyAction(action: HotkeyActionId): boolean {
  switch (action) {
    case 'none':
      return false
    case 'back':
      handleReaderBack()
      return true
    case 'fullscreen':
      toggleFullscreen()
      return true
    case 'nightMode':
      store.toggleNight()
      return true
    case 'catalog':
      store.togglePanel('catalog')
      return true
    case 'settings':
      store.togglePanel('settings')
      return true
    case 'search':
      openSearch()
      return true
    case 'autoScroll':
      store.isAutoScrolling = !store.isAutoScrolling
      return true
    case 'speech':
      if (store.isSpeaking) store.stopTTS()
      else startSpeech()
      return true
    default:
      break
  }

  // 以下动作都需要滚动容器。
  const container = scrollContainerRef.value
  if (!container) return false

  switch (action) {
    case 'prevPage':
      pageBackward()
      return true
    case 'nextPage':
      pageForward()
      return true
    case 'prevChapter':
      void prevChapter()
      return true
    case 'nextChapter':
      void nextChapter()
      return true
    case 'chapterStart':
      scrollToTop()
      return true
    case 'chapterEnd':
      scrollToBottom()
      return true
    default:
      return false
  }
}

/** 键盘/鼠标共用的快捷键派发。返回 true 表示已处理(调用方据此 preventDefault)。
 *
 *  抽出这一层的直接原因: 鼠标键与滚轮也要走完全相同的「模式分流 → 弹层拦截 →
 *  返回优先」规则, 任何一条在两处各写一遍都迟早走偏。 */
function dispatchHotkey(keyId: string | null): boolean {
  if (!keyId) return false
  // 设置面板正在录入快捷键: 这次按键/滚轮属于录入, 不能执行任何功能。
  // 见 readerHotkeys 的「录入态」说明 —— 面板开关类动作在弹层打开时是放行的,
  // 不做这层拦截会把设置面板自己切走。
  if (isHotkeyCaptureActive()) return false
  const action = resolveHotkeyAction(store.hotkeyBindings, keyId, readingModeKind.value)
  if (!action) return false

  // 「返回」优先于弹层拦截: 否则 Esc 在弹层打开时会被下面的 return 吃掉。
  if (action === 'back') {
    return runHotkeyAction(action)
  }

  // 全屏/搜索是界面级动作, 面板开关是 toggle 语义 —— 弹层打开时都要放行,
  // 否则「打开目录」的键无法再关掉目录。其余按键在弹层打开时不生效, 避免误触翻页。
  if (store.activePanel) {
    if (isActionAllowedWhilePanelOpen(action)) return runHotkeyAction(action)
    return false
  }

  // 自动滚动期间的导航按键先中断翻页补间并暂停, 静止后自动恢复。
  if (['prevPage', 'nextPage', 'chapterStart', 'chapterEnd'].includes(action)) {
    handleManualReadingInput()
  }

  return runHotkeyAction(action)
}

/** 键盘: 焦点在输入类控件里时不当作快捷键(用户正在打字)。
 *  与旧实现保持一致 —— 只看 input/textarea/select/可编辑区, 不含 button:
 *  工具栏按钮获得焦点后按方向键仍应翻页。 */
function isKeyboardHotkeyBlocked(): boolean {
  const activeElement = document.activeElement as HTMLElement | null
  const tagName = activeElement?.tagName?.toLowerCase()
  return tagName === 'input' || tagName === 'textarea' || tagName === 'select'
    || Boolean(activeElement?.isContentEditable)
}

/** 鼠标/滚轮: 事件落在可交互控件上时不当作快捷键(实现见 utils/readerHotkeys)。
 *
 *  中键/侧键用的是 isMouseButtonHotkeyBlocked —— 规则与滚轮不同, 原因见那里的说明。 */

function handleKeydown(e: KeyboardEvent) {
  const keyId = hotkeyIdFromEvent(e)
  if (!keyId) return
  // 「返回」不受输入框限制。搜索面板一打开就把焦点放进输入框, 若连 Esc 也拦掉,
  // 用户在搜索框里按 Esc 会毫无反应(面板关不掉, 也退不出阅读页), 只能先用鼠标
  // 点一下正文。返回是"离开当前层"的通用语义, 与正在打字不冲突: 它不往输入框
  // 里插入字符, 也不改变光标位置。
  const action = resolveHotkeyAction(store.hotkeyBindings, keyId, readingModeKind.value)
  if (action !== 'back' && isKeyboardHotkeyBlocked()) return

  // 矩阵里没有换绑的按键不拦截: 用户在设置里把某键设为「不操作」后,
  // 该键应彻底交还浏览器, 而不是被静默吞掉。
  if (dispatchHotkey(keyId)) e.preventDefault()
}

/** 该鼠标键在当前模式下是否有**可执行**的快捷键动作(仅解析, 不执行)。
 *
 *  判定逻辑在 utils/readerHotkeys 的 mouseButtonHotkeyAction —— 抽到那里是为了
 *  能被单测直接锁住, 而不是让测试复刻一份同样的代码(改了实现测试也不失败)。 */
function mouseButtonHotkeyAction(e: MouseEvent): HotkeyActionId | null {
  return resolveMouseButtonHotkeyAction(store.hotkeyBindings, e, readingModeKind.value)
}

/** 鼠标键(中键/侧键)触发快捷键。
 *
 *  左右键不参与(见 utils/readerHotkeys 的说明): 它们是点击翻页与右键菜单的
 *  载体。中键/侧键在浏览器里还有默认行为 —— 中键自动滚动在 mousedown 起步,
 *  侧键前进/后退在松开时触发 —— 因此命中的按键要在 mousedown、mouseup、
 *  auxclick 三处一致地拦掉。 */
function handleReaderMouseDown(e: MouseEvent) {
  if (isMouseButtonHotkeyBlocked(e.target)) return
  if (!mouseButtonHotkeyAction(e)) return

  // 鼠标键已绑定到一个真实动作 → 这一次按下归应用所有。
  //
  // 为什么不能只按 dispatchHotkey 的返回值来决定是否拦截:
  //  弹层打开时翻页类动作会被**合法地拒绝**(返回 false), 但按键的默认行为
  //  (侧键导航)仍必须拦掉, 否则对着目录按侧键会把整个页面导航走。
  //
  // 为什么必须 stopPropagation(用户反馈「侧键已录入但按了没反应」的根因):
  //  事件先到 window 捕获阶段的这里, 执行 nextPage → 启动 rAF 翻页补间;
  //  若不停止传播, 它会继续冒泡到滚动容器上的 @mousedown=
  //  handleManualReadingInput, 后者调用 cancelPageScrollAnimation ——
  //  补间每帧才写 scrollTop, 被取消后位置一帧都没动, 表现就是「按了没反应」。
  //  键盘翻页不经过 @mousedown, 所以只有鼠标键会失灵。
  //  实测: 不停止传播时补间被取消(位置 0 → 0), 停止后补间跑完 62 帧(0 → 200)。
  e.preventDefault()
  e.stopPropagation()
  dispatchHotkey(hotkeyIdFromMouseEvent(e))
}

/** 中键点击链接会在新标签打开(auxclick 默认行为), 命中绑定时一并拦掉。 */
function handleReaderAuxClick(e: MouseEvent) {
  if (isMouseButtonHotkeyBlocked(e.target)) return
  if (!mouseButtonHotkeyAction(e)) return
  e.preventDefault()
}

/** 侧键的前进/后退导航在松开时触发(Chromium 行为, 见 MDN 与 three.js
 *  issue #15713), 只拦 mousedown 不够。
 *
 *  用捕获阶段: 要抢在 document 冒泡阶段的选区菜单监听之前, 且中键/侧键的松开
 *  本就不该被当成一次文本选择。 */
function handleReaderMouseUpForHotkey(e: MouseEvent) {
  if (isMouseButtonHotkeyBlocked(e.target)) return
  if (!mouseButtonHotkeyAction(e)) return
  e.preventDefault()
  e.stopPropagation()
}

// Toolbar actions
async function toggleBookmark() {
  store.togglePanel('bookmark')
}

function handleTTS() {
  ttsPanelDismissed.value = false
  showTTSPanel.value = true
}

/** 关闭听书面板 = 结束听书。否则面板消失后朗读还在继续, 而用户已经没有任何
 *  入口回到它(除非再点一次听书), 容易变成"关不掉的声音"。 */
function closeTTSPanel() {
  showTTSPanel.value = false
  ttsPanelDismissed.value = true
  cancelPendingChapterFollow()
  cancelSpeechTransition()
  store.stopTTS()
}

function toggleSpeechFromPanel() {
  ttsPanelDismissed.value = false
  showTTSPanel.value = true
  if (store.isPaused) {
    cancelSpeechTransition()
    store.pauseTTS()
    return
  }
  if (!store.isSpeaking) {
    startSpeech()
    return
  }
  cancelSpeechTransition()
  store.pauseTTS()
}

function handleStopTTS() {
  cancelPendingChapterFollow()
  cancelSpeechTransition()
  store.stopTTS()
}

watch(() => store.isAutoScrolling, (val) => {
  store.autoReading = val
  if (val) {
    // 自动阅读与翻页补间都写 scrollTop。常规翻页会先中断补间, 但工具栏的
    // 「自动阅读」按钮在滚动容器之外, 不经过手动输入拦截, 这里补一次。
    cancelPageScrollAnimation(scrollContainerRef.value)
    startAutoScroll()
  } else {
    stopAutoScroll()
  }
})

watch(showTTSPanel, (visible) => {
  if (!visible) return
})

/** 面板显隐/形态变化都会改变它占的高度, 量完必须重新分页:
 *  分页按容器高度切页, 容器没变高就还是旧页边界, 底部照样被遮。
 *  监听渲染形态而不是偏好值: 进出迷你模式时形态会被覆盖成气泡,
 *  只看 ttsPanelState 会漏掉这次变化, 高度与分页都停在旧值。 */
watch([showTTSPanel, ttsPanelDisplayState], async () => {
  await nextTick()
  measureTtsPanelInset()
  await nextTick()
  if (isHorizontalPageMode.value) {
    await rebuildHorizontalPages()
  }
})

function changeVoice(name: string) {
  store.setVoiceName(name)
  ttsPanelDismissed.value = false
  showTTSPanel.value = true
  if (store.isSpeaking && !store.isPaused) {
    restartSpeechFromCurrentParagraph()
  }
}

function changeOpenAIVoice(voiceId: string) {
  store.setOpenAISpeechVoice(voiceId)
  ttsPanelDismissed.value = false
  showTTSPanel.value = true
  if (store.isSpeaking && !store.isPaused) {
    restartSpeechFromCurrentParagraph()
  }
}

function adjustSpeechRate(delta: number) {
  const next = Math.max(0.5, Math.min(3, parseFloat((store.speechConfig.speechRate + delta).toFixed(1))))
  store.setSpeechRate(next)
  ttsPanelDismissed.value = false
  showTTSPanel.value = true
  if (store.isSpeaking && !store.isPaused) {
    restartSpeechFromCurrentParagraph()
  }
}

function adjustSpeechPitch(delta: number) {
  const next = Math.max(0.5, Math.min(2, parseFloat((store.speechConfig.speechPitch + delta).toFixed(1))))
  store.setSpeechPitch(next)
  ttsPanelDismissed.value = false
  showTTSPanel.value = true
  if (store.isSpeaking && !store.isPaused) {
    restartSpeechFromCurrentParagraph()
  }
}

function setSpeechTimer(minutes: number) {
  store.setSpeechStopTimer(minutes)
  ttsPanelDismissed.value = false
  showTTSPanel.value = true
}
async function openInfo() {
  if (!store.book) return
  showBookInfo.value = true
  bookInfoBook.value = {
    ...store.book,
    durChapterIndex: store.currentIndex,
    durChapterTitle: store.currentChapter?.title || store.book.durChapterTitle,
  }
  try {
    const latest = await getBookInfo(store.book.bookUrl, store.book.origin)
    bookInfoBook.value = {
      ...store.book,
      ...latest,
      durChapterIndex: store.currentIndex,
      durChapterTitle: store.currentChapter?.title || latest.durChapterTitle || store.book.durChapterTitle,
    }
  } catch {
    appStore.showToast('获取书籍详情失败，已显示当前缓存信息', 'warning')
  }
}

/** 离开阅读页: 同步写出本地位置, 后台再补服务端/WebDAV 两份。
 *
 *  这里**不能** await 进度写入 —— 那是 issue #19「有时候几秒才退出、有时候
 *  秒出」的根因: 写入耗时取决于后端/网盘, 守卫一挡就成了退出耗时的一部分。
 *  本地位置在 captureProgressForSync + savePosition 里同步落盘, 已经足够保证
 *  「下次打开落在原处」; 远程两份由 exitSaver 在后台跑完。 */
onBeforeRouteLeave(() => {
  captureProgressForSync()
  void readerProgressExitSaver.flushBeforeRouteLeave()
  return true
})

/** 等目录加载完再恢复位置/绑事件。
 *
 *  超时兜底是必需的: 详情页点章节现在是「先跳转、后加载」, 阅读页挂载时目录往往
 *  还在拉取。若书源请求长挂不返回, 没有超时就会一直卡在这里, 后面的键盘/滚轮监听、
 *  TTS 计时器、事件注册全都不执行 —— 页面停在半初始化状态。到点后先按现有状态
 *  继续初始化, 目录回来后 store 的响应式仍会驱动正文渲染。 */
const CHAPTER_LIST_WAIT_TIMEOUT_MS = 5000

async function waitForChapterListReady() {
  if (!store.chaptersLoading) return
  await new Promise<void>((resolve) => {
    let settled = false
    const finish = () => {
      if (settled) return
      settled = true
      stop()
      clearTimeout(timer)
      resolve()
    }
    const stop = watch(() => store.chaptersLoading, (loading) => {
      if (!loading) finish()
    }, { flush: 'sync' })
    const timer = setTimeout(finish, CHAPTER_LIST_WAIT_TIMEOUT_MS)
  })
}

let horizontalWheelLockUntil = 0
/** 滚轮快捷键的节流: 一次物理滚动会产生一串 wheel 事件, 不节流会连续翻很多页。 */
let wheelHotkeyLockUntil = 0

function handleReaderWheel(event: WheelEvent) {
  if (isPointerHotkeyBlocked(event.target)) return

  // 滚轮快捷键优先于内置滚轮行为(分页模式翻页 / Ctrl+滚轮改字号):
  //  用户在设置里把滚轮绑成某个动作, 就该执行那个动作, 否则这个绑定永远
  //  抢不过内置行为、形同虚设。Ctrl+滚轮已被字号缩放占用, 这里让路。
  //
  //  刻意跳过弹层打开的情形: 面板里滚轮必须归面板自己滚动(目录/设置都是长
  //  列表)。键盘在弹层打开时仍放行目录/设置/搜索/全屏, 滚轮没跟这条规则对齐
  //  —— 代价是把滚轮绑到「目录」后不能用滚轮关掉自己打开的面板, 但换来的是
  //  面板内滚动始终可用, 这个取舍更划算。
  //
  //  只有在动作**真的执行了**才拦: 绑到「不操作」、或需要滚动容器而容器尚未
  //  就绪时 dispatchHotkey 返回 false, 此时必须放行给下面的内置滚动 —— 否则
  //  滚轮既没触发动作、又被 preventDefault 吞掉, 页面就再也滚不动了。
  if (!store.activePanel) {
    const wheelKeyId = hotkeyIdFromWheelEvent(event)
    if (wheelKeyId && store.hotkeyBindings[wheelKeyId]?.length) {
      const now = Date.now()
      // 节流窗口内: 这次滚轮同属一串连续事件, 一并拦掉以免漏给内置行为。
      if (now < wheelHotkeyLockUntil) {
        event.preventDefault()
        return
      }
      if (dispatchHotkey(wheelKeyId)) {
        wheelHotkeyLockUntil = now + 180
        event.preventDefault()
        return
      }
    }
  }

  if (store.activePanel) return

  // 左右分页模式: 滚轮/触控板横向滑动手势直接翻页。
  // 不受「点击翻页: 禁用」影响, 避免禁用后除键盘外无法翻页。
  if (isHorizontalPageMode.value && !event.ctrlKey) {
    event.preventDefault()
    const now = Date.now()
    if (now < horizontalWheelLockUntil) return
    if (event.deltaY > 0 || event.deltaX > 0) {
      horizontalWheelLockUntil = now + 180
      pageForward()
    } else if (event.deltaY < 0 || event.deltaX < 0) {
      horizontalWheelLockUntil = now + 180
      pageBackward()
    }
    return
  }

  handleReaderFontSizeWheel(
    config.value.fontSize,
    event,
    (fontSize) => store.updateConfig('fontSize', fontSize),
  )
}

onMounted(async () => {
  syncViewportSize()
  if (!store.book) {
    const restored = await store.restorePersistedSession()
    if (!restored) {
      router.replace('/')
      return
    }
    appStore.showToast('已恢复最近阅读的离线章节', 'success')
  }
  appStore.startReadingSession(store.book?.bookUrl, store.book?.name, store.book?.author)
  void store.fetchCustomFonts().catch(() => undefined)
  await waitForChapterListReady()
  if (readerViewUnmounted) return
  // 直接进入阅读器(含刷新恢复)时正文已经就位, 不会再触发 store.content 的
  // 变化监听 —— 本地 PDF 的页面渲染要在这里补一次。
  void renderPdfPagesIfNeeded()
  loadSavedReadingPosition()
  window.addEventListener('keydown', handleKeydown)
  window.addEventListener('wheel', handleReaderWheel, { passive: false })
  // 鼠标键(中键/侧键)可绑快捷键。捕获阶段: 要抢在 WebView2 的中键自动滚动与
  // 侧键前进后退之前消费掉这次按下。
  window.addEventListener('mousedown', handleReaderMouseDown, true)
  window.addEventListener('auxclick', handleReaderAuxClick, true)
  window.addEventListener('mouseup', handleReaderMouseUpForHotkey, true)
  window.addEventListener('click', closeReaderContextMenu)
  // 书源图片气泡的 click 脚本(捕获阶段, 抢在翻页逻辑之前)
  document.addEventListener('click', handleSourceImageClick, true)
  document.addEventListener('mouseup', handleMouseUpSelection)
  document.addEventListener('touchend', handleTouchEndSelection)
    document.addEventListener('selectionchange', handleSelectionChange)
    checkMedia()
    window.addEventListener('resize', checkMedia)
    window.addEventListener('resize', measureTtsPanelInset)
    window.addEventListener(APP_VIEWPORT_CHANGE_EVENT, handleViewportChange)
    window.addEventListener('pagehide', handlePageHide)
    window.addEventListener('beforeunload', handleBeforeUnload)
    document.addEventListener('visibilitychange', handleVisibilityChange)
    // 悬停展开: 桌面指针环境启用。迷你模式不启用(见 supportsHoverPointer 的注释),
    // 因此还要监听 miniMode 变化 —— 切进小窗时退出悬停模式, 切回来再恢复。
    hoverMediaQuery = window.matchMedia('(hover: hover) and (pointer: fine)')
    hoverMediaQuery.addEventListener('change', syncHoverPointerSupport)
    syncHoverPointerSupport()
    hoverSupportStop = watch(
      [() => appStore.miniMode, () => config.value.hoverRevealControls],
      ([, hoverReveal]) => {
        syncHoverPointerSupport()
        // 用户刚关掉悬停展开时, 已弹出的工具栏要一并收起: 指针此后不再触发展开/收起
        // 判定, 留在屏幕上的那条工具栏只能靠点击正文中部才能关掉。
        if (!hoverReveal && showControls.value && !store.activePanel) showControls.value = false
      },
    )
    store.fetchVoices()
  applySystemTheme(store.isNight ? 'dark' : appStore.theme, store.currentTheme.body)
  if (typeof window !== 'undefined' && window.speechSynthesis) {
    window.speechSynthesis.onvoiceschanged = () => store.fetchVoices()
  }
  speechTimerTicker = window.setInterval(() => {
    speechTimerNow.value = Date.now()
  }, 15000)
  await Promise.all([
    store.fetchBookmarks(),
    store.fetchReplaceRules(),
  ])
  scheduleRefreshOfflineCacheState()
  updateHorizontalMetrics()
  await rebuildHorizontalPages()
  // 挂载期结束: 此后 resize 才允许触发重新分页。
  mounted = true
  scrollContainerRef.value?.querySelectorAll('img').forEach(queueLoadedImagePagination)
  // 章节列表 watcher(2114)可能已完成初始化并应用了位置恢复; 这里只在列表
  // 尚未初始化时补一次, 否则重复初始化会把已恢复的视口拽回章节开头。
  if (isContinuousMode.value && !continuousChapters.value.length) {
    await initializeContinuousChapters(store.currentIndex, false)
  }
  scheduleRestoreReadingPosition()
})

onUnmounted(() => {
    readerViewUnmounted = true
    persistReadingProgressKeepalive()
    appStore.stopReadingSession()
  disposePdfPages?.()
  disposePdfPages = null
  // 离开阅读器: PDF 文档不再需要, 释放它占的内存
  if (store.book?.bookUrl) void releasePdfBook(store.book.bookUrl)
  cancelPageScrollAnimation(scrollContainerRef.value)
  window.removeEventListener('keydown', handleKeydown)
  window.removeEventListener('wheel', handleReaderWheel)
  window.removeEventListener('mousedown', handleReaderMouseDown, true)
  window.removeEventListener('auxclick', handleReaderAuxClick, true)
  window.removeEventListener('mouseup', handleReaderMouseUpForHotkey, true)
  window.removeEventListener('click', closeReaderContextMenu)
  document.removeEventListener('click', handleSourceImageClick, true)
  document.removeEventListener('mouseup', handleMouseUpSelection)
  document.removeEventListener('touchend', handleTouchEndSelection)
    document.removeEventListener('selectionchange', handleSelectionChange)
    window.removeEventListener('resize', checkMedia)
    window.removeEventListener('resize', measureTtsPanelInset)
    window.removeEventListener(APP_VIEWPORT_CHANGE_EVENT, handleViewportChange)
    window.removeEventListener('pagehide', handlePageHide)
    window.removeEventListener('beforeunload', handleBeforeUnload)
    document.removeEventListener('visibilitychange', handleVisibilityChange)
  if (speechTimerTicker) clearInterval(speechTimerTicker)
  if (restorePositionTimer) clearTimeout(restorePositionTimer)
  if (persistPositionTimer) clearTimeout(persistPositionTimer)
  if (legadoPositionRestoreTimer) clearTimeout(legadoPositionRestoreTimer)
  if (refreshOfflineCacheStateTimer) clearTimeout(refreshOfflineCacheStateTimer)
  if (horizontalRebuildTimer) clearTimeout(horizontalRebuildTimer)
  hoverSupportStop?.()
  hoverSupportStop = null
  hoverMediaQuery?.removeEventListener('change', syncHoverPointerSupport)
  hoverMediaQuery = null
  hoverPointerInZone = false
  hoverPointerOnControls = false
  hoverRevealSuppressed = false
  cancelHoverHide()
  clearRestoreStabilizers()
  disposeSelection()
  disposeContinuousReading()
  disposeAutoPlayback()
  store.stopTTS()
  if (typeof window !== 'undefined' && window.speechSynthesis) {
    window.speechSynthesis.onvoiceschanged = null
  }
  applySystemTheme(appStore.theme)
  store.closePanel()
  // 工具栏可见性是持久化的 store 状态: 不清掉的话, 在「工具栏已呼出」的状态下
  // 退出阅读页, 下次进来工具栏会默认展开, 而不是干净的全屏阅读态。
  store.readerControlsVisible = false
})

watch(() => config.value.readMethod, async () => {
  clearSelectionState()
  if (isContinuousMode.value) {
    await initializeContinuousChapters(store.currentIndex, false)
  } else {
    clearContinuousChapters()
    await nextTick()
    if (scrollContainerRef.value) {
      scrollContainerRef.value.scrollTo({ top: 0, left: 0, behavior: 'auto' })
    }
  }
  if (isHorizontalPageMode.value && scrollContainerRef.value) {
    resetHorizontalPagePosition()
  }
  await rebuildHorizontalPages()
  scheduleRestoreReadingPosition()
})

watch(() => store.currentIndex, () => {
  loadedReaderImageSizes.clear()
  if (!isHorizontalPageMode.value) return
  // 换章必须先置位, **不能**只靠上面那个按差值判断的 watch: 这里是"换章"这个事实的
  // 唯一权威信号, 而差值不可靠 —— 本章只有 2 页时末页索引是 1, 清 0 的差值恰好为 1,
  // 会被它的 `<= 1` 判成普通翻页, 于是画面仍用「动画时长」从第 2 页滑回第 1 页
  // (方向与用户按的"下一页"相反), 再硬切到新章首页 —— 不连贯只是换了形态。
  beginHorizontalJump(true)
  resetHorizontalPagePosition()
  void rebuildHorizontalPages().then(applyPendingPrevChapterLanding)
})

// 排版参数变化(字号/行高/段距/页宽/首行缩进)会重新分页, 页数与页边界都变了。
// 按**阅读进度比例**把位置映射到新分页, 而不是一律回到第一页 —— 否则在设置里
// 拖一下字号或页宽就会跳回本章开头, 还会把这个 0 进度写回书架。
watch(
  [() => config.value.fontSize, () => config.value.fontWeight, () => config.value.lineHeight, () => config.value.paragraphSpacing, () => config.value.firstLineIndent, () => config.value.pageWidth, () => config.value.pageVerticalPadding],
  () => {
    if (!isHorizontalPageMode.value) return
    const previousMax = Math.max(0, horizontalPages.value.length - 1)
    const progress = previousMax > 0 ? horizontalPageIndex.value / previousMax : 0
    void rebuildHorizontalPages().then(() => {
      if (!isHorizontalPageMode.value) return
      // 未兑现的跨章回退落位优先: 这次重建可能就是它唯一的机会(用户恰好在"进上一章"
      // 的窗口里改了字号, 后面不会再有 rebuild 来兑现)。而下面算出的 progress 来自
      // **旧章**的索引比例, 写回去会把用户从"上一章末页"拉到按比例映射的位置, 极端
      // 情况就是回标题页 —— 本次要修的 bug 本身。
      // 只有真的兑现了才让路; 还不能兑现(正文没到/页数还是旧章的)就照常走比例映射。
      if (applyPendingPrevChapterLanding()) return
      // 远端进度正在精确恢复时不要抢位置, 交给那套机制。
      if (pendingLegadoPageRestore.value) return
      horizontalPageIndex.value = readerPageIndexFromProgress(
        progress,
        horizontalPages.value.length,
      )
      syncHorizontalPageState()
    })
  },
)

// 正文换了(换章/原地刷新): 内容不同了, 从第一页重新开始。
//
// 页内搜索的高亮虽然也改正文 HTML, 但**不能**跟着一起清索引 —— 打开面板或每敲
// 一个字都把读者送回本章第一页是 bug。搜索引起的重分页由监听 [showSearch,
// searchQuery, ...] 的 watch 负责(见下方), 它不动位置; 跳转落位交给 locateSearchMatch。
watch(() => store.content, () => {
  loadedReaderImageSizes.clear()
  void renderPdfPagesIfNeeded()
  if (!isHorizontalPageMode.value) return
  if (!pendingLegadoPageRestore.value) horizontalPageIndex.value = 0
  // 跨章回退时正文到达触发的这次重建, 才是"新章分页就绪"的主路径(见
  // applyPendingPrevChapterLanding); 上面刚把索引清 0, 这里紧接着兑现落位。
  void rebuildHorizontalPages().then(applyPendingPrevChapterLanding)
})

watch(() => store.currentIndex, async () => {
  loadSavedReadingPosition()
  if (!store.isSpeaking) {
    clearReadingClass()
  }
  if (hideReadChaptersMode.value) {
    void pruneReadChapters(store.currentIndex)
  }
  if (!isContinuousMode.value && config.value.enablePreload) {
    store.preloadAroundChapter(store.currentIndex)
  }
  // 连续阅读(含隐藏已读)此前完全没接入预加载: 取正文走 buildContinuousChapter,
  // 既不触发上面的分支, 也不经过 loadChapter, 「预加载」开关在这里等于无效。
  // 按列表尾部预载下游, 让下一章在滚到之前就已经躺在缓存里。
  if (isContinuousMode.value && config.value.enablePreload) {
    preloadContinuousAhead()
  }
  if (isContinuousMode.value && !suppressContinuousSync.value) {
    await syncContinuousToStoreState()
  }
  scheduleRefreshOfflineCacheState()
  scheduleRestoreReadingPosition()
})

watch(
  [() => store.chapters.length, () => store.chaptersLoading, () => store.loading, isContinuousMode],
  async ([chapterCount, chaptersLoading, loadingNow, continuousMode]) => {
    if (!continuousMode || !chapterCount || chaptersLoading || loadingNow || continuousChapters.value.length) return
    await initializeContinuousChapters(store.currentIndex, false)
    // 打开书籍期间位置恢复的重试预算(~1 秒)可能被目录网络耗时耗尽,
    // 章节就绪后在这里重新读取保存位置再调度恢复。
    loadSavedReadingPosition()
    scheduleRestoreReadingPosition()
  },
  { immediate: true },
)

watch(() => store.content, () => {
  if (isContinuousMode.value) {
    const current = getContinuousChapter(store.currentIndex)
    if (current) {
      current.content = store.content
      current.html = renderChapterHtml(store.content)
    } else if (store.content) {
      void initializeContinuousChapters(store.currentIndex, false)
    }
  }
  handleContentUpdated()
  scheduleRefreshOfflineCacheState()
  scheduleRestoreReadingPosition()
})

/**
 * 原地刷新后正文长度可能变化, 章节 section 高度随之改变, 视口会被重排挤走。
 * 这里按刷新前的章节内进度把视口锚回同一位置; 因为索引与进度都没变, 恢复的
 * 一定是刷新前那一章, 不会再被 handleScroll 判成上一章。
 */
watch(() => store.contentRefreshToken, async () => {
  // 原地刷新(净化规则改动等)会重建正文 DOM: PDF 的 canvas 一并重画
  void renderPdfPagesIfNeeded()
  if (!isContinuousMode.value) return
  const index = store.currentIndex
  const progress = store.chapterScrollProgress
  // 刷新期间不让 handleScroll 用重排中的 offsetTop 反推当前章。
  suppressContinuousScrollSyncUntil = Date.now() + 700
  suppressContinuousAutoLoadUntil = Date.now() + 700
  markSkipPositionRestore(index)
  await nextTick()
  if (typeof window !== 'undefined') {
    await new Promise<void>((resolve) => window.requestAnimationFrame(() => resolve()))
  }
  if (store.currentIndex !== index) return
  scrollToContinuousChapter(index, false, progress)
})

watch(() => store.loading, (loading) => {
  if (!loading && !store.restoringLegadoProgress && pendingRestorePosition.value) {
    scheduleRestoreReadingPosition()
  }
})

watch(() => store.book?.bookUrl, (_next, previous) => {
  // 换书(阅读器内切书、不重挂载)时清掉上一本残留的跳过标记与落位意图。
  // 标记现在是"命中的那一次进入即作废"的计数消费, 没有时间兜底: 换书后若
  // 恰好进入同号章节, 残留标记会误豁免那一次本该恢复的位置。
  skipPositionRestoreIndex = null
  clearPendingPrevChapterLanding()
  // PDF 文档按 bookUrl 常驻内存, 换书时释放上一本, 否则整本 PDF 一直占着。
  if (previous) void releasePdfBook(previous)
  loadSavedReadingPosition()
  scheduleRefreshOfflineCacheState()
})

watch(() => store.restoringLegadoProgress, (restoring) => {
  if (!restoring) {
    if (pendingLegadoPageRestore.value) {
      scheduleRestoreReadingPosition()
    } else if (pendingRestorePosition.value) {
      scheduleRestoreReadingPosition()
    }
  }
})

watch(() => store.legadoPositionRestore, (restore) => {
  if (!restore) {
    pendingLegadoPageRestore.value = null
    legadoPositionRestoreAttempts = 0
    if (legadoPositionRestoreTimer) {
      clearTimeout(legadoPositionRestoreTimer)
      legadoPositionRestoreTimer = null
    }
    return
  }
  pendingLegadoPageRestore.value = {
    index: restore.index,
    position: restore.position,
    progress: restore.progress,
    token: restore.token,
  }
  pendingRestorePosition.value = null
  pendingRestoreAttempts = 0
  legadoPositionRestoreAttempts = 0
  legadoPositionRestoreSettling = false
  clearRestoreStabilizers()
  scheduleLegadoPositionRestore()
}, { immediate: true })

watch([showSearch, searchQuery, () => config.value.paragraphSpacing, () => config.value.firstLineIndent, () => config.value.chineseMode, () => store.replaceRules], () => {
  if (isContinuousMode.value) {
    syncContinuousChapterHtml()
  }
  // 左右分页渲染的是 horizontalPages 快照, 不像纵向/连续模式那样直接跟随
  // formattedContent。简繁或净化规则变化后必须重新分页, 否则页面一直显示旧正文。
  if (isHorizontalPageMode.value) {
    void rebuildHorizontalPages()
  }
  handlePresentationUpdated()
})

watch(() => config.value.selectAction, (value) => {
  if (value !== 'popup') {
    clearSelectionState()
  }
})

watch(() => store.isSpeaking, (speaking) => {
  if (speaking && !ttsPanelDismissed.value) {
    showTTSPanel.value = true
  }
  if (!speaking && !store.isAutoScrolling) {
    clearReadingClass()
  }
})

watch(
  [() => store.isNight, () => store.currentTheme.body, () => appStore.theme],
  ([isNight, body]) => {
    applySystemTheme(isNight ? 'dark' : appStore.theme, body)
  },
  { immediate: true },
)
</script>

<style scoped>
.reader-view {
  height: 100%;
  width: 100%;
  display: flex;
  position: relative;
  overflow: hidden;
  transition: background 0.3s, color 0.3s;
  padding-top: var(--safe-area-top);
  box-sizing: border-box;
}

.reader-view.disable-system-callout .chapter-text,
.reader-view.disable-system-callout .horizontal-page-content,
.reader-view.disable-system-callout .continuous-reading {
  -webkit-touch-callout: none;
}

.auto-reading-status {
  position: absolute;
  top: 12px;
  right: 48px;
  z-index: 18;
  display: flex;
  align-items: center;
  gap: 8px;
  min-height: 38px;
  max-width: calc(100% - 96px);
  padding: 5px 5px 5px 12px;
  border: 1px solid color-mix(in srgb, var(--auto-status-color) 14%, transparent);
  border-radius: 14px;
  background: color-mix(in srgb, var(--auto-status-bg) 88%, transparent);
  color: var(--auto-status-color);
  box-shadow: 0 8px 24px rgba(0, 0, 0, 0.14);
  backdrop-filter: blur(18px) saturate(120%);
  -webkit-backdrop-filter: blur(18px) saturate(120%);
}

.auto-reading-indicator {
  width: 7px;
  height: 7px;
  flex: 0 0 auto;
  border-radius: 50%;
  background: var(--color-primary);
  box-shadow: 0 0 0 4px color-mix(in srgb, var(--color-primary) 16%, transparent);
  animation: auto-reading-pulse 1.8s ease-in-out infinite;
}

.auto-reading-label {
  overflow: hidden;
  font-size: 13px;
  font-weight: 600;
  line-height: 1;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.auto-reading-mode {
  color: color-mix(in srgb, var(--auto-status-color) 62%, transparent);
  font-size: 12px;
  line-height: 1;
}

.auto-reading-status button {
  width: 28px;
  height: 28px;
  flex: 0 0 auto;
  display: grid;
  place-items: center;
  border-radius: 9px;
  background: color-mix(in srgb, var(--auto-status-color) 8%, transparent);
  transition: background 0.18s ease, transform 0.18s ease;
}

.auto-reading-status button:hover {
  background: color-mix(in srgb, var(--auto-status-color) 14%, transparent);
}

.auto-reading-status button:active {
  transform: scale(0.94);
}

.auto-reading-status button span {
  width: 9px;
  height: 9px;
  border-radius: 2px;
  background: currentColor;
}

@keyframes auto-reading-pulse {
  0%, 100% { opacity: 0.65; }
  50% { opacity: 1; }
}

.reader-drawer.with-custom-background,
.selection-menu.with-custom-background,
.drawer-translucent {
  backdrop-filter: blur(20px) saturate(120%);
  -webkit-backdrop-filter: blur(20px) saturate(120%);
}

.reader-scroll-container {
  flex: 1;
  height: 100%;
  overflow-y: auto;
  overflow-anchor: none;
  position: relative;
  /* 必须是 auto: 平滑由 utils/readerPaging 的显式补间提供, 时长受「动画时长」
   * 设置控制。若这里保留 smooth, 赋 scrollTop 会被浏览器转成时长不可控的
   * 原生平滑动画, 设置项就永远不起作用(自动滚动当初也是为此显式传 instant)。 */
  scroll-behavior: auto;
  overscroll-behavior: contain;
  -webkit-overflow-scrolling: touch;
  scrollbar-width: none;
  -ms-overflow-style: none;
  z-index: 1;
}

.reader-scroll-container.horizontal-page-mode {
  overflow-x: hidden;
  overflow-y: hidden;
  touch-action: pan-y pinch-zoom;
  overscroll-behavior: none;
}

/* 听书面板浮在正文之上、不参与滚动。分页模式下正文高度恒等于一屏,
   被面板压住的那几行永远翻不出来, 所以面板在时给容器让出底部空间:
   容器变矮 → clientHeight 变小 → 分页重建时按更矮的页高重新切页,
   最后一行落在面板上方而不是被它盖住。 */
.reader-scroll-container.tts-panel-open {
  padding-bottom: var(--tts-panel-inset, 0px);
  box-sizing: border-box;
}

/* Hide scrollbar */
.reader-scroll-container::-webkit-scrollbar {
  width: 0;
  height: 0;
  display: none;
}
.reader-scroll-container::-webkit-scrollbar-thumb {
  background: rgba(0,0,0,0.1);
  border-radius: 4px;
}
.reader-view[style*="background: #1a1a2e"] .reader-scroll-container::-webkit-scrollbar-thumb {
  background: rgba(255,255,255,0.1);
}

.content-loading {
  display: flex;
  align-items: center;
  justify-content: center;
  height: 100%;
}

.offline-banner {
  position: sticky;
  top: 0;
  z-index: 6;
  margin: 0 auto;
  width: min(100%, 880px);
  padding: 10px 16px;
  background: rgba(201, 127, 58, 0.12);
  color: var(--color-primary);
  border-bottom: 1px solid rgba(201, 127, 58, 0.18);
  font-size: 13px;
  line-height: 1.5;
  text-align: center;
  backdrop-filter: blur(6px);
}

/* 打开书失败的错误态。占满阅读区、居中，与加载动画同一个位置，
   让"转圈"和"失败"在视觉上是同一个坑位的两种状态。 */
.book-load-error {
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 10px;
  height: 100%;
  padding: 40px 24px;
  text-align: center;
}

.book-load-error-title {
  margin: 0;
  font-size: 16px;
  font-weight: 600;
  color: var(--color-primary);
}

.book-load-error-message {
  margin: 0;
  max-width: 460px;
  font-size: 13px;
  line-height: 1.6;
  opacity: 0.72;
  word-break: break-word;
}

.book-load-error-actions {
  display: flex;
  flex-wrap: wrap;
  justify-content: center;
  gap: 10px;
  margin-top: 8px;
}

.book-load-error-btn {
  min-width: 96px;
  padding: 8px 18px;
  border-radius: 999px;
  border: 1px solid currentColor;
  background: transparent;
  color: inherit;
  font-size: 13px;
  opacity: 0.7;
  cursor: pointer;
}

.book-load-error-btn:hover { opacity: 1; }

.book-load-error-btn.primary {
  border-color: transparent;
  background: var(--color-primary);
  color: #fff;
  opacity: 1;
}

.loading-spinner {
  width: 32px;
  height: 32px;
  border: 3px solid rgba(0,0,0,0.1);
  border-top-color: var(--color-primary);
  border-radius: 50%;
  animation: spin 1s linear infinite;
}
.reader-view[style*="background: #1a1a2e"] .loading-spinner {
  border-color: rgba(255,255,255,0.1);
  border-top-color: var(--color-primary);
}

@keyframes spin {
  to { transform: rotate(360deg); }
}

.chapter-content {
  margin: 0 auto;
  padding: 80px 24px;
  min-height: 100%;
  transition: all 0.3s ease;
}

.chapter-content.horizontal-page-article {
  margin: 0;
  height: 100%;
  min-height: 100%;
  /* 分页模式下宽度恒为一页(不再是 max-content 的超宽横带)。 */
  width: var(--reader-page-step);
  min-width: var(--reader-page-step);
  padding: 0;
}

.horizontal-page-layout {
  width: var(--reader-page-step);
  min-width: var(--reader-page-step);
  height: 100%;
}

.horizontal-content-page {
  width: var(--reader-page-step);
  min-width: var(--reader-page-step);
  height: 100%;
  min-height: 100%;
  padding: 0;
  box-sizing: border-box;
}

/* 分页视口: 尺寸恒为一页, 且**裁剪**掉溢出的一页。
   裁剪不只是为了视觉 —— 它同时把这一层的绘制/光栅边界钉在「一屏」大小。
   早先这里是 `display:flex; width:max-content`, 整章所有页横向排成一条超宽横带
   (10 页 × 1174 = 14675px), 再由父层整体 translate3d 平移; WebView2 对这么大的
   合成层只能分块光栅, 换章后旧块有时不失效, 于是屏幕上残留上一章同页码位置的
   文字位图 —— 它不在 DOM 里(选不中), 改窗口大小或重进阅读页才恢复。 */
.horizontal-pages {
  position: relative;
  width: 100%;
  height: 100%;
  min-height: 100%;
  overflow: hidden;
}

/* 一页 = 一屏：宽度恒等于容器宽(--reader-page-step 由 JS 写为容器宽)。
   正文的**行宽**由里面的 .horizontal-page-content 单独控制, 并在页内居中,
   所以宽窗口下是「正文居中 + 两侧留白」, 而不是一屏并排显示两页。
   这里的左右 padding 必须与 useHorizontalPaging 的 HORIZONTAL_PAGE_SIDE_PADDING
   保持一致 —— 那边用它算行宽的可用宽度, 不一致会让测量与实际排版不符。

   每页**各自**用 transform 定位(见 horizontalPageTransform): 当前页位移为 0,
   其余页平移 ±n 个屏宽。这样每个合成层都只有一页大小, 不存在超宽层; 翻页动画
   仍然是「旧页滑出、新页滑入」, 观感与整体平移一致。 */
.horizontal-page {
  position: absolute;
  top: 0;
  left: 0;
  width: var(--reader-page-step);
  min-width: var(--reader-page-step);
  height: 100%;
  min-height: 100%;
  padding: var(--reader-page-vertical-padding) var(--reader-side-padding);
  box-sizing: border-box;
  transition-property: transform;
  transition-timing-function: cubic-bezier(0.22, 0.61, 0.36, 1);
  /* will-change 由 horizontalPageWillChange 逐页给出(只有参与滑动的相邻页需要),
     不要在这里写死: 那会让每一页都常驻一个视口大小的光栅层。 */
}

.continuous-reading {
  margin: 0 auto;
  padding: 32px 0 80px;
}

.continuous-chapter {
  min-height: auto;
  padding-top: 48px;
  padding-bottom: 24px;
}

.chapter-title {
  font-size: 1.6em;
  font-weight: 700;
  margin-bottom: 2em;
  text-align: center;
  line-height: 1.4;
}

/* 净化脚本未执行时的降级提示：低调但看得见，跟着正文字号走。 */
.replace-rule-degraded {
  margin: -1em 0 2em;
  padding: 8px 12px;
  border-radius: 8px;
  border: 1px solid var(--color-warning-border, rgba(217, 119, 6, 0.35));
  background: var(--color-warning-bg, rgba(217, 119, 6, 0.08));
  color: var(--color-warning-text, #b45309);
  font-size: 0.85em;
  line-height: 1.6;
}

.replace-rule-degraded-link {
  margin-left: 4px;
  padding: 0;
  border: 0;
  background: none;
  color: inherit;
  font: inherit;
  text-decoration: underline;
  cursor: pointer;
}

/* 章节加载阶段：浮层形式，横向分页/普通/连续三种模式共用 */
.chapter-loading-stage {
  position: fixed;
  top: 72px;
  left: 50%;
  transform: translateX(-50%);
  /* 高于左抽屉弹层与阅读工具栏，保证任何时刻都看得见 */
  z-index: 200;
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 8px 16px;
  border-radius: 999px;
  font-size: 13px;
  line-height: 1.2;
  box-shadow: 0 6px 18px rgba(0, 0, 0, 0.18);
  /* 不拦截点击，避免挡住翻页/菜单手势 */
  pointer-events: none;
}

.chapter-loading-spinner {
  width: 12px;
  height: 12px;
  flex-shrink: 0;
  border: 2px solid currentColor;
  border-top-color: transparent;
  border-radius: 50%;
  animation: chapter-loading-spin 0.8s linear infinite;
}

@keyframes chapter-loading-spin {
  to {
    transform: rotate(360deg);
  }
}

.chapter-text {
  word-break: normal;
  overflow-wrap: anywhere;
  text-align: justify;
  user-select: text;
  -webkit-user-select: text;
  -webkit-touch-callout: default;
}

/* 本地 PDF: 每个占位块就是原书的一页, 由 pdf.js 画成 canvas。
   渲染前先占住约一页的比例, 避免图片插入时正文跳动。 */
.pdf-page {
  display: flex;
  justify-content: center;
  margin: 0 auto 12px;
  max-width: 100%;
  background: #fff;
  box-shadow: 0 1px 4px rgba(0, 0, 0, 0.12);
  min-height: 60vh;
  align-items: center;
}

.pdf-page-canvas {
  display: block;
  max-width: 100%;
  height: auto;
}

.pdf-page.pdf-page-ready {
  min-height: 0;
}

.pdf-page.pdf-page-missing {
  min-height: 120px;
  background: transparent;
  box-shadow: none;
}

.pdf-page.pdf-page-missing::after {
  content: '本页无法显示';
  color: #999;
  font-size: 0.9em;
}

/* 页内正文列：宽度 = 行宽(「页面宽度」设置, 由 JS 写进 --reader-text-width)。
   `margin: 0 auto` 让它在页宽大于行宽时居中, 因此宽窗口下呈现为
   「正文居中 + 两侧留白」, 而不是一屏并排显示两页。 */
.horizontal-page-content {
  width: var(--reader-text-width, 100%);
  max-width: 100%;
  margin: 0 auto;
  height: 100%;
  overflow: hidden;
  overflow-wrap: break-word;
  text-align: left;
  word-break: normal;
  box-sizing: border-box;
}

:deep(.horizontal-page-content .horizontal-flow-title) {
  margin: 0 0 1em 0;
  font-size: 1.5em;
  line-height: 1.35;
  font-weight: 700;
  text-align: center;
  break-inside: avoid;
}

:deep(.horizontal-page-content p:first-child) {
  margin-top: 0 !important;
}

:deep(.horizontal-page-content p:last-child) {
  margin-bottom: 0 !important;
}

:deep(.chapter-text p.reading) {
  background: rgba(201, 127, 58, 0.12);
  border-radius: 10px;
  box-shadow: inset 0 0 0 1px rgba(201, 127, 58, 0.18);
}

/* 书源正文的图片。
 *
 * 两条都必须写:
 *  1. `max-width: 100%` —— 不溢出正文列。
 *  2. `height: auto` —— 书源图片自带 `width`/`height` 属性(番茄源每张都有),
 *     只限宽不限高时, 浏览器会把**属性里的绝对高度**当最终高度: 一张
 *     1024x576 的图在 380px 宽的列里会被拉成 380x576, 严重变形。
 *
 * 加上 `height: auto` 后宽高比才交给浏览器按属性推导, 于是:
 *  - 图片加载**前**就能按真实比例占住位置(不再从 0 高度开始);
 *  - 加载**后**高度不变, 下方正文不会突然上移/下移。
 * 这正是 issue #21「带图章节排版有延迟/跳动」的成因 —— 用户的 GIF 里
 * 图片区域从空白变成图片的同时, 其下内容整体位移三百多像素。 */
:deep(.chapter-text img) {
  max-width: 100%;
  height: auto;
}

/* 本地 EPUB: 正文排版以原书 CSS 为准(原书规则随章节内联, 在 DOM 中位于本组件
   样式之后, 因此会覆盖这里的阅读器默认值)。这里只兜底图片不溢出容器。 */
:deep(.epub-chapter-content img),
:deep(.epub-chapter-content svg) {
  max-width: 100%;
  height: auto;
}

:deep(.chapter-text font),
:deep(.chapter-text big),
:deep(.chapter-text small),
:deep(.chapter-text [style*="font-size"]),
:deep(.chapter-text [style*="font-family"]),
:deep(.chapter-text [style*="line-height"]) {
  font-size: inherit !important;
  font-family: inherit !important;
  line-height: inherit !important;
}

.reader-view::selection,
.reader-view :deep(*)::selection {
  color: var(--reader-selection-color);
  background: var(--reader-selection-bg);
}

:deep(.chapter-text p.reader-indent) {
  text-indent: 2em !important;
}

/* 书源正文的默认无缩进。
 *
 * 这里用 `:where()` 把整条选择器的特异性压到 **0** —— 包括 Vue 的 scoped 属性
 * 选择器也一并被归零。原因: 这条兜底原本特异性 (0,2,1), 会压过本地 EPUB 的原书
 * 样式 `.epub-chapter-content p { text-indent: 2em }`(0,1,1), 把原书排版静默清零,
 * 表现为"EPUB 里首行缩进不生效"。
 *
 * 归零之后, 任何原书声明都能自然胜出(原书还写了 `p.foot`/`.sign` 等 text-indent:0
 * 的例外段落, 它们也一并恢复原意)。书源正文没有原书样式, 仍然得到无缩进;
 * 需要缩进的段落继续走上面那条 `.reader-indent`(带 !important)。
 *
 * 只改 CSS 特异性, 不碰 DOM 结构、不碰分页测量 —— 与正文截断问题完全无关。 */
:deep(:where(.chapter-text p)) {
  text-indent: 0;
}

/* 用户关闭「首行缩进」时, 连同原书的缩进一起压掉(明确要求, 优先于原书)。 */
:deep(.epub-chapter-content.reader-no-indent p) {
  text-indent: 0 !important;
}

:deep(.chapter-text p) {
  user-select: text;
  -webkit-user-select: text;
}

.chapter-footer {
  margin-top: 60px;
  text-align: center;
  padding-bottom: 40px;
}

.continuous-loading-inline {
  text-align: center;
  padding: 18px 24px;
  opacity: 0.6;
  font-size: 13px;
}

.next-btn {
  padding: 12px 36px;
  border-radius: 30px;
  background: transparent;
  border: 1px solid currentColor;
  color: inherit;
  font-size: 14px;
  opacity: 0.6;
  cursor: pointer;
  transition: all 0.2s;
}

.next-btn:hover:not(:disabled) {
  opacity: 1;
  background: rgba(0, 0, 0, 0.05);
}

.next-btn:disabled {
  opacity: 0.2;
  cursor: not-allowed;
}

/* Slide Drawer Overlay */
.reader-overlay {
  position: fixed;
  top: var(--titlebar-height, 30px);
  right: 0;
  bottom: 0;
  left: 0;
  background: rgba(0,0,0,0.4);
  z-index: 40;
}

.reader-drawer {
  position: fixed;
  top: var(--titlebar-height, 30px);
  bottom: var(--safe-area-bottom);
  left: 0;
  width: min(340px, 85vw);
  box-sizing: border-box;
  padding-top: var(--safe-area-top);
  z-index: 50;
  /* 文字色由模板 inline style 给出(见 .reader-drawer 的 :style), 作为抽屉内
   * currentColor 取色的兜底 —— 目前依赖"每个子组件自己设 color: theme.fontColor",
   * 将来新增子组件忘记设置时, 文字色不会掉回应用主题的 --color-text。 */
  box-shadow: 4px 0 24px rgba(0,0,0,0.15);
  transition: background 0.3s;
}

/* 设置面板内容较宽(配色卡片 / 快捷键列表), 单独放宽抽屉;
 * 目录、书架等窄面板保持原宽度。 */
.reader-drawer.drawer-wide {
  width: min(520px, 94vw);
}

.selection-menu {
  position: fixed;
  z-index: 60;
  min-width: 220px;
  max-width: min(320px, calc(100vw - 32px));
  border-radius: 14px;
  box-shadow: 0 10px 30px rgba(0, 0, 0, 0.18);
  border: 1px solid rgba(0, 0, 0, 0.06);
  overflow: hidden;
}

.selection-menu-text {
  padding: 12px 14px 8px;
  font-size: 13px;
  line-height: 1.5;
  opacity: 0.72;
  word-break: break-all;
}

.selection-menu-actions {
  display: grid;
  grid-template-columns: repeat(2, minmax(0, 1fr));
  gap: 8px;
  padding: 0 12px 12px;
}

.selection-menu-actions button {
  border: 1px solid color-mix(in srgb, var(--color-primary) 34%, transparent);
  border-radius: 10px;
  padding: 10px 12px;
  /* 划词菜单挂在 .reader-view 内, 文字的 color 来自 chromeTheme.fontColor
   * (见模板 :style), 所以 currentColor 取到的就是阅读主题文字色。 */
  background: color-mix(in srgb, var(--color-primary) 14%, transparent);
  color: currentColor;
  font-size: 13px;
  cursor: pointer;
}

.selection-menu-actions button:hover {
  background: color-mix(in srgb, var(--color-primary) 20%, transparent);
}

.selection-menu-actions button:first-child {
  grid-column: 1 / -1;
}

.reader-context-menu {
  position: fixed;
  z-index: 80;
  width: 190px;
  display: grid;
  padding: 6px;
  border: 1px solid color-mix(in srgb, currentColor 12%, transparent);
  border-radius: 10px;
  box-shadow: 0 14px 32px rgba(0, 0, 0, 0.2);
  backdrop-filter: blur(18px) saturate(120%);
}

.reader-context-menu button {
  min-height: 36px;
  padding: 0 10px;
  border: 0;
  border-radius: 7px;
  background: transparent;
  color: inherit;
  text-align: left;
  font-size: 13px;
}

.reader-context-menu button:hover:not(:disabled) {
  background: color-mix(in srgb, currentColor 10%, transparent);
}

.reader-context-menu button:disabled {
  opacity: 0.38;
}

.reader-replace-overlay {
  position: fixed;
  inset: 0;
  z-index: 90;
  display: grid;
  place-items: center;
  padding: 20px;
  background: rgba(0, 0, 0, 0.42);
  backdrop-filter: blur(4px);
}

.reader-replace-dialog {
  width: min(420px, 100%);
  box-sizing: border-box;
  display: grid;
  gap: 14px;
  padding: 20px;
  border: 1px solid color-mix(in srgb, currentColor 14%, transparent);
  border-radius: 14px;
  background: inherit;
  box-shadow: 0 18px 48px rgba(0, 0, 0, 0.28);
}

.reader-replace-dialog h3 {
  margin: 0;
  font-size: 16px;
  font-weight: 600;
}

.reader-replace-field {
  display: grid;
  gap: 7px;
}

.reader-replace-field > span,
.reader-replace-check {
  font-size: 13px;
}

.reader-replace-field textarea {
  width: 100%;
  box-sizing: border-box;
  resize: vertical;
  padding: 9px 10px;
  border: 1px solid color-mix(in srgb, currentColor 20%, transparent);
  border-radius: 8px;
  background: color-mix(in srgb, currentColor 6%, transparent);
  color: inherit;
  font: inherit;
  line-height: 1.6;
}

.reader-replace-field textarea:focus {
  outline: none;
  border-color: var(--color-primary);
}

.reader-replace-check {
  display: flex;
  align-items: center;
  gap: 8px;
}

.reader-replace-scopes,
.reader-replace-actions {
  display: flex;
  gap: 8px;
}

.reader-replace-scopes button,
.reader-replace-actions button {
  padding: 9px 12px;
  border: 1px solid color-mix(in srgb, currentColor 18%, transparent);
  border-radius: 8px;
  background: transparent;
  color: inherit;
  font: inherit;
  font-size: 13px;
  cursor: pointer;
}

.reader-replace-scopes button {
  flex: 1;
}

.reader-replace-scopes button.active {
  border-color: var(--color-primary);
  background: color-mix(in srgb, var(--color-primary) 16%, transparent);
}

.reader-replace-actions {
  justify-content: flex-end;
  margin-top: 2px;
}

.reader-replace-actions .primary {
  border-color: var(--color-primary);
  background: color-mix(in srgb, var(--color-primary) 18%, transparent);
}

.reader-replace-actions button:disabled {
  opacity: 0.45;
  cursor: not-allowed;
}

:deep(.search-highlight) {
  background: yellow;
  color: black;
  border-radius: 2px;
}

/* 书源图片气泡: 剥离 `,{json}` 后可点击的段评图标 */
:deep(img.source-clickable) {
  cursor: pointer;
}

:deep(img.source-inline-bubble) {
  display: inline-block;
  height: 1.15em;
  width: auto;
  vertical-align: -0.2em;
  margin: 0 0.15em;
}

:deep(.search-highlight.current-match) {
  background: orange;
}

@media (max-width: 768px) {
  .auto-reading-status {
    top: 8px;
    right: 10px;
    max-width: calc(100% - 20px);
  }

  .auto-reading-mode {
    display: none;
  }

  .reader-scroll-container.horizontal-page-mode {
    scroll-behavior: auto;
  }

  .chapter-content {
    padding: 24px 20px 8px;
    min-height: auto;
    height: auto;
  }

  .continuous-reading {
    padding: 16px 0 8px;
  }

  .continuous-chapter {
    padding-top: 20px;
    padding-bottom: 8px;
  }

  .chapter-title {
    margin-bottom: 0.9em;
  }

  .chapter-footer {
    margin-top: 12px;
    padding-bottom: 0;
  }

  .reader-drawer {
    top: calc(var(--titlebar-height, 30px) + var(--safe-area-top));
    bottom: var(--safe-area-bottom);
    width: min(340px, 85vw);
    padding-top: 0;
    padding-bottom: var(--safe-area-bottom);
    box-sizing: border-box;
  }
}

/* Transitions */
.fade-enter-active, .fade-leave-active { transition: opacity 0.3s; }
.fade-enter-from, .fade-leave-to { opacity: 0; }

.auto-reading-status-enter-active,
.auto-reading-status-leave-active {
  transition: opacity 0.2s ease, transform 0.2s ease;
}

.auto-reading-status-enter-from,
.auto-reading-status-leave-to {
  opacity: 0;
  transform: translateY(-6px);
}

.slide-left-enter-active, .slide-left-leave-active { transition: transform 0.35s cubic-bezier(0.2, 0.8, 0.2, 1); }
.slide-left-enter-from, .slide-left-leave-to { transform: translateX(-100%); }

.fade-slide-right-enter-active, .fade-slide-right-leave-active { transition: all 0.3s ease; }
.fade-slide-right-enter-from, .fade-slide-right-leave-to { transform: translateX(-20px); opacity: 0; }

.fade-slide-left-enter-active, .fade-slide-left-leave-active { transition: all 0.3s ease; }
.fade-slide-left-enter-from, .fade-slide-left-leave-to { transform: translateX(20px); opacity: 0; }
</style>
