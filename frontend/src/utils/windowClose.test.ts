import { describe, expect, it, vi } from 'vitest'
import { resolveWindowClose } from './windowClose'

function createTarget() {
  return {
    hide: vi.fn(async () => undefined),
    destroy: vi.fn(async () => undefined),
  }
}

describe('window close behavior', () => {
  it('hides the window when close-to-tray is enabled', async () => {
    const target = createTarget()

    await resolveWindowClose(target, true)

    expect(target.hide).toHaveBeenCalledOnce()
    expect(target.destroy).not.toHaveBeenCalled()
  })

  it('destroys the window when close-to-tray is disabled', async () => {
    const target = createTarget()

    await resolveWindowClose(target, false)

    expect(target.destroy).toHaveBeenCalledOnce()
    expect(target.hide).not.toHaveBeenCalled()
  })
})
