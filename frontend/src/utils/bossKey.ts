export interface BossKeyCaptureResult {
  shortcut?: string
  error?: string
}

export const DEFAULT_BOSS_KEY_SHORTCUT = 'CommandOrControl+Shift+H'
const FUNCTION_KEY_PATTERN = /^F([1-9]|1[0-2])$/

export function hasSystemKey(shortcut: string): boolean {
  return shortcut
    .split('+')
    .some((part) => ['super', 'meta', 'win'].includes(part.trim().toLowerCase()))
}

export function normalizeBossKeyShortcut(shortcut: string | null): string {
  const value = shortcut?.trim() || ''
  return value && !hasSystemKey(value) ? value : DEFAULT_BOSS_KEY_SHORTCUT
}

function resolveMainKey(event: KeyboardEvent): string | null {
  if (FUNCTION_KEY_PATTERN.test(event.key.toUpperCase())) return event.key.toUpperCase()
  if (/^Key[A-Z]$/.test(event.code)) return event.code.slice(3)
  if (/^Digit[0-9]$/.test(event.code)) return event.code.slice(5)
  if (event.code === 'Space') return 'Space'

  const keyMap: Record<string, string> = {
    ArrowUp: 'ArrowUp',
    ArrowDown: 'ArrowDown',
    ArrowLeft: 'ArrowLeft',
    ArrowRight: 'ArrowRight',
    Backspace: 'Backspace',
    Delete: 'Delete',
    End: 'End',
    Enter: 'Enter',
    Home: 'Home',
    Insert: 'Insert',
    PageDown: 'PageDown',
    PageUp: 'PageUp',
    Space: 'Space',
    Tab: 'Tab',
  }
  return keyMap[event.key] || null
}

export function captureBossKey(event: KeyboardEvent): BossKeyCaptureResult {
  if (event.key === 'Escape') return { error: 'cancel' }
  if (event.metaKey || event.key === 'Meta' || event.code.startsWith('Meta')) return {}
  if (['Control', 'Alt', 'Shift'].includes(event.key)) return {}

  const key = resolveMainKey(event)
  if (!key) return { error: '不支持这个按键，请换一个按键组合' }
  if (FUNCTION_KEY_PATTERN.test(key) && !event.ctrlKey && !event.altKey && !event.shiftKey) {
    return { shortcut: key }
  }
  if (!event.ctrlKey && !event.altKey && !event.shiftKey) {
    return { error: '快捷键需要包含 Ctrl、Alt 或 Shift' }
  }

  const parts: string[] = []
  if (event.ctrlKey) parts.push('Control')
  if (event.altKey) parts.push('Alt')
  if (event.shiftKey) parts.push('Shift')
  parts.push(key)
  return { shortcut: parts.join('+') }
}

export function formatBossKey(shortcut: string): string {
  return shortcut
    .replace(/CommandOrControl/gi, 'Ctrl / Command')
    .replace(/Control/gi, 'Ctrl')
    .replace(/\+/g, ' + ')
}
