<template>
  <div v-if="modelValue" class="modal-overlay" @click="$emit('update:modelValue', false)">
    <div class="modal-content" @click.stop>
      <div class="modal-header">
        <h3>选择分组</h3>
        <button class="close-btn" @click="$emit('update:modelValue', false)">
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M18 6 6 18M6 6l12 12" /></svg>
        </button>
      </div>

      <div class="modal-body">
        <div class="group-list">
          <button
            v-for="group in shelfStore.groups"
            :key="group.groupId"
            class="group-item"
            @click="handleSelect(group.groupId)"
          >
            <span class="group-icon">#</span>
            <span class="group-name">{{ group.groupName }}</span>
          </button>
          
          <div class="new-group">
             <input 
               v-model="newGroupName" 
               placeholder="新建分组名称..." 
               class="group-input"
               @keyup.enter="handleCreate"
             />
             <button class="add-btn" @click="handleCreate" :disabled="!newGroupName.trim()">
               新建并移动
             </button>
          </div>
        </div>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { ref } from 'vue'
import { useBookshelfStore } from '../../stores/bookshelf'
import { useAppStore } from '../../stores/app'

defineProps<{
  modelValue: boolean
}>()

const emit = defineEmits<{
  'update:modelValue': [value: boolean]
  select: [groupId: number]
}>()

const shelfStore = useBookshelfStore()
const appStore = useAppStore()
const newGroupName = ref('')

function handleSelect(id: number) {
  emit('select', id)
  emit('update:modelValue', false)
}

async function handleCreate() {
  if (!newGroupName.value.trim()) return
  try {
    const groupId = await shelfStore.saveGroup(newGroupName.value.trim())
    emit('select', groupId)
    newGroupName.value = ''
    emit('update:modelValue', false)
  } catch (e: unknown) {
    appStore.showToast((e as Error).message || '创建分组失败', 'error')
  }
}
</script>

<style scoped>
.modal-overlay {
  position: fixed;
  inset: 0;
  background: rgba(0, 0, 0, 0.4);
  backdrop-filter: blur(4px);
  display: flex;
  align-items: center;
  justify-content: center;
  padding:
    calc(16px + var(--safe-area-top))
    calc(16px + var(--safe-area-right))
    calc(16px + var(--safe-area-bottom))
    calc(16px + var(--safe-area-left));
  z-index: 1000;
}

.modal-content {
  width: 100%;
  max-width: 360px;
  background: var(--color-bg-elevated);
  border-radius: var(--radius-2xl);
  box-shadow: var(--shadow-2xl);
  overflow: hidden;
  max-height: calc(var(--app-height, 100dvh) - var(--safe-area-top) - var(--safe-area-bottom) - 32px);
  display: flex;
  flex-direction: column;
}

.modal-header {
  padding: var(--space-4) var(--space-5);
  display: flex;
  align-items: center;
  justify-content: space-between;
  border-bottom: 1px solid var(--color-border-light);
}

.modal-header h3 { margin: 0; font-size: var(--text-lg); font-weight: 600; }

.close-btn {
  width: 32px; height: 32px;
  display: flex; align-items: center; justify-content: center;
  border-radius: var(--radius-md); color: var(--color-text-secondary);
  background: transparent; border: none; cursor: pointer;
}

.modal-body {
  padding: var(--space-2);
  overflow-y: auto;
  -webkit-overflow-scrolling: touch;
}

.group-list {
  display: flex; flex-direction: column; gap: 4px;
}

.group-item {
  display: flex; align-items: center; gap: var(--space-3);
  padding: var(--space-3) var(--space-4);
  border-radius: var(--radius-lg);
  border: none; background: transparent; color: var(--color-text);
  cursor: pointer; text-align: left; transition: all 0.2s;
}

.group-item:hover {
  background: var(--color-bg-hover);
  color: var(--color-primary);
}

.group-icon { opacity: 0.4; font-weight: 700; }
.group-name { font-weight: 500; font-size: 15px; }

.new-group {
  margin-top: var(--space-2);
  padding: var(--space-3);
  border-top: 1px solid var(--color-border-light);
  display: flex; gap: var(--space-2);
}

.group-input {
  flex: 1; padding: var(--space-2) var(--space-3);
  border-radius: var(--radius-md); border: 1px solid var(--color-border);
  background: var(--color-bg-sunken); color: var(--color-text);
  font-size: var(--text-sm);
}

.add-btn {
  padding: 0 var(--space-3); border-radius: var(--radius-md);
  background: var(--color-primary); color: white;
  border: none; font-size: var(--text-sm); font-weight: 500; cursor: pointer;
}

.add-btn:disabled { opacity: 0.5; cursor: not-allowed; }
</style>
