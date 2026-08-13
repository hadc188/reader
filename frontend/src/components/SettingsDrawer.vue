<template>
  <Teleport to="body">
    <Transition name="fade">
      <div v-if="modelValue" class="drawer-overlay" @click="close"></div>
    </Transition>
    <Transition name="slide-right">
      <aside
        v-if="modelValue"
        class="settings-drawer"
        :class="{ 'with-custom-background': hasCustomBackground }"
      >
        <div class="drawer-header">
          <h2>&#35774;&#32622;</h2>
          <button class="close-btn" @click="close">
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
              <path d="M18 6 6 18M6 6l12 12" />
            </svg>
          </button>
        </div>

        <nav class="settings-tabs" aria-label="设置分类">
          <button
            v-for="tab in settingsTabs"
            :key="tab.value"
            type="button"
            :class="{ active: activeSettingsTab === tab.value }"
            @click="activeSettingsTab = tab.value"
          >{{ tab.label }}</button>
        </nav>

        <div class="drawer-body">

          <section v-show="activeSettingsTab === 'common'" class="drawer-section">
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

          <section v-if="isDesktopApp" v-show="activeSettingsTab === 'app'" class="drawer-section">
            <h3 class="section-title">
              <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="18" height="18">
                <circle cx="12" cy="12" r="3" />
                <path d="M19.4 15a1.7 1.7 0 0 0 .34 1.88l.06.06-2.83 2.83-.06-.06A1.7 1.7 0 0 0 15 19.4a1.7 1.7 0 0 0-1 .6 1.7 1.7 0 0 0-.4 1.1V21h-4v-.09A1.7 1.7 0 0 0 8.6 19.4a1.7 1.7 0 0 0-1.88.34l-.06.06-2.83-2.83.06-.06A1.7 1.7 0 0 0 4.6 15a1.7 1.7 0 0 0-.6-1 1.7 1.7 0 0 0-1.1-.4H3v-4h.09A1.7 1.7 0 0 0 4.6 8.6a1.7 1.7 0 0 0-.34-1.88l-.06-.06 2.83-2.83.06.06A1.7 1.7 0 0 0 9 4.6a1.7 1.7 0 0 0 1-.6 1.7 1.7 0 0 0 .4-1.1V3h4v.09A1.7 1.7 0 0 0 15.4 4.6a1.7 1.7 0 0 0 1.88-.34l.06-.06 2.83 2.83-.06.06A1.7 1.7 0 0 0 19.4 9c.14.38.35.72.6 1 .3.31.68.5 1.1.6h.09v4h-.09c-.42.1-.8.29-1.1.6-.25.28-.46.62-.6 1Z" />
              </svg>
              网络代理
            </h3>
            <div class="proxy-setting">
              <div class="proxy-mode-toggle" role="group" aria-label="网络代理模式">
                <button
                  type="button"
                  :class="{ active: proxyModeDraft === 'system' }"
                  @click="proxyModeDraft = 'system'"
                >跟随系统</button>
                <button
                  type="button"
                  :class="{ active: proxyModeDraft === 'manual' }"
                  @click="proxyModeDraft = 'manual'"
                >手动代理</button>
              </div>
              <div v-if="proxyModeDraft === 'manual'" class="proxy-address-field">
                <label for="network-proxy-url">代理地址</label>
                <input
                  id="network-proxy-url"
                  v-model.trim="proxyUrlDraft"
                  type="text"
                  inputmode="url"
                  autocomplete="off"
                  placeholder="http://127.0.0.1:7890"
                  @keydown.enter="saveNetworkProxy"
                >
              </div>
              <small class="proxy-hint">{{ proxyDescription }}</small>
              <button
                class="action-btn primary proxy-save-btn"
                type="button"
                :disabled="savingProxy || (proxyModeDraft === 'manual' && !proxyUrlDraft.trim())"
                @click="saveNetworkProxy"
              >{{ savingProxy ? '应用中...' : '保存并应用' }}</button>
            </div>
          </section>


          <section v-show="activeSettingsTab === 'common'" class="drawer-section">
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

          <section v-show="activeSettingsTab === 'app'" class="drawer-section">
            <h3 class="section-title">
              <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="18" height="18">
                <path d="M12 16V4" />
                <path d="m7 9 5-5 5 5" />
                <path d="M20 16.5a2.5 2.5 0 0 1-2.5 2.5h-11A2.5 2.5 0 0 1 4 16.5" />
              </svg>
              &#24212;&#29992;
            </h3>
            <div class="status-card">
              <span>{{ appVersion }}</span>
              <small>当前应用版本</small>
            </div>
            <div v-if="isDesktopApp" class="setting-switch-row">
              <div class="setting-switch-copy">
                <span>关闭时隐藏到系统托盘</span>
                <small>{{ appStore.closeToTray ? '关闭窗口后继续在后台运行' : '关闭窗口后直接退出应用' }}</small>
              </div>
              <button
                class="switch-control"
                :class="{ on: appStore.closeToTray }"
                type="button"
                role="switch"
                :aria-checked="appStore.closeToTray"
                aria-label="关闭时隐藏到系统托盘"
                @click="appStore.toggleCloseToTray()"
              >
                <span></span>
              </button>
            </div>
            <div v-if="isDesktopApp" class="boss-key-setting">
              <div class="setting-switch-row compact-row">
                <div class="setting-switch-copy">
                  <span>老板键</span>
                </div>
                <button
                  class="switch-control"
                  :class="{ on: appStore.bossKeyEnabled }"
                  type="button"
                  role="switch"
                  :aria-checked="appStore.bossKeyEnabled"
                  @click="toggleBossKey"
                ><span></span></button>
              </div>
              <button
                class="boss-key-recorder"
                :class="{ recording: recordingBossKey }"
                type="button"
                :disabled="!appStore.bossKeyEnabled"
                aria-label="老板键快捷键"
                @click="startBossKeyRecording"
                @keydown="recordBossKey"
                @blur="recordingBossKey = false"
              >
                {{ recordingBossKey ? '请按下新的快捷键' : formatBossKey(appStore.bossKeyShortcut) }}
              </button>
              <small class="boss-key-hint">需包含 Ctrl、Alt 或 Shift，F1 至 F12 可单独使用</small>
            </div>
            <template v-if="appStore.canCheckVersionUpdate">
              <div
                class="status-card"
                :class="{ accent: appStore.hasVersionUpdateReminder, muted: appStore.versionUpdateLoading && !appStore.desktopUpdateLoading }"
              >
                <span>{{ versionUpdateTitle }}</span>
                <small>{{ versionUpdateMessage }}</small>
                <div v-if="showDesktopUpdateProgress" class="update-progress" :class="{ failed: appStore.desktopUpdateProgress?.stage === 'failed' }" :role="appStore.desktopUpdateProgress?.stage === 'failed' ? undefined : 'progressbar'" :aria-valuenow="appStore.desktopUpdateProgress?.percent ?? undefined" aria-valuemin="0" aria-valuemax="100" :aria-valuetext="desktopUpdateProgressMessage">
                  <div class="update-progress-track">
                    <span :class="{ indeterminate: appStore.desktopUpdateProgress?.percent == null && appStore.desktopUpdateProgress?.stage !== 'failed' }" :style="updateProgressStyle"></span>
                  </div>
                  <small>{{ desktopUpdateProgressMessage }}</small>
                </div>
              </div>
              <div class="btn-group version-actions">
                <button class="action-btn" :disabled="!appStore.versionUpdate?.releaseUrl" @click="handleOpenRelease">
                  查看发行页
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
                <button
                  v-if="isDesktopApp"
                  class="action-btn primary"
                  :disabled="!appStore.versionUpdate?.updateAvailable || appStore.desktopUpdateLoading"
                  @click="handleDesktopUpdate"
                >
                  {{ desktopUpdateButtonLabel }}
                </button>
              </div>
            </template>
          </section>

          <section v-show="activeSettingsTab === 'common'" class="drawer-section">
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

          <section v-show="activeSettingsTab === 'appearance'" class="drawer-section">
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
            <div class="background-setting">
              <div class="setting-switch-copy">
                <span>桌面背景图</span>
              </div>
              <div class="background-actions">
                <button
                  class="background-picker"
                  type="button"
                  :disabled="processingBackground"
                  @click="backgroundInputRef?.click()"
                >
                  <span
                    class="background-preview"
                    :class="{ empty: !readerStore.config.backgroundImage }"
                    :style="readerStore.config.backgroundImage
                      ? { backgroundImage: `url(${readerStore.config.backgroundImage})` }
                      : undefined"
                  ></span>
                  <span>{{ processingBackground ? '正在处理' : (readerStore.config.backgroundImage ? '更换图片' : '选择图片') }}</span>
                </button>
                <button
                  v-if="readerStore.config.backgroundImage"
                  class="background-remove"
                  type="button"
                  :disabled="processingBackground"
                  @click="removeBackgroundImage"
                >移除</button>
                <input
                  ref="backgroundInputRef"
                  class="hidden-input"
                  type="file"
                  accept="image/jpeg,image/png,image/webp,image/bmp"
                  @change="handleBackgroundImageChange"
                >
              </div>
              <div v-if="readerStore.config.backgroundImage" class="background-opacity-row">
                <label for="desktop-background-opacity">背景透明度</label>
                <input
                  id="desktop-background-opacity"
                  type="range"
                  min="0"
                  max="1"
                  step="0.05"
                  :value="readerStore.config.backgroundOpacity"
                  @input="handleBackgroundOpacityChange"
                >
                <output>{{ Math.round(readerStore.config.backgroundOpacity * 100) }}%</output>
              </div>
            </div>
            <div class="setting-switch-row">
              <div class="setting-switch-copy">
                <span>应用到阅读页</span>
                <small>{{ readerBackgroundDescription }}</small>
              </div>
              <button
                class="switch-control"
                :class="{ on: readerStore.config.applyBackgroundToReader }"
                type="button"
                role="switch"
                :disabled="!readerStore.config.backgroundImage"
                :aria-checked="readerStore.config.applyBackgroundToReader"
                aria-label="将桌面背景图应用到阅读页"
                @click="toggleReaderBackground"
              >
                <span></span>
              </button>
            </div>
          </section>

          <section v-show="activeSettingsTab === 'appearance'" class="drawer-section">
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
import { computed, ref, watch } from 'vue'
import { useRoute } from 'vue-router'
import { useAppStore } from '../stores/app'
import { useBookshelfStore } from '../stores/bookshelf'
import { useReaderStore } from '../stores/reader'
import {
  normalizeReaderBackgroundOpacity,
  prepareReaderBackgroundImage,
} from '../utils/readerBackground'
import { captureBossKey, formatBossKey } from '../utils/bossKey'

const props = defineProps<{
  modelValue: boolean
}>()

const emit = defineEmits<{
  'update:modelValue': [value: boolean]
}>()

const appStore = useAppStore()
const shelfStore = useBookshelfStore()
const readerStore = useReaderStore()
const route = useRoute()
const appVersion = __APP_VERSION__
const isDesktopApp = typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window
const backgroundInputRef = ref<HTMLInputElement | null>(null)
const processingBackground = ref(false)
const recordingBossKey = ref(false)
const settingsTabs = [
  { value: 'common', label: '常用' },
  { value: 'appearance', label: '外观' },
  { value: 'app', label: '应用' },
] as const
const activeSettingsTab = ref<(typeof settingsTabs)[number]['value']>('common')
const proxyModeDraft = ref<'system' | 'manual'>(appStore.networkProxyMode)
const proxyUrlDraft = ref(appStore.networkProxyUrl)
const savingProxy = ref(false)
const hasCustomBackground = computed(() => Boolean(readerStore.config.backgroundImage) && (
  route.name !== 'reader' || readerStore.config.applyBackgroundToReader
))
const readerBackgroundDescription = computed(() => {
  if (!readerStore.config.backgroundImage) return '请先选择桌面背景图片'
  return readerStore.config.applyBackgroundToReader
    ? '桌面背景图也会显示在阅读页'
    : '阅读页继续使用阅读主题背景'
})
const proxyDescription = computed(() => {
  if (proxyModeDraft.value === 'manual') {
    return '书源、订阅源、封面和未单独配置代理的语音请求会使用此地址。'
  }
  if (appStore.networkProxyStatus?.mode === 'system') {
    return appStore.networkProxyStatus.active
      ? '已读取本机系统代理，相关网络请求会自动使用。'
      : '本机未启用系统代理，相关网络请求将直接连接。'
  }
  return '默认读取本机系统代理；未启用时自动直接连接。'
})

watch(() => props.modelValue, (visible) => {
  if (!visible) return
  proxyModeDraft.value = appStore.networkProxyMode
  proxyUrlDraft.value = appStore.networkProxyUrl
  if (isDesktopApp && proxyModeDraft.value === 'system') {
    void appStore.applyNetworkProxy().catch(() => undefined)
  }
})

// Single-user desktop: local WebDAV backup is always available.
const webdavStatusTitle = computed(() => '\u6587\u4ef6\u5907\u4efd\u4e0e\u6062\u590d')
const webdavStatusMessage = computed(() => '进入备份窗口后，可手动配置 WebDAV 地址并执行备份、下载和恢复。不会自动上传或恢复。')

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
    return `当前 ${info.currentVersion}，最新 ${info.latestVersion}。更新时会自动匹配安装包版或便携版。`
  }
  if (info.updateAvailable) {
    return `当前 ${info.currentVersion}，最新 ${info.latestVersion}，本版本已设置不再提醒。`
  }
  if (info.error) return `当前 ${info.currentVersion}，上次检查失败：${info.error}`
  return `当前 ${info.currentVersion}。`
})
const updateProgressStyle = computed(() => ({
  width: appStore.desktopUpdateProgress?.stage === 'failed'
    ? '100%'
    : appStore.desktopUpdateProgress?.percent == null
    ? '36%'
    : `${appStore.desktopUpdateProgress.percent}%`,
}))
const showDesktopUpdateProgress = computed(() => (
  appStore.desktopUpdateLoading || appStore.desktopUpdateProgress?.stage === 'failed'
))
const desktopUpdateProgressMessage = computed(() => {
  const progress = appStore.desktopUpdateProgress
  if (!progress) return '正在准备更新'
  if (progress.stage === 'downloading' && progress.percent != null) {
    return `${progress.message} ${progress.percent}%`
  }
  return progress.message
})
const desktopUpdateButtonLabel = computed(() => {
  const progress = appStore.desktopUpdateProgress
  if (!appStore.desktopUpdateLoading) return progress?.stage === 'failed' ? '重试更新' : '下载并更新'
  if (progress?.stage === 'downloading' && progress.percent != null) return `下载 ${progress.percent}%`
  if (progress?.stage === 'verifying') return '正在校验...'
  if (progress?.stage === 'ready') return '正在启动...'
  if (progress?.stage === 'failed') return '重试更新'
  return '正在准备...'
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

async function toggleBossKey() {
  try {
    await appStore.setBossKeyEnabled(!appStore.bossKeyEnabled)
    appStore.showToast(appStore.bossKeyEnabled ? '老板键已启用' : '老板键已关闭', 'success')
  } catch (error) {
    appStore.showToast((error as Error).message || '老板键设置失败', 'error')
  }
}

async function saveNetworkProxy() {
  if (savingProxy.value) return
  savingProxy.value = true
  try {
    const status = await appStore.setNetworkProxy(proxyModeDraft.value, proxyUrlDraft.value)
    appStore.showToast(
      status?.active ? '代理设置已应用' : '已使用直接连接',
      'success',
    )
  } catch (error) {
    appStore.showToast((error as Error).message || '代理设置失败', 'error')
  } finally {
    savingProxy.value = false
  }
}

function startBossKeyRecording(event: MouseEvent) {
  recordingBossKey.value = true
  ;(event.currentTarget as HTMLButtonElement).focus()
}

async function recordBossKey(event: KeyboardEvent) {
  if (!recordingBossKey.value || event.repeat) return
  event.preventDefault()
  event.stopPropagation()
  const result = captureBossKey(event)
  if (result.error === 'cancel') {
    recordingBossKey.value = false
    return
  }
  if (result.error) {
    appStore.showToast(result.error, 'error')
    return
  }
  if (!result.shortcut) return

  try {
    await appStore.setBossKeyShortcut(result.shortcut)
    recordingBossKey.value = false
    appStore.showToast('老板键快捷键已更新', 'success')
  } catch (error) {
    appStore.showToast((error as Error).message || '快捷键已被其他应用占用', 'error')
  }
}

function refreshCache() {
  shelfStore.fetchBooks()
  appStore.showToast('\u4e66\u67b6\u5df2\u5237\u65b0', 'success')
  close()
}

function setTheme(t: 'light' | 'dark') {
  appStore.setTheme(t)
}

function toggleReaderBackground() {
  if (!readerStore.config.backgroundImage) return
  readerStore.updateConfig(
    'applyBackgroundToReader',
    !readerStore.config.applyBackgroundToReader,
  )
}

async function handleBackgroundImageChange(event: Event) {
  const input = event.target as HTMLInputElement
  const file = input.files?.[0]
  if (!file) return
  processingBackground.value = true
  try {
    const dataUrl = await prepareReaderBackgroundImage(file)
    readerStore.setBackgroundImage(dataUrl)
    appStore.showToast('桌面背景已更新', 'success')
  } catch (error) {
    appStore.showToast((error as Error).message || '背景图片处理失败', 'error')
  } finally {
    processingBackground.value = false
    input.value = ''
  }
}

function handleBackgroundOpacityChange(event: Event) {
  const value = (event.target as HTMLInputElement).valueAsNumber
  readerStore.updateConfig('backgroundOpacity', normalizeReaderBackgroundOpacity(value))
}

function removeBackgroundImage() {
  readerStore.clearBackgroundImage()
  appStore.showToast('桌面背景已移除', 'success')
}

async function handleDesktopUpdate() {
  await appStore.applyDesktopUpdate()
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
  top: var(--titlebar-height, 30px);
  right: 0;
  bottom: 0;
  left: 0;
  background: rgba(0, 0, 0, 0.4);
  z-index: var(--z-overlay);
  backdrop-filter: blur(4px);
}

.settings-drawer {
  position: fixed;
  top: var(--titlebar-height, 30px);
  right: 0;
  bottom: 0;
  width: min(420px, 94vw);
  box-sizing: border-box;
  background: var(--color-bg-elevated);
  z-index: var(--z-modal);
  display: flex;
  flex-direction: column;
  box-shadow: var(--shadow-xl);
  border-left: 1px solid var(--color-divider);
}

.settings-drawer.with-custom-background {
  background: color-mix(in srgb, var(--color-bg-elevated) 82%, transparent);
  backdrop-filter: blur(24px) saturate(120%);
  -webkit-backdrop-filter: blur(24px) saturate(120%);
}

.settings-drawer.with-custom-background .status-card,
.settings-drawer.with-custom-background .setting-switch-row,
.settings-drawer.with-custom-background .background-setting,
.settings-drawer.with-custom-background .proxy-setting,
.settings-drawer.with-custom-background .theme-option {
  background: color-mix(in srgb, var(--color-bg-elevated) 66%, transparent);
  border-color: color-mix(in srgb, var(--color-border) 72%, transparent);
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

.settings-tabs {
  display: grid;
  grid-template-columns: repeat(3, minmax(0, 1fr));
  gap: 3px;
  margin: 0 var(--space-5) var(--space-2);
  padding: 3px;
  border-radius: var(--radius-lg);
  background: var(--color-bg-sunken);
}

.settings-tabs button {
  min-height: 38px;
  border-radius: calc(var(--radius-lg) - 3px);
  color: var(--color-text-secondary);
  font-size: var(--text-sm);
  font-weight: 600;
  transition: background var(--duration-fast), color var(--duration-fast), box-shadow var(--duration-fast), transform var(--duration-fast);
}

.settings-tabs button:hover:not(.active) {
  background: var(--color-bg-hover);
  color: var(--color-text);
}

.settings-tabs button:active {
  transform: scale(0.98);
}

.settings-tabs button.active {
  background: var(--color-bg-elevated);
  color: var(--color-text);
  box-shadow: var(--shadow-sm);
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

.boss-key-setting {
  display: grid;
  gap: var(--space-2);
  margin-bottom: var(--space-3);
  padding: var(--space-3);
  border: 1px solid var(--color-border-light);
  border-radius: var(--radius-lg);
  background: var(--color-bg);
}

.boss-key-setting .compact-row {
  margin: 0;
  padding: 0;
  border: 0;
}

.boss-key-recorder {
  min-height: 40px;
  padding: 0 var(--space-3);
  border: 1px solid var(--color-border);
  border-radius: var(--radius-md);
  background: var(--color-bg-elevated);
  color: var(--color-text);
  text-align: left;
  cursor: pointer;
}

.boss-key-recorder.recording {
  border-color: var(--color-primary);
  box-shadow: 0 0 0 3px color-mix(in srgb, var(--color-primary) 15%, transparent);
}

.boss-key-recorder:disabled {
  cursor: not-allowed;
  opacity: 0.55;
}

.boss-key-hint {
  color: var(--color-text-tertiary);
  line-height: 1.5;
}

.proxy-setting {
  display: grid;
  gap: var(--space-3);
  padding: var(--space-3);
  border: 1px solid var(--color-border-light);
  border-radius: var(--radius-lg);
  background: var(--color-bg);
}

.proxy-mode-toggle {
  display: grid;
  grid-template-columns: repeat(2, minmax(0, 1fr));
  gap: 3px;
  padding: 3px;
  border-radius: var(--radius-md);
  background: var(--color-bg-sunken);
}

.proxy-mode-toggle button {
  min-height: 36px;
  border-radius: calc(var(--radius-md) - 2px);
  color: var(--color-text-secondary);
  font-size: var(--text-sm);
}

.proxy-mode-toggle button.active {
  background: var(--color-bg-elevated);
  color: var(--color-text);
  box-shadow: var(--shadow-sm);
}

.proxy-address-field {
  display: grid;
  gap: var(--space-2);
}

.proxy-address-field label {
  color: var(--color-text-secondary);
  font-size: var(--text-sm);
}

.proxy-address-field input {
  width: 100%;
  min-height: 42px;
  box-sizing: border-box;
  padding: 0 var(--space-3);
  border: 1px solid var(--color-border);
  border-radius: var(--radius-md);
  background: var(--color-bg-elevated);
  color: var(--color-text);
}

.proxy-address-field input:focus {
  border-color: var(--color-primary);
  box-shadow: 0 0 0 3px color-mix(in srgb, var(--color-primary) 14%, transparent);
  outline: none;
}

.proxy-hint {
  color: var(--color-text-tertiary);
  line-height: 1.5;
}

.proxy-save-btn {
  justify-content: center;
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
  justify-content: center;
  gap: var(--space-2);
  min-height: 38px;
  padding: 0 var(--space-4);
  border-radius: var(--radius-md);
  font-size: var(--text-sm);
  font-weight: 600;
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
  margin-bottom: var(--space-3);
}

.background-setting {
  display: flex;
  flex-direction: column;
  gap: var(--space-3);
  padding: var(--space-3);
  margin-bottom: var(--space-3);
  border: 1px solid var(--color-border-light);
  border-radius: var(--radius-lg);
  background: var(--color-bg);
}

.background-actions {
  display: flex;
  align-items: center;
  gap: var(--space-2);
  min-width: 0;
}

.background-picker {
  min-width: 0;
  min-height: 44px;
  display: inline-flex;
  align-items: center;
  gap: var(--space-3);
  padding: 5px var(--space-3) 5px 5px;
  border: 1px solid var(--color-border);
  border-radius: var(--radius-md);
  background: var(--color-bg-elevated);
  color: var(--color-text-secondary);
  font-size: var(--text-sm);
  transition: border-color var(--duration-fast), background var(--duration-fast);
}

.background-picker:hover:not(:disabled) {
  border-color: var(--color-primary);
  background: var(--color-bg-hover);
}

.background-preview {
  width: 42px;
  height: 32px;
  flex: 0 0 auto;
  display: grid;
  place-items: center;
  border-radius: var(--radius-sm);
  background-position: center;
  background-repeat: no-repeat;
  background-size: cover;
  box-shadow: inset 0 0 0 1px var(--color-border-light);
}

.background-preview.empty {
  background: var(--color-bg-sunken);
  color: var(--color-text-tertiary);
}

.background-preview.empty::after {
  content: '+';
  font-size: 19px;
  font-weight: 400;
}

.background-remove {
  min-height: 34px;
  padding: 0 var(--space-3);
  border-radius: var(--radius-md);
  border: 1px solid var(--color-border);
  color: var(--color-text-tertiary);
  font-size: var(--text-xs);
}

.background-remove:hover:not(:disabled) {
  border-color: var(--color-danger);
  color: var(--color-danger);
}

.background-picker:disabled,
.background-remove:disabled {
  cursor: wait;
  opacity: 0.55;
}

.background-opacity-row {
  display: grid;
  grid-template-columns: auto minmax(100px, 1fr) 38px;
  align-items: center;
  gap: var(--space-3);
  color: var(--color-text-secondary);
  font-size: var(--text-xs);
}

.background-opacity-row input {
  width: 100%;
  accent-color: var(--color-primary);
}

.background-opacity-row output {
  text-align: right;
  color: var(--color-text-tertiary);
  font-variant-numeric: tabular-nums;
}

.hidden-input {
  display: none;
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

.update-progress {
  display: grid;
  gap: 5px;
  margin-top: var(--space-1);
}

.update-progress-track {
  height: 5px;
  overflow: hidden;
  border-radius: 999px;
  background: rgba(var(--color-primary-rgb), 0.16);
}

.update-progress-track span {
  display: block;
  height: 100%;
  border-radius: inherit;
  background: var(--color-primary);
  transition: width 160ms ease;
}

.update-progress-track span.indeterminate {
  animation: update-progress-slide 1.1s ease-in-out infinite alternate;
}

.update-progress.failed small {
  color: var(--color-danger);
}

.update-progress.failed .update-progress-track span {
  background: var(--color-danger);
}

@keyframes update-progress-slide {
  from { transform: translateX(-30%); }
  to { transform: translateX(205%); }
}

.setting-switch-row {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: var(--space-4);
  padding: var(--space-3);
  margin-bottom: var(--space-3);
  border: 1px solid var(--color-border-light);
  border-radius: var(--radius-lg);
  background: var(--color-bg);
}

.setting-switch-copy {
  display: flex;
  min-width: 0;
  flex-direction: column;
  gap: 3px;
}

.setting-switch-copy span {
  color: var(--color-text);
  font-size: var(--text-sm);
  font-weight: 600;
}

.setting-switch-copy small {
  color: var(--color-text-tertiary);
  font-size: var(--text-xs);
}

.switch-control {
  position: relative;
  width: 44px;
  height: 24px;
  flex: 0 0 44px;
  border: 1px solid var(--color-border);
  border-radius: var(--radius-full);
  background: var(--color-bg-sunken);
  transition: background var(--duration-fast), border-color var(--duration-fast);
}

.switch-control span {
  position: absolute;
  top: 3px;
  left: 3px;
  width: 16px;
  height: 16px;
  border-radius: var(--radius-full);
  background: var(--color-bg-elevated);
  box-shadow: var(--shadow-xs);
  transition: transform var(--duration-fast);
}

.switch-control.on {
  border-color: var(--color-primary);
  background: var(--color-primary);
}

.switch-control.on span {
  transform: translateX(20px);
}

.switch-control:disabled {
  cursor: not-allowed;
  opacity: 0.45;
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
