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

          <section v-show="activeSettingsTab === 'common'" class="drawer-section">
            <h3 class="section-title">
              <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="18" height="18">
                <path d="M6 3h12a2 2 0 0 1 2 2v16l-8-5-8 5V5a2 2 0 0 1 2-2Z" />
              </svg>
              阅读数据
            </h3>
            <div class="btn-group">
              <button class="action-btn" @click="openBookmarkManager">
                <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="16" height="16">
                  <path d="M6 3h12a2 2 0 0 1 2 2v16l-8-5-8 5V5a2 2 0 0 1 2-2Z" />
                </svg>
                书签管理
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
              <span class="section-badge">{{ appVersion }}</span>
            </h3>
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
                <span class="boss-key-value">
                  {{ recordingBossKey ? '请按下新的快捷键' : formatBossKey(appStore.bossKeyShortcut) }}
                </span>
                <span class="boss-key-hint">{{ recordingBossKey ? 'Esc 取消' : '点击修改' }}</span>
              </button>
            </div>
            <div v-if="isDesktopApp && appStore.borderlessKind === 'shadow'" class="boss-key-setting">
              <div class="setting-switch-row compact-row">
                <div class="setting-switch-copy">
                  <span>无边框模式</span>
                </div>
                <button
                  class="switch-control"
                  :class="{ on: appStore.borderlessMode }"
                  type="button"
                  role="switch"
                  :aria-checked="appStore.borderlessMode"
                  @click="toggleBorderlessModeSetting"
                ><span></span></button>
              </div>
              <button
                class="boss-key-recorder"
                :class="{ recording: recordingBorderlessShortcut }"
                type="button"
                aria-label="切换无边框模式快捷键"
                @click="startBorderlessRecording"
                @keydown="recordBorderlessShortcut"
                @blur="recordingBorderlessShortcut = false"
              >
                <span class="boss-key-value">
                  {{ recordingBorderlessShortcut ? '请按下新的快捷键' : formatBossKey(appStore.borderlessShortcut) }}
                </span>
                <span class="boss-key-hint">{{ recordingBorderlessShortcut ? 'Esc 取消' : '点击修改' }}</span>
              </button>
            </div>
            <div v-if="isDesktopApp" class="boss-key-setting">
              <div class="setting-switch-row compact-row">
                <div class="setting-switch-copy">
                  <span>隐藏标题栏</span>
                </div>
                <button
                  class="switch-control"
                  :class="{ on: appStore.hideTitlebar }"
                  type="button"
                  role="switch"
                  :aria-checked="appStore.hideTitlebar"
                  @click="appStore.toggleTitlebar()"
                ><span></span></button>
              </div>
              <button
                class="boss-key-recorder"
                :class="{ recording: recordingTitlebarShortcut }"
                type="button"
                aria-label="切换标题栏快捷键"
                @click="startTitlebarRecording"
                @keydown="recordTitlebarShortcut"
                @blur="recordingTitlebarShortcut = false"
              >
                <span class="boss-key-value">
                  {{ recordingTitlebarShortcut ? '请按下新的快捷键' : formatBossKey(appStore.titlebarShortcut) }}
                </span>
                <span class="boss-key-hint">{{ recordingTitlebarShortcut ? 'Esc 取消' : '点击修改' }}</span>
              </button>
            </div>
            <!-- 与上面的「老板键」同一层级: 都是自带边框的卡片, 不再额外加小标题,
                 否则两个相邻分组的标题层级不一致(一个有标题、一个没有)。 -->
            <WebAccessSetting v-if="isDesktopApp" />
            <template v-if="appStore.canCheckVersionUpdate">
              <div class="setting-switch-row">
                <div class="setting-switch-copy">
                  <span>自动检查更新</span>
                  <small>{{ appStore.autoCheckUpdate ? '启动时检查版本，有更新时弹窗提示' : '启动时不检查版本更新' }}</small>
                </div>
                <button
                  class="switch-control"
                  :class="{ on: appStore.autoCheckUpdate }"
                  type="button"
                  role="switch"
                  :aria-checked="appStore.autoCheckUpdate"
                  aria-label="自动检查更新"
                  @click="appStore.setAutoCheckUpdate(!appStore.autoCheckUpdate)"
                >
                  <span></span>
                </button>
              </div>
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
              <div class="version-actions">
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
                  {{ appStore.versionUpdateLoading ? '检查中…' : '重新检查' }}
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
            <!--
              背景图相关整块在 Web 端隐藏: 选图后要 `upload_background_image`
              把文件落到应用目录, 那是 Tauri 的 octet-stream 上传命令, 浏览器里
              没有等价路由。留下的只有"点了报错"和"永远禁用的开关"。
            -->
            <div v-if="!appStore.isWebMode" class="background-setting">
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
                      ? { backgroundImage: `url('${readerStore.config.backgroundImage}')` }
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
                  class="settings-slider"
                  type="range"
                  min="0"
                  max="1"
                  :step="READER_BACKGROUND_OPACITY_STEP"
                  :value="readerStore.config.backgroundOpacity"
                  @input="handleBackgroundOpacityChange"
                >
                <label class="opacity-input-wrap" title="可直接输入 0-100 的数值">
                  <input
                    class="opacity-input"
                    type="number"
                    min="0"
                    max="100"
                    step="1"
                    :value="readerOpacityToPercent(readerStore.config.backgroundOpacity)"
                    @change="applyBackgroundOpacityPercent(($event.target as HTMLInputElement).value, $event.target as HTMLInputElement)"
                  >
                  <span class="opacity-unit">%</span>
                </label>
              </div>
            </div>
            <div v-if="!appStore.isWebMode" class="setting-switch-row">
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

            <!-- 窗口透明度：与桌面背景图并列的「应用级」外观项。
                 只有 Windows 桌面壳支持整窗 alpha，其它平台不渲染。 -->
            <div
              v-if="!appStore.isWebMode && readerStore.windowOpacitySupported"
              class="background-setting"
            >
              <div class="setting-switch-copy">
                <span>窗口透明度</span>
              </div>
              <div class="background-opacity-row">
                <label for="app-window-opacity">不透明度</label>
                <input
                  id="app-window-opacity"
                  class="settings-slider"
                  type="range"
                  :min="windowOpacityFloorPercent"
                  max="100"
                  step="1"
                  :value="readerOpacityToPercent(readerStore.config.windowOpacity)"
                  @input="updateWindowOpacityPercent(($event.target as HTMLInputElement).value)"
                >
                <label class="opacity-input-wrap" title="可直接输入数值">
                  <input
                    class="opacity-input"
                    type="number"
                    :min="windowOpacityFloorPercent"
                    max="100"
                    step="1"
                    :value="readerOpacityToPercent(readerStore.config.windowOpacity)"
                    @change="applyWindowOpacityPercent(($event.target as HTMLInputElement).value, $event.target as HTMLInputElement)"
                  >
                  <span class="opacity-unit">%</span>
                </label>
              </div>
              <small class="window-opacity-note">只影响书架、设置等非阅读页面；阅读页在阅读设置中单独调节。</small>
            </div>
          </section>

          <!-- 隐藏功能: 只对桌面端有意义。Web 端本来就没有统计与 RSS,
               这两个开关没有可隐藏的对象。 -->
          <section
            v-if="!appStore.isWebMode"
            v-show="activeSettingsTab === 'appearance'"
            class="drawer-section"
          >
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
import { computed, ref, watch, type Ref } from 'vue'
import { useRoute } from 'vue-router'
import { useAppStore } from '../stores/app'
import { useBookshelfStore } from '../stores/bookshelf'
import { useReaderStore } from '../stores/reader'
import { isTauriRuntime } from '../api/transport'
import {
  READER_WINDOW_OPACITY_MIN,
  READER_BACKGROUND_OPACITY_STEP,
  normalizeReaderBackgroundOpacity,
  normalizeReaderWindowOpacity,
  percentToReaderOpacity,
  prepareReaderBackgroundImage,
  readerOpacityToPercent,
} from '../utils/readerBackground'
import { captureBossKey, formatBossKey } from '../utils/bossKey'
import { uploadBackgroundImage } from '../api/background'
import WebAccessSetting from './WebAccessSetting.vue'

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
const isDesktopApp = isTauriRuntime()
const backgroundInputRef = ref<HTMLInputElement | null>(null)
const processingBackground = ref(false)
const recordingBossKey = ref(false)
const recordingBorderlessShortcut = ref(false)
const recordingTitlebarShortcut = ref(false)

/**
 * 设置分类。Web 端只留「外观」——
 *
 * 「常用」里是书源/备份/代理这些桌面能力, 「应用」里是托盘、老板键、版本更新、
 * Web 访问开关(自己给自己开后门没有意义)。浏览器里点开只会看到一堆点了报错的
 * 按钮, 不如直接不给。
 *
 * 注意保留「外观」: 阅读页的字号、行距、主题在手机上同样要调。
 */
const allSettingsTabs = [
  { value: 'common', label: '常用' },
  { value: 'appearance', label: '外观' },
  { value: 'app', label: '应用' },
] as const
const settingsTabs = computed(() =>
  appStore.isWebMode
    ? allSettingsTabs.filter((tab) => tab.value === 'appearance')
    : [...allSettingsTabs],
)
const activeSettingsTab = ref<(typeof allSettingsTabs)[number]['value']>(
  // Web 端只有「外观」一个标签, 直接把默认值设成它 ——
  // 否则会停在一个根本不渲染的「常用」上, 面板看起来是空的。
  appStore.isWebMode ? 'appearance' : 'common',
)

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
  // 没有更新时不重复版本号: 标题旁已有版本徽标, 这里再写一遍纯属冗余。
  return `已确认 ${info.currentVersion} 为最新，无需更新。`
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

function openBookmarkManager() {
  close()
  appStore.showBookmarkManager = true
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

async function toggleBorderlessModeSetting() {
  try {
    await appStore.toggleBorderlessMode()
    appStore.showToast(
      appStore.borderlessMode ? '已进入无边框模式' : '已退出无边框模式',
      'success',
    )
  } catch (error) {
    appStore.showToast((error as Error).message || '无边框模式设置失败', 'error')
  }
}

function startShortcutRecording(
  event: MouseEvent,
  recording: Ref<boolean>,
) {
  recording.value = true
  ;(event.currentTarget as HTMLButtonElement).focus()
}

function recordSystemShortcut(
  event: KeyboardEvent,
  recording: Ref<boolean>,
  apply: (shortcut: string) => void,
  successMessage: string,
) {
  if (!recording.value || event.repeat) return
  event.preventDefault()
  event.stopPropagation()
  const result = captureBossKey(event)
  if (result.error === 'cancel') {
    recording.value = false
    return
  }
  if (result.error) {
    appStore.showToast(result.error, 'error')
    return
  }
  if (!result.shortcut) return
  apply(result.shortcut)
  recording.value = false
  appStore.showToast(successMessage, 'success')
}

function startBorderlessRecording(event: MouseEvent) {
  startShortcutRecording(event, recordingBorderlessShortcut)
}

function recordBorderlessShortcut(event: KeyboardEvent) {
  recordSystemShortcut(
    event,
    recordingBorderlessShortcut,
    appStore.setBorderlessShortcut,
    '无边框模式快捷键已更新',
  )
}

function startTitlebarRecording(event: MouseEvent) {
  startShortcutRecording(event, recordingTitlebarShortcut)
}

function recordTitlebarShortcut(event: KeyboardEvent) {
  recordSystemShortcut(
    event,
    recordingTitlebarShortcut,
    appStore.setTitlebarShortcut,
    '标题栏快捷键已更新',
  )
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
    // 原分辨率上传, 由后端落盘 —— 不再压到 1600px(issue #20 第 1 条)。
    const prepared = await prepareReaderBackgroundImage(file)
    const url = await uploadBackgroundImage('desktop', prepared.blob, prepared.fileName)
    readerStore.setBackgroundImage(url)
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

/** 百分比输入框: 非数字输入不写回(保持原值), 越界自动夹到 0-100。
 *  与阅读页背景图共用 percentToReaderOpacity 的边界规则。
 *
 *  无论接受与否, 都把手动输入框重新对齐到真实值: 输入框是单向绑定(`:value`),
 *  store 值没变时 Vue 不重写 DOM, 于是清空/输入 150/输入 -5 会让框里停着非法
 *  文本而实际透明度没变, UI 与真实值脱节。 */
function applyBackgroundOpacityPercent(raw: string, input: HTMLInputElement) {
  const next = percentToReaderOpacity(raw)
  if (next === null) {
    syncBackgroundOpacityInput(input, readerStore.config.backgroundOpacity)
    return
  }
  readerStore.updateConfig('backgroundOpacity', next)
  syncBackgroundOpacityInput(input, next)
}

function syncBackgroundOpacityInput(input: HTMLInputElement, applied: number) {
  const actual = String(readerOpacityToPercent(applied))
  if (input.value !== actual) input.value = actual
}

/** 窗口透明度下限（百分比整数），与 Rust 侧保持一致。 */
const windowOpacityFloorPercent = Math.round(READER_WINDOW_OPACITY_MIN * 100)

function updateWindowOpacityPercent(raw: string) {
  const percent = Number(raw)
  if (!Number.isFinite(percent)) return
  readerStore.updateConfig('windowOpacity', normalizeReaderWindowOpacity(percent / 100))
}

function applyWindowOpacityPercent(raw: string, input: HTMLInputElement) {
  const parsed = percentToReaderOpacity(raw)
  if (parsed === null) {
    syncBackgroundOpacityInput(input, readerStore.config.windowOpacity)
    return
  }
  const next = normalizeReaderWindowOpacity(parsed)
  readerStore.updateConfig('windowOpacity', next)
  syncBackgroundOpacityInput(input, next)
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
  /* `--titlebar-height` 是桌面端自绘标题栏的高度。Web 端由 global.css 把它
     归零(见 `html.web-mode`), 这里不需要特殊处理。 */
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
.settings-drawer.with-custom-background .theme-option,
/* 老板键与 Web 访问也是同一层级的卡片, 背景图模式下必须一起半透明 ——
 * 漏掉它们会让这几块变成不透明实心色, 在背景图上显得像贴了块补丁。
 *
 * 类名必须是 `.web-access`: 那是 `WebAccessSetting.vue` 根元素上的名字。
 * 这里曾写成 `.web-access-setting`(照组件文件名臆造的), 选择器永远不匹配,
 * 症状正是注释里说的"贴了块补丁" —— 而它恰好是最难联想到"写错类名"的那种。 */
.settings-drawer.with-custom-background .boss-key-setting,
.settings-drawer.with-custom-background .web-access {
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

.section-hint {
  display: block;
  margin-top: var(--space-2);
  color: var(--color-text-tertiary);
  font-size: var(--text-xs);
  line-height: 1.5;
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

/* `.boss-key-*` 的样式已提到 `styles/global.css`。原因同 Web 访问:
 * 它们是独立的一块控件, 放在 scoped 里无法被其它组件/演示页复用, 而且
 * 同一个类在文件里出现过两份(后者覆盖前者的 display:flex), 已一并清理。 */

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

/* 版本区四个按钮等宽排列, 2×2 网格。
 *
 * 不能用 `.btn-group` 的 `flex-wrap`: 四个按钮文案长度差得多("查看发行页" 5 字,
 * "本版本不再提醒" 7 字), 换行后每行起点和宽度都不一致, 看起来像没对齐。
 * 两列等宽网格在任何字数下都对齐, "下载并更新"也自然落在"重新检查"旁边,
 * 不会单独占一整行。`minmax(0, 1fr)` 防止长文案把列宽撑破。 */
.version-actions {
  display: grid;
  grid-template-columns: repeat(2, minmax(0, 1fr));
  gap: var(--space-2);
  margin-bottom: var(--space-3);
}

.version-actions .action-btn {
  width: 100%;
}

/* 标题旁的版本号徽标。
 * 原来是单独一张 status-card, 但下面「版本检查」卡片里已经写着"当前 v1.5.8",
 * 同一个信息出现两次、还各占一块底色, 显得零碎。 */
.section-badge {
  margin-left: auto;
  padding: 2px 8px;
  border-radius: var(--radius-full);
  background: var(--color-bg-sunken);
  color: var(--color-text-tertiary);
  font-size: var(--text-xs);
  font-weight: 500;
  font-variant-numeric: tabular-nums;
  letter-spacing: 0;
}

/* `.action-btn` / `.btn-group` 的公共样式已提到 `styles/global.css` ——
 * 它们同时被 WebAccessSetting、WebdavManager 等独立组件复用，留在 scoped
 * 样式里子组件拿不到。此处只保留设置面板自己的修饰类。 */

/* 主操作走"淡底 + 主色文字/描边", 而不是一整块高饱和橙压白字。
 * 两个理由: 一是整块纯主色在浅色面板里分量过重, 把周围浅色控件的层次全压平;
 * 二是白字压在 #c56f35 上对比度只有约 4:1, 观感发闷、显脏。
 * 权重靠"有彩度 vs 无彩度"区分(对比旁边的 .action-btn 白底灰边), 主次依旧清楚。
 * 文字色用 primary 与 text 的混合色: 浅色主题下比纯 primary 更暗、深色主题下
 * 自动偏向浅橙, 两个主题都能保证可读性, 不必分别写死。 */
.action-btn.primary {
  background: var(--color-selected-bg);
  /* 60% 主色 + 40% 正文色: 浅色主题 #835131 在淡橙底上 5.3:1, 深色主题 #e3bb9f
   * 在暗底上 8.5:1, 两边都过 AA。调高比例会让浅色主题跌破 4.5:1
   * (80% 只有 3.8:1), 所以这个值不要再往上抬。 */
  color: color-mix(in srgb, var(--color-primary) 60%, var(--color-text));
  border-color: var(--color-selected-border);
}

.action-btn.primary:hover {
  background: var(--color-selected-bg-hover);
  border-color: var(--color-primary);
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

.window-opacity-note {
  display: block;
  margin-top: 8px;
  color: var(--color-text-secondary);
  font-size: 12px;
  line-height: 1.5;
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
  grid-template-columns: auto minmax(100px, 1fr) auto;
  align-items: center;
  gap: var(--space-3);
  color: var(--color-text-secondary);
  font-size: var(--text-xs);
}

/* 与阅读页设置里的"字体大小"滑块同款: 4px 圆角轨道 + 14px 圆点。
 * 不能复用 ReadSettings.vue 的 .settings-slider —— 那是 scoped 样式, 跨组件不生效,
 * 所以这里必须复刻一份; 改其一记得同步另一处。 */
.background-opacity-row .settings-slider {
  width: 100%;
  appearance: none;
  -webkit-appearance: none;
  height: 18px;
  margin: 0;
  background: transparent;
  cursor: pointer;
}

.background-opacity-row .settings-slider::-webkit-slider-runnable-track {
  height: 4px;
  border-radius: 999px;
  background: color-mix(in srgb, var(--color-text-primary) 20%, transparent);
}

.background-opacity-row .settings-slider::-webkit-slider-thumb {
  -webkit-appearance: none;
  appearance: none;
  width: 14px;
  height: 14px;
  margin-top: -5px;
  border-radius: 50%;
  background: var(--color-text-primary);
  box-shadow: 0 1px 3px rgba(0, 0, 0, 0.25);
}

.background-opacity-row .settings-slider::-moz-range-track {
  height: 4px;
  border-radius: 999px;
  background: color-mix(in srgb, var(--color-text-primary) 20%, transparent);
}

.background-opacity-row .settings-slider::-moz-range-thumb {
  width: 14px;
  height: 14px;
  border: none;
  border-radius: 50%;
  background: var(--color-text-primary);
  box-shadow: 0 1px 3px rgba(0, 0, 0, 0.25);
}

/* 透明度百分比: 平时就是一段右对齐的 "35%" 文本 —— 常驻的边框与底色会让它像
 * 一颗按钮, 和左边的滑块抢注意力。悬停/聚焦时才显形, 提示这里可以直接输入。
 * 与 % 之间不留 gap: 被撑开时读起来像两个分离的元素, 而它是一个数。
 * 与阅读页设置里的同名控件保持一致的观感。 */
.opacity-input-wrap {
  display: inline-flex;
  align-items: center;
  gap: 0;
  flex-shrink: 0;
  padding: 3px 5px;
  border-radius: 6px;
  /* 透明边框占位: 悬停/聚焦只换颜色, 不会冒出 1px 的布局抖动。 */
  border: 1px solid transparent;
  cursor: text;
  transition: background 0.16s ease, border-color 0.16s ease;
}

.opacity-input-wrap:hover {
  background: var(--color-bg-hover);
}

.opacity-input-wrap:focus-within {
  background: var(--color-bg-active);
  border-color: var(--color-border);
}

.opacity-input {
  /* 3.6ch ≈ 1.8em, 恰好容下三位数字(tabular-nums 下每位约 0.55em, "100" 要
   * 1.65em)。再宽就会把数字推离 %, 再窄则三位数会顶格甚至被裁。 */
  width: 3.6ch;
  padding: 0;
  border: none;
  background: transparent;
  font: inherit;
  font-variant-numeric: tabular-nums;
  text-align: right;
  color: var(--color-text-secondary);
  -moz-appearance: textfield;
  appearance: textfield;
}

.opacity-input:focus {
  outline: none;
  color: var(--color-text-primary);
}

/* number 输入的上下箭头在这么小的框里只会挤掉数字本身。 */
.opacity-input::-webkit-outer-spin-button,
.opacity-input::-webkit-inner-spin-button {
  -webkit-appearance: none;
  margin: 0;
}

.opacity-unit {
  color: var(--color-text-tertiary);
}

.hidden-input {
  display: none;
}

/* `.status-card` 公共样式在 `styles/global.css`。 */

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

/* `.setting-switch-row` / `.setting-switch-copy` / `.switch-control` 的公共
 * 样式已提到 `styles/global.css`, 供 WebAccessSetting 等独立组件复用 ——
 * 留在 scoped 样式里, 子组件内部元素拿不到(它们没有本组件的 data-v 属性)。 */

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
  color: var(--color-text-secondary);
}
</style>
