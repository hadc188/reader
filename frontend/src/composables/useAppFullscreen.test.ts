import { describe, expect, it, vi, beforeEach, afterEach } from 'vitest'
import { __resetFullscreenStateForTest, toggleAppFullscreen } from './useAppFullscreen'

/**
 * 全屏是窗口级状态，不随路由变化。早先它只写在 ReaderView 的 keydown 监听里，
 * 离开阅读页后按 F11 毫无反应（issue #33 补充反馈）。这里锁住 composable 本身的
 * 行为：连续切换必须真的来回切，而不是把同一个值设两遍。
 */
describe('useAppFullscreen', () => {
  const setFullscreen = vi.fn()
  let fullscreenState = false

  beforeEach(() => {
    __resetFullscreenStateForTest()
    fullscreenState = false
    setFullscreen.mockReset()
    setFullscreen.mockImplementation(async (next: boolean) => {
      fullscreenState = next
    })
    vi.stubGlobal('window', {
      __TAURI_INTERNALS__: {},
      requestAnimationFrame: (cb: FrameRequestCallback) => {
        cb(0)
        return 0
      },
    })
    vi.doMock('@tauri-apps/api/window', () => ({
      getCurrentWindow: () => ({
        isFullscreen: async () => fullscreenState,
        setFullscreen,
      }),
    }))
  })

  afterEach(() => {
    vi.unstubAllGlobals()
    vi.doUnmock('@tauri-apps/api/window')
    __resetFullscreenStateForTest()
  })

  it('进入全屏后再调用能退出，不需要回到阅读页', async () => {
    await toggleAppFullscreen()
    expect(setFullscreen).toHaveBeenLastCalledWith(true)

    await toggleAppFullscreen()
    expect(setFullscreen).toHaveBeenLastCalledWith(false)
  })

  /**
   * 快速连按两下必须真的「进 → 出」。若只依赖 isFullscreen 的真实状态，
   * 两次 IPC 之间读到的是过期值，结果会停在「进」。
   */
  it('连按两下按期望值取反，不会被过期状态卡住', async () => {
    // 让 setFullscreen 慢一拍，模拟两次按键落在同一次 IPC 往返之内。
    setFullscreen.mockImplementation(async () => {
      await new Promise((resolve) => setTimeout(resolve, 10))
      fullscreenState = !fullscreenState
    })

    const first = toggleAppFullscreen()
    const second = toggleAppFullscreen()
    await Promise.all([first, second])

    expect(setFullscreen).toHaveBeenCalledTimes(2)
    expect(setFullscreen.mock.calls[0][0]).toBe(true)
    expect(setFullscreen.mock.calls[1][0]).toBe(false)
  })

  /** 失败后必须清掉期望值，否则下一次取反方向会相反。 */
  it('切换失败后状态不会错乱', async () => {
    setFullscreen.mockRejectedValueOnce(new Error('boom'))
    await expect(toggleAppFullscreen()).rejects.toThrow('boom')

    const { resetFullscreenIntent } = await import('./useAppFullscreen')
    resetFullscreenIntent()

    setFullscreen.mockImplementation(async (next: boolean) => {
      fullscreenState = next
    })
    await toggleAppFullscreen()
    expect(setFullscreen).toHaveBeenLastCalledWith(true)
  })
})
