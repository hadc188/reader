<template>
  <div
    class="app-shell"
    :class="{
      'has-custom-background': showCustomBackground,
      'mini-mode': appStore.miniMode,
    }"
    :style="appShellStyle"
  >
    <div
      v-if="showCustomBackground"
      class="app-custom-background"
      aria-hidden="true"
      :style="customBackgroundStyle"
    ></div>
    <!-- 标题栏只在桌面端渲染: 它整套是为无边框窗口服务的(拖动区、最小化/
         最大化/关闭、迷你模式)。浏览器里没有窗口可控制, 留着只会显示三个
         点了没反应的按钮, 还白占 30px 顶部空间。 -->
    <TitleBar v-if="!appStore.isWebMode" :surface="titlebarSurface" />
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
    <UpdateProgressDialog />
    <ContextMenu />

    <!-- Toast notifications -->
    <div class="toast-container">
      <TransitionGroup name="slide-up">
        <div
          v-for="toast in appStore.toasts"
          :key="toast.id"
          class="toast"
          :class="toast.type"
          role="status"
          title="点击关闭"
          @click="appStore.dismissToast(toast.id)"
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
import UpdateProgressDialog from './components/UpdateProgressDialog.vue'
import ContextMenu from './components/ContextMenu.vue'
import TitleBar from './components/TitleBar.vue'
import { resolveWindowClose } from './utils/windowClose'
import { resolveShellBackgroundImage } from './utils/readerColor'
import { resetFullscreenIntent, toggleAppFullscreen } from './composables/useAppFullscreen'
import { resolveHotkeyAction, type HotkeyActionId } from './utils/readerHotkeys'

const route = useRoute()
const appStore = useAppStore()
const readerStore = useReaderStore()

const showHeader = computed(() => route.name !== 'reader' && !appStore.miniMode)
/* Web 端不渲染底部导航: 只剩书架和最近阅读两项, 用顶部的书架/最近入口
   就够切换, 底部常驻一条导航反而占掉手机的阅读高度。 */
const showBottomNav = computed(
  () => route.name !== 'reader' && !appStore.miniMode && !appStore.isWebMode,
)
const titlebarSurface = computed<'settings' | 'reader-panel' | null>(() => {
  if (appStore.showSettingsDrawer) return 'settings'
  if (appStore.showSourceManager || appStore.showWebdavManager) return 'settings'
  if (route.name === 'reader' && readerStore.activePanel) return 'reader-panel'
  return null
})
const showCustomBackground = computed(() => Boolean(shellBackgroundImage.value))
const shellBackgroundImage = computed(() => {
  const onReader = route.name === 'reader'
  // 不按页面清空参数: 由 resolveShellBackgroundImage 自己按 onReader 分流。
  // 早先在这里把阅读页专属参数置空, 配合函数内部的阅读页优先级, 会让
  // applyBackgroundToReader=false 时连桌面背景都判成空。
  return resolveShellBackgroundImage({
    customBackgroundColor: readerStore.readerBackgroundColor,
    readerBackgroundImage: readerStore.config.readerBackgroundImage,
    backgroundImage: readerStore.config.backgroundImage,
    applyBackgroundToReader: readerStore.config.applyBackgroundToReader,
  }, onReader)
})
const appShellStyle = computed(() => ({
  background: route.name === 'reader'
    ? readerStore.readerTheme.body
    : 'var(--color-bg)',
}))
const customBackgroundStyle = computed(() => {
  // 透明度要跟着**实际铺出去的那张图**走: 铺的是阅读页专属图就用它的透明度,
  // 否则用桌面图自己的。早先只看 readerBackgroundImage 是否存在, 于是在桌面页
  // (铺的是桌面图)也会误用阅读页专属图的透明度。
  const usingReaderImage = route.name === 'reader'
    && Boolean(readerStore.config.readerBackgroundImage)
  return {
    // 图铺在外壳最底层(标题栏也在其中), 因此标题栏必须透明才能透出背景。
    backgroundImage: shellBackgroundImage.value ? `url("${shellBackgroundImage.value}")` : undefined,
    opacity: usingReaderImage
      ? readerStore.config.readerImageOpacity
      : readerStore.config.backgroundOpacity,
  }
})

const stopBackgroundClassSync = watch(showCustomBackground, (active) => {
  if (typeof document === 'undefined') return
  document.body.classList.toggle('custom-background-active', active)
}, { immediate: true })

onMounted(() => {
  void appStore.runStartupVersionCheck()
  void appStore.applyBossKey().catch(() => undefined)
  void appStore.applyNetworkProxy().catch(() => undefined)
  // 迷你模式状态由 Rust 侧持有(桌面壳启动时据此建窗), 这里同步一份用于紧凑布局。
  void appStore.syncWindowMode()
})

let closeUnlisten: (() => void) | undefined
let legadoUnlisten: (() => void) | undefined
let handlingCloseRequest = false

/**
 * 处理 `legado://` 一键导入。
 *
 * 冷启动（网页按钮直接拉起应用）时 URL 在启动参数里，桌面壳先暂存，这里挂载后
 * 主动来取；应用已在运行时被再次唤起，则由 shell 发 `legado-deeplink` 事件通知。
 * 两种情况都走同一个"取暂存 → 导入"流程。
 */
async function consumePendingLegadoDeepLink() {
  const { invokeEnvelope } = await import('./api/invoke')
  const url = await invokeEnvelope<string | null>('take_pending_legado_deeplink', {})
  if (!url) return
  const { importFromLegadoDeepLink } = await import('./utils/legadoImport')
  try {
    const result = await importFromLegadoDeepLink(url)
    appStore.showToast(result.message, result.type)
  } catch (error) {
    appStore.showToast((error as Error).message || '一键导入失败', 'error')
  }
}

onMounted(() => {
  if (typeof window === 'undefined' || !('__TAURI_INTERNALS__' in window)) return
  void consumePendingLegadoDeepLink()
  listen('legado-deeplink', () => {
    void consumePendingLegadoDeepLink()
  }).then((un) => {
    legadoUnlisten = un
  })
})

/** 关闭窗口前的收尾: 把阅读位置落到书架。
 *
 *  legado 在 `ReadBookActivity.onPause` 里必然执行 `ReadBook.saveRead()`, 关闭/切走
 *  的那一刻位置就已写库。本项目的等价时机只有这里: `destroy()` 会立即销毁
 *  WebView, `beforeunload` 与 Vue 的 `onUnmounted` 都不保证执行, 异步 IPC 也会被
 *  终止。所以必须在销毁窗口**之前**主动 flush 一次。
 *
 *  阅读页用 keepalive 版本(一次 IPC 往返即返回, 不等 WebDAV 上传); 其它页面用
 *  普通版本 —— 阅读器状态在离开阅读页后仍保留在 store 里, 从这里补一次能救回
 *  "边界情况下路由离开钩子没跑完"的进度。两者都受 `resolveWindowClose` 的
 *  1.5s 上限约束, 不会把关窗拖成卡顿。 */
async function persistBeforeClose() {
  if (route.name === 'reader') {
    await readerStore.flushProgressToServerKeepalive(true)
    return
  }
  await readerStore.flushProgressToServer(true)
}

onMounted(() => {
  if (typeof window === 'undefined' || !('__TAURI_INTERNALS__' in window)) return
  listen<unknown>('close-requested', async () => {
    if (handlingCloseRequest) return
    handlingCloseRequest = true
    try {
      const { getCurrentWindow } = await import('@tauri-apps/api/window')
      const { invoke } = await import('@tauri-apps/api/core')
      await resolveWindowClose(
        getCurrentWindow(),
        appStore.closeToTray,
        persistBeforeClose,
        // 回执让外壳的关窗看门狗退让。没有它, 选择"关闭到托盘"后从托盘重新
        // 打开窗口会撞上看门狗的宽限期, 窗口被当作"没人响应"直接销毁。
        () => invoke('acknowledge_close_request'),
      )
    } finally {
      handlingCloseRequest = false
    }
  }).then((un) => {
    closeUnlisten = un
  })
})

/**
 * 全局 F11 全屏兜底。
 *
 * 全屏是**窗口级**状态，不随路由变化，但早先它只写在 ReaderView 的 keydown
 * 监听里：按 F11 进全屏 → 返回书架 → 监听器随组件卸载消失 → 再也按不出全屏，
 * 必须回阅读页才能退出（issue #33 补充反馈）。
 *
 * 这里在应用根挂一个兜底监听，覆盖书架 / 发现 / 统计 / RSS 等所有非阅读页。
 * 阅读页自己也会处理 F11（它要把 F11 一并纳入可换绑的快捷键矩阵，并处理弹层
 * 打开时放行），因此：
 *   - 只在**非阅读页**生效，避免同一次按键被处理两遍（连按语义会变成"没变化"）；
 *   - **尊重用户的换绑结果**：设置里把 F11 改成「不操作」或别的动作时，这里
 *     不能擅自切全屏。判定复用阅读页那份快捷键矩阵，两端行为一致。
 *   - 输入框内不拦截，与阅读页的 isKeyboardHotkeyBlocked 保持一致。
 */
function handleGlobalFullscreenKey(e: KeyboardEvent) {
  if (e.key !== 'F11' || e.repeat) return
  if (e.ctrlKey || e.metaKey || e.altKey || e.shiftKey) return
  if (route.name === 'reader') return
  let action: HotkeyActionId | null = null
  try {
    action = resolveHotkeyAction(readerStore.hotkeyBindings, 'F11', 'scroll')
  } catch {
    action = 'fullscreen'
  }
  if (action !== 'fullscreen') return
  const active = document.activeElement as HTMLElement | null
  const tagName = active?.tagName?.toLowerCase()
  if (tagName === 'input' || tagName === 'textarea' || tagName === 'select' || active?.isContentEditable) {
    return
  }
  e.preventDefault()
  void toggleAppFullscreen().catch((error) => {
    resetFullscreenIntent()
    appStore.showToast((error as Error)?.message || '切换全屏失败', 'warning')
  })
}

onMounted(() => {
  if (typeof window === 'undefined') return
  window.addEventListener('keydown', handleGlobalFullscreenKey)
})

onBeforeUnmount(() => {
  closeUnlisten?.()
  legadoUnlisten?.()
  stopBackgroundClassSync()
  window.removeEventListener('keydown', handleGlobalFullscreenKey)
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
  /* 这里**不能**加 opacity 过渡。
   *
   * 切换页面时「铺哪张图」和「用哪个透明度」是在同一帧一起变的: 从书架进阅读页
   * 会同时换成阅读页专属图并使用 readerImageOpacity。若 opacity 还在 transition,
   * 新图会先以旧图的透明度渲染整个过渡时长 —— 阅读页背景透明度设成 0 时最刺眼:
   * 阅读图先以桌面图的 100% 不透明度整张闪一下, 再淡出成透明(issue #20 第 2 条)。
   *
   * 透明度滑块每次 input 都会更新, 不需要过渡来"看起来平滑"。 */
}

.app-shell > .titlebar {
  position: relative;
  z-index: calc(var(--z-modal) + 20);
}

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

.app-shell.has-custom-background :is(
  .explore-view,
  .stats-view,
  .rss-view,
  .rss-manage-view,
  .rss-article-view,
  .recent-view
) {
  background: transparent !important;
}

.app-shell.has-custom-background :is(
  /* .explore-toolbar 是发现页现用类名; .explore-header 是旧名, 保留兼容
     以免回滚或其它引用处漏掉玻璃效果。 */
  .explore-toolbar,
  .explore-header,
  .categories-panel,
  .source-selector select,
  .stat-card,
  .chart-card,
  .book-stats-card,
  .range-chip,
  .rss-toolbar,
  .article-list-panel,
  .article-content-panel,
  .article-page-card,
  .rss-manage-hero,
  .rss-manage-tools,
  .rss-sources,
  .rss-editor,
  .recent-search-input,
  .filter-chip,
  .recent-clear-btn
) {
  background: color-mix(in srgb, var(--color-bg-elevated) 82%, transparent) !important;
  border-color: color-mix(in srgb, var(--color-border) 76%, transparent) !important;
  backdrop-filter: blur(18px) saturate(120%);
  -webkit-backdrop-filter: blur(18px) saturate(120%);
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

/* 迷你模式: 隐藏顶栏/底栏后正文能占满小窗。顶栏与底栏本身由 showHeader /
   showBottomNav 控制不渲染, 这里只收掉标题栏里的应用名, 留出拖动区域。 */
.app-shell.mini-mode > .titlebar .titlebar-title {
  display: none;
}

/* Teleported dialogs live outside .app-shell. Keep their fixed viewport below
   the custom titlebar so the draggable area never covers dialog content. */
:is(
  .modal-overlay,
  .modal-container,
  .drawer-overlay,
  .reader-overlay,
  .confirm-overlay,
  .login-preview-container,
  .subscription-backdrop
) {
  top: var(--titlebar-height, 30px) !important;
}
</style>
