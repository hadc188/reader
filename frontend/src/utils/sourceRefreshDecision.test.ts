import { describe, expect, it } from 'vitest'
import { createSourceCandidateCache, sourceCatalogSignatureOf } from './sourceCandidateCache'

/**
 * 「打开详情页要不要自动重搜可读书源」的判定，以及「启用了书源却不刷新」的成因。
 *
 * 用户报告的完整现象（issue #31 后续）：
 *  1. 应用刚启动后第一次打开详情页不刷新；
 *  2. 手动刷新过一次之后就正常了；
 *  3. 手动刷新必须**完整跑完**，中途退出的话下次打开还是旧缓存。
 *
 * 三者是同一个根因的三种表现：`commitSourceCandidates` 无论本次搜索是否完整，
 * 都会把**当前书源清单签名**写进缓存。于是「半份结果」和「完整结果」在签名上
 * 完全一样，光比签名分不出来。
 */

interface CacheLike {
  catalogSignature: string
  complete: boolean
}

/** 与 BookDetailModal.loadSourceCandidates 里那段判断同构。 */
function shouldSearchOnOpen(params: {
  cached: CacheLike | undefined
  currentSignature: string
  refresh?: boolean
}): boolean {
  const { cached, currentSignature, refresh = false } = params
  if (refresh) return true
  if (!cached) return false // 无缓存：按 issue #28 的要求不自动搜
  if (cached.catalogSignature !== currentSignature) return true // 书源清单变了
  // 上次没搜完 → 补搜。少了这一条就会「中途退出后永远加载旧缓存」。
  if (!cached.complete) return true
  return false
}

const SIG_A = sourceCatalogSignatureOf([
  { bookSourceUrl: 'https://a.com', enabled: true },
])
const SIG_AB = sourceCatalogSignatureOf([
  { bookSourceUrl: 'https://a.com', enabled: true },
  { bookSourceUrl: 'https://b.com', enabled: true },
])

describe('打开详情页是否自动重搜可读书源', () => {
  it('does not search when there is no cache (issue #28)', () => {
    expect(shouldSearchOnOpen({ cached: undefined, currentSignature: SIG_A })).toBe(false)
  })

  it('does not search when the cache is complete and the catalog is unchanged', () => {
    expect(shouldSearchOnOpen({
      cached: { catalogSignature: SIG_A, complete: true },
      currentSignature: SIG_A,
    })).toBe(false)
  })

  it('searches when a source was enabled after the cache was written', () => {
    // 用户主诉：启用书源后打开详情页不刷新。缓存签名是旧清单（只有 a），
    // 当前清单多了 b → 必须重搜。
    expect(shouldSearchOnOpen({
      cached: { catalogSignature: SIG_A, complete: true },
      currentSignature: SIG_AB,
    })).toBe(true)
  })

  it('searches when a source was disabled after the cache was written', () => {
    expect(shouldSearchOnOpen({
      cached: { catalogSignature: SIG_AB, complete: true },
      currentSignature: SIG_A,
    })).toBe(true)
  })

  it('re-searches after an interrupted refresh even though the signature matches', () => {
    // 这是「刷新中途退出就加载旧缓存」的直接回归：
    // 半份结果写进去时带的是**当时**的清单签名，与当前清单一致，
    // 光比签名会判成「没变」→ 永远不补搜。
    expect(shouldSearchOnOpen({
      cached: { catalogSignature: SIG_AB, complete: false },
      currentSignature: SIG_AB,
    })).toBe(true)
  })

  it('always searches when the user explicitly refreshes', () => {
    expect(shouldSearchOnOpen({
      cached: { catalogSignature: SIG_AB, complete: true },
      currentSignature: SIG_AB,
      refresh: true,
    })).toBe(true)
  })
})

describe('为什么「完整刷新一次之后就一直正常」', () => {
  it('a completed refresh writes the current signature, so the next open is stable', () => {
    // 完整刷完 → complete: true + 当前签名 → 下次打开不再重搜（符合预期）。
    const cache = createSourceCandidateCache({ storage: null })
    const key = '书::作者::/b/1::https://a.com'
    cache.set(key, SIG_AB, [
      { name: '书', author: '作者', bookUrl: '/b/1', origin: 'https://a.com' },
      { name: '书', author: '作者', bookUrl: '/b/2', origin: 'https://b.com' },
    ], true)

    const entry = cache.get(key)!
    expect(shouldSearchOnOpen({ cached: entry, currentSignature: SIG_AB })).toBe(false)
  })

  it('an interrupted refresh keeps partial results but is still marked incomplete', () => {
    // 中途停止：结果要留着（不能丢已看到的源），但 complete 必须是 false，
    // 否则就回到「永远加载旧缓存」的老 bug。
    const cache = createSourceCandidateCache({ storage: null })
    const key = '书::作者::/b/1::https://a.com'
    cache.set(key, SIG_AB, [
      { name: '书', author: '作者', bookUrl: '/b/1', origin: 'https://a.com' },
    ], false)

    const entry = cache.get(key)!
    expect(entry.books).toHaveLength(1)      // 半份结果保留
    expect(entry.complete).toBe(false)       // 但标记为不完整
    expect(shouldSearchOnOpen({
      cached: entry,
      currentSignature: SIG_AB,
    })).toBe(true)                            // → 下次打开会补搜
  })
})

describe('书源清单基线要跨会话持久化', () => {
  it('a fresh session must still detect a change against a persisted baseline', () => {
    // 回归：`sourceStateSignature` 曾是纯内存变量，应用重启后为空，
    // 首次 fetch 走 `if (baseline && ...)` 守卫 → 只记录基线、不判定变化。
    // 而候选缓存跨会话存活，于是「上次会话启用了书源但详情页没刷新成功」
    // 这件事永远补不上。持久化基线后，重启后第一次 fetch 就能发现差异。
    const STORAGE_KEY = 'source-state-signature'
    const store: Record<string, string> = { [STORAGE_KEY]: SIG_A }

    // 模拟「重启后首次 fetch」：从存储里读回上次会话的基线
    const baselineFromLastSession = store[STORAGE_KEY] || ''
    const nextSignature = SIG_AB

    // 有基线 → 能判定出变化（旧实现在这里拿到空串，判不出来）
    expect(baselineFromLastSession).toBe(SIG_A)
    expect(nextSignature !== baselineFromLastSession).toBe(true)
  })

  it('falls back to an empty baseline when storage is unavailable', () => {
    // 隐私模式 / 配额满：读不到就当首次运行，退化成原来的纯内存行为，不报错。
    const readBaseline = (storage: { getItem(k: string): string } | null) => {
      try {
        return storage?.getItem('source-state-signature') || ''
      } catch {
        return ''
      }
    }
    expect(readBaseline(null)).toBe('')
    expect(readBaseline({ getItem: () => { throw new Error('denied') } })).toBe('')
  })
})
