import { beforeEach, describe, expect, it, vi } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'
import { useAppStore } from './app'

function installBrowserGlobals() {
  vi.stubGlobal('localStorage', {
    getItem: vi.fn(() => null),
    setItem: vi.fn(() => undefined),
    removeItem: vi.fn(() => undefined),
  })
  vi.stubGlobal('navigator', { onLine: true })
}

describe('toast dismiss', () => {
  beforeEach(() => {
    vi.restoreAllMocks()
    vi.useRealTimers()
    setActivePinia(createPinia())
    installBrowserGlobals()
  })

  it('adds a toast and removes it after the timeout', () => {
    vi.useFakeTimers()
    const store = useAppStore()

    store.showToast('已保存', 'success')
    expect(store.toasts).toHaveLength(1)
    expect(store.toasts[0]?.message).toBe('已保存')

    vi.advanceTimersByTime(3000)
    expect(store.toasts).toHaveLength(0)
  })

  it('dismisses immediately when clicked', () => {
    vi.useFakeTimers()
    const store = useAppStore()

    store.showToast('已保存')
    const id = store.toasts[0]!.id
    store.dismissToast(id)

    expect(store.toasts).toHaveLength(0)
  })

  /**
   * 点击关闭后, 原本的自动消失定时器要被清掉(不留悬空回调)。
   * 注意: 旧实现按唯一 id 过滤, 残留定时器并不会误删别的提示, 因此这里只
   * 断言"被点掉的那条不会复活"。
   */
  it('does not resurrect a dismissed toast when its original timer would fire', () => {
    vi.useFakeTimers()
    const store = useAppStore()

    store.showToast('第一条')
    store.dismissToast(store.toasts[0]!.id)
    expect(store.toasts).toHaveLength(0)

    // 走过第一条原本的 3 秒到期点, 它必须保持消失。
    vi.advanceTimersByTime(3000)
    expect(store.toasts).toHaveLength(0)

    // 新提示有自己独立的生命周期。
    store.showToast('第二条')
    expect(store.toasts).toHaveLength(1)
  })

  it('only dismisses the clicked toast when several are visible', () => {
    vi.useFakeTimers()
    const store = useAppStore()

    store.showToast('A')
    store.showToast('B')
    store.showToast('C')
    expect(store.toasts).toHaveLength(3)

    store.dismissToast(store.toasts[1]!.id)

    expect(store.toasts.map((t) => t.message)).toEqual(['A', 'C'])
    vi.advanceTimersByTime(3000)
    expect(store.toasts).toHaveLength(0)
  })

  it('is a no-op for an unknown id', () => {
    const store = useAppStore()

    store.showToast('唯一一条')
    expect(() => store.dismissToast(99999)).not.toThrow()
    expect(store.toasts).toHaveLength(1)
  })
})
