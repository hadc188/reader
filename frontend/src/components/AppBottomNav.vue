<template>
  <div
    class="bottom-nav-shell"
    :class="`theme-${theme}`"
    @touchmove.prevent="handleTouchMove"
    @touchend="handleTouchEnd"
    @touchcancel="handleTouchEnd"
    @mousemove="handleMouseMove"
    @mouseup="handleMouseUp"
    @mouseleave="handleMouseUp"
  >
    <div ref="navRef" class="bottom-nav">
      <div v-if="indicatorStyle" class="nav-indicator" :style="indicatorStyle" />
      <button
        v-for="item in items"
        :key="item.key"
        :ref="(el) => setItemRef(el, item.key)"
        class="nav-item"
        :class="{ active: activeKey === item.key }"
        :style="getItemStyle(item.key)"
        :title="item.label"
        @click="go(item.path)"
        @touchstart="handlePointerStart($event, item.key)"
        @mousedown.prevent="handlePointerStart($event, item.key)"
      >
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
          <path v-for="(path, index) in item.paths" :key="index" :d="path" />
        </svg>
        <span :style="getLabelStyle(item.key)">{{ item.label }}</span>
      </button>
    </div>
  </div>
</template>

<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { useAppStore } from '../stores/app'

type NavKey = 'home' | 'explore' | 'recent' | 'rss'

const router = useRouter()
const route = useRoute()
const appStore = useAppStore()

const allItems: Array<{ key: NavKey; label: string; path: string; paths: string[] }> = [
  {
    key: 'home',
    label: '书架',
    path: '/',
    paths: ['M4 5.5A2.5 2.5 0 0 1 6.5 3H20', 'M4 5.5V19a2 2 0 0 0 2 2h14', 'M8 7h8', 'M8 11h8', 'M8 15h5'],
  },
  {
    key: 'explore',
    label: '书海',
    path: '/explore',
    paths: ['M12 3a9 9 0 1 0 9 9', 'm16.24 7.76-2.12 6.36-6.36 2.12 2.12-6.36 6.36-2.12z'],
  },
  {
    key: 'recent',
    label: '统计',
    path: '/stats',
    paths: ['M12 7v5l3 2', 'M12 3a9 9 0 1 0 9 9'],
  },
  {
    key: 'rss',
    label: 'RSS',
    path: '/rss',
    paths: ['M4 11a9 9 0 0 1 9 9', 'M4 4a16 16 0 0 1 16 16', 'M5 19h.01'],
  },
]

// Filter out hidden features; hide-key maps to a feature key ('stats', 'rss').
const items = computed(() => {
  const hidden = (key: NavKey) => {
    if (key === 'recent') return appStore.isFeatureHidden('stats')
    if (key === 'rss') return appStore.isFeatureHidden('rss')
    return false
  }
  return allItems.filter((item) => !hidden(item.key))
})

const navRef = ref<HTMLElement | null>(null)
const itemRefs = ref<Record<NavKey, HTMLElement | null>>({
  home: null,
  explore: null,
  recent: null,
  rss: null,
})
const dragging = ref(false)
const dragX = ref(0)
const pointerOffsetX = ref(0)
const indicatorRect = ref({ left: 0, width: 0 })

const activeKey = computed<NavKey>(() => {
  if (route.path.startsWith('/explore')) return 'explore'
  if (route.path.startsWith('/stats')) return 'recent'
  if (route.path.startsWith('/rss')) return 'rss'
  return 'home'
})
const theme = computed(() => appStore.theme)

function setItemRef(el: Element | { $el?: Element } | null, key: NavKey) {
  const resolved = el instanceof HTMLElement
    ? el
    : el && '$el' in el && el.$el instanceof HTMLElement
      ? el.$el
      : null
  itemRefs.value[key] = resolved
}

function syncIndicatorToActive() {
  const nav = navRef.value
  const target = itemRefs.value[activeKey.value]
  if (!nav || !target) return
  const navRect = nav.getBoundingClientRect()
  const rect = target.getBoundingClientRect()
  indicatorRect.value = {
    left: rect.left - navRect.left,
    width: rect.width,
  }
}

function go(path: string) {
  if (route.path !== path) {
    router.push(path)
  }
}

function nearestKey(clientX: number): NavKey {
  const nav = navRef.value
  if (!nav) return activeKey.value
  const navRect = nav.getBoundingClientRect()
  let winner: NavKey = activeKey.value
  let winnerDistance = Number.POSITIVE_INFINITY
  for (const item of items.value) {
    const el = itemRefs.value[item.key]
    if (!el) continue
    const rect = el.getBoundingClientRect()
    const centerX = rect.left - navRect.left + rect.width / 2
    const distance = Math.abs(clientX - navRect.left - centerX)
    if (distance < winnerDistance) {
      winnerDistance = distance
      winner = item.key
    }
  }
  return winner
}

function getClientX(event: TouchEvent | MouseEvent) {
  return 'touches' in event ? event.touches[0]?.clientX ?? 0 : event.clientX
}

function handlePointerStart(event: TouchEvent | MouseEvent, key: NavKey) {
  const nav = navRef.value
  const el = itemRefs.value[key]
  if (!nav || !el) return
  const navRect = nav.getBoundingClientRect()
  const rect = el.getBoundingClientRect()
  dragging.value = true
  pointerOffsetX.value = getClientX(event) - rect.left
  dragX.value = rect.left - navRect.left
}

function handleTouchMove(event: TouchEvent) {
  if (!dragging.value || !navRef.value) return
  const navRect = navRef.value.getBoundingClientRect()
  const width = indicatorRect.value.width || itemRefs.value[activeKey.value]?.getBoundingClientRect().width || 0
  dragX.value = Math.min(
    Math.max(getClientX(event) - navRect.left - pointerOffsetX.value, 0),
    Math.max(0, navRect.width - width),
  )
}

function handleMouseMove(event: MouseEvent) {
  if (!dragging.value || !navRef.value) return
  const navRect = navRef.value.getBoundingClientRect()
  const width = indicatorRect.value.width || itemRefs.value[activeKey.value]?.getBoundingClientRect().width || 0
  dragX.value = Math.min(
    Math.max(event.clientX - navRect.left - pointerOffsetX.value, 0),
    Math.max(0, navRect.width - width),
  )
}

function handleTouchEnd() {
  if (!dragging.value) return
  const key = nearestKey((navRef.value?.getBoundingClientRect().left || 0) + dragX.value + indicatorRect.value.width / 2)
  dragging.value = false
  go(items.value.find((item) => item.key === key)?.path || '/')
  nextTick(syncIndicatorToActive)
}

function handleMouseUp() {
  if (!dragging.value) return
  const key = nearestKey((navRef.value?.getBoundingClientRect().left || 0) + dragX.value + indicatorRect.value.width / 2)
  dragging.value = false
  go(items.value.find((item) => item.key === key)?.path || '/')
  nextTick(syncIndicatorToActive)
}

const indicatorStyle = computed(() => {
  const left = dragging.value ? dragX.value : indicatorRect.value.left
  const width = indicatorRect.value.width
  if (!width) return null
  return {
    width: `${width}px`,
    transform: `translate3d(${left}px, 0, 0)`,
  }
})

function getIndicatorCenter() {
  const left = dragging.value ? dragX.value : indicatorRect.value.left
  return left + indicatorRect.value.width / 2
}

function getItemDistortion(key: NavKey) {
  if (!dragging.value || !navRef.value) return { stretch: 0, lift: 0, fade: 0 }
  const el = itemRefs.value[key]
  if (!el) return { stretch: 0, lift: 0, fade: 0 }
  const navRect = navRef.value.getBoundingClientRect()
  const rect = el.getBoundingClientRect()
  const center = rect.left - navRect.left + rect.width / 2
  const distance = Math.abs(center - getIndicatorCenter())
  const influence = Math.max(0, 1 - distance / 150)
  return {
    stretch: influence,
    lift: influence * 7,
    fade: influence * 0.18,
  }
}

function getItemStyle(key: NavKey) {
  const { stretch, lift, fade } = getItemDistortion(key)
  if (!stretch) return undefined
  return {
    transform: `translateY(${-lift}px) scale(${1 - stretch * 0.09}, ${1 + stretch * 0.09})`,
    opacity: `${1 - fade}`,
  }
}

function getLabelStyle(key: NavKey) {
  const { stretch } = getItemDistortion(key)
  if (!stretch) return undefined
  return {
    letterSpacing: `${-0.03 * stretch}em`,
    transform: `scale(${1 - stretch * 0.05}, ${1 + stretch * 0.05})`,
  }
}

watch(activeKey, () => nextTick(syncIndicatorToActive))
onMounted(() => {
  nextTick(syncIndicatorToActive)
  window.addEventListener('resize', syncIndicatorToActive)
})
onBeforeUnmount(() => {
  window.removeEventListener('resize', syncIndicatorToActive)
})
</script>

<style scoped>
.bottom-nav-shell {
  position: fixed;
  left: 50%;
  bottom: calc(18px + var(--safe-area-bottom));
  transform: translateX(-50%);
  z-index: calc(var(--z-sticky) + 2);
  width: min(720px, calc(100vw - 24px));
}

.bottom-nav {
  position: relative;
  display: grid;
  grid-template-columns: repeat(auto-fit, minmax(0, 1fr));
  align-items: center;
  gap: 8px;
  padding: 10px;
  border-radius: 999px;
  background: rgba(255, 255, 255, 0.92);
  border: 1px solid rgba(255, 255, 255, 0.75);
  box-shadow:
    0 18px 40px rgba(0, 0, 0, 0.10),
    inset 0 1px 0 rgba(255, 255, 255, 0.85);
  backdrop-filter: blur(22px) saturate(165%);
  -webkit-backdrop-filter: blur(22px) saturate(165%);
}

.bottom-nav-shell.theme-dark .bottom-nav {
  background: rgba(28, 29, 34, 0.82);
  border-color: rgba(255, 255, 255, 0.08);
  box-shadow:
    0 18px 40px rgba(0, 0, 0, 0.36),
    inset 0 1px 0 rgba(255, 255, 255, 0.08);
}

.nav-indicator {
  position: absolute;
  top: 8px;
  bottom: 8px;
  left: 0;
  border-radius: 999px;
  background:
    linear-gradient(180deg, rgba(255, 255, 255, 0.9), rgba(244, 244, 246, 0.68)),
    rgba(255, 255, 255, 0.36);
  border: 1px solid rgba(255, 255, 255, 0.95);
  box-shadow:
    inset 0 1px 0 rgba(255, 255, 255, 0.9),
    0 10px 24px rgba(0, 0, 0, 0.08);
  transition: transform 380ms cubic-bezier(0.2, 0.85, 0.22, 1), width 380ms cubic-bezier(0.2, 0.85, 0.22, 1);
  will-change: transform;
}

.bottom-nav-shell.theme-dark .nav-indicator {
  background:
    linear-gradient(180deg, rgba(255, 255, 255, 0.075), rgba(150, 190, 255, 0.015)),
    rgba(255, 255, 255, 0.02);
  border-color: rgba(255, 255, 255, 0.22);
  box-shadow:
    inset 0 1px 0 rgba(255, 255, 255, 0.3),
    inset 0 -14px 24px rgba(120, 170, 255, 0.02),
    0 8px 18px rgba(0, 0, 0, 0.08),
    0 0 0 1px rgba(255, 255, 255, 0.035);
  backdrop-filter: blur(32px) saturate(200%) brightness(1.08);
  -webkit-backdrop-filter: blur(32px) saturate(200%) brightness(1.08);
}

.nav-item {
  position: relative;
  z-index: 1;
  display: flex;
  align-items: center;
  justify-content: center;
  gap: 8px;
  min-height: 58px;
  border-radius: 999px;
  color: #22242a;
  font-size: var(--text-sm);
  font-weight: 600;
  transition: color 180ms ease, transform 180ms ease;
}

.bottom-nav-shell.theme-dark .nav-item {
  color: rgba(245, 246, 248, 0.88);
}

.nav-item svg {
  width: 22px;
  height: 22px;
  flex-shrink: 0;
}

.nav-item span {
  display: inline-flex;
  transform-origin: center center;
}

.nav-item.active {
  color: #179a57;
}

.bottom-nav-shell.theme-dark .nav-item.active {
  color: #59d38e;
}

.nav-item:active {
  transform: scale(0.98);
}

@media (max-width: 640px) {
  .bottom-nav-shell {
    width: calc(100vw - 18px);
    bottom: calc(12px + var(--safe-area-bottom));
  }

  .bottom-nav {
    gap: 4px;
    padding: 8px;
  }

  .nav-item {
    min-height: 52px;
    gap: 6px;
    font-size: 13px;
  }
}
</style>
