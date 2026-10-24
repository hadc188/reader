<template>
  <Transition name="slide-up">
    <!-- 圆形气泡态: 只留一个按钮, 最不挡正文。 -->
    <div
      v-if="show && state === 'bubble'"
      class="tts-bubble"
      :class="{ mini }"
      :style="{ background: theme.popup, color: theme.fontColor }"
    >
      <button class="tts-bubble-play" :aria-label="isPaused ? '继续' : '暂停'" @click="$emit('toggle-play')">
        <svg v-if="isPaused" viewBox="0 0 24 24" fill="currentColor" aria-hidden="true">
          <path d="M8 5v14l11-7z" />
        </svg>
        <svg v-else viewBox="0 0 24 24" fill="currentColor" aria-hidden="true">
          <path d="M6 5h4v14H6zM14 5h4v14h-4z" />
        </svg>
      </button>
      <!-- 圆形态仍要能关掉: 否则只能先展开再关, 等于绕两层。 -->
      <button class="tts-bubble-close" aria-label="关闭听书并停止朗读" @click="$emit('close')">
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.4" aria-hidden="true">
          <path d="M18 6 6 18M6 6l12 12" />
        </svg>
      </button>
      <!--
        迷你窗口只有 320×240, 展开出去的大面板会占满整窗、且形态由 ReaderView
        强制成气泡(点展开也展不开), 所以索性不渲染这个角标 —— 留个点了没反应的
        按钮比没有更糟。关闭角标保留, 否则迷你模式下关不掉听书。
      -->
      <button v-if="!mini" class="tts-bubble-expand" aria-label="展开听书面板" @click="setState('bar')">
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" aria-hidden="true">
          <path d="m6 15 6-6 6 6" />
        </svg>
      </button>
    </div>

    <div
      v-else-if="show"
      class="tts-controls"
      :class="{ collapsed: state === 'bar' }"
      :style="{ background: theme.popup, color: theme.fontColor }"
    >
      <!-- 一行态: 播放控制常驻, 但不挡掉正文一大块。 -->
      <div v-if="state === 'bar'" class="tts-collapsed-row">
        <button class="tts-icon-btn" :aria-label="isPaused ? '继续' : '暂停'" @click="$emit('toggle-play')">
          <svg v-if="isPaused" viewBox="0 0 24 24" fill="currentColor" aria-hidden="true">
            <path d="M8 5v14l11-7z" />
          </svg>
          <svg v-else viewBox="0 0 24 24" fill="currentColor" aria-hidden="true">
            <path d="M6 5h4v14H6zM14 5h4v14h-4z" />
          </svg>
        </button>
        <div class="tts-collapsed-info">
          <span class="tts-collapsed-title">{{ chapterTitle }}</span>
          <span class="tts-collapsed-mode">{{ providerLabel }}<span v-if="isLoading"> · 加载中</span></span>
        </div>
        <button class="tts-icon-btn" aria-label="上一段" @click="$emit('prev')">
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" aria-hidden="true">
            <path d="M19 20 9 12l10-8zM5 19V5" />
          </svg>
        </button>
        <button class="tts-icon-btn" aria-label="下一段" @click="$emit('next')">
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" aria-hidden="true">
            <path d="m5 4 10 8-10 8zM19 5v14" />
          </svg>
        </button>
        <button class="tts-icon-btn" aria-label="展开听书设置" @click="setState('expanded')">
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" aria-hidden="true">
            <path d="m6 15 6-6 6 6" />
          </svg>
        </button>
        <button class="tts-icon-btn" aria-label="收成圆形气泡" @click="setState('bubble')">
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" aria-hidden="true">
            <circle cx="12" cy="12" r="7" />
          </svg>
        </button>
        <button class="tts-icon-btn tts-icon-danger" aria-label="关闭听书并停止朗读" @click="$emit('close')">
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" aria-hidden="true">
            <path d="M18 6 6 18M6 6l12 12" />
          </svg>
        </button>
      </div>

      <template v-else>
        <div class="tts-head">
          <div class="tts-info">
            <div>正在朗读: {{ chapterTitle }}</div>
            <div class="tts-mode">
              当前模式: {{ providerLabel }}
              <span v-if="provider === 'openai'"> · {{ openaiModel }} / {{ openaiVoice }}</span>
            </div>
          </div>
          <div class="tts-head-actions">
            <button class="tts-close" @click="setState('bar')" aria-label="收起为一行">
              <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                <path d="m6 9 6 6 6-6" />
              </svg>
            </button>
            <button class="tts-close tts-icon-danger" @click="$emit('close')" aria-label="关闭听书并停止朗读">
              <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2">
                <path d="M18 6 6 18M6 6l12 12" />
              </svg>
            </button>
          </div>
        </div>
        <div class="tts-btns">
          <button @click="$emit('prev')">上一段</button>
          <button :disabled="isLoading" @click="$emit('toggle-play')">{{ isLoading ? '加载中' : (isPaused ? '恢复' : (isSpeaking ? '暂停' : '开始')) }}</button>
          <button @click="$emit('stop')">停止</button>
          <button @click="$emit('next')">下一段</button>
        </div>
        <div v-if="isSpeaking || isLoading || isPaused" class="tts-progress" role="progressbar" :aria-valuenow="progressPercent" aria-valuemin="0" aria-valuemax="100">
          <div class="tts-progress-fill" :style="{ width: `${progressPercent}%` }"></div>
        </div>
        <select v-if="provider === 'system'" class="tts-voice-select" :value="voiceName" @change="$emit('voice-change', ($event.target as HTMLSelectElement).value)">
          <option value="">系统默认</option>
          <option v-for="voice in voices" :key="voice.name" :value="voice.name">
            {{ voice.name }} ({{ voice.lang }})
          </option>
        </select>
        <input
          v-if="provider === 'openai'"
          class="tts-voice-select"
          type="text"
          :value="openaiVoice"
          placeholder="alloy"
          @input="$emit('openai-voice-change', ($event.target as HTMLInputElement).value)"
        >
        <div class="tts-tuning">
          <div class="tts-stepper">
            <span class="tts-label">语速</span>
            <button @click="$emit('rate-change', -0.1)">-</button>
            <span>{{ rate.toFixed(1) }}</span>
            <button @click="$emit('rate-change', 0.1)">+</button>
          </div>
          <div v-if="supportsPitch" class="tts-stepper">
            <span class="tts-label">语调</span>
            <button @click="$emit('pitch-change', -0.1)">-</button>
            <span>{{ pitch.toFixed(1) }}</span>
            <button @click="$emit('pitch-change', 0.1)">+</button>
          </div>
        </div>
        <div class="tts-timer-row">
          <span class="tts-label">定时停止</span>
          <div class="tts-timer-actions">
            <button :class="{ active: stopAfterMinutes === 0 }" @click="$emit('timer-change', 0)">关闭</button>
            <button :class="{ active: stopAfterMinutes === 15 }" @click="$emit('timer-change', 15)">15分钟</button>
            <button :class="{ active: stopAfterMinutes === 30 }" @click="$emit('timer-change', 30)">30分钟</button>
            <button :class="{ active: stopAfterMinutes === 60 }" @click="$emit('timer-change', 60)">60分钟</button>
          </div>
          <div v-if="timerText" class="tts-timer-text">{{ timerText }}</div>
        </div>
      </template>
    </div>
  </Transition>
</template>

<script setup lang="ts">
import { computed } from 'vue'
import type { ThemePreset } from '../../stores/reader'

/** 听书面板的三种形态, 由宽到窄: 展开设置 → 单行控制条 → 圆形气泡。 */
type TtsPanelState = 'expanded' | 'bar' | 'bubble'

const props = withDefaults(defineProps<{
  show: boolean
  theme: ThemePreset | { popup: string; fontColor: string }
  chapterTitle?: string
  /** http / edge / msdemo 与 openai 一样是「整段音频」形态，面板只按此区分是否显示 API 语音信息。 */
  provider: 'system' | 'openai' | 'http' | 'edge' | 'msdemo'
  providerLabel: string
  isSpeaking: boolean
  isLoading: boolean
  isPaused: boolean
  progress: number
  voices: SpeechSynthesisVoice[]
  voiceName: string
  rate: number
  pitch: number
  supportsPitch: boolean
  openaiModel: string
  openaiVoice: string
  stopAfterMinutes: number
  timerText: string
  /** 当前形态。默认 'bar': 展开态会挡住约三分之一的正文。 */
  state: TtsPanelState
  /**
   * 迷你窗口(最小 240×180)里不摆得下面板: 调用方会把 state 压成 'bubble',
   * 这里再收一圈尺寸。默认为 false, 宽屏与手机端不受影响。
   */
  mini?: boolean
}>(), {
  mini: false,
})

const progressPercent = computed(() => Math.round(Math.max(0, Math.min(1, props.progress)) * 100))

const emit = defineEmits<{
  close: []
  prev: []
  'toggle-play': []
  stop: []
  next: []
  'voice-change': [value: string]
  'openai-voice-change': [value: string]
  'rate-change': [delta: number]
  'pitch-change': [delta: number]
  'timer-change': [minutes: number]
  'update:state': [value: TtsPanelState]
}>()

/** 切换形态只改显示, 不影响朗读本身。 */
function setState(next: TtsPanelState) {
  emit('update:state', next)
}
</script>

<style scoped>
.tts-controls {
  position: fixed;
  bottom: calc(24px + var(--safe-area-bottom));
  left: 50%;
  transform: translateX(-50%);
  width: min(720px, calc(100vw - 24px));
  max-width: calc(100vw - 24px);
  padding: 16px 18px;
  border-radius: 28px;
  box-shadow: 0 4px 20px rgba(0, 0, 0, 0.2);
  display: flex;
  flex-direction: column;
  align-items: stretch;
  gap: 12px;
  z-index: 30;
  box-sizing: border-box;
}

/* 收起态: 一行的高度, 常态就是它。展开才占大块屏幕。 */
.tts-controls.collapsed {
  padding: 8px 12px;
  border-radius: 999px;
  gap: 0;
}

/* 圆形气泡: 最小形态。只占一个圆, 正文几乎不受影响。 */
.tts-bubble {
  position: fixed;
  right: calc(20px + var(--safe-area-right, 0px));
  bottom: calc(28px + var(--safe-area-bottom));
  width: 56px;
  height: 56px;
  border-radius: 50%;
  display: flex;
  align-items: center;
  justify-content: center;
  box-shadow: 0 4px 16px rgba(0, 0, 0, 0.22);
  z-index: 30;
  box-sizing: border-box;
}

.tts-bubble-play {
  width: 100%;
  height: 100%;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  padding: 0;
  border: none;
  border-radius: 50%;
  background: transparent;
  color: inherit;
  cursor: pointer;
}

.tts-bubble-play svg {
  width: 24px;
  height: 24px;
}

/* 关闭与展开做成角标: 圆本身只留给播放, 不牺牲点击面积。 */
.tts-bubble-close,
.tts-bubble-expand {
  position: absolute;
  width: 22px;
  height: 22px;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  padding: 0;
  border: none;
  border-radius: 50%;
  background: color-mix(in srgb, currentColor 12%, transparent);
  color: inherit;
  cursor: pointer;
  opacity: 0;
  transition: opacity 0.15s;
}

.tts-bubble-close svg,
.tts-bubble-expand svg {
  width: 13px;
  height: 13px;
}

/* 触摸端没有 hover: 角标常驻, 否则关不掉也展不开。 */
@media (hover: none) {
  .tts-bubble-close,
  .tts-bubble-expand {
    opacity: 1;
  }
}

.tts-bubble:hover .tts-bubble-close,
.tts-bubble:hover .tts-bubble-expand,
.tts-bubble-close:focus-visible,
.tts-bubble-expand:focus-visible {
  opacity: 1;
}

.tts-bubble-close {
  top: -4px;
  right: -4px;
}

.tts-bubble-expand {
  bottom: -4px;
  right: -4px;
}

/* 迷你窗口: 气泡整体收一圈, 免得 40px 的圆在 320×240 里也显眼。
 * 桌角留白同时从 20/28 收到 8 —— 迷你窗的标题栏之外几乎没有可让的空间。 */
.tts-bubble.mini {
  width: 40px;
  height: 40px;
  right: calc(8px + var(--safe-area-right, 0px));
  bottom: calc(8px + var(--safe-area-bottom));
  box-shadow: 0 2px 8px rgba(0, 0, 0, 0.18);
}

.tts-bubble.mini .tts-bubble-play svg {
  width: 20px;
  height: 20px;
}

/* 角标跟着缩: 22px 的圆挂在 40px 的气泡上会盖掉小半个播放键。 */
.tts-bubble.mini .tts-bubble-close {
  width: 16px;
  height: 16px;
  top: -3px;
  right: -3px;
}

.tts-bubble.mini .tts-bubble-close svg {
  width: 10px;
  height: 10px;
}

.tts-icon-danger {
  color: #d9534f;
}

.tts-collapsed-row {
  display: flex;
  align-items: center;
  gap: 6px;
  min-width: 0;
}

.tts-collapsed-info {
  flex: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
  gap: 1px;
  padding: 0 4px;
}

.tts-collapsed-title {
  font-size: 13px;
  line-height: 1.3;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.tts-collapsed-mode {
  font-size: 11px;
  line-height: 1.2;
  opacity: 0.6;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.tts-icon-btn {
  width: 36px;
  height: 36px;
  flex-shrink: 0;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  padding: 0;
  border: none;
  border-radius: 50%;
  background: transparent;
  color: inherit;
  cursor: pointer;
}

.tts-icon-btn:hover {
  background: color-mix(in srgb, currentColor 10%, transparent);
}

.tts-icon-btn svg {
  width: 20px;
  height: 20px;
}

.tts-head-actions {
  display: flex;
  align-items: center;
  gap: 6px;
  flex-shrink: 0;
}

.tts-head {
  width: 100%;
  display: flex;
  align-items: flex-start;
  justify-content: space-between;
  gap: 12px;
}

.tts-info {
  flex: 1;
  min-width: 0;
  font-size: 14px;
  line-height: 1.5;
  opacity: 0.7;
  word-break: break-word;
}

.tts-mode {
  margin-top: 4px;
  font-size: 12px;
  opacity: 0.6;
}

.tts-close {
  width: 40px;
  height: 40px;
  flex-shrink: 0;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  border: none;
  border-radius: 12px;
  background: rgba(0, 0, 0, 0.05);
  color: inherit;
  cursor: pointer;
}

.tts-close svg {
  width: 20px;
  height: 20px;
}

.tts-btns {
  display: flex;
  flex-wrap: wrap;
  gap: 12px;
  justify-content: center;
}

.tts-btns button {
  /* 同 ReaderSearchPanel: 淡主色底 + currentColor 文字, 跟随阅读主题。 */
  background: color-mix(in srgb, var(--color-primary) 14%, transparent);
  color: currentColor;
  border: 1px solid color-mix(in srgb, var(--color-primary) 34%, transparent);
  padding: 10px 18px;
  border-radius: 10px;
  cursor: pointer;
  min-width: 0;
}

.tts-btns button:hover {
  background: color-mix(in srgb, var(--color-primary) 20%, transparent);
}

/* 模板里按钮有 :disabled="isLoading", 但没有对应用户可见的状态变化。 */
.tts-btns button:disabled {
  opacity: 0.45;
  cursor: not-allowed;
}

.tts-progress {
  width: 100%;
  height: 4px;
  overflow: hidden;
  border-radius: 2px;
  background: rgba(127, 127, 127, 0.24);
}

.tts-progress-fill {
  height: 100%;
  background: #c97f3a;
  transition: width 160ms linear;
}

.tts-voice-select {
  width: 100%;
  min-width: 0;
  padding: 10px 12px;
  border-radius: 12px;
  border: 1px solid rgba(0, 0, 0, 0.08);
  background: rgba(255, 255, 255, 0.65);
  color: inherit;
}

.tts-tuning {
  width: 100%;
  display: flex;
  gap: 10px;
  min-width: 0;
}

.tts-stepper {
  flex: 1;
  min-width: 0;
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
  padding: 10px 12px;
  border-radius: 12px;
  background: rgba(0, 0, 0, 0.04);
  font-size: 14px;
}

.tts-stepper button {
  width: 36px;
  height: 36px;
  border: 1px solid color-mix(in srgb, var(--color-primary) 34%, transparent);
  border-radius: 10px;
  background: color-mix(in srgb, var(--color-primary) 14%, transparent);
  color: currentColor;
  cursor: pointer;
}

.tts-stepper button:hover {
  background: color-mix(in srgb, var(--color-primary) 20%, transparent);
}

.tts-label {
  opacity: 0.7;
}

.tts-timer-row {
  width: 100%;
  display: flex;
  flex-direction: column;
  gap: 8px;
}

.tts-timer-actions {
  display: flex;
  flex-wrap: wrap;
  gap: 8px;
}

.tts-timer-actions button {
  border: 1px solid rgba(0, 0, 0, 0.08);
  border-radius: 999px;
  background: rgba(0, 0, 0, 0.04);
  color: inherit;
  padding: 8px 12px;
  font-size: 13px;
  cursor: pointer;
}

.tts-timer-actions button.active {
  background: color-mix(in srgb, var(--color-primary) 14%, transparent);
  border-color: color-mix(in srgb, var(--color-primary) 34%, transparent);
  /* 同 ReaderTtsPanel 其余按钮: 文字跟随阅读主题, 不能用主色(浅色阅读主题下
   * 主色与底色亮度接近, 只有约 2.3:1)。 */
  color: currentColor;
  font-weight: 600;
}

.tts-timer-text {
  font-size: 12px;
  opacity: 0.65;
}

@media (max-width: 768px) {
  .tts-controls {
    width: calc(100vw - 16px);
    max-width: calc(100vw - 16px);
    bottom: calc(80px + var(--safe-area-bottom));
    padding: 14px 14px 16px;
    border-radius: 24px;
  }

  .tts-btns {
    display: grid;
    grid-template-columns: repeat(4, minmax(0, 1fr));
    gap: 8px;
  }

  .tts-btns button {
    width: 100%;
    padding: 9px 0;
    min-height: 40px;
    border-radius: 10px;
    font-size: 14px;
    white-space: nowrap;
  }

  .tts-tuning {
    display: grid;
    grid-template-columns: repeat(2, minmax(0, 1fr));
    gap: 8px;
  }

  .tts-stepper {
    padding: 8px 10px;
    gap: 6px;
    font-size: 13px;
  }

  .tts-stepper button {
    width: 32px;
    height: 32px;
    border-radius: 9px;
    font-size: 18px;
  }

  .tts-stepper:only-child {
    grid-column: 1 / -1;
  }

  .tts-label {
    font-size: 13px;
  }
}
</style>
