import { describe, expect, it } from 'vitest'
import {
  createSearchSessionCache,
  searchSessionKey,
} from './searchSessionCache'
import type { SearchBook } from '../types'

function book(name: string, origin = 'https://a.com'): SearchBook {
  return { name, author: '作者', bookUrl: `/book/${name}`, origin }
}

describe('searchSessionKey', () => {
  it('separates the same keyword under different scopes', () => {
    // 同一个词在「全部书源」和「单个书源」下是两批不同的结果，键必须不同。
    const all = searchSessionKey({ keyword: '遮天', scope: 'all' })
    const single = searchSessionKey({ keyword: '遮天', scope: 'source', sourceUrl: 'https://a.com' })
    const group = searchSessionKey({ keyword: '遮天', scope: 'group', group: '玄幻' })
    expect(new Set([all, single, group]).size).toBe(3)
  })

  it('separates different groups and sources', () => {
    const g1 = searchSessionKey({ keyword: '遮天', scope: 'group', group: '玄幻' })
    const g2 = searchSessionKey({ keyword: '遮天', scope: 'group', group: '都市' })
    const s1 = searchSessionKey({ keyword: '遮天', scope: 'source', sourceUrl: 'https://a.com' })
    const s2 = searchSessionKey({ keyword: '遮天', scope: 'source', sourceUrl: 'https://b.com' })
    expect(g1).not.toBe(g2)
    expect(s1).not.toBe(s2)
  })

  it('trims the keyword and treats case as significant-free only via trim', () => {
    expect(searchSessionKey({ keyword: '  遮天  ' }))
      .toBe(searchSessionKey({ keyword: '遮天' }))
  })

  it('returns an empty key for an empty keyword so nothing gets cached', () => {
    expect(searchSessionKey({ keyword: '   ' })).toBe('')
  })
})

describe('searchSessionCache', () => {
  it('returns the cached results for the same conditions', () => {
    const cache = createSearchSessionCache()
    const key = searchSessionKey({ keyword: '遮天' })
    cache.set(key, '遮天', [book('遮天')])

    expect(cache.get(key)?.results).toHaveLength(1)
  })

  it('does not hit for a different keyword or scope', () => {
    const cache = createSearchSessionCache()
    cache.set(searchSessionKey({ keyword: '遮天' }), '遮天', [book('遮天')])

    expect(cache.get(searchSessionKey({ keyword: '剑来' }))).toBeUndefined()
    expect(cache.get(searchSessionKey({
      keyword: '遮天', scope: 'source', sourceUrl: 'https://a.com',
    }))).toBeUndefined()
  })

  it('does not cache an empty result so a failed search is retried', () => {
    const cache = createSearchSessionCache()
    const key = searchSessionKey({ keyword: '遮天' })
    cache.set(key, '遮天', [])
    expect(cache.get(key)).toBeUndefined()
    expect(cache.size()).toBe(0)
  })

  it('keeps the incomplete flag so a stopped search can be resumed', () => {
    const cache = createSearchSessionCache()
    const key = searchSessionKey({ keyword: '遮天' })
    cache.set(key, '遮天', [book('遮天')], false)
    expect(cache.get(key)!.complete).toBe(false)
  })

  it('records the resume index alongside the incomplete flag', () => {
    // 断点续搜靠这个下标：回来时要从这里接着要后面的批次，而不是从头再来。
    const cache = createSearchSessionCache()
    const key = searchSessionKey({ keyword: '遮天' })
    cache.set(key, '遮天', [book('遮天')], false, 6)
    expect(cache.get(key)!.lastIndex).toBe(6)
  })

  it('defaults lastIndex to -1 when not given', () => {
    const cache = createSearchSessionCache()
    const key = searchSessionKey({ keyword: '遮天' })
    cache.set(key, '遮天', [book('遮天')])
    expect(cache.get(key)!.lastIndex).toBe(-1)
  })

  it('evicts least recently used entries past the limit', () => {
    const cache = createSearchSessionCache({ limit: 2 })
    const k1 = searchSessionKey({ keyword: '一' })
    const k2 = searchSessionKey({ keyword: '二' })
    const k3 = searchSessionKey({ keyword: '三' })
    cache.set(k1, '一', [book('一')])
    cache.set(k2, '二', [book('二')])
    cache.get(k1)
    cache.set(k3, '三', [book('三')])

    expect(cache.get(k1)).toBeDefined()
    expect(cache.get(k2)).toBeUndefined()
  })

  it('clear() drops everything', () => {
    const cache = createSearchSessionCache()
    cache.set(searchSessionKey({ keyword: '遮天' }), '遮天', [book('遮天')])
    cache.clear()
    expect(cache.size()).toBe(0)
  })

  it('delete() drops only one keyword, leaving the others', () => {
    // 点「重新搜索」只该丢当前这个词的结果，不该把用户其它搜索的缓存一起清了。
    const cache = createSearchSessionCache()
    const a = searchSessionKey({ keyword: '遮天' })
    const b = searchSessionKey({ keyword: '剑来' })
    cache.set(a, '遮天', [book('遮天')])
    cache.set(b, '剑来', [book('剑来')])

    cache.delete(a)

    expect(cache.get(a)).toBeUndefined()
    expect(cache.get(b)).toBeDefined()
  })

  it('snapshots the list so later mutation of the live array does not leak in', () => {
    // SearchResults 里的 results 是响应式数组，会持续被替换/追加；
    // 缓存必须拿一份独立副本，否则"缓存下来的结果"会跟着变。
    const cache = createSearchSessionCache()
    const key = searchSessionKey({ keyword: '遮天' })
    const live = [book('遮天')]
    cache.set(key, '遮天', live)
    live.push(book('遮天二'))

    expect(cache.get(key)!.results).toHaveLength(1)
  })
})

describe('搜索会话缓存的过期清理', () => {
  it('drops entries older than the TTL on read', () => {
    // 应用可以连开好几天，同一组条件的结果一直躺着就是过期信息（书下架、
    // 书源增删），不能当成「不用重搜」。
    let clock = 0
    const cache = createSearchSessionCache({ ttlMs: 1000, now: () => clock })
    const key = searchSessionKey({ keyword: '遮天' })
    cache.set(key, '遮天', [book('遮天')])

    clock = 900
    expect(cache.get(key)).toBeDefined()

    clock = 1500
    expect(cache.get(key)).toBeUndefined()
  })

  it('prunes expired entries when new results are stored', () => {
    // 过期记录不能一直占着 LRU 名额把新结果挤掉。
    let clock = 0
    const cache = createSearchSessionCache({ limit: 2, ttlMs: 1000, now: () => clock })
    const oldKey = searchSessionKey({ keyword: '一' })
    cache.set(oldKey, '一', [book('一')])

    clock = 2000
    cache.set(searchSessionKey({ keyword: '二' }), '二', [book('二')])

    expect(cache.get(oldKey)).toBeUndefined()
    expect(cache.size()).toBe(1)
  })

  it('reports staleness without deleting, like the source-candidate cache', () => {
    // 与 sourceCandidateCache 口径一致：过期只标记，由调用方决定丢弃还是照常显示。
    // 注意 `size()` 会先清过期项，所以这里不能拿它当「没删」的证据 ——
    // 真正要保证的是：调用方手上那条 entry 仍然完整可用。
    let clock = 0
    const cache = createSearchSessionCache({ ttlMs: 1000, now: () => clock })
    const key = searchSessionKey({ keyword: '遮天' })
    cache.set(key, '遮天', [book('遮天')], true, 3)

    const entry = cache.get(key)!
    clock = 2000
    expect(cache.isStale(entry)).toBe(true)
    // entry 本体没被破坏，调用方仍可把它摆到界面上
    expect(entry.results).toHaveLength(1)
    expect(entry.lastIndex).toBe(3)
  })

  it('keeps a finished search usable inside the TTL', () => {
    let clock = 0
    const cache = createSearchSessionCache({ ttlMs: 1000, now: () => clock })
    const key = searchSessionKey({ keyword: '遮天' })
    cache.set(key, '遮天', [book('遮天')], true, 5)

    clock = 999
    const entry = cache.get(key)!
    expect(entry.complete).toBe(true)
    expect(cache.isStale(entry)).toBe(false)
  })
})
