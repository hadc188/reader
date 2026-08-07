<template>
  <Transition name="slide-up">
    <div v-if="show" class="tts-controls" :style="{ background: theme.popup, color: theme.fontColor }">
      <div class="tts-head">
        <div class="tts-info">
          <div>正在朗读: {{ chapterTitle }}</div>
          <div class="tts-mode">
            当前模式: {{ providerLabel }}
            <span v-if="provider === 'openai'"> · {{ openaiSource === 'server' ? '后端配置' : `${openaiModel} / ${openaiVoice}` }}</span>
          </div>
        </div>
        <button class="tts-close" @click="$emit('close')" aria-label="close tts panel">
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
            <path d="M18 6 6 18M6 6l12 12" />
          </svg>
        </button>
      </div>
      <div class="tts-btns">
        <button @click="$emit('prev')">上一段</button>
        <button :disabled="isLoading" @click="$emit('toggle-play')">{{ isLoading ? '加载中' : (!isSpeaking ? '开始' : (isPaused ? '恢复' : '暂停')) }}</button>
        <button @click="$emit('stop')">停止</button>
        <button @click="$emit('next')">下一段</button>
      </div>
      <select v-if="provider === 'system'" class="tts-voice-select" :value="voiceName" @change="$emit('voice-change', ($event.target as HTMLSelectElement).value)">
        <option value="">系统默认</option>
        <option v-for="voice in voices" :key="voice.name" :value="voice.name">
          {{ voice.name }} ({{ voice.lang }})
        </option>
      </select>
      <div v-else-if="openaiSource === 'server'" class="tts-source-note">
        OpenAI Speech 使用后端配置
      </div>
      <input
        v-else
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
    </div>
  </Transition>
</template>

<script setup lang="ts">
import type { ThemePreset } from '../../stores/reader'

defineProps<{
  show: boolean
  theme: ThemePreset | { popup: string; fontColor: string }
  chapterTitle?: string
  provider: 'system' | 'openai'
  providerLabel: string
  isSpeaking: boolean
  isLoading: boolean
  isPaused: boolean
  voices: SpeechSynthesisVoice[]
  voiceName: string
  rate: number
  pitch: number
  supportsPitch: boolean
  openaiModel: string
  openaiVoice: string
  openaiSource: 'browser' | 'server'
  stopAfterMinutes: number
  timerText: string
}>()

defineEmits<{
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
}>()
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
  background: var(--color-primary);
  color: white;
  border: none;
  padding: 10px 18px;
  border-radius: 10px;
  cursor: pointer;
  min-width: 0;
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

.tts-source-note {
  width: 100%;
  min-width: 0;
  padding: 10px 12px;
  border-radius: 12px;
  background: rgba(0, 0, 0, 0.04);
  color: inherit;
  font-size: 13px;
  opacity: 0.75;
  box-sizing: border-box;
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
  border: none;
  border-radius: 10px;
  background: var(--color-primary);
  color: #fff;
  cursor: pointer;
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
  background: var(--color-primary);
  border-color: var(--color-primary);
  color: #fff;
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
