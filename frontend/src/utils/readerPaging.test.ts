import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import {
  READER_PAGE_ANIMATION_MAX_MS,
  animatePageScrollBy,
  animatePageScrollTo,
  cancelPageScrollAnimation,
  getReaderPageStep,
  normalizePageAnimationDuration,
} from './readerPaging'

function createContainer(options: { scrollHeight: number; clientHeight: number; scrollTop?: number }) {
  return {
    scrollTop: options.scrollTop ?? 0,
    scrollHeight: options.scrollHeight,
    clientHeight: options.clientHeight,
  } as unknown as HTMLElement
}

/** 受控的 requestAnimationFrame：帧由测试显式推进，补间过程可逐步断言。 */
function installFrameDriver() {
  let nextHandle = 1
  const pending = new Map<number, FrameRequestCallback>()
  let now = 0

  vi.stubGlobal('performance', { now: () => now })
  vi.stubGlobal('requestAnimationFrame', (callback: FrameRequestCallback) => {
    const handle = nextHandle
    nextHandle += 1
    pending.set(handle, callback)
    return handle
  })
  vi.stubGlobal('cancelAnimationFrame', (handle: number) => {
    pending.delete(handle)
  })

  return {
    /** 推进到指定时刻并执行当前待处理的帧回调。 */
    advance(ms: number) {
      now += ms
      const callbacks = Array.from(pending.entries())
      pending.clear()
      callbacks.forEach(([, callback]) => callback(now))
    },
    pendingCount: () => pending.size,
  }
}

describe('normalizePageAnimationDuration', () => {
  it('treats zero and negative durations as animation off', () => {
    expect(normalizePageAnimationDuration(0)).toBe(0)
    expect(normalizePageAnimationDuration(-100)).toBe(0)
  })

  it('falls back to off for non-finite input', () => {
    expect(normalizePageAnimationDuration(Number.NaN)).toBe(0)
    expect(normalizePageAnimationDuration(Number.POSITIVE_INFINITY)).toBe(0)
  })

  it('keeps normal durations and clamps oversized ones', () => {
    expect(normalizePageAnimationDuration(300)).toBe(300)
    expect(normalizePageAnimationDuration(5000)).toBe(READER_PAGE_ANIMATION_MAX_MS)
  })
})

describe('getReaderPageStep', () => {
  it('snaps the step to whole lines so the next page starts at a line boundary', () => {
    // 900 / 32.4 = 27.7 行 → 27 行整
    expect(getReaderPageStep(900, 32.4)).toBe(Math.floor(27 * 32.4))
    expect(getReaderPageStep(900, 32.4)).toBeLessThan(900)
  })

  it('never scrolls less than one line and stays positive', () => {
    expect(getReaderPageStep(900, 900)).toBe(900)
    expect(getReaderPageStep(900, 1200)).toBe(900)
    expect(getReaderPageStep(900, 32)).toBeGreaterThan(0)
  })

  it('falls back to a near-full viewport when the line height is unknown', () => {
    expect(getReaderPageStep(1000, 0)).toBe(900)
    expect(getReaderPageStep(1000, Number.NaN)).toBe(900)
  })

  it('returns zero for a degenerate viewport instead of scrolling backwards', () => {
    expect(getReaderPageStep(0, 32)).toBe(0)
    expect(getReaderPageStep(Number.NaN, 32)).toBe(0)
  })
})

describe('animatePageScrollTo', () => {
  let frames: ReturnType<typeof installFrameDriver>

  beforeEach(() => {
    frames = installFrameDriver()
  })

  afterEach(() => {
    vi.unstubAllGlobals()
  })

  it('jumps straight to the target when the animation is off', () => {
    const container = createContainer({ scrollHeight: 3000, clientHeight: 900 })

    animatePageScrollTo(container, 874, 0)

    expect(container.scrollTop).toBe(874)
    expect(frames.pendingCount()).toBe(0)
  })

  it('eases towards the target over the configured duration', () => {
    const container = createContainer({ scrollHeight: 3000, clientHeight: 900 })

    animatePageScrollTo(container, 900, 300)
    frames.advance(150)
    const midway = container.scrollTop
    expect(midway).toBeGreaterThan(0)
    expect(midway).toBeLessThan(900)

    frames.advance(150)
    expect(container.scrollTop).toBe(900)
    expect(frames.pendingCount()).toBe(0)
  })

  it('clamps the target to the scrollable range', () => {
    const container = createContainer({ scrollHeight: 2000, clientHeight: 900 })

    animatePageScrollTo(container, 99999, 0)

    expect(container.scrollTop).toBe(1100)
  })

  it('cancels the running animation before starting the next one', () => {
    const container = createContainer({ scrollHeight: 5000, clientHeight: 900 })

    animatePageScrollTo(container, 900, 300)
    frames.advance(150)
    animatePageScrollTo(container, 1800, 300)
    frames.advance(300)

    expect(container.scrollTop).toBe(1800)
    expect(frames.pendingCount()).toBe(0)
  })

  it('stops moving once cancelled', () => {
    const container = createContainer({ scrollHeight: 5000, clientHeight: 900 })

    animatePageScrollTo(container, 900, 300)
    frames.advance(150)
    const stopped = container.scrollTop
    cancelPageScrollAnimation(container)
    frames.advance(1000)

    expect(container.scrollTop).toBe(stopped)
  })

  it('yields when something else moves the container', () => {
    // 连续阅读插入上一章后会用 offsetTop 差值校正 scrollTop; 位置恢复也会直接
    // 赋值。补间必须让位, 否则下一帧就把校正覆盖掉。
    const container = createContainer({ scrollHeight: 5000, clientHeight: 900 })

    animatePageScrollTo(container, 900, 300)
    frames.advance(150)
    container.scrollTop = 4321
    frames.advance(16)

    expect(container.scrollTop).toBe(4321)
    expect(frames.pendingCount()).toBe(0)
  })

  it('tolerates sub-pixel rounding of its own writes', () => {
    const container = createContainer({ scrollHeight: 5000, clientHeight: 900 })

    animatePageScrollTo(container, 900, 300)
    frames.advance(150)
    // 浏览器按 DPR 量化 scrollTop, 不能被误判成外部改动。
    container.scrollTop = Math.round(container.scrollTop) + 1
    frames.advance(150)

    expect(container.scrollTop).toBe(900)
  })

  it('ignores a non-finite target instead of writing NaN into scrollTop', () => {
    const container = createContainer({ scrollHeight: 3000, clientHeight: 900, scrollTop: 120 })

    animatePageScrollTo(container, Number.NaN, 300)

    expect(container.scrollTop).toBe(120)
    expect(frames.pendingCount()).toBe(0)
  })
})

describe('animatePageScrollBy', () => {
  beforeEach(() => {
    installFrameDriver()
  })

  afterEach(() => {
    vi.unstubAllGlobals()
  })

  it('moves by the delta relative to the current position', () => {
    const container = createContainer({ scrollHeight: 3000, clientHeight: 900, scrollTop: 200 })

    animatePageScrollBy(container, 800, 0)

    expect(container.scrollTop).toBe(1000)
  })

  it('ignores a zero or non-finite delta', () => {
    const container = createContainer({ scrollHeight: 3000, clientHeight: 900, scrollTop: 200 })

    animatePageScrollBy(container, 0, 300)
    animatePageScrollBy(container, Number.NaN, 300)

    expect(container.scrollTop).toBe(200)
  })
})
