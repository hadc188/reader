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
  hotkeyIdFromMouseEvent,
  hotkeyIdFromWheelEvent,
  hotkeyLabel,
  isActionAllowedWhilePanelOpen,
  isDefaultHotkeyBindings,
  isHotkeyCaptureActive,
  isMouseButtonHotkeyBlocked,
  isPointerHotkeyBlocked,
  isValidHotkeyId,
  listHotkeyConflicts,
  mouseButtonHotkeyAction,
  normalizeHotkeyBindings,
  resolveHotkeyAction,
  setHotkeyCaptureActive,
  shouldPreventSystemMenuKey,
  unbindActionFromKey,
  type HotkeyBindings,
} from './readerHotkeys'

/* ─── 事件目标的阻挡规则 ───
 *
 *  回归测试: 中键/侧键与滚轮**不能用同一套规则**。它们曾共用一套(含 button),
 *  于是设置面板里录入侧键时, 鼠标必然停在「录入按键」那个 <button> 上 —— 事件被
 *  直接放弃、不阻止默认行为, 侧键触发浏览器前进/后退; 本应用用 hash 路由, hash
 *  一变就跳路由、面板被卸载, 侧键根本录不进去。 */
describe('isMouseButtonHotkeyBlocked / isPointerHotkeyBlocked', () => {
  /** 造一个只有 closest/isContentEditable 的假目标。matches 记录被查询的选择器。 */
  function fakeTarget(matches: string[], { contentEditable = false } = {}) {
    const queries: string[] = []
    return {
      queries,
      target: {
        isContentEditable: contentEditable,
        closest(selector: string) {
          queries.push(selector)
          // 粗略模拟: 选择器里任一 token 命中即算匹配。
          const tokens = selector.split(',').map((s) => s.trim())
          return tokens.some((token) => matches.includes(token)) ? {} : null
        },
      },
    }
  }

  it('blocks middle/side buttons only over text inputs and editors', () => {
    for (const token of ['input', 'textarea', 'select']) {
      const { target } = fakeTarget([token])
      expect(isMouseButtonHotkeyBlocked(target)).toBe(true)
    }
    const { target: editor } = fakeTarget([], { contentEditable: true })
    expect(isMouseButtonHotkeyBlocked(editor)).toBe(true)
  })

  it('does NOT block middle/side buttons over a button element', () => {
    // 这正是「录入按键」那个按钮所处的情形 —— 必须放行, 否则录不进侧键。
    const { target } = fakeTarget(['button'])
    expect(isMouseButtonHotkeyBlocked(target)).toBe(false)
  })

  it('does NOT block middle/side buttons over the reader context menu', () => {
    const { target } = fakeTarget(['.reader-context-menu'])
    expect(isMouseButtonHotkeyBlocked(target)).toBe(false)
  })

  it('blocks wheel over buttons and the context menu, unlike mouse buttons', () => {
    // 滚轮必须保留 button: 面板是长列表, 悬停在按钮上时滚动要归面板自己。
    for (const token of ['input', 'textarea', 'select', 'button', '.reader-context-menu']) {
      const { target } = fakeTarget([token])
      expect(isPointerHotkeyBlocked(target)).toBe(true)
    }
  })

  it('never queries button in the mouse-button rule', () => {
    // 锁住实现细节: 一旦有人把 button 加回这条规则, 侧键录入会立刻再次失败。
    const { target, queries } = fakeTarget(['button'])
    isMouseButtonHotkeyBlocked(target)
    expect(queries.join(' ')).not.toContain('button')
    expect(queries.join(' ')).toContain('input')
  })

  it('treats a plain element as unblocked for both rules', () => {
    const { target } = fakeTarget([])
    expect(isMouseButtonHotkeyBlocked(target)).toBe(false)
    expect(isPointerHotkeyBlocked(target)).toBe(false)
  })

  it('treats a missing target as unblocked', () => {
    expect(isMouseButtonHotkeyBlocked(null)).toBe(false)
    expect(isMouseButtonHotkeyBlocked(undefined)).toBe(false)
    expect(isPointerHotkeyBlocked(null)).toBe(false)
    // 没有 closest 的对象(如 window/document 的某些目标)不该抛错。
    expect(isMouseButtonHotkeyBlocked({} as never)).toBe(false)
    expect(isPointerHotkeyBlocked({} as never)).toBe(false)
  })
})

/* ─── 鼠标键该不该被接管 ───
 *
 *  ReaderView 的 handleReaderMouseDown 用「已绑定」作为接管的唯一依据:
 *  绑定了就拦掉默认行为(否则侧键会把整个页面导航走), 没绑定就完全放行
 *  (用户习惯的侧键前进/后退必须保留)。
 *
 *  这里锁住那个判定 —— 它一旦写反, 要么绑定的键失效, 要么未绑定的键被吞掉。
 *
 *  **直接调用实现导出的 mouseButtonHotkeyAction**, 不复刻它的逻辑:
 *  曾经这份测试在文件里重写了一遍同样的判定, 结果改动真实实现时测试照样通过 ——
 *  那种"复刻式测试"只是把实现抄了两遍, 保护不了任何东西。 */
describe('鼠标键的接管判定', () => {
  /** 直接调实现; captureActive 通过 setHotkeyCaptureActive 走真实通路。 */
  function shouldTakeOver(options: {
    captureActive: boolean
    button: number
    bindings: HotkeyBindings
    mode: 'paged' | 'scroll'
  }): boolean {
    setHotkeyCaptureActive(options.captureActive)
    try {
      return mouseButtonHotkeyAction(options.bindings, { button: options.button }, options.mode) !== null
    } finally {
      setHotkeyCaptureActive(false)
    }
  }

  it('takes over a bound side button', () => {
    expect(shouldTakeOver({
      captureActive: false,
      button: 3,
      bindings: { Mouse4: ['nextPage'] },
      mode: 'scroll',
    })).toBe(true)
  })

  it('leaves an unbound side button alone so it can still navigate', () => {
    // 这是刻意的设计: 没绑定的侧键要保留浏览器原本的前进/后退。
    expect(shouldTakeOver({
      captureActive: false,
      button: 3,
      bindings: { ArrowDown: ['nextPage'] },
      mode: 'scroll',
    })).toBe(false)
  })

  it('leaves the left and right buttons alone even if bound', () => {
    // 左右键不参与快捷键(它们是点击翻页与右键菜单的载体), 因此查不到标识。
    for (const button of [0, 2]) {
      expect(shouldTakeOver({
        captureActive: false,
        button,
        bindings: { Mouse4: ['nextPage'], Mouse5: ['prevPage'] },
        mode: 'scroll',
      })).toBe(false)
    }
  })

  it('does not take over while recording a shortcut', () => {
    // 录入态: 这次按键属于「设置快捷键」, 不能被运行时接走。
    expect(shouldTakeOver({
      captureActive: true,
      button: 3,
      bindings: { Mouse4: ['nextPage'] },
      mode: 'scroll',
    })).toBe(false)
  })

  it('does not take over when the user set the key to 不操作', () => {
    expect(shouldTakeOver({
      captureActive: false,
      button: 3,
      bindings: { Mouse4: ['none'] },
      mode: 'scroll',
    })).toBe(false)
  })

  it('绑定带修饰键的鼠标键后, 无修饰键的同一键不接管', () => {
    // Ctrl+侧键1 被绑到某动作, 不等于裸侧键1 被绑 —— 后者必须放行,
    // 否则用户会发现「绑了 Ctrl+侧键之后, 单按侧键也不导航了」。
    expect(shouldTakeOver({
      captureActive: false,
      button: 3,
      bindings: { 'Control+Mouse4': ['nextPage'] },
      mode: 'scroll',
    })).toBe(false)
    // 按着 Ctrl 时才是被绑定的那个键 → 接管。
    setHotkeyCaptureActive(false)
    expect(mouseButtonHotkeyAction({ 'Control+Mouse4': ['nextPage'] }, { button: 3, ctrlKey: true }, 'scroll'))
      .toBe('nextPage')
  })

  it('绑定只对同一个物理键生效, 不牵连另一个侧键', () => {
    // button 3 = 侧键1(Mouse4), button 4 = 侧键2(Mouse5)。
    const bindings: HotkeyBindings = { Mouse4: ['nextPage'] }
    expect(shouldTakeOver({ captureActive: false, button: 3, bindings, mode: 'scroll' })).toBe(true)
    // 侧键2 没绑定 → 必须放行, 保留浏览器原本的前进/后退。
    expect(shouldTakeOver({ captureActive: false, button: 4, bindings, mode: 'scroll' })).toBe(false)
  })
})

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

describe('system menu key guard', () => {
  it('prevents the bare Alt and F10 system menus in the reader', () => {
    expect(shouldPreventSystemMenuKey({ key: 'Alt' })).toBe(true)
    expect(shouldPreventSystemMenuKey({ key: 'F10' })).toBe(true)
  })

  it('leaves key combinations available for boss key and hotkeys', () => {
    expect(shouldPreventSystemMenuKey({ key: 'e', altKey: true })).toBe(false)
    expect(shouldPreventSystemMenuKey({ key: 'Alt', altKey: true, ctrlKey: true })).toBe(false)
    expect(shouldPreventSystemMenuKey({ key: 'F10', shiftKey: true })).toBe(false)
    expect(shouldPreventSystemMenuKey({ key: 'F10', ctrlKey: true })).toBe(false)
  })
})

describe('hotkeyIdFromMouseEvent', () => {
  it('maps the side buttons and the middle button', () => {
    expect(hotkeyIdFromMouseEvent({ button: 1 })).toBe('MouseMiddle')
    expect(hotkeyIdFromMouseEvent({ button: 3 })).toBe('Mouse4')
    expect(hotkeyIdFromMouseEvent({ button: 4 })).toBe('Mouse5')
  })

  it('never treats the primary or secondary button as a hotkey', () => {
    // 左键=点击翻页/呼出控件, 右键=自定义菜单, 绑成快捷键会把日常操作劫持掉。
    expect(hotkeyIdFromMouseEvent({ button: 0 })).toBeNull()
    expect(hotkeyIdFromMouseEvent({ button: 2 })).toBeNull()
    expect(hotkeyIdFromMouseEvent({ button: 9 })).toBeNull()
  })

  it('keeps modifiers so a side button can form a chord', () => {
    expect(hotkeyIdFromMouseEvent({ button: 3, ctrlKey: true })).toBe('Control+Mouse4')
    expect(hotkeyIdFromMouseEvent({ button: 3, ctrlKey: true, shiftKey: true }))
      .toBe('Control+Shift+Mouse4')
  })

  it('produces ids that survive the storage validator', () => {
    for (const button of [1, 3, 4]) {
      const id = hotkeyIdFromMouseEvent({ button })
      expect(id).not.toBeNull()
      expect(isValidHotkeyId(id)).toBe(true)
    }
  })
})

describe('hotkeyIdFromWheelEvent', () => {
  it('maps the scroll direction', () => {
    expect(hotkeyIdFromWheelEvent({ deltaX: 0, deltaY: 120 })).toBe('WheelDown')
    expect(hotkeyIdFromWheelEvent({ deltaX: 0, deltaY: -120 })).toBe('WheelUp')
  })

  it('uses the dominant axis for trackpad diagonal gestures', () => {
    expect(hotkeyIdFromWheelEvent({ deltaX: -40, deltaY: 5 })).toBe('WheelUp')
    expect(hotkeyIdFromWheelEvent({ deltaX: 3, deltaY: 60 })).toBe('WheelDown')
  })

  it('yields Ctrl+wheel to the font-size zoom', () => {
    // Ctrl+滚轮已被阅读页的「字号缩放」占用, 不能同时触发绑定的滚轮动作。
    expect(hotkeyIdFromWheelEvent({ deltaX: 0, deltaY: 120, ctrlKey: true })).toBeNull()
  })

  it('ignores a zero or non-finite delta', () => {
    expect(hotkeyIdFromWheelEvent({ deltaX: 0, deltaY: 0 })).toBeNull()
    expect(hotkeyIdFromWheelEvent({ deltaX: 0, deltaY: Number.NaN })).toBeNull()
  })

  it('produces ids that survive the storage validator', () => {
    for (const id of ['WheelUp', 'WheelDown']) {
      expect(isValidHotkeyId(id)).toBe(true)
    }
  })
})

describe('鼠标键的默认行为接管判定', () => {
  // ReaderView 靠「解析出可执行动作」决定是否拦掉鼠标键的默认行为
  // (中键自动滚动 / 侧键前进后退)。这组用例锁住那个前提:
  // 未绑定、或用户显式设成「不操作」时都必须放行, 否则会把用户习惯的
  // 侧键导航与中键滚动一并杀掉。
  //
  // 这里直接调用**实现导出的** mouseButtonHotkeyAction, 不再复刻一份同样的
  // 判定 —— 复刻版改实现不会失败, 那种测试是自欺。
  const actionFor = (button: number, bindings: HotkeyBindings) =>
    mouseButtonHotkeyAction(bindings, { button }, 'scroll')

  it('leaves unbound mouse buttons to the browser', () => {
    // 默认绑定里没有任何鼠标键。
    expect(actionFor(1, DEFAULT_HOTKEY_BINDINGS)).toBeNull()
    expect(actionFor(3, DEFAULT_HOTKEY_BINDINGS)).toBeNull()
    expect(actionFor(4, DEFAULT_HOTKEY_BINDINGS)).toBeNull()
  })

  it('takes over once the user binds an action', () => {
    const bindings = normalizeHotkeyBindings({ Mouse4: ['prevPage'] })
    expect(actionFor(3, bindings)).toBe('prevPage')
  })

  it('keeps handing the button back when bound to「不操作」', () => {
    const bindings = normalizeHotkeyBindings({ Mouse5: ['none'] })
    expect(actionFor(4, bindings)).toBeNull()
  })

  it('never claims the primary or secondary button', () => {
    const bindings = normalizeHotkeyBindings({ Mouse4: ['prevPage'], Mouse5: ['nextPage'] })
    expect(actionFor(0, bindings)).toBeNull()
    expect(actionFor(2, bindings)).toBeNull()
  })
})

describe('hotkeyLabel', () => {
  it('renders friendly names for the well-known keys', () => {
    expect(hotkeyLabel('ArrowDown')).toBe('下键')
    expect(hotkeyLabel('Space')).toBe('空格')
    expect(hotkeyLabel('PageUp')).toBe('PgUp')
    expect(hotkeyLabel('Escape')).toBe('Esc')
  })

  it('renders mouse buttons and wheel directions readably', () => {
    expect(hotkeyLabel('MouseMiddle')).toBe('鼠标中键')
    expect(hotkeyLabel('Mouse4')).toBe('鼠标侧键1')
    expect(hotkeyLabel('Mouse5')).toBe('鼠标侧键2')
    expect(hotkeyLabel('WheelUp')).toBe('滚轮上')
    expect(hotkeyLabel('WheelDown')).toBe('滚轮下')
    expect(hotkeyLabel('Control+Mouse4')).toBe('Ctrl+鼠标侧键1')
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

describe('录入态 (hotkey capture)', () => {
  // 回归测试: 面板开关类动作在弹层打开时是放行的, 因此「把侧键改绑到目录」时
  // 按侧键会先被阅读页接走、顺手切走面板。录入期间必须整体让路。
  it('is inactive by default so runtime dispatch is unaffected', () => {
    expect(isHotkeyCaptureActive()).toBe(false)
  })

  it('flags capture on and back off', () => {
    setHotkeyCaptureActive(true)
    expect(isHotkeyCaptureActive()).toBe(true)
    setHotkeyCaptureActive(false)
    expect(isHotkeyCaptureActive()).toBe(false)
  })

  it('still reports panel-open allowance independently of capture state', () => {
    // 两者是不同的概念: 录入态只是让运行时整体静音, 不改变允许列表本身。
    setHotkeyCaptureActive(true)
    expect(isActionAllowedWhilePanelOpen('catalog')).toBe(true)
    setHotkeyCaptureActive(false)
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
