import { defineStore } from 'pinia'
import { ref, watch, computed } from 'vue'
import { applyDesktopVersionUpdate, dismissVersionUpdate, getVersionUpdate } from '../api/update'
import { addReadingStats } from '../api/readingStats'
import type { DesktopUpdateProgress, VersionUpdateInfo } from '../types'
import { applySystemTheme } from '../utils/systemUi'
import type { LegadoWebdavConfig } from '../api/webdav'
import { invokeRaw } from '../api/invoke'
import { normalizeBossKeyShortcut } from '../utils/bossKey'

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
  const savedBossKeyShortcut = localStorage.getItem(`${BOSS_KEY_KEY}-shortcut`)
  const bossKeyShortcut = ref(normalizeBossKeyShortcut(savedBossKeyShortcut))
  if (savedBossKeyShortcut !== bossKeyShortcut.value) {
    localStorage.setItem(`${BOSS_KEY_KEY}-shortcut`, bossKeyShortcut.value)
  }

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
    const normalizedValue = normalizeBossKeyShortcut(value)
    const previous = bossKeyShortcut.value
    bossKeyShortcut.value = normalizedValue
    try {
      await applyBossKey()
      localStorage.setItem(`${BOSS_KEY_KEY}-shortcut`, normalizedValue)
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
  const AUTO_CHECK_UPDATE_KEY = 'reader-auto-check-update'
  const initialAutoCheckUpdate = (() => {
    try {
      return localStorage.getItem(AUTO_CHECK_UPDATE_KEY) !== '0'
    } catch {
      return true
    }
  })()
  /** 启动时自动检查版本更新并在有更新时弹窗提示。 */
  const autoCheckUpdate = ref(initialAutoCheckUpdate)

  function setAutoCheckUpdate(value: boolean) {
    autoCheckUpdate.value = value
    try {
      localStorage.setItem(AUTO_CHECK_UPDATE_KEY, value ? '1' : '0')
    } catch {
      // Ignore unavailable storage; keep runtime state anyway.
    }
  }

  const versionUpdate = ref<VersionUpdateInfo | null>(null)
  const versionUpdateLoading = ref(false)
  const desktopUpdateLoading = ref(false)
  const desktopUpdateProgress = ref<DesktopUpdateProgress | null>(null)
  const versionUpdateChecked = ref(false)
  let versionUpdateToastVersion = ''
  const canCheckVersionUpdate = computed(() => true)
  const hasVersionUpdateReminder = computed(() => !!versionUpdate.value?.shouldRemind)

  async function checkVersionUpdate(
    force = false,
    opts: { notify?: 'toast' | 'dialog'; silent?: boolean } = {},
  ) {
    if (versionUpdateLoading.value) return versionUpdate.value
    versionUpdateLoading.value = true
    try {
      const info = await getVersionUpdate(force)
      versionUpdate.value = info
      versionUpdateChecked.value = true
      if (info.shouldRemind && info.latestVersion && versionUpdateToastVersion !== info.latestVersion) {
        versionUpdateToastVersion = info.latestVersion
        if (opts.notify === 'dialog') {
          void promptVersionUpdate(info)
        } else {
          showToast(`发现新版本 ${info.latestVersion}`, 'warning')
        }
      }
      return info
    } catch (error) {
      if (force && !opts.silent) {
        showToast((error as Error).message || '检查更新失败', 'error')
      }
      return null
    } finally {
      versionUpdateLoading.value = false
    }
  }

  async function promptVersionUpdate(info: VersionUpdateInfo) {
    const go = await confirmDialog(
      `最新版本 ${info.latestVersion} 已发布，是否立即更新？\n（下载已自动接入系统/应用代理，直连失败时会尝试镜像加速）`,
      { title: '发现新版本' },
    )
    if (!go) return
    // 直接在更新进度弹窗中展示下载进度并开始应用内更新。
    updateDialogVisible.value = true
    const result = await applyDesktopUpdate()
    if (!result && desktopUpdateProgress.value?.stage !== 'failed') {
      // 未真正开始下载(如已是最新版本), 不停留在进度弹窗。
      updateDialogVisible.value = false
    }
  }

  /** 应用启动时的自动检查, 受「自动检查更新」开关控制。 */
  async function runStartupVersionCheck() {
    if (!autoCheckUpdate.value) return
    // 强制拉新: 启动检查若读缓存, 上次检查后新发布的版本要等缓存过期才可见,
    // 用户感知就是「打开了应用却没有更新弹窗」。每次启动一次请求远低于限额。
    // 失败静默: 离线/网络波动时不弹错误打扰。
    await checkVersionUpdate(true, { notify: 'dialog', silent: true })
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
  /** 更新进度弹窗(确认更新后直接展示下载进度, 不跳转设置抽屉)。 */
  const updateDialogVisible = ref(false)

  function closeUpdateProgressDialog() {
    updateDialogVisible.value = false
  }
  const showSourceManager = ref(false)
  const showUserManager = ref(false)
  const showWebdavManager = ref(false)
  const isOnline = ref(typeof navigator !== 'undefined' ? navigator.onLine : true)

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
  let readingSessionBook: { bookUrl: string; bookName: string; bookAuthor: string } | null = null

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
        bookAuthor: readingSessionBook?.bookAuthor,
      }).catch(() => undefined)
    }
  }

  async function applyDesktopUpdate() {
    if (desktopUpdateLoading.value) return null
    if (!versionUpdate.value?.updateAvailable) {
      showToast('当前没有可安装的新版本', 'warning')
      return null
    }
    desktopUpdateLoading.value = true
    desktopUpdateProgress.value = {
      stage: 'checking',
      percent: 0,
      downloaded: 0,
      total: 0,
      message: '正在确认最新版本',
    }
    try {
      const result = await applyDesktopVersionUpdate((progress) => {
        desktopUpdateProgress.value = progress
      })
      showToast(result.message, 'success')
      return result
    } catch (error) {
      if (desktopUpdateProgress.value?.stage !== 'failed') {
        desktopUpdateProgress.value = {
          stage: 'failed',
          percent: null,
          downloaded: desktopUpdateProgress.value?.downloaded || 0,
          total: desktopUpdateProgress.value?.total || 0,
          message: '更新失败',
        }
      }
      showToast((error as Error).message || '更新失败', 'error')
      return null
    } finally {
      desktopUpdateLoading.value = false
    }
  }

  function setReadingSessionBook(bookUrl?: string, bookName?: string, bookAuthor?: string) {
    const normalizedUrl = bookUrl?.trim() || ''
    const nextBook = normalizedUrl
      ? { bookUrl: normalizedUrl, bookName: bookName?.trim() || '未命名书籍', bookAuthor: bookAuthor?.trim() || '' }
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

  function startReadingSession(bookUrl?: string, bookName?: string, bookAuthor?: string) {
    setReadingSessionBook(bookUrl, bookName, bookAuthor)
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
    versionUpdate, versionUpdateLoading, desktopUpdateLoading, desktopUpdateProgress, versionUpdateChecked, canCheckVersionUpdate, hasVersionUpdateReminder,
    autoCheckUpdate, setAutoCheckUpdate, runStartupVersionCheck,
    checkVersionUpdate, dismissVersionUpdateReminder, applyDesktopUpdate,
    showLoginModal, showSettingsDrawer, showSourceManager, showUserManager, showWebdavManager,
    updateDialogVisible, closeUpdateProgressDialog,
    isOnline, setOnlineStatus,
    readingStats, readingStatsSummary, startReadingSession, stopReadingSession, setReadingSessionBook, markBookOpened, markChapterRead,
    toasts, showToast,
    confirmState, confirmDialog, resolveConfirm,
    hiddenFeatures, isFeatureHidden, toggleHiddenFeature,
  }
})
