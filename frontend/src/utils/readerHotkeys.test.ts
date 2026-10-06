import { describe, expect, it } from 'vitest'
import {
  DEFAULT_HOTKEY_BINDINGS,
  HOTKEY_ACTIONS,
  HOTKEY_ACTION_MAP,
  HOTKEY_KEYS,
  applicableActionsFor,
  bindActionToKey,
  hasHotkeyConflict,
  hotkeyIdFromEvent,
  hotkeyLabel,
  isActionAllowedWhilePanelOpen,
  isDefaultHotkeyBindings,
  isValidHotkeyId,
  listHotkeyConflicts,
  normalizeHotkeyBindings,
  resolveHotkeyAction,
  unbindActionFromKey,
} from './readerHotkeys'

describe('hotkeyIdFromEvent', () => {
  it('maps the navigation keys the reader binds by default', () => {
    expect(hotkeyIdFromEvent({ key: 'ArrowUp' })).toBe('ArrowUp')
    expect(hotkeyIdFromEvent({ key: 'PageDown' })).toBe('PageDown')
    expect(hotkeyIdFromEvent({ key: 'Home' })).toBe('Home')
    expect(hotkeyIdFromEvent({ key: 'Escape' })).toBe('Escape')
    expect(hotkeyIdFromEvent({ key: 'F11' })).toBe('F11')
  })

  it('normalises the space bar to a stable id', () => {
    expect(hotkeyIdFromEvent({ key: ' ' })).toBe('Space')
    expect(hotkeyIdFromEvent({ key: 'Space' })).toBe('Space')
  })

  it('supports rebinding function-style combinations', () => {
    expect(hotkeyIdFromEvent({ key: 'f', ctrlKey: true })).toBe('Control+f')
  })

  it('accepts arbitrary keys so the user can bind their own habit', () => {
    expect(hotkeyIdFromEvent({ key: 'a' })).toBe('a')
    expect(hotkeyIdFromEvent({ key: 'F5' })).toBe('F5')
    expect(hotkeyIdFromEvent({ key: 'Enter' })).toBe('Enter')
    expect(hotkeyIdFromEvent({ key: 'Tab' })).toBe('Tab')
    expect(hotkeyIdFromEvent({ key: '3' })).toBe('3')
  })

  it('records modifiers in a fixed order so one chord has one spelling', () => {
    expect(hotkeyIdFromEvent({ key: 'k', ctrlKey: true, shiftKey: true })).toBe('Control+Shift+k')
    expect(hotkeyIdFromEvent({ key: 'k', shiftKey: true, ctrlKey: true })).toBe('Control+Shift+k')
    expect(hotkeyIdFromEvent({ key: 'ArrowDown', altKey: true })).toBe('Alt+ArrowDown')
  })

  it('distinguishes a chord from its bare key', () => {
    expect(hotkeyIdFromEvent({ key: 'ArrowDown' })).toBe('ArrowDown')
    expect(hotkeyIdFromEvent({ key: 'ArrowDown', ctrlKey: true })).toBe('Control+ArrowDown')
    expect(hotkeyIdFromEvent({ key: 'f', ctrlKey: true, shiftKey: true })).toBe('Control+Shift+f')
  })

  it('ignores bare modifier presses, which carry no meaning', () => {
    expect(hotkeyIdFromEvent({ key: 'Control', ctrlKey: true })).toBeNull()
    expect(hotkeyIdFromEvent({ key: 'Shift', shiftKey: true })).toBeNull()
    expect(hotkeyIdFromEvent({ key: 'Alt', altKey: true })).toBeNull()
    expect(hotkeyIdFromEvent({ key: 'Meta', metaKey: true })).toBeNull()
  })

  it('rejects an empty or unusable key', () => {
    expect(hotkeyIdFromEvent({ key: '' })).toBeNull()
  })

  it('lower-cases single characters so Shift does not create a second binding', () => {
    expect(hotkeyIdFromEvent({ key: 'K', shiftKey: true })).toBe('Shift+k')
  })
})

describe('hotkeyLabel', () => {
  it('renders friendly names for the well-known keys', () => {
    expect(hotkeyLabel('ArrowDown')).toBe('下键')
    expect(hotkeyLabel('Space')).toBe('空格')
    expect(hotkeyLabel('PageUp')).toBe('PgUp')
    expect(hotkeyLabel('Escape')).toBe('Esc')
  })

  it('renders a chord readably', () => {
    expect(hotkeyLabel('Control+f')).toBe('Ctrl+F')
    expect(hotkeyLabel('Control+Shift+k')).toBe('Ctrl+Shift+K')
    expect(hotkeyLabel('Alt+ArrowDown')).toBe('Alt+下键')
  })

  it('falls back to the raw key for anything not in the table', () => {
    expect(hotkeyLabel('F5')).toBe('F5')
  })
})

describe('isValidHotkeyId', () => {
  it('accepts preset keys, chords and free-form keys', () => {
    expect(isValidHotkeyId('ArrowUp')).toBe(true)
    expect(isValidHotkeyId('Control+f')).toBe(true)
    expect(isValidHotkeyId('Control+Shift+k')).toBe(true)
    expect(isValidHotkeyId('F5')).toBe(true)
  })

  it('rejects malformed or hostile identifiers from storage', () => {
    expect(isValidHotkeyId('')).toBe(false)
    expect(isValidHotkeyId(null)).toBe(false)
    expect(isValidHotkeyId(42)).toBe(false)
    expect(isValidHotkeyId('Control+')).toBe(false)
    expect(isValidHotkeyId('Control')).toBe(false)
    expect(isValidHotkeyId('Control+Control+a')).toBe(false)
    expect(isValidHotkeyId('Control+Shift+Alt+Meta+Super+x')).toBe(false)
    expect(isValidHotkeyId('a'.repeat(30))).toBe(false)
  })
})

describe('isActionAllowedWhilePanelOpen (弹层打开时的快捷键)', () => {
  // 回归测试: 曾只放行 fullscreen/search/back, 于是「目录」键能打开目录却关不掉
  // —— 用户按同一键没有任何反应, 只能点 X 或遮罩。
  it('lets the panel-toggle actions through so the same key can close the panel', () => {
    expect(isActionAllowedWhilePanelOpen('catalog')).toBe(true)
    expect(isActionAllowedWhilePanelOpen('settings')).toBe(true)
  })

  it('keeps the interface-level actions working over a panel', () => {
    expect(isActionAllowedWhilePanelOpen('fullscreen')).toBe(true)
    expect(isActionAllowedWhilePanelOpen('search')).toBe(true)
  })

  it('blocks reading actions so they do not fire behind an open panel', () => {
    // 目录开着时按方向键不该偷偷翻页。
    for (const action of ['prevPage', 'nextPage', 'prevChapter', 'nextChapter',
      'chapterStart', 'chapterEnd', 'speech', 'autoScroll', 'nightMode', 'none'] as const) {
      expect(isActionAllowedWhilePanelOpen(action)).toBe(false)
    }
  })
})

describe('resolveHotkeyAction', () => {
  it('serves both reading modes with one page action', () => {
    // 翻页不再分半页: 同一个动作在两种模式下各自分流(滚动模式即整页滚动)。
    expect(resolveHotkeyAction(DEFAULT_HOTKEY_BINDINGS, 'ArrowDown', 'paged')).toBe('nextPage')
    expect(resolveHotkeyAction(DEFAULT_HOTKEY_BINDINGS, 'ArrowDown', 'scroll')).toBe('nextPage')
    expect(resolveHotkeyAction(DEFAULT_HOTKEY_BINDINGS, 'ArrowUp', 'scroll')).toBe('prevPage')
    expect(resolveHotkeyAction(DEFAULT_HOTKEY_BINDINGS, 'Space', 'scroll')).toBe('nextPage')
  })

  it('prefers the first applicable action in the selected order', () => {
    const bindings = normalizeHotkeyBindings({
      ArrowDown: ['chapterEnd', 'nextPage'],
      ArrowUp: [],
    })

    expect(resolveHotkeyAction(bindings, 'ArrowDown', 'scroll')).toBe('chapterEnd')
    expect(resolveHotkeyAction(bindings, 'ArrowDown', 'paged')).toBe('chapterEnd')
  })

  it('skips actions that do not apply to the current mode', () => {
    const bindings = normalizeHotkeyBindings({ ArrowDown: ['nextPage'] })

    expect(resolveHotkeyAction(bindings, 'ArrowDown', 'paged')).toBe('nextPage')
    expect(resolveHotkeyAction(bindings, 'ArrowDown', 'scroll')).toBe('nextPage')
  })

  it('lets a mode-agnostic action serve both modes', () => {
    expect(resolveHotkeyAction(DEFAULT_HOTKEY_BINDINGS, 'ArrowRight', 'paged')).toBe('nextChapter')
    expect(resolveHotkeyAction(DEFAULT_HOTKEY_BINDINGS, 'ArrowRight', 'scroll')).toBe('nextChapter')
    expect(resolveHotkeyAction(DEFAULT_HOTKEY_BINDINGS, 'Escape', 'scroll')).toBe('back')
  })

  it('resolves a user-recorded chord', () => {
    const bindings = normalizeHotkeyBindings({ 'Control+Shift+k': ['catalog'] })

    expect(resolveHotkeyAction(bindings, 'Control+Shift+k', 'paged')).toBe('catalog')
  })

  it('reports no action for an unbound key, so it is never hijacked', () => {
    const bindings = normalizeHotkeyBindings({ ArrowDown: [] })

    expect(resolveHotkeyAction(bindings, 'ArrowDown', 'scroll')).toBeNull()
    expect(resolveHotkeyAction(bindings, 'UnknownKey', 'scroll')).toBeNull()
    expect(resolveHotkeyAction(bindings, 'q', 'scroll')).toBeNull()
  })

  it('treats the explicit 不操作 choice as a bound no-op instead of a missing binding', () => {
    const bindings = normalizeHotkeyBindings({ ArrowDown: ['none'] })

    expect(resolveHotkeyAction(bindings, 'ArrowDown', 'scroll')).toBe('none')
    expect(resolveHotkeyAction(bindings, 'ArrowDown', 'paged')).toBe('none')
  })
})

describe('normalizeHotkeyBindings', () => {
  it('falls back to the defaults when storage is empty or malformed', () => {
    expect(normalizeHotkeyBindings(null).ArrowDown).toEqual(['nextPage'])
    expect(normalizeHotkeyBindings('nonsense').ArrowDown).toEqual(['nextPage'])
    expect(normalizeHotkeyBindings({}).Home).toEqual(['chapterStart'])
  })

  it('drops unknown actions and de-duplicates while keeping order', () => {
    const bindings = normalizeHotkeyBindings({
      ArrowDown: ['chapterEnd', 'notAnAction', 'chapterEnd', 'nextChapter'],
    })

    expect(bindings.ArrowDown).toEqual(['chapterEnd', 'nextChapter'])
  })

  it('keeps an explicitly emptied key empty instead of restoring the default', () => {
    expect(normalizeHotkeyBindings({ ArrowDown: [] }).ArrowDown).toEqual([])
  })

  it('keeps every valid key, including user-recorded chords and free keys', () => {
    const bindings = normalizeHotkeyBindings({
      ArrowDown: ['nextPage'],
      'Control+Shift+k': ['catalog'],
      F5: ['nightMode'],
      Enter: ['autoScroll'],
    })

    expect(bindings['Control+Shift+k']).toEqual(['catalog'])
    expect(bindings.F5).toEqual(['nightMode'])
    expect(bindings.Enter).toEqual(['autoScroll'])
  })

  it('drops malformed key identifiers from storage', () => {
    const tooLong = 'a'.repeat(40)
    const bindings = normalizeHotkeyBindings({
      ArrowDown: ['nextPage'],
      'Control+': ['catalog'],
      Control: ['catalog'],
      [tooLong]: ['catalog'],
      'Control+Control+a': ['catalog'],
    })

    expect(Object.keys(bindings)).not.toContain('Control+')
    expect(Object.keys(bindings)).not.toContain('Control')
    expect(Object.keys(bindings)).not.toContain(tooLong)
    expect(Object.keys(bindings)).not.toContain('Control+Control+a')
  })

  it('still backfills every built-in default key', () => {
    const bindings = normalizeHotkeyBindings({ ArrowDown: ['nextPage'] })

    Object.keys(DEFAULT_HOTKEY_BINDINGS).forEach((keyId) => {
      expect(bindings[keyId]).toEqual(DEFAULT_HOTKEY_BINDINGS[keyId])
    })
  })
})

describe('isDefaultHotkeyBindings', () => {
  it('recognises the pristine default table', () => {
    expect(isDefaultHotkeyBindings(normalizeHotkeyBindings(null))).toBe(true)
  })

  it('detects any user change', () => {
    expect(isDefaultHotkeyBindings(normalizeHotkeyBindings({ Home: ['nextChapter'] }))).toBe(false)
    expect(isDefaultHotkeyBindings(normalizeHotkeyBindings({ Home: [] }))).toBe(false)
  })

  it('treats an added custom key as a deviation', () => {
    expect(isDefaultHotkeyBindings(normalizeHotkeyBindings({
      'Control+Shift+k': ['catalog'],
    }))).toBe(false)
  })

  it('ignores ordering differences only when they are truly equal', () => {
    expect(isDefaultHotkeyBindings(normalizeHotkeyBindings({
      ArrowDown: ['chapterEnd', 'nextPage'],
    }))).toBe(false)
  })
})

describe('bindActionToKey / unbindActionFromKey', () => {
  it('binds a key to a previously unbound action', () => {
    const bindings = bindActionToKey(normalizeHotkeyBindings(null), 'ArrowUp', 'catalog')

    expect(bindings.ArrowUp).toEqual(['prevPage', 'catalog'])
  })

  it('appends instead of prepending, so existing behaviour is preserved', () => {
    // 插到最前会把上键默认的翻页动作顶掉 —— 那是更常用的动作, 不能悄悄改变。
    const bindings = bindActionToKey(normalizeHotkeyBindings(null), 'ArrowUp', 'catalog')

    expect(resolveHotkeyAction(bindings, 'ArrowUp', 'paged')).toBe('prevPage')
    expect(resolveHotkeyAction(bindings, 'ArrowUp', 'scroll')).toBe('prevPage')
  })

  it('binds a user-recorded chord', () => {
    const bindings = bindActionToKey(normalizeHotkeyBindings(null), 'Control+Shift+k', 'catalog')

    expect(bindings['Control+Shift+k']).toEqual(['catalog'])
    expect(resolveHotkeyAction(bindings, 'Control+Shift+k', 'scroll')).toBe('catalog')
  })

  it('lets one action be driven by several keys', () => {
    let bindings = normalizeHotkeyBindings(null)
    bindings = bindActionToKey(bindings, 'ArrowUp', 'catalog')
    bindings = bindActionToKey(bindings, 'ArrowDown', 'catalog')

    expect(bindings.ArrowUp).toContain('catalog')
    expect(bindings.ArrowDown).toContain('catalog')
  })

  it('does not duplicate an action already bound to that key', () => {
    let bindings = bindActionToKey(normalizeHotkeyBindings(null), 'Home', 'nextChapter')
    bindings = bindActionToKey(bindings, 'Home', 'nextChapter')

    expect(bindings.Home.filter((id) => id === 'nextChapter')).toHaveLength(1)
  })

  it('clears an explicit 不操作 when a real action is bound', () => {
    let bindings = normalizeHotkeyBindings({ Home: ['none'] })
    expect(resolveHotkeyAction(bindings, 'Home', 'paged')).toBe('none')

    bindings = bindActionToKey(bindings, 'Home', 'nextChapter')

    expect(bindings.Home).not.toContain('none')
    expect(resolveHotkeyAction(bindings, 'Home', 'paged')).toBe('nextChapter')
  })

  it('removes exactly one action on unbind', () => {
    const bindings = unbindActionFromKey(normalizeHotkeyBindings(null), 'ArrowDown', 'nextPage')

    expect(bindings.ArrowDown).toEqual([])
    expect(bindings.ArrowUp).toEqual(['prevPage'])
  })

  it('leaves the key unbound when its last action is removed', () => {
    const bindings = unbindActionFromKey(normalizeHotkeyBindings({ Home: ['chapterStart'] }), 'Home', 'chapterStart')

    expect(bindings.Home).toEqual([])
    expect(resolveHotkeyAction(bindings, 'Home', 'paged')).toBeNull()
  })

  it('drops a malformed key instead of resurrecting it', () => {
    const bindings = bindActionToKey(normalizeHotkeyBindings(null), 'Control+', 'catalog')

    expect(Object.keys(bindings)).not.toContain('Control+')
  })
})

describe('conflict detection', () => {
  it('reports no conflict for the pristine defaults', () => {
    // 每个键只绑一个动作, 且翻页跨模式通用, 不应误报。
    expect(listHotkeyConflicts(normalizeHotkeyBindings(null))).toEqual([])
  })

  it('sees a default key as having a single applicable action in both modes', () => {
    const bindings = normalizeHotkeyBindings(null)

    expect(applicableActionsFor(bindings, 'ArrowDown', 'paged')).toEqual(['nextPage'])
    expect(applicableActionsFor(bindings, 'ArrowDown', 'scroll')).toEqual(['nextPage'])
    expect(hasHotkeyConflict(bindings, 'ArrowDown', 'paged')).toBe(false)
    expect(hasHotkeyConflict(bindings, 'ArrowDown', 'scroll')).toBe(false)
  })

  it('flags a key whose same-mode actions compete', () => {
    // 目录与上一页都适用于两种模式: 只有排在前面的会执行。
    const bindings = bindActionToKey(normalizeHotkeyBindings(null), 'ArrowUp', 'catalog')

    expect(hasHotkeyConflict(bindings, 'ArrowUp', 'paged')).toBe(true)
    expect(applicableActionsFor(bindings, 'ArrowUp', 'paged')).toEqual(['prevPage', 'catalog'])
    expect(resolveHotkeyAction(bindings, 'ArrowUp', 'paged')).toBe('prevPage')
  })

  it('lists conflicts with the mode and the losing actions', () => {
    const bindings = bindActionToKey(normalizeHotkeyBindings(null), 'ArrowUp', 'catalog')
    const conflicts = listHotkeyConflicts(bindings)

    const paged = conflicts.find((c) => c.keyId === 'ArrowUp' && c.mode === 'paged')
    expect(paged?.actions).toEqual(['prevPage', 'catalog'])
  })

  it('flags a conflict for two mode-agnostic actions on one key', () => {
    // 翻页动作现在是 both, 因此给上键再加「上一章」会真的冲突。
    const bindings = bindActionToKey(normalizeHotkeyBindings(null), 'ArrowUp', 'prevChapter')

    expect(hasHotkeyConflict(bindings, 'ArrowUp', 'paged')).toBe(true)
    expect(hasHotkeyConflict(bindings, 'ArrowUp', 'scroll')).toBe(true)
  })

  it('treats 不操作 as clashing with any real action in both modes', () => {
    const bindings = normalizeHotkeyBindings({ ArrowDown: ['none', 'nextPage'] })

    expect(hasHotkeyConflict(bindings, 'ArrowDown', 'paged')).toBe(true)
    expect(resolveHotkeyAction(bindings, 'ArrowDown', 'paged')).toBe('none')
  })
})

describe('hotkey table integrity', () => {
  it('declares a unique id for every action', () => {
    const ids = HOTKEY_ACTIONS.map((action) => action.id)
    expect(new Set(ids).size).toBe(ids.length)
  })

  it('no longer exposes the removed half-page actions', () => {
    // 滚动模式改为整页后, 半页动作应当彻底消失, 避免留下死配置。
    expect(HOTKEY_ACTION_MAP.halfPageUp).toBeUndefined()
    expect(HOTKEY_ACTION_MAP.halfPageDown).toBeUndefined()
  })

  it('only references declared actions from the defaults', () => {
    Object.values(DEFAULT_HOTKEY_BINDINGS).forEach((actions) => {
      actions.forEach((id) => expect(HOTKEY_ACTION_MAP[id]).toBeDefined())
    })
  })

  it('only binds declared preset keys from the defaults', () => {
    const keyIds = new Set(HOTKEY_KEYS.map((key) => key.id))
    Object.keys(DEFAULT_HOTKEY_BINDINGS).forEach((id) => expect(keyIds.has(id)).toBe(true))
  })

  it('covers both reading modes from the default table alone', () => {
    // 每一个**有默认绑定**的键, 在两种模式下都必须有动作, 不留「按了没反应」的死键。
    // 预设表里另有一些键(回车/Tab/退格/Del)默认不绑定, 供用户自行录入, 不参与此检查。
    const modes = ['paged', 'scroll'] as const
    const deadKeys: string[] = []
    Object.keys(DEFAULT_HOTKEY_BINDINGS).forEach((keyId) => {
      modes.forEach((mode) => {
        if (!resolveHotkeyAction(DEFAULT_HOTKEY_BINDINGS, keyId, mode)) {
          deadKeys.push(`${hotkeyLabel(keyId)}@${mode}`)
        }
      })
    })

    expect(deadKeys).toEqual([])
  })

  it('offers unbound presets so the user has somewhere to start', () => {
    const unbound = HOTKEY_KEYS.filter((key) => !(key.id in DEFAULT_HOTKEY_BINDINGS))

    expect(unbound.length).toBeGreaterThan(0)
    expect(unbound.map((key) => key.id)).toContain('Enter')
  })

  it('drives the page turn from one action in both modes', () => {
    expect(resolveHotkeyAction(DEFAULT_HOTKEY_BINDINGS, 'Space', 'scroll')).toBe('nextPage')
    expect(resolveHotkeyAction(DEFAULT_HOTKEY_BINDINGS, 'Space', 'paged')).toBe('nextPage')
    expect(resolveHotkeyAction(DEFAULT_HOTKEY_BINDINGS, 'PageUp', 'scroll')).toBe('prevPage')
    expect(resolveHotkeyAction(DEFAULT_HOTKEY_BINDINGS, 'PageDown', 'scroll')).toBe('nextPage')
    expect(resolveHotkeyAction(DEFAULT_HOTKEY_BINDINGS, 'Home', 'paged')).toBe('chapterStart')
    expect(resolveHotkeyAction(DEFAULT_HOTKEY_BINDINGS, 'Control+f', 'paged')).toBe('search')
  })

  it('has no conflict anywhere in the shipped defaults', () => {
    expect(listHotkeyConflicts(normalizeHotkeyBindings(null))).toEqual([])
  })
})
