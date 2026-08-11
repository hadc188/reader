export interface WindowCloseTarget {
  hide: () => Promise<void>
  destroy: () => Promise<void>
}

export async function resolveWindowClose(target: WindowCloseTarget, closeToTray: boolean) {
  if (closeToTray) {
    await target.hide()
    return
  }
  await target.destroy()
}
