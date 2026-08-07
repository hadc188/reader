import { describe, expect, it, vi } from 'vitest'
import { createReaderProgressAutoSaveScheduler, createReaderProgressExitSaver } from './readerProgressAutoSave'

describe('readerProgressAutoSave', () => {
  it('coalesces repeated schedule calls into one timed flush', () => {
    const timers = new Map<number, () => void>()
    let nextTimerId = 1
    const flush = vi.fn()

    const scheduler = createReaderProgressAutoSaveScheduler({
      intervalMs: 10000,
      flush,
      setTimer: (callback) => {
        const id = nextTimerId++
        timers.set(id, () => {
          timers.delete(id)
          callback()
        })
        return id
      },
      clearTimer: (id) => {
        timers.delete(id as number)
      },
    })

    scheduler.schedule()
    scheduler.schedule()

    expect(timers.size).toBe(1)
    timers.get(1)?.()

    expect(flush).toHaveBeenCalledTimes(1)
    expect(timers.size).toBe(0)
  })

  it('flushes immediately before navigation and clears a pending timer', async () => {
    const timers = new Map<number, () => void>()
    let nextTimerId = 1
    const flush = vi.fn()

    const scheduler = createReaderProgressAutoSaveScheduler({
      intervalMs: 10000,
      flush,
      setTimer: (callback) => {
        const id = nextTimerId++
        timers.set(id, callback)
        return id
      },
      clearTimer: (id) => {
        timers.delete(id as number)
      },
    })

    scheduler.schedule()
    await scheduler.flushNow()

    expect(timers.size).toBe(0)
    expect(flush).toHaveBeenCalledTimes(1)
  })

  it('persists only once when home navigation, route leave, and unmount overlap', async () => {
    const disposeAutoSave = vi.fn()
    const savePosition = vi.fn()
    const flushToServer = vi.fn()
    const flushToServerKeepalive = vi.fn()
    const saver = createReaderProgressExitSaver({
      disposeAutoSave,
      savePosition,
      flushToServer,
      flushToServerKeepalive,
    })

    await saver.flushBeforeRouteLeave()
    await saver.flushBeforeRouteLeave()
    saver.flushKeepalive()

    expect(disposeAutoSave).toHaveBeenCalledTimes(1)
    expect(savePosition).toHaveBeenCalledTimes(1)
    expect(flushToServer).toHaveBeenCalledTimes(1)
    expect(flushToServerKeepalive).not.toHaveBeenCalled()
  })

  it('sends only one keepalive when pagehide, beforeunload, and unmount overlap', () => {
    const disposeAutoSave = vi.fn()
    const savePosition = vi.fn()
    const flushToServer = vi.fn()
    const flushToServerKeepalive = vi.fn()
    const saver = createReaderProgressExitSaver({
      disposeAutoSave,
      savePosition,
      flushToServer,
      flushToServerKeepalive,
    })

    saver.flushKeepalive()
    saver.flushKeepalive()

    expect(disposeAutoSave).toHaveBeenCalledTimes(1)
    expect(savePosition).toHaveBeenCalledTimes(1)
    expect(flushToServer).not.toHaveBeenCalled()
    expect(flushToServerKeepalive).toHaveBeenCalledTimes(1)
  })

  it('does not run a blocking flush after a gesture navigation keepalive already persisted', async () => {
    const disposeAutoSave = vi.fn()
    const savePosition = vi.fn()
    const flushToServer = vi.fn()
    const flushToServerKeepalive = vi.fn()
    const saver = createReaderProgressExitSaver({
      disposeAutoSave,
      savePosition,
      flushToServer,
      flushToServerKeepalive,
    })

    saver.flushKeepalive()
    await saver.flushBeforeRouteLeave()

    expect(disposeAutoSave).toHaveBeenCalledTimes(1)
    expect(savePosition).toHaveBeenCalledTimes(1)
    expect(flushToServer).not.toHaveBeenCalled()
    expect(flushToServerKeepalive).toHaveBeenCalledTimes(1)
  })

  it('allows a final navigation save after temporary background keepalive', async () => {
    const disposeAutoSave = vi.fn()
    const savePosition = vi.fn()
    const flushToServer = vi.fn()
    const flushToServerKeepalive = vi.fn()
    const saver = createReaderProgressExitSaver({
      disposeAutoSave,
      savePosition,
      flushToServer,
      flushToServerKeepalive,
    })

    saver.flushTemporaryKeepalive()
    await saver.flushBeforeRouteLeave()

    expect(disposeAutoSave).toHaveBeenCalledTimes(1)
    expect(savePosition).toHaveBeenCalledTimes(2)
    expect(flushToServerKeepalive).toHaveBeenCalledTimes(1)
    expect(flushToServer).toHaveBeenCalledTimes(1)
  })
})
