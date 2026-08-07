<template>
  <Transition name="fade">
    <div v-if="visible" class="subscription-backdrop" @click.self="$emit('close')">
      <section class="subscription-panel">
        <header class="subscription-header">
          <div>
            <h3>远程书源同步</h3>
            <p>保存书源合集链接，后续可以一键同步更新。</p>
          </div>
          <button class="icon-btn" type="button" title="关闭" @click="$emit('close')">
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
              <path d="M18 6 6 18M6 6l12 12" />
            </svg>
          </button>
        </header>

        <div class="remote-form">
          <input
            :value="remoteUrl"
            type="text"
            placeholder="输入远程书源合集链接"
            @input="$emit('update:remoteUrl', ($event.target as HTMLInputElement).value)"
          />
          <button class="action-btn primary" type="button" @click="$emit('sync')">立即同步</button>
          <button class="action-btn" type="button" @click="$emit('save')">保存订阅</button>
        </div>

        <div v-if="subscriptions.length" class="subscription-list">
          <article v-for="item in subscriptions" :key="item.url" class="subscription-item">
            <div class="subscription-main">
              <span class="subscription-url">{{ item.url }}</span>
              <span v-if="item.lastSyncedAt" class="subscription-time">上次同步 {{ formatTime(item.lastSyncedAt) }}</span>
            </div>
            <div class="subscription-actions">
              <button class="mini-btn" type="button" @click="$emit('sync-subscription', item.url)">同步</button>
              <button class="mini-btn danger" type="button" @click="$emit('remove-subscription', item.url)">删除</button>
            </div>
          </article>
        </div>
        <div v-else class="subscription-empty">还没有保存的远程订阅。</div>
      </section>
    </div>
  </Transition>
</template>

<script setup lang="ts">
export type SourceSubscription = {
  url: string
  lastSyncedAt?: number
}

defineProps<{
  visible: boolean
  remoteUrl: string
  subscriptions: SourceSubscription[]
}>()

defineEmits<{
  'update:remoteUrl': [value: string]
  sync: []
  save: []
  'sync-subscription': [url: string]
  'remove-subscription': [url: string]
  close: []
}>()

function formatTime(ts: number) {
  return new Date(ts).toLocaleString()
}
</script>

<style scoped>
.subscription-backdrop {
  position: fixed;
  inset: 0;
  z-index: calc(var(--z-modal) + 1);
  display: flex;
  align-items: center;
  justify-content: center;
  padding: 24px;
  background: rgba(0, 0, 0, 0.28);
}

.subscription-panel {
  width: min(720px, 100%);
  max-height: min(78vh, 720px);
  display: flex;
  flex-direction: column;
  gap: 14px;
  padding: 20px;
  border: 1px solid var(--color-border-light);
  border-radius: var(--radius-xl);
  background: var(--color-bg-elevated);
  box-shadow: var(--shadow-xl);
  overflow: auto;
}

.subscription-header {
  display: flex;
  align-items: flex-start;
  justify-content: space-between;
  gap: 16px;
}

.subscription-header h3 {
  margin: 0;
  font-size: 17px;
}

.subscription-header p {
  margin-top: 6px;
  color: var(--color-text-tertiary);
  font-size: 13px;
}

.remote-form {
  display: flex;
  gap: 10px;
}

.remote-form input {
  flex: 1;
  min-width: 0;
  min-height: 40px;
  padding: 0 12px;
  border: 1px solid var(--color-border);
  border-radius: var(--radius-md);
  background: var(--color-bg);
}

.subscription-list {
  display: flex;
  flex-direction: column;
  gap: 8px;
}

.subscription-item {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
  padding: 12px;
  border: 1px solid var(--color-border-light);
  border-radius: var(--radius-lg);
  background: var(--color-bg);
}

.subscription-main {
  min-width: 0;
  display: flex;
  flex-direction: column;
}

.subscription-url {
  font-size: 13px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.subscription-time,
.subscription-empty {
  margin-top: 4px;
  color: var(--color-text-tertiary);
  font-size: 12px;
}

.subscription-actions {
  display: flex;
  gap: 8px;
}

.action-btn,
.mini-btn,
.icon-btn {
  border: 1px solid var(--color-border);
  border-radius: var(--radius-md);
  background: transparent;
  transition: all var(--duration-fast);
}

.action-btn {
  min-height: 40px;
  padding: 0 12px;
  font-size: 13px;
  white-space: nowrap;
}

.action-btn.primary {
  background: var(--color-primary);
  border-color: var(--color-primary);
  color: #fff;
}

.mini-btn {
  min-height: 32px;
  padding: 0 10px;
  font-size: 12px;
}

.mini-btn.danger {
  color: var(--color-danger);
  border-color: rgba(245, 34, 45, 0.2);
}

.icon-btn {
  width: 34px;
  height: 34px;
  display: flex;
  align-items: center;
  justify-content: center;
}

.icon-btn svg {
  width: 16px;
  height: 16px;
}

.action-btn:hover,
.mini-btn:hover,
.icon-btn:hover {
  background: var(--color-bg-hover);
}

.fade-enter-active,
.fade-leave-active {
  transition: opacity var(--duration-normal) var(--ease-out);
}

.fade-enter-from,
.fade-leave-to {
  opacity: 0;
}

@media (max-width: 640px) {
  .subscription-backdrop {
    align-items: stretch;
    padding: 12px;
  }

  .remote-form,
  .subscription-item {
    flex-direction: column;
    align-items: stretch;
  }
}
</style>
