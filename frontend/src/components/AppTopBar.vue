<template>
  <header class="app-topbar">
    <div class="topbar-inner">
      <div class="topbar-left">
        <div class="logo" @click="goHome">
          <svg class="logo-icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
            <path d="M2 3h6a4 4 0 0 1 4 4v14a3 3 0 0 0-3-3H2z" />
            <path d="M22 3h-6a4 4 0 0 0-4 4v14a3 3 0 0 1 3-3h7z" />
          </svg>
          <span class="logo-text">阅读</span>
        </div>

        <form
          v-if="showGlobalSearch"
          class="search-box"
          :class="{ focused: searchFocused }"
          role="search"
          @submit.prevent="handleSearch"
        >
          <input
            v-model="searchValue"
            type="text"
            placeholder="搜索书籍..."
            @focus="searchFocused = true"
            @blur="searchFocused = false"
          />
          <button v-if="searchValue" class="search-clear" type="button" @click="clearSearch">
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
              <path d="M18 6 6 18M6 6l12 12" />
            </svg>
          </button>
          <button
            class="search-submit"
            type="submit"
            title="搜索"
            aria-label="搜索"
            :disabled="!canSearch"
          >
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
              <circle cx="11" cy="11" r="8" />
              <path d="m21 21-4.3-4.3" />
            </svg>
          </button>
        </form>
      </div>

      <div class="topbar-right">
        <button class="topbar-btn" type="button" title="书源管理" aria-label="书源管理" @click="openSourceManager">
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
            <path d="M4 19.5V5a2 2 0 0 1 2-2h3v18H6a2 2 0 0 1-2-1.5Z" />
            <path d="M9 5h4v16H9" />
            <path d="m15 4 3-.8 3.8 15.4-3.8.9Z" />
          </svg>
        </button>
        <a class="topbar-btn" href="https://github.com/hadc188/reader" target="_blank" title="GitHub" rel="noopener noreferrer">
    <svg viewBox="0 0 24 24" fill="currentColor">
      <path d="M12 0c-6.626 0-12 5.373-12 12 0 5.302 3.438 9.8 8.207 11.387.599.111.793-.261.793-.577v-2.234c-3.338.726-4.033-1.416-4.033-1.416-.546-1.387-1.333-1.756-1.333-1.756-1.089-.745.083-.729.083-.729 1.205.084 1.839 1.237 1.839 1.237 1.07 1.834 2.807 1.304 3.492.997.107-.775.418-1.305.762-1.604-2.665-.305-5.467-1.334-5.467-5.931 0-1.311.469-2.381 1.236-3.221-.124-.303-.535-1.524.117-3.176 0 0 1.008-.322 3.301 1.23.957-.266 1.983-.399 3.003-.404 1.02.005 2.047.138 3.006.404 2.291-1.552 3.297-1.23 3.297-1.23.653 1.653.242 2.874.118 3.176.77.84 1.235 1.911 1.235 3.221 0 4.609-2.807 5.624-5.479 5.921.43.372.823 1.102.823 2.222v3.293c0 .319.192.694.801.576 4.765-1.589 8.199-6.086 8.199-11.386 0-6.627-5.373-12-12-12z" />
    </svg>
  </a>
  <button class="topbar-btn" @click="toggleTheme" title="切换主题">
          <svg v-if="theme === 'light'" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
            <path d="M21 12.79A9 9 0 1 1 11.21 3 7 7 0 0 0 21 12.79z" />
          </svg>
          <svg v-else viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
            <circle cx="12" cy="12" r="4" />
            <path d="M12 2v2M12 20v2M4.93 4.93l1.41 1.41M17.66 17.66l1.41 1.41M2 12h2M20 12h2M6.34 17.66l-1.41 1.41M19.07 4.93l-1.41 1.41" />
          </svg>
        </button>

        <button class="topbar-btn" @click="openSettings" title="设置">
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
            <path d="M12.22 2h-.44a2 2 0 0 0-2 2v.18a2 2 0 0 1-1 1.73l-.43.25a2 2 0 0 1-2 0l-.15-.08a2 2 0 0 0-2.73.73l-.22.38a2 2 0 0 0 .73 2.73l.15.1a2 2 0 0 1 1 1.72v.51a2 2 0 0 1-1 1.74l-.15.09a2 2 0 0 0-.73 2.73l.22.38a2 2 0 0 0 2.73.73l.15-.08a2 2 0 0 1 2 0l.43.25a2 2 0 0 1 1 1.73V20a2 2 0 0 0 2 2h.44a2 2 0 0 0 2-2v-.18a2 2 0 0 1 1-1.73l.43-.25a2 2 0 0 1 2 0l.15.08a2 2 0 0 0 2.73-.73l.22-.39a2 2 0 0 0-.73-2.73l-.15-.08a2 2 0 0 1-1-1.74v-.5a2 2 0 0 1 1-1.74l.15-.09a2 2 0 0 0 .73-2.73l-.22-.38a2 2 0 0 0-2.73-.73l-.15.08a2 2 0 0 1-2 0l-.43-.25a2 2 0 0 1-1-1.73V4a2 2 0 0 0-2-2z" />
            <circle cx="12" cy="12" r="3" />
          </svg>
          <span v-if="hasVersionUpdateReminder" class="update-indicator" aria-hidden="true"></span>
        </button>
      </div>
    </div>
  </header>
</template>

<script setup lang="ts">
import { computed, ref } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { useAppStore } from '../stores/app'
import { useBookshelfStore } from '../stores/bookshelf'
import { useExploreStore } from '../stores/explore'

const router = useRouter()
const route = useRoute()
const appStore = useAppStore()
const shelfStore = useBookshelfStore()
const exploreStore = useExploreStore()

const searchFocused = ref(false)
const searchValue = ref('')

const theme = computed(() => appStore.theme)
const hasVersionUpdateReminder = computed(() => appStore.hasVersionUpdateReminder)
const showGlobalSearch = computed(() => !route.path.startsWith('/rss') && route.path !== '/stats')
const canSearch = computed(() => searchValue.value.trim().length > 0)

function goHome() {
  shelfStore.clearSearch()
  router.replace('/')
}

function handleSearch() {
  const value = searchValue.value.trim()
  if (!value) return

  shelfStore.startSearch(value, {
    scope: 'all',
    sourceUrl: route.path === '/explore' ? exploreStore.activeSourceUrl : '',
  })

  if (route.path !== '/') {
    router.push('/')
  }
}

function clearSearch() {
  searchValue.value = ''
  shelfStore.clearSearch()
}

function toggleTheme() {
  appStore.toggleTheme()
}

function openSettings() {
  appStore.showSettingsDrawer = true
}

function openSourceManager() {
  appStore.showSourceManager = true
}
</script>

<style scoped>
.app-topbar {
  position: sticky;
  top: 0;
  z-index: var(--z-sticky);
  min-height: calc(var(--header-height) + var(--safe-area-top));
  padding-top: var(--safe-area-top);
  background: color-mix(in srgb, var(--color-bg-elevated) 94%, transparent);
  border-bottom: 1px solid var(--color-divider);
  backdrop-filter: blur(18px) saturate(130%);
  -webkit-backdrop-filter: blur(18px) saturate(130%);
  box-sizing: border-box;
}

.topbar-inner {
  max-width: var(--content-max-width);
  margin: 0 auto;
  min-height: var(--header-height);
  display: flex;
  align-items: center;
  gap: var(--space-4);
  padding: 0 var(--space-6);
}

.topbar-left {
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
}

.logo-icon {
  width: 25px;
  height: 25px;
  color: var(--color-primary);
}

.logo-text {
  font-size: 19px;
  font-weight: 700;
  letter-spacing: 0;
  color: var(--color-text);
}

.search-box {
  display: flex;
  align-items: center;
  gap: var(--space-2);
  background: var(--color-bg);
  border: 1px solid var(--color-border-light);
  border-radius: var(--radius-lg);
  padding: 5px 6px 5px var(--space-4);
  max-width: 500px;
  flex: 1 1 420px;
  min-width: 220px;
  transition: all var(--duration-normal) var(--ease-out);
}

.search-box.focused {
  border-color: var(--color-primary);
  background: var(--color-bg-elevated);
  box-shadow: var(--focus-ring);
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
}

.search-clear svg {
  width: 14px;
  height: 14px;
}

.search-submit {
  width: 30px;
  height: 30px;
  display: flex;
  align-items: center;
  justify-content: center;
  border-radius: var(--radius-md);
  color: var(--color-text-inverse);
  background: var(--color-primary);
  flex-shrink: 0;
  padding: 0;
  transition: transform var(--duration-fast), opacity var(--duration-fast), background var(--duration-fast);
}

.search-submit:hover:not(:disabled) {
  background: var(--color-primary-dark);
}

.search-submit:active:not(:disabled) {
  transform: scale(0.94);
}

.search-submit:disabled {
  color: var(--color-text-tertiary);
  background: transparent;
  opacity: 0.75;
}

.search-submit svg {
  width: 15px;
  height: 15px;
}

.topbar-right {
  display: flex;
  align-items: center;
  justify-content: flex-end;
  gap: 4px;
  flex: 0 0 auto;
  margin-left: auto;
}

.topbar-btn {
  position: relative;
  display: flex;
  align-items: center;
  justify-content: center;
  min-width: 38px;
  min-height: 38px;
  padding: 9px;
  border-radius: var(--radius-lg);
  color: var(--color-text-secondary);
  transition: all var(--duration-fast) var(--ease-out);
}

.update-indicator {
  position: absolute;
  top: 7px;
  right: 7px;
  width: 9px;
  height: 9px;
  border-radius: var(--radius-full);
  background: var(--color-warning);
  border: 2px solid var(--color-bg-elevated);
  box-shadow: 0 0 0 2px rgba(201, 127, 58, 0.14);
}

.topbar-btn:hover {
  background: var(--color-bg-hover);
  color: var(--color-text);
}

.topbar-btn:active {
  background: var(--color-bg-active);
  transform: scale(0.97);
}

.topbar-btn svg {
  width: 20px;
  height: 20px;
  flex-shrink: 0;
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

@media (max-width: 640px) {
  .topbar-inner {
    padding: 0 var(--space-3);
    gap: var(--space-2);
  }

  .logo-text {
    display: none;
  }

  .search-box {
    max-width: none;
    min-width: 0;
    gap: 6px;
    padding: 5px 6px 5px var(--space-3);
  }

  .search-submit {
    width: 28px;
    height: 28px;
  }

  .topbar-left {
    gap: var(--space-3);
  }

  .topbar-btn {
    min-width: 36px;
    min-height: 36px;
    padding: 8px;
  }

  .topbar-btn svg {
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
