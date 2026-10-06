/** 书架分组位域运算。
 *
 *  书的 `group` 是一个位域: 第 N 位为 1 表示这本书属于 id 为 `1 << N` 的分组
 *  (Legado 的约定)。删除分组时必须把对应位清掉, 否则书会继续"属于"一个已不存在的
 *  分组 —— 界面表现为分组计数不对、书在筛选后凭空消失。
 *
 *  这些是纯函数, 便于单测: 位运算出错很难靠肉眼发现。 */

/** 清掉一本书里属于 `groupIds` 的位。返回新对象; 没有变化时返回原对象。 */
export function clearGroupsFromBook<T extends { group?: number }>(book: T, groupIds: number[]): T {
  const mask = groupMask(groupIds)
  if (mask === 0) return book
  const current = book.group ?? 0
  // `& ~mask` 只清理目标位, 其它分组的归属原样保留。
  const next = current & ~mask
  if (next === current) return book
  return { ...book, group: next }
}

/** 把一组分组 id 合成位掩码。负数与非整数会被忽略 ——
 *  位域只能表达 `1 << N` 这种正值, 负数在 Legado 里是虚拟分组(不参与位域)。 */
export function groupMask(groupIds: number[]): number {
  let mask = 0
  for (const id of groupIds) {
    if (!Number.isInteger(id) || id <= 0) continue
    // 位运算在 JS 里是 32 位有符号, 超出范围会回绕 —— 当作无效位忽略。
    if (id > 0x8000_0000) continue
    mask |= id
  }
  return mask
}

/** 一本书是否属于任一给定分组。
 *
 *  @internal 当前生产代码只用 clearGroupsFromBook(删除分组时清位); 这个正向判定
 *  暂无调用点, 保留是因为它与 groupMask 一起界定了位域语义, 并有测试固定行为。
 *  若将来要按分组筛选, 用它而不是在组件里重写一遍 `group & mask`。 */
export function bookInAnyGroup(book: { group?: number }, groupIds: number[]): boolean {
  const mask = groupMask(groupIds)
  if (mask === 0) return false
  return ((book.group ?? 0) & mask) !== 0
}
