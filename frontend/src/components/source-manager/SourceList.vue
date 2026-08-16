<template>
  <section class="source-list-wrapper">
    <div v-if="loading" class="loading-state">
      <div class="loading-spinner"></div>
      <p>加载书源中...</p>
    </div>
    <div v-else class="source-list">
      <article
        v-for="source in sources"
        :key="source.bookSourceUrl"
        class="source-item"
        :class="{
          active: activeUrl === source.bookSourceUrl,
          selected: selectedUrls.has(source.bookSourceUrl),
        }"
        @contextmenu.prevent="$emit('contextmenu', { source, event: $event })"
      >
        <label class="row-check" @click.stop>
          <input
            type="checkbox"
            :checked="selectedUrls.has(source.bookSourceUrl)"
            @change="$emit('toggle-selection', source)"
          />
        </label>
        <button class="source-info" type="button" @click="$emit('edit', source)">
          <span class="source-name">{{ source.bookSourceName }}</span>
          <span class="source-url">{{ source.bookSourceUrl }}</span>
          <span v-if="source.bookSourceGroup" class="source-group">{{ source.bookSourceGroup }}</span>
        </button>
        <div class="source-actions">
          <button
            class="icon-btn small"
            type="button"
            :title="isPinned(source) ? '取消置顶' : '置顶'"
            :class="{ pinned: isPinned(source) }"
            @click="togglePin(source)"
          >
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
              <path d="M12 17v5" />
              <path d="M9 10.8V4a1 1 0 0 1 1-1h4a1 1 0 0 1 1 1v6.8l1.5 2.2a1 1 0 0 1-.8 1.5H8.3a1 1 0 0 1-.8-1.5L9 10.8Z" />
            </svg>
          </button>
          <label class="toggle" title="启用状态">
            <input type="checkbox" :checked="source.enabled !== false" @change="$emit('toggle-enabled', source)" />
            <span class="toggle-slider"></span>
          </label>
          <button class="icon-btn small" type="button" title="编辑" @click="$emit('edit', source)">
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
              <path d="M12 20h9" />
              <path d="M16.5 3.5a2.1 2.1 0 0 1 3 3L7 19l-4 1 1-4Z" />
            </svg>
          </button>
          <button class="icon-btn small danger" type="button" title="删除" @click="$emit('delete', source)">
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
              <path d="M3 6h18M19 6v14c0 1-1 2-2 2H7c-1 0-2-1-2-2V6M8 6V4c0-1 1-2 2-2h4c1 0 2 1 2 2v2" />
            </svg>
          </button>
        </div>
      </article>
      <div v-if="!sources.length" class="empty-state">
        <strong>{{ emptyTitle }}</strong>
        <span>{{ emptyDescription }}</span>
      </div>
    </div>
  </section>
</template>

<script setup lang="ts">
import type { BookSource } from '../../types'

defineProps<{
  sources: BookSource[]
  loading: boolean
  selectedUrls: ReadonlySet<string>
  activeUrl?: string
  emptyTitle: string
  emptyDescription: string
}>()

function isPinned(source: BookSource) {
  return typeof source.customOrder === 'number' && source.customOrder < 0
}

function togglePin(source: BookSource) {
  if (isPinned(source)) {
    emit('unpin', source)
  } else {
    emit('pin', source)
  }
}

const emit = defineEmits<{
  edit: [source: BookSource]
  'toggle-enabled': [source: BookSource]
  'toggle-selection': [source: BookSource]
  delete: [source: BookSource]
  pin: [source: BookSource]
  unpin: [source: BookSource]
  contextmenu: [{ source: BookSource; event: MouseEvent }]
}>()
</script>

<style scoped>
.source-list-wrapper {
  min-height: 0;
  border: 1px solid var(--color-border-light);
  border-radius: var(--radius-lg);
  overflow: hidden;
  display: flex;
  flex-direction: column;
  background: var(--color-bg-elevated);
}

.source-list {
  flex: 1;
  overflow: auto;
}

.source-item {
  display: grid;
  grid-template-columns: auto minmax(0, 1fr) auto;
  align-items: center;
  gap: 12px;
  padding: 13px 14px;
  border-bottom: 1px solid var(--color-border-light);
  transition: background var(--duration-fast) var(--ease-out), box-shadow var(--duration-fast) var(--ease-out);
}

.source-item.active {
  background: rgba(201, 127, 58, 0.06);
}

.source-item.selected {
  background: rgba(201, 127, 58, 0.1);
  box-shadow: inset 3px 0 0 var(--color-primary);
}

.row-check {
  display: inline-flex;
  align-items: center;
}

.row-check input {
  width: 16px;
  height: 16px;
  accent-color: var(--color-primary);
}

.source-info {
  min-width: 0;
  display: flex;
  flex-direction: column;
  align-items: flex-start;
  gap: 4px;
  text-align: left;
  cursor: pointer;
}

.source-name {
  max-width: 100%;
  font-weight: 600;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.source-url {
  max-width: 100%;
  font-size: 12px;
  color: var(--color-text-tertiary);
  overflow-wrap: anywhere;
}

.source-group {
  max-width: 100%;
  padding: 2px 8px;
  border-radius: var(--radius-full);
  background: var(--color-bg-hover);
  color: var(--color-text-secondary);
  font-size: 11px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.source-actions {
  display: flex;
  align-items: center;
  gap: 8px;
}

.toggle {
  position: relative;
  display: inline-flex;
  width: 38px;
  height: 22px;
}

.toggle input {
  opacity: 0;
  width: 0;
  height: 0;
}

.toggle-slider {
  position: absolute;
  inset: 0;
  border-radius: var(--radius-full);
  background: var(--color-border);
  transition: background var(--duration-fast);
}

.toggle-slider::before {
  content: '';
  position: absolute;
  width: 18px;
  height: 18px;
  left: 2px;
  top: 2px;
  border-radius: 50%;
  background: #fff;
  box-shadow: var(--shadow-xs);
  transition: transform var(--duration-fast);
}

.toggle input:checked + .toggle-slider {
  background: var(--color-primary);
}

.toggle input:checked + .toggle-slider::before {
  transform: translateX(16px);
}

.icon-btn {
  width: 30px;
  height: 30px;
  display: flex;
  align-items: center;
  justify-content: center;
  border: 1px solid var(--color-border);
  border-radius: var(--radius-md);
  color: var(--color-text-secondary);
  transition: all var(--duration-fast);
}

.icon-btn:hover {
  background: var(--color-bg-hover);
}

.icon-btn svg {
  width: 14px;
  height: 14px;
}

.icon-btn.danger:hover {
  background: rgba(245, 34, 45, 0.08);
  color: var(--color-danger);
}

.icon-btn.pinned {
  background: rgba(201, 127, 58, 0.14);
  color: var(--color-primary);
  border-color: rgba(201, 127, 58, 0.4);
}

.loading-state,
.empty-state {
  flex: 1;
  min-height: 260px;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 10px;
  padding: 24px;
  text-align: center;
  color: var(--color-text-tertiary);
}

.empty-state strong {
  color: var(--color-text-secondary);
}

.loading-spinner {
  width: 28px;
  height: 28px;
  border: 3px solid var(--color-border);
  border-top-color: var(--color-primary);
  border-radius: 50%;
  animation: spin 0.8s linear infinite;
}

@keyframes spin {
  to { transform: rotate(360deg); }
}

@media (max-width: 560px) {
  .source-item {
    grid-template-columns: auto minmax(0, 1fr);
  }

  .source-actions {
    grid-column: 2;
    justify-content: flex-start;
  }
}
</style>
