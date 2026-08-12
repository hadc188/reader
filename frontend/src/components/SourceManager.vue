<template>
  <Teleport to="body">
    <Transition name="fade">
      <div v-if="modelValue" class="modal-overlay" @click="close"></div>
    </Transition>

    <Transition name="scale">
      <div v-if="modelValue" class="modal-container" @click.self="close">
        <div class="source-modal">
          <SourceManagerHeader
            :total="sourceStats.total"
            :enabled="sourceStats.enabled"
            :filtered="sourceStats.filtered"
            :selected="selectedFilteredSources.length"
            :loading="loading"
            :testing="testingSources"
            :invalid-count="invalidSources.length"
            @refresh="loadSources"
            @import-local="triggerFileImport"
            @open-subscriptions="subscriptionPanelVisible = true"
            @export="exportSources"
            @test-sources="testSources"
            @delete-invalid="removeInvalidSources"
            @create="createSource"
            @close="close"
          />

          <input
            ref="fileInputRef"
            type="file"
            accept=".json,.txt"
            class="hidden-input"
            @change="handleFileImport"
          />

          <SourceFilterBar
            :filter-text="filterText"
            :filter-group="filterGroup"
            :groups="groupList"
            :all-selected="allFilteredSelected"
            :partially-selected="partiallyFilteredSelected"
            :selected-count="selectedFilteredSources.length"
            :has-sources="filteredSources.length > 0"
            @update:filter-text="filterText = $event"
            @update:filter-group="filterGroup = $event"
            @toggle-current-selection="toggleFilteredSelection"
            @clear-selection="clearSelection"
            @delete-selection="removeSelectedSources"
          />

          <div class="content-grid">
            <SourceList
              class="source-list-slot"
              :sources="filteredSources"
              :loading="loading"
              :selected-urls="selectedSourceUrls"
              :active-url="editingSource?.bookSourceUrl"
              :empty-title="sources.length ? '没有匹配的书源' : '暂无书源'"
              :empty-description="sources.length ? '调整搜索关键词或分组筛选后再试' : '可以本地导入、远程同步或手动新增'"
              @edit="editSource"
              @toggle-enabled="toggleSource"
              @toggle-selection="toggleSourceSelection"
              @delete="removeSource"
              @pin="pinSource"
              @unpin="unpinSource"
            />

            <SourceEditorPanel
              class="editor-slot"
              :source="editingSource"
              :editor-text="editorText"
              :can-login="canLoginSource"
              :login-loading="sourceLoginLoading"
              :reset-signal="resetSignal"
              @update:editor-text="editorText = $event"
              @format="formatEditor"
              @save="saveEditor"
              @login="handleSourceLogin"
              @create="createSource"
              @import-local="triggerFileImport"
            />
          </div>

          <footer class="modal-footer">
            <span class="count-info">显示 {{ filteredSources.length }} / {{ sources.length }}</span>
          </footer>
        </div>
      </div>
    </Transition>

    <SourceSubscriptionPanel
      v-model:remote-url="remoteUrl"
      :visible="subscriptionPanelVisible"
      :subscriptions="subscriptions"
      @sync="importRemoteSource"
      @save="saveSubscription"
      @sync-subscription="syncSubscription"
      @remove-subscription="removeSubscription"
      @close="subscriptionPanelVisible = false"
    />

    <Transition name="scale">
      <div v-if="loginPreviewVisible" class="login-preview-container" @click.self="loginPreviewVisible = false">
        <div class="login-preview-modal">
          <div class="login-preview-header">
            <div>
              <h3>书源登录页</h3>
              <p>{{ loginPreviewUrl }}</p>
            </div>
            <button class="icon-btn" type="button" @click="loginPreviewVisible = false" title="关闭">
              <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                <path d="M18 6 6 18M6 6l12 12" />
              </svg>
            </button>
          </div>
          <iframe class="login-preview-frame" :src="loginPreviewFrameUrl"></iframe>
        </div>
      </div>
    </Transition>
  </Teleport>
</template>

<script setup lang="ts">
import { ref, computed, watch } from 'vue'
import {
  getBookSources,
  deleteBookSource,
  deleteBookSources,
  deleteInvalidBookSources,
  loginBookSource,
  saveBookSource,
  saveBookSources,
  testBookSources,
  readRemoteSourceFile,
  readSourceFile,
  exportBookSources,
  pinBookSource,
  unpinBookSource,
} from '../api/source'
import { useAppStore } from '../stores/app'
import { useSourceStore } from '../stores/source'
import type { BookSource } from '../types'
import {
  filterBookSources,
  getBookSourceGroups,
  getBookSourceStats,
  getVisibleSelection,
  toBookSourceDeletePayload,
} from '../utils/sourceSelection'
import { chunkBookSourceUrls, mergeBookSourceTestResponses } from '../utils/sourceTesting'
import SourceEditorPanel from './source-manager/SourceEditorPanel.vue'
import SourceFilterBar from './source-manager/SourceFilterBar.vue'
import SourceList from './source-manager/SourceList.vue'
import SourceManagerHeader from './source-manager/SourceManagerHeader.vue'
import SourceSubscriptionPanel from './source-manager/SourceSubscriptionPanel.vue'

type SourceSubscription = {
  url: string
  lastSyncedAt?: number
}

const SUBSCRIPTION_KEY = 'reader-source-subscriptions'

const props = defineProps<{
  modelValue: boolean
}>()

const emit = defineEmits<{
  'update:modelValue': [value: boolean]
}>()

const appStore = useAppStore()
const sourceStore = useSourceStore()

const sources = ref<BookSource[]>([])
const loading = ref(false)
const testingSources = ref(false)
const filterText = ref('')
const filterGroup = ref('')
const fileInputRef = ref<HTMLInputElement | null>(null)
const selectedSourceUrls = ref(new Set<string>())

const subscriptionPanelVisible = ref(false)
const remoteUrl = ref('')
const subscriptions = ref<SourceSubscription[]>(loadSubscriptions())

const editingSource = ref<BookSource | null>(null)
const editorText = ref(JSON.stringify(createEmptySource(), null, 2))
const resetSignal = ref(0)
const sourceLoginLoading = ref(false)
const loginPreviewVisible = ref(false)
const loginPreviewUrl = ref('')
const loginPreviewFrameUrl = ref('')

const groupList = computed(() => getBookSourceGroups(sources.value))

const filteredSources = computed(() =>
  filterBookSources(sources.value, filterText.value, filterGroup.value)
)

const sourceStats = computed(() => getBookSourceStats(sources.value, filteredSources.value))

const selectedFilteredSources = computed(() =>
  getVisibleSelection(filteredSources.value, selectedSourceUrls.value, (source) => source.bookSourceUrl)
)

const invalidSources = computed(() =>
  sources.value.filter((source) => hasSourceGroup(source, '失效'))
)

const allFilteredSelected = computed(() =>
  filteredSources.value.length > 0 && selectedFilteredSources.value.length === filteredSources.value.length
)

const partiallyFilteredSelected = computed(() =>
  selectedFilteredSources.value.length > 0 && !allFilteredSelected.value
)

const canLoginSource = computed(() => {
  try {
    const parsed = JSON.parse(editorText.value) as BookSource
    return Boolean(parsed.bookSourceUrl?.trim() && parsed.loginUrl?.trim())
  } catch {
    return false
  }
})

function createEmptySource(): BookSource {
  return {
    bookSourceName: '新增书源',
    bookSourceUrl: '',
    enabled: true,
  }
}

function loadSubscriptions(): SourceSubscription[] {
  try {
    const raw = localStorage.getItem(SUBSCRIPTION_KEY)
    return raw ? JSON.parse(raw) : []
  } catch {
    return []
  }
}

function persistSubscriptions() {
  localStorage.setItem(SUBSCRIPTION_KEY, JSON.stringify(subscriptions.value))
}

async function loadSources() {
  loading.value = true
  try {
    sources.value = await getBookSources()
    pruneSelection()
  } catch (e: unknown) {
    appStore.showToast((e as Error).message, 'error')
  } finally {
    loading.value = false
    // Keep the shared source store in sync so the search page's group/source
    // dropdowns reflect add/delete/edit done here.
    sourceStore.fetchSources().catch(() => undefined)
  }
}

async function toggleSource(source: BookSource) {
  const next = { ...source, enabled: source.enabled === false ? true : false }
  try {
    await saveBookSource(next)
    Object.assign(source, next)
    appStore.showToast('书源状态已更新', 'success')
  } catch (e: unknown) {
    appStore.showToast((e as Error).message, 'error')
  }
}

async function pinSource(source: BookSource) {
  try {
    await pinBookSource(source.bookSourceUrl)
    appStore.showToast(`已置顶「${source.bookSourceName}」`, 'success')
    await loadSources()
  } catch (e: unknown) {
    appStore.showToast((e as Error).message, 'error')
  }
}

async function unpinSource(source: BookSource) {
  try {
    await unpinBookSource(source.bookSourceUrl)
    appStore.showToast(`已取消置顶「${source.bookSourceName}」`, 'success')
    await loadSources()
  } catch (e: unknown) {
    appStore.showToast((e as Error).message, 'error')
  }
}

async function removeSource(source: BookSource) {
  const ok = await appStore.confirmDialog(`确定删除书源 "${source.bookSourceName}"？`, { title: '删除书源', danger: true })
  if (!ok) return
  try {
    await deleteBookSource(source.bookSourceUrl)
    sources.value = sources.value.filter((s) => s.bookSourceUrl !== source.bookSourceUrl)
    selectedSourceUrls.value.delete(source.bookSourceUrl)
    if (editingSource.value?.bookSourceUrl === source.bookSourceUrl) {
      createSource()
    }
    appStore.showToast('已删除', 'success')
    sourceStore.fetchSources().catch(() => undefined)
  } catch (e: unknown) {
    appStore.showToast((e as Error).message, 'error')
  }
}

async function removeSelectedSources() {
  const targets = selectedFilteredSources.value
  if (!targets.length) {
    appStore.showToast('请先选择要删除的书源', 'warning')
    return
  }
  const ok = await appStore.confirmDialog(`确定删除选中的 ${targets.length} 个书源？`, { title: '删除书源(批量)', danger: true })
  if (!ok) return
  try {
    await deleteBookSources(toBookSourceDeletePayload(targets))
    const targetUrls = new Set(targets.map((source) => source.bookSourceUrl))
    sources.value = sources.value.filter((source) => !targetUrls.has(source.bookSourceUrl))
    selectedSourceUrls.value.clear()
    if (editingSource.value && targetUrls.has(editingSource.value.bookSourceUrl)) {
      createSource()
    }
    appStore.showToast(`已删除 ${targets.length} 个书源`, 'success')
    sourceStore.fetchSources().catch(() => undefined)
  } catch (e: unknown) {
    appStore.showToast((e as Error).message || '批量删除失败', 'error')
  }
}

async function testSources() {
  const targets = selectedFilteredSources.value.length ? selectedFilteredSources.value : sources.value
  if (!targets.length) {
    appStore.showToast('没有可测试的书源', 'warning')
    return
  }
  const scopeText = selectedFilteredSources.value.length ? `选中的 ${targets.length}` : `全部 ${targets.length}`
  const ok = await appStore.confirmDialog(`将测试${scopeText} 个书源。测试会请求外部站点，耗时可能较长，是否继续？`, { title: '测试书源' })
  if (!ok) return

  testingSources.value = true
  try {
    const batches = chunkBookSourceUrls(targets.map((source) => source.bookSourceUrl))
    const responses = []
    for (const batch of batches) {
      responses.push(
        await testBookSources({
          bookSourceUrls: batch,
          markInvalid: true,
          concurrent: 12,
        })
      )
    }
    const result = mergeBookSourceTestResponses(responses)
    await loadSources()
    if (result.invalid > 0) {
      filterGroup.value = '失效'
    }
    appStore.showToast(
      `测试完成：有效 ${result.valid} 个，失效 ${result.invalid} 个，更新分组 ${result.markedInvalid} 个`,
      result.invalid > 0 ? 'warning' : 'success'
    )
  } catch (e: unknown) {
    appStore.showToast((e as Error).message || '书源测试失败', 'error')
  } finally {
    testingSources.value = false
  }
}

async function removeInvalidSources() {
  if (!invalidSources.value.length) {
    appStore.showToast('没有失效书源', 'warning')
    return
  }
  const ok = await appStore.confirmDialog(`确定删除 ${invalidSources.value.length} 个失效书源？此操作不可撤销。`, { title: '删除失效书源', danger: true })
  if (!ok) return
  try {
    const result = await deleteInvalidBookSources()
    const invalidUrls = new Set(invalidSources.value.map((source) => source.bookSourceUrl))
    sources.value = sources.value.filter((source) => !invalidUrls.has(source.bookSourceUrl))
    selectedSourceUrls.value.clear()
    if (editingSource.value && invalidUrls.has(editingSource.value.bookSourceUrl)) {
      createSource()
    }
    appStore.showToast(`已删除 ${result.deleted} 个失效书源`, 'success')
  } catch (e: unknown) {
    appStore.showToast((e as Error).message || '删除失效书源失败', 'error')
  }
}

function toggleSourceSelection(source: BookSource) {
  const selected = selectedSourceUrls.value
  if (selected.has(source.bookSourceUrl)) {
    selected.delete(source.bookSourceUrl)
  } else {
    selected.add(source.bookSourceUrl)
  }
}

function toggleFilteredSelection() {
  const selected = selectedSourceUrls.value
  if (allFilteredSelected.value) {
    filteredSources.value.forEach((source) => selected.delete(source.bookSourceUrl))
    return
  }
  filteredSources.value.forEach((source) => selected.add(source.bookSourceUrl))
}

function clearSelection() {
  selectedSourceUrls.value.clear()
}

function pruneSelection() {
  const availableUrls = new Set(sources.value.map((source) => source.bookSourceUrl))
  Array.from(selectedSourceUrls.value).forEach((url) => {
    if (!availableUrls.has(url)) {
      selectedSourceUrls.value.delete(url)
    }
  })
}

function hasSourceGroup(source: BookSource, groupName: string) {
  return (source.bookSourceGroup || '')
    .split(/[,;；、]/)
    .map((group) => group.trim())
    .filter(Boolean)
    .includes(groupName)
}

function createSource() {
  editingSource.value = null
  editorText.value = JSON.stringify(createEmptySource(), null, 2)
  resetSignal.value += 1
}

function editSource(source: BookSource) {
  editingSource.value = source
  editorText.value = JSON.stringify(source, null, 2)
}

function formatEditor() {
  try {
    const parsed = JSON.parse(editorText.value)
    editorText.value = JSON.stringify(parsed, null, 2)
  } catch {
    appStore.showToast('JSON 格式错误，无法格式化', 'error')
  }
}

async function saveEditor() {
  try {
    const raw = JSON.parse(editorText.value)
    const parsed = (Array.isArray(raw) ? raw : [raw]) as BookSource[]
    if (!parsed.length) {
      throw new Error('书源内容为空')
    }
    for (const source of parsed) {
      if (!source.bookSourceName?.trim()) {
        throw new Error('书源名称不能为空')
      }
      if (!source.bookSourceUrl?.trim()) {
        throw new Error('书源链接不能为空')
      }
    }
    await saveBookSources(parsed)
    appStore.showToast(`保存书源成功（${parsed.length} 个）`, 'success')
    await loadSources()
    const latest = sources.value.find((item) => item.bookSourceUrl === parsed[0].bookSourceUrl)
    if (latest) {
      editSource(latest)
    }
  } catch (e: unknown) {
    appStore.showToast((e as Error).message || '保存失败', 'error')
  }
}

async function handleSourceLogin() {
  try {
    const parsed = JSON.parse(editorText.value) as BookSource
    if (!parsed.bookSourceUrl?.trim()) {
      throw new Error('书源链接不能为空')
    }
    if (!parsed.loginUrl?.trim()) {
      throw new Error('当前书源未配置 loginUrl')
    }

    sourceLoginLoading.value = true
    if (!editingSource.value || editingSource.value.bookSourceUrl !== parsed.bookSourceUrl) {
      await saveBookSource(parsed)
      await loadSources()
      const latest = sources.value.find((item) => item.bookSourceUrl === parsed.bookSourceUrl)
      if (latest) {
        editSource(latest)
      }
    }

    const result = await loginBookSource(parsed.bookSourceUrl)
    const check = typeof result.checkResult === 'string' && result.checkResult.trim()
      ? `，校验结果：${result.checkResult}`
      : ''
    if (result.url?.trim()) {
      loginPreviewUrl.value = result.url
      loginPreviewFrameUrl.value = buildLoginProxyUrl(parsed.bookSourceUrl, result.url)
      loginPreviewVisible.value = true
    }
    appStore.showToast(`书源登录请求已完成，状态 ${result.status}${check}`, 'success')
  } catch (e: unknown) {
    appStore.showToast((e as Error).message || '书源登录失败', 'error')
  } finally {
    sourceLoginLoading.value = false
  }
}

function buildLoginProxyUrl(bookSourceUrl: string, targetUrl: string) {
  const params = new URLSearchParams()
  params.set('bookSourceUrl', bookSourceUrl)
  params.set('url', targetUrl)
  return `http://reader.localhost/bookSourceProxy?${params.toString()}`
}

function triggerFileImport() {
  fileInputRef.value?.click()
}

async function handleFileImport(event: Event) {
  const input = event.target as HTMLInputElement
  const file = input.files?.[0]
  if (!file) return
  try {
    const imported = await readSourceFile(file)
    if (!imported.length) {
      throw new Error('文件中没有可导入的书源')
    }
    await saveBookSources(imported)
    appStore.showToast(`成功导入 ${imported.length} 个书源`, 'success')
    await loadSources()
  } catch (e: unknown) {
    appStore.showToast((e as Error).message || '导入失败', 'error')
  } finally {
    input.value = ''
  }
}

async function importRemoteSource() {
  if (!remoteUrl.value) {
    appStore.showToast('请输入远程书源链接', 'warning')
    return
  }
  try {
    const raw = await readRemoteSourceFile(remoteUrl.value)
    const parsed = raw.flatMap((item) => {
      try {
        const value = JSON.parse(item)
        return Array.isArray(value) ? value : [value]
      } catch {
        return []
      }
    }) as BookSource[]
    if (!parsed.length) {
      throw new Error('远程书源文件为空或格式错误')
    }
    await saveBookSources(parsed)
    touchSubscription(remoteUrl.value)
    appStore.showToast(`成功同步 ${parsed.length} 个书源`, 'success')
    await loadSources()
  } catch (e: unknown) {
    appStore.showToast((e as Error).message || '远程同步失败', 'error')
  }
}

function saveSubscription() {
  if (!remoteUrl.value) {
    appStore.showToast('请输入远程书源链接', 'warning')
    return
  }
  if (!subscriptions.value.find((item) => item.url === remoteUrl.value)) {
    subscriptions.value.unshift({ url: remoteUrl.value })
    persistSubscriptions()
    appStore.showToast('订阅已保存', 'success')
  }
}

async function syncSubscription(url: string) {
  remoteUrl.value = url
  await importRemoteSource()
}

function removeSubscription(url: string) {
  subscriptions.value = subscriptions.value.filter((item) => item.url !== url)
  persistSubscriptions()
}

function touchSubscription(url: string) {
  const existing = subscriptions.value.find((item) => item.url === url)
  if (existing) {
    existing.lastSyncedAt = Date.now()
  } else {
    subscriptions.value.unshift({ url, lastSyncedAt: Date.now() })
  }
  persistSubscriptions()
}

async function exportSources() {
  const payload = JSON.stringify(sources.value, null, 2)
  if (typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window) {
    try {
      const result = await exportBookSources(sources.value)
      if (result.cancelled) return
      appStore.showToast('书源已导出', 'success')
      return
    } catch (error) {
      appStore.showToast((error as Error).message || '导出失败', 'error')
      return
    }
  }
  const blob = new Blob([payload], { type: 'application/json;charset=utf-8' })
  const url = URL.createObjectURL(blob)
  const anchor = document.createElement('a')
  anchor.href = url
  anchor.download = `reader-book-sources-${formatDateForFile()}.json`
  anchor.click()
  URL.revokeObjectURL(url)
}

function formatDateForFile() {
  const date = new Date()
  const pad = (v: number) => `${v}`.padStart(2, '0')
  return `${date.getFullYear()}${pad(date.getMonth() + 1)}${pad(date.getDate())}-${pad(date.getHours())}${pad(date.getMinutes())}${pad(date.getSeconds())}`
}

function close() {
  emit('update:modelValue', false)
}

watch(() => props.modelValue, (v) => {
  if (v) {
    if (sources.value.length === 0) {
      loadSources()
    }
    if (!editingSource.value && !editorText.value.trim()) {
      createSource()
    }
  }
}, { immediate: true })
</script>

<style scoped>
.modal-overlay {
  position: fixed;
  inset: 0;
  background: rgba(0, 0, 0, 0.5);
  z-index: var(--z-overlay);
  backdrop-filter: blur(4px);
}

.modal-container {
  position: fixed;
  inset: 0;
  z-index: var(--z-modal);
  display: flex;
  align-items: center;
  justify-content: center;
  padding:
    calc(var(--space-6) + var(--safe-area-top))
    calc(var(--space-6) + var(--safe-area-right))
    calc(var(--space-6) + var(--safe-area-bottom))
    calc(var(--space-6) + var(--safe-area-left));
}

.source-modal {
  width: min(1180px, 100%);
  height: min(780px, calc(var(--app-height, 100dvh) - var(--safe-area-top) - var(--safe-area-bottom) - 32px));
  max-height: min(88vh, calc(var(--app-height, 100dvh) - var(--safe-area-top) - var(--safe-area-bottom) - 32px));
  background: var(--color-bg-elevated);
  border-radius: var(--radius-xl);
  display: flex;
  flex-direction: column;
  box-shadow: var(--shadow-xl);
  overflow: hidden;
}

.hidden-input {
  display: none;
}

.content-grid {
  display: grid;
  grid-template-columns: minmax(320px, 42%) minmax(420px, 58%);
  gap: 14px;
  min-height: 0;
  flex: 1;
  padding: 14px 24px 0;
  overflow: hidden;
}

.source-list-slot,
.editor-slot {
  min-height: 0;
}

.modal-footer {
  min-height: 44px;
  display: flex;
  align-items: center;
  justify-content: flex-start;
  gap: 12px;
  padding: 8px 24px 14px;
  flex-shrink: 0;
}

.count-info {
  font-size: 12px;
  color: var(--color-text-tertiary);
}

.mini-btn,
.icon-btn {
  border: 1px solid var(--color-border);
  border-radius: var(--radius-md);
  background: transparent;
  transition: all var(--duration-fast);
}

.mini-btn {
  min-height: 32px;
  padding: 0 10px;
  font-size: 12px;
  white-space: nowrap;
}

.mini-btn.danger {
  color: var(--color-danger);
  border-color: rgba(245, 34, 45, 0.2);
}

.icon-btn {
  width: 36px;
  height: 36px;
  display: flex;
  align-items: center;
  justify-content: center;
  color: var(--color-text-secondary);
}

.icon-btn svg {
  width: 18px;
  height: 18px;
}

.mini-btn:hover,
.icon-btn:hover {
  background: var(--color-bg-hover);
}

.login-preview-container {
  position: fixed;
  inset: 0;
  z-index: calc(var(--z-modal) + 2);
  display: flex;
  align-items: center;
  justify-content: center;
  padding: 24px;
  background: rgba(0, 0, 0, 0.35);
}

.login-preview-modal {
  width: min(980px, 100%);
  height: min(86vh, 900px);
  background: var(--color-bg-elevated);
  border-radius: var(--radius-xl);
  overflow: hidden;
  display: flex;
  flex-direction: column;
  box-shadow: var(--shadow-xl);
}

.login-preview-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 16px;
  padding: 16px 20px;
  border-bottom: 1px solid var(--color-border-light);
}

.login-preview-header h3 {
  margin: 0;
  font-size: 16px;
  font-weight: 700;
}

.login-preview-header p {
  margin-top: 6px;
  font-size: 12px;
  color: var(--color-text-tertiary);
  word-break: break-all;
}

.login-preview-frame {
  flex: 1;
  width: 100%;
  border: none;
  background: #fff;
}

@media (max-width: 900px) {
  .modal-container {
    align-items: stretch;
    padding: 8px;
  }

  .source-modal {
    width: 100%;
    height: calc(var(--app-height, 100dvh) - 16px);
    max-height: calc(var(--app-height, 100dvh) - 16px);
    border-radius: var(--radius-xl);
    overflow-y: auto;
    -webkit-overflow-scrolling: touch;
  }

  .content-grid {
    display: flex;
    flex-direction: column;
    overflow: visible;
    flex: none;
    padding-left: 16px;
    padding-right: 16px;
  }

  .editor-slot {
    order: -1;
    min-height: 360px;
    max-height: min(58vh, 560px);
  }

  .source-list-slot {
    max-height: min(46vh, 460px);
  }

  .modal-footer {
    padding: 10px 16px 14px;
    background: var(--color-bg-elevated);
    border-top: 1px solid var(--color-border-light);
  }
}

@media (max-width: 420px) {
  .modal-container {
    padding:
      calc(8px + var(--safe-area-top))
      calc(8px + var(--safe-area-right))
      calc(8px + var(--safe-area-bottom))
      calc(8px + var(--safe-area-left));
  }

  .source-modal {
    border-radius: var(--radius-lg);
  }
}
</style>
