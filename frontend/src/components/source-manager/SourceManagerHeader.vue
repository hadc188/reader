<template>
  <header class="source-manager-header">
    <div class="title-block">
      <h2>书源管理</h2>
      <p>
        共 {{ total }} 个 · 启用 {{ enabled }} 个 · 当前筛选 {{ filtered }} 个
        <span v-if="selected > 0"> · 已选 {{ selected }} 个</span>
      </p>
    </div>
    <div class="header-actions">
      <button class="icon-btn" :class="{ spinning: loading }" type="button" title="刷新" @click="$emit('refresh')">
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
          <path d="M21 12a9 9 0 0 0-9-9 9.75 9.75 0 0 0-6.74 2.74L3 8" />
          <path d="M3 3v5h5" />
          <path d="M3 12a9 9 0 0 0 9 9 9.75 9.75 0 0 0 6.74-2.74L21 16" />
          <path d="M16 16h5v5" />
        </svg>
      </button>
      <button class="action-btn" type="button" @click="$emit('import-local')">本地导入</button>
      <button class="action-btn" type="button" @click="$emit('open-subscriptions')">远程同步</button>
      <button class="action-btn" type="button" @click="$emit('export')">导出</button>
      <button class="action-btn" :disabled="testing || total === 0" type="button" @click="$emit('test-sources')">
        {{ testing ? '测试中...' : '测试书源' }}
      </button>
      <button
        class="action-btn danger"
        :disabled="testing || invalidCount === 0"
        type="button"
        @click="$emit('delete-invalid')"
      >
        删除失效<span v-if="invalidCount"> {{ invalidCount }}</span>
      </button>
      <button class="action-btn primary" type="button" @click="$emit('create')">新增</button>
      <button class="icon-btn close-btn" type="button" title="关闭" @click="$emit('close')">
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
          <path d="M18 6 6 18M6 6l12 12" />
        </svg>
      </button>
    </div>
  </header>
</template>

<script setup lang="ts">
defineProps<{
  total: number
  enabled: number
  filtered: number
  selected: number
  loading: boolean
  testing: boolean
  invalidCount: number
}>()

defineEmits<{
  refresh: []
  'import-local': []
  'open-subscriptions': []
  export: []
  'test-sources': []
  'delete-invalid': []
  create: []
  close: []
}>()
</script>

<style scoped>
.source-manager-header {
  display: flex;
  align-items: flex-start;
  justify-content: space-between;
  gap: 16px;
  padding: calc(var(--space-5) + var(--safe-area-top)) var(--space-6) var(--space-5);
  border-bottom: 1px solid var(--color-border-light);
  background: var(--color-bg-elevated);
  flex-shrink: 0;
}

.title-block {
  min-width: 0;
}

.title-block h2 {
  font-size: var(--text-lg);
  font-weight: 700;
}

.title-block p {
  margin-top: 6px;
  font-size: 13px;
  color: var(--color-text-tertiary);
}

.header-actions {
  display: flex;
  flex-wrap: wrap;
  justify-content: flex-end;
  gap: var(--space-2);
}

.action-btn,
.icon-btn {
  border-radius: var(--radius-md);
  border: 1px solid var(--color-border);
  background: transparent;
  cursor: pointer;
  transition: all var(--duration-fast);
}

.action-btn {
  min-height: 36px;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  padding: 8px 12px;
  font-size: 13px;
  white-space: nowrap;
}

.action-btn.primary {
  background: var(--color-primary);
  border-color: var(--color-primary);
  color: #fff;
}

.action-btn.danger {
  color: var(--color-danger);
  border-color: rgba(245, 34, 45, 0.26);
}

.action-btn:disabled {
  cursor: not-allowed;
  opacity: 0.52;
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

.action-btn:hover,
.icon-btn:hover {
  background: var(--color-bg-hover);
}

.icon-btn.spinning svg {
  animation: spin 1s linear infinite;
}

@keyframes spin {
  to { transform: rotate(360deg); }
}

@media (max-width: 640px) {
  .source-manager-header {
    position: relative;
    flex-direction: column;
    padding: calc(var(--space-4) + var(--safe-area-top)) var(--space-4) var(--space-4);
  }

  .header-actions {
    width: 100%;
    justify-content: flex-start;
  }

  .close-btn {
    position: absolute;
    top: calc(var(--space-4) + var(--safe-area-top));
    right: var(--space-4);
  }
}
</style>
