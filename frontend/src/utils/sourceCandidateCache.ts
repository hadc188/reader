import type { SearchBook } from '../types'

/**
 * 详情页「可读书源」结果的会话内缓存（issue #28 第 2 点）。
 *
 * 背景：弹窗一打开就并发搜 12 路书源，既有明显的网络与磁盘开销，也会让
 * 用户只是想看看目录时被迫等一次全量搜索。现在改为「打开只读缓存，要最新
 * 的自己点刷新」。
 *
 * 每条记录都带**书源目录签名**（启用源地址 + 启停状态的排序拼串）：
 * - 签名一致 → 缓存仍然可信，直接用；
 * - 签名不一致 → 用户增删/停启过书源，缓存作废，应当自动重搜一次。
 *   这就是「书源变了就自动刷新」的落点，不需要额外记一份源列表。
 *
 * 一个关键取舍：**刷新不清空旧缓存**。用户点了刷新、看到 8 个源、中途按了
 * 停止，下次打开必须还是那 8 个 —— 所以中断时把「已拿到的」和「旧缓存的」
 * 并起来存，只有完整搜完之后才用新结果替换旧结果。
 */

/** 一条缓存记录：某本书 + 某份书源清单下的一次搜索结果。 */
export interface SourceCandidateCacheEntry {
  bookKey: string
  /** 产生这份结果时的书源目录签名；与当前签名不符即说明书源被改过。 */
  catalogSignature: string
  books: SearchBook[]
  savedAt: number
  /** 上次搜索是否完整跑完（没有中途停止、没有源访问失败）。 */
  complete: boolean
}

/** localStorage 里存这份缓存的键。 */
export const SOURCE_CANDIDATE_STORAGE_KEY = 'source-candidates-cache'

/** 可注入的存储后端（默认 localStorage）。测试里传一个内存实现。 */
export interface SourceCandidateStorage {
  getItem(key: string): string | null
  setItem(key: string, value: string): void
  removeItem(key: string): void
}

export interface SourceCandidateCacheOptions {
  /** 注入时钟，便于测试。 */
  now?: () => number
  /** 持久化后端；传 null 表示只在内存里（默认读 localStorage）。 */
  storage?: SourceCandidateStorage | null
  /** 持久化用的键。 */
  storageKey?: string
}

export interface SourceCandidateCache {
  /** 取某本书的缓存（不分签名）。读一次算一次使用。 */
  get(bookKey: string): SourceCandidateCacheEntry | undefined
  /** 写入某本书的缓存；同一本书只保留一条（旧的签名直接被顶掉）。 */
  set(bookKey: string, catalogSignature: string, books: SearchBook[], complete?: boolean): void
  /** 丢弃某本书的缓存（这本书被删掉/移出书架时调用）。返回是否真的删了。 */
  delete(bookKey: string): boolean
  /** 丢弃全部缓存（书源被整体清空/批量导入这类全量变更时调用）。 */
  clear(): void
  /** 判断一条缓存是否与当前书源清单相符。 */
  matches(entry: SourceCandidateCacheEntry, catalogSignature: string): boolean
  /** 当前条目数（不做任何清理）。 */
  size(): number
}

export function createSourceCandidateCache(
  options: SourceCandidateCacheOptions = {},
): SourceCandidateCache {
  const now = options.now ?? (() => Date.now())
  const storage = options.storage === undefined ? defaultStorage() : options.storage
  const storageKey = options.storageKey ?? SOURCE_CANDIDATE_STORAGE_KEY

  /** 缓存条目。**没有数量上限**，也不会按时间过期删除。
   *
   *  只有下面三种显式操作会移除条目：
   *    1. 书被删除 / 移出书架（`delete()`，见 bookshelf store）；
   *    2. 书源被整表清空（`clear()`，见 WebDAV 恢复）；
   *    3. 写法覆盖：同一本书再搜一次，用新结果顶掉旧的（`set()`）。
   *
   *  为什么不做 LRU 上限：每条只有几条到十几条搜索结果，量级很小；
   *  而用户删掉的缓存不该因为「点开过 21 本书」就悄悄消失 —— 那正是
   *  「几小时前的缓存打开就没了」的同类问题。 */
  const entries = new Map<string, SourceCandidateCacheEntry>()

  /** 把内存里的内容整份写回存储。
   *
   *  写失败（隐私模式 / 配额满）时静默降级成纯内存缓存 —— 缓存不是关键数据,
   *  为了它弹一个错误反而更糟。 */
  function persist() {
    if (!storage) return
    try {
      storage.setItem(storageKey, JSON.stringify({
        version: 1,
        entries: [...entries.values()],
      }))
    } catch {
      // 忽略：下次打开会重新搜一次，仅此而已。
    }
  }

  /** 启动时把上次的内容读回来。数据损坏就当没有 —— 绝不因为缓存坏了打不开详情页。 */
  function hydrate() {
    if (!storage) return
    let raw: string | null = null
    try {
      raw = storage.getItem(storageKey)
    } catch {
      return
    }
    if (!raw) return
    try {
      const parsed = JSON.parse(raw) as { version?: number; entries?: unknown }
      if (!Array.isArray(parsed.entries)) return
      for (const item of parsed.entries) {
        // 逐条校验: 一条坏记录不该让整份缓存作废。
        if (!item || typeof item !== 'object') continue
        const entry = item as Partial<SourceCandidateCacheEntry>
        if (
          typeof entry.bookKey !== 'string'
          || typeof entry.catalogSignature !== 'string'
          || typeof entry.savedAt !== 'number'
          || !Array.isArray(entry.books)
        ) continue
        entries.set(entry.bookKey, {
          bookKey: entry.bookKey,
          catalogSignature: entry.catalogSignature,
          books: entry.books as SearchBook[],
          savedAt: entry.savedAt,
          complete: entry.complete !== false,
        })
      }
      // 原样读回：不按 TTL 清、也不按数量裁。
    } catch {
      entries.clear()
    }
  }

  hydrate()

  return {
    get(bookKey) {
      const entry = entries.get(bookKey)
      if (!entry) return undefined
      // 读一次就把它挪到末尾：Map 的插入序即淘汰顺序，常用的书不会被挤掉。
      entries.delete(bookKey)
      entries.set(bookKey, entry)
      return entry
    },

    set(bookKey, catalogSignature, books, complete = true) {
      // 只缓存非空结果：一条都没搜到通常意味着当时的网络/源站异常，缓存它
      // 会让用户下次打开看到「未找到其他书源」，反而更困惑。
      if (!books.length) return
      entries.delete(bookKey)
      entries.set(bookKey, {
        bookKey,
        catalogSignature,
        books: books.slice(),
        savedAt: now(),
        complete,
      })
      persist()
    },

    delete(bookKey) {
      const existed = entries.delete(bookKey)
      // 只有真删到东西才落盘：删书是批量操作，避免对每条不存在的记录都写一次。
      if (existed) persist()
      return existed
    },

    clear() {
      entries.clear()
      if (!storage) return
      try {
        storage.removeItem(storageKey)
      } catch {
        // 忽略
      }
    },

    matches(entry, catalogSignature) {
      return entry.catalogSignature === catalogSignature
    },

    size() {
      return entries.size
    },
  }
}

/** 默认后端：localStorage。SSR / 测试环境没有它时退化成 null（纯内存）。 */
function defaultStorage(): SourceCandidateStorage | null {
  try {
    if (typeof localStorage === 'undefined') return null
    return localStorage
  } catch {
    return null
  }
}

/**
 * 全局共享的那一份缓存（详情页弹窗 + 阅读页换源面板共用）。
 *
 * 两处必须用**同一个实例**，不能各自 `createSourceCandidateCache()`：
 * 那样只有 localStorage 里的内容共享，对方在本会话内新写的结果本进程看不到，
 * 于是「详情页刷出 6 个源 → 进阅读页换源还是 3 个」。
 */
let sharedCache: SourceCandidateCache | null = null

export function getSourceCandidateCache(): SourceCandidateCache {
  sharedCache ??= createSourceCandidateCache()
  return sharedCache
}

/** 仅供测试：丢掉共享实例，让下次调用重新读取存储。 */
export function resetSourceCandidateCacheForTests(): void {
  sharedCache = null
}

/** 由启用的书源地址算一份「书源目录签名」。
 *
 *  详情页与阅读页都用它，保证两边对「缓存是否过期」的判断完全一致 ——
 *  否则一个面板认为过期、另一个认为不过期，缓存就会互相打架。
 */
export function sourceCatalogSignatureOf(
  sources: ReadonlyArray<{ bookSourceUrl: string; enabled?: boolean }>,
): string {
  return sources
    .filter((source) => source.enabled !== false)
    .map((source) => source.bookSourceUrl.trim().replace(/\/+$/, ''))
    .sort()
    .join('\n')
}

/** 书籍身份键。两个面板口径必须一致，否则会各写一份、互相看不见。 */
export function sourceCandidateBookKey(
  book: { name: string; author?: string; bookUrl: string; origin: string },
): string {
  return `${book.name}::${book.author ?? ''}::${book.bookUrl}::${book.origin}`
}



/**
 * 把一轮搜索的结果写回缓存时该存什么。
 *
 * - 完整搜完：这一轮就是权威结果，直接替换。
 * - 被打断 / 部分源失败：与旧缓存**合并**。用户点刷新只是想补全，不是想丢掉
 *   已经看到的那些源 —— 曾经这里是直接覆盖，于是「刷新显示 8 个、中途停止、
 *   再打开变回刷新前的 4 个」。合并后 `complete = false`，下次打开能如实提示。
 *
 * `merge` 由调用方注入（项目里是 searchRank 的 mergeSourceCandidates），
 * 这样这个模块不依赖业务排序规则，测试里也能用简单的实现。
 */
export function mergeForCommit(
  previous: readonly SearchBook[],
  current: readonly SearchBook[],
  complete: boolean,
  merge: (a: readonly SearchBook[], b: readonly SearchBook[]) => SearchBook[],
): SearchBook[] {
  return complete ? current.slice() : merge(previous, current)
}
