import { isBookOnShelf } from './bookEdit'
import type { Book } from '../types'

/**
 * 阅读页「加入书架」的显示判定（issue #28 第 1 点）。
 *
 * 需求是「当前书籍未加入书架时，能通过菜单加入」。难点不在写入，而在**判定
 * 时机**：书架 store 是异步拉的，阅读页可能比它先就绪（直接开书、深链、重启
 * 后从最近阅读进入）。如果那会儿书架还是空数组就下判断，会认定「未上架」，
 * 于是对一本其实已经在书架里的书也显示「加入书架」——点下去是一次覆盖写。
 *
 * 所以这里把「能不能安全判定」和「判定结果」分开：
 * - `shelfReady` 为假（还没加载过）→ `unknown`，界面什么都不做；
 * - 加载完成 → 用 `isBookOnShelf` 同时按 bookUrl 与「书名+作者」两个口径比，
 *   与搜索页/详情页的判重保持一致。
 */
export type ShelfMembership = 'unknown' | 'on-shelf' | 'off-shelf'

export interface ShelfState {
  /** 书架书列表。 */
  books: ReadonlyArray<{ bookUrl: string; name?: string; author?: string; origin?: string }>
  /** 是否已经成功拉过一次书架（失败或被跳过都算没就绪）。 */
  loaded: boolean
  /** 是否正在加载。加载中同样不能判定。 */
  loading?: boolean
}

export function resolveShelfMembership(
  book: Pick<Book, 'bookUrl' | 'name' | 'author' | 'origin'> | null | undefined,
  shelf: ShelfState,
): ShelfMembership {
  if (!book?.bookUrl) return 'unknown'
  if (!shelf.loaded || shelf.loading) return 'unknown'
  return isBookOnShelf(
    book,
    shelf.books as { bookUrl: string; name?: string; author?: string; origin?: string }[],
  )
    ? 'on-shelf'
    : 'off-shelf'
}

/** 「加入书架」这个动作当前是否应该出现。
 *
 *  只在**确定未上架**时出现。已上架、未知一律不出现：
 * - 已上架 → 按需求隐藏，避免误触覆盖；
 * - 未知   → 宁可先不显示，也不要给出一个可能写坏数据的按钮。
 */
export function canAddCurrentBookToShelf(
  membership: ShelfMembership,
  options: { readonly?: boolean; adding?: boolean } = {},
): boolean {
  if (options.readonly) return false
  if (options.adding) return false
  return membership === 'off-shelf'
}
