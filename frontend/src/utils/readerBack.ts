/**
 * 阅读页「返回」（Esc / 返回键 / 鼠标侧键）的逐层退出顺序。
 *
 * 抽成纯函数是为了能单测: 这一串优先级里有一条规则反直觉, 而它在界面上只
 * 表现为"Esc 要按两次", 很难联想到是工具栏在吃掉按键。
 */

export interface ReaderBackLayers {
  /** 目录/设置/书架等抽屉面板已打开。 */
  panelOpen: boolean
  /** 划词菜单已弹出。 */
  selectionMenuVisible: boolean
  /** 搜索面板已打开。 */
  searchOpen: boolean
  /** 朗读面板已打开。 */
  ttsOpen: boolean
  /** 书籍详情浮层已打开。 */
  bookInfoOpen: boolean
  /** 全屏「点击区域设置」九宫格已打开。 */
  clickAreaOpen?: boolean
}

export type ReaderBackTarget =
  | 'panel'
  | 'selectionMenu'
  | 'search'
  | 'tts'
  | 'bookInfo'
  | 'clickArea'
  | 'leave'

/**
 * 决定这一下「返回」该退哪一层: 弹层 → 选区菜单 → 搜索 → 朗读 → 书详情 → 退出。
 *
 * **工具栏（上/下菜单栏与两侧悬浮钮）不算一层**, 哪怕它正显示着。
 *
 * 旧实现把工具栏排在"退出"之前, 于是用户按第一次 Esc 只是把菜单栏收起来,
 * 第二次才回书架 —— 也就是 issue #19 报的「第一次会先隐藏顶层菜单栏, 第二次
 * 才退出书架」。工具栏与正文同属阅读页的一层界面, 不是盖在阅读页上的浮层:
 * 它本来就会随滚动、点击正文、指针离开而自己收起, 不需要靠「返回」来关。
 *
 * 反过来把工具栏算作一层还会让行为依赖鼠标位置: 桌面端指针停在边缘热区时
 * 工具栏会自己弹出(可配置), 那时同一串按键会时好时坏, 更难理解。
 *
 * 真正需要「先关掉」的是会挡住正文、并且必须显式关闭的层(抽屉、搜索、朗读、
 * 详情、划词菜单、全屏点击区域设置)—— 这些保持原顺序。
 *
 * 「点击区域设置」排在最前: 它是从设置抽屉里再打开的一层, 也是用户**最后**打开
 * 的那个界面, 所以"按 Esc 关掉最近打开的东西"要求先关它; 否则一次按键会直接
 * 连着把抽屉和阅读页都退掉。
 */
export function resolveReaderBackTarget(layers: ReaderBackLayers): ReaderBackTarget {
  if (layers.clickAreaOpen) return 'clickArea'
  if (layers.panelOpen) return 'panel'
  if (layers.selectionMenuVisible) return 'selectionMenu'
  if (layers.searchOpen) return 'search'
  if (layers.ttsOpen) return 'tts'
  if (layers.bookInfoOpen) return 'bookInfo'
  return 'leave'
}
