import { describe, expect, it } from 'vitest'
import { captureBossKey, formatBossKey } from './bossKey'

function keyboardEvent(overrides: Partial<KeyboardEvent>): KeyboardEvent {
  return {
    key: '',
    code: '',
    ctrlKey: false,
    altKey: false,
    shiftKey: false,
    metaKey: false,
    ...overrides,
  } as KeyboardEvent
}

describe('boss key capture', () => {
  it('converts a modified key into a global shortcut string', () => {
    expect(captureBossKey(keyboardEvent({ key: 'h', code: 'KeyH', ctrlKey: true, shiftKey: true })))
      .toEqual({ shortcut: 'Control+Shift+H' })
  })

  it('allows F1 through F12 without a modifier', () => {
    expect(captureBossKey(keyboardEvent({ key: 'F9', code: 'F9' })))
      .toEqual({ shortcut: 'F9' })
  })

  it('supports the space key with a modifier', () => {
    expect(captureBossKey(keyboardEvent({ key: ' ', code: 'Space', altKey: true })))
      .toEqual({ shortcut: 'Alt+Space' })
  })

  it('rejects unmodified regular keys', () => {
    expect(captureBossKey(keyboardEvent({ key: 'q', code: 'KeyQ' })).error)
      .toContain('Ctrl')
  })

  it('formats stored shortcuts for display', () => {
    expect(formatBossKey('CommandOrControl+Shift+H')).toBe('Ctrl / Command + Shift + H')
  })
})
