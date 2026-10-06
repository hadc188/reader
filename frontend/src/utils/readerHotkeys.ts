/** 阅读页自定义快捷键。
 *
 *  模型: 每个「按键」可绑定**多个**动作(与设置界面里的矩阵一致), 实际触发时
 *  只执行**当前阅读模式下第一个适用**的动作 —— 因此同一个键可以在「左右分页」
 *  和「上下滚动」下表现不同(例如下键: 分页模式翻页, 滚动模式下半页)。
 *
 *  纯数据与纯函数, 不碰 DOM, 便于单测。 */

/** 阅读模式大类: 分页(左右翻页) 与 滚动(上下滑动/连续阅读/隐藏已读)。 */
export type ReadingModeKind = 'paged' | 'scroll'

export type HotkeyActionId =
  | 'prevPage'
  | 'nextPage'
  | 'prevChapter'
  | 'nextChapter'
  | 'chapterStart'
  | 'chapterEnd'
  | 'catalog'
  | 'settings'
  | 'search'
  | 'speech'
  | 'autoScroll'
  | 'fullscreen'
  | 'nightMode'
  | 'back'
  | 'none'

export interface HotkeyAction {
  id: HotkeyActionId
  label: string
  /** 适用模式; both 表示两种模式都适用。 */
  scope: 'both' | ReadingModeKind
  hint?: string
}

/** 动作表(即设置界面里的一行)。
 *
 *  翻页只有「上一页 / 下一页」两个动作, 不分半页: 滚动模式下的整页滚动就是
 *  翻一页, 两个动作各自按当前模式分流(见 ReaderView 的 pageForward)。之前把
 *  半页拆成独立动作, 会让同一按键在同模式下挂上两个互斥动作, 徒增冲突。 */
export const HOTKEY_ACTIONS: HotkeyAction[] = [
  { id: 'prevPage', label: '上一页', scope: 'both', hint: '分页模式向左翻页；滚动模式上移整页' },
  { id: 'nextPage', label: '下一页', scope: 'both', hint: '分页模式向右翻页；滚动模式下移整页' },
  { id: 'prevChapter', label: '上一章', scope: 'both' },
  { id: 'nextChapter', label: '下一章', scope: 'both' },
  { id: 'chapterStart', label: '首页', scope: 'both', hint: '跳到本章开头' },
  { id: 'chapterEnd', label: '尾页', scope: 'both', hint: '跳到本章末尾' },
  { id: 'catalog', label: '目录', scope: 'both' },
  { id: 'settings', label: '设置', scope: 'both' },
  { id: 'search', label: '搜索', scope: 'both', hint: '打开页内搜索' },
  { id: 'speech', label: '朗读', scope: 'both', hint: '开始/暂停朗读' },
  { id: 'autoScroll', label: '自动滚动', scope: 'both', hint: '开关自动阅读' },
  { id: 'fullscreen', label: '全屏', scope: 'both' },
  { id: 'nightMode', label: '夜间', scope: 'both' },
  { id: 'back', label: '返回', scope: 'both', hint: '关闭弹层；无弹层时回到书架' },
  { id: 'none', label: '不操作', scope: 'both' },
]

export interface HotkeyKeyDefinition {
  /** 与 hotkeyIdFromEvent 返回的标识一致。 */
  id: string
  label: string
}

/** 常用键预设: 设置界面的下拉里先列这些, 方便快速选择。
 *  用户也可以直接**按下任意键**录入, 不限于此表。 */
export const HOTKEY_KEYS: HotkeyKeyDefinition[] = [
  { id: 'ArrowUp', label: '上键' },
  { id: 'ArrowDown', label: '下键' },
  { id: 'ArrowLeft', label: '左键' },
  { id: 'ArrowRight', label: '右键' },
  { id: 'Space', label: '空格' },
  { id: 'PageUp', label: 'PgUp' },
  { id: 'PageDown', label: 'PgDn' },
  { id: 'Home', label: 'Home' },
  { id: 'End', label: 'End' },
  { id: 'Enter', label: '回车' },
  { id: 'Tab', label: 'Tab' },
  { id: 'Backspace', label: '退格' },
  { id: 'Delete', label: 'Del' },
  { id: 'Escape', label: 'Esc' },
  { id: 'F11', label: 'F11' },
  { id: 'Control+f', label: 'Ctrl+F' },
]

export const HOTKEY_ACTION_MAP: Record<string, HotkeyAction> = Object.fromEntries(
  HOTKEY_ACTIONS.map((action) => [action.id, action]),
)

/* ─── 按键标识 ───
 *  标识格式: 修饰键(固定顺序 Control+Alt+Shift+Meta) + 主键, 用 + 连接。
 *  例: 'ArrowDown'、'Space'、'Control+f'、'Control+Shift+k'。
 *  顺序固定是为了让同一组合只有一种写法, 去重与比较才能直接比字符串。
 *  修饰键名用 'Control'(与 DOM 的 ctrlKey 及既有存储一致), 显示时才写成 Ctrl。 */

const MODIFIER_NAMES = ['Control', 'Alt', 'Shift', 'Meta'] as const

/** 修饰键本身不能作为主键绑定(只按 Ctrl 不该触发任何功能)。 */
const BARE_MODIFIER_KEYS = new Set(['Control', 'Alt', 'Shift', 'Meta', 'AltGraph', 'CapsLock', 'Dead'])

/** 主键的显示名。未列出的按原样显示(如 F5、k、1)。 */
const KEY_LABELS: Record<string, string> = {
  ArrowUp: '上键',
  ArrowDown: '下键',
  ArrowLeft: '左键',
  ArrowRight: '右键',
  Space: '空格',
  Escape: 'Esc',
  PageUp: 'PgUp',
  PageDown: 'PgDn',
  Enter: '回车',
  Backspace: '退格',
  Delete: 'Del',
  Insert: 'Ins',
  Home: 'Home',
  End: 'End',
  Tab: 'Tab',
  Control: 'Ctrl',
}

/** 主键标准化: 空格统一成 Space, 单字符统一小写(避免 Shift 造成大小写抖动)。 */
function normalizeBaseKey(key: string): string {
  if (key === ' ' || key === 'Spacebar') return 'Space'
  if (key.length === 1) return key.toLowerCase()
  return key
}

/** 校验一个标识是否合法(存储可能被手工编辑, 不能直接信任)。 */
export function isValidHotkeyId(id: unknown): id is string {
  if (typeof id !== 'string' || !id) return false
  const parts = id.split('+')
  if (parts.length > MODIFIER_NAMES.length + 1) return false
  const base = parts[parts.length - 1]
  if (!base || base.length > 20) return false
  if ((MODIFIER_NAMES as readonly string[]).includes(base)) return false
  const modifiers = parts.slice(0, -1)
  if (new Set(modifiers).size !== modifiers.length) return false
  return modifiers.every((name) => (MODIFIER_NAMES as readonly string[]).includes(name))
}

/** 修饰键在界面上的显示名。 */
const MODIFIER_LABELS: Record<string, string> = {
  Control: 'Ctrl',
  Meta: 'Win',
}

/** 标识 → 显示标签。'Control+Shift+k' → 'Ctrl+Shift+K'。 */
export function hotkeyLabel(keyId: string): string {
  const parts = keyId.split('+')
  const base = parts[parts.length - 1]
  const label = KEY_LABELS[base] || (base.length === 1 ? base.toUpperCase() : base)
  const modifiers = parts.slice(0, -1).map((name) => MODIFIER_LABELS[name] || name)
  return [...modifiers, label].join('+')
}
/** 按键 → 已绑定动作列表。顺序有意义: 靠前的优先被选中。 */
export type HotkeyBindings = Record<string, HotkeyActionId[]>

/** 默认绑定。翻页动作跨模式通用, 因此一个键只需绑一个动作。 */
export const DEFAULT_HOTKEY_BINDINGS: HotkeyBindings = {
  ArrowUp: ['prevPage'],
  ArrowDown: ['nextPage'],
  ArrowLeft: ['prevChapter'],
  ArrowRight: ['nextChapter'],
  Space: ['nextPage'],
  PageUp: ['prevPage'],
  PageDown: ['nextPage'],
  Home: ['chapterStart'],
  End: ['chapterEnd'],
  Escape: ['back'],
  F11: ['fullscreen'],
  'Control+f': ['search'],
}

/** 从键盘事件取出按键标识。
 *
 *  接受**任意键**, 让用户能按自己的习惯录入(见设置界面的「录入按键」)。
 *  只拒绝两类: 纯修饰键(只按 Ctrl 没有语义), 以及无法识别的主键。
 *  空白键位由 resolveHotkeyAction 兜底 —— 没绑定的键返回 null, 不会被拦截。 */
export function hotkeyIdFromEvent(event: {
  key: string
  ctrlKey?: boolean
  metaKey?: boolean
  altKey?: boolean
  shiftKey?: boolean
}): string | null {
  const raw = event.key
  if (!raw || BARE_MODIFIER_KEYS.has(raw)) return null
  const base = normalizeBaseKey(raw)
  if (!base) return null
  const modifiers: string[] = []
  if (event.ctrlKey) modifiers.push('Control')
  if (event.altKey) modifiers.push('Alt')
  if (event.shiftKey) modifiers.push('Shift')
  if (event.metaKey) modifiers.push('Meta')
  const id = [...modifiers, base].join('+')
  return isValidHotkeyId(id) ? id : null
}

/** 弹层(目录/设置/书架/换源等)打开时, 哪些动作依然要执行。
 *
 *  面板开关类必须是 toggle 语义 —— 用「目录」键打开目录后, 再按同一键就该关掉它,
 *  否则快捷键只能呼出不能关闭。若此时开着的是别的面板, `togglePanel` 会切到目录。
 *
 *  其余动作(翻页/章节跳转/朗读等)在弹层打开时一律拦掉, 避免对着目录误触翻页。 */
const ACTIONS_ALLOWED_WHILE_PANEL_OPEN: HotkeyActionId[] = [
  // 界面级动作, 与旧实现一致(F11 / Ctrl+F 原本就在弹层检查之前)
  'fullscreen',
  'search',
  // 面板开关: 让同名键可以关掉自己打开的面板
  'catalog',
  'settings',
]

/** 弹层打开时该动作是否仍应生效。 */
export function isActionAllowedWhilePanelOpen(action: HotkeyActionId): boolean {
  return ACTIONS_ALLOWED_WHILE_PANEL_OPEN.includes(action)
}

/** 取出该按键在当前模式下应执行的动作(第一个适用者); 无适用动作返回 null。 */
export function resolveHotkeyAction(
  bindings: HotkeyBindings,
  keyId: string,
  mode: ReadingModeKind,
): HotkeyActionId | null {
  const actions = bindings[keyId]
  if (!actions?.length) return null
  for (const id of actions) {
    const action = HOTKEY_ACTION_MAP[id]
    if (!action) continue
    if (action.scope === 'both' || action.scope === mode) return id
  }
  return null
}

/** 校验并归一化持久化的绑定。
 *
 *  按键不再限定于预设表 —— 用户可以录入任意键, 所以这里保留所有**合法标识**
 *  (见 isValidHotkeyId), 同时丢弃非法标识与未知动作。内置默认键若在存储里
 *  缺失, 回落到默认值; 用户自建的键只要存储里有就保留。
 *  存储可能来自旧版本或被手工编辑, 不能直接信任。 */
export function normalizeHotkeyBindings(raw: unknown): HotkeyBindings {
  const result: HotkeyBindings = {}
  const source = (raw && typeof raw === 'object' ? raw : {}) as Record<string, unknown>

  const collect = (list: unknown): HotkeyActionId[] => {
    if (!Array.isArray(list)) return []
    const seen = new Set<HotkeyActionId>()
    list.forEach((item) => {
      if (typeof item !== 'string') return
      if (!HOTKEY_ACTION_MAP[item]) return
      seen.add(item as HotkeyActionId)
    })
    return Array.from(seen)
  }

  // 先收用户存储里所有合法按键(含自定义键)。
  Object.keys(source).forEach((keyId) => {
    if (!isValidHotkeyId(keyId)) return
    result[keyId] = collect(source[keyId])
  })

  // 内置默认键: 存储里完全没有时才回落默认值, 显式清空([])仍保持为空。
  Object.keys(DEFAULT_HOTKEY_BINDINGS).forEach((keyId) => {
    if (!(keyId in source)) result[keyId] = [...DEFAULT_HOTKEY_BINDINGS[keyId]]
  })

  return result
}

/** 当前绑定是否与默认值一致(用于界面上的「默认 / 自定义」状态)。
 *  自定义键只要存在就算偏离默认。 */
export function isDefaultHotkeyBindings(bindings: HotkeyBindings): boolean {
  const keys = new Set([...Object.keys(DEFAULT_HOTKEY_BINDINGS), ...Object.keys(bindings)])
  for (const keyId of keys) {
    const current = bindings[keyId] || []
    const preset = DEFAULT_HOTKEY_BINDINGS[keyId] || []
    if (current.length !== preset.length) return false
    if (!current.every((id, index) => id === preset[index])) return false
  }
  return true
}

/** 某个按键在给定模式下会「适用」的所有动作。
 *  运行时只执行第一个(resolveHotkeyAction), 因此长度 > 1 即为冲突:
 *  后面的动作在这个模式下永远不会触发。设置界面用「功能→按键」表达,
 *  用户看不到隐藏顺序, 必须显式提示, 否则会以为绑上就生效。 */
export function applicableActionsFor(
  bindings: HotkeyBindings,
  keyId: string,
  mode: ReadingModeKind,
): HotkeyActionId[] {
  const actions = bindings[keyId]
  if (!actions?.length) return []
  return actions.filter((id) => {
    const action = HOTKEY_ACTION_MAP[id]
    return Boolean(action) && (action.scope === 'both' || action.scope === mode)
  })
}

/** 该按键在某个模式下是否存在冲突(有多个适用动作)。 */
export function hasHotkeyConflict(
  bindings: HotkeyBindings,
  keyId: string,
  mode: ReadingModeKind,
): boolean {
  return applicableActionsFor(bindings, keyId, mode).length > 1
}

/** 列出所有冲突。设置界面据此给对应的按键标签加警示标记。 */
export function listHotkeyConflicts(
  bindings: HotkeyBindings,
): Array<{ keyId: string; mode: ReadingModeKind; actions: HotkeyActionId[] }> {
  const modes: ReadingModeKind[] = ['paged', 'scroll']
  const conflicts: Array<{ keyId: string; mode: ReadingModeKind; actions: HotkeyActionId[] }> = []
  // 遍历实际存在的按键(含用户自定义键), 不只预设表。
  Object.keys(bindings).forEach((keyId) => {
    modes.forEach((mode) => {
      const actions = applicableActionsFor(bindings, keyId, mode)
      if (actions.length > 1) conflicts.push({ keyId, mode, actions })
    })
  })
  return conflicts
}

/** 把一个按键绑到某功能上, **追加到末尾**(保留已有功能的优先次序)。
 *
 *  之所以不插到最前: 同一按键可绑多个功能, 运行时只取第一个适用的。若插到最前,
 *  用户给「目录」绑上键就会把上键默认的翻页覆盖掉 —— 那是更常用的动作, 反直觉。
 *  追加则保证原有行为不变, 新功能在同模式已被占用时不触发; 这种冲突由界面
 *  显式提示(见 listHotkeyConflicts), 而不是悄悄改变已有行为。
 *  「不操作」(none) 与新功能互斥, 绑定时摘除。 */
export function bindActionToKey(
  bindings: HotkeyBindings,
  keyId: string,
  actionId: HotkeyActionId,
): HotkeyBindings {
  const current = bindings[keyId] || []
  const rest = current.filter((id) => id !== actionId && id !== 'none')
  return normalizeHotkeyBindings({ ...bindings, [keyId]: [...rest, actionId] })
}

/** 解绑: 从该按键的动作列表里去掉一个功能。 */
export function unbindActionFromKey(
  bindings: HotkeyBindings,
  keyId: string,
  actionId: HotkeyActionId,
): HotkeyBindings {
  const current = bindings[keyId] || []
  return normalizeHotkeyBindings({ ...bindings, [keyId]: current.filter((id) => id !== actionId) })
}
