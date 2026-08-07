const APP_VIEWPORT_CHANGE_EVENT = 'app-viewport-change'
const STABILIZE_DELAYS = [0, 120, 320, 760]

function getViewportMetrics() {
  if (typeof window === 'undefined') {
    return {
      height: 0,
      width: 0,
      visualHeight: 0,
      visualWidth: 0,
      offsetTop: 0,
      offsetLeft: 0,
      offsetBottom: 0,
      offsetRight: 0,
    }
  }

  const viewport = window.visualViewport
  const layoutHeightCandidates = [window.innerHeight, document.documentElement.clientHeight]
  const layoutWidthCandidates = [window.innerWidth, document.documentElement.clientWidth]
  const validHeights = layoutHeightCandidates.filter(value => Number.isFinite(value) && value > 0)
  const validWidths = layoutWidthCandidates.filter(value => Number.isFinite(value) && value > 0)
  const height = validHeights.length ? Math.max(...validHeights) : 0
  const width = validWidths.length ? Math.max(...validWidths) : 0
  const visualHeight = viewport?.height && viewport.height > 0 ? viewport.height : height
  const visualWidth = viewport?.width && viewport.width > 0 ? viewport.width : width
  const offsetTop = Math.max(0, viewport?.offsetTop || 0)
  const offsetLeft = Math.max(0, viewport?.offsetLeft || 0)
  const offsetBottom = Math.max(0, height - visualHeight - offsetTop)
  const offsetRight = Math.max(0, width - visualWidth - offsetLeft)

  return {
    height,
    width,
    visualHeight,
    visualWidth,
    offsetTop,
    offsetLeft,
    offsetBottom,
    offsetRight,
  }
}

function roundViewportValue(value: number) {
  return Math.round(value * 100) / 100
}

function setViewportCssVariables() {
  if (typeof document === 'undefined') return false

  const root = document.documentElement
  const metrics = getViewportMetrics()
  if (!metrics.height || !metrics.width) return false

  const nextHeight = `${roundViewportValue(metrics.height)}px`
  const nextWidth = `${roundViewportValue(metrics.width)}px`
  const nextVisualHeight = `${roundViewportValue(metrics.visualHeight)}px`
  const nextVisualWidth = `${roundViewportValue(metrics.visualWidth)}px`
  const nextOffsetTop = `${roundViewportValue(metrics.offsetTop)}px`
  const nextOffsetLeft = `${roundViewportValue(metrics.offsetLeft)}px`
  const nextOffsetBottom = `${roundViewportValue(metrics.offsetBottom)}px`
  const nextOffsetRight = `${roundViewportValue(metrics.offsetRight)}px`

  const changed =
    root.style.getPropertyValue('--app-height') !== nextHeight ||
    root.style.getPropertyValue('--app-width') !== nextWidth ||
    root.style.getPropertyValue('--app-visual-height') !== nextVisualHeight ||
    root.style.getPropertyValue('--app-visual-width') !== nextVisualWidth ||
    root.style.getPropertyValue('--viewport-offset-top') !== nextOffsetTop ||
    root.style.getPropertyValue('--viewport-offset-left') !== nextOffsetLeft ||
    root.style.getPropertyValue('--viewport-offset-bottom') !== nextOffsetBottom ||
    root.style.getPropertyValue('--viewport-offset-right') !== nextOffsetRight

  root.style.setProperty('--app-height', nextHeight)
  root.style.setProperty('--app-width', nextWidth)
  root.style.setProperty('--app-visual-height', nextVisualHeight)
  root.style.setProperty('--app-visual-width', nextVisualWidth)
  root.style.setProperty('--viewport-offset-top', nextOffsetTop)
  root.style.setProperty('--viewport-offset-left', nextOffsetLeft)
  root.style.setProperty('--viewport-offset-bottom', nextOffsetBottom)
  root.style.setProperty('--viewport-offset-right', nextOffsetRight)
  root.style.setProperty('height', nextHeight)
  root.style.setProperty('min-height', nextHeight)

  if (document.body) {
    document.body.style.setProperty('height', nextHeight)
    document.body.style.setProperty('min-height', nextHeight)
  }

  if (changed) {
    window.dispatchEvent(new CustomEvent(APP_VIEWPORT_CHANGE_EVENT, { detail: metrics }))
  }

  return changed
}

let cleanupViewportSync: (() => void) | null = null

export function syncViewportSize() {
  return setViewportCssVariables()
}

export function registerViewportSync() {
  if (typeof window === 'undefined') {
    return () => {}
  }

  if (cleanupViewportSync) return cleanupViewportSync

  let rafId: number | null = null
  const timeoutIds = new Set<number>()

  const runSync = () => {
    if (rafId !== null) {
      window.cancelAnimationFrame(rafId)
    }
    rafId = window.requestAnimationFrame(() => {
      rafId = null
      setViewportCssVariables()
    })
  }

  const scheduleSync = () => {
    runSync()
    STABILIZE_DELAYS.forEach((delay) => {
      const timeoutId = window.setTimeout(() => {
        timeoutIds.delete(timeoutId)
        runSync()
      }, delay)
      timeoutIds.add(timeoutId)
    })
  }

  const handleVisibilityChange = () => {
    if (document.visibilityState === 'visible') {
      scheduleSync()
    }
  }

  const viewport = window.visualViewport

  syncViewportSize()
  scheduleSync()

  window.addEventListener('load', scheduleSync)
  window.addEventListener('resize', scheduleSync)
  window.addEventListener('orientationchange', scheduleSync)
  window.addEventListener('pageshow', scheduleSync)
  window.addEventListener('focus', scheduleSync)
  document.addEventListener('visibilitychange', handleVisibilityChange)
  viewport?.addEventListener('resize', scheduleSync)
  viewport?.addEventListener('scroll', scheduleSync)

  cleanupViewportSync = () => {
    if (rafId !== null) {
      window.cancelAnimationFrame(rafId)
      rafId = null
    }
    timeoutIds.forEach(timeoutId => window.clearTimeout(timeoutId))
    timeoutIds.clear()

    window.removeEventListener('load', scheduleSync)
    window.removeEventListener('resize', scheduleSync)
    window.removeEventListener('orientationchange', scheduleSync)
    window.removeEventListener('pageshow', scheduleSync)
    window.removeEventListener('focus', scheduleSync)
    document.removeEventListener('visibilitychange', handleVisibilityChange)
    viewport?.removeEventListener('resize', scheduleSync)
    viewport?.removeEventListener('scroll', scheduleSync)

    cleanupViewportSync = null
  }

  return cleanupViewportSync
}

export { APP_VIEWPORT_CHANGE_EVENT }
