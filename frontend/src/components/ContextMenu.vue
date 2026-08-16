<template>
  <Teleport to="body">
    <Transition name="ctx-menu">
      <div
        v-if="state.visible"
        class="context-menu"
        :style="{ top: `${state.top}px`, left: `${state.left}px`, width: `${state.width}px` }"
        @contextmenu.prevent
        @mousedown.stop
      >
        <template v-for="(item, i) in state.items" :key="i">
          <div v-if="item.divider" class="context-menu-divider" />
          <button
            v-else
            class="context-menu-item"
            :class="{
              danger: item.danger,
              disabled: item.disabled,
              active: state.activeIndex === i,
            }"
            type="button"
            :disabled="item.disabled"
            @click="onSelect(item)"
            @mouseenter="setActiveIndex(i)"
          >
            <span v-if="item.icon" class="context-menu-icon" aria-hidden="true">{{ item.icon }}</span>
            <span v-else class="context-menu-icon-placeholder" aria-hidden="true"></span>
            <span class="context-menu-label">{{ item.label }}</span>
            <span v-if="item.shortcut" class="context-menu-shortcut">{{ item.shortcut }}</span>
          </button>
        </template>
      </div>
    </Transition>
  </Teleport>
</template>

<script setup lang="ts">
import { useContextMenu, hideContextMenu, setActiveIndex, type MenuItem } from '../composables/useContextMenu'

const { state } = useContextMenu()

function onSelect(item: MenuItem) {
  if (item.disabled) return
  item.action?.()
  hideContextMenu()
}
</script>

<style scoped>
.context-menu {
  position: fixed;
  z-index: calc(var(--z-toast) + 20);
  display: flex;
  flex-direction: column;
  gap: 1px;
  padding: 6px;
  min-width: 180px;
  border: 1px solid color-mix(in srgb, var(--color-border) 80%, transparent);
  border-radius: var(--radius-lg);
  background: color-mix(in srgb, var(--color-bg-elevated) 92%, transparent);
  color: var(--color-text);
  box-shadow: var(--shadow-xl), 0 0 0 1px rgba(0, 0, 0, 0.02);
  backdrop-filter: blur(24px) saturate(140%);
  -webkit-backdrop-filter: blur(24px) saturate(140%);
  user-select: none;
  -webkit-user-select: none;
}

.context-menu-item {
  display: flex;
  align-items: center;
  gap: 10px;
  min-height: 34px;
  padding: 0 10px 0 8px;
  border: 0;
  border-radius: var(--radius-sm);
  background: transparent;
  color: inherit;
  text-align: left;
  font-size: var(--text-sm);
  font-family: var(--font-body);
  cursor: pointer;
  transition: background var(--duration-fast) var(--ease-out),
              color var(--duration-fast) var(--ease-out);
}

.context-menu-item:hover:not(.disabled),
.context-menu-item.active:not(.disabled) {
  background: var(--color-primary-bg);
  color: var(--color-primary);
}

.context-menu-item.danger {
  color: var(--color-danger);
}

.context-menu-item.danger:hover:not(.disabled),
.context-menu-item.danger.active:not(.disabled) {
  background: color-mix(in srgb, var(--color-danger) 12%, transparent);
  color: var(--color-danger);
}

.context-menu-item.disabled {
  opacity: 0.4;
  cursor: not-allowed;
}

.context-menu-icon {
  flex-shrink: 0;
  width: 18px;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  font-size: 14px;
  line-height: 1;
  opacity: 0.8;
}

.context-menu-icon-placeholder {
  flex-shrink: 0;
  width: 18px;
}

.context-menu-label {
  flex: 1;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.context-menu-shortcut {
  flex-shrink: 0;
  margin-left: 12px;
  font-size: var(--text-xs);
  color: var(--color-text-tertiary);
  font-family: ui-monospace, SFMono-Regular, Consolas, monospace;
}

.context-menu-item.active:not(.disabled) .context-menu-shortcut,
.context-menu-item:hover:not(.disabled) .context-menu-shortcut {
  color: color-mix(in srgb, var(--color-primary) 70%, transparent);
}

.context-menu-divider {
  height: 1px;
  margin: 4px 6px;
  background: var(--color-divider);
}

.ctx-menu-enter-active {
  transition: opacity var(--duration-fast) var(--ease-out),
              transform var(--duration-fast) var(--ease-out);
  transition-duration: 160ms;
}

.ctx-menu-leave-active {
  transition: opacity var(--duration-fast) var(--ease-in-out),
              transform var(--duration-fast) var(--ease-in-out);
  transition-duration: 120ms;
}

.ctx-menu-enter-from,
.ctx-menu-leave-to {
  opacity: 0;
  transform: scale(0.94) translateY(-4px);
}

.ctx-menu-enter-to,
.ctx-menu-leave-from {
  opacity: 1;
  transform: scale(1) translateY(0);
}

.context-menu {
  transform-origin: top left;
}
</style>
