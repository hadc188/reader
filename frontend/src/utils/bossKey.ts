export interface BossKeyCaptureResult {
  shortcut?: string
  error?: string
}

export const DEFAULT_BOSS_KEY_SHORTCUT = 'CommandOrControl+Shift+H'
export const DEFAULT_TITLEBAR_SHORTCUT = 'Alt+T'
export const DEFAULT_BORDERLESS_SHORTCUT = 'Alt+B'
const FUNCTION_KEY_PATTERN = /^F([1-9]|1[0-2])$/

/** 可以单独绑定、不必再配修饰键的主键。
 *
 *  编辑键区与方向键: Insert/Delete/Home/End/PageUp/PageDown 与四个方向键。
 *  它们不会被当作文字输入, 单按也不会在输入框里留下内容, 因此单独绑定不会
 *  误敲出字符。Backspace/Enter/Tab 虽然也在键盘上, 但打字时高频使用, 单独
 *  绑定会把它们从所有程序里抢走, 所以仍要求配修饰键。字母、数字、空格同理。
 *  (issue #14 第 12 点) */
const STANDALONE_KEYS = new Set([
  'ArrowUp',
  'ArrowDown',
  'ArrowLeft',
  'ArrowRight',
  'Delete',
  'End',
  'Home',
  'Insert',
  'PageDown',
  'PageUp',
])

/** 该主键能否省略 Ctrl/Alt/Shift 单独作为老板键。 */
export function canUseStandaloneKey(key: string): boolean {
  return FUNCTION_KEY_PATTERN.test(key) || STANDALONE_KEYS.has(key)
}

export function hasSystemKey(shortcut: string): boolean {
  return shortcut
    .split('+')
    .some((part) => ['super', 'meta', 'win'].includes(part.trim().toLowerCase()))
}

export function normalizeBossKeyShortcut(shortcut: string | null): string {
  const value = shortcut?.trim() || ''
  return value && !hasSystemKey(value) ? value : DEFAULT_BOSS_KEY_SHORTCUT
}

export function normalizeTitlebarShortcut(shortcut: string | null): string {
  const value = shortcut?.trim() || ''
  return value && !hasSystemKey(value) ? value : DEFAULT_TITLEBAR_SHORTCUT
}

export function normalizeBorderlessShortcut(shortcut: string | null): string {
  const value = shortcut?.trim() || ''
  return value && !hasSystemKey(value) ? value : DEFAULT_BORDERLESS_SHORTCUT
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
  if (!event.ctrlKey && !event.altKey && !event.shiftKey) {
    if (canUseStandaloneKey(key)) return { shortcut: key }
    return { error: '这个按键需要搭配 Ctrl、Alt 或 Shift' }
  }

  const parts: string[] = []
  if (event.ctrlKey) parts.push('Control')
  if (event.altKey) parts.push('Alt')
  if (event.shiftKey) parts.push('Shift')
  parts.push(key)
  return { shortcut: parts.join('+') }
}

/** 键名的显示写法。存储与注册仍用事件里的原始名(ArrowLeft/PageDown),
 *  只有界面上换成更好认的符号 —— "ArrowLeft" 这种英文长串挤在按钮里既占宽又难扫。 */
const KEY_DISPLAY: Record<string, string> = {
  ArrowUp: '↑',
  ArrowDown: '↓',
  ArrowLeft: '←',
  ArrowRight: '→',
  PageUp: 'PgUp',
  PageDown: 'PgDn',
  Insert: 'Ins',
}

export function formatBossKey(shortcut: string): string {
  return shortcut
    .replace(/CommandOrControl/gi, 'Ctrl / Command')
    .replace(/Control/gi, 'Ctrl')
    .split('+')
    .map((part) => KEY_DISPLAY[part.trim()] || part)
    .join(' + ')
}

/** 快捷键匹配键盘事件，格式与老板键捕获结果一致。 */
export function matchesShortcut(
  event: Pick<KeyboardEvent, 'key' | 'code' | 'ctrlKey' | 'altKey' | 'shiftKey' | 'metaKey'>,
  shortcut: string,
): boolean {
  if (event.metaKey) return false
  const parts = shortcut.split('+')
  const main = parts.pop() || ''
  const modifiers = new Set(parts.map((part) => part.toLowerCase()))
  if (event.ctrlKey !== modifiers.has('control')) return false
  if (event.altKey !== modifiers.has('alt')) return false
  if (event.shiftKey !== modifiers.has('shift')) return false
  const resolved = resolveMainKey(event as KeyboardEvent)
  return resolved?.toLowerCase() === main.toLowerCase()
}
