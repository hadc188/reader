<template>
  <Teleport to="body">
    <Transition name="fade">
      <div v-if="modelValue" class="modal-overlay" @click="$emit('update:modelValue', false)"></div>
    </Transition>
    <Transition name="scale">
      <div v-if="modelValue" class="modal-container" @click.self="$emit('update:modelValue', false)">
        <div class="modal-card">
          <div class="modal-header">
            <h3>分组管理</h3>
            <button class="close-btn" @click="$emit('update:modelValue', false)">
              <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M18 6 6 18M6 6l12 12" /></svg>
            </button>
          </div>

          <div class="modal-body">
            <div class="create-row">
              <input v-model.trim="newGroupName" class="group-input" placeholder="新建分组名称" @keyup.enter="createGroup" />
              <button class="primary-btn" :disabled="!newGroupName" @click="createGroup">新建</button>
            </div>

            <div class="group-list">
              <div
                v-for="group in shelfStore.groups"
                :key="group.groupId"
                class="group-item"
                :class="{ 'is-selected': selectedIds.has(group.groupId) }"
              >
                <label class="group-check">
                  <input
                    type="checkbox"
                    :checked="selectedIds.has(group.groupId)"
                    :aria-label="`选择分组 ${group.groupName}`"
                    @change="toggleSelect(group.groupId)"
                  />
                </label>
                <input
                  v-model.trim="editingNames[group.groupId]"
                  class="group-name-input"
                  :aria-label="`分组名称 ${group.groupName}`"
                  @keyup.enter="renameGroup(group.groupId)"
                />
                <div class="group-actions">
                  <button class="ghost-btn" @click="renameGroup(group.groupId)">保存</button>
                  <button class="ghost-btn danger" @click="deleteGroup(group.groupId, group.groupName)">删除</button>
                </div>
              </div>
              <p v-if="!shelfStore.groups.length" class="empty-hint">还没有分组</p>
            </div>
          </div>

          <div v-if="shelfStore.groups.length" class="modal-footer">
            <label class="select-all">
              <input
                type="checkbox"
                :checked="allSelected"
                :indeterminate.prop="someSelected && !allSelected"
                @change="toggleSelectAll"
              />
              <span>全选</span>
            </label>
            <span class="selection-count">已选 {{ selectedIds.size }} / {{ shelfStore.groups.length }}</span>
            <button
              class="danger-btn"
              :disabled="!selectedIds.size || deleting"
              @click="deleteSelected"
            >{{ deleting ? '删除中…' : '删除所选' }}</button>
          </div>
        </div>
      </div>
    </Transition>
  </Teleport>
</template>

<script setup lang="ts">
import { computed, reactive, ref, watch } from 'vue'
import { useBookshelfStore } from '../../stores/bookshelf'
import { useAppStore } from '../../stores/app'

defineProps<{
  modelValue: boolean
}>()

defineEmits<{
  'update:modelValue': [value: boolean]
}>()

const shelfStore = useBookshelfStore()
const appStore = useAppStore()
const newGroupName = ref('')
const editingNames = reactive<Record<number, string>>({})
const selectedIds = reactive(new Set<number>())
const deleting = ref(false)

const allSelected = computed(() => (
  shelfStore.groups.length > 0 && selectedIds.size === shelfStore.groups.length
))
const someSelected = computed(() => selectedIds.size > 0)

watch(() => shelfStore.groups, (groups) => {
  groups.forEach((group) => {
    editingNames[group.groupId] = group.groupName
  })
  // 分组可能已被删掉(含批量删除), 清掉已不存在的选中项, 否则计数会虚高。
  const alive = new Set(groups.map((group) => group.groupId))
  for (const id of [...selectedIds]) {
    if (!alive.has(id)) selectedIds.delete(id)
  }
}, { immediate: true, deep: true })

function toggleSelect(groupId: number) {
  if (selectedIds.has(groupId)) selectedIds.delete(groupId)
  else selectedIds.add(groupId)
}

function toggleSelectAll() {
  if (allSelected.value) {
    selectedIds.clear()
  } else {
    shelfStore.groups.forEach((group) => selectedIds.add(group.groupId))
  }
}

async function deleteGroup(groupId: number, groupName: string) {
  const ok = await appStore.confirmDialog(`确定删除分组“${groupName}”？`, { title: '删除分组', danger: true })
  if (!ok) return
  try {
    await shelfStore.removeGroup(groupId)
    appStore.showToast('分组已删除', 'success')
  } catch (e: unknown) {
    appStore.showToast((e as Error).message || '删除分组失败', 'error')
  }
}

/** 批量删除选中的分组。只弹一次确认 —— 逐个确认会让"批量"失去意义。 */
async function deleteSelected() {
  const ids = [...selectedIds]
  if (!ids.length || deleting.value) return
  const ok = await appStore.confirmDialog(
    `确定删除选中的 ${ids.length} 个分组？分组内的书会移到未分组，不会被删除。`,
    { title: '批量删除分组', danger: true },
  )
  if (!ok) return
  deleting.value = true
  try {
    await shelfStore.removeGroups(ids)
    selectedIds.clear()
    appStore.showToast(`已删除 ${ids.length} 个分组`, 'success')
  } catch (e: unknown) {
    appStore.showToast((e as Error).message || '批量删除分组失败', 'error')
  } finally {
    deleting.value = false
  }
}

async function createGroup() {
  if (!newGroupName.value) return
  try {
    await shelfStore.saveGroup(newGroupName.value)
    newGroupName.value = ''
    appStore.showToast('分组已创建', 'success')
  } catch (e: unknown) {
    appStore.showToast((e as Error).message || '创建分组失败', 'error')
  }
}

async function renameGroup(groupId: number) {
  const name = editingNames[groupId]?.trim()
  if (!name) return
  try {
    await shelfStore.saveGroup(name, groupId)
    appStore.showToast('分组已更新', 'success')
  } catch (e: unknown) {
    appStore.showToast((e as Error).message || '更新分组失败', 'error')
  }
}
</script>

<style scoped>
.modal-overlay {
  position: fixed;
  inset: 0;
  background: rgba(0, 0, 0, 0.4);
  backdrop-filter: blur(4px);
  z-index: var(--z-overlay);
}

.modal-container {
  position: fixed;
  inset: 0;
  display: flex;
  align-items: center;
  justify-content: center;
  padding:
    calc(24px + var(--safe-area-top))
    calc(24px + var(--safe-area-right))
    calc(24px + var(--safe-area-bottom))
    calc(24px + var(--safe-area-left));
  z-index: var(--z-modal);
}

.modal-card {
  width: min(560px, 100%);
  background: var(--color-bg-elevated);
  border-radius: var(--radius-xl);
  box-shadow: var(--shadow-xl);
  overflow: hidden;
  max-height: calc(var(--app-height, 100dvh) - var(--safe-area-top) - var(--safe-area-bottom) - 32px);
  display: flex;
  flex-direction: column;
}

.modal-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: var(--space-4);
  padding: var(--space-4) var(--space-5);
  border-bottom: 1px solid var(--color-border-light);
}

.modal-header h3 {
  margin: 0;
  font-size: var(--text-lg);
  font-weight: 600;
  letter-spacing: -0.01em;
}

.close-btn {
  width: 30px;
  height: 30px;
  display: flex;
  align-items: center;
  justify-content: center;
  flex-shrink: 0;
  border-radius: var(--radius-md);
  color: var(--color-text-tertiary);
  transition:
    background-color 0.18s var(--ease-out),
    color 0.18s var(--ease-out);
}

.close-btn:hover {
  background: var(--color-bg-active);
  color: var(--color-text);
}

.close-btn svg {
  width: 17px;
  height: 17px;
}

.modal-body {
  padding: var(--space-5);
  display: flex;
  flex-direction: column;
  gap: var(--space-4);
  overflow-y: auto;
  -webkit-overflow-scrolling: touch;
}

/* ─── 新建分组 ─── */
.create-row {
  display: flex;
  gap: var(--space-2);
}

.group-input {
  flex: 1;
  min-width: 0;
  border: 1px solid var(--color-border);
  border-radius: var(--radius-md);
  padding: 9px var(--space-3);
  background: var(--color-bg-sunken);
  color: var(--color-text);
  transition:
    border-color 0.18s var(--ease-out),
    background-color 0.18s var(--ease-out);
}

.group-input::placeholder {
  color: var(--color-text-tertiary);
}

.group-input:focus {
  outline: none;
  border-color: var(--color-primary);
  background: var(--color-bg-elevated);
  box-shadow: var(--focus-ring);
}

.primary-btn {
  flex-shrink: 0;
  border-radius: var(--radius-md);
  border: 1px solid var(--color-primary);
  padding: 9px var(--space-5);
  background: var(--color-primary);
  color: var(--color-text-inverse);
  font-weight: 600;
  transition:
    background-color 0.18s var(--ease-out),
    border-color 0.18s var(--ease-out),
    transform 0.12s var(--ease-out);
}

.primary-btn:hover:not(:disabled) {
  background: var(--color-primary-dark);
  border-color: var(--color-primary-dark);
}

.primary-btn:active:not(:disabled) {
  transform: translateY(1px);
}

.primary-btn:disabled {
  opacity: 0.45;
}

/* ─── 分组列表 ─── */
.group-list {
  display: flex;
  flex-direction: column;
  gap: 2px;
}

.group-item {
  display: flex;
  align-items: center;
  gap: var(--space-2);
  padding: 4px var(--space-2);
  border-radius: var(--radius-md);
  border: 1px solid transparent;
  transition:
    background-color 0.18s var(--ease-out),
    border-color 0.18s var(--ease-out);
}

.group-item:hover {
  background: var(--color-bg-hover);
}

/* 选中态: 淡色底 + 主色描边。不给行内输入框加底色,
   否则选中行会变成「橙底 + 深色输入框」的块中块。 */
.group-item.is-selected {
  background: var(--color-selected-bg);
  border-color: var(--color-selected-border);
}

/* 行内名称输入框: 平时就是普通文字, 悬停才显出边界, 聚焦才变成输入框。
   这样一行里只有一个视觉层级, 不会出现嵌套的深色方块。 */
.group-name-input {
  flex: 1;
  min-width: 0;
  border: 1px solid transparent;
  border-radius: var(--radius-md);
  padding: 7px var(--space-2);
  background: transparent;
  color: var(--color-text);
  transition:
    border-color 0.18s var(--ease-out),
    background-color 0.18s var(--ease-out);
}

.group-name-input:hover {
  border-color: var(--color-border);
}

.group-name-input:focus {
  outline: none;
  border-color: var(--color-primary);
  background: var(--color-bg-elevated);
  box-shadow: var(--focus-ring);
}

.group-actions {
  display: flex;
  gap: var(--space-1);
  flex-shrink: 0;
}

/* 行内操作按钮: 默认无边框无底色, 悬停才浮现 —— 避免每行挂两个方框。 */
.ghost-btn {
  flex-shrink: 0;
  border-radius: var(--radius-md);
  border: 1px solid transparent;
  padding: 7px var(--space-3);
  background: transparent;
  color: var(--color-text-secondary);
  font-size: var(--text-sm);
  transition:
    background-color 0.18s var(--ease-out),
    color 0.18s var(--ease-out),
    transform 0.12s var(--ease-out);
}

.ghost-btn:hover {
  background: var(--color-bg-active);
  color: var(--color-text);
}

.ghost-btn:active {
  transform: translateY(1px);
}

.ghost-btn.danger {
  /* 默认用次级文字色: 每行常驻一个红字会让列表很吵(4 行就是 4 个红点),
     而破坏性动作的提示应在指针真正靠近时才出现。 */
  color: var(--color-text-tertiary);
}

.ghost-btn.danger:hover {
  background: color-mix(in srgb, var(--color-danger) 12%, transparent);
  color: var(--color-danger);
}

.group-check {
  display: flex;
  align-items: center;
  flex-shrink: 0;
}

.group-check input,
.select-all input {
  width: 16px;
  height: 16px;
  margin: 0;
  flex-shrink: 0;
  accent-color: var(--color-primary);
  cursor: pointer;
}

.empty-hint {
  margin: 0;
  padding: var(--space-8) 0;
  text-align: center;
  color: var(--color-text-tertiary);
  font-size: var(--text-sm);
}

/* ─── 底部批量操作条 ─── */
.modal-footer {
  display: flex;
  align-items: center;
  gap: var(--space-3);
  padding: var(--space-3) var(--space-5);
  border-top: 1px solid var(--color-border-light);
}

.select-all {
  display: flex;
  align-items: center;
  gap: var(--space-2);
  font-size: var(--text-sm);
  color: var(--color-text-secondary);
  cursor: pointer;
  user-select: none;
  transition: color 0.18s var(--ease-out);
}

.select-all:hover {
  color: var(--color-text);
}

.selection-count {
  flex: 1;
  font-size: var(--text-sm);
  color: var(--color-text-tertiary);
  /* 数字用等宽字形, 勾选时计数不会左右跳动。 */
  font-variant-numeric: tabular-nums;
}

.danger-btn {
  flex-shrink: 0;
  border-radius: var(--radius-md);
  border: 1px solid color-mix(in srgb, var(--color-danger) 32%, transparent);
  padding: 7px var(--space-4);
  background: color-mix(in srgb, var(--color-danger) 9%, transparent);
  color: var(--color-danger);
  font-size: var(--text-sm);
  font-weight: 500;
  transition:
    background-color 0.18s var(--ease-out),
    border-color 0.18s var(--ease-out),
    color 0.18s var(--ease-out),
    transform 0.12s var(--ease-out);
}

.danger-btn:hover:not(:disabled) {
  background: var(--color-danger);
  border-color: var(--color-danger);
  color: var(--color-text-inverse);
}

.danger-btn:active:not(:disabled) {
  transform: translateY(1px);
}

/* 没有选中任何分组时, 按钮退成中性灰 —— 禁用状态不该用醒目的红色描边。 */
.danger-btn:disabled {
  border-color: var(--color-border);
  background: transparent;
  color: var(--color-text-tertiary);
  opacity: 0.7;
}
</style>
