import { beforeEach, describe, expect, it, vi } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'
import { useAppStore } from './app'

function installBrowserGlobals(initial: Record<string, string> = {}) {
  const values = new Map(Object.entries(initial))
  vi.stubGlobal('localStorage', {
    getItem: vi.fn((key: string) => values.get(key) ?? null),
    setItem: vi.fn((key: string, value: string) => values.set(key, value)),
    removeItem: vi.fn((key: string) => values.delete(key)),
  })
  vi.stubGlobal('navigator', { onLine: true })
  return values
}

describe('app close preference', () => {
  beforeEach(() => {
    vi.restoreAllMocks()
    setActivePinia(createPinia())
  })

  it('defaults to exiting and persists close-to-tray changes', () => {
    const values = installBrowserGlobals()
    const store = useAppStore()

    expect(store.closeToTray).toBe(false)
    store.setCloseToTray(true)

    expect(store.closeToTray).toBe(true)
    expect(values.get('reader-close-to-tray')).toBe('true')
  })

  it('restores the saved close-to-tray preference', () => {
    installBrowserGlobals({ 'reader-close-to-tray': 'true' })

    expect(useAppStore().closeToTray).toBe(true)
  })
})
