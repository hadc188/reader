/** 章节「已读」历史。
 *
 *  这些标记只服务阅读页目录的「已读」角标与「隐藏已读」模式，纯前端可见，
 *  因此存在浏览器 localStorage 而不是后端缓存：按书 URL 分键，一本书一条。
 *  删除书籍时必须一并删掉，否则重新加入同一本书会继承上一次的已读状态。
 */

export const READ_HISTORY_PREFIX = 'reader-read-history:'

/** 某本书的已读历史存储键；没有 bookUrl 时返回空串。 */
export function readHistoryStorageKey(bookUrl?: string | null) {
  const url = bookUrl?.trim()
  return url ? `${READ_HISTORY_PREFIX}${url}` : ''
}

/** 删除一本书的已读历史。返回是否真的删掉了某个键。 */
export function clearReadHistory(bookUrl?: string | null) {
  const key = readHistoryStorageKey(bookUrl)
  if (!key) return false
  try {
    const existed = localStorage.getItem(key) !== null
    localStorage.removeItem(key)
    return existed
  } catch {
    // 隐私模式/配额异常下 localStorage 可能不可用，不能因此让删书失败。
    return false
  }
}
