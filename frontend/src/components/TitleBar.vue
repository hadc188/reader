<template>
  <div
    class="titlebar"
    :class="{ 'reader-titlebar': isReader }"
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

const readerStore = useReaderStore()
const route = useRoute()
const appWindow = getCurrentWindow()
const isMaximized = ref(false)
const isReader = computed(() => route.name === 'reader')
const titlebarStyle = computed(() => isReader.value
  ? {
      background: readerStore.currentTheme.body,
      color: readerStore.currentTheme.fontColor,
    }
  : {
      background: 'var(--color-bg-elevated)',
      color: 'var(--color-text-secondary)',
    })

async function minimize() {
  await appWindow.minimize()
}
async function toggleMaximize() {
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
  await appWindow.close()
}

onMounted(async () => {
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
  display: flex;
  align-items: center;
  justify-content: space-between;
  z-index: calc(var(--z-modal) + 20);
  color: var(--color-text-secondary);
  background: var(--color-bg-elevated);
  border-bottom: 1px solid var(--color-border-light);
  user-select: none;
  transition: background var(--duration-normal), color var(--duration-normal), border-color var(--duration-normal);
}

.titlebar.reader-titlebar {
  border-bottom-color: transparent;
}

.titlebar-drag {
  flex: 1;
  height: 100%;
  display: flex;
  align-items: center;
  padding-left: 12px;
}

.titlebar-title {
  font-size: 12px;
  color: inherit;
  font-weight: 600;
  opacity: 0.58;
}

.titlebar-controls {
  display: flex;
  height: 100%;
}

.tb-btn {
  width: 46px;
  height: 100%;
  display: flex;
  align-items: center;
  justify-content: center;
  background: transparent;
  border: none;
  color: inherit;
  cursor: pointer;
  opacity: 0.72;
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

@media (max-width: 640px) {
  .titlebar {
    height: 40px;
  }
}
</style>
