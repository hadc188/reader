<template>
  <Teleport to="body">
    <Transition name="fade">
      <div v-if="appStore.updateDialogVisible" class="update-overlay">
        <div class="update-dialog">
          <h3 class="update-title">应用更新</h3>

          <template v-if="isFailed">
            <p class="update-message error">{{ progress?.message || '更新失败' }}</p>
            <p v-if="appStore.versionUpdate?.latestVersion" class="update-subtitle">
              可尝试重新下载，或前往发行页手动下载。
            </p>
          </template>

          <template v-else>
            <p class="update-message">{{ headline }}</p>
            <div class="progress-track" :class="{ indeterminate: percent == null }">
              <span
                v-if="percent != null"
                class="progress-fill"
                :style="{ width: `${percent}%` }"
              ></span>
              <span v-else class="progress-indeterminate"></span>
            </div>
            <p class="update-meta">
              <span v-if="percent != null">{{ percent }}%</span>
              <span v-if="progress && progress.total > 0">{{ formatBytes(progress.downloaded) }} / {{ formatBytes(progress.total) }}</span>
              <span v-if="speedText">{{ speedText }}</span>
            </p>
          </template>

          <div class="update-actions">
            <template v-if="isFailed">
              <button class="update-btn" type="button" @click="openReleasePage">查看发行页</button>
              <button class="update-btn primary" type="button" :disabled="appStore.desktopUpdateLoading" @click="retry">重试更新</button>
            </template>
            <button v-if="!appStore.desktopUpdateLoading || isFailed" class="update-btn" type="button" @click="close">
              关闭
            </button>
          </div>
        </div>
      </div>
    </Transition>
  </Teleport>
</template>

<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { useAppStore } from '../stores/app'

const appStore = useAppStore()

const progress = computed(() => appStore.desktopUpdateProgress)

const isFailed = computed(() => progress.value?.stage === 'failed')
const percent = computed(() => {
  const value = progress.value?.percent
  if (value == null) return null
  return Math.max(0, Math.min(100, Math.round(value)))
})

const headline = computed(() => {
  switch (progress.value?.stage) {
    case 'downloading':
      return '正在下载更新文件'
    case 'verifying':
      return '正在校验更新文件'
    case 'ready':
      return '更新文件已准备完成，应用即将退出并安装'
    default:
      return '正在确认最新版本'
  }
})

// 简易测速: 相邻两次进度回调的增量 ÷ 时间。
const speedText = ref('')
let lastSpeedSample: { at: number; bytes: number } | null = null

watch(() => progress.value?.downloaded, (downloaded) => {
  if (progress.value?.stage !== 'downloading' || downloaded == null) {
    speedText.value = ''
    lastSpeedSample = null
    return
  }
  const now = Date.now()
  if (!lastSpeedSample || now - lastSpeedSample.at < 800) {
    if (!lastSpeedSample) lastSpeedSample = { at: now, bytes: downloaded }
    return
  }
  const deltaBytes = downloaded - lastSpeedSample.bytes
  const deltaSeconds = (now - lastSpeedSample.at) / 1000
  if (deltaSeconds > 0 && deltaBytes >= 0) {
    speedText.value = `${formatBytes(deltaBytes / deltaSeconds)}/秒`
  }
  lastSpeedSample = { at: now, bytes: downloaded }
})

watch(() => progress.value?.stage, () => {
  speedText.value = ''
  lastSpeedSample = null
})

function formatBytes(value: number) {
  if (!Number.isFinite(value) || value <= 0) return '0 MB'
  const mb = value / (1024 * 1024)
  if (mb >= 1024) return `${(mb / 1024).toFixed(2)} GB`
  if (mb >= 10) return `${mb.toFixed(0)} MB`
  return `${mb.toFixed(1)} MB`
}

function close() {
  appStore.closeUpdateProgressDialog()
}

function retry() {
  void appStore.applyDesktopUpdate()
}

function openReleasePage() {
  const url = appStore.versionUpdate?.releaseUrl
  if (url) {
    window.open(url, '_blank', 'noopener,noreferrer')
  }
}
</script>

<style scoped>
.update-overlay {
  position: fixed;
  inset: 0;
  z-index: calc(var(--z-modal) + 10);
  display: flex;
  align-items: center;
  justify-content: center;
  background: rgba(0, 0, 0, 0.45);
  backdrop-filter: blur(2px);
  padding: 24px;
}

.update-dialog {
  width: min(380px, 100%);
  background: var(--color-bg-elevated);
  border-radius: var(--radius-lg);
  padding: 20px;
  box-shadow: var(--shadow-xl);
}

.update-title {
  margin: 0 0 12px;
  font-size: 15px;
  font-weight: 700;
}

.update-message {
  margin: 0 0 14px;
  font-size: 13px;
  line-height: 1.6;
  color: var(--color-text-secondary);
  word-break: break-all;
}

.update-message.error {
  color: var(--color-danger);
}

.update-subtitle {
  margin: -8px 0 14px;
  font-size: 12px;
  color: var(--color-text-tertiary);
}

.progress-track {
  position: relative;
  height: 8px;
  border-radius: 999px;
  background: var(--color-border-light);
  overflow: hidden;
}

.progress-fill {
  display: block;
  height: 100%;
  border-radius: inherit;
  background: var(--color-primary);
  transition: width 0.2s ease;
}

.progress-indeterminate {
  position: absolute;
  top: 0;
  bottom: 0;
  width: 36%;
  border-radius: inherit;
  background: var(--color-primary);
  animation: indeterminate 1.2s ease-in-out infinite;
}

@keyframes indeterminate {
  0% { left: -36%; }
  100% { left: 100%; }
}

.update-meta {
  margin: 8px 0 0;
  display: flex;
  gap: 12px;
  font-size: 12px;
  font-variant-numeric: tabular-nums;
  color: var(--color-text-tertiary);
}

.update-actions {
  margin-top: 16px;
  display: flex;
  justify-content: flex-end;
  gap: 10px;
}

.update-btn {
  min-height: 34px;
  padding: 0 16px;
  border: 1px solid var(--color-border);
  border-radius: var(--radius-md);
  background: transparent;
  color: var(--color-text-secondary);
  font-size: 13px;
  cursor: pointer;
}

.update-btn.primary {
  background: var(--color-primary);
  border-color: var(--color-primary);
  color: #fff;
}

.update-btn:disabled {
  opacity: 0.6;
  cursor: not-allowed;
}

.update-btn:hover:not(:disabled) {
  background: var(--color-bg-hover);
}

.update-btn.primary:hover:not(:disabled) {
  background: var(--color-primary-dark);
}

.fade-enter-active,
.fade-leave-active {
  transition: opacity 0.15s ease;
}

.fade-enter-from,
.fade-leave-to {
  opacity: 0;
}
</style>
