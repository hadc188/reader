type TimerHandle = ReturnType<typeof setTimeout> | number

export interface ReaderProgressAutoSaveSchedulerOptions {
  intervalMs: number
  flush: () => Promise<void> | void
  setTimer?: (callback: () => void, delay: number) => TimerHandle
  clearTimer?: (timer: TimerHandle) => void
}

export interface ReaderProgressExitSaverOptions {
  disposeAutoSave: () => void
  savePosition: () => void
  flushToServer: () => Promise<void> | void
  flushToServerKeepalive: () => void
}

export function createReaderProgressAutoSaveScheduler({
  intervalMs,
  flush,
  setTimer = (callback, delay) => window.setTimeout(callback, delay),
  clearTimer = (timer) => window.clearTimeout(timer as ReturnType<typeof setTimeout>),
}: ReaderProgressAutoSaveSchedulerOptions) {
  let timer: TimerHandle | null = null
  let flushPromise: Promise<void> | null = null

  function clearScheduledFlush() {
    if (timer == null) return
    clearTimer(timer)
    timer = null
  }

  function runFlush() {
    if (flushPromise) return flushPromise
    const current = Promise.resolve(flush())
      .catch(() => undefined)
      .finally(() => {
        if (flushPromise === current) {
          flushPromise = null
        }
      })
    flushPromise = current
    return current
  }

  return {
    schedule() {
      if (timer != null) return
      timer = setTimer(() => {
        timer = null
        void runFlush()
      }, Math.max(0, intervalMs))
    },

    async flushNow() {
      clearScheduledFlush()
      await runFlush()
    },

    dispose() {
      clearScheduledFlush()
    },
  }
}

export function createReaderProgressExitSaver({
  disposeAutoSave,
  savePosition,
  flushToServer,
  flushToServerKeepalive,
}: ReaderProgressExitSaverOptions) {
  let persisted = false
  let routeFlushPromise: Promise<void> | null = null

  function beginPersist() {
    if (persisted) return false
    persisted = true
    disposeAutoSave()
    savePosition()
    return true
  }

  return {
    async flushBeforeRouteLeave() {
      if (routeFlushPromise) return routeFlushPromise
      if (!beginPersist()) return
      routeFlushPromise = Promise.resolve(flushToServer()).catch(() => undefined)
      await routeFlushPromise
    },

    flushKeepalive() {
      if (!beginPersist()) return
      flushToServerKeepalive()
    },

    flushTemporaryKeepalive() {
      savePosition()
      flushToServerKeepalive()
    },
  }
}
