<template>
  <Teleport to="body">
    <Transition name="fade">
      <div v-if="modelValue" class="dialog-overlay" @click="close"></div>
    </Transition>
    <Transition name="fade">
      <section
        v-if="modelValue"
        class="settings-rule-dialog"
        role="dialog"
        aria-modal="true"
        :aria-label="title"
      >
        <header class="dialog-header">
          <div>
            <h2>{{ title }}</h2>
            <p>{{ subtitle }}</p>
          </div>
          <button class="icon-button" type="button" aria-label="关闭" @click="close">
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M18 6 6 18M6 6l12 12" /></svg>
          </button>
        </header>
        <slot />
      </section>
    </Transition>
  </Teleport>
</template>

<script setup lang="ts">
defineProps<{
  modelValue: boolean
  title: string
  subtitle?: string
}>()
const emit = defineEmits<{ 'update:modelValue': [value: boolean] }>()
function close() {
  emit('update:modelValue', false)
}
</script>

<style scoped>
.dialog-overlay {
  position: fixed;
  inset: 0;
  z-index: var(--z-overlay);
  background: rgba(0, 0, 0, 0.24);
}

.settings-rule-dialog {
  position: fixed;
  top: calc(var(--titlebar-height, 30px) + (100vh - var(--titlebar-height, 30px)) / 2);
  left: 50%;
  transform: translate(-50%, -50%);
  z-index: var(--z-modal);
  width: min(680px, calc(100vw - 40px));
  height: min(620px, calc(100vh - var(--titlebar-height, 30px) - 80px));
  overflow: hidden;
  display: flex;
  flex-direction: column;
  border: 1px solid var(--color-border-light);
  border-radius: var(--radius-lg);
  background: var(--color-bg-elevated);
  color: var(--color-text);
  box-shadow: var(--shadow-xl);
}

.settings-rule-dialog :deep(.rule-toolbar) {
  flex: 0 0 auto;
}

.settings-rule-dialog :deep(.rule-body) {
  flex: 1 1 auto;
  min-height: 0;
}

.settings-rule-dialog :deep(.sub-page) {
  position: absolute;
  inset: 0;
}

.dialog-header {
  flex: 0 0 auto;
  display: flex;
  justify-content: space-between;
  gap: 16px;
  padding: 22px 24px 16px;
  border-bottom: 1px solid var(--color-divider);
}

.dialog-header h2,
.dialog-header p {
  margin: 0;
}

.dialog-header h2 {
  font-size: var(--text-lg);
}

.dialog-header p {
  margin-top: 4px;
  color: var(--color-text-tertiary);
  font-size: var(--text-xs);
}

.icon-button {
  width: 32px;
  height: 32px;
  flex: 0 0 auto;
  display: grid;
  place-items: center;
  border-radius: var(--radius-md);
  color: var(--color-text-secondary);
}

.icon-button:hover {
  background: var(--color-bg-sunken);
}

.icon-button svg {
  width: 18px;
  height: 18px;
}

@media (max-width: 600px) {
  .settings-rule-dialog {
    width: calc(100vw - 20px);
    height: min(520px, calc(100vh - var(--titlebar-height, 30px) - 20px));
    border-radius: var(--radius-lg);
  }
}
</style>
