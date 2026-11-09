import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'
import { useAppStore } from './app'
import { addReadingStats } from '../api/readingStats'

vi.mock('../api/readingStats', () => ({
  addReadingStats: vi.fn(() => Promise.resolve({ saved: true })),
}))

function installBrowserGlobals() {
  const values = new Map<string, string>()
  const documentListeners = new Map<string, () => void>()
  const windowListeners = new Map<string, () => void>()
  vi.stubGlobal('localStorage', {
    getItem: vi.fn((key: string) => values.get(key) ?? null),
    setItem: vi.fn((key: string, value: string) => values.set(key, value)),
    removeItem: vi.fn((key: string) => values.delete(key)),
  })
  vi.stubGlobal('navigator', { onLine: true })
  const style = () => ({ setProperty: vi.fn() })
  const head = { appendChild: vi.fn() }
  const documentStub = {
    visibilityState: 'visible',
    documentElement: { setAttribute: vi.fn(), style: style() },
    body: { style: style() },
    head,
    getElementById: vi.fn(() => null),
    querySelector: vi.fn(() => null),
    createElement: vi.fn(() => ({ setAttribute: vi.fn(), parentNode: null })),
    addEventListener: vi.fn((type: string, handler: () => void) => {
      documentListeners.set(type, handler)
    }),
    removeEventListener: vi.fn(),
  }
  vi.stubGlobal('document', documentStub)
  vi.stubGlobal('requestAnimationFrame', vi.fn((callback: FrameRequestCallback) => {
    callback(0)
    return 0
  }))
  vi.stubGlobal('window', {
    addEventListener: vi.fn((type: string, handler: () => void) => {
      windowListeners.set(type, handler)
    }),
    removeEventListener: vi.fn(),
  })
  return {
    values,
    /** 触发窗口失焦/聚焦, 对应真实的 pause → resume 路径。 */
    blur: () => windowListeners.get('blur')?.(),
    focus: () => windowListeners.get('focus')?.(),
    setVisibility: (state: 'visible' | 'hidden') => {
      documentStub.visibilityState = state
      documentListeners.get('visibilitychange')?.()
    },
  }
}

describe('app reading session clock', () => {
  let browser: ReturnType<typeof installBrowserGlobals>

  beforeEach(() => {
    vi.restoreAllMocks()
    vi.mocked(addReadingStats).mockClear()
    setActivePinia(createPinia())
    browser = installBrowserGlobals()
    vi.useFakeTimers()
  })

  afterEach(() => {
    vi.useRealTimers()
  })

  it('keeps counting after switching to another book', () => {
    const store = useAppStore()
    store.startReadingSession('book-a', '第一本', '作者')
    vi.advanceTimersByTime(10_000)

    // 换书: setReadingSessionBook 会 flush 旧书时长并立刻 resume 新书计时。
    store.setReadingSessionBook('book-b', '第二本', '作者')
    vi.advanceTimersByTime(5_000)

    store.stopReadingSession()

    // 10s(第一本) + 5s(第二本) 都要计入；换书后计时不能冻结。
    expect(store.readingStats.totalSeconds).toBe(15)
  })

  it('does not double count while the session keeps running', () => {
    const store = useAppStore()
    store.startReadingSession('book-a', '第一本', '作者')
    vi.advanceTimersByTime(4_000)

    // 同一本书重复启动只更新书名, 不重置也不重复累加。
    store.startReadingSession('book-a', '第一本(改名)', '作者')
    vi.advanceTimersByTime(2_000)

    store.stopReadingSession()

    expect(store.readingStats.totalSeconds).toBe(6)
  })

  /** 失焦会 flush 并暂停; 重新聚焦必须能继续累计。
   *
   *  修复前 `flushReadingSession` 把起点归零却留着「正在计时」标志, 于是
   *  `resumeReadingSessionClock` 空转 —— 真实表现是切走一次后台再回来, 时长
   *  不再增长。 */
  it('resumes counting after the window regains focus', () => {
    const store = useAppStore()
    store.startReadingSession('book-a', '第一本', '作者')
    vi.advanceTimersByTime(6_000)

    browser.blur()
    // 失焦期间的时间不计入。
    vi.advanceTimersByTime(30_000)
    browser.focus()
    vi.advanceTimersByTime(4_000)

    store.stopReadingSession()

    expect(store.readingStats.totalSeconds).toBe(10)
  })

  /** 页面隐藏同样暂停, 回到可见后继续。 */
  it('resumes counting after the page becomes visible again', () => {
    const store = useAppStore()
    store.startReadingSession('book-a', '第一本', '作者')
    vi.advanceTimersByTime(5_000)

    browser.setVisibility('hidden')
    vi.advanceTimersByTime(20_000)
    browser.setVisibility('visible')
    vi.advanceTimersByTime(5_000)

    store.stopReadingSession()

    expect(store.readingStats.totalSeconds).toBe(10)
  })

  it('splits a long session into server-sized chunks', async () => {
    const store = useAppStore()
    store.startReadingSession('book-a', '第一本', '作者')
    vi.advanceTimersByTime(2 * 60 * 60 * 1000)

    store.stopReadingSession()
    await vi.waitFor(() => expect(addReadingStats).toHaveBeenCalledTimes(2))

    expect(store.readingStats.totalSeconds).toBe(7200)
    expect(vi.mocked(addReadingStats).mock.calls.map(([params]) => params.seconds)).toEqual([3600, 3600])
    expect(vi.mocked(addReadingStats).mock.calls.every(([params]) => params.bookUrl === 'book-a')).toBe(true)
  })

  it('marks a chapter as read only once', () => {
    const store = useAppStore()

    expect(store.markChapterRead('book-a', 2, 10)).toBe(true)
    expect(store.markChapterRead('book-a', 2, 10)).toBe(false)
    expect(store.readingStats.readChapters.filter((key) => key === 'book-a#2')).toHaveLength(1)
  })
})
