import { reactive, readonly } from 'vue'

export interface MenuItem {
  /** 菜单项文字（divider 项可省略）。 */
  label?: string
  /** 点击后执行的动作。执行后菜单自动关闭。 */
  action?: () => void
  disabled?: boolean
  danger?: boolean
  divider?: boolean
  shortcut?: string
  /** 可选的图标（SVG 路径或 emoji 字符），显示在文字左侧。 */
  icon?: string
}

export interface ContextMenuState {
  visible: boolean
  top: number
  left: number
  width?: number
  /** 每项传入的上下文数据，供 action 使用。 */
  payload: unknown
  items: MenuItem[]
  /** 键盘导航当前高亮项索引（-1 表示无高亮）。 */
  activeIndex: number
}

const state = reactive<ContextMenuState>({
  visible: false,
  top: 0,
  left: 0,
  width: 220,
  payload: null,
  items: [],
  activeIndex: -1,
})

function clampToViewport(top: number, left: number, width: number, height: number) {
  const pad = 8
  const maxTop = window.innerHeight - height - pad
  const maxLeft = window.innerWidth - width - pad
  return {
    top: Math.min(Math.max(pad, top), Math.max(pad, maxTop)),
    left: Math.min(Math.max(pad, left), Math.max(pad, maxLeft)),
  }
}

/** 估算菜单高度，用于视口钳制。比固定 items.length*40 更接近真实尺寸。 */
function estimateMenuHeight(items: MenuItem[]) {
  const itemHeight = 36
  const dividerHeight = 11
  const padding = 12
  let h = 0
  for (const item of items) {
    h += item.divider ? dividerHeight : itemHeight
  }
  // 给快捷键换行留余量，宽菜单项更高
  const hasShortcuts = items.some((i) => !i.divider && i.shortcut)
  const extra = hasShortcuts ? items.filter((i) => !i.divider).length * 2 : 0
  return h + padding + extra
}

/** 第一个可聚焦（非 divider、非 disabled）项索引。 */
function firstFocusableIndex(items: MenuItem[]): number {
  for (let i = 0; i < items.length; i++) {
    if (!items[i].divider && !items[i].disabled) return i
  }
  return -1
}

/** 最后一个可聚焦项索引。 */
function lastFocusableIndex(items: MenuItem[]): number {
  for (let i = items.length - 1; i >= 0; i--) {
    if (!items[i].divider && !items[i].disabled) return i
  }
  return -1
}

function onGlobalMousedown(e: MouseEvent) {
  // 菜单本体已阻止冒泡，这里只处理点击到其它元素的情况。
  const target = e.target as HTMLElement | null
  if (target && target.closest('.context-menu')) return
  hideContextMenu()
}

function onGlobalKeydown(e: KeyboardEvent) {
  if (!state.visible) return
  switch (e.key) {
    case 'Escape':
      e.preventDefault()
      hideContextMenu()
      break
    case 'ArrowDown': {
      e.preventDefault()
      const last = lastFocusableIndex(state.items)
      if (last < 0) break
      let next = state.activeIndex
      for (let i = 0; i <= state.items.length; i++) {
        next = next >= last ? 0 : next + 1
        if (!state.items[next].divider && !state.items[next].disabled) {
          state.activeIndex = next
          break
        }
      }
      break
    }
    case 'ArrowUp': {
      e.preventDefault()
      const first = firstFocusableIndex(state.items)
      if (first < 0) break
      let prev = state.activeIndex
      for (let i = 0; i <= state.items.length; i++) {
        prev = prev <= first ? state.items.length - 1 : prev - 1
        if (!state.items[prev].divider && !state.items[prev].disabled) {
          state.activeIndex = prev
          break
        }
      }
      break
    }
    case 'Enter': {
      e.preventDefault()
      const item = state.items[state.activeIndex]
      if (item && !item.divider && !item.disabled) {
        item.action?.()
        hideContextMenu()
      }
      break
    }
    case 'Tab':
      // 阻止 Tab 把焦点移出菜单
      e.preventDefault()
      break
  }
}

/**
 * 全局 contextmenu 拦截：阻止浏览器原生右键菜单在整个应用内弹出。
 * 注册一次即可，showContextMenu/hideContextMenu 不影响此监听。
 * 用 capture 阶段确保先于页面内任何 handler 执行。
 */
function onGlobalContextMenu(e: MouseEvent) {
  // 允许在 input/textarea/contenteditable 里保留原生菜单(方便编辑操作)
  const target = e.target as HTMLElement | null
  if (target) {
    const tag = target.tagName
    if (
      tag === 'INPUT'
      || tag === 'TEXTAREA'
      || target.isContentEditable
    ) {
      return
    }
  }
  e.preventDefault()
}

function ensureGlobalListeners() {
  if (typeof window === 'undefined') return
  // contextmenu 拦截只需注册一次（常驻），mousedown/keydown 随菜单开关动态增删
  if (!(window as unknown as { __ctxMenuGlobalBound?: boolean }).__ctxMenuGlobalBound) {
    window.addEventListener('contextmenu', onGlobalContextMenu, true)
    ;(window as unknown as { __ctxMenuGlobalBound?: boolean }).__ctxMenuGlobalBound = true
  }
}

/** 打开右键菜单。items 的 action 可从 payload 读取上下文（如书籍对象）。 */
export function showContextMenu(
  e: MouseEvent | { clientX: number; clientY: number },
  items: MenuItem[],
  payload: unknown = null,
  opts: { width?: number } = {},
) {
  ensureGlobalListeners()
  const width = opts.width ?? 220
  const height = estimateMenuHeight(items)
  const pos = clampToViewport(e.clientY, e.clientX, width, height)
  state.visible = true
  state.top = pos.top
  state.left = pos.left
  state.width = width
  state.payload = payload
  state.items = items
  state.activeIndex = firstFocusableIndex(items)
  window.addEventListener('mousedown', onGlobalMousedown, true)
  window.addEventListener('keydown', onGlobalKeydown, true)
}

export function hideContextMenu() {
  state.visible = false
  state.items = []
  state.payload = null
  state.activeIndex = -1
  if (typeof window !== 'undefined') {
    window.removeEventListener('mousedown', onGlobalMousedown, true)
    window.removeEventListener('keydown', onGlobalKeydown, true)
  }
}

export function setActiveIndex(index: number) {
  state.activeIndex = index
}

export function disposeContextMenu() {
  hideContextMenu()
}

export function useContextMenu() {
  // ContextMenu 组件会在应用启动时挂载。提前安装捕获阶段的拦截器，
  // 避免必须先打开一次自定义菜单后，系统右键菜单才开始被禁用。
  ensureGlobalListeners()
  return {
    state: readonly(state),
    show: showContextMenu,
    hide: hideContextMenu,
    dispose: disposeContextMenu,
  }
}
