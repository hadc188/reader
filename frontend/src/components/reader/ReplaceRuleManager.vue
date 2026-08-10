<template>
  <div class="rule-manager" :style="{ background: theme.popup, color: theme.fontColor }">
    <div class="rule-header">
      <h3>内容净化规则</h3>
      <div class="header-actions">
        <button class="add-btn" @click="openEditModal()">+ 新增规则</button>
        <button class="close-btn" @click="store.backPanel()">
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
            <path d="m15 18-6-6 6-6" />
          </svg>
        </button>
      </div>
    </div>

    <div class="rule-list">
      <div v-if="loading" class="loading">加载中...</div>
      <div v-else-if="!store.replaceRules.length" class="empty">
        暂无自定义规则
        <p>规则按“排序”从小到大依次执行</p>
      </div>
      <div
        v-else
        v-for="rule in sortedRules"
        :key="rule.id"
        class="rule-item"
        :class="{ disabled: !rule.isEnabled }"
      >
        <div class="rule-info">
          <div class="rule-name-row">
            <span class="rule-name">{{ rule.name }}</span>
            <span class="rule-badge" v-if="rule.isRegex">正则</span>
          </div>
          <div class="rule-scope">{{ describeScope(rule.scope) }}</div>
          <div class="rule-pattern">匹配: {{ rule.pattern }}</div>
          <div class="rule-replace">替换为: {{ rule.replacement || '(空)' }}</div>
        </div>
        <div class="rule-ops">
          <label class="switch">
            <input type="checkbox" v-model="rule.isEnabled" @change="toggleRule(rule)">
            <span class="slider"></span>
          </label>
          <button class="op-btn" @click="openEditModal(rule)">编辑</button>
          <button class="op-btn delete" @click="handleDelete(rule)">删除</button>
        </div>
      </div>
    </div>

    <!-- Edit Modal (Simplified for now as a sub-panel or use a real modal) -->
    <Transition name="fade">
      <div v-if="editingRule" class="edit-overlay" :style="{ background: theme.popup }">
        <div class="edit-header">
          <h4>{{ editingRule.id ? '编辑规则' : '新增规则' }}</h4>
        </div>
        <div class="edit-body">
          <div class="form-item">
            <label>名称</label>
            <input v-model="editingRule.name" placeholder="起个名字" />
          </div>
          <div class="form-item">
            <label>匹配项 (Pattern)</label>
            <textarea v-model="editingRule.pattern" placeholder="文本或正则表达式"></textarea>
          </div>
          <div class="form-item">
            <label>替换为 (Replacement)</label>
            <input v-model="editingRule.replacement" placeholder="留空则为删除" />
          </div>
          <div class="form-item">
            <label>生效范围</label>
            <div class="scope-group">
              <button type="button" class="scope-btn" :class="{ active: getScopeMode(editingRule) === 'global' }" @click="setScopeMode('global')">全部书籍</button>
              <button type="button" class="scope-btn" :class="{ active: getScopeMode(editingRule) === 'book' }" @click="setScopeMode('book')">当前书籍</button>
              <button type="button" class="scope-btn" :class="{ active: getScopeMode(editingRule) === 'source' }" @click="setScopeMode('source')">当前书源</button>
            </div>
            <div class="scope-preview">{{ describeScope(editingRule.scope) }}</div>
          </div>
          <div class="form-row">
            <label><input type="checkbox" v-model="editingRule.isRegex"> 正则模式</label>
            <label>排序: <input class="order-input" type="number" v-model="editingRule.order"></label>
          </div>
        </div>
        <div class="edit-footer">
          <button @click="editingRule = null">取消</button>
          <button class="primary" @click="handleSave">保存</button>
        </div>
      </div>
    </Transition>
  </div>
</template>

<script setup lang="ts">
import { ref, computed, onMounted } from 'vue'
import { useReaderStore } from '../../stores/reader'
import { useAppStore } from '../../stores/app'
import { saveReplaceRule, deleteReplaceRule } from '../../api/replaceRule'
import type { ReplaceRule } from '../../types'

const store = useReaderStore()
const appStore = useAppStore()
const theme = computed(() => store.currentTheme)
const loading = ref(false)
const editingRule = ref<ReplaceRule | null>(null)

const sortedRules = computed(() => {
  return [...store.replaceRules].sort((a, b) => a.order - b.order)
})

function describeScope(scope?: string) {
  if (!scope || scope === '*') return '范围: 全部书籍'
  if (scope.startsWith('source:')) {
    return `范围: 当前书源 (${scope.slice('source:'.length)})`
  }
  if (scope.startsWith('book:')) {
    return `范围: 当前书籍 (${scope.slice('book:'.length)})`
  }
  return `范围: ${scope}`
}

function getScopeMode(rule: ReplaceRule) {
  const scope = rule.scope || '*'
  if (scope.startsWith('source:')) return 'source'
  if (scope.startsWith('book:')) return 'book'
  return 'global'
}

onMounted(async () => {
  loading.value = true
  await store.fetchReplaceRules()
  loading.value = false
})

function openEditModal(rule?: ReplaceRule) {
  if (rule) {
    editingRule.value = { ...rule }
  } else {
    editingRule.value = {
      id: 0,
      name: '',
      pattern: '',
      replacement: '',
      scope: '*',
      isEnabled: true,
      isRegex: false,
      order: store.replaceRules.length + 1
    }
  }
}

function setScopeMode(mode: 'global' | 'book' | 'source') {
  if (!editingRule.value) return
  if (mode === 'global') {
    editingRule.value.scope = '*'
    return
  }
  if (!store.book) return
  editingRule.value.scope = mode === 'source'
    ? `source:${store.book.origin}`
    : `book:${store.book.bookUrl}`
}

async function handleSave() {
  if (!editingRule.value) return
  if (!editingRule.value.name || !editingRule.value.pattern) {
    alert('请填写名称和匹配项')
    return
  }
  try {
    await saveReplaceRule(editingRule.value)
    await store.fetchReplaceRules()
    editingRule.value = null
  } catch (e: any) {
    alert(e.message)
  }
}

async function toggleRule(rule: ReplaceRule) {
  try {
    await saveReplaceRule(rule)
  } catch (e: any) {
    alert(e.message)
    rule.isEnabled = !rule.isEnabled
  }
}

async function handleDelete(rule: ReplaceRule) {
  const ok = await appStore.confirmDialog(`确定删除规则 "${rule.name}" 吗？`, { title: '删除规则', danger: true })
  if (!ok) return
  try {
    await deleteReplaceRule(rule)
    await store.fetchReplaceRules()
  } catch (e: any) {
    alert(e.message)
  }
}
</script>

<style scoped>
.rule-manager {
  display: flex;
  flex-direction: column;
  height: 100%;
  position: relative;
}

.rule-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 16px 20px;
  border-bottom: 1px solid rgba(0,0,0,0.06);
  flex-shrink: 0;
}

.rule-header h3 { margin: 0; font-size: 16px; }

.header-actions { display: flex; gap: 12px; align-items: center; }

.add-btn {
  background: var(--color-primary);
  color: white;
  border: none;
  padding: 6px 12px;
  border-radius: 6px;
  font-size: 13px;
  cursor: pointer;
}

.close-btn {
  width: 32px;
  height: 32px;
  display: flex;
  align-items: center;
  justify-content: center;
  border-radius: 8px;
  color: inherit;
  opacity: 0.6;
  background: transparent;
  border: none;
  cursor: pointer;
}

.rule-list {
  flex: 1;
  overflow-y: auto;
  padding: 12px;
  display: flex;
  flex-direction: column;
  gap: 12px;
  -webkit-overflow-scrolling: touch;
  overscroll-behavior: contain;
}

.loading, .empty {
  padding: 40px;
  text-align: center;
  opacity: 0.5;
  font-size: 14px;
}

.rule-item {
  padding: 12px;
  border-radius: 12px;
  background: rgba(0,0,0,0.03);
  border: 1px solid rgba(0,0,0,0.05);
  display: flex;
  justify-content: space-between;
  align-items: flex-start;
  transition: opacity 0.2s;
}

.rule-item.disabled { opacity: 0.5; }

.rule-info { flex: 1; min-width: 0; }

.rule-name-row { display: flex; align-items: center; gap: 8px; margin-bottom: 4px; }
.rule-name { font-weight: 600; font-size: 14px; }
.rule-badge { font-size: 10px; background: var(--color-primary); color: white; padding: 1px 4px; border-radius: 3px; }
.rule-scope { font-size: 12px; color: var(--color-primary); opacity: 0.9; margin-bottom: 4px; }

.rule-pattern, .rule-replace {
  font-size: 12px;
  opacity: 0.6;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
  margin-top: 2px;
}

.rule-ops { display: flex; flex-direction: column; align-items: flex-end; gap: 8px; }

.op-btn {
  font-size: 12px;
  background: transparent;
  border: 1px solid var(--color-border);
  padding: 2px 8px;
  border-radius: 4px;
  cursor: pointer;
  color: inherit;
}

.op-btn.delete { color: #ef4444; border-color: rgba(239, 68, 68, 0.2); }

/* Switch style */
.switch { position: relative; display: inline-block; width: 34px; height: 18px; }
.switch input { opacity: 0; width: 0; height: 0; }
.slider { position: absolute; cursor: pointer; top: 0; left: 0; right: 0; bottom: 0; background-color: #ccc; transition: .4s; border-radius: 18px; }
.slider:before { position: absolute; content: ""; height: 14px; width: 14px; left: 2px; bottom: 2px; background-color: white; transition: .4s; border-radius: 50%; }
input:checked + .slider { background-color: var(--color-primary); }
input:checked + .slider:before { transform: translateX(16px); }

/* Edit Overlay */
.edit-overlay {
  position: absolute;
  top: 0;
  left: 0;
  right: 0;
  bottom: 0;
  z-index: 10;
  display: flex;
  flex-direction: column;
  padding: 20px;
  overflow-y: auto;
  -webkit-overflow-scrolling: touch;
}

.edit-header h4 { margin: 0 0 16px 0; }

.edit-body { flex: 1; display: flex; flex-direction: column; gap: 16px; }

.form-item { display: flex; flex-direction: column; gap: 6px; }
.form-item label { font-size: 12px; opacity: 0.7; }
.form-item input, .form-item textarea {
  background: color-mix(in srgb, currentColor 7%, transparent);
  border: 1px solid color-mix(in srgb, currentColor 18%, transparent);
  padding: 8px 12px;
  border-radius: 8px;
  color: inherit;
  outline: none;
  font-size: 14px;
}
.form-item textarea { height: 80px; resize: none; }

.scope-group {
  display: flex;
  gap: 8px;
  flex-wrap: wrap;
}

.scope-btn {
  border: 1px solid rgba(0,0,0,0.1);
  background: transparent;
  color: inherit;
  padding: 8px 12px;
  border-radius: 999px;
  cursor: pointer;
  font-size: 12px;
}

.scope-btn.active {
  background: var(--color-primary);
  color: white;
  border-color: var(--color-primary);
}

.scope-preview {
  font-size: 12px;
  opacity: 0.65;
}

.form-row { display: flex; align-items: center; gap: 20px; font-size: 13px; }

.order-input {
  width: 64px;
  min-height: 32px;
  padding: 5px 8px;
  border: 1px solid color-mix(in srgb, currentColor 20%, transparent);
  border-radius: 8px;
  outline: none;
  background: color-mix(in srgb, currentColor 8%, transparent);
  color: inherit;
  color-scheme: inherit;
  font-variant-numeric: tabular-nums;
}

.order-input:focus {
  border-color: var(--color-primary);
  box-shadow: 0 0 0 3px rgba(201, 127, 58, 0.16);
}

.edit-footer { display: flex; justify-content: flex-end; gap: 12px; margin-top: 20px; }
.edit-footer button { padding: 8px 20px; border-radius: 8px; border: 1px solid var(--color-border); background: transparent; color: inherit; cursor: pointer; }
.edit-footer button.primary { background: var(--color-primary); color: white; border: none; }
</style>
