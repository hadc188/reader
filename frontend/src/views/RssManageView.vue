<template>
  <div class="rss-manage-view">
    <header class="rss-manage-hero">
      <div>
        <h1>RSS 源管理</h1>
        <p>共 {{ store.sources.length }} 个，已启用 {{ enabledCount }} 个，当前筛选 {{ filteredSources.length }} 个。</p>
      </div>
      <div class="rss-manage-actions">
        <button class="ghost-btn" @click="goBack">返回 RSS</button>
        <button class="ghost-btn" @click="refreshAll">刷新</button>
        <button class="ghost-btn" @click="exportSources">导出 JSON</button>
      </div>
    </header>

    <section class="rss-manage-tools">
      <button class="tool-btn" @click="triggerFileImport">本地导入 JSON</button>
      <div class="remote-import-group">
        <input v-model.trim="remoteJsonUrl" class="remote-input" placeholder="输入 Legado RSS JSON 链接" />
        <button class="tool-btn" @click="importRemoteJson">远程导入 JSON</button>
      </div>
      <button class="tool-btn" :class="{ primary: !editingSource }" @click="createSource">新增 RSS 源</button>
      <button class="tool-btn danger" :disabled="!selectedFilteredSources.length" @click="handleBulkDelete">
        批量删除{{ selectedFilteredSources.length ? ` (${selectedFilteredSources.length})` : '' }}
      </button>
      <input ref="fileInputRef" type="file" accept=".json,.txt" class="hidden-input" @change="handleFileImport" />
    </section>

    <section class="rss-manage-layout">
      <aside class="rss-sources">
        <div class="panel-title-row compact">
          <div class="panel-title">RSS 源 ({{ filteredSources.length }})</div>
          <button v-if="selectedFilteredSources.length" class="mini-btn" @click="clearSelection">清空选择</button>
        </div>
        <div class="filter-row">
          <label class="bulk-check" :class="{ muted: !filteredSources.length }">
            <input
              type="checkbox"
              :checked="allFilteredSelected"
              :disabled="!filteredSources.length"
              :aria-checked="partiallyFilteredSelected ? 'mixed' : allFilteredSelected"
              @change="toggleFilteredSelection"
            />
            <span>{{ allFilteredSelected ? '取消全选' : '全选当前' }}</span>
          </label>
          <input v-model.trim="filterText" placeholder="搜索源名称或链接" />
          <select v-model="filterGroup">
            <option value="">全部分组</option>
            <option v-for="group in groupList" :key="group" :value="group">{{ group }}</option>
          </select>
        </div>

        <div v-if="!filteredSources.length" class="empty-box">
          {{ store.sources.length ? '没有匹配的 RSS 源' : '暂无 RSS 源' }}
        </div>
        <div
          v-for="source in filteredSources"
          :key="source.sourceUrl"
          class="source-item"
          role="button"
          tabindex="0"
          :class="{
            active: editingSource?.sourceUrl === source.sourceUrl,
            selected: selectedSourceUrls.has(source.sourceUrl),
          }"
          @click="editSource(source)"
          @keydown.enter="editSource(source)"
          @keydown.space.prevent="editSource(source)"
        >
          <label class="row-check" @click.stop>
            <input
              type="checkbox"
              :checked="selectedSourceUrls.has(source.sourceUrl)"
              @change="toggleSourceSelection(source)"
            />
          </label>
          <div class="source-main">
            <div class="source-name-row">
              <span class="source-name">{{ source.sourceName }}</span>
              <span v-if="source.sourceGroup" class="source-group">{{ source.sourceGroup }}</span>
            </div>
            <div class="source-url">{{ source.sourceUrl }}</div>
          </div>
          <div class="source-actions">
            <label class="toggle">
              <input type="checkbox" :checked="source.enabled !== false" @change.stop="toggleSource(source)" />
              <span class="toggle-slider"></span>
            </label>
            <button class="mini-btn danger" @click.stop="handleDelete(source)">删除</button>
          </div>
        </div>
        <div v-if="selectedFilteredSources.length" class="bulk-action-card">
          <span>已选择 {{ selectedFilteredSources.length }} 个当前结果</span>
          <button class="mini-btn danger" @click="handleBulkDelete">删除选中</button>
        </div>
      </aside>

      <section class="rss-editor">
        <div class="panel-title-row">
          <div class="panel-title">{{ editingSource ? '编辑 RSS 源' : '新增 RSS 源' }}</div>
          <div class="panel-actions">
            <button class="ghost-btn" @click="resetEditor">重置</button>
            <button class="ghost-btn primary" @click="saveEditor">保存</button>
          </div>
        </div>

        <div class="visual-form">
          <label>
            <span>源名称</span>
            <input v-model.trim="formState.sourceName" placeholder="请输入 RSS 名称" />
          </label>
          <label>
            <span>源链接</span>
            <input v-model.trim="formState.sourceUrl" placeholder="请输入 RSS 链接" />
          </label>
          <label class="group-field">
            <span>分组</span>
            <input v-model.trim="formState.sourceGroup" placeholder="多个分组可用逗号分隔" />
          </label>
          <div class="group-chip-row" v-if="groupList.length">
            <button
              v-for="group in groupList"
              :key="group"
              class="group-chip-btn"
              :class="{ active: currentEditorGroups.includes(group) }"
              @click="toggleEditorGroup(group)"
            >
              {{ group }}
            </button>
          </div>

          <label>
            <span>排序链接</span>
            <input v-model.trim="formState.sortUrl" placeholder="为空时默认使用源链接" />
          </label>
          <label>
            <span>图标链接</span>
            <input v-model.trim="formState.sourceIcon" placeholder="可选" />
          </label>
          <label class="full-span">
            <span>备注</span>
            <textarea v-model.trim="formState.sourceComment" rows="2" placeholder="可选备注"></textarea>
          </label>

          <label class="switch-field">
            <span>启用源</span>
            <input v-model="formState.enabled" type="checkbox" />
          </label>
          <label class="switch-field">
            <span>启用 CookieJar</span>
            <input v-model="formState.enabledCookieJar" type="checkbox" />
          </label>
          <label class="switch-field">
            <span>启用 JS</span>
            <input v-model="formState.enableJs" type="checkbox" />
          </label>
          <label class="switch-field">
            <span>单链接模式</span>
            <input v-model="formState.singleUrl" type="checkbox" />
          </label>
          <label class="switch-field">
            <span>基于源地址补全</span>
            <input v-model="formState.loadWithBaseUrl" type="checkbox" />
          </label>

          <label>
            <span>并发频率</span>
            <input v-model.trim="formState.concurrentRate" placeholder="可选，如 1/1000" />
          </label>
          <label>
            <span>自定义排序</span>
            <input v-model.number="formState.customOrder" type="number" placeholder="0" />
          </label>
          <label>
            <span>文章样式</span>
            <input v-model.number="formState.articleStyle" type="number" placeholder="0" />
          </label>

          <label class="full-span">
            <span>请求头</span>
            <textarea v-model.trim="formState.header" rows="3" placeholder="可选，JSON 字符串"></textarea>
          </label>
          <label class="full-span">
            <span>登录链接</span>
            <input v-model.trim="formState.loginUrl" placeholder="可选" />
          </label>
          <label class="full-span">
            <span>登录校验 JS</span>
            <textarea v-model.trim="formState.loginCheckJs" rows="2" placeholder="可选"></textarea>
          </label>

          <label class="full-span">
            <span>文章列表规则</span>
            <textarea v-model.trim="formState.ruleArticles" rows="2" placeholder="ruleArticles"></textarea>
          </label>
          <label>
            <span>下一页规则</span>
            <input v-model.trim="formState.ruleNextPage" placeholder="ruleNextPage" />
          </label>
          <label>
            <span>标题规则</span>
            <input v-model.trim="formState.ruleTitle" placeholder="ruleTitle" />
          </label>
          <label>
            <span>时间规则</span>
            <input v-model.trim="formState.rulePubDate" placeholder="rulePubDate" />
          </label>
          <label>
            <span>摘要规则</span>
            <input v-model.trim="formState.ruleDescription" placeholder="ruleDescription" />
          </label>
          <label>
            <span>图片规则</span>
            <input v-model.trim="formState.ruleImage" placeholder="ruleImage" />
          </label>
          <label>
            <span>链接规则</span>
            <input v-model.trim="formState.ruleLink" placeholder="ruleLink" />
          </label>
          <label class="full-span">
            <span>正文规则</span>
            <textarea v-model.trim="formState.ruleContent" rows="3" placeholder="ruleContent"></textarea>
          </label>
          <label class="full-span">
            <span>样式</span>
            <textarea v-model.trim="formState.style" rows="2" placeholder="可选"></textarea>
          </label>
        </div>
      </section>
    </section>
  </div>
</template>

<script setup lang="ts">
import { computed, onMounted, reactive, ref, watch } from 'vue'
import { useRouter } from 'vue-router'
import { readRemoteRssSourceFile, readRssSourceFile, saveRssSource } from '../api/rss'
import { useAppStore } from '../stores/app'
import { useRssStore } from '../stores/rss'
import type { RssSource } from '../types'
import { getVisibleSelection } from '../utils/sourceSelection'

const router = useRouter()
const appStore = useAppStore()
const store = useRssStore()

const fileInputRef = ref<HTMLInputElement | null>(null)
const remoteJsonUrl = ref('')
const filterText = ref('')
const filterGroup = ref('')
const editingSource = ref<RssSource | null>(null)
const formState = reactive<RssSource>(createEmptySource())
const selectedSourceUrls = ref(new Set<string>())

const groupList = computed(() => {
  const groups = new Set<string>()
  store.sources.forEach((source) => {
    if (source.sourceGroup) {
      source.sourceGroup.split(/[,;，、]/).forEach((item) => {
        const trimmed = item.trim()
        if (trimmed) groups.add(trimmed)
      })
    }
  })
  return Array.from(groups).sort()
})

const currentEditorGroups = computed(() => (formState.sourceGroup || '').split(/[,;，、]/).map((item) => item.trim()).filter(Boolean))

const filteredSources = computed(() => {
  let list = store.sources
  if (filterText.value) {
    const keyword = filterText.value.toLowerCase()
    list = list.filter((source) =>
      source.sourceName.toLowerCase().includes(keyword) ||
      source.sourceUrl.toLowerCase().includes(keyword),
    )
  }
  if (filterGroup.value) {
    list = list.filter((source) => source.sourceGroup?.includes(filterGroup.value))
  }
  return list
})

const selectedFilteredSources = computed(() =>
  getVisibleSelection(filteredSources.value, selectedSourceUrls.value, (source) => source.sourceUrl)
)

const allFilteredSelected = computed(() =>
  filteredSources.value.length > 0 && selectedFilteredSources.value.length === filteredSources.value.length
)

const partiallyFilteredSelected = computed(() =>
  selectedFilteredSources.value.length > 0 && !allFilteredSelected.value
)

const enabledCount = computed(() => store.sources.filter((source) => source.enabled !== false).length)

onMounted(async () => {
  await store.fetchSources()
  if (store.sources.length) {
    editSource(store.sources[0])
  } else {
    resetEditor()
  }
})

function createEmptySource(): RssSource {
  return {
    sourceName: '新增 RSS 源',
    sourceUrl: '',
    enabled: true,
    enabledCookieJar: true,
    enableJs: true,
    loadWithBaseUrl: true,
    singleUrl: true,
    sourceGroup: '',
    sourceIcon: '',
    customOrder: 0,
    articleStyle: 0,
    lastUpdateTime: 0,
  }
}

function replaceFormState(next: RssSource) {
  Object.assign(formState, createEmptySource(), next)
}

function toggleEditorGroup(group: string) {
  const groups = new Set(currentEditorGroups.value)
  if (groups.has(group)) {
    groups.delete(group)
  } else {
    groups.add(group)
  }
  formState.sourceGroup = Array.from(groups).join(', ')
}

async function refreshAll() {
  await store.fetchSources()
}

function goBack() {
  router.push('/rss')
}

function editSource(source: RssSource) {
  editingSource.value = source
  replaceFormState(source)
}

function createSource() {
  editingSource.value = null
  replaceFormState(createEmptySource())
}

function resetEditor() {
  if (editingSource.value) {
    replaceFormState(editingSource.value)
    return
  }
  createSource()
}

async function saveEditor() {
  try {
    const parsed = {
      ...createEmptySource(),
      ...formState,
      sourceName: formState.sourceName?.trim() || '',
      sourceUrl: formState.sourceUrl?.trim() || '',
      sourceGroup: formState.sourceGroup?.trim() || '',
      sortUrl: formState.sortUrl?.trim() || '',
      sourceIcon: formState.sourceIcon?.trim() || '',
      sourceComment: formState.sourceComment?.trim() || '',
      header: formState.header?.trim() || '',
      loginUrl: formState.loginUrl?.trim() || '',
      loginCheckJs: formState.loginCheckJs?.trim() || '',
      ruleArticles: formState.ruleArticles?.trim() || '',
      ruleNextPage: formState.ruleNextPage?.trim() || '',
      ruleTitle: formState.ruleTitle?.trim() || '',
      rulePubDate: formState.rulePubDate?.trim() || '',
      ruleDescription: formState.ruleDescription?.trim() || '',
      ruleImage: formState.ruleImage?.trim() || '',
      ruleLink: formState.ruleLink?.trim() || '',
      ruleContent: formState.ruleContent?.trim() || '',
      style: formState.style?.trim() || '',
    } as RssSource
    if (!parsed.sourceName?.trim()) throw new Error('RSS 名称不能为空')
    if (!parsed.sourceUrl?.trim()) throw new Error('RSS 链接不能为空')
    await saveRssSource(parsed)
    await store.fetchSources()
    const latest = store.sources.find((item) => item.sourceUrl === parsed.sourceUrl)
    if (latest) {
      editSource(latest)
    }
    appStore.showToast('RSS 源已保存', 'success')
  } catch (error) {
    appStore.showToast((error as Error).message || '保存失败', 'error')
  }
}

async function toggleSource(source: RssSource) {
  const next = { ...source, enabled: source.enabled === false ? true : false }
  await saveRssSource(next)
  Object.assign(source, next)
  if (editingSource.value?.sourceUrl === source.sourceUrl) {
    editSource(next)
  }
  appStore.showToast('RSS 源状态已更新', 'success')
}

async function handleDelete(source: RssSource) {
  const ok = await appStore.confirmDialog(`确定删除 RSS 源 "${source.sourceName}"？`, { title: '删除RSS源', danger: true })
  if (!ok) return
  await store.removeSource(source)
  selectedSourceUrls.value.delete(source.sourceUrl)
  if (editingSource.value?.sourceUrl === source.sourceUrl) {
    createSource()
  }
  appStore.showToast('RSS 源已删除', 'success')
}

async function handleBulkDelete() {
  const targets = selectedFilteredSources.value
  if (!targets.length) {
    appStore.showToast('请先选择要删除的 RSS 源', 'warning')
    return
  }
  const ok = await appStore.confirmDialog(`确定删除选中的 ${targets.length} 个 RSS 源？`, { title: '批量删除RSS', danger: true })
  if (!ok) return
  try {
    const targetUrls = new Set(targets.map((source) => source.sourceUrl))
    const deleted = await store.removeSources(targets)
    selectedSourceUrls.value.clear()
    if (editingSource.value && targetUrls.has(editingSource.value.sourceUrl)) {
      createSource()
    }
    appStore.showToast(`已删除 ${deleted || targets.length} 个 RSS 源`, 'success')
  } catch (error) {
    appStore.showToast((error as Error).message || '批量删除失败', 'error')
  }
}

function toggleSourceSelection(source: RssSource) {
  const selected = selectedSourceUrls.value
  if (selected.has(source.sourceUrl)) {
    selected.delete(source.sourceUrl)
  } else {
    selected.add(source.sourceUrl)
  }
}

function toggleFilteredSelection() {
  const selected = selectedSourceUrls.value
  if (allFilteredSelected.value) {
    filteredSources.value.forEach((source) => selected.delete(source.sourceUrl))
    return
  }
  filteredSources.value.forEach((source) => selected.add(source.sourceUrl))
}

function clearSelection() {
  selectedSourceUrls.value.clear()
}

function pruneSelection() {
  const availableUrls = new Set(store.sources.map((source) => source.sourceUrl))
  Array.from(selectedSourceUrls.value).forEach((url) => {
    if (!availableUrls.has(url)) {
      selectedSourceUrls.value.delete(url)
    }
  })
}

function triggerFileImport() {
  fileInputRef.value?.click()
}

async function handleFileImport(event: Event) {
  const input = event.target as HTMLInputElement
  const file = input.files?.[0]
  if (!file) return
  try {
    const imported = await readRssSourceFile(file)
    if (!imported.length) throw new Error('文件中没有可导入的 RSS 源')
    await store.addSources(imported)
    appStore.showToast(`成功导入 ${imported.length} 个 RSS 源`, 'success')
  } catch (error) {
    appStore.showToast((error as Error).message || '导入失败', 'error')
  } finally {
    input.value = ''
  }
}

async function importRemoteJson() {
  if (!remoteJsonUrl.value) {
    appStore.showToast('请输入远程 RSS JSON 链接', 'warning')
    return
  }
  try {
    const raw = await readRemoteRssSourceFile(remoteJsonUrl.value)
    const parsed = raw.flatMap((item) => {
      try {
        const value = JSON.parse(item)
        return Array.isArray(value) ? value : [value]
      } catch {
        return []
      }
    }) as RssSource[]
    if (!parsed.length) throw new Error('远程 RSS JSON 为空或格式错误')
    await store.addSources(parsed)
    appStore.showToast(`成功导入 ${parsed.length} 个 RSS 源`, 'success')
  } catch (error) {
    appStore.showToast((error as Error).message || '远程导入失败', 'error')
  }
}

function exportSources() {
  const blob = new Blob([JSON.stringify(store.sources, null, 2)], { type: 'application/json;charset=utf-8' })
  const url = URL.createObjectURL(blob)
  const anchor = document.createElement('a')
  anchor.href = url
  anchor.download = `reader-rss-sources-${Date.now()}.json`
  anchor.click()
  URL.revokeObjectURL(url)
}

watch(() => store.sources, pruneSelection)
</script>

<style scoped>
.rss-manage-view {
  height: calc(var(--app-height, 100dvh) - var(--header-height) - 104px - var(--safe-area-top) - var(--safe-area-bottom));
  min-height: calc(var(--app-height, 100dvh) - var(--header-height) - 104px - var(--safe-area-top) - var(--safe-area-bottom));
  box-sizing: border-box;
  padding: 24px;
  display: flex;
  flex-direction: column;
  gap: 16px;
  overflow: hidden;
}

.rss-manage-hero,
.rss-manage-tools,
.rss-sources,
.rss-editor {
  background: var(--color-bg-elevated);
  border: 1px solid var(--color-border-light);
  border-radius: 24px;
}

.rss-manage-hero,
.rss-manage-tools {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
  flex-wrap: wrap;
  padding: 18px 20px;
}

.rss-manage-hero h1 {
  margin: 0;
}

.rss-manage-hero p {
  margin: 6px 0 0;
  color: var(--color-text-tertiary);
}

.rss-manage-actions,
.panel-actions,
.source-actions {
  display: flex;
  gap: 8px;
  flex-wrap: wrap;
}

.rss-manage-tools {
  justify-content: flex-start;
}

.remote-import-group {
  display: flex;
  align-items: center;
  gap: 8px;
  flex: 1;
  min-width: min(520px, 100%);
}

.remote-input,
.filter-row input,
.filter-row select,
.visual-form input,
.visual-form textarea {
  border: 1px solid var(--color-border);
  background: var(--color-bg);
  border-radius: 12px;
  padding: 10px 12px;
}

.bulk-check,
.row-check {
  display: inline-flex;
  align-items: center;
  gap: 8px;
  color: var(--color-text-secondary);
  font-size: 13px;
}

.bulk-check {
  padding: 10px 12px;
  border: 1px solid var(--color-border);
  border-radius: 12px;
  background: var(--color-bg);
}

.bulk-check.muted {
  opacity: 0.5;
}

.bulk-check input,
.row-check input {
  width: 16px;
  height: 16px;
  accent-color: var(--color-primary);
}

.remote-input {
  flex: 1;
  min-width: 0;
}

.rss-manage-layout {
  flex: 1;
  min-height: 0;
  display: grid;
  grid-template-columns: 320px minmax(0, 1fr);
  gap: 16px;
}

.rss-sources,
.rss-editor {
  min-height: 0;
  overflow: auto;
  padding: 16px;
  box-sizing: border-box;
}

.visual-form {
  display: grid;
  grid-template-columns: repeat(2, minmax(0, 1fr));
  gap: 12px;
}

.visual-form label {
  display: flex;
  flex-direction: column;
  gap: 6px;
  min-width: 0;
}

.visual-form label > span {
  font-size: 13px;
  font-weight: 600;
  color: var(--color-text-secondary);
}

.visual-form textarea {
  resize: vertical;
  min-height: 80px;
}

.full-span {
  grid-column: 1 / -1;
}

.group-chip-row {
  grid-column: 1 / -1;
  display: flex;
  gap: 8px;
  flex-wrap: wrap;
  margin-top: -2px;
}

.group-chip-btn {
  padding: 8px 12px;
  border-radius: 999px;
  border: 1px solid var(--color-border-light);
  background: var(--color-bg);
  color: var(--color-text-secondary);
}

.group-chip-btn.active {
  border-color: rgba(201, 127, 58, 0.26);
  background: rgba(201, 127, 58, 0.1);
  color: var(--color-primary);
}

.switch-field {
  flex-direction: row !important;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
  padding: 10px 12px;
  border: 1px solid var(--color-border-light);
  border-radius: 14px;
  background: var(--color-bg);
}

.switch-field input {
  width: 18px;
  height: 18px;
}

.panel-title {
  font-size: 14px;
  font-weight: 700;
  margin-bottom: 12px;
}

.panel-title-row {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
  margin-bottom: 12px;
}

.panel-title-row .panel-title {
  margin-bottom: 0;
}

.panel-title-row.compact .mini-btn {
  padding: 6px 10px;
  font-size: 12px;
}

.filter-row,
.visual-form {
  display: grid;
  gap: 10px;
  margin-bottom: 14px;
}

.visual-form label {
  display: grid;
  gap: 6px;
}

.visual-form span {
  font-size: 13px;
  color: var(--color-text-tertiary);
}

.group-chip-btn,
.tool-btn,
.ghost-btn,
.mini-btn {
  padding: 10px 16px;
  border-radius: 999px;
  border: 1px solid var(--color-border-light);
  background: var(--color-bg);
  transition: all var(--duration-fast) var(--ease-out);
}

.group-chip-btn.active,
.tool-btn.primary,
.ghost-btn.primary {
  background: rgba(201, 127, 58, 0.12);
  color: var(--color-primary);
  border-color: rgba(201, 127, 58, 0.2);
}

.tool-btn.danger {
  background: rgba(245, 34, 45, 0.05);
  color: var(--color-danger);
  border-color: rgba(245, 34, 45, 0.18);
}

.tool-btn:disabled,
.mini-btn:disabled {
  cursor: not-allowed;
  opacity: 0.48;
}

.source-item {
  width: 100%;
  display: flex;
  align-items: flex-start;
  gap: 10px;
  cursor: pointer;
  text-align: left;
  border: 1px solid transparent;
  background: var(--color-bg);
  border-radius: 16px;
  padding: 14px;
  margin-bottom: 10px;
  transition: all var(--duration-fast) var(--ease-out);
}

.source-item.active {
  border-color: rgba(201, 127, 58, 0.26);
  background: rgba(201, 127, 58, 0.08);
}

.source-item:focus-visible {
  outline: 2px solid var(--color-primary-border);
  outline-offset: 2px;
}

.source-item.selected {
  border-color: rgba(201, 127, 58, 0.3);
  background: rgba(201, 127, 58, 0.12);
  box-shadow: inset 3px 0 0 var(--color-primary);
}

.source-main {
  flex: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
  gap: 8px;
}

.source-name-row {
  display: flex;
  align-items: center;
  gap: 8px;
  flex-wrap: wrap;
}

.source-name {
  font-weight: 700;
}

.source-group {
  padding: 2px 8px;
  border-radius: 999px;
  background: rgba(201, 127, 58, 0.12);
  color: var(--color-primary);
  font-size: 12px;
}

.source-url {
  color: var(--color-text-tertiary);
  font-size: 12px;
  line-height: 1.5;
  word-break: break-all;
}

.source-actions {
  margin-top: 0;
  min-width: 74px;
  justify-content: space-between;
  align-items: center;
}

.mini-btn.danger {
  color: var(--color-danger);
}

.bulk-action-card {
  position: sticky;
  bottom: 0;
  z-index: 1;
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
  margin-top: 10px;
  padding: 10px 12px;
  border: 1px solid rgba(201, 127, 58, 0.22);
  border-radius: 16px;
  background: color-mix(in srgb, var(--color-bg-elevated) 90%, var(--color-primary) 10%);
  box-shadow: var(--shadow-sm);
  color: var(--color-text-secondary);
  font-size: 13px;
}

@media (max-width: 960px) {
  .rss-manage-view {
    padding: 12px;
    gap: 12px;
  }

  .rss-manage-layout {
    grid-template-columns: 1fr;
  }

  .rss-sources {
    max-height: 32dvh;
  }

  .remote-import-group {
    min-width: 100%;
  }

  .bulk-action-card {
    align-items: stretch;
    flex-direction: column;
  }
}

.empty-box {
  min-height: 140px;
  display: flex;
  align-items: center;
  justify-content: center;
  text-align: center;
  color: var(--color-text-tertiary);
}

.hidden-input {
  display: none;
}

.toggle {
  position: relative;
  display: inline-flex;
  align-items: center;
}

.toggle input {
  display: none;
}

.toggle-slider {
  width: 42px;
  height: 24px;
  border-radius: 999px;
  background: rgba(0, 0, 0, 0.14);
  position: relative;
}

.toggle-slider::after {
  content: '';
  position: absolute;
  top: 3px;
  left: 3px;
  width: 18px;
  height: 18px;
  border-radius: 50%;
  background: white;
  transition: transform var(--duration-fast) var(--ease-out);
}

.toggle input:checked + .toggle-slider {
  background: rgba(201, 127, 58, 0.38);
}

.toggle input:checked + .toggle-slider::after {
  transform: translateX(18px);
}

@media (max-width: 960px) {
  .rss-manage-layout {
    grid-template-columns: 1fr;
  }
}
</style>
