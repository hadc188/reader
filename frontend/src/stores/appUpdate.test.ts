import { beforeEach, describe, expect, it, vi } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'
import { useAppStore } from './app'
import { applyDesktopVersionUpdate, getVersionUpdate, dismissVersionUpdate } from '../api/update'

vi.mock('../api/update', () => ({
  getVersionUpdate: vi.fn(),
  dismissVersionUpdate: vi.fn(),
  applyDesktopVersionUpdate: vi.fn(),
}))

function installBrowserGlobals() {
  const values = new Map<string, string>()
  vi.stubGlobal('localStorage', {
    getItem: vi.fn((key: string) => values.get(key) ?? null),
    setItem: vi.fn((key: string, value: string) => {
      values.set(key, value)
    }),
    removeItem: vi.fn((key: string) => {
      values.delete(key)
    }),
  })
  vi.stubGlobal('navigator', { onLine: true })
}

describe('app update reminders', () => {
  beforeEach(() => {
    vi.restoreAllMocks()
    installBrowserGlobals()
    setActivePinia(createPinia())
  })

  it('stores active release reminders and shows one admin toast', async () => {
    vi.mocked(getVersionUpdate).mockResolvedValue({
      currentVersion: 'v1.0.5',
      latestVersion: 'v1.0.6',
      latestName: 'v1.0.6',
      releaseUrl: 'https://github.com/hadc188/reader/releases/tag/v1.0.6',
      publishedAt: '2026-05-15T08:00:00Z',
      updateAvailable: true,
      shouldRemind: true,
      dismissedVersion: null,
      checkedAt: 1_778_828_800,
      error: null,
      assets: [],
    })
    const store = useAppStore()

    await store.checkVersionUpdate()

    expect(store.versionUpdate?.latestVersion).toBe('v1.0.6')
    expect(store.hasVersionUpdateReminder).toBe(true)
    expect(store.toasts.map((toast) => toast.message)).toContain('发现新版本 v1.0.6')
  })

  it('dismisses the current latest release reminder', async () => {
    vi.mocked(getVersionUpdate).mockResolvedValue({
      currentVersion: 'v1.0.5',
      latestVersion: 'v1.0.6',
      latestName: 'v1.0.6',
      releaseUrl: 'https://github.com/hadc188/reader/releases/tag/v1.0.6',
      publishedAt: '2026-05-15T08:00:00Z',
      updateAvailable: true,
      shouldRemind: true,
      dismissedVersion: null,
      checkedAt: 1_778_828_800,
      error: null,
      assets: [],
    })
    vi.mocked(dismissVersionUpdate).mockResolvedValue({
      currentVersion: 'v1.0.5',
      latestVersion: 'v1.0.6',
      latestName: 'v1.0.6',
      releaseUrl: 'https://github.com/hadc188/reader/releases/tag/v1.0.6',
      publishedAt: '2026-05-15T08:00:00Z',
      updateAvailable: true,
      shouldRemind: false,
      dismissedVersion: 'v1.0.6',
      checkedAt: 1_778_828_800,
      error: null,
      assets: [],
    })
    const store = useAppStore()
    await store.checkVersionUpdate()

    await store.dismissVersionUpdateReminder()

    expect(dismissVersionUpdate).toHaveBeenCalledWith('v1.0.6')
    expect(store.hasVersionUpdateReminder).toBe(false)
    expect(store.versionUpdate?.dismissedVersion).toBe('v1.0.6')
  })

  it('starts the desktop updater for an available release', async () => {
    vi.mocked(getVersionUpdate).mockResolvedValue({
      currentVersion: 'v1.0.5',
      latestVersion: 'v1.0.6',
      latestName: 'v1.0.6',
      releaseUrl: 'https://github.com/hadc188/reader/releases/tag/v1.0.6',
      publishedAt: '2026-05-15T08:00:00Z',
      updateAvailable: true,
      shouldRemind: true,
      dismissedVersion: null,
      checkedAt: 1_778_828_800,
      error: null,
      assets: [],
    })
    vi.mocked(applyDesktopVersionUpdate).mockImplementation(async (onProgress) => {
      onProgress?.({
        stage: 'downloading',
        percent: 48,
        downloaded: 48,
        total: 100,
        message: '正在下载更新文件',
      })
      return {
        mode: 'installer',
        platform: 'windows',
        assetName: 'Reader-v1.0.6-windows-x64-setup.exe',
        message: '更新已下载',
      }
    })
    const store = useAppStore()
    await store.checkVersionUpdate()

    await store.applyDesktopUpdate()

    expect(applyDesktopVersionUpdate).toHaveBeenCalledOnce()
    expect(store.desktopUpdateProgress?.percent).toBe(48)
  })

  it('keeps the failure stage when an update download fails', async () => {
    const store = useAppStore()
    store.versionUpdate = {
      currentVersion: 'v1.0.5',
      latestVersion: 'v1.0.6',
      latestName: 'v1.0.6',
      releaseUrl: null,
      publishedAt: null,
      updateAvailable: true,
      shouldRemind: true,
      dismissedVersion: null,
      checkedAt: 1,
      error: null,
      assets: [],
    }
    vi.mocked(applyDesktopVersionUpdate).mockImplementation(async (onProgress) => {
      onProgress?.({
        stage: 'downloading',
        percent: 25,
        downloaded: 25,
        total: 100,
        message: '正在下载更新文件',
      })
      throw new Error('下载中断')
    })

    await store.applyDesktopUpdate()

    expect(store.desktopUpdateProgress?.stage).toBe('failed')
    expect(store.desktopUpdateProgress?.message).toBe('更新失败')
    expect(store.toasts.at(-1)?.message).toContain('下载中断')
  })
})
