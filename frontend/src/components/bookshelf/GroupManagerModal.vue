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
              <div v-for="group in shelfStore.groups" :key="group.groupId" class="group-item">
                <input v-model.trim="editingNames[group.groupId]" class="group-input" />
                <div class="group-actions">
                  <button class="mini-btn" @click="renameGroup(group.groupId)">保存</button>
                  <button class="mini-btn danger" @click="deleteGroup(group.groupId, group.groupName)">删除</button>
                </div>
              </div>
            </div>
          </div>
        </div>
      </div>
    </Transition>
  </Teleport>
</template>

<script setup lang="ts">
import { reactive, ref, watch } from 'vue'
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

watch(() => shelfStore.groups, (groups) => {
  groups.forEach((group) => {
    editingNames[group.groupId] = group.groupName
  })
}, { immediate: true, deep: true })

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
</script>

<style scoped>
.modal-overlay {
  position: fixed;
  inset: 0;
  background: rgba(0, 0, 0, 0.4);
  backdrop-filter: blur(4px);
  z-index: 1000;
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
  z-index: 1001;
}

.modal-card {
  width: min(560px, 100%);
  background: var(--color-bg-elevated);
  border-radius: 24px;
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
  padding: 18px 20px;
  border-bottom: 1px solid var(--color-border-light);
}

.modal-header h3 {
  margin: 0;
  font-size: 18px;
}

.close-btn {
  width: 32px;
  height: 32px;
  display: flex;
  align-items: center;
  justify-content: center;
  border-radius: 10px;
}

.close-btn svg {
  width: 18px;
  height: 18px;
}

.modal-body {
  padding: 20px;
  display: flex;
  flex-direction: column;
  gap: 16px;
  overflow-y: auto;
  -webkit-overflow-scrolling: touch;
}

.create-row,
.group-item {
  display: flex;
  gap: 10px;
}

.group-list {
  display: flex;
  flex-direction: column;
  gap: 10px;
}

.group-input {
  flex: 1;
  border: 1px solid var(--color-border);
  border-radius: 12px;
  padding: 10px 12px;
  background: var(--color-bg);
}

.group-actions {
  display: flex;
  gap: 8px;
}

.primary-btn,
.mini-btn {
  border-radius: 12px;
  border: 1px solid var(--color-border);
  padding: 10px 14px;
  cursor: pointer;
}

.primary-btn {
  background: var(--color-primary);
  border-color: var(--color-primary);
  color: #fff;
}

.mini-btn.danger {
  color: var(--color-danger);
  border-color: rgba(245, 34, 45, 0.2);
}
</style>
