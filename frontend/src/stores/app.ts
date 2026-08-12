import { defineStore } from 'pinia'
import { ref, watch, computed } from 'vue'
import { dismissVersionUpdate, getVersionUpdate } from '../api/update'
import { addReadingStats } from '../api/readingStats'
import type { VersionUpdateInfo } from '../types'
import { applySystemTheme } from '../utils/systemUi'
import type { LegadoWebdavConfig } from '../api/webdav'
import { invokeRaw } from '../api/invoke'

export const useAppStore = defineStore('app', () => {
  const STATS_KEY = 'reader-stats'
  const CLOSE_TO_TRAY_KEY = 'reader-close-to-tray'
  const LEGADO_WEBDAV_KEY = 'reader-legado-webdav'
  const LEGADO_SYNC_ENABLED_KEY = 'reader-legado-sync-enabled'
  const BOSS_KEY_KEY = 'reader-boss-key'
  const NETWORK_PROXY_MODE_KEY = 'reader-network-proxy-mode'
  const NETWORK_PROXY_URL_KEY = 'reader-network-proxy-url'
  // ─── Theme ───
  const savedTheme = localStorage.getItem('theme') as 'light' | 'dark' | null
  const legacyReaderNight = localStorage.getItem('reader-isNight') === 'true'
  const theme = ref<'light' | 'dark'>(
    savedTheme || (legacyReaderNight ? 'dark' : 'light')
  )

  function setTheme(value: 'light' | 'dark') {
    theme.value = value
    localStorage.setItem('theme', value)
    applySystemTheme(value)
  }

  function toggleTheme() {
    setTheme(theme.value === 'light' ? 'dark' : 'light')
  }

  watch(theme, (val) => {
    localStorage.setItem('theme', val)
    applySystemTheme(val)
  }, { immediate: true })

  // ─── Window close behavior ───
  const closeToTray = ref(localStorage.getItem(CLOSE_TO_TRAY_KEY) === 'true')

  function setCloseToTray(value: boolean) {
    closeToTray.value = value
    localStorage.setItem(CLOSE_TO_TRAY_KEY, String(value))
  }

  function toggleCloseToTray() {
    setCloseToTray(!closeToTray.value)
  }

  const legadoWebdavConfig = ref<LegadoWebdavConfig>(loadLegadoWebdavConfig())

  function loadLegadoWebdavConfig(): LegadoWebdavConfig {
    try {
      const value = JSON.parse(localStorage.getItem(LEGADO_WEBDAV_KEY) || '{}')
      return {
        url: typeof value.url === 'string' ? value.url : '',
        account: typeof value.account === 'string' ? value.account : '',
        password: typeof value.password === 'string' ? value.password : '',
        directory: typeof value.directory === 'string' && value.directory.trim() ? value.directory : 'legado',
      }
    } catch {
      return { url: '', account: '', password: '', directory: 'legado' }
    }
  }

  function setLegadoWebdavConfig(value: LegadoWebdavConfig) {
    const next = {
      url: value.url.trim(),
      account: value.account.trim(),
      password: value.password,
      directory: value.directory?.trim() || 'legado',
    }
    legadoWebdavConfig.value = next
    localStorage.setItem(LEGADO_WEBDAV_KEY, JSON.stringify(next))
  }

  const legadoSyncEnabled = ref(localStorage.getItem(LEGADO_SYNC_ENABLED_KEY) !== 'disabled')

  function setLegadoSyncEnabled(value: boolean) {
    legadoSyncEnabled.value = value
    localStorage.setItem(LEGADO_SYNC_ENABLED_KEY, value ? 'enabled' : 'disabled')
  }

  const bossKeyEnabled = ref(localStorage.getItem(BOSS_KEY_KEY) === 'enabled')
  const bossKeyShortcut = ref(localStorage.getItem(`${BOSS_KEY_KEY}-shortcut`) || 'CommandOrControl+Shift+H')

  async function applyBossKey() {
    if (typeof window === 'undefined' || !('__TAURI_INTERNALS__' in window)) return
    await invokeRaw('configure_boss_key', {
      shortcut: bossKeyEnabled.value ? bossKeyShortcut.value : null,
    })
  }

  async function setBossKeyEnabled(value: boolean) {
    const previous = bossKeyEnabled.value
    bossKeyEnabled.value = value
    try {
      await applyBossKey()
      localStorage.setItem(BOSS_KEY_KEY, value ? 'enabled' : 'disabled')
    } catch (error) {
      bossKeyEnabled.value = previous
      throw error
    }
  }

  async function setBossKeyShortcut(value: string) {
    const previous = bossKeyShortcut.value
    bossKeyShortcut.value = value
    try {
      await applyBossKey()
      localStorage.setItem(`${BOSS_KEY_KEY}-shortcut`, value)
    } catch (error) {
      bossKeyShortcut.value = previous
      await applyBossKey().catch(() => undefined)
      throw error
    }
  }

  // ─── Network proxy ───
  type NetworkProxyMode = 'system' | 'manual'
  type NetworkProxyStatus = {
    mode: NetworkProxyMode
    active: boolean
    address?: string | null
  }
  const savedProxyMode = localStorage.getItem(NETWORK_PROXY_MODE_KEY)
  const networkProxyMode = ref<NetworkProxyMode>(savedProxyMode === 'manual' ? 'manual' : 'system')
  const networkProxyUrl = ref(localStorage.getItem(NETWORK_PROXY_URL_KEY) || '')
  const networkProxyStatus = ref<NetworkProxyStatus | null>(null)

  async function applyNetworkProxy(
    mode: NetworkProxyMode = networkProxyMode.value,
    proxyUrl: string = networkProxyUrl.value,
  ) {
    if (typeof window === 'undefined' || !('__TAURI_INTERNALS__' in window)) return null
    const status = await invokeRaw<NetworkProxyStatus>('configure_network_proxy', {
      mode,
      proxyUrl: mode === 'manual' ? proxyUrl.trim() : null,
    })
    networkProxyStatus.value = status
    return status
  }

  async function setNetworkProxy(mode: NetworkProxyMode, proxyUrl: string) {
    const normalizedUrl = proxyUrl.trim()
    const status = await applyNetworkProxy(mode, normalizedUrl)
    networkProxyMode.value = mode
    networkProxyUrl.value = normalizedUrl
    localStorage.setItem(NETWORK_PROXY_MODE_KEY, mode)
    localStorage.setItem(NETWORK_PROXY_URL_KEY, normalizedUrl)
    return status
  }

  // ─── 隐藏功能（在设置里可隐藏统计、RSS 等导航入口）───
  const HIDDEN_FEATURES_KEY = 'reader-hidden-features'
  const hiddenFeatures = ref<Set<string>>(new Set(readHiddenFeatures()))

  function readHiddenFeatures(): string[] {
    try {
      const raw = localStorage.getItem(HIDDEN_FEATURES_KEY)
      if (!raw) return []
      const parsed = JSON.parse(raw)
      return Array.isArray(parsed) ? parsed.filter((x) => typeof x === 'string') : []
    } catch {
      return []
    }
  }

  function persistHiddenFeatures() {
    localStorage.setItem(HIDDEN_FEATURES_KEY, JSON.stringify(Array.from(hiddenFeatures.value)))
  }

  function isFeatureHidden(key: string): boolean {
    return hiddenFeatures.value.has(key)
  }

  function toggleHiddenFeature(key: string) {
    if (hiddenFeatures.value.has(key)) {
      hiddenFeatures.value.delete(key)
    } else {
      hiddenFeatures.value.add(key)
    }
    persistHiddenFeatures()
  }

  // ─── User (single-user desktop: no auth) ───
  const versionUpdate = ref<VersionUpdateInfo | null>(null)
  const versionUpdateLoading = ref(false)
  const versionUpdateChecked = ref(false)
  let versionUpdateToastVersion = ''
  const canCheckVersionUpdate = computed(() => true)
  const hasVersionUpdateReminder = computed(() => !!versionUpdate.value?.shouldRemind)

  async function checkVersionUpdate(force = false) {
    if (versionUpdateLoading.value) return versionUpdate.value
    versionUpdateLoading.value = true
    try {
      const info = await getVersionUpdate(force)
      versionUpdate.value = info
      versionUpdateChecked.value = true
      if (info.shouldRemind && info.latestVersion && versionUpdateToastVersion !== info.latestVersion) {
        versionUpdateToastVersion = info.latestVersion
        showToast(`发现新版本 ${info.latestVersion}`, 'warning')
      }
      return info
    } catch (error) {
      if (force) {
        showToast((error as Error).message || '检查更新失败', 'error')
      }
      return null
    } finally {
      versionUpdateLoading.value = false
    }
  }

  async function dismissVersionUpdateReminder(version = versionUpdate.value?.latestVersion || '') {
    if (!version) {
      showToast('当前没有可忽略的版本', 'warning')
      return null
    }
    versionUpdateLoading.value = true
    try {
      const info = await dismissVersionUpdate(version)
      versionUpdate.value = info
      versionUpdateToastVersion = version
      showToast('已忽略当前版本更新提醒', 'success')
      return info
    } catch (error) {
      showToast((error as Error).message || '忽略版本失败', 'error')
      return null
    } finally {
      versionUpdateLoading.value = false
    }
  }

  // ─── UI State ───
  const showLoginModal = ref(false)
  const showSettingsDrawer = ref(false)
  const showSourceManager = ref(false)
  const showUserManager = ref(false)
  const showWebdavManager = ref(false)
  const isOnline = ref(typeof navigator !== 'undefined' ? navigator.onLine : true)
  const pwaUpdateAvailable = ref(false)
  const deferredInstallPrompt = ref<any>(null)
  const waitingServiceWorker = ref<ServiceWorker | null>(null)

  const initialReadingStats = (() => {
    try {
      return JSON.parse(localStorage.getItem(STATS_KEY) || '{"totalSeconds":0,"openedBooks":[],"readChapters":[],"completedBooks":[]}')
    } catch {
      return { totalSeconds: 0, openedBooks: [], readChapters: [], completedBooks: [] }
    }
  })()

  const readingStats = ref<{
    totalSeconds: number
    openedBooks: string[]
    readChapters: string[]
    completedBooks: string[]
  }>(initialReadingStats)
  let readingSessionStartedAt = 0
  let readingSessionBook: { bookUrl: string; bookName: string } | null = null

  function persistStats() {
    localStorage.setItem(STATS_KEY, JSON.stringify(readingStats.value))
  }

  function flushReadingSession() {
    if (!readingSessionStartedAt) return
    const delta = Math.max(0, Math.round((Date.now() - readingSessionStartedAt) / 1000))
    readingStats.value.totalSeconds += delta
    readingSessionStartedAt = 0
    persistStats()
    // Sync to the server-side reading_stats table (best-effort).
    if (delta > 0) {
      void addReadingStats({
        seconds: delta,
        bookUrl: readingSessionBook?.bookUrl,
        bookName: readingSessionBook?.bookName,
      }).catch(() => undefined)
    }
  }

  function setReadingSessionBook(bookUrl?: string, bookName?: string) {
    const normalizedUrl = bookUrl?.trim() || ''
    const nextBook = normalizedUrl
      ? { bookUrl: normalizedUrl, bookName: bookName?.trim() || '未命名书籍' }
      : null

    if (readingSessionBook?.bookUrl === nextBook?.bookUrl) {
      readingSessionBook = nextBook
      return
    }

    const wasRunning = readingSessionStartedAt > 0
    if (wasRunning) flushReadingSession()
    readingSessionBook = nextBook
    if (wasRunning) readingSessionStartedAt = Date.now()
  }

  function startReadingSession(bookUrl?: string, bookName?: string) {
    setReadingSessionBook(bookUrl, bookName)
    if (!readingSessionStartedAt) readingSessionStartedAt = Date.now()
  }

  function stopReadingSession() {
    flushReadingSession()
    readingSessionBook = null
  }

  function markBookOpened(bookUrl: string) {
    if (!readingStats.value.openedBooks.includes(bookUrl)) {
      readingStats.value.openedBooks.push(bookUrl)
      persistStats()
    }
  }

  function markChapterRead(bookUrl: string, index: number, totalChapters: number) {
    const key = `${bookUrl}#${index}`
    if (!readingStats.value.readChapters.includes(key)) {
      readingStats.value.readChapters.push(key)
    }
    if (totalChapters > 0 && index >= totalChapters - 1 && !readingStats.value.completedBooks.includes(bookUrl)) {
      readingStats.value.completedBooks.push(bookUrl)
    }
    persistStats()
  }

  const readingStatsSummary = computed(() => {
    const totalMinutes = Math.floor(readingStats.value.totalSeconds / 60)
    const hours = Math.floor(totalMinutes / 60)
    const minutes = totalMinutes % 60
    return {
      totalSeconds: readingStats.value.totalSeconds,
      totalTimeText: hours ? `${hours}小时${minutes}分钟` : `${Math.max(1, totalMinutes)}分钟`,
      openedBooks: readingStats.value.openedBooks.length,
      readChapters: readingStats.value.readChapters.length,
      completedBooks: readingStats.value.completedBooks.length,
    }
  })

  // ─── Toast ───
  const toasts = ref<Array<{ id: number; message: string; type: string }>>([])
  let toastId = 0

  function showToast(message: string, type: 'success' | 'error' | 'warning' = 'success') {
    const id = ++toastId
    toasts.value.push({ id, message, type })
    setTimeout(() => {
      toasts.value = toasts.value.filter((t) => t.id !== id)
    }, 3000)
  }

  function setOnlineStatus(value: boolean) {
    isOnline.value = value
  }

  function setPwaUpdateAvailable(value: boolean) {
    pwaUpdateAvailable.value = value
  }

  function setWaitingServiceWorker(value: ServiceWorker | null) {
    waitingServiceWorker.value = value
  }

  function setDeferredInstallPrompt(value: any) {
    deferredInstallPrompt.value = value
  }

  async function installPwa() {
    if (!deferredInstallPrompt.value) return false
    deferredInstallPrompt.value.prompt()
    const result = await deferredInstallPrompt.value.userChoice.catch(() => null)
    deferredInstallPrompt.value = null
    return result?.outcome === 'accepted'
  }

  function applyPwaUpdate() {
    if (!waitingServiceWorker.value) return false
    waitingServiceWorker.value.postMessage({ type: 'SKIP_WAITING' })
    return true
  }

  // ─── 全局确认弹窗（替代原生 confirm，避免 Tauri 里显示 tauri.localhost）───
  const confirmState = ref<{
    message: string
    title?: string
    danger?: boolean
    resolve: (ok: boolean) => void
  } | null>(null)

  function confirmDialog(message: string, opts: { title?: string; danger?: boolean } = {}): Promise<boolean> {
    return new Promise((resolve) => {
      confirmState.value = { message, title: opts.title, danger: opts.danger, resolve }
    })
  }

  function resolveConfirm(ok: boolean) {
    confirmState.value?.resolve(ok)
    confirmState.value = null
  }

  return {
    theme, setTheme, toggleTheme,
    closeToTray, setCloseToTray, toggleCloseToTray,
    legadoWebdavConfig, setLegadoWebdavConfig, legadoSyncEnabled, setLegadoSyncEnabled,
    bossKeyEnabled, bossKeyShortcut, applyBossKey, setBossKeyEnabled, setBossKeyShortcut,
    networkProxyMode, networkProxyUrl, networkProxyStatus, applyNetworkProxy, setNetworkProxy,
    versionUpdate, versionUpdateLoading, versionUpdateChecked, canCheckVersionUpdate, hasVersionUpdateReminder,
    checkVersionUpdate, dismissVersionUpdateReminder,
    showLoginModal, showSettingsDrawer, showSourceManager, showUserManager, showWebdavManager,
    isOnline, pwaUpdateAvailable, deferredInstallPrompt, waitingServiceWorker,
    setOnlineStatus, setPwaUpdateAvailable, setDeferredInstallPrompt, setWaitingServiceWorker, installPwa, applyPwaUpdate,
    readingStats, readingStatsSummary, startReadingSession, stopReadingSession, setReadingSessionBook, markBookOpened, markChapterRead,
    toasts, showToast,
    confirmState, confirmDialog, resolveConfirm,
    hiddenFeatures, isFeatureHidden, toggleHiddenFeature,
  }
})
