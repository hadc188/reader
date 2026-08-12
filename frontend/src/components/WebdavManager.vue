<template>
  <Teleport to="body">
    <Transition name="fade">
      <div v-if="modelValue" class="modal-overlay" @click="close"></div>
    </Transition>
    <Transition name="scale">
      <div v-if="modelValue" class="modal-container" @click.self="close">
        <section class="webdav-modal">
          <header class="modal-header">
            <div>
              <h2>备份与恢复</h2>
              <p class="subtitle">手动管理本地备份，并与 Legado 兼容的 WebDAV 备份</p>
            </div>
            <button class="icon-btn" @click="close" aria-label="关闭">
              <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                <path d="M18 6 6 18M6 6l12 12" />
              </svg>
            </button>
          </header>

          <div v-if="!webdavAvailable" class="notice warning">
            <strong>{{ unavailableTitle }}</strong>
            <span>{{ unavailableMessage }}</span>
          </div>

          <template v-else>
            <section class="remote-backup-panel">
              <div class="panel-heading">
                <div>
                  <h3>WebDAV 网盘备份</h3>
                  <p>备份文件保存到配置目录根下，手机端可直接恢复。不会自动上传或恢复。</p>
                </div>
                <button class="action-btn" :disabled="remoteWorking" @click="testRemote">
                  {{ remoteWorking && remoteAction === 'test' ? '测试中...' : '测试连接' }}
                </button>
              </div>
              <div class="remote-config-grid">
                <input v-model.trim="remoteDraft.url" type="url" autocomplete="url" placeholder="WebDAV 服务地址">
                <input v-model.trim="remoteDraft.account" type="text" autocomplete="username" placeholder="账号">
                <input v-model="remoteDraft.password" type="password" autocomplete="current-password" placeholder="应用密码">
                <input v-model.trim="remoteDraft.directory" type="text" placeholder="子目录（默认 legado）">
              </div>
              <div class="remote-actions">
                <button class="action-btn primary" :disabled="remoteWorking" @click="saveRemoteConfig">
                  保存配置
                </button>
                <button class="action-btn" :disabled="remoteWorking || !remoteConfigured" @click="uploadRemoteBackup">
                  {{ remoteWorking && remoteAction === 'upload' ? '上传中...' : '备份到网盘' }}
                </button>
                <button class="action-btn" :disabled="remoteWorking || !remoteConfigured" @click="loadRemoteBackups">
                  刷新网盘备份
                </button>
              </div>
              <div class="sync-progress-toggle">
                <div class="setting-switch-copy">
                  <span>同步阅读进度</span>
                  <small>{{ appStore.legadoSyncEnabled ? '进入和退出阅读页时与手机端同步' : '已关闭手机端阅读进度同步' }}</small>
                </div>
                <button
                  class="switch-control"
                  :class="{ on: appStore.legadoSyncEnabled }"
                  type="button"
                  role="switch"
                  :aria-checked="appStore.legadoSyncEnabled"
                  aria-label="同步阅读进度"
                  @click="appStore.setLegadoSyncEnabled(!appStore.legadoSyncEnabled)"
                ><span></span></button>
              </div>
              <div v-if="remoteError" class="notice error remote-error">{{ remoteError }}</div>
              <div v-if="remoteLoading" class="empty-state compact-empty">正在读取网盘备份...</div>
              <div v-else-if="remoteConfigured && remoteBackups.length === 0" class="empty-state compact-empty">网盘中暂无备份文件</div>
              <div v-else-if="remoteBackups.length" class="remote-backup-list">
                <div v-for="entry in remoteBackups" :key="entry.name" class="remote-backup-row">
                  <div class="remote-backup-info">
                    <strong>{{ entry.name }}</strong>
                    <small>{{ formatSize(entry.size) }} · {{ formatDate(entry.lastModified) }}</small>
                  </div>
                  <div class="file-actions">
                    <button class="mini-btn" :disabled="remoteWorking" @click="restoreRemoteBackup(entry)">恢复</button>
                    <button class="mini-btn" :disabled="remoteWorking" @click="downloadRemoteBackup(entry)">下载</button>
                    <button class="mini-btn danger" :disabled="remoteWorking" @click="deleteRemoteBackup(entry)">删除</button>
                  </div>
                </div>
              </div>
            </section>

            <div class="backup-location">
              <span class="backup-location-label">备份目录</span>
              <code class="backup-location-path">{{ backupPath || '加载中...' }}</code>
              <button class="action-btn" :disabled="!backupPath" @click="openFolder">
                在文件夹中打开
              </button>
            </div>

            <div class="toolbar">
              <div class="toolbar-left">
                <button class="action-btn primary" :disabled="working" @click="createBackup">
                  备份当前数据
                </button>
                <button class="action-btn" :disabled="working || loading" @click="loadFiles(currentPath)">
                  刷新列表
                </button>
                <button class="action-btn" :disabled="working" @click="triggerUpload">
                  上传文件
                </button>
                <input
                  ref="fileInputRef"
                  type="file"
                  multiple
                  accept=".json,.zip,application/json,application/zip"
                  class="hidden-input"
                  @change="handleUpload"
                />
              </div>
              <button
                class="action-btn danger"
                :disabled="working || selectedPaths.length === 0"
                @click="removeSelected"
              >
                删除选中项
              </button>
            </div>

            <div class="path-bar">
              <span class="path-label">当前目录</span>
              <code>{{ currentPath }}</code>
            </div>

            <div v-if="errorMessage" class="notice error">
              <strong>加载失败</strong>
              <span>{{ errorMessage }}</span>
            </div>

            <div class="file-list">
              <div v-if="loading" class="empty-state">正在加载文件列表...</div>
              <div v-else-if="entries.length === 0" class="empty-state">当前目录为空</div>
              <div v-else v-for="entry in entries" :key="entry.path" class="file-row">
                <label class="file-check" v-if="!entry.toParent">
                  <input
                    type="checkbox"
                    :checked="selectedPaths.includes(entry.path)"
                    @change="toggleSelection(entry.path)"
                  />
                </label>
                <span v-else class="file-check placeholder"></span>

                <button
                  class="file-main"
                  :class="{ directory: entry.isDirectory }"
                  @click="openEntry(entry)"
                >
                  <span class="file-icon">{{ entry.isDirectory ? '📁' : '📄' }}</span>
                  <span class="file-name">{{ entry.name }}</span>
                </button>

                <span class="file-meta">{{ entry.isDirectory ? '目录' : formatSize(entry.size) }}</span>
                <span class="file-meta">{{ formatDate(entry.lastModified) }}</span>

                <div class="file-actions">
                  <button
                    v-if="!entry.isDirectory && isBackupFile(entry.name)"
                    class="mini-btn"
                    :disabled="working"
                    @click="restoreBackup(entry)"
                  >
                    恢复
                  </button>
                  <button
                    v-if="!entry.isDirectory"
                    class="mini-btn"
                    :disabled="working"
                    @click="downloadEntry(entry)"
                  >
                    下载
                  </button>
                  <button
                    v-if="!entry.toParent"
                    class="mini-btn danger"
                    :disabled="working"
                    @click="removeEntry(entry)"
                  >
                    删除
                  </button>
                </div>
              </div>
            </div>
          </template>
        </section>
      </div>
    </Transition>
  </Teleport>
</template>

<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { useAppStore } from '../stores/app'
import {
  createWebdavBackupArchive,
  deleteWebdavFile,
  deleteWebdavFileList,
  getWebdavBackupArchive,
  getWebdavFileList,
  getWebdavFileText,
  getWebdavHome,
  openWebdavFolder,
  testLegadoWebdav,
  listLegadoWebdavBackups,
  uploadLegadoWebdavBackup,
  saveLegadoWebdavBackupAs,
  getLegadoWebdavBackupArchive,
  deleteLegadoWebdavBackup,
  saveWebdavFileAs,
  type LegadoWebdavBackupEntry,
  type LegadoWebdavConfig,
  type WebdavFileEntry,
  uploadFilesToWebdav,
} from '../api/webdav'
import {
  createCompatibleBackupArchiveFiles,
  parseCompatibleBackupArchive,
  createWebdavBackupPayload,
  parseWebdavBackup,
  restoreWebdavBackup,
} from '../utils/webdavBackup'

type EntryRow = WebdavFileEntry & { toParent?: boolean }

const props = defineProps<{
  modelValue: boolean
}>()

const emit = defineEmits<{
  'update:modelValue': [value: boolean]
}>()

const appStore = useAppStore()
const fileInputRef = ref<HTMLInputElement | null>(null)
const currentPath = ref('/')
const entries = ref<EntryRow[]>([])
const selectedPaths = ref<string[]>([])
const loading = ref(false)
const working = ref(false)
const errorMessage = ref('')
const backupPath = ref('')
const remoteDraft = ref<LegadoWebdavConfig>({ ...appStore.legadoWebdavConfig })
const remoteBackups = ref<LegadoWebdavBackupEntry[]>([])
const remoteLoading = ref(false)
const remoteWorking = ref(false)
const remoteAction = ref<'test' | 'upload' | 'restore' | 'download' | 'delete' | ''>('')
const remoteError = ref('')
const remoteConfigured = computed(() => Boolean(
  remoteDraft.value.url.trim() && remoteDraft.value.account.trim() && remoteDraft.value.password,
))

// Single-user desktop: WebDAV backup is always available.
const webdavAvailable = computed(() => true)

const unavailableTitle = computed(() => '')

const unavailableMessage = computed(() => '')

watch(
  () => props.modelValue,
  (visible) => {
    if (visible && webdavAvailable.value) {
      remoteDraft.value = { ...appStore.legadoWebdavConfig }
      void loadFiles(currentPath.value)
      getWebdavHome()
        .then((home) => { backupPath.value = home.path })
        .catch(() => { backupPath.value = '' })
      if (remoteConfigured.value) void loadRemoteBackups()
    }
    if (!visible) {
      errorMessage.value = ''
      selectedPaths.value = []
    }
  },
)

function close() {
  emit('update:modelValue', false)
}

async function openFolder() {
  try {
    await openWebdavFolder()
  } catch (error) {
    appStore.showToast((error as Error).message || '打开文件夹失败', 'error')
  }
}

function isBackupFile(name: string) {
  const lowerName = name.toLowerCase()
  return lowerName.endsWith('.json') || lowerName.endsWith('.zip')
}

function saveRemoteConfig() {
  appStore.setLegadoWebdavConfig({ ...remoteDraft.value, directory: remoteDraft.value.directory?.trim() || 'legado' })
  appStore.showToast('WebDAV 配置已保存', 'success')
  remoteError.value = ''
}

async function testRemote() {
  remoteWorking.value = true
  remoteAction.value = 'test'
  remoteError.value = ''
  try {
    await testLegadoWebdav(remoteDraft.value)
    saveRemoteConfig()
    appStore.showToast('WebDAV 连接成功', 'success')
    await loadRemoteBackups()
  } catch (error) {
    remoteError.value = (error as Error).message || '连接失败'
  } finally {
    remoteWorking.value = false
    remoteAction.value = ''
  }
}

async function loadRemoteBackups() {
  if (!remoteConfigured.value) return
  remoteLoading.value = true
  remoteError.value = ''
  try {
    remoteBackups.value = await listLegadoWebdavBackups(remoteDraft.value)
  } catch (error) {
    remoteBackups.value = []
    remoteError.value = (error as Error).message || '读取网盘备份失败'
  } finally {
    remoteLoading.value = false
  }
}

async function uploadRemoteBackup() {
  remoteWorking.value = true
  remoteAction.value = 'upload'
  try {
    saveRemoteConfig()
    const payload = await createWebdavBackupPayload()
    await uploadLegadoWebdavBackup(remoteDraft.value, buildBackupFilename(), createCompatibleBackupArchiveFiles(payload))
    appStore.showToast('备份已上传到 WebDAV', 'success')
    await loadRemoteBackups()
  } catch (error) {
    remoteError.value = (error as Error).message || '上传备份失败'
  } finally {
    remoteWorking.value = false
    remoteAction.value = ''
  }
}

async function downloadRemoteBackup(entry: LegadoWebdavBackupEntry) {
  remoteWorking.value = true
  remoteAction.value = 'download'
  try {
    const result = await saveLegadoWebdavBackupAs(remoteDraft.value, entry.name)
    if (result.saved) {
      appStore.showToast(`备份已保存到 ${result.path || '所选位置'}`, 'success')
    }
  } catch (error) {
    remoteError.value = (error as Error).message || '下载备份失败'
  } finally {
    remoteWorking.value = false
    remoteAction.value = ''
  }
}

async function restoreRemoteBackup(entry: LegadoWebdavBackupEntry) {
  const ok = await appStore.confirmDialog(`确定从 ${entry.name} 恢复数据吗？这会覆盖当前数据。`, { title: '恢复数据', danger: true })
  if (!ok) return
  remoteWorking.value = true
  remoteAction.value = 'restore'
  try {
    const contents = await getLegadoWebdavBackupArchive(remoteDraft.value, entry.name)
    const result = parseCompatibleBackupArchive(contents)
    await restoreWebdavBackup(result.payload)
    appStore.showToast('恢复完成，正在刷新页面', 'success')
    window.setTimeout(() => window.location.reload(), 800)
  } catch (error) {
    remoteError.value = (error as Error).message || '恢复备份失败'
  } finally {
    remoteWorking.value = false
    remoteAction.value = ''
  }
}

async function deleteRemoteBackup(entry: LegadoWebdavBackupEntry) {
  const ok = await appStore.confirmDialog(`确定删除 ${entry.name} 吗？`, { title: '删除备份', danger: true })
  if (!ok) return
  remoteWorking.value = true
  remoteAction.value = 'delete'
  try {
    await deleteLegadoWebdavBackup(remoteDraft.value, entry.name)
    appStore.showToast('远端备份已删除', 'success')
    await loadRemoteBackups()
  } catch (error) {
    remoteError.value = (error as Error).message || '删除备份失败'
  } finally {
    remoteWorking.value = false
    remoteAction.value = ''
  }
}

function formatSize(size: number) {
  if (!size) return '0 B'
  const units = ['B', 'KB', 'MB', 'GB']
  let value = size
  let unitIndex = 0
  while (value >= 1024 && unitIndex < units.length - 1) {
    value /= 1024
    unitIndex += 1
  }
  return `${value >= 10 || unitIndex === 0 ? value.toFixed(0) : value.toFixed(1)} ${units[unitIndex]}`
}

function formatDate(timestamp: number) {
  if (!timestamp) return '-'
  return new Date(timestamp).toLocaleString()
}

function toParentPath(path: string) {
  if (path === '/' || !path) return '/'
  const parts = path.split('/').filter(Boolean)
  parts.pop()
  return parts.length ? `/${parts.join('/')}` : '/'
}

function toggleSelection(path: string) {
  if (selectedPaths.value.includes(path)) {
    selectedPaths.value = selectedPaths.value.filter((item) => item !== path)
  } else {
    selectedPaths.value = selectedPaths.value.concat(path)
  }
}

async function loadFiles(path = '/') {
  loading.value = true
  errorMessage.value = ''
  try {
    const list = await getWebdavFileList(path)
    currentPath.value = path
    selectedPaths.value = []
    const rows: EntryRow[] = path !== '/'
      ? [{
          name: '..',
          size: 0,
          path: toParentPath(path),
          lastModified: 0,
          isDirectory: true,
          toParent: true,
        }]
      : []
    rows.push(...list.sort((a, b) => {
      if (a.isDirectory !== b.isDirectory) return a.isDirectory ? -1 : 1
      return a.name.localeCompare(b.name)
    }))
    entries.value = rows
  } catch (error) {
    errorMessage.value = (error as Error).message || '无法读取本地备份文件列表'
    entries.value = []
  } finally {
    loading.value = false
  }
}

function openEntry(entry: EntryRow) {
  if (entry.isDirectory) {
    void loadFiles(entry.path)
  }
}

function triggerUpload() {
  fileInputRef.value?.click()
}

async function handleUpload(event: Event) {
  const input = event.target as HTMLInputElement
  const files = Array.from(input.files || [])
  if (!files.length) return
  working.value = true
  try {
    await uploadFilesToWebdav(
      files.map((file) => ({ file, name: file.name })),
      currentPath.value,
    )
    appStore.showToast('文件已上传到本地', 'success')
    await loadFiles(currentPath.value)
  } catch (error) {
    appStore.showToast((error as Error).message || '上传失败', 'error')
  } finally {
    working.value = false
    input.value = ''
  }
}

function buildBackupFilename() {
  const now = new Date()
  const pad = (value: number) => String(value).padStart(2, '0')
  return `backup${now.getFullYear()}-${pad(now.getMonth() + 1)}-${pad(now.getDate())}-reader-desktop-${pad(now.getHours())}${pad(now.getMinutes())}${pad(now.getSeconds())}.zip`
}

async function createBackup() {
  working.value = true
  try {
    const payload = await createWebdavBackupPayload()
    await createWebdavBackupArchive(
      createCompatibleBackupArchiveFiles(payload),
      buildBackupFilename(),
      '/backups',
    )
    appStore.showToast('备份已保存到 /backups', 'success')
    await loadFiles('/backups')
  } catch (error) {
    appStore.showToast((error as Error).message || '备份失败', 'error')
  } finally {
    working.value = false
  }
}

async function downloadEntry(entry: EntryRow) {
  working.value = true
  try {
    const result = await saveWebdavFileAs(entry.path)
    if (result.saved) {
      appStore.showToast(`备份已保存到 ${result.path || '所选位置'}`, 'success')
    }
  } catch (error) {
    appStore.showToast((error as Error).message || '下载失败', 'error')
  } finally {
    working.value = false
  }
}

async function removeEntry(entry: EntryRow) {
  const ok = await appStore.confirmDialog(`确定删除 ${entry.name} 吗？`, { title: '删除文件', danger: true })
  if (!ok) return
  working.value = true
  try {
    await deleteWebdavFile(entry.path)
    appStore.showToast('文件已删除', 'success')
    await loadFiles(currentPath.value)
  } catch (error) {
    appStore.showToast((error as Error).message || '删除失败', 'error')
  } finally {
    working.value = false
  }
}

async function removeSelected() {
  if (!selectedPaths.value.length) return
  const ok = await appStore.confirmDialog(`确定删除选中的 ${selectedPaths.value.length} 个项目吗？`, { title: '批量删除', danger: true })
  if (!ok) return
  working.value = true
  try {
    await deleteWebdavFileList(selectedPaths.value)
    appStore.showToast('选中文件已删除', 'success')
    await loadFiles(currentPath.value)
  } catch (error) {
    appStore.showToast((error as Error).message || '批量删除失败', 'error')
  } finally {
    working.value = false
  }
}

async function restoreBackup(entry: EntryRow) {
  const ok = await appStore.confirmDialog(`确定从 ${entry.name} 恢复数据吗？这会覆盖当前书架、书源、RSS、书签和净化规则。`, { title: '恢复数据', danger: true })
  if (!ok) {
    return
  }

  working.value = true
  try {
    const result = entry.name.toLowerCase().endsWith('.zip')
      ? parseCompatibleBackupArchive(await getWebdavBackupArchive(entry.path))
      : {
          payload: parseWebdavBackup(await getWebdavFileText(entry.path)),
          format: 'reader' as const,
          skippedLocalBooks: 0,
        }
    await restoreWebdavBackup(result.payload)
    const skippedMessage = result.skippedLocalBooks > 0
      ? `，已跳过 ${result.skippedLocalBooks} 本仅含安卓路径的本地书籍`
      : ''
    appStore.showToast(`恢复完成${skippedMessage}，正在刷新页面`, 'success')
    window.setTimeout(() => {
      window.location.reload()
    }, 800)
  } catch (error) {
    appStore.showToast((error as Error).message || '恢复失败', 'error')
    working.value = false
  }
}
</script>

<style scoped>
.modal-overlay {
  position: fixed;
  inset: 0;
  background: rgba(15, 23, 42, 0.45);
  backdrop-filter: blur(6px);
  z-index: var(--z-overlay);
}

.modal-container {
  position: fixed;
  inset: 0;
  z-index: var(--z-modal);
  display: flex;
  align-items: center;
  justify-content: center;
  padding: var(--space-6);
}

.webdav-modal {
  width: min(980px, 100%);
  max-height: min(88vh, 920px);
  background: var(--color-bg-elevated);
  border: 1px solid var(--color-border-light);
  border-radius: var(--radius-xl);
  box-shadow: var(--shadow-xl);
  display: flex;
  flex-direction: column;
  overflow: hidden;
}

.remote-backup-panel {
  margin: var(--space-4) var(--space-6) 0;
  padding: var(--space-4);
  border: 1px solid var(--color-border-light);
  border-radius: var(--radius-lg);
  background: var(--color-bg-sunken);
}

.panel-heading {
  display: flex;
  align-items: flex-start;
  justify-content: space-between;
  gap: var(--space-4);
}

.panel-heading h3 {
  font-size: var(--text-md);
  font-weight: 700;
}

.panel-heading p {
  margin-top: 4px;
  color: var(--color-text-secondary);
  font-size: var(--text-xs);
}

.remote-config-grid {
  display: grid;
  grid-template-columns: repeat(2, minmax(0, 1fr));
  gap: var(--space-2);
  margin-top: var(--space-3);
}

.remote-config-grid input {
  width: 100%;
  min-height: 36px;
  padding: 0 var(--space-3);
  border: 1px solid var(--color-border-light);
  border-radius: var(--radius-md);
  background: var(--color-bg-elevated);
  color: var(--color-text);
  font: inherit;
  font-size: var(--text-sm);
}

.remote-actions {
  display: flex;
  flex-wrap: wrap;
  gap: var(--space-2);
  margin-top: var(--space-3);
}

.sync-progress-toggle {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: var(--space-3);
  margin-top: var(--space-3);
  padding-top: var(--space-3);
  border-top: 1px solid var(--color-divider);
}

.sync-progress-toggle .setting-switch-copy {
  display: grid;
  gap: 3px;
  min-width: 0;
}

.sync-progress-toggle .setting-switch-copy span {
  font-size: var(--text-sm);
  font-weight: 600;
}

.sync-progress-toggle .setting-switch-copy small {
  color: var(--color-text-tertiary);
  font-size: var(--text-xs);
  line-height: 1.4;
}

.sync-progress-toggle .switch-control {
  position: relative;
  flex: 0 0 auto;
  width: 42px;
  height: 24px;
  padding: 0;
  border: 0;
  border-radius: 999px;
  background: var(--color-border);
  cursor: pointer;
  transition: background 0.2s ease;
}

.sync-progress-toggle .switch-control span {
  position: absolute;
  top: 3px;
  left: 3px;
  width: 18px;
  height: 18px;
  border-radius: 50%;
  background: #fff;
  box-shadow: 0 1px 3px rgba(0, 0, 0, 0.24);
  transition: transform 0.2s ease;
}

.sync-progress-toggle .switch-control.on {
  background: var(--color-primary);
}

.sync-progress-toggle .switch-control.on span {
  transform: translateX(18px);
}

.remote-backup-list {
  margin-top: var(--space-3);
  border-top: 1px solid var(--color-divider);
}

.remote-backup-row {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: var(--space-3);
  padding: var(--space-3) 0;
  border-bottom: 1px solid var(--color-divider);
}

.remote-backup-info {
  display: grid;
  gap: 4px;
  min-width: 0;
}

.remote-backup-info strong {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  font-size: var(--text-sm);
}

.remote-backup-info small {
  color: var(--color-text-tertiary);
  font-size: var(--text-xs);
}

.compact-empty {
  min-height: 80px;
}

.remote-error {
  margin: var(--space-3) 0 0;
}

.modal-header {
  display: flex;
  align-items: flex-start;
  justify-content: space-between;
  gap: var(--space-4);
  padding: var(--space-5) var(--space-6);
  border-bottom: 1px solid var(--color-divider);
}

.modal-header h2 {
  font-size: var(--text-xl);
  font-weight: 700;
}

.subtitle {
  margin-top: var(--space-1);
  color: var(--color-text-secondary);
  font-size: var(--text-sm);
}

.icon-btn,
.action-btn,
.mini-btn,
.file-main {
  border: none;
  background: none;
  font: inherit;
}

.icon-btn {
  width: 38px;
  height: 38px;
  border-radius: var(--radius-md);
  color: var(--color-text-secondary);
  display: flex;
  align-items: center;
  justify-content: center;
}

.icon-btn:hover {
  background: var(--color-bg-hover);
}

.icon-btn svg {
  width: 18px;
  height: 18px;
}

.toolbar {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: var(--space-3);
  padding: var(--space-4) var(--space-6);
  border-bottom: 1px solid var(--color-divider);
}

.toolbar-left {
  display: flex;
  flex-wrap: wrap;
  gap: var(--space-2);
}

.action-btn,
.mini-btn {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  border-radius: var(--radius-md);
  border: 1px solid var(--color-border-light);
  background: var(--color-bg-sunken);
  color: var(--color-text);
  transition: all var(--duration-fast);
}

.action-btn {
  min-height: 38px;
  padding: 0 var(--space-4);
  font-size: var(--text-sm);
  font-weight: 600;
}

.mini-btn {
  min-height: 30px;
  padding: 0 var(--space-3);
  font-size: var(--text-xs);
  font-weight: 600;
}

.action-btn.primary {
  background: var(--color-primary);
  border-color: var(--color-primary);
  color: #fff;
}

.action-btn.danger,
.mini-btn.danger {
  color: var(--color-danger);
}

.action-btn:disabled,
.mini-btn:disabled {
  opacity: 0.45;
  cursor: not-allowed;
}

.path-bar,
.notice {
  margin: var(--space-4) var(--space-6) 0;
}

.backup-location {
  display: flex;
  align-items: center;
  gap: var(--space-3);
  margin: var(--space-4) var(--space-6) 0;
  padding: var(--space-3) var(--space-4);
  border-radius: var(--radius-md);
  background: var(--color-bg-sunken);
  font-size: var(--text-sm);
}

.backup-location-label {
  flex-shrink: 0;
  color: var(--color-text-secondary);
}

.backup-location-path {
  flex: 1;
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  color: var(--color-text);
}

.backup-location .action-btn {
  flex-shrink: 0;
  min-height: 30px;
  padding: 0 var(--space-3);
  font-size: var(--text-xs);
}

.path-bar {
  display: flex;
  align-items: center;
  gap: var(--space-3);
  color: var(--color-text-secondary);
  font-size: var(--text-sm);
}

.path-bar code {
  padding: var(--space-1) var(--space-2);
  border-radius: var(--radius-sm);
  background: var(--color-bg-sunken);
  color: var(--color-text);
}

.notice {
  display: grid;
  gap: 4px;
  padding: var(--space-3) var(--space-4);
  border-radius: var(--radius-md);
  font-size: var(--text-sm);
}

.notice.warning {
  background: rgba(201, 127, 58, 0.12);
  border: 1px solid rgba(201, 127, 58, 0.18);
}

.notice.error {
  background: rgba(245, 34, 45, 0.08);
  border: 1px solid rgba(245, 34, 45, 0.14);
}

.file-list {
  flex: 1;
  overflow: auto;
  padding: var(--space-4) var(--space-6) var(--space-6);
}

.file-row {
  display: grid;
  grid-template-columns: 28px minmax(0, 1fr) 90px 180px auto;
  align-items: center;
  gap: var(--space-3);
  min-height: 56px;
  padding: 0 var(--space-3);
  border-bottom: 1px solid var(--color-divider);
}

.file-row:last-child {
  border-bottom: none;
}

.file-check {
  display: flex;
  align-items: center;
  justify-content: center;
}

.file-check.placeholder {
  width: 20px;
}

.file-main {
  display: flex;
  align-items: center;
  gap: var(--space-3);
  min-width: 0;
  padding: 0;
  text-align: left;
  color: var(--color-text);
}

.file-main.directory .file-name {
  color: var(--color-primary);
}

.file-icon {
  font-size: 18px;
}

.file-name {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.file-meta {
  font-size: var(--text-xs);
  color: var(--color-text-tertiary);
}

.file-actions {
  display: flex;
  justify-content: flex-end;
  gap: var(--space-2);
}

.empty-state {
  display: flex;
  align-items: center;
  justify-content: center;
  min-height: 220px;
  color: var(--color-text-tertiary);
  font-size: var(--text-sm);
}

.hidden-input {
  display: none;
}

@media (max-width: 768px) {
  .modal-container {
    padding: var(--space-3);
  }

  .webdav-modal {
    max-height: 92vh;
  }

  .remote-config-grid {
    grid-template-columns: 1fr;
  }

  .remote-backup-row,
  .panel-heading {
    align-items: stretch;
    flex-direction: column;
  }

  .toolbar {
    flex-direction: column;
    align-items: stretch;
  }

  .toolbar-left {
    width: 100%;
  }

  .file-row {
    grid-template-columns: 28px minmax(0, 1fr);
    padding: var(--space-3);
  }

  .file-meta {
    display: none;
  }

  .file-actions {
    grid-column: 2;
    justify-content: flex-start;
    flex-wrap: wrap;
  }
}
</style>
