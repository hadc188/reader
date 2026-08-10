<template>
  <header class="app-header">
    <div class="header-inner">
      <!-- Left: Logo & Search -->
      <div class="header-left">
        <div class="logo" @click="goHome">
          <svg class="logo-icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
            <path d="M2 3h6a4 4 0 0 1 4 4v14a3 3 0 0 0-3-3H2z" />
            <path d="M22 3h-6a4 4 0 0 0-4 4v14a3 3 0 0 1 3-3h7z" />
          </svg>
          <span class="logo-text">阅读</span>
        </div>

        <div class="search-box" :class="{ focused: searchFocused }">
          <svg class="search-icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
            <circle cx="11" cy="11" r="8" />
            <path d="m21 21-4.3-4.3" />
          </svg>
          <input
            v-model="searchValue"
            type="text"
            placeholder="搜索书籍..."
            @focus="searchFocused = true"
            @blur="searchFocused = false"
            @keyup.enter="handleSearch"
          />
          <button
            v-if="searchValue"
            class="search-clear"
            @click="clearSearch"
          >
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
              <path d="M18 6 6 18M6 6l12 12" />
            </svg>
          </button>
        </div>
      </div>

      <!-- Right: Actions -->
      <div class="header-right">
        <button class="header-btn" :class="{ active: currentSection === 'home' }" @click="goHome" title="书架">
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
            <path d="M4 5.5A2.5 2.5 0 0 1 6.5 3H20" />
            <path d="M4 5.5V19a2 2 0 0 0 2 2h14" />
            <path d="M8 7h8" />
            <path d="M8 11h8" />
            <path d="M8 15h5" />
          </svg>
          <span class="btn-label">书架</span>
        </button>

        <button class="header-btn" :class="{ active: currentSection === 'explore' }" @click="handleExplore" title="书海">
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
            <circle cx="12" cy="12" r="10" />
            <path d="m16.24 7.76-2.12 6.36-6.36 2.12 2.12-6.36 6.36-2.12z" />
          </svg>
          <span class="btn-label">书海</span>
        </button>

        <button class="header-btn" :class="{ active: currentSection === 'recent' }" @click="handleRecent" title="最近阅读">
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
            <path d="M12 7v5l3 2" />
            <circle cx="12" cy="12" r="9" />
          </svg>
          <span class="btn-label">最近</span>
        </button>

        <button class="header-btn" :class="{ active: currentSection === 'rss' }" @click="handleRss" title="RSS">
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
            <path d="M4 11a9 9 0 0 1 9 9" />
            <path d="M4 4a16 16 0 0 1 16 16" />
            <circle cx="5" cy="19" r="1" fill="currentColor" stroke="none" />
          </svg>
          <span class="btn-label">RSS</span>
        </button>

        <button class="header-btn" @click="toggleTheme" title="切换主题">
          <svg v-if="theme === 'light'" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
            <path d="M21 12.79A9 9 0 1 1 11.21 3 7 7 0 0 0 21 12.79z" />
          </svg>
          <svg v-else viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
            <circle cx="12" cy="12" r="4" />
            <path d="M12 2v2M12 20v2M4.93 4.93l1.41 1.41M17.66 17.66l1.41 1.41M2 12h2M20 12h2M6.34 17.66l-1.41 1.41M19.07 4.93l-1.41 1.41" />
          </svg>
        </button>

        <button class="header-btn settings-btn" @click="openSettings" title="设置">
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
            <path d="M12.22 2h-.44a2 2 0 0 0-2 2v.18a2 2 0 0 1-1 1.73l-.43.25a2 2 0 0 1-2 0l-.15-.08a2 2 0 0 0-2.73.73l-.22.38a2 2 0 0 0 .73 2.73l.15.1a2 2 0 0 1 1 1.72v.51a2 2 0 0 1-1 1.74l-.15.09a2 2 0 0 0-.73 2.73l.22.38a2 2 0 0 0 2.73.73l.15-.08a2 2 0 0 1 2 0l.43.25a2 2 0 0 1 1 1.73V20a2 2 0 0 0 2 2h.44a2 2 0 0 0 2-2v-.18a2 2 0 0 1 1-1.73l.43-.25a2 2 0 0 1 2 0l.15.08a2 2 0 0 0 2.73-.73l.22-.39a2 2 0 0 0-.73-2.73l-.15-.08a2 2 0 0 1-1-1.74v-.5a2 2 0 0 1 1-1.74l.15-.09a2 2 0 0 0 .73-2.73l-.22-.38a2 2 0 0 0-2.73-.73l-.15.08a2 2 0 0 1-2 0l-.43-.25a2 2 0 0 1-1-1.73V4a2 2 0 0 0-2-2z" />
            <circle cx="12" cy="12" r="3" />
          </svg>
          <span class="btn-label">设置</span>
        </button>
      </div>
    </div>
  </header>
</template>

<script setup lang="ts">
import { ref, computed } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { useAppStore } from '../stores/app'
import { useBookshelfStore } from '../stores/bookshelf'

const router = useRouter()
const route = useRoute()
const appStore = useAppStore()
const shelfStore = useBookshelfStore()

const searchFocused = ref(false)
const searchValue = ref('')

const theme = computed(() => appStore.theme)
const currentSection = computed(() => {
  if (route.path === '/' || route.path === '') return 'home'
  if (route.path.startsWith('/explore')) return 'explore'
  if (route.path.startsWith('/stats')) return 'recent'
  if (route.path.startsWith('/rss')) return 'rss'
  return ''
})

const emit = defineEmits<{
  explore: []
  recent: []
  rss: []
}>()

function goHome() {
  shelfStore.clearSearch()
  router.replace('/')
}

function handleSearch() {
  if (searchValue.value.trim()) {
    shelfStore.searchKey = searchValue.value.trim()
  }
}

function clearSearch() {
  searchValue.value = ''
  shelfStore.clearSearch()
}

function handleExplore() {
  emit('explore')
}

function handleRss() {
  emit('rss')
}

function handleRecent() {
  emit('recent')
}

function toggleTheme() {
  appStore.toggleTheme()
}

function openSettings() {
  appStore.showSettingsDrawer = true
}
</script>

<style scoped>
.app-header {
  position: sticky;
  top: 0;
  z-index: var(--z-sticky);
  min-height: calc(var(--header-height) + var(--safe-area-top) + 10px);
  padding-top: var(--safe-area-top);
  background: var(--color-bg-elevated);
  border-bottom: 1px solid var(--color-border-light);
  backdrop-filter: blur(12px);
  -webkit-backdrop-filter: blur(12px);
  box-sizing: border-box;
}

.header-inner {
  max-width: var(--content-max-width);
  margin: 0 auto;
  min-height: calc(var(--header-height) + 10px);
  display: flex;
  align-items: center;
  padding: 0 var(--space-6);
  gap: var(--space-5);
}

.header-left {
  display: flex;
  align-items: center;
  gap: var(--space-4);
  flex: 1 1 auto;
  min-width: 0;
}

.logo {
  display: flex;
  align-items: center;
  gap: var(--space-2);
  cursor: pointer;
  flex-shrink: 0;
  transition: opacity var(--duration-fast);
}

.logo:hover {
  opacity: 0.8;
}

.logo-icon {
  width: 28px;
  height: 28px;
  color: var(--color-primary);
}

.logo-text {
  font-size: var(--text-xl);
  font-weight: 700;
  letter-spacing: 0;
  background: linear-gradient(135deg, var(--color-primary), var(--color-primary-dark));
  -webkit-background-clip: text;
  -webkit-text-fill-color: transparent;
  background-clip: text;
}

.search-box {
  display: flex;
  align-items: center;
  gap: var(--space-2);
  background: var(--color-bg-sunken);
  border: 1.5px solid transparent;
  border-radius: var(--radius-full);
  padding: var(--space-2) var(--space-4);
  max-width: 460px;
  flex: 1 1 420px;
  min-width: 220px;
  transition: all var(--duration-normal) var(--ease-out);
}

.search-box.focused {
  border-color: var(--color-primary);
  background: var(--color-bg-elevated);
  box-shadow: 0 0 0 3px var(--color-primary-bg);
}

.search-icon {
  width: 18px;
  height: 18px;
  color: var(--color-text-tertiary);
  flex-shrink: 0;
}

.search-box input {
  flex: 1;
  border: none;
  background: none;
  outline: none;
  font-size: var(--text-sm);
  color: var(--color-text);
  min-width: 0;
}

.search-box input::placeholder {
  color: var(--color-text-tertiary);
}

.search-clear {
  width: 18px;
  height: 18px;
  display: flex;
  align-items: center;
  justify-content: center;
  color: var(--color-text-tertiary);
  flex-shrink: 0;
  padding: 0;
  transition: color var(--duration-fast);
}

.search-clear:hover {
  color: var(--color-text);
}

.search-clear svg {
  width: 14px;
  height: 14px;
}

.header-right {
  display: flex;
  align-items: center;
  justify-content: flex-end;
  gap: 4px;
  flex: 0 0 auto;
  margin-left: auto;
}

.header-btn {
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 10px 12px;
  border-radius: var(--radius-full);
  color: var(--color-text-secondary);
  transition: all var(--duration-fast) var(--ease-out);
  font-size: var(--text-sm);
  white-space: nowrap;
}

.header-btn:hover {
  background: var(--color-bg-elevated);
  color: var(--color-text);
  box-shadow: 0 6px 16px rgba(0, 0, 0, 0.06);
}

.header-btn.active {
  position: relative;
  color: var(--color-text);
  background:
    linear-gradient(180deg, rgba(255, 255, 255, 0.72), rgba(255, 255, 255, 0.34)),
    rgba(255, 255, 255, 0.12);
  border: 1px solid rgba(255, 255, 255, 0.52);
  box-shadow:
    inset 0 1px 0 rgba(255, 255, 255, 0.78),
    0 10px 24px rgba(0, 0, 0, 0.08),
    0 0 0 1px rgba(255, 255, 255, 0.22);
  backdrop-filter: blur(16px) saturate(160%);
  -webkit-backdrop-filter: blur(16px) saturate(160%);
}

.header-btn.active::after {
  content: '';
  position: absolute;
  inset: 1px;
  border-radius: inherit;
  background: linear-gradient(135deg, rgba(255, 255, 255, 0.28), transparent 55%);
  pointer-events: none;
}

.header-btn:active {
  background: var(--color-bg-active);
  transform: scale(0.97);
}

.header-btn svg {
  width: 20px;
  height: 20px;
  flex-shrink: 0;
}

.btn-label {
  font-weight: 600;
}

.header-btn.spinning svg {
  animation: spin 1s linear infinite;
}

@keyframes spin {
  from { transform: rotate(0deg); }
  to { transform: rotate(360deg); }
}

.user-avatar {
  width: 30px;
  height: 30px;
  border-radius: var(--radius-full);
  background: linear-gradient(135deg, var(--color-primary), var(--color-primary-light));
  color: white;
  display: flex;
  align-items: center;
  justify-content: center;
  font-size: var(--text-sm);
  font-weight: 600;
}

/* Mobile */
@media (max-width: 640px) {
  .header-inner {
    padding: 0 var(--space-3);
    gap: var(--space-2);
  }
  .btn-label {
    display: none;
  }
  .logo-text {
    display: none;
  }
  .search-box {
    max-width: none;
    min-width: 0;
    padding: var(--space-2) var(--space-3);
  }
  .header-left {
    gap: var(--space-3);
  }
  .header-right {
    gap: 0;
  }
  .header-btn {
    padding: var(--space-2);
  }
  .header-btn svg {
    width: 18px;
    height: 18px;
  }
  .user-avatar {
    width: 28px;
    height: 28px;
    font-size: 12px;
  }
}
</style>
