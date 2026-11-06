/**
 * 阅读页「点击区域」九宫格。
 *
 * 对齐手机端 legado 的 `ClickActionConfigDialog`：把屏幕切成 3×3 九格，每格单独
 * 指定点击动作（呼出菜单 / 上一页 / 下一页 / 上一章 / 下一章 / 书签 / 目录 / 搜索 /
 * 听书 / 无操作）。手机端把九个格子存在 `clickActionTopLeft` 等九个偏好里，桌面端
 * 只有一个 localStorage 的 `readConfig`，所以统一存成 `clickAreas` 对象。
 *
 * **两种阅读模式各有一套独立的九宫格**（`ClickAreaProfiles`）：左右翻页一套、
 * 上下滚动一套，互不影响。这不是可有可无的拆分 —— 两种模式的分区方向本来就相反
 * （分页按列、滚动按行），同一个网格套到另一种模式上必然变味，所以它们各有各的
 * `custom` 开关与格子内容，用户可以在分页模式下用九宫格、滚动模式继续用默认三分区。
 *
 * 保持 `auto` 预设而不是把默认值写死成一张九宫格，是为了**不改变老用户的手感**：
 * 用户真正改过某一格之后才切到 `custom`，从那时起九宫格说了算。
 */

/** 九宫格位置：行(t/m/b) + 列(l/c/r)。 */
export type ClickAreaZone = 'tl' | 'tc' | 'tr' | 'ml' | 'mc' | 'mr' | 'bl' | 'bc' | 'br'

/** 网格顺序（渲染与遍历都用它，避免对象键序带来的不确定）。 */
export const CLICK_AREA_ZONES: readonly ClickAreaZone[] = [
  'tl', 'tc', 'tr',
  'ml', 'mc', 'mr',
  'bl', 'bc', 'br',
]

export type ClickAreaAction =
  | 'menu'
  | 'prevPage'
  | 'nextPage'
  | 'prevChapter'
  | 'nextChapter'
  | 'bookmark'
  | 'catalog'
  | 'search'
  | 'tts'
  | 'none'

export interface ClickAreaActionMeta {
  id: ClickAreaAction
  label: string
}

/** 动作表。顺序即选择面板里的顺序，与手机端一致：菜单/翻页/章节/功能。 */
export const CLICK_AREA_ACTIONS: readonly ClickAreaActionMeta[] = [
  { id: 'menu', label: '呼出菜单' },
  { id: 'prevPage', label: '上一页' },
  { id: 'nextPage', label: '下一页' },
  { id: 'prevChapter', label: '上一章' },
  { id: 'nextChapter', label: '下一章' },
  { id: 'bookmark', label: '加入书签' },
  { id: 'catalog', label: '目录' },
  { id: 'search', label: '搜索' },
  { id: 'tts', label: '听书' },
  { id: 'none', label: '无操作' },
]

const ACTION_IDS = new Set<string>(CLICK_AREA_ACTIONS.map((action) => action.id))

export type ClickAreaMap = Record<ClickAreaZone, ClickAreaAction>

/** 一套九宫格属于哪种阅读模式。 */
export type ClickAreaProfile = 'paged' | 'scroll'

export const CLICK_AREA_PROFILES: readonly ClickAreaProfile[] = ['paged', 'scroll']

/** 两种模式各自的九宫格内容。 */
export type ClickAreaProfiles = Record<ClickAreaProfile, ClickAreaMap>

/** 两种模式各自的「用默认三分区 / 用九宫格」开关。 */
export type ClickAreaModes = Record<ClickAreaProfile, 'auto' | 'custom'>

/** 模式的可读名（界面上的两个切换按钮）。 */
export const CLICK_AREA_PROFILE_LABELS: Record<ClickAreaProfile, string> = {
  paged: '左右翻页',
  scroll: '上下滚动',
}

/** 左右翻页：左列上一页、中列菜单、右列下一页。 */
export function pagedClickAreas(): ClickAreaMap {
  return {
    tl: 'prevPage', tc: 'menu', tr: 'nextPage',
    ml: 'prevPage', mc: 'menu', mr: 'nextPage',
    bl: 'prevPage', bc: 'menu', br: 'nextPage',
  }
}

/** 上下滚动：上行上一页、中行菜单、下行下一页。 */
export function scrollClickAreas(): ClickAreaMap {
  return {
    tl: 'prevPage', tc: 'prevPage', tr: 'prevPage',
    ml: 'menu', mc: 'menu', mr: 'menu',
    bl: 'nextPage', bc: 'nextPage', br: 'nextPage',
  }
}

/** 某个阅读模式的默认预设。 */
export function presetClickAreas(kind: ClickAreaProfile): ClickAreaMap {
  return kind === 'paged' ? pagedClickAreas() : scrollClickAreas()
}

/** 两套预设（各自独立的新对象）。 */
export function defaultClickAreaProfiles(): ClickAreaProfiles {
  return { paged: pagedClickAreas(), scroll: scrollClickAreas() }
}

/** 两套都从默认三分区起步。 */
export function defaultClickAreaModes(): ClickAreaModes {
  return { paged: 'auto', scroll: 'auto' }
}

/**
 * 把点击坐标换算成九宫格位置。
 *
 * 入参是 0..1 的比例，调用方负责除以视口宽高。边界值（恰好 1/3、2/3）归入后
 * 一格，和手机端 `RectF.contains` 的左闭右开一致。
 */
export function clickAreaZoneAt(xRatio: number, yRatio: number): ClickAreaZone {
  const column = xRatio < 1 / 3 ? 'l' : xRatio < 2 / 3 ? 'c' : 'r'
  const row = yRatio < 1 / 3 ? 't' : yRatio < 2 / 3 ? 'm' : 'b'
  return `${row}${column}` as ClickAreaZone
}

/** 宽松解析单套九宫格：缺格/脏值一律回落预设，避免出现空动作。 */
export function normalizeClickAreas(raw: unknown, fallback: ClickAreaMap): ClickAreaMap {
  const result = { ...fallback }
  if (!raw || typeof raw !== 'object' || Array.isArray(raw)) return result
  const record = raw as Record<string, unknown>
  for (const zone of CLICK_AREA_ZONES) {
    const value = record[zone]
    if (typeof value === 'string' && ACTION_IDS.has(value)) {
      result[zone] = value as ClickAreaAction
    }
  }
  return result
}

/** 宽松解析两套九宫格。缺一套就把那一套回落预设，另一套照常保留。 */
export function normalizeClickAreaProfiles(raw: unknown, fallback: ClickAreaProfiles): ClickAreaProfiles {
  const record = (raw && typeof raw === 'object' && !Array.isArray(raw))
    ? raw as Record<string, unknown>
    : {}
  return {
    paged: normalizeClickAreas(record.paged, fallback.paged),
    scroll: normalizeClickAreas(record.scroll, fallback.scroll),
  }
}

/** 宽松解析两套开关；只认 `custom`，其余一律按 `auto`（脏数据不该开启九宫格）。 */
export function normalizeClickAreaModes(raw: unknown): ClickAreaModes {
  const record = (raw && typeof raw === 'object' && !Array.isArray(raw))
    ? raw as Record<string, unknown>
    : {}
  return {
    paged: record.paged === 'custom' ? 'custom' : 'auto',
    scroll: record.scroll === 'custom' ? 'custom' : 'auto',
  }
}

/**
 * 把老存档里「单套 clickAreas + 单个 clickAreaMode」迁移成两套。
 *
 * 旧版本两套共用一张网格，所以迁移时**两套都填同一份内容**，老用户手感不变。
 * 新存档里 `clickAreas` 已经是 `{paged, scroll}` 对象，不会被这条路径碰到。
 */
export function migrateLegacyClickAreas(
  legacyAreas: unknown,
  legacyMode: unknown,
  fallback: ClickAreaProfiles,
): { areas: ClickAreaProfiles; modes: ClickAreaModes } {
  const shared = normalizeClickAreas(legacyAreas, fallback.scroll)
  const custom = legacyMode === 'custom'
  return {
    areas: { paged: { ...shared }, scroll: { ...shared } },
    modes: { paged: custom ? 'custom' : 'auto', scroll: custom ? 'custom' : 'auto' },
  }
}


/** 九宫格里是否还有「呼出菜单」——一个都没有就等于把自己锁在正文里。 */
export function hasMenuZone(areas: ClickAreaMap): boolean {
  return CLICK_AREA_ZONES.some((zone) => areas[zone] === 'menu')
}

/**
 * 改一格并保证菜单可达。
 *
 * 手机端在关闭设置时做 `detectClickArea()`：九格都不是菜单就把中间格强制设回菜单。
 * 这里在**写配置的那一刻**就做，返回 `forcedCenterMenu` 供界面提示，避免用户点完
 * 保存才发现再也呼不出工具栏。
 */
export function setClickAreaAction(
  areas: ClickAreaMap,
  zone: ClickAreaZone,
  action: ClickAreaAction,
): { areas: ClickAreaMap; forcedCenterMenu: boolean } {
  const next: ClickAreaMap = { ...areas, [zone]: action }
  if (hasMenuZone(next)) return { areas: next, forcedCenterMenu: false }
  // 用户自己把中间格设成非菜单时，让位给「他刚点的这一格」还是「中间格」？
  // 手机端的做法是重启式地把中间格抢回来：菜单是唯一出口，不能动。
  return { areas: { ...next, mc: 'menu' }, forcedCenterMenu: true }
}

/** 动作的可读名，找不到时回落 `无操作`（脏数据不该渲染成 undefined）。 */
export function clickAreaActionLabel(action: ClickAreaAction | undefined): string {
  return CLICK_AREA_ACTIONS.find((item) => item.id === action)?.label ?? '无操作'
}
