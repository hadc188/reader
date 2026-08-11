<template>
  <div
    class="app-shell"
    :class="{ 'has-custom-background': showCustomBackground }"
    :style="appShellStyle"
  >
    <div
      v-if="showCustomBackground"
      class="app-custom-background"
      aria-hidden="true"
      :style="customBackgroundStyle"
    ></div>
    <TitleBar />
    <div class="app-body">
      <AppTopBar v-if="showHeader" />
      <main class="app-main" :class="{ 'with-bottom-nav': showBottomNav, 'without-header': !showHeader }">
        <router-view />
      </main>
      <AppBottomNav v-if="showBottomNav" />
    </div>
    <SettingsDrawer v-model="appStore.showSettingsDrawer" />
    <SourceManager v-model="appStore.showSourceManager" />
    <WebdavManager v-model="appStore.showWebdavManager" />
    <ConfirmDialog />

    <!-- Toast notifications -->
    <div class="toast-container">
      <TransitionGroup name="slide-up">
        <div
          v-for="toast in appStore.toasts"
          :key="toast.id"
          class="toast"
          :class="toast.type"
        >
          {{ toast.message }}
        </div>
      </TransitionGroup>
    </div>
  </div>
</template>

<script setup lang="ts">
import { computed, onMounted, onBeforeUnmount, watch } from 'vue'
import { useRoute } from 'vue-router'
import { listen } from '@tauri-apps/api/event'
import { useAppStore } from './stores/app'
import { useReaderStore } from './stores/reader'
import AppTopBar from './components/AppTopBar.vue'
import AppBottomNav from './components/AppBottomNav.vue'
import SettingsDrawer from './components/SettingsDrawer.vue'
import SourceManager from './components/SourceManager.vue'
import WebdavManager from './components/WebdavManager.vue'
import ConfirmDialog from './components/ConfirmDialog.vue'
import TitleBar from './components/TitleBar.vue'
import { resolveWindowClose } from './utils/windowClose'

const route = useRoute()
const appStore = useAppStore()
const readerStore = useReaderStore()

const showHeader = computed(() => route.name !== 'reader')
const showBottomNav = computed(() => route.name !== 'reader')
const showCustomBackground = computed(() => {
  if (!readerStore.config.backgroundImage) return false
  return route.name === 'home'
    || (route.name === 'reader' && readerStore.config.applyBackgroundToReader)
})
const appShellStyle = computed(() => ({
  background: route.name === 'reader'
    ? readerStore.currentTheme.body
    : 'var(--color-bg)',
}))
const customBackgroundStyle = computed(() => ({
  backgroundImage: `url(${readerStore.config.backgroundImage})`,
  opacity: readerStore.config.backgroundOpacity,
}))

const stopBackgroundClassSync = watch(showCustomBackground, (active) => {
  if (typeof document === 'undefined') return
  document.body.classList.toggle('custom-background-active', active)
}, { immediate: true })

onMounted(() => {
  void appStore.checkVersionUpdate()
})

let closeUnlisten: (() => void) | undefined
let handlingCloseRequest = false
onMounted(() => {
  if (typeof window === 'undefined' || !('__TAURI_INTERNALS__' in window)) return
  listen<unknown>('close-requested', async () => {
    if (handlingCloseRequest) return
    handlingCloseRequest = true
    try {
      const { getCurrentWindow } = await import('@tauri-apps/api/window')
      await resolveWindowClose(getCurrentWindow(), appStore.closeToTray)
    } finally {
      handlingCloseRequest = false
    }
  }).then((un) => {
    closeUnlisten = un
  })
})

onBeforeUnmount(() => {
  closeUnlisten?.()
  stopBackgroundClassSync()
  if (typeof document !== 'undefined') {
    document.body.classList.remove('custom-background-active')
  }
})
</script>

<style>
html,
body {
  height: var(--app-height, 100dvh);
  overflow: hidden;
}

.app-shell {
  height: var(--app-height, 100dvh);
  overflow: hidden;
  display: flex;
  flex-direction: column;
  position: relative;
  isolation: isolate;
  transition: background var(--duration-normal);
}

.app-custom-background {
  position: absolute;
  inset: 0;
  z-index: 0;
  background-position: center;
  background-repeat: no-repeat;
  background-size: cover;
  pointer-events: none;
  transition: opacity var(--duration-normal);
}

.app-shell > .titlebar,
.app-shell > .app-body {
  position: relative;
  z-index: 1;
}

.app-shell.has-custom-background .app-topbar {
  background: color-mix(in srgb, var(--color-bg-elevated) 74%, transparent);
  border-bottom-color: color-mix(in srgb, var(--color-border) 72%, transparent);
  backdrop-filter: blur(20px) saturate(125%);
  -webkit-backdrop-filter: blur(20px) saturate(125%);
}

.app-shell.has-custom-background .app-topbar .search-box {
  background: color-mix(in srgb, var(--color-bg-elevated) 68%, transparent);
  border-color: color-mix(in srgb, var(--color-border) 70%, transparent);
  backdrop-filter: blur(14px) saturate(120%);
  -webkit-backdrop-filter: blur(14px) saturate(120%);
}

.app-shell.has-custom-background .app-topbar .search-box.focused {
  background: color-mix(in srgb, var(--color-bg-elevated) 84%, transparent);
}

.app-shell.has-custom-background .shelf-btn,
.app-shell.has-custom-background .book-card,
.app-shell.has-custom-background .batch-toolbar {
  background: color-mix(in srgb, var(--color-bg-elevated) 82%, transparent);
  border-color: color-mix(in srgb, var(--color-border) 76%, transparent);
}

.app-shell.has-custom-background .bottom-nav {
  background: color-mix(in srgb, var(--color-bg-elevated) 78%, transparent);
  border-color: color-mix(in srgb, var(--color-border) 74%, transparent);
}

.app-shell.has-custom-background .reader-search-panel,
.app-shell.has-custom-background .tts-controls {
  backdrop-filter: blur(20px) saturate(120%);
  -webkit-backdrop-filter: blur(20px) saturate(120%);
  border-color: color-mix(in srgb, currentColor 10%, transparent);
}

body.custom-background-active {
  --glass-window-bg: color-mix(in srgb, var(--color-bg-elevated) 82%, transparent);
  --glass-inner-bg: color-mix(in srgb, var(--color-bg-elevated) 58%, transparent);
  --glass-border-color: color-mix(in srgb, var(--color-border) 72%, transparent);
}

body.custom-background-active :is(
  .source-modal,
  .login-preview-modal,
  .subscription-panel,
  .webdav-modal,
  .detail-modal,
  .cache-modal,
  .confirm-dialog,
  .modal-card,
  .modal-content
) {
  background: var(--glass-window-bg) !important;
  border-color: var(--glass-border-color) !important;
  backdrop-filter: blur(24px) saturate(120%);
  -webkit-backdrop-filter: blur(24px) saturate(120%);
}

body.custom-background-active :is(
  .reader-drawer,
  .reader-search-panel,
  .tts-controls,
  .selection-menu
) {
  border-color: color-mix(in srgb, currentColor 10%, transparent) !important;
  backdrop-filter: blur(24px) saturate(120%);
  -webkit-backdrop-filter: blur(24px) saturate(120%);
}

body.custom-background-active .reader-drawer > :is(
  .reader-catalog,
  .read-settings,
  .reader-bookshelf,
  .reader-source,
  .rule-manager,
  .cache-manager
) {
  background: transparent !important;
}

body.custom-background-active .source-modal :is(
  .source-manager-header,
  .source-list-wrapper,
  .editor-panel,
  .modal-footer
) {
  background: transparent !important;
}

body.custom-background-active .source-modal :is(
  .editor-tabs,
  .overview-card,
  .login-card,
  .source-url-block
),
body.custom-background-active .subscription-panel :is(
  .subscription-item
),
body.custom-background-active .webdav-modal :is(
  .backup-location,
  .path-bar code,
  .action-btn:not(.primary),
  .mini-btn
),
body.custom-background-active .cache-modal .cache-toolbar,
body.custom-background-active .detail-modal :is(.tag, .action-btn:not(.primary)) {
  background: var(--glass-inner-bg) !important;
  border-color: var(--glass-border-color) !important;
}

.app-body {
  flex: 1;
  min-height: 0;
  display: flex;
  flex-direction: column;
  overflow: hidden;
}

.app-main {
  flex: 1;
  min-height: 0;
  overflow: hidden;
}

.app-main.without-header {
  height: auto;
  flex: 1;
  min-height: 0;
}

.app-main.with-bottom-nav {
  padding-bottom: calc(88px + var(--safe-area-bottom));
}
</style>
