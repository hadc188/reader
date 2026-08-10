<template>
  <Teleport to="body">
    <Transition name="fade">
      <div v-if="modelValue" class="drawer-overlay" @click="close"></div>
    </Transition>
    <Transition name="slide-right">
      <aside v-if="modelValue" class="settings-drawer">
        <div class="drawer-header">
          <h2>&#35774;&#32622;</h2>
          <button class="close-btn" @click="close">
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
              <path d="M18 6 6 18M6 6l12 12" />
            </svg>
          </button>
        </div>

        <div class="drawer-body">

          <section class="drawer-section">
            <h3 class="section-title">
              <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="18" height="18">
                <path d="M4 19.5v-15A2.5 2.5 0 0 1 6.5 2H20v20H6.5a2.5 2.5 0 0 1 0-5H20" />
              </svg>
              &#20070;&#28304;&#31649;&#29702;
            </h3>
            <div class="btn-group">
              <button class="action-btn" @click="openSourceManager">
                <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="16" height="16">
                  <path d="M12 20h9M16.5 3.5a2.12 2.12 0 0 1 3 3L7 19l-4 1 1-4Z" />
                </svg>
                &#20070;&#28304;&#31649;&#29702;
              </button>
            </div>
          </section>


          <section class="drawer-section">
            <h3 class="section-title">
              <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="18" height="18">
                <path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4" />
                <path d="M7 10l5 5 5-5" />
                <path d="M12 15V3" />
              </svg>
              &#22791;&#20221;&#19982;&#24674;&#22797;
            </h3>
            <div class="status-card">
              <span>{{ webdavStatusTitle }}</span>
              <small>{{ webdavStatusMessage }}</small>
            </div>
            <div class="btn-group">
              <button class="action-btn" @click="openWebdavManager">
                &#22791;&#20221;&#19982;&#24674;&#22797;
              </button>
            </div>
          </section>

          <section class="drawer-section">
            <h3 class="section-title">
              <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="18" height="18">
                <path d="M12 16V4" />
                <path d="m7 9 5-5 5 5" />
                <path d="M20 16.5a2.5 2.5 0 0 1-2.5 2.5h-11A2.5 2.5 0 0 1 4 16.5" />
              </svg>
              &#24212;&#29992;
            </h3>
            <div class="status-card">
              <span>{{ appStore.isOnline ? '\u5728\u7ebf' : '\u79bb\u7ebf' }}</span>
              <small>{{ appStore.pwaReady ? '\u5df2\u542f\u7528\u79bb\u7ebf\u5916\u58f3\u7f13\u5b58' : '\u79bb\u7ebf\u5916\u58f3\u672a\u542f\u7528' }}</small>
            </div>
            <div class="status-card">
              <span>{{ appVersion }}</span>
              <small>当前应用版本</small>
            </div>
            <template v-if="appStore.canCheckVersionUpdate">
              <div
                class="status-card"
                :class="{ accent: appStore.hasVersionUpdateReminder, muted: appStore.versionUpdateLoading }"
              >
                <span>{{ versionUpdateTitle }}</span>
                <small>{{ versionUpdateMessage }}</small>
              </div>
              <div class="btn-group version-actions">
                <button class="action-btn" :disabled="!appStore.versionUpdate?.releaseUrl" @click="handleOpenRelease">
                  查看 Release
                </button>
                <button
                  class="action-btn"
                  :disabled="!appStore.hasVersionUpdateReminder || appStore.versionUpdateLoading"
                  @click="handleDismissVersionUpdate"
                >
                  本版本不再提醒
                </button>
                <button class="action-btn" :disabled="appStore.versionUpdateLoading" @click="handleCheckVersionUpdate">
                  {{ appStore.versionUpdateLoading ? '检查中...' : '重新检查' }}
                </button>
              </div>
            </template>
            <div v-if="appStore.pwaUpdateAvailable" class="status-card accent">
              <span>&#21457;&#29616;&#26032;&#29256;&#26412;</span>
              <small>&#21047;&#26032;&#21518;&#21487;&#20351;&#29992;&#26368;&#26032;&#31163;&#32447;&#36164;&#28304;</small>
            </div>
            <div class="btn-group">
              <button class="action-btn" :disabled="!appStore.deferredInstallPrompt" @click="handleInstallPwa">
                &#23433;&#35013;&#21040;&#20027;&#23631;&#24149;
              </button>
              <button class="action-btn primary" :disabled="!appStore.pwaUpdateAvailable" @click="handleApplyUpdate">
                &#26356;&#26032;&#24212;&#29992;
              </button>
            </div>
          </section>

          <section class="drawer-section">
            <h3 class="section-title">
              <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="18" height="18">
                <rect width="7" height="7" x="3" y="3" rx="1" />
                <rect width="7" height="7" x="14" y="3" rx="1" />
                <rect width="7" height="7" x="3" y="14" rx="1" />
                <rect width="7" height="7" x="14" y="14" rx="1" />
              </svg>
              &#20070;&#26550;&#35774;&#32622;
            </h3>
            <div class="btn-group">
              <button class="action-btn" @click="refreshCache">
                <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="16" height="16">
                  <path d="M21 12a9 9 0 0 0-9-9 9.75 9.75 0 0 0-6.74 2.74L3 8" />
                  <path d="M3 3v5h5" />
                  <path d="M3 12a9 9 0 0 0 9 9 9.75 9.75 0 0 0 6.74-2.74L21 16" />
                  <path d="M16 16h5v5" />
                </svg>
                &#21047;&#26032;&#32531;&#23384;
              </button>
            </div>
          </section>

          <section class="drawer-section">
            <h3 class="section-title">
              <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="18" height="18">
                <circle cx="12" cy="12" r="4" />
                <path d="M12 2v2M12 20v2M4.93 4.93l1.41 1.41M17.66 17.66l1.41 1.41M2 12h2M20 12h2M6.34 17.66l-1.41 1.41M19.07 4.93l-1.41 1.41" />
              </svg>
              &#22806;&#35266;
            </h3>
            <div class="theme-toggle">
              <button
                class="theme-option"
                :class="{ active: appStore.theme === 'light' }"
                @click="setTheme('light')"
              >
                <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="20" height="20">
                  <circle cx="12" cy="12" r="4" />
                  <path d="M12 2v2M12 20v2M4.93 4.93l1.41 1.41M17.66 17.66l1.41 1.41M2 12h2M20 12h2M6.34 17.66l-1.41 1.41M19.07 4.93l-1.41 1.41" />
                </svg>
                &#20142;&#33394;
              </button>
              <button
                class="theme-option"
                :class="{ active: appStore.theme === 'dark' }"
                @click="setTheme('dark')"
              >
                <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="20" height="20">
                  <path d="M21 12.79A9 9 0 1 1 11.21 3 7 7 0 0 0 21 12.79z" />
                </svg>
                &#26263;&#33394;
              </button>
            </div>
          </section>

          <section class="drawer-section">
            <h3 class="section-title">
              <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="18" height="18">
                <path d="M17.94 17.94 12 12m0 0a3 3 0 1 0-3-3 3 3 0 0 0 3 3zM12 12l5.5-5.5" />
                <path d="M21 12.79A9 9 0 1 1 11.21 3 7 7 0 0 0 21 12.79z" />
              </svg>
              &#38544;&#34255;&#21151;&#33021;
            </h3>
            <div class="feature-toggle-list">
              <label class="feature-toggle">
                <span>隐藏统计</span>
                <button
                  class="toggle-btn"
                  :class="{ on: appStore.isFeatureHidden('stats') }"
                  type="button"
                  @click="appStore.toggleHiddenFeature('stats')"
                >
                  {{ appStore.isFeatureHidden('stats') ? '已隐藏' : '显示' }}
                </button>
              </label>
              <label class="feature-toggle">
                <span>隐藏 RSS</span>
                <button
                  class="toggle-btn"
                  :class="{ on: appStore.isFeatureHidden('rss') }"
                  type="button"
                  @click="appStore.toggleHiddenFeature('rss')"
                >
                  {{ appStore.isFeatureHidden('rss') ? '已隐藏' : '显示' }}
                </button>
              </label>
            </div>
          </section>
        </div>
      </aside>
    </Transition>
  </Teleport>
</template>

<script setup lang="ts">
import { computed } from 'vue'
import { useAppStore } from '../stores/app'
import { useBookshelfStore } from '../stores/bookshelf'

const props = defineProps<{
  modelValue: boolean
}>()

const emit = defineEmits<{
  'update:modelValue': [value: boolean]
}>()

const appStore = useAppStore()
const shelfStore = useBookshelfStore()
const appVersion = __APP_VERSION__

// Single-user desktop: local WebDAV backup is always available.
const webdavStatusTitle = computed(() => '\u6587\u4ef6\u5907\u4efd\u4e0e\u6062\u590d')
const webdavStatusMessage = computed(() => '\u652f\u6301\u5c06\u4e66\u67b6\u3001\u4e66\u7b7e\u7b49\u6570\u636e\u5907\u4efd\u5230\u4f60\u7684 WebDAV \u670d\u52a1\u5668\uff0c\u4e0b\u8f7d\u5907\u4efd\u6587\u4ef6\u5e76\u6267\u884c\u6062\u590d\u3002')

const versionUpdateTitle = computed(() => {
  const info = appStore.versionUpdate
  if (appStore.versionUpdateLoading && !info) return '正在检查新版本'
  if (!info) return '版本检查'
  if (info.error && !info.latestVersion) return '版本检查失败'
  if (info.updateAvailable) return `发现新版本 ${info.latestVersion}`
  return '已是最新版本'
})
const versionUpdateMessage = computed(() => {
  const info = appStore.versionUpdate
  if (appStore.versionUpdateLoading && !info) return '正在从 GitHub Release 获取最新版本。'
  if (!info) return '可检查 GitHub Release，发现新版后会在设置入口提示。'
  if (info.error && !info.latestVersion) return info.error
  if (info.updateAvailable && info.shouldRemind) {
    return `当前 ${info.currentVersion}，最新 ${info.latestVersion}。`
  }
  if (info.updateAvailable) {
    return `当前 ${info.currentVersion}，最新 ${info.latestVersion}，本版本已设置不再提醒。`
  }
  if (info.error) return `当前 ${info.currentVersion}，上次检查失败：${info.error}`
  return `当前 ${info.currentVersion}。`
})


function close() {
  emit('update:modelValue', false)
}


function openSourceManager() {
  close()
  appStore.showSourceManager = true
}


function openWebdavManager() {
  close()
  appStore.showWebdavManager = true
}

function refreshCache() {
  shelfStore.fetchBooks()
  appStore.showToast('\u4e66\u67b6\u5df2\u5237\u65b0', 'success')
  close()
}

function setTheme(t: 'light' | 'dark') {
  appStore.setTheme(t)
}

async function handleInstallPwa() {
  const accepted = await appStore.installPwa()
  if (!accepted) {
    appStore.showToast('\u5f53\u524d\u73af\u5883\u6682\u4e0d\u652f\u6301\u5b89\u88c5\uff0c\u6216\u7528\u6237\u5df2\u53d6\u6d88', 'warning')
    return
  }
  appStore.showToast('\u5b89\u88c5\u8bf7\u6c42\u5df2\u63d0\u4ea4', 'success')
}

function handleApplyUpdate() {
  const ok = appStore.applyPwaUpdate()
  if (!ok) {
    appStore.showToast('\u5f53\u524d\u6ca1\u6709\u53ef\u5e94\u7528\u7684\u65b0\u7248\u672c', 'warning')
  }
}

function handleOpenRelease() {
  const url = appStore.versionUpdate?.releaseUrl
  if (!url) return
  window.open(url, '_blank', 'noopener,noreferrer')
}

async function handleDismissVersionUpdate() {
  await appStore.dismissVersionUpdateReminder()
}

async function handleCheckVersionUpdate() {
  await appStore.checkVersionUpdate(true)
}
</script>

<style scoped>
.drawer-overlay {
  position: fixed;
  inset: 0;
  background: rgba(0, 0, 0, 0.4);
  z-index: var(--z-overlay);
  backdrop-filter: blur(4px);
}

.settings-drawer {
  position: fixed;
  top: var(--titlebar-height, 32px);
  right: 0;
  bottom: 0;
  width: min(420px, 94vw);
  background: var(--color-bg-elevated);
  z-index: var(--z-modal);
  display: flex;
  flex-direction: column;
  box-shadow: var(--shadow-xl);
  border-left: 1px solid var(--color-divider);
}

.drawer-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  min-height: 62px;
  padding: calc(var(--space-4) + var(--safe-area-top)) calc(var(--space-5) + var(--safe-area-right)) var(--space-4) var(--space-5);
  border-bottom: 1px solid var(--color-border-light);
  flex-shrink: 0;
}

.drawer-header h2 {
  font-size: var(--text-xl);
  font-weight: 700;
  letter-spacing: 0;
}

.close-btn {
  width: 36px;
  height: 36px;
  display: flex;
  align-items: center;
  justify-content: center;
  border-radius: var(--radius-full);
  color: var(--color-text-secondary);
  transition: all var(--duration-fast);
}

.close-btn:hover {
  background: var(--color-bg-hover);
  color: var(--color-text);
}

.close-btn svg {
  width: 20px;
  height: 20px;
}

.drawer-body {
  flex: 1;
  overflow-y: auto;
  -webkit-overflow-scrolling: touch;
  overscroll-behavior: contain;
  padding: var(--space-2) calc(var(--space-5) + var(--safe-area-right)) calc(var(--space-5) + var(--safe-area-bottom)) var(--space-5);
}

@media (max-width: 768px) {
  .settings-drawer {
    width: min(420px, 92vw);
  }
}

.drawer-section {
  padding: var(--space-5) 0;
  border-bottom: 1px solid var(--color-divider);
}

.drawer-section:last-child {
  border-bottom: none;
}

.section-title {
  display: flex;
  align-items: center;
  gap: var(--space-2);
  font-size: var(--text-sm);
  font-weight: 600;
  color: var(--color-text-secondary);
  letter-spacing: 0;
  margin-bottom: var(--space-3);
}

.user-info-card {
  display: flex;
  align-items: flex-start;
  gap: var(--space-3);
  padding: var(--space-3);
  background: var(--color-bg-sunken);
  border-radius: var(--radius-md);
}

.user-panel {
  flex: 1;
  display: grid;
  gap: var(--space-3);
}

.user-card-header {
  display: flex;
  align-items: flex-start;
  gap: var(--space-3);
}

.user-avatar-lg {
  width: 40px;
  height: 40px;
  border-radius: var(--radius-full);
  background: linear-gradient(135deg, var(--color-primary), var(--color-primary-light));
  color: white;
  display: flex;
  align-items: center;
  justify-content: center;
  font-weight: 700;
  font-size: var(--text-lg);
  flex-shrink: 0;
}

.user-detail {
  flex: 1;
  display: flex;
  flex-direction: column;
}

.user-name {
  font-weight: 600;
  font-size: var(--text-sm);
}

.user-role {
  font-size: var(--text-xs);
  color: var(--color-text-tertiary);
}

.password-panel {
  display: grid;
  gap: var(--space-3);
  padding: var(--space-3);
  border: 1px solid var(--color-border-light);
  border-radius: var(--radius-md);
  background: var(--color-bg);
}

.password-panel.embedded {
  background: var(--color-bg-elevated);
}

.password-field {
  display: grid;
  gap: var(--space-2);
}

.password-field span {
  font-size: var(--text-sm);
  color: var(--color-text-secondary);
}

.password-field input {
  min-height: 40px;
  padding: 0 var(--space-3);
  border-radius: var(--radius-md);
  border: 1px solid var(--color-border);
  background: var(--color-bg);
  color: inherit;
}

.password-actions {
  display: flex;
  justify-content: flex-start;
}

.btn-group {
  display: flex;
  flex-wrap: wrap;
  gap: var(--space-2);
}

.version-actions {
  margin-bottom: var(--space-3);
}

.action-btn {
  display: inline-flex;
  align-items: center;
  gap: var(--space-2);
  padding: var(--space-2) var(--space-4);
  border-radius: var(--radius-md);
  font-size: var(--text-sm);
  font-weight: 500;
  background: var(--color-bg);
  color: var(--color-text);
  border: 1px solid var(--color-border-light);
  transition: all var(--duration-fast) var(--ease-out);
}

.action-btn:hover {
  background: var(--color-bg-hover);
  border-color: var(--color-border);
}

.action-btn:active {
  transform: scale(0.97);
}

.action-btn.primary {
  background: var(--color-primary);
  color: white;
  border-color: var(--color-primary);
}

.action-btn.primary:hover {
  background: var(--color-primary-dark);
}

.action-btn.danger {
  color: var(--color-danger);
  border-color: transparent;
  background: transparent;
  padding: var(--space-1) var(--space-2);
}

.action-btn.danger:hover {
  background: rgba(245, 34, 45, 0.08);
}

.action-btn.full {
  width: 100%;
  justify-content: center;
}

.inline-link {
  padding: 0;
  background: transparent;
  border: none;
  color: var(--color-primary);
  font-weight: 500;
  justify-content: flex-start;
}

.inline-link:hover {
  background: transparent;
  border: none;
  color: var(--color-primary-dark);
}

.theme-toggle {
  display: flex;
  gap: var(--space-2);
}

.status-card {
  display: flex;
  flex-direction: column;
  gap: 4px;
  padding: var(--space-3);
  background: var(--color-bg);
  border: 1px solid var(--color-border-light);
  border-radius: var(--radius-lg);
  margin-bottom: var(--space-3);
}

.status-card span {
  font-size: var(--text-sm);
  font-weight: 600;
}

.status-card small {
  color: var(--color-text-tertiary);
}

.status-card.accent {
  background: rgba(var(--color-primary-rgb), 0.1);
  border-color: rgba(var(--color-primary-rgb), 0.2);
}

.status-card.muted {
  opacity: 0.72;
}

.action-btn:disabled {
  opacity: 0.45;
  cursor: not-allowed;
}

.feature-toggle-list {
  display: flex;
  flex-direction: column;
  gap: 10px;
}

.feature-toggle {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
  font-size: 13px;
  color: var(--color-text-secondary);
}

.toggle-btn {
  min-height: 30px;
  padding: 0 14px;
  border-radius: var(--radius-full);
  border: 1px solid var(--color-border);
  background: transparent;
  color: var(--color-text-secondary);
  font-size: 12px;
}

.toggle-btn.on {
  background: var(--color-bg-hover);
  color: var(--color-text-tertiary);
}

.theme-option {
  flex: 1;
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: var(--space-2);
  padding: var(--space-4);
  border-radius: var(--radius-md);
  border: 1px solid var(--color-border);
  background: var(--color-bg);
  font-size: var(--text-sm);
  font-weight: 500;
  transition: all var(--duration-fast);
  color: var(--color-text-secondary);
}

.theme-option.active {
  border-color: var(--color-primary);
  box-shadow: var(--focus-ring);
  color: var(--color-primary);
  background: var(--color-primary-bg);
}

.theme-option:hover:not(.active) {
  border-color: var(--color-border);
}
</style>
