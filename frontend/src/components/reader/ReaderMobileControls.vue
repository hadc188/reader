<template>
  <div
    class="mobile-controls"
    :style="{ '--popup-bg': theme.popup, '--font-color': theme.fontColor }"
    @click.stop
    @touchstart.stop
    @touchmove.stop
    @touchend.stop
  >
    <!-- Top Bar -->
    <Transition name="slide-down">
      <div v-show="show" class="m-top-bar">
        <div class="m-top-item" @click="$emit('goHome')">
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="m15 18-6-6 6-6" /></svg>
          <span>首页</span>
        </div>
        <div class="m-top-item" :class="{ active: store.activePanel === 'bookshelf' }" @click="store.togglePanel('bookshelf')">
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M4 19.5v-15A2.5 2.5 0 0 1 6.5 2H20v20H6.5a2.5 2.5 0 0 1 0-5H20" /></svg>
          <span>书架</span>
        </div>
        <div class="m-top-item" :class="{ active: store.activePanel === 'source' }" @click="store.togglePanel('source')">
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><rect width="7" height="7" x="3" y="3" rx="1" /><rect width="7" height="7" x="14" y="3" rx="1" /><rect width="7" height="7" x="3" y="14" rx="1" /><rect width="7" height="7" x="14" y="14" rx="1" /></svg>
          <span>书源</span>
        </div>
        <div class="m-top-item" :class="{ active: store.activePanel === 'catalog' }" @click="store.togglePanel('catalog')">
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M3 12h18M3 6h18M3 18h18" /></svg>
          <span>目录</span>
        </div>
        <div class="m-top-item" :class="{ active: store.activePanel === 'settings' }" @click="store.togglePanel('settings')">
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M12.22 2h-.44a2 2 0 0 0-2 2v.18a2 2 0 0 1-1 1.73l-.43.25a2 2 0 0 1-2 0l-.15-.08a2 2 0 0 0-2.73.73l-.22.38a2 2 0 0 0 .73 2.73l.15.1a2 2 0 0 1 1 1.72v.51a2 2 0 0 1-1 1.74l-.15.09a2 2 0 0 0-.73 2.73l.22.38a2 2 0 0 0 2.73.73l.15-.08a2 2 0 0 1 2 0l.43.25a2 2 0 0 1 1 1.73V20a2 2 0 0 0 2 2h.44a2 2 0 0 0 2-2v-.18a2 2 0 0 1 1-1.73l.43-.25a2 2 0 0 1 2 0l.15.08a2 2 0 0 0 2.73-.73l.22-.39a2 2 0 0 0-.73-2.73l-.15-.08a2 2 0 0 1-1-1.74v-.5a2 2 0 0 1 1-1.74l.15-.09a2 2 0 0 0 .73-2.73l-.22-.38a2 2 0 0 0-2.73-.73l-.15.08a2 2 0 0 1-2 0l-.43-.25a2 2 0 0 1-1-1.73V4a2 2 0 0 0-2-2z" /><circle cx="12" cy="12" r="3" /></svg>
          <span>设置</span>
        </div>
      </div>
    </Transition>

    <!-- Bottom Bar -->
    <Transition name="slide-up">
      <div v-show="show" class="m-bottom-bar">
        <div class="progress-row" @click="$emit('progress')">
          <div class="progress-track">
            <!-- Mock slider for now -->
            <div class="progress-fill" :style="{ width: store.readingProgress }"></div>
            <div class="progress-thumb" :style="{ left: store.readingProgress }"></div>
          </div>
          <span class="page-text">第 1/1 页</span>
        </div>
        <div class="nav-row">
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
      </div>
    </Transition>

    <!-- Left Floating -->
    <Transition name="fade">
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
        <button class="m-btn" @click="$emit('ai')" title="AI资料" aria-label="AI资料">
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
            <path d="M6 3h8l4 4v14H6z" />
            <path d="M14 3v5h5" />
            <path d="m8 16 2-5 2 5" />
            <path d="M8.7 14h2.6" />
            <path d="M15 11v5" />
          </svg>
        </button>
        <button class="m-btn" @click="$emit('scrollTop')">
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M12 19V5M5 12l7-7 7 7" /></svg>
        </button>
        <button class="m-btn" @click="$emit('scrollBottom')">
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M12 5v14M19 12l-7 7-7-7" /></svg>
        </button>
      </div>
    </Transition>

    <!-- Right Floating -->
    <Transition name="fade">
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
import { computed } from 'vue'
import { useReaderStore } from '../../stores/reader'
import { useAppStore } from '../../stores/app'

const store = useReaderStore()
const appStore = useAppStore()
const theme = computed(() => {
  if (store.isNight || appStore.theme === 'dark') {
    return {
      ...store.currentTheme,
      popup: 'var(--color-bg-elevated)',
      fontColor: 'var(--color-text)',
    }
  }
  return store.currentTheme
})

defineProps<{ 
  show: boolean
  isSpeaking?: boolean
  isPaused?: boolean
}>()

defineEmits<{
  goHome: []
  scrollTop: []
  scrollBottom: []
  prev: []
  next: []
  bookmark: []
  search: []
  info: []
  ai: []
  tts: []
  progress: []
}>()
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
