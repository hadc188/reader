<template>
  <div class="app-shell">
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
    <CloseChoiceDialog />

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
import { computed, onMounted, onBeforeUnmount } from 'vue'
import { useRoute } from 'vue-router'
import { listen } from '@tauri-apps/api/event'
import { useAppStore } from './stores/app'
import AppTopBar from './components/AppTopBar.vue'
import AppBottomNav from './components/AppBottomNav.vue'
import SettingsDrawer from './components/SettingsDrawer.vue'
import SourceManager from './components/SourceManager.vue'
import WebdavManager from './components/WebdavManager.vue'
import ConfirmDialog from './components/ConfirmDialog.vue'
import CloseChoiceDialog from './components/CloseChoiceDialog.vue'
import TitleBar from './components/TitleBar.vue'

const route = useRoute()
const appStore = useAppStore()

const showHeader = computed(() => route.name !== 'reader')
const showBottomNav = computed(() => route.name !== 'reader')

onMounted(() => {
  appStore.fetchUserInfo()
})

// When the window close button is pressed (title bar close button, Alt+F4,
// taskbar close, etc.), the Rust side prevents the close and emits this event
// so the user can pick hide-to-tray or quit.
let closeUnlisten: (() => void) | undefined
onMounted(() => {
  if (typeof window === 'undefined' || !('__TAURI_INTERNALS__' in window)) return
  listen<unknown>('close-requested', () => {
    void appStore.askCloseChoice().then((choice) => {
      if (choice === 'quit') {
        // destroy() exits without re-triggering close-requested.
        void import('@tauri-apps/api/window').then(({ getCurrentWindow }) =>
          getCurrentWindow().destroy()
        )
      }
      // 'tray' → the CloseChoiceDialog already hid the window.
      // 'cancel' → the user dismissed the dialog; the window stays as-is.
    })
  }).then((un) => {
    closeUnlisten = un
  })
})

onBeforeUnmount(() => {
  closeUnlisten?.()
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
  padding-bottom: calc(76px + var(--safe-area-bottom));
}
</style>
