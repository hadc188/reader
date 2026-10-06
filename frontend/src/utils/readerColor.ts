/** 阅读页自定义配色的取值校验。
 *
 *  这些值会直接写进 style, 且来自 localStorage, 必须先收紧为合法色值。 */

const HEX_LONG = /^#[0-9a-f]{6}$/
const HEX_SHORT = /^#[0-9a-f]{3}$/

/** 归一化为 #rrggbb; 非法或空值返回 ''(表示「跟随主题」)。 */
export function normalizeReaderColor(value: unknown): string {
  if (typeof value !== 'string') return ''
  const color = value.trim().toLowerCase()
  if (HEX_LONG.test(color)) return color
  if (HEX_SHORT.test(color)) {
    return `#${color[1]}${color[1]}${color[2]}${color[2]}${color[3]}${color[3]}`
  }
  return ''
}

export interface ReaderBackgroundInput {
  /** 归一化后的自定义阅读页背景色, '' 表示未设置。 */
  customBackgroundColor: string
  /** 阅读页专属背景图, '' 表示未设置。 */
  readerBackgroundImage: string
  backgroundImage: string
  applyBackgroundToReader: boolean
}

/** 阅读页背景来源。优先级由高到低。 */
export type ReaderBackgroundSource = 'readerImage' | 'readerColor' | 'desktopImage' | 'theme'

/** 决定阅读页背景用哪一层。
 *
 *  优先级: 阅读页专属背景图 > 自定义背景色 > 桌面背景图 > 主题预设。
 *  自定义色优先于桌面图 —— 否则「单独自定义阅读页背景」会被桌面图盖住;
 *  阅读页专属图又优先于纯色, 因为选图是更明确的意图。 */
export function resolveReaderBackground(input: ReaderBackgroundInput): ReaderBackgroundSource {
  if (input.readerBackgroundImage) return 'readerImage'
  if (input.customBackgroundColor) return 'readerColor'
  if (input.backgroundImage && input.applyBackgroundToReader) return 'desktopImage'
  return 'theme'
}

/** 是否该露出桌面背景图(供 App 外壳判断, 阅读页之外的页面仍用桌面图)。
 *  设了自定义阅读页背景色或专属图时, 阅读页由自己的图层接管。
 *
 *  @internal 只回答"**阅读页**是否露出桌面图"这一个问题; 外壳层该铺哪张图请用
 *  resolveShellBackgroundImage(它按 onReader 分流)。两者职责不同: 曾经把阅读页的
 *  优先级套到所有页面上, 导致关掉「应用到阅读页」后连桌面背景也一起消失。 */
export function shouldUseDesktopBackground(input: ReaderBackgroundInput): boolean {
  return resolveReaderBackground(input) === 'desktopImage'
}

/** 该在整窗最底层铺哪张图(含标题栏 —— 标题栏在 .app-body 之外, 只有铺在
 *  外壳层才能被覆盖到)。没有图时返回 ''。
 *
 *  分两种情况, 区别在于「是否处于阅读页」:
 *  - 阅读页: 沿用 resolveReaderBackground 的优先级, `applyBackgroundToReader`
 *    为 false 时桌面图让位给阅读主题(这正是该开关的语义)。
 *  - 其它页面(书架/设置/...): 只要有桌面背景图就铺 —— `applyBackgroundToReader`
 *    只管阅读页, 不该把桌面自己的背景一并关掉。
 *
 *  早先的实现对所有页面都套用阅读页优先级, 于是关掉该开关后连桌面背景也消失,
 *  表现为「必须进一次阅读页才看得到背景图」。 */
export function resolveShellBackgroundImage(input: ReaderBackgroundInput, onReader = true): string {
  if (!onReader) return input.backgroundImage
  const source = resolveReaderBackground(input)
  if (source === 'readerImage') return input.readerBackgroundImage
  if (source === 'desktopImage') return input.backgroundImage
  return ''
}
