<template>
  <div
    class="titlebar"
    :class="{
      'reader-titlebar': isReader,
      'has-custom-background': hasCustomBackground,
      'surface-open': surface !== null,
      'settings-surface-open': surface === 'settings',
      'reader-surface-open': surface === 'reader-panel',
    }"
    :style="titlebarStyle"
  >
    <div class="titlebar-drag" data-tauri-drag-region @dblclick="toggleMaximize">
      <span class="titlebar-title" data-tauri-drag-region>阅读</span>
    </div>
    <div class="titlebar-controls">
      <button class="tb-btn" title="最小化" @click="minimize">
        <svg viewBox="0 0 12 12" width="12" height="12"><path d="M2 6h8" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" /></svg>
      </button>
      <button class="tb-btn" :title="isMaximized ? '还原' : '最大化'" @click="toggleMaximize">
        <svg v-if="isMaximized" viewBox="0 0 12 12" width="12" height="12"><rect x="2.5" y="2.5" width="7" height="7" fill="none" stroke="currentColor" stroke-width="1.2" /><path d="M4.5 2.5V4M9.5 4.5H8M4.5 7.5H8" stroke="currentColor" stroke-width="1.2" /></svg>
        <svg v-else viewBox="0 0 12 12" width="12" height="12"><rect x="2.5" y="2.5" width="7" height="7" fill="none" stroke="currentColor" stroke-width="1.2" /></svg>
      </button>
      <button class="tb-btn close" title="关闭" @click="close">
        <svg viewBox="0 0 12 12" width="12" height="12"><path d="M3 3l6 6M9 3l-6 6" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" /></svg>
      </button>
    </div>
  </div>
</template>

<script setup lang="ts">
import { computed, onMounted, onBeforeUnmount, ref } from 'vue'
import { useRoute } from 'vue-router'
import { getCurrentWindow } from '@tauri-apps/api/window'
import { useReaderStore } from '../stores/reader'

const { surface = null } = defineProps<{
  surface?: 'settings' | 'reader-panel' | null
}>()

const readerStore = useReaderStore()
const route = useRoute()
const isTauri = typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window
const appWindow = isTauri ? getCurrentWindow() : null
const isMaximized = ref(false)
const isReader = computed(() => route.name === 'reader')
const hasCustomBackground = computed(() => Boolean(readerStore.config.backgroundImage) && (
  !isReader.value || readerStore.config.applyBackgroundToReader
))
const titlebarStyle = computed(() => {
  const surfaceBackground = surface === 'reader-panel'
    ? readerStore.currentTheme.popup
    : 'var(--color-bg-elevated)'

  if (isReader.value) {
    return {
      '--titlebar-surface-background': surfaceBackground,
      background: surface !== null
        ? surfaceBackground
        : hasCustomBackground.value
        ? `color-mix(in srgb, ${readerStore.currentTheme.body} 24%, transparent)`
        : readerStore.currentTheme.body,
      color: readerStore.currentTheme.fontColor,
    }
  }
  return {
    '--titlebar-surface-background': surfaceBackground,
    background: surface !== null
      ? surfaceBackground
      : hasCustomBackground.value
      ? 'color-mix(in srgb, var(--color-bg-elevated) 30%, transparent)'
      : 'var(--color-bg-elevated)',
    color: 'var(--color-text-secondary)',
  }
})

async function minimize() {
  await appWindow?.minimize()
}
async function toggleMaximize() {
  if (!appWindow) return
  const m = await appWindow.isMaximized()
  if (m) {
    await appWindow.unmaximize()
    isMaximized.value = false
  } else {
    await appWindow.maximize()
    isMaximized.value = true
  }
}
async function close() {
  await appWindow?.close()
}

onMounted(async () => {
  if (!appWindow) return
  isMaximized.value = await appWindow.isMaximized()
  const unlisten = await appWindow.onResized(() => {
    void appWindow.isMaximized().then((m) => { isMaximized.value = m })
  })
  cleanup = () => void unlisten()
})
let cleanup: () => void = () => undefined
onBeforeUnmount(() => cleanup())
</script>

<style scoped>
.titlebar {
  /* In normal flow: #app is a flex column, so this pushes .app-body below it
     instead of floating over the page (position:fixed overlapped the reader). */
  position: relative;
  flex-shrink: 0;
  height: var(--titlebar-height, 32px);
  box-sizing: border-box;
  display: flex;
  align-items: center;
  justify-content: space-between;
  z-index: calc(var(--z-modal) + 20);
  color: var(--color-text-secondary);
  background: var(--color-bg-elevated);
  border-bottom: 1px solid var(--color-divider);
  user-select: none;
  transition: background var(--duration-normal), color var(--duration-normal), border-color var(--duration-normal);
}

.titlebar.reader-titlebar {
  border-bottom-color: transparent;
}

.titlebar.has-custom-background {
  border-bottom-color: color-mix(in srgb, currentColor 10%, transparent);
  backdrop-filter: blur(4px) saturate(115%);
  -webkit-backdrop-filter: blur(4px) saturate(115%);
  text-shadow: 0 1px 3px rgba(0, 0, 0, 0.18);
}

.titlebar.surface-open {
  background: var(--titlebar-surface-background) !important;
  border-bottom-color: transparent;
  backdrop-filter: none !important;
  -webkit-backdrop-filter: none !important;
  text-shadow: none;
}

.titlebar.settings-surface-open {
  box-shadow: inset -420px -1px 0 color-mix(in srgb, currentColor 10%, transparent);
}

.titlebar.reader-surface-open {
  box-shadow: inset 340px -1px 0 color-mix(in srgb, currentColor 10%, transparent);
}

@media (max-width: 494px) {
  .titlebar.settings-surface-open {
    box-shadow: inset -94vw -1px 0 color-mix(in srgb, currentColor 10%, transparent);
  }
}

@media (max-width: 400px) {
  .titlebar.reader-surface-open {
    box-shadow: inset 85vw -1px 0 color-mix(in srgb, currentColor 10%, transparent);
  }
}

.titlebar-drag {
  flex: 1;
  height: 100%;
  display: flex;
  align-items: center;
  padding-left: 14px;
}

.titlebar-title {
  font-size: 11px;
  color: inherit;
  font-weight: 500;
  opacity: 0.52;
}

.titlebar-controls {
  display: flex;
  height: 100%;
}

.tb-btn {
  width: 44px;
  height: 100%;
  display: flex;
  align-items: center;
  justify-content: center;
  background: transparent;
  border: none;
  color: inherit;
  cursor: pointer;
  opacity: 0.66;
  transition: background var(--duration-fast), opacity var(--duration-fast);
}

.tb-btn:hover {
  background: var(--color-bg-hover);
  opacity: 1;
}

.reader-titlebar .tb-btn:hover {
  background: rgba(127, 127, 127, 0.14);
}

.tb-btn.close:hover {
  background: #e81123;
  color: #fff;
}

</style>
