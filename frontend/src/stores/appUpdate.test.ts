import { beforeEach, describe, expect, it, vi } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'
import { useAppStore } from './app'
import { getVersionUpdate, dismissVersionUpdate } from '../api/update'

vi.mock('../api/user', () => ({
  getUserInfo: vi.fn(),
}))

vi.mock('../api/update', () => ({
  getVersionUpdate: vi.fn(),
  dismissVersionUpdate: vi.fn(),
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
      releaseUrl: 'https://github.com/givenge/reader-rust/releases/tag/v1.0.6',
      publishedAt: '2026-05-15T08:00:00Z',
      updateAvailable: true,
      shouldRemind: true,
      dismissedVersion: null,
      checkedAt: 1_778_828_800,
      error: null,
    })
    const store = useAppStore()

    await store.checkVersionUpdate()

    expect(store.versionUpdate?.latestVersion).toBe('v1.0.6')
    expect(store.hasVersionUpdateReminder).toBe(true)
    expect(store.toasts.map((toast) => toast.message)).toContain('发现服务端新版本 v1.0.6')
  })

  it('dismisses the current latest release reminder', async () => {
    vi.mocked(getVersionUpdate).mockResolvedValue({
      currentVersion: 'v1.0.5',
      latestVersion: 'v1.0.6',
      latestName: 'v1.0.6',
      releaseUrl: 'https://github.com/givenge/reader-rust/releases/tag/v1.0.6',
      publishedAt: '2026-05-15T08:00:00Z',
      updateAvailable: true,
      shouldRemind: true,
      dismissedVersion: null,
      checkedAt: 1_778_828_800,
      error: null,
    })
    vi.mocked(dismissVersionUpdate).mockResolvedValue({
      currentVersion: 'v1.0.5',
      latestVersion: 'v1.0.6',
      latestName: 'v1.0.6',
      releaseUrl: 'https://github.com/givenge/reader-rust/releases/tag/v1.0.6',
      publishedAt: '2026-05-15T08:00:00Z',
      updateAvailable: true,
      shouldRemind: false,
      dismissedVersion: 'v1.0.6',
      checkedAt: 1_778_828_800,
      error: null,
    })
    const store = useAppStore()
    await store.checkVersionUpdate()

    await store.dismissVersionUpdateReminder()

    expect(dismissVersionUpdate).toHaveBeenCalledWith('v1.0.6')
    expect(store.hasVersionUpdateReminder).toBe(false)
    expect(store.versionUpdate?.dismissedVersion).toBe('v1.0.6')
  })
})
