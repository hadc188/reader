import type { useAppStore } from '../stores/app'

type AppStore = ReturnType<typeof useAppStore>

function isLocalhostEnv() {
  if (typeof window === 'undefined') return false
  const hostname = window.location.hostname
  return hostname === 'localhost'
    || hostname === '127.0.0.1'
    || hostname === 'tauri.localhost'
    || hostname === 'reader.localhost'
}

async function clearServiceWorkerCaches() {
  if (typeof navigator === 'undefined' || !('serviceWorker' in navigator)) return
  const registrations = await navigator.serviceWorker.getRegistrations()
  await Promise.all(registrations.map((registration) => registration.unregister()))
  if (typeof window !== 'undefined' && 'caches' in window) {
    const keys = await caches.keys()
    await Promise.all(keys.map((key) => caches.delete(key)))
  }
}

export function registerPwa(appStore: AppStore) {
  if (typeof window === 'undefined') return

  appStore.setOnlineStatus(navigator.onLine)

  window.addEventListener('online', () => {
    appStore.setOnlineStatus(true)
    appStore.showToast('网络已恢复', 'success')
  })

  window.addEventListener('offline', () => {
    appStore.setOnlineStatus(false)
    appStore.showToast('已进入离线模式', 'warning')
  })

  window.addEventListener('beforeinstallprompt', (event) => {
    event.preventDefault()
    appStore.setDeferredInstallPrompt(event)
  })

  window.addEventListener('appinstalled', () => {
    appStore.setDeferredInstallPrompt(null)
    appStore.showToast('已安装到主屏幕', 'success')
  })

  if (!('serviceWorker' in navigator)) return

  if (isLocalhostEnv()) {
    window.addEventListener('load', () => {
      void clearServiceWorkerCaches().finally(() => {
        appStore.setPwaReady(false)
      })
    })
    return
  }

  function bindWaitingWorker(worker: ServiceWorker | null | undefined) {
    if (!worker) return
    const wasAvailable = appStore.pwaUpdateAvailable
    appStore.setWaitingServiceWorker(worker)
    appStore.setPwaUpdateAvailable(true)
    worker.postMessage({ type: 'SKIP_WAITING' })
    if (!wasAvailable) {
      appStore.showToast('发现新版本，正在切换', 'success')
    }
  }

  async function checkForUpdates(registration: ServiceWorkerRegistration) {
    try {
      await registration.update()
      if (registration.waiting) {
        bindWaitingWorker(registration.waiting)
      }
    } catch {
      // Ignore update check errors and keep the current app available.
    }
  }

  window.addEventListener('load', () => {
    navigator.serviceWorker.register('/sw.js').then((registration) => {
      appStore.setPwaReady(true)

      if (registration.waiting) {
        bindWaitingWorker(registration.waiting)
      }

      void checkForUpdates(registration)

      registration.addEventListener('updatefound', () => {
        const installing = registration.installing
        if (!installing) return
        installing.addEventListener('statechange', () => {
          if (installing.state === 'installed' && navigator.serviceWorker.controller) {
            bindWaitingWorker(registration.waiting || installing)
          }
        })
      })

      let refreshing = false
      navigator.serviceWorker.addEventListener('controllerchange', () => {
        if (refreshing) return
        refreshing = true
        appStore.setPwaUpdateAvailable(false)
        appStore.setWaitingServiceWorker(null)
        window.location.reload()
      })

      window.setInterval(() => {
        if (!navigator.onLine) return
        void checkForUpdates(registration)
      }, 5 * 60 * 1000)
    }).catch(() => {
      appStore.setPwaReady(false)
    })
  })
}
