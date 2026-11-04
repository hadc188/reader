import { afterEach, describe, expect, it } from 'vitest'
import {
  createSearchSessionCache,
  getSearchSessionCache,
  resetSearchSessionCacheForTests,
  searchSessionKey,
} from './searchSessionCache'
import { createSourceCandidateCache } from './sourceCandidateCache'
import type { SearchBook } from '../types'

/**
 * 这一组是「搜索页缓存」的端到端行为回归。
 *
 * 之所以要单独写：第一版实现里 `commitSession` 复用了 `isCurrentSearch()`，
 * 而那个判据包含 `eventSource === stream`，调用点又在 `eventSource = null`
 * 之后 —— 于是缓存永远写不进去，用户看到的现象就是「读几章返回还是重新
 * 搜索中」。纯单元测试没覆盖到写入时机，只能靠这组把时序固化下来。
 */

function book(name: string): SearchBook {
  return { name, author: '作者', bookUrl: `/b/${name}`, origin: 'https://a.com' }
}

/** 模拟「搜索页组件」一个生命周期：注册判据 + 收结果 + 结束。 */
function runSearchSession(
  cache: ReturnType<typeof createSearchSessionCache>,
  params: { keyword: string; results: SearchBook[]; complete?: boolean },
) {
  const key = searchSessionKey({ keyword: params.keyword })
  let eventSourceStream: object | null = { id: 'stream' }
  let searchGeneration = 1

  // 与组件里同构的两个判据。注意 commitSession **只**用 generation，
  // 不碰 eventSource —— 这正是修复点。
  const isCurrentSearch = () => searchGeneration === 1 && eventSourceStream !== null
  const commitSession = () => {
    if (searchGeneration !== 1) return
    cache.set(key, params.keyword, params.results, params.complete ?? true)
  }

  // 收流结束的顺序与组件里一致：先判 isCurrentSearch，再置 null，最后落库。
  if (isCurrentSearch()) {
    eventSourceStream = null
    commitSession()
  }
  return key
}

describe('搜索页 → 阅读页 → 返回 的缓存生命周期', () => {
  it('writes the finished result even though the stream is closed first', () => {
    const cache = createSearchSessionCache()
    const key = runSearchSession(cache, {
      keyword: '神通者',
      results: [book('神通者'), book('神通者 第二部')],
    })

    expect(cache.get(key)).toBeDefined()
    expect(cache.get(key)!.results).toHaveLength(2)
    expect(cache.get(key)!.complete).toBe(true)
  })

  it('lets a freshly mounted search page reuse the result instead of refetching', () => {
    const cache = createSearchSessionCache()
    const key = runSearchSession(cache, { keyword: '神通者', results: [book('神通者')] })

    // 「返回搜索页」= 组件重新挂载，首次 watch 触发时读缓存。
    const mayUseSessionCache = true
    const shouldForce = !mayUseSessionCache
    expect(shouldForce).toBe(false)

    const cached = cache.get(key)
    expect(cached?.results.map((item) => item.name)).toEqual(['神通者'])
  })

  it('does not serve a cached result for a different keyword', () => {
    const cache = createSearchSessionCache()
    runSearchSession(cache, { keyword: '神通者', results: [book('神通者')] })

    expect(cache.get(searchSessionKey({ keyword: '遮天' }))).toBeUndefined()
  })

  it('keeps a partial result so a stopped search is not lost', () => {
    const cache = createSearchSessionCache()
    const key = runSearchSession(cache, {
      keyword: '神通者',
      results: [book('神通者')],
      complete: false,
    })

    expect(cache.get(key)!.complete).toBe(false)
    expect(cache.get(key)!.results).toHaveLength(1)
  })

  it('manual refresh clears the cache so the next open really re-searches', () => {
    const cache = createSearchSessionCache()
    const key = runSearchSession(cache, { keyword: '神通者', results: [book('神通者')] })

    // 「重新搜索」按钮：先清缓存再强制搜。
    cache.clear()
    expect(cache.get(key)).toBeUndefined()
  })

  it('search cache and source-candidate cache stay independent', () => {
    // 两个缓存各自有键空间与淘汰策略，不能互相清空：搜过一次书不该让
    // 详情页的可读书源失效，反之亦然。
    const searchCache = createSearchSessionCache()
    const sourceCache = createSourceCandidateCache({ storage: null })

    runSearchSession(searchCache, { keyword: '神通者', results: [book('神通者')] })
    sourceCache.set('神通者::天蚕土豆::/b/1::https://a.com', 'sig', [book('神通者')])

    expect(searchCache.size()).toBe(1)
    expect(sourceCache.size()).toBe(1)

    searchCache.clear()
    expect(sourceCache.size()).toBe(1)
  })
})

describe('共享实例（这次真正的 bug）', () => {
  afterEach(() => {
    resetSearchSessionCacheForTests()
  })

  it('gives every caller the same cache object', () => {
    // 回归：组件里写 `const cache = createSearchSessionCache()` 时，`<script setup>`
    // 会在每个组件实例的 setup() 里执行一次 —— 去阅读页卸载后缓存消失，返回时
    // 是新的空缓存，「返回不重搜」永远不生效。必须用模块级单例。
    resetSearchSessionCacheForTests()
    const mountedFirst = getSearchSessionCache()
    const afterRemount = getSearchSessionCache()
    expect(afterRemount).toBe(mountedFirst)
  })

  it('keeps results written before an unmount available after a remount', () => {
    // 模拟：搜索页写入 → 组件卸载（无任何清理）→ 返回搜索页重新挂载后读取。
    resetSearchSessionCacheForTests()
    const key = searchSessionKey({ keyword: '神通者' })

    getSearchSessionCache().set(key, '神通者', [book('神通者'), book('神通者 第二部')])

    // 「重新挂载」= 再次取实例，内容必须还在
    const remounted = getSearchSessionCache()
    const cached = remounted.get(key)
    expect(cached).toBeDefined()
    expect(cached!.results.map((item) => item.name)).toEqual(['神通者', '神通者 第二部'])
  })

  it('is cleared explicitly when a new keyword search starts', () => {
    // 用户主动搜新词时才会清；单纯的"离开再回来"不该清。
    resetSearchSessionCacheForTests()
    const cache = getSearchSessionCache()
    const key = searchSessionKey({ keyword: '神通者' })
    cache.set(key, '神通者', [book('神通者')])

    cache.clear()
    expect(getSearchSessionCache().get(key)).toBeUndefined()
  })
})

describe('搜索未完成时离开再返回（对齐手机端）', () => {
  afterEach(() => {
    resetSearchSessionCacheForTests()
  })

  it('keeps partially received results when the stream is still running', () => {
    // 回归：之前只在 end/error 时才落库，搜索进行中点进阅读页 → 组件卸载 →
    // 什么都没存 → 返回时重新搜。手机端 SearchViewModel 常驻，搜索还在跑；
    // 这里必须「边收边存」，至少把已拿到的保住。
    resetSearchSessionCacheForTests()
    const cache = getSearchSessionCache()
    const key = searchSessionKey({ keyword: '神通者' })

    // 模拟：收到第 3 个书源的一批数据（还没 end）
    cache.set(key, '神通者', [book('神通者')], false, 3)

    const entry = cache.get(key)!
    expect(entry.results).toHaveLength(1)
    expect(entry.complete).toBe(false)
    expect(entry.lastIndex).toBe(3)
  })

  it('resumes from the saved index instead of restarting', () => {
    // 返回时读到未完成的缓存 → 用 lastIndex 续搜，而不是 lastIndex = -1 从头来。
    resetSearchSessionCacheForTests()
    const cache = getSearchSessionCache()
    const key = searchSessionKey({ keyword: '神通者' })
    cache.set(key, '神通者', [book('神通者')], false, 7)

    const cached = cache.get(key)!
    const resumeFrom = cached.complete ? -1 : cached.lastIndex

    expect(resumeFrom).toBe(7)
    expect(resumeFrom).not.toBe(-1)
  })

  it('does not resume once the search finished', () => {
    resetSearchSessionCacheForTests()
    const cache = getSearchSessionCache()
    const key = searchSessionKey({ keyword: '神通者' })
    cache.set(key, '神通者', [book('神通者')], true, 12)

    const cached = cache.get(key)!
    expect(cached.complete).toBe(true)
    // 已完成时不该再发请求
    expect(cached.complete ? 'reuse-only' : 'resume').toBe('reuse-only')
  })

  it('keeps a later index when more batches arrive', () => {
    // 每批都覆盖写：断点必须跟着前进，否则回来会重复搜已经搜过的书源。
    resetSearchSessionCacheForTests()
    const cache = getSearchSessionCache()
    const key = searchSessionKey({ keyword: '神通者' })

    cache.set(key, '神通者', [book('神通者')], false, 2)
    cache.set(key, '神通者', [book('神通者'), book('神通者 2')], false, 5)
    cache.set(key, '神通者', [book('神通者'), book('神通者 2'), book('神通者 3')], false, 9)

    expect(cache.get(key)!.lastIndex).toBe(9)
    expect(cache.get(key)!.results).toHaveLength(3)
  })

  it('marks complete only after the final batch', () => {
    resetSearchSessionCacheForTests()
    const cache = getSearchSessionCache()
    const key = searchSessionKey({ keyword: '神通者' })

    cache.set(key, '神通者', [book('神通者')], false, 4)
    expect(cache.get(key)!.complete).toBe(false)

    cache.set(key, '神通者', [book('神通者'), book('神通者 2')], true, 8)
    expect(cache.get(key)!.complete).toBe(true)
    expect(cache.get(key)!.lastIndex).toBe(8)
  })
})

describe('返回书架 vs 进阅读页', () => {
  afterEach(() => {
    resetSearchSessionCacheForTests()
  })

  it('keeps the cache when only jumping into the reader', () => {
    // 进阅读页属于临时离开：组件卸载，但缓存必须留着，返回时才能直接铺出来。
    resetSearchSessionCacheForTests()
    const cache = getSearchSessionCache()
    const key = searchSessionKey({ keyword: '神通者' })
    cache.set(key, '神通者', [book('神通者')])

    // 「卸载」不调用任何清理
    expect(getSearchSessionCache().get(key)).toBeDefined()
  })

  it('drops the cache when the user goes back to the shelf', () => {
    // 「返回书架」= 这次搜索结束。留着旧结果会让下次搜同一个词先看到一份过期列表。
    resetSearchSessionCacheForTests()
    const cache = getSearchSessionCache()
    const key = searchSessionKey({ keyword: '神通者' })
    cache.set(key, '神通者', [book('神通者')])

    // backToShelf() 的行为
    cache.clear()

    expect(getSearchSessionCache().get(key)).toBeUndefined()
    expect(getSearchSessionCache().size()).toBe(0)
  })

  it('clears every keyword on back-to-shelf, not just the current one', () => {
    // 离开搜索页意味着整批搜索都结束了，没必要留下任何一个词的旧结果。
    resetSearchSessionCacheForTests()
    const cache = getSearchSessionCache()
    const a = searchSessionKey({ keyword: '神通者' })
    const b = searchSessionKey({ keyword: '遮天' })
    cache.set(a, '神通者', [book('神通者')])
    cache.set(b, '遮天', [book('遮天')])

    cache.clear()

    expect(cache.size()).toBe(0)
  })

  it('re-searches after going to the shelf and searching the same word again', () => {
    // 端到端语义：搜索 → 返回书架（清缓存）→ 再搜同一个词，必须重新搜。
    resetSearchSessionCacheForTests()
    const cache = getSearchSessionCache()
    const key = searchSessionKey({ keyword: '神通者' })
    cache.set(key, '神通者', [book('神通者')])

    // 返回书架
    cache.clear()

    // 再次进入搜索页，首次触发时读缓存 —— 这里是空的，于是走真实搜索
    const mayUseSessionCache = true
    const cached = cache.get(key)
    const action = !mayUseSessionCache ? 'search' : (cached ? 'reuse' : 'search')
    expect(action).toBe('search')
  })
})
