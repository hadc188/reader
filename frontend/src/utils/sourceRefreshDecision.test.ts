import { describe, expect, it } from 'vitest'
import {
  createSourceCandidateCache,
  sourceCatalogSignatureOf,
  type SourceCandidateStorage,
} from './sourceCandidateCache'

/**
 * 详情页 / 换源面板如何对待已有缓存。
 *
 * 早先靠「书源清单签名」判断要不要重搜，但签名只记录**当时**用的是哪份清单：
 * 用户升级版本后，缓存里可能只含 A/B 两个源的结果，而实际清单有 A~E ——
 * 若 C/D/E 当时不在清单里，算出来的签名与缓存里那份**恰好相同**，于是判成
 * 「没变」→ 只显示 2 个源，而且启停 C/D/E 也不会刷新。
 *
 * 现在统一成一条规则：**打开就搜，旧缓存垫底先显示**。下面这些用例锁住
 * 替代它的那部分行为（缓存仍然要能用于首屏、中断结果不能丢、写入仍记签名）。
 */

const SIG_AB = sourceCatalogSignatureOf([
  { bookSourceUrl: 'https://a.com', enabled: true },
  { bookSourceUrl: 'https://b.com', enabled: true },
])

function bookOf(origin: string): {
  name: string
  author: string
  bookUrl: string
  origin: string
} {
  return { name: '书', author: '作者', bookUrl: `/${origin}`, origin }
}

/** 内存版 localStorage：用来模拟「关掉应用再打开」。 */
function memoryStorage() {
  const data = new Map<string, string>()
  const storage: SourceCandidateStorage = {
    getItem: (key) => data.get(key) ?? null,
    setItem: (key, value) => { data.set(key, value) },
    removeItem: (key) => { data.delete(key) },
  }
  return { storage, data }
}

describe('旧缓存用于首屏渲染（搜索在后台同时进行）', () => {
  it('exposes the cached list so the panel can render before the search finishes', () => {
    // 方案 A 的体验前提：打开时先摆出上次的结果，用户不必空等一次全量搜索。
    // 这条锁住「缓存仍然可读」—— 如果哪天为了简化把缓存也删了，首屏会白屏。
    const cache = createSourceCandidateCache({ storage: null })
    const key = '书::作者::/b/1::https://a.com'
    cache.set(key, SIG_AB, [bookOf('https://a.com'), bookOf('https://b.com')], true)

    expect(cache.get(key)!.books).toHaveLength(2)
  })

  it('does not require the cached signature to match the current catalog', () => {
    // 关键：方案 A 下签名不参与「要不要搜」的决策，所以签名对不上时缓存照样
    // 可以用于首屏。旧实现会在这里判定「清单变了」——方向是对的，但仍会漏掉
    // 「签名碰巧相同、结果却只含一部分源」的那种情况。
    const cache = createSourceCandidateCache({ storage: null })
    const key = '书::作者::/b/1::https://a.com'
    const otherSig = sourceCatalogSignatureOf([
      { bookSourceUrl: 'https://z.com', enabled: true },
    ])
    cache.set(key, otherSig, [bookOf('https://a.com')], true)

    const entry = cache.get(key)!
    expect(cache.matches(entry, SIG_AB)).toBe(false) // 签名确实对不上
    expect(entry.books).toHaveLength(1)              // 但缓存依然可读、可用
  })
})

describe('中断的搜索不会丢掉已有结果', () => {
  it('keeps partial results and marks the round incomplete', () => {
    // 中途停止 / 部分源失败：结果要留着（用户想要的正是「就这些」），
    // 但 complete 必须是 false，好让界面如实提示「部分书源访问失败」。
    // 「下次打开会不会补搜」不再依赖这个标记 —— 打开就搜，必然补上。
    const cache = createSourceCandidateCache({ storage: null })
    const key = '书::作者::/b/1::https://a.com'
    cache.set(key, SIG_AB, [bookOf('https://a.com')], false)

    const entry = cache.get(key)!
    expect(entry.books).toHaveLength(1)
    expect(entry.complete).toBe(false)
  })
})

describe('写入仍然记录书源清单签名', () => {
  it('records the signature alongside the results', () => {
    // 签名不再决定「要不要搜」，但仍写进条目：它描述「这份结果是对哪份清单搜的」，
    // 是排查问题时唯一能还原当时上下文的信息。
    const cache = createSourceCandidateCache({ storage: null })
    const key = '书::作者::/b/1::https://a.com'
    cache.set(key, SIG_AB, [bookOf('https://a.com')], true)

    expect(cache.get(key)!.catalogSignature).toBe(SIG_AB)
  })
})

describe('搜到一个就写一次缓存（不再等全部跑完）', () => {
  it('persists each batch so an interrupted search keeps what was already found', () => {
    // 用户报的问题：搜索必须**彻底跑完**才会写缓存。搜到一半关掉详情页再打开，
    // 磁盘上还是搜索前那份 —— 明明已经搜到 3 个源，却全丢了。
    // 修法：`onmessage` 每收到一批就 commit 一次（complete=false）。
    const { storage, data } = memoryStorage()
    const cache = createSourceCandidateCache({ storage })
    const key = '书::作者::/b/1::https://a.com'

    // 搜索前的旧缓存：只有 1 条（本书自己），按设计不写入 —— 这里先模拟
    // 「之前搜到过 2 个源」的旧状态。
    cache.set(key, SIG_AB, [bookOf('https://a.com'), bookOf('https://b.com')], true)

    // ── 新一轮搜索，逐批到达 ──
    const received: ReturnType<typeof bookOf>[] = []
    const onBatch = (batch: ReturnType<typeof bookOf>[]) => {
      received.push(...batch)
      cache.set(key, SIG_AB, [...received], false) // onmessage 里的 commit
    }

    onBatch([bookOf('https://c.com')])
    // 第一批就已落盘：此刻模拟「用户直接关掉详情页」
    let entry = createSourceCandidateCache({ storage }).get(key)!
    expect(entry.books.map((b) => b.origin)).toEqual(['https://c.com'])
    expect(entry.complete).toBe(false)

    onBatch([bookOf('https://d.com')])
    entry = createSourceCandidateCache({ storage }).get(key)!
    expect(entry.books).toHaveLength(2)
    expect(entry.complete).toBe(false)

    // 存储里确实有内容（不是只改了内存）
    expect(data.has('source-candidates-cache')).toBe(true)
  })

  it('a finished run overwrites the provisional entry with the authoritative one', () => {
    // 中途写的是「未完成」，跑完后 `finish()` 用权威结果覆盖成 complete=true。
    const { storage } = memoryStorage()
    const cache = createSourceCandidateCache({ storage })
    const key = '书::作者::/b/1::https://a.com'

    cache.set(key, SIG_AB, [bookOf('https://a.com')], false)  // 中途
    cache.set(key, SIG_AB, [bookOf('https://a.com'), bookOf('https://b.com')], true) // 跑完

    const entry = cache.get(key)!
    expect(entry.complete).toBe(true)
    expect(entry.books).toHaveLength(2)
  })
})
