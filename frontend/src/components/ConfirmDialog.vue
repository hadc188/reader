<template>
  <Teleport to="body">
    <Transition name="fade">
      <div v-if="appStore.confirmState" class="confirm-overlay" @click.self="cancel">
        <div class="confirm-dialog" :class="{ danger: appStore.confirmState.danger }">
          <h3 v-if="appStore.confirmState.title" class="confirm-title">{{ appStore.confirmState.title }}</h3>
          <p class="confirm-message">{{ appStore.confirmState.message }}</p>
          <div class="confirm-actions">
            <button class="confirm-btn" type="button" @click="cancel">取消</button>
            <button class="confirm-btn primary" type="button" @click="ok">确定</button>
          </div>
        </div>
      </div>
    </Transition>
  </Teleport>
</template>

<script setup lang="ts">
import { useAppStore } from '../stores/app'

const appStore = useAppStore()

function ok() {
  appStore.resolveConfirm(true)
}

function cancel() {
  appStore.resolveConfirm(false)
}
</script>

<style scoped>
.confirm-overlay {
  position: fixed;
  inset: 0;
  z-index: calc(var(--z-modal) + 10);
  display: flex;
  align-items: center;
  justify-content: center;
  background: rgba(0, 0, 0, 0.45);
  backdrop-filter: blur(2px);
  padding: 24px;
}

.confirm-dialog {
  width: min(360px, 100%);
  background: var(--color-bg-elevated);
  border-radius: var(--radius-lg);
  padding: 20px;
  box-shadow: var(--shadow-xl);
}

.confirm-title {
  margin: 0 0 8px;
  font-size: 15px;
  font-weight: 700;
}

.confirm-message {
  margin: 0 0 18px;
  font-size: 13px;
  line-height: 1.6;
  color: var(--color-text-secondary);
  word-break: break-all;
}

.confirm-actions {
  display: flex;
  justify-content: flex-end;
  gap: 10px;
}

.confirm-btn {
  min-height: 34px;
  padding: 0 16px;
  border: 1px solid var(--color-border);
  border-radius: var(--radius-md);
  background: transparent;
  color: var(--color-text-secondary);
  font-size: 13px;
  cursor: pointer;
}

.confirm-btn.primary {
  background: var(--color-primary);
  border-color: var(--color-primary);
  color: #fff;
}

.confirm-dialog.danger .confirm-btn.primary {
  background: var(--color-danger);
  border-color: var(--color-danger);
}

.confirm-btn:hover {
  background: var(--color-bg-hover);
}

.confirm-btn.primary:hover {
  background: var(--color-primary-dark);
}

.confirm-dialog.danger .confirm-btn.primary:hover {
  background: var(--color-danger);
  opacity: 0.9;
}

.fade-enter-active,
.fade-leave-active {
  transition: opacity 0.15s ease;
}

.fade-enter-from,
.fade-leave-to {
  opacity: 0;
}
</style>
