import { describe, expect, it } from 'vitest'
import { resolveHotkeyAction, type HotkeyBindings } from '../utils/readerHotkeys'

/**
 * 全局 F11 兜底（App.vue）复用这份矩阵判断"F11 现在绑定的是什么"。
 * 用户把 F11 改成「不操作」后，书架页按 F11 不能再擅自切全屏 ——
 * 那是设置被无视，而不是 bug 修复。
 */
describe('global F11 fallback respects hotkey bindings', () => {
  it('默认把 F11 解析为全屏', () => {
    const bindings: HotkeyBindings = { F11: ['fullscreen'] }
    expect(resolveHotkeyAction(bindings, 'F11', 'scroll')).toBe('fullscreen')
  })

  it('用户改成「不操作」后不再触发全屏', () => {
    const bindings: HotkeyBindings = { F11: ['none'] }
    expect(resolveHotkeyAction(bindings, 'F11', 'scroll')).not.toBe('fullscreen')
  })

  it('用户改成别的动作后不触发全屏', () => {
    const bindings: HotkeyBindings = { F11: ['nightMode'] }
    expect(resolveHotkeyAction(bindings, 'F11', 'scroll')).not.toBe('fullscreen')
  })

  it('未绑定的按键不会解析出全屏', () => {
    expect(resolveHotkeyAction({}, 'F11', 'scroll')).not.toBe('fullscreen')
  })
})
