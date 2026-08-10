<template>
  <Teleport to="body">
    <Transition name="fade">
      <div v-if="appStore.closeChoice" class="close-overlay" @click.self="dismiss">
        <div class="close-dialog">
          <h3 class="close-title">关闭阅读</h3>
          <p class="close-message">选择关闭方式：</p>
          <div class="close-options">
            <button class="close-option primary" type="button" @click="hideToTray">
              <span class="option-icon">⏵</span>
              <span>
                <strong>隐藏到系统托盘</strong>
                <em>后台继续运行</em>
              </span>
            </button>
            <button class="close-option" type="button" @click="quit">
              <span class="option-icon">✕</span>
              <span>
                <strong>退出</strong>
                <em>关闭应用</em>
              </span>
            </button>
          </div>
        </div>
      </div>
    </Transition>
  </Teleport>
</template>

<script setup lang="ts">
import { getCurrentWindow } from '@tauri-apps/api/window'
import { useAppStore } from '../stores/app'

const appStore = useAppStore()
const isTauri = typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window
const appWindow = isTauri ? getCurrentWindow() : null

async function hideToTray() {
  appStore.resolveCloseChoice('tray')
  await appWindow?.hide()
}

async function quit() {
  appStore.resolveCloseChoice('quit')
  // destroy() exits the app without re-triggering the close-requested handler.
  await appWindow?.destroy()
}

function dismiss() {
  // Clicking the backdrop just closes the dialog; the window stays as-is.
  appStore.resolveCloseChoice('cancel')
}
</script>

<style scoped>
.close-overlay {
  position: fixed;
  inset: 0;
  z-index: calc(var(--z-modal) + 30);
  display: flex;
  align-items: center;
  justify-content: center;
  background: rgba(0, 0, 0, 0.45);
  backdrop-filter: blur(2px);
  padding: 24px;
}

.close-dialog {
  width: min(340px, 100%);
  background: var(--color-bg-elevated);
  border-radius: var(--radius-lg);
  padding: 20px;
  box-shadow: var(--shadow-xl);
}

.close-title {
  margin: 0 0 4px;
  font-size: 15px;
  font-weight: 700;
}

.close-message {
  margin: 0 0 16px;
  font-size: 13px;
  color: var(--color-text-secondary);
}

.close-options {
  display: flex;
  flex-direction: column;
  gap: 10px;
}

.close-option {
  display: flex;
  align-items: center;
  gap: 12px;
  padding: 12px 14px;
  border: 1px solid var(--color-border);
  border-radius: var(--radius-md);
  background: transparent;
  cursor: pointer;
  text-align: left;
  transition: all var(--duration-fast);
}

.close-option:hover {
  background: var(--color-bg-hover);
  border-color: var(--color-primary);
}

.close-option.primary {
  border-color: var(--color-primary);
}

.close-option.primary:hover {
  background: var(--color-primary-bg);
}

.option-icon {
  width: 36px;
  height: 36px;
  flex-shrink: 0;
  display: flex;
  align-items: center;
  justify-content: center;
  border-radius: var(--radius-full);
  background: var(--color-bg-sunken);
  font-size: 16px;
}

.close-option strong,
.close-option em {
  display: block;
}

.close-option strong {
  font-size: 14px;
  color: var(--color-text);
}

.close-option em {
  font-size: 12px;
  font-style: normal;
  color: var(--color-text-tertiary);
  margin-top: 2px;
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
