<template>
  <section class="filter-bar">
    <label class="bulk-check" :class="{ muted: !hasSources }">
      <input
        type="checkbox"
        :checked="allSelected"
        :disabled="!hasSources"
        :aria-checked="partiallySelected ? 'mixed' : allSelected"
        @change="$emit('toggle-current-selection')"
      />
      <span>{{ allSelected ? '取消全选' : '全选当前' }}</span>
    </label>
    <div class="search-field">
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="16" height="16">
        <circle cx="11" cy="11" r="8" />
        <path d="m21 21-4.3-4.3" />
      </svg>
      <input
        :value="filterText"
        type="text"
        placeholder="搜索书源名称或 URL"
        @input="$emit('update:filterText', ($event.target as HTMLInputElement).value)"
      />
    </div>
    <select
      :value="filterGroup"
      class="group-select"
      @change="$emit('update:filterGroup', ($event.target as HTMLSelectElement).value)"
    >
      <option value="">全部分组</option>
      <option v-for="group in groups" :key="group" :value="group">{{ group }}</option>
    </select>
    <div v-if="selectedCount > 0" class="selection-tools">
      <span>已选 {{ selectedCount }}</span>
      <button class="mini-btn" type="button" @click="$emit('clear-selection')">清空</button>
      <button class="mini-btn danger" type="button" @click="$emit('delete-selection')">删除选中</button>
    </div>
  </section>
</template>

<script setup lang="ts">
defineProps<{
  filterText: string
  filterGroup: string
  groups: string[]
  allSelected: boolean
  partiallySelected: boolean
  selectedCount: number
  hasSources: boolean
}>()

defineEmits<{
  'update:filterText': [value: string]
  'update:filterGroup': [value: string]
  'toggle-current-selection': []
  'clear-selection': []
  'delete-selection': []
}>()
</script>

<style scoped>
.filter-bar {
  display: flex;
  align-items: center;
  gap: 12px;
  padding: 14px 24px 0;
}

.bulk-check {
  min-height: 40px;
  display: inline-flex;
  align-items: center;
  gap: 8px;
  padding: 0 12px;
  border: 1px solid var(--color-border);
  border-radius: var(--radius-md);
  background: var(--color-bg);
  color: var(--color-text-secondary);
  font-size: 13px;
  white-space: nowrap;
}

.bulk-check.muted {
  opacity: 0.5;
}

.bulk-check input {
  width: 16px;
  height: 16px;
  accent-color: var(--color-primary);
}

.search-field {
  flex: 1;
  min-width: 180px;
  display: flex;
  align-items: center;
  gap: 8px;
  min-height: 40px;
  padding: 0 12px;
  border: 1px solid var(--color-border);
  border-radius: var(--radius-md);
  background: var(--color-bg);
}

.search-field input {
  flex: 1;
  min-width: 0;
  border: none;
  background: transparent;
  outline: none;
}

.group-select {
  min-width: 150px;
  min-height: 40px;
  padding: 0 12px;
  border: 1px solid var(--color-border);
  border-radius: var(--radius-md);
  background: var(--color-bg);
  color: inherit;
}

.selection-tools {
  display: inline-flex;
  align-items: center;
  gap: 8px;
  min-height: 40px;
  padding: 4px 6px 4px 10px;
  border: 1px solid rgba(201, 127, 58, 0.22);
  border-radius: var(--radius-md);
  background: rgba(201, 127, 58, 0.08);
  color: var(--color-text-secondary);
  font-size: 12px;
  white-space: nowrap;
}

.selection-tools span {
  color: var(--color-text-secondary);
}

.mini-btn {
  min-height: 36px;
  padding: 7px 10px;
  border: 1px solid var(--color-border);
  border-radius: var(--radius-md);
  background: transparent;
  font-size: 12px;
  white-space: nowrap;
}

.mini-btn:hover {
  background: var(--color-bg-hover);
}

.mini-btn.danger {
  color: var(--color-danger);
  border-color: rgba(245, 34, 45, 0.22);
}

.mini-btn.danger:hover {
  background: rgba(245, 34, 45, 0.08);
}

@media (max-width: 760px) {
  .filter-bar {
    flex-direction: column;
    align-items: stretch;
    padding-left: 16px;
    padding-right: 16px;
  }

  .selection-tools {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 8px;
    padding: 10px;
  }

  .selection-tools span {
    grid-column: 1 / -1;
  }

  .selection-tools .mini-btn {
    min-height: 42px;
  }
}
</style>
