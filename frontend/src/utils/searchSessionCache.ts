import type { SearchBook } from '../types'

/**
 * 搜索结果的会话缓存，对齐 Legado 的搜索页语义。
 *
 * Legado 的搜索页是独立 Activity：点书名进详情/阅读后它仍在返回栈里，返回时
 * 同一个实例、结果原样还在，不会重搜。本项目的搜索页是 HomeView 里的组件，
 * 进阅读页会把它整个卸载，回来重新挂载时 `watch(..., { immediate: true })`
 * 又会跑一遍 doSearch() —— 表现就是「搜完点进去读几章，返回又要等一次全量
 * 搜索，而且结果从上千条变少（前面的批次还没回来）」。
 *
 * 这里按「搜索条件」缓存一次完整结果，重挂载时命中就直接铺出来，后台不再
 * 重新发请求；用户主动改关键词/换范围，或点了刷新才重搜。
 *
 * 为什么不落盘：Legado 确实把 searchBooks 存进了 SQLite，但那是为了跨启动
 * 复用同一批书上其它功能（换源、视频）的数据。搜索结果本身是**可重建的**，
 * 而且条数动辄上千、每条带简介，写进 localStorage 会让启动解析变慢。这里
 * 只做会话级（进程内存），重启后重搜一次即可 —— 与「可读书源缓存」用
 * localStorage 的理由不同（那份数据小且重建代价高）。
 */

/** 一次搜索的完整快照。 */
export interface SearchSessionEntry {
  /** 缓存键：搜索条件（关键词 + 范围 + 分组/书源）。 */
  key: string
  keyword: string
  results: SearchBook[]
  /** 结果是否已经收完（false = 中途停止/出错/还在搜，回来应继续搜）。 */
  complete: boolean
  /**
   * 上次收到第几个书源（后端 `lastIndex`）。
   *
   * 断点续搜用：搜索进行中进阅读页会让流中断，回来时从这里接着要后面的批次，
   * 而不是从头再搜一遍。`-1` 表示还没收到任何批次。
   */
  lastIndex: number
  savedAt: number
}

export interface SearchSessionCacheOptions {
  /** 最多保留几组搜索条件的结果。 */
  limit?: number
  /**
   * 结果保质期。只在**可复用**的意义上生效：超时的结果不会被当成「不用重搜」，
   * 但已经搜出来的数据本身仍会被替换掉（见 `get()`）。
   */
  ttlMs?: number
  /** 注入时钟，便于测试。 */
  now?: () => number
}

export interface SearchSessionCache {
  get(key: string): SearchSessionEntry | undefined
  set(key: string, keyword: string, results: SearchBook[], complete?: boolean, lastIndex?: number): void
  /** 只丢掉某一组条件的结果（重搜当前词时用），不影响其它关键词。 */
  delete(key: string): void
  clear(): void
  /** 这条结果是否已过保质期（调用方据此决定要不要重搜）。 */
  isStale(entry: SearchSessionEntry): boolean
  size(): number
}

/**
 * 由搜索条件拼出缓存键。
 *
 * 必须把关键词、范围、分组、书源都算进去：同一个词在「全部书源」和「单个书源」
 * 下结果是两批完全不同的书，混用会让用户看到上一个范围的结果。
 */
export function searchSessionKey(params: {
  keyword: string
  scope?: 'all' | 'group' | 'source'
  group?: string
  sourceUrl?: string
}): string {
  const keyword = params.keyword.trim()
  if (!keyword) return ''
  const scope = params.scope || 'all'
  const target = scope === 'group' ? (params.group || '') : (scope === 'source' ? (params.sourceUrl || '') : '')
  return `${keyword}\u0000${scope}\u0000${target}`
}

export const DEFAULT_SEARCH_SESSION_LIMIT = 8
/**
 * 搜索结果会话缓存的保质期。
 *
 * 这份缓存纯在内存里，进程退出即消失，看似不需要 TTL —— 但应用可以连开好几天，
 * 期间同一组条件的结果会一直躺着。搜索结果反映的是「书源上现在有什么」，
 * 放久了就是过期信息（书可能已下架、新增了章节/书源）。30 分钟与详情页可读书源
 * 缓存的 TTL 取齐，两处对「多旧算旧」的判断保持一致。
 */
export const DEFAULT_SEARCH_SESSION_TTL_MS = 30 * 60 * 1000

/**
 * 一次 watch 触发该不该复用缓存。
 *
 * 这是「搜索页 → 阅读页 → 返回不重搜」的核心判断，抽出来单独可测：
 *
 * - `firstTrigger`：本次组件实例的第一次触发（`immediate` 那次）。只有它
 *   可能读缓存 —— 它代表「进入 / 返回搜索页」。
 * - `sameKey`：触发条件与上一次真正搜索的条件相同。`ensureSearchSelection()`
 *   补齐下拉选项会二次触发同一个条件，这种不应该被当成用户改条件。
 * - `hasCache`：该条件下确实存有结果。
 *
 * 返回 `'reuse'` 表示直接铺缓存、不发请求；`'search'` 表示必须重新搜。
 */
export function decideSearchAction(params: {
  firstTrigger: boolean
  sameKey: boolean
  hasCache: boolean
}): 'reuse' | 'skip' | 'search' {
  // 同一条件被重复触发：屏幕上已经是这个条件的结果了，什么都不用做。
  if (!params.firstTrigger && params.sameKey) return 'skip'
  if (params.firstTrigger && params.hasCache) return 'reuse'
  return 'search'
}

export function createSearchSessionCache(
  options: SearchSessionCacheOptions = {},
): SearchSessionCache {
  const limit = Math.max(1, options.limit ?? DEFAULT_SEARCH_SESSION_LIMIT)
  const ttlMs = Math.max(0, options.ttlMs ?? DEFAULT_SEARCH_SESSION_TTL_MS)
  const now = options.now ?? (() => Date.now())
  const entries = new Map<string, SearchSessionEntry>()

  /** 清掉所有过期记录。
   *
   *  在 `set` / `get` / `size` 时顺手做：没有定时器（也不需要为此养一个
   *  `setInterval` 拖住事件循环），但保证「存在 Map 里的一定还在保质期内」，
   *  不会无限积压。 */
  function pruneExpired() {
    const cutoff = now() - ttlMs
    for (const [key, entry] of entries) {
      if (entry.savedAt <= cutoff) entries.delete(key)
    }
  }

  return {
    get(key) {
      pruneExpired()
      const entry = entries.get(key)
      if (!entry) return undefined
      // 读一次挪到末尾：Map 插入序即淘汰顺序，最近看过的不会被挤掉。
      entries.delete(key)
      entries.set(key, entry)
      return entry
    },

    set(key, keyword, results, complete = true, lastIndex = -1) {
      if (!key || !results.length) return
      // 先清过期的，再按 LRU 上限裁 —— 否则过期记录会占着名额把新结果挤掉。
      pruneExpired()
      entries.delete(key)
      entries.set(key, {
        key,
        keyword,
        results: results.slice(),
        complete,
        lastIndex,
        savedAt: now(),
      })
      while (entries.size > limit) {
        const oldest = entries.keys().next().value
        if (oldest === undefined) break
        entries.delete(oldest)
      }
    },

    delete(key) {
      entries.delete(key)
    },

    clear() {
      entries.clear()
    },

    isStale(entry) {
      return now() - entry.savedAt > ttlMs
    },

    size() {
      pruneExpired()
      return entries.size
    },
  }
}

/**
 * 全局共享的那一份搜索缓存。
 *
 * **必须从模块里取，不能在组件里 `createSearchSessionCache()`。**
 * Vue 的 `<script setup>` 顶层代码会被编译进 `setup()`，每个组件实例执行一次
 * —— 搜索页进阅读页时组件整个卸载，实例里的缓存跟着销毁，返回时新实例是空的，
 * 「返回不重搜」就永远不生效（第一版正是踩了这个坑）。
 *
 * 只有模块作用域的变量才是真·跨挂载存活。同理可参考 `getSourceCandidateCache()`。
 */
let sharedSearchSessionCache: SearchSessionCache | null = null

export function getSearchSessionCache(): SearchSessionCache {
  sharedSearchSessionCache ??= createSearchSessionCache({ limit: DEFAULT_SEARCH_SESSION_LIMIT })
  return sharedSearchSessionCache
}

/** 仅供测试：丢掉共享实例，让下次调用重新创建。 */
export function resetSearchSessionCacheForTests(): void {
  sharedSearchSessionCache = null
}
