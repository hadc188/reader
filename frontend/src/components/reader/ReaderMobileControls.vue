<template>
  <div
    class="mobile-controls"
    :class="{ mini }"
    :style="{ '--popup-bg': theme.popup, '--font-color': theme.fontColor }"
    @click.stop
    @touchstart.stop
    @touchmove.stop
    @touchend.stop
  >
    <!-- Top Bar -->
    <Transition name="slide-down">
      <div v-show="show" class="m-top-bar">
        <!--
          迷你模式只保留上一章/下一章/目录(见下方 nav-row 与这里的目录项),
          其余入口(首页/书架/书源/设置)与两侧悬浮钮都不渲染 —— 小窗里那些
          入口既占地方又基本用不到。
        -->
        <div v-if="!mini" class="m-top-item" @click="$emit('goHome')">
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="m15 18-6-6 6-6" /></svg>
          <span>首页</span>
        </div>
        <div v-if="!mini" class="m-top-item" :class="{ active: store.activePanel === 'bookshelf' }" @click="store.togglePanel('bookshelf')">
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M4 19.5v-15A2.5 2.5 0 0 1 6.5 2H20v20H6.5a2.5 2.5 0 0 1 0-5H20" /></svg>
          <span>书架</span>
        </div>
        <div v-if="!mini" class="m-top-item" :class="{ active: store.activePanel === 'source' }" @click="store.togglePanel('source')">
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><rect width="7" height="7" x="3" y="3" rx="1" /><rect width="7" height="7" x="14" y="3" rx="1" /><rect width="7" height="7" x="3" y="14" rx="1" /><rect width="7" height="7" x="14" y="14" rx="1" /></svg>
          <span>书源</span>
        </div>
        <div class="m-top-item" :class="{ active: store.activePanel === 'catalog' }" @click="store.togglePanel('catalog')">
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M3 12h18M3 6h18M3 18h18" /></svg>
          <span>目录</span>
        </div>
        <div v-if="!mini" class="m-top-item" :class="{ active: store.activePanel === 'settings' }" @click="store.togglePanel('settings')">
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M12.22 2h-.44a2 2 0 0 0-2 2v.18a2 2 0 0 1-1 1.73l-.43.25a2 2 0 0 1-2 0l-.15-.08a2 2 0 0 0-2.73.73l-.22.38a2 2 0 0 0 .73 2.73l.15.1a2 2 0 0 1 1 1.72v.51a2 2 0 0 1-1 1.74l-.15.09a2 2 0 0 0-.73 2.73l.22.38a2 2 0 0 0 2.73.73l.15-.08a2 2 0 0 1 2 0l.43.25a2 2 0 0 1 1 1.73V20a2 2 0 0 0 2 2h.44a2 2 0 0 0 2-2v-.18a2 2 0 0 1 1-1.73l.43-.25a2 2 0 0 1 2 0l.15.08a2 2 0 0 0 2.73-.73l.22-.39a2 2 0 0 0-.73-2.73l-.15-.08a2 2 0 0 1-1-1.74v-.5a2 2 0 0 1 1-1.74l.15-.09a2 2 0 0 0 .73-2.73l-.22-.38a2 2 0 0 0-2.73-.73l-.15.08a2 2 0 0 1-2 0l-.43-.25a2 2 0 0 1-1-1.73V4a2 2 0 0 0-2-2z" /><circle cx="12" cy="12" r="3" /></svg>
          <span>设置</span>
        </div>
      </div>
    </Transition>

    <!-- Bottom Bar -->
    <Transition name="slide-up">
      <div v-show="show" class="m-bottom-bar">
        <!-- 迷你模式下进度条与页码行没有空间, 只留章节导航。 -->
        <div v-if="!mini" class="progress-row">
          <!--
            进度条可点击/拖动跳章。原来只是一个装饰(点整行打开缓存面板),
            宽屏下用户会自然地拖它 —— 拖动时不再冒泡到 progress-row, 避免
            拖完又弹出面板。
          -->
          <div
            ref="progressTrackRef"
            class="progress-track"
            role="slider"
            :aria-valuemin="1"
            :aria-valuemax="Math.max(1, store.chapters.length)"
            :aria-valuenow="store.currentIndex + 1"
            aria-label="阅读进度"
            tabindex="0"
            @click.stop="seekToPointer($event)"
            @pointerdown.stop="startSeek($event)"
            @keydown.left.prevent="$emit('seekChapter', store.currentIndex - 1)"
            @keydown.right.prevent="$emit('seekChapter', store.currentIndex + 1)"
          >
            <div class="progress-fill" :style="{ width: store.readingProgress }"></div>
            <div class="progress-thumb" :style="{ left: store.readingProgress }"></div>
          </div>
          <!-- 真实页码/章节号, 不再是写死的"第 1/1 页"。 -->
          <button type="button" class="page-text" @click="$emit('progress')">
            {{ chapterPositionText }}
          </button>
        </div>
        <div v-if="!mini" class="nav-row">
          <div class="nav-btn" :class="{ disabled: !store.hasPrev }" @click="$emit('prev')">
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="m15 18-6-6 6-6" /></svg>
            上一章
          </div>
          <div class="progress-percent">阅读进度: {{ store.readingProgress }}</div>
          <div class="nav-btn" :class="{ disabled: !store.hasNext }" @click="$emit('next')">
            下一章
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="m9 18 6-6-6-6" /></svg>
          </div>
        </div>

        <!--
          迷你模式的底栏: 章节导航与字体面板共用同一行 —— 小窗只有 240px 高,
          另起一行会把正文挤掉, 而调字体时恰恰最需要看到正文的变化。
        -->
        <div v-else class="nav-row mini-nav-row">
          <template v-if="miniFontOpen">
            <button
              class="mini-font-toggle active"
              title="收起字体设置"
              @click="miniFontOpen = false"
            >Aa</button>
            <div class="mini-font-group">
              <span class="mini-font-label">字号</span>
              <button class="mini-step" :disabled="store.config.fontSize <= READER_FONT_SIZE_MIN" @click="stepFontSize(-1)">−</button>
              <span class="mini-step-val">{{ store.config.fontSize }}</span>
              <button class="mini-step" :disabled="store.config.fontSize >= READER_FONT_SIZE_MAX" @click="stepFontSize(1)">+</button>
            </div>
            <div class="mini-font-group">
              <span class="mini-font-label">字重</span>
              <button class="mini-step" :disabled="store.config.fontWeight <= MIN_FONT_WEIGHT" @click="stepFontWeight(-100)">−</button>
              <span class="mini-step-val">{{ store.config.fontWeight }}</span>
              <button class="mini-step" :disabled="store.config.fontWeight >= MAX_FONT_WEIGHT" @click="stepFontWeight(100)">+</button>
            </div>
          </template>
          <template v-else>
            <div class="nav-btn" :class="{ disabled: !store.hasPrev }" @click="$emit('prev')">
              <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="m15 18-6-6 6-6" /></svg>
              上一章
            </div>
            <button class="mini-font-toggle" title="字体大小与粗细" @click="miniFontOpen = true">Aa</button>
            <div class="nav-btn" :class="{ disabled: !store.hasNext }" @click="$emit('next')">
              下一章
              <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="m9 18 6-6-6-6" /></svg>
            </div>
          </template>
        </div>
      </div>
    </Transition>

    <!-- Left Floating: 迷你模式隐藏, 这些入口在小窗里用不到。 -->
    <Transition v-if="!mini" name="fade">
      <div v-show="show" class="m-float m-float-left">
        <button class="m-btn" @click="$emit('bookmark')">
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="m19 21-7-4-7 4V5a2 2 0 0 1 2-2h10a2 2 0 0 1 2 2v16z" /></svg>
        </button>
        <button class="m-btn" @click="$emit('search')">
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><circle cx="11" cy="11" r="8" /><path d="m21 21-4.3-4.3" /></svg>
        </button>
        <button class="m-btn" @click="$emit('info')">
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><circle cx="12" cy="12" r="10" /><path d="M12 16v-4M12 8h.01" /></svg>
        </button>
        <button class="m-btn" @click="$emit('scrollTop')">
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M12 19V5M5 12l7-7 7 7" /></svg>
        </button>
        <button class="m-btn" @click="$emit('scrollBottom')">
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M12 5v14M19 12l-7 7-7-7" /></svg>
        </button>
      </div>
    </Transition>

    <!-- Right Floating: 同上, 迷你模式隐藏。 -->
    <Transition v-if="!mini" name="fade">
      <div v-show="show" class="m-float m-float-right">
        <button class="m-btn" :class="{ spinning: store.loading }" @click="store.refreshContent()">
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M21 12a9 9 0 0 0-9-9 9.75 9.75 0 0 0-6.74 2.74L3 8" /><path d="M3 3v5h5" /><path d="M3 12a9 9 0 0 0 9 9 9.75 9.75 0 0 0 6.74-2.74L21 16" /><path d="M16 16h5v5" /></svg>
        </button>
        <button class="m-btn" :class="{ active: store.isAutoScrolling }" @click="store.toggleAutoReading()">
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M2 12s3-7 10-7 10 7 10 7-3 7-10 7-10-7-10-7Z" /><circle cx="12" cy="12" r="3" /></svg>
        </button>
        <button class="m-btn" :class="{ active: isSpeaking }" @click="$emit('tts')">
          <svg v-if="!isSpeaking" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M3 18v-6a9 9 0 0 1 18 0v6" /><path d="M21 19a2 2 0 0 1-2 2h-1a2 2 0 0 1-2-2v-3a2 2 0 0 1 2-2h3zM3 19a2 2 0 0 0 2 2h1a2 2 0 0 0 2-2v-3a2 2 0 0 0-2-2H3z" /></svg>
          <svg v-else-if="isPaused" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="m5 3 14 9-14 9V3z" /></svg>
          <svg v-else viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><rect x="6" y="4" width="4" height="16" /><rect x="14" y="4" width="4" height="16" /></svg>
        </button>
        <button class="m-btn" @click="store.toggleNight()">
          <svg v-if="!store.isNight" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M21 12.79A9 9 0 1 1 11.21 3 7 7 0 0 0 21 12.79z" /></svg>
          <svg v-else viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><circle cx="12" cy="12" r="4" /><path d="M12 2v2M12 20v2M4.93 4.93l1.41 1.41M17.66 17.66l1.41 1.41M2 12h2M20 12h2M6.34 17.66l-1.41 1.41M19.07 4.93l-1.41 1.41" /></svg>
        </button>
      </div>
    </Transition>
  </div>
</template>

<script setup lang="ts">
import { computed, onBeforeUnmount, ref, watch } from 'vue'
import { useReaderStore } from '../../stores/reader'
import {
  READER_FONT_SIZE_MAX,
  READER_FONT_SIZE_MIN,
  READER_FONT_WEIGHT_MAX,
  READER_FONT_WEIGHT_MIN,
  stepReaderFontSize,
  stepReaderFontWeight,
} from '../../utils/readerFontSize'

const store = useReaderStore()
const theme = computed(() => store.chromeTheme)

/** 底部栏的章节位置提示, 取代原来写死的"第 1/1 页"。 */
const chapterPositionText = computed(() => {
  const total = store.chapters.length
  if (total === 0) return '—'
  return `第 ${store.currentIndex + 1}/${total} 章`
})

const progressTrackRef = ref<HTMLElement | null>(null)

/**
 * 迷你模式的字体面板是否展开。与章节导航共用底栏那一行, 因此不占额外高度。
 * 不持久化: 它是一次临时调整, 每次呼出控件时默认回到章节导航(见下方 watch)。
 */
const miniFontOpen = ref(false)

const MIN_FONT_WEIGHT = READER_FONT_WEIGHT_MIN
const MAX_FONT_WEIGHT = READER_FONT_WEIGHT_MAX

/** 字号加减。边界逻辑复用被单测覆盖的工具函数, 到界后按钮会自行禁用。 */
function stepFontSize(delta: number) {
  const next = stepReaderFontSize(store.config.fontSize, delta)
  if (next !== store.config.fontSize) {
    store.updateConfig('fontSize', next)
  }
}

/** 字重加减, 步长 100 与设置面板的滑块一致。 */
function stepFontWeight(delta: number) {
  const next = stepReaderFontWeight(store.config.fontWeight, delta)
  if (next !== store.config.fontWeight) {
    store.updateConfig('fontWeight', next)
  }
}

const props = defineProps<{
  show: boolean
  isSpeaking?: boolean
  isPaused?: boolean
  /** 迷你模式: 只保留上一章/下一章/目录。 */
  mini?: boolean
}>()

const emit = defineEmits<{
  goHome: []
  scrollTop: []
  scrollBottom: []
  prev: []
  next: []
  bookmark: []
  search: []
  info: []
  tts: []
  progress: []
  /** 拖动进度条后请求跳到指定章节下标。 */
  seekChapter: [index: number]
}>()

/** 把指针位置换算成章节下标(按比例, 结果夹在合法范围内)。 */
function chapterIndexFromPointer(clientX: number): number {
  const track = progressTrackRef.value
  const total = store.chapters.length
  if (!track || total <= 0) return 0
  const rect = track.getBoundingClientRect()
  if (rect.width <= 0) return 0
  const ratio = (clientX - rect.left) / rect.width
  const clamped = Math.max(0, Math.min(1, ratio))
  return Math.round(clamped * (total - 1))
}

function seekToPointer(event: PointerEvent | MouseEvent) {
  emit('seekChapter', chapterIndexFromPointer(event.clientX))
}

/**
 * 拖动跳章。
 *
 * 用 pointer 事件而非 mouse/touch: 一套逻辑同时覆盖鼠标与触摸, 且 `setPointerCapture`
 * 能在指针移出进度条(甚至移出窗口)后继续收到 move —— 手拖快了不会中途丢失。
 */
function startSeek(event: PointerEvent) {
  const target = event.currentTarget as HTMLElement | null
  target?.setPointerCapture?.(event.pointerId)
  seekToPointer(event)

  const onMove = (moveEvent: PointerEvent) => seekToPointer(moveEvent)
  const onUp = (upEvent: PointerEvent) => {
    target?.releasePointerCapture?.(upEvent.pointerId)
    window.removeEventListener('pointermove', onMove)
    window.removeEventListener('pointerup', onUp)
    window.removeEventListener('pointercancel', onUp)
    seekCleanup = null
  }

  window.addEventListener('pointermove', onMove)
  window.addEventListener('pointerup', onUp)
  window.addEventListener('pointercancel', onUp)
  // 组件在拖动过程中被卸载(例如退出阅读页)时, 清掉挂在 window 上的监听,
  // 否则监听会一直留着并操作已卸载组件的状态。
  // 三个监听都要能清掉: 只存 onUp 会漏掉 pointermove, 它会继续触发跳章。
  seekCleanup = () => {
    window.removeEventListener('pointermove', onMove)
    window.removeEventListener('pointerup', onUp)
    window.removeEventListener('pointercancel', onUp)
  }
}

let seekCleanup: (() => void) | null = null

onBeforeUnmount(() => {
  seekCleanup?.()
  seekCleanup = null
})

// 每次重新呼出控件时收起字体面板: 它是临时调整, 不该"粘"在下一次呼出上。
// (组件常驻, 只靠 v-show 隐藏, 因此 ref 不会随隐藏重置。)
watch(() => props.show, (visible) => {
  if (visible) miniFontOpen.value = false
})
</script>

<style scoped>
.mobile-controls {
  position: absolute;
  inset: 0;
  z-index: 30;
  pointer-events: none;
}

.m-top-bar {
  position: absolute;
  top: 0;
  left: 0;
  right: 0;
  min-height: 56px;
  background: var(--popup-bg);
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
  padding: calc(8px + var(--safe-area-top)) calc(16px + var(--safe-area-right)) 8px calc(16px + var(--safe-area-left));
  z-index: 20;
  box-shadow: 0 2px 10px rgba(0,0,0,0.05);
  color: var(--font-color);
  box-sizing: border-box;
  overflow-x: auto;
  overflow-y: hidden;
  -webkit-overflow-scrolling: touch;
  scrollbar-width: none;
  pointer-events: auto;
}

.m-top-item {
  display: flex;
  align-items: center;
  gap: 4px;
  font-size: 13px;
  opacity: 0.7;
  cursor: pointer;
  min-width: 0;
  flex: 1 1 0;
  justify-content: center;
  padding: 0 2px;
}

.m-top-item svg { width: 18px; height: 18px; }
.m-top-item span {
  white-space: nowrap;
}
.m-top-item.active { opacity: 1; color: var(--color-primary, #c97f3a); }

.m-bottom-bar {
  position: absolute;
  bottom: 0;
  left: 0;
  right: 0;
  padding: 16px calc(16px + var(--safe-area-right)) calc(16px + var(--safe-area-bottom)) calc(16px + var(--safe-area-left));
  background: var(--popup-bg);
  z-index: 20;
  box-shadow: 0 -2px 10px rgba(0,0,0,0.05);
  color: var(--font-color);
  display: flex;
  flex-direction: column;
  gap: 16px;
  box-sizing: border-box;
  pointer-events: auto;
}

.progress-row {
  display: flex;
  align-items: center;
  gap: 12px;
}

.progress-track {
  flex: 1;
  height: 4px;
  background: rgba(0,0,0,0.1);
  border-radius: 2px;
  position: relative;
  cursor: pointer;
  touch-action: none;
}

/*
 * 4px 的轨道太细不好点中。用伪元素把可点区域向上下各扩 8px(总高 20px),
 * 视觉仍是 4px。不能用 padding 扩: 填充条/滑块是绝对定位, 以 padding box
 * 为基准, 加了 padding 会被一起撑高。
 */
.progress-track::before {
  content: '';
  position: absolute;
  left: 0;
  right: 0;
  top: -8px;
  bottom: -8px;
}

.progress-track:focus-visible {
  outline: 2px solid var(--color-primary, #c97f3a);
  outline-offset: 6px;
}

.progress-fill {
  position: absolute;
  left: 0;
  top: 0;
  bottom: 0;
  background: var(--color-primary, #c97f3a);
  border-radius: 2px;
}

.progress-thumb {
  position: absolute;
  top: 50%;
  transform: translate(-50%, -50%);
  width: 14px;
  height: 14px;
  border-radius: 50%;
  background: white;
  border: 2px solid var(--color-primary, #c97f3a);
  box-shadow: 0 1px 3px rgba(0,0,0,0.2);
}

.page-text {
  font-size: 12px;
  opacity: 0.6;
  border: none;
  background: transparent;
  color: inherit;
  cursor: pointer;
  white-space: nowrap;
  font-variant-numeric: tabular-nums;
}

.page-text:hover {
  opacity: 1;
  color: var(--color-primary, #c97f3a);
}

.nav-row {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
}

.nav-btn {
  display: flex;
  align-items: center;
  gap: 4px;
  font-size: 14px;
  cursor: pointer;
}
.nav-btn svg { width: 16px; height: 16px; }
.nav-btn.disabled { opacity: 0.3; cursor: not-allowed; }

/* ─── 迷你模式的字体调节(与章节导航共用底栏一行) ─── */
/*
 * 迷你模式下把主题色的选中态压成中性: 小窗本来就为"不显眼"服务, 顶栏的
 * 目录、Aa、加减按钮若亮成主题色会很抢眼。限定在 .mini 作用域内覆盖,
 * 普通窗口的配色完全不受影响。
 */
.mobile-controls.mini .m-top-item.active {
  color: inherit;
  opacity: 1;
}

/*
 * 用 .nav-row.mini-nav-row 提高优先级: 窄屏断点里的 .nav-row 设了
 * flex-wrap: wrap, 展开字体面板时若换行, 底栏会变高并吃掉正文高度。
 * 两者优先级相同时按源码顺序决胜, 而断点在文件末尾, 因此必须显式压过它。
 */
.nav-row.mini-nav-row {
  gap: 6px;
  flex-wrap: nowrap;
}

.mini-font-toggle {
  flex: 0 0 auto;
  min-width: 32px;
  padding: 6px 8px;
  font-size: 13px;
  font-weight: 600;
  line-height: 1;
  border: 1px solid currentColor;
  border-radius: 6px;
  background: transparent;
  color: inherit;
  opacity: 0.75;
  cursor: pointer;
}

/* 悬停与展开态统一用中性灰, 不用主题色 —— 迷你模式追求不显眼, 彩色描边
   在小窗里很抢眼。展开态另加一层淡底纹以便和悬停区分。 */
.mini-font-toggle:hover,
.mini-font-toggle.active {
  opacity: 1;
  border-color: rgba(127, 127, 127, 0.5);
  background: rgba(127, 127, 127, 0.12);
}

/* 展开后两个参数并排, 各自是一组「标签 − 值 +」。 */
.mini-font-group {
  display: flex;
  align-items: center;
  gap: 2px;
  flex: 1 1 auto;
  min-width: 0;
}

/* 320px 下空间很紧, 标签优先被压缩/隐藏, 保留可点的加减按钮。 */
.mini-font-label {
  font-size: 11px;
  opacity: 0.6;
  white-space: nowrap;
  overflow: hidden;
}

.mini-step {
  flex: 0 0 auto;
  width: 24px;
  height: 24px;
  display: flex;
  align-items: center;
  justify-content: center;
  font-size: 15px;
  line-height: 1;
  border: 1px solid rgba(127, 127, 127, 0.35);
  border-radius: 6px;
  background: transparent;
  color: inherit;
  cursor: pointer;
}

/* 悬停反馈也保持中性: 只把描边和底纹加深一档, 不引入主题色。 */
.mini-step:hover:not(:disabled) {
  border-color: rgba(127, 127, 127, 0.6);
  background: rgba(127, 127, 127, 0.12);
}

.mini-step:disabled {
  opacity: 0.3;
  cursor: not-allowed;
}

.mini-step-val {
  min-width: 26px;
  text-align: center;
  font-size: 12px;
  font-variant-numeric: tabular-nums;
}

.progress-percent {
  font-size: 12px;
  opacity: 0.6;
  white-space: nowrap;
  text-align: center;
}

.m-float {
  position: absolute;
  top: calc(50% + (var(--safe-area-top) - var(--safe-area-bottom)) / 2);
  transform: translateY(-50%);
  display: flex;
  flex-direction: column;
  gap: 16px;
  z-index: 20;
  max-height: calc(100% - var(--safe-area-top) - var(--safe-area-bottom) - 32px);
  overflow: auto;
  scrollbar-width: none;
  pointer-events: auto;
}

.m-float-left { left: calc(16px + var(--safe-area-left)); }
.m-float-right { right: calc(16px + var(--safe-area-right)); }

.m-btn {
  width: 40px;
  height: 40px;
  border-radius: 50%;
  background: var(--popup-bg);
  box-shadow: 0 2px 8px rgba(0,0,0,0.1);
  display: flex;
  align-items: center;
  justify-content: center;
  color: var(--font-color);
  border: 1px solid rgba(0,0,0,0.05);
  opacity: 0.8;
  cursor: pointer;
}

.m-btn svg { width: 18px; height: 18px; }
.m-btn.active { color: var(--color-primary, #c97f3a); opacity: 1; }
.m-btn.spinning svg { animation: spin 1s linear infinite; }

.slide-down-enter-active, .slide-down-leave-active { transition: transform 0.3s ease; }
.slide-down-enter-from, .slide-down-leave-to { transform: translateY(-100%); }

.slide-up-enter-active, .slide-up-leave-active { transition: transform 0.3s ease; }
.slide-up-enter-from, .slide-up-leave-to { transform: translateY(100%); }

.fade-enter-active, .fade-leave-active { transition: opacity 0.3s ease; }
.fade-enter-from, .fade-leave-to { opacity: 0; }

.m-float::-webkit-scrollbar {
  display: none;
}

.m-top-bar::-webkit-scrollbar {
  display: none;
}

/*
 * 宽屏适配: 这套控件原本只为窄屏设计, 桌面大窗口下有两点会被"拉坏" ——
 *   1. 顶栏每个 .m-top-item 是 `flex: 1 1 0`, 铺满整宽后 5 个按钮会散到
 *      屏幕两端、彼此相距几百像素;
 *   2. 底栏进度条铺满整宽, 观感同样失衡。
 * 因此宽屏时把内容收成居中一簇(保留移动版样式不动, 用 min-width 断点隔离)。
 * 左右悬浮钮仍贴边: 呼出式控件贴边符合直觉, 也避免遮挡正文。
 */
@media (min-width: 769px) {
  .m-top-bar {
    justify-content: center;
    gap: 40px;
  }

  .m-top-item {
    flex: 0 0 auto;
    padding: 0 var(--space-2);
  }

  .progress-row,
  .nav-row {
    width: 100%;
    max-width: 640px;
    margin: 0 auto;
  }
}

@keyframes spin {
  to { transform: rotate(360deg); }
}

@media (max-width: 420px) {
  .m-top-bar {
    justify-content: space-between;
    gap: 14px;
  }

  .m-top-item {
    font-size: 12px;
    flex: 0 0 auto;
  }

  .m-bottom-bar {
    gap: 12px;
  }

  .progress-row {
    gap: 10px;
  }

  .page-text {
    font-size: 11px;
  }

  .nav-row {
    flex-wrap: wrap;
    justify-content: center;
  }

  .nav-btn {
    font-size: 13px;
  }

  .progress-percent {
    order: 3;
    width: 100%;
  }

  .m-float {
    gap: 12px;
  }

  .m-btn {
    width: 38px;
    height: 38px;
  }
}
</style>
