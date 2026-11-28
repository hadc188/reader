<template>
  <SettingsRuleDialog
    :model-value="modelValue"
    title="TXT 目录规则"
    subtitle="按排序尝试启用规则"
    @update:model-value="emit('update:modelValue', $event)"
  >
    <div class="rule-toolbar">
      <span>{{ rules.length }} 条规则</span>
      <div class="toolbar-actions">
        <button class="secondary-button" type="button" @click="openImport">导入</button>
        <button class="primary-button" type="button" @click="editRule()">新增</button>
      </div>
    </div>
    <div class="rule-body">
      <p v-if="loading" class="rule-state">正在读取规则…</p>
      <p v-else-if="!rules.length" class="rule-state">还没有自定义规则</p>
      <article v-for="rule in rules" v-else :key="rule.id" class="rule-row" :class="{ muted: !rule.enable }">
        <button
          class="switch-button"
          type="button"
          role="switch"
          :aria-checked="rule.enable"
          :aria-label="`启用 ${rule.name}`"
          :class="{ on: rule.enable }"
          @click="toggleRule(rule)"
        ></button>
        <div class="rule-copy">
          <strong>{{ rule.name }}</strong>
          <code>{{ rule.rule || '默认分章' }}</code>
          <small>{{ rule.example || '无示例' }}</small>
        </div>
        <div class="rule-actions">
          <button type="button" @click="editRule(rule)">编辑</button>
          <button type="button" class="danger" @click="removeRule(rule)">删除</button>
        </div>
      </article>
    </div>

    <div v-if="editing" class="rule-editor">
      <header>
        <button class="back-button" type="button" aria-label="返回" @click="editing = null">
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="m15 18-6-6 6-6" /></svg>
        </button>
        <h3>{{ editing.id ? '编辑规则' : '新增规则' }}</h3>
      </header>
      <div class="editor-body">
        <label>
          <span>名称</span>
          <input v-model="editing.name" placeholder="例如 数字章节">
        </label>
        <label>
          <span>正则表达式</span>
          <textarea v-model="editing.rule" rows="6" placeholder="(?m)^0?\\d{1,5}[、.．,:：_—\\-\\s]"></textarea>
        </label>
        <label>
          <span>示例</span>
          <input v-model="editing.example" placeholder="01、初见">
        </label>
        <div class="editor-row">
          <label class="inline-check"><input v-model="editing.enable" type="checkbox">启用</label>
          <label class="order-field">
            <span>排序</span>
            <input v-model.number="editing.order" type="number" min="1">
          </label>
        </div>
      </div>
      <footer>
        <button class="secondary-button" type="button" @click="editing = null">取消</button>
        <button type="button" class="primary-button" :disabled="saving" @click="saveRule">
          {{ saving ? '保存中…' : '保存' }}
        </button>
      </footer>
    </div>

    <div v-if="importing" class="rule-editor">
      <header>
        <button class="back-button" type="button" aria-label="返回" @click="closeImport">
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="m15 18-6-6 6-6" /></svg>
        </button>
        <h3>导入规则</h3>
      </header>
      <div class="editor-body">
        <label>
          <span>规则 JSON 或链接</span>
          <textarea v-model="importSource" rows="3" placeholder="https://example.test/txt-toc-rules.json"></textarea>
        </label>
        <div class="import-actions">
          <button class="primary-button" type="button" :disabled="importBusy" @click="importFromSource">
            {{ importBusy ? '导入中…' : '网络导入' }}
          </button>
          <button class="secondary-button" type="button" :disabled="importBusy" @click="fileInputRef?.click()">选择本地文件</button>
          <input ref="fileInputRef" type="file" accept=".json,application/json" hidden @change="importLocalFile">
        </div>
      </div>
    </div>

    <div v-if="importCandidates.length" class="rule-editor">
      <header>
        <button class="back-button" type="button" aria-label="返回" @click="closeImport">
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="m15 18-6-6 6-6" /></svg>
        </button>
        <h3>选择导入规则</h3>
      </header>
      <div class="editor-body">
        <button class="select-all" type="button" @click="toggleSelectAll">
          {{ selectedCandidates.length === importCandidates.length ? '取消全选' : '全选' }}
          {{ selectedCandidates.length }} / {{ importCandidates.length }}
        </button>
        <label v-for="candidate in importCandidates" :key="`${candidate.id}-${candidate.name}`" class="import-candidate">
          <input v-model="candidate.selected" type="checkbox">
          <span class="candidate-copy">
            <strong>{{ candidate.name || '未命名规则' }}</strong>
            <small>{{ candidate.importState === 'new' ? '新增' : candidate.importState === 'update' ? '更新' : '已有' }}</small>
          </span>
        </label>
      </div>
      <footer>
        <button class="secondary-button" type="button" @click="closeImport">取消</button>
        <button class="primary-button" type="button" :disabled="importBusy || !selectedCandidates.length" @click="applyImport">
          导入 {{ selectedCandidates.length }} 条
        </button>
      </footer>
    </div>
  </SettingsRuleDialog>
</template>

<script setup lang="ts">
import { computed, onMounted, ref, watch } from 'vue'
import { useAppStore } from '../stores/app'
import { deleteTxtTocRule, getTxtTocRules, saveTxtTocRule, type TxtTocRule } from '../api/txtTocRule'
import SettingsRuleDialog from './SettingsRuleDialog.vue'
import {
  markImportedRules,
  parseImportedRuleList,
  readRuleImportText,
} from '../utils/ruleImport'

const props = defineProps<{ modelValue: boolean }>()
const emit = defineEmits<{ 'update:modelValue': [value: boolean] }>()
const appStore = useAppStore()
const rules = ref<TxtTocRule[]>([])
const loading = ref(false)
const saving = ref(false)
const editing = ref<TxtTocRule | null>(null)
const importing = ref(false)
const importBusy = ref(false)
const importSource = ref('')
const fileInputRef = ref<HTMLInputElement | null>(null)
const importCandidates = ref<Array<TxtTocRule & { selected: boolean; importState: 'new' | 'update' | 'same' }>>([])
const selectedCandidates = computed(() => importCandidates.value.filter((rule) => rule.selected))

async function fetchRules() {
  loading.value = true
  try {
    rules.value = await getTxtTocRules()
  } catch (error) {
    appStore.showToast((error as Error).message || '读取规则失败', 'error')
  } finally {
    loading.value = false
  }
}

watch(() => props.modelValue, (visible) => {
  if (visible) void fetchRules()
  else closeImport()
})

onMounted(() => {
  if (props.modelValue) void fetchRules()
})

function editRule(rule?: TxtTocRule) {
  editing.value = rule
    ? { ...rule }
    : { id: 0, name: '', rule: '', example: '', enable: true, order: rules.value.length + 1 }
}

function openImport() {
  editing.value = null
  importSource.value = ''
  importCandidates.value = []
  importing.value = true
}

function closeImport() {
  importing.value = false
  importCandidates.value = []
  importSource.value = ''
}

async function importFromSource() {
  await prepareImport(importSource.value)
}

async function importLocalFile(event: Event) {
  const input = event.target as HTMLInputElement
  const file = input.files?.[0]
  if (!file) return
  try {
    await prepareImport('', file)
  } finally {
    input.value = ''
  }
}

async function prepareImport(source: string, file?: File) {
  if (importBusy.value) return
  importBusy.value = true
  try {
    const text = await readRuleImportText(source, file)
    const raw = JSON.parse(text)
    const incoming = parseImportedRuleList<TxtTocRule>(raw).map((rule, index) => ({
      ...rule,
      id: Number(rule.id) || -Date.now() - index - 1,
      name: String(rule.name || '').trim(),
      rule: String(rule.rule || ''),
      example: String(rule.example || ''),
      enable: rule.enable !== false,
      order: Number(rule.order) || 0,
    }))
    const valid = incoming.filter((rule) => rule.name)
    if (!valid.length) throw new Error('没有可导入的目录规则')
    importCandidates.value = markImportedRules(valid, rules.value)
    importing.value = false
  } catch (error) {
    appStore.showToast((error as Error).message || '导入失败', 'error')
  } finally {
    importBusy.value = false
  }
}

function toggleSelectAll() {
  const select = selectedCandidates.value.length !== importCandidates.value.length
  importCandidates.value.forEach((rule) => {
    rule.selected = select
  })
}

async function applyImport() {
  const selected = selectedCandidates.value
  if (!selected.length || importBusy.value) return
  importBusy.value = true
  try {
    for (const rule of selected) {
      const { selected: _selected, importState: _importState, ...payload } = rule
      await saveTxtTocRule(payload)
    }
    closeImport()
    await fetchRules()
    appStore.showToast(`已导入 ${selected.length} 条目录规则`, 'success')
  } catch (error) {
    appStore.showToast((error as Error).message || '导入失败', 'error')
  } finally {
    importBusy.value = false
  }
}

async function saveRule() {
  const rule = editing.value
  if (!rule) return
  if (!rule.name.trim()) {
    appStore.showToast('请填写规则名称', 'warning')
    return
  }
  saving.value = true
  try {
    await saveTxtTocRule(rule)
    editing.value = null
    await fetchRules()
  } catch (error) {
    appStore.showToast((error as Error).message || '保存失败', 'error')
  } finally {
    saving.value = false
  }
}

async function toggleRule(rule: TxtTocRule) {
  rule.enable = !rule.enable
  try {
    await saveTxtTocRule(rule)
    await fetchRules()
  } catch (error) {
    appStore.showToast((error as Error).message || '规则状态保存失败', 'error')
    await fetchRules()
  }
}

async function removeRule(rule: TxtTocRule) {
  const confirmed = await appStore.confirmDialog(`确定删除规则 "${rule.name}" 吗？`, {
    title: '删除规则',
    danger: true,
  })
  if (!confirmed) return
  try {
    await deleteTxtTocRule(rule.id)
    await fetchRules()
  } catch (error) {
    appStore.showToast((error as Error).message || '删除失败', 'error')
  }
}
</script>

<style scoped>
.rule-toolbar,
.toolbar-actions,
.editor-row {
  display: flex;
  align-items: center;
  gap: 12px;
}

.rule-toolbar,
.editor-row {
  justify-content: space-between;
}

.toolbar-actions,
.import-actions {
  display: flex;
  align-items: center;
  flex-wrap: wrap;
  justify-content: flex-end;
  gap: 10px;
}

.import-actions {
  justify-self: start;
  justify-content: flex-start;
}

.rule-toolbar {
  flex: 0 0 auto;
  padding: 12px 24px;
  border-bottom: 1px solid var(--color-divider);
  color: var(--color-text-tertiary);
  font-size: 12px;
}

.rule-body {
  flex: 1;
  min-height: 0;
  overflow-y: auto;
  padding: 16px 24px 24px;
}

.rule-state {
  padding: 32px 0;
  text-align: center;
  color: var(--color-text-tertiary);
}

.rule-row {
  display: grid;
  grid-template-columns: auto minmax(0, 1fr) auto;
  align-items: center;
  gap: 14px;
  padding: 14px 0;
  border-bottom: 1px solid var(--color-divider);
}

.rule-row.muted {
  background: color-mix(in srgb, currentColor 2.5%, transparent);
}

.rule-row.muted .rule-copy {
  opacity: 0.56;
}

.switch-button {
  position: relative;
  width: 38px;
  height: 22px;
  flex: 0 0 auto;
  border-radius: 999px;
  background: var(--color-border);
}

.switch-button::after {
  content: "";
  position: absolute;
  top: 2px;
  left: 2px;
  width: 18px;
  height: 18px;
  border-radius: 50%;
  background: #fff;
  box-shadow: 0 1px 3px rgba(0, 0, 0, 0.3);
  transition: transform 0.18s;
}

.switch-button.on {
  background: var(--color-primary);
}

.switch-button.on::after {
  transform: translateX(16px);
}

.rule-copy {
  min-width: 0;
  display: grid;
  gap: 5px;
}

.rule-copy strong {
  font-size: var(--text-sm);
}

.rule-copy code {
  overflow: hidden;
  color: var(--color-text-secondary);
  font-size: 11px;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.rule-copy small {
  color: var(--color-text-tertiary);
  font-size: 11px;
}

.rule-actions {
  display: flex;
  align-items: center;
  gap: 2px;
}

.rule-actions button {
  min-height: 28px;
  padding: 4px 8px;
  border-radius: var(--radius-sm);
  color: var(--color-primary);
  font-size: 12px;
  transition: background 0.15s ease, color 0.15s ease;
}

.rule-actions button:hover {
  background: var(--color-bg-sunken);
}

.rule-actions button:active {
  background: color-mix(in srgb, var(--color-primary) 12%, transparent);
}

.rule-actions .danger {
  color: var(--color-danger);
}

.primary-button {
  min-height: 34px;
  padding: 0 14px;
  border: 1px solid color-mix(in srgb, var(--color-primary) 82%, transparent);
  border-radius: 8px;
  background: var(--color-primary);
  color: #fff;
  font-size: 12px;
  font-weight: 600;
  line-height: 1;
  white-space: nowrap;
  box-shadow: 0 2px 8px color-mix(in srgb, var(--color-primary) 20%, transparent);
  transition: filter 0.15s ease, transform 0.15s ease, opacity 0.15s ease;
}

.primary-button:hover:not(:disabled) {
  filter: brightness(1.06);
}

.primary-button:active:not(:disabled) {
  transform: translateY(1px);
}

.primary-button:disabled {
  opacity: 0.5;
  cursor: not-allowed;
  box-shadow: none;
}

.secondary-button {
  min-height: 34px;
  padding: 0 12px;
  border: 1px solid var(--color-border);
  border-radius: 8px;
  background: transparent;
  color: var(--color-text-secondary);
  font-size: 12px;
  line-height: 1;
  white-space: nowrap;
  transition: background 0.15s ease, border-color 0.15s ease, color 0.15s ease;
}

.secondary-button:hover:not(:disabled) {
  border-color: var(--color-border-light);
  background: var(--color-bg-sunken);
  color: var(--color-text);
}

.secondary-button:active:not(:disabled) {
  transform: translateY(1px);
}

.secondary-button:disabled {
  opacity: 0.45;
  cursor: not-allowed;
}

.rule-editor {
  position: absolute;
  inset: 0;
  z-index: 2;
  display: flex;
  flex-direction: column;
  background: var(--color-bg-elevated);
}

.rule-editor header {
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 18px 24px;
  border-bottom: 1px solid var(--color-divider);
}

.rule-editor h3 {
  margin: 0;
  font-size: var(--text-md);
}

.back-button {
  width: 32px;
  height: 32px;
  display: grid;
  place-items: center;
  border-radius: 8px;
  color: var(--color-text-secondary);
  transition: background 0.15s ease, color 0.15s ease;
}

.back-button:hover {
  background: var(--color-bg-sunken);
  color: var(--color-text);
}

.back-button svg {
  width: 18px;
  height: 18px;
}

.editor-body {
  flex: 1;
  overflow-y: auto;
  display: grid;
  align-content: start;
  gap: 16px;
  padding: 20px 24px;
}

.editor-body label {
  display: grid;
  gap: 7px;
  color: var(--color-text-secondary);
  font-size: var(--text-sm);
}

.editor-body input,
.editor-body textarea {
  min-height: 38px;
  padding: 8px 10px;
  border: 1px solid var(--color-border);
  border-radius: var(--radius-md);
  background: var(--color-bg);
  color: var(--color-text);
}

.select-all {
  justify-self: start;
  min-height: 30px;
  padding: 0 9px;
  border: 1px solid transparent;
  border-radius: 7px;
  color: var(--color-primary);
  font-size: 12px;
  font-weight: 500;
  transition: background 0.15s ease, border-color 0.15s ease;
}

.select-all:hover {
  background: var(--color-bg-sunken);
  border-color: var(--color-border-light);
}

.select-all:active {
  background: color-mix(in srgb, var(--color-primary) 12%, transparent);
}

.import-candidate {
  display: flex !important;
  grid-template-columns: auto minmax(0, 1fr);
  align-items: center;
  gap: 10px !important;
  padding: 9px 0;
  border-bottom: 1px solid var(--color-divider);
}

.candidate-copy {
  min-width: 0;
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 10px;
}

.candidate-copy strong {
  overflow: hidden;
  font-weight: 500;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.candidate-copy small {
  flex: 0 0 auto;
  color: var(--color-text-tertiary);
  font-size: 10px;
}

.editor-body textarea {
  resize: vertical;
  font-family: 'Cascadia Mono', Consolas, monospace;
}

.inline-check {
  display: flex !important;
  grid-template-columns: auto 1fr;
  align-items: center;
}

.switch-button:hover {
  filter: brightness(1.08);
}

.switch-button:active {
  transform: scale(0.96);
}

.order-field {
  grid-template-columns: auto 76px;
  align-items: center;
}

footer {
  display: flex;
  flex-wrap: wrap;
  justify-content: flex-end;
  gap: 10px;
  padding: 16px 24px;
  border-top: 1px solid var(--color-divider);
}

@media (max-width: 600px) {
  .rule-toolbar,
  .rule-body,
  .rule-editor header,
  .editor-body,
  footer {
    padding-left: 16px;
    padding-right: 16px;
  }

  .rule-row {
    grid-template-columns: auto minmax(0, 1fr);
  }

  .rule-actions {
    grid-column: 2;
  }

  .rule-actions button,
  .select-all {
    min-height: 36px;
  }

  .back-button {
    width: 38px;
    height: 38px;
  }

  .switch-button {
    width: 44px;
    height: 26px;
  }

  .switch-button::after {
    width: 22px;
    height: 22px;
  }

  .switch-button.on::after {
    transform: translateX(18px);
  }
}
</style>
