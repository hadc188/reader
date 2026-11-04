import { describe, expect, it } from 'vitest'
import {
  createSourceCandidateCache,
  sourceCatalogSignatureOf,
} from './sourceCandidateCache'
import type { SearchBook } from '../types'

/**
 * 「书源增删/停启后，打开详情页要自动重搜」的回归。
 *
 * 这个行为依赖一个很容易被误伤的约定：**不要整份清掉候选缓存**。
 * 详情页判断方式是
 *   cached = cache.get(bookKey)
 *   catalogChanged = !!cached && !cache.matches(cached, 当前签名)
 * 一旦缓存被 `clear()`，`cached` 就是 undefined，`catalogChanged` 恒为 false ——
 * 打开详情页看到的是旧的候选、而且不会自动刷新。
 *
 * 上一轮「清理临时缓存」时我在 `source.ts` 的 `fetchSources()` 里加了
 * `getSourceCandidateCache().clear()`，正好踩中这条，用户实测「启动/停止书源后
 * 再打开详情页加载的是旧缓存」。这组用例把契约钉死。
 */

function source(url: string, enabled = true) {
  return { bookSourceUrl: url, enabled }
}

function book(name: string): SearchBook {
  return { name, author: '作者', bookUrl: `/b/${name}`, origin: 'https://a.com' }
}

describe('书源清单变化 → 详情页自动重搜', () => {
  it('detects a newly enabled source', () => {
    const cache = createSourceCandidateCache({ storage: null })
    const before = sourceCatalogSignatureOf([source('https://a.com'), source('https://b.com', false)])
    const after = sourceCatalogSignatureOf([source('https://a.com'), source('https://b.com')])

    cache.set('书::作者::/b/1::https://a.com', before, [book('书')])

    const cached = cache.get('书::作者::/b/1::https://a.com')!
    // 这正是详情页的判据；为 true 才会自动重搜。
    expect(cache.matches(cached, after)).toBe(false)
  })

  it('detects a disabled source', () => {
    const cache = createSourceCandidateCache({ storage: null })
    const before = sourceCatalogSignatureOf([source('https://a.com'), source('https://b.com')])
    const after = sourceCatalogSignatureOf([source('https://a.com'), source('https://b.com', false)])

    cache.set('k', before, [book('书')])
    expect(cache.matches(cache.get('k')!, after)).toBe(false)
  })

  it('detects an added or removed source', () => {
    const cache = createSourceCandidateCache({ storage: null })
    const before = sourceCatalogSignatureOf([source('https://a.com')])
    const added = sourceCatalogSignatureOf([source('https://a.com'), source('https://c.com')])
    const removed = sourceCatalogSignatureOf([])

    cache.set('k', before, [book('书')])
    expect(cache.matches(cache.get('k')!, added)).toBe(false)
    expect(cache.matches(cache.get('k')!, removed)).toBe(false)
  })

  it('does not fire when the catalog is unchanged', () => {
    // 幂等：同样的清单算出的签名必须稳定，否则每次打开详情页都会白搜一遍。
    const cache = createSourceCandidateCache({ storage: null })
    const sig = sourceCatalogSignatureOf([source('https://a.com'), source('https://b.com')])
    cache.set('k', sig, [book('书')])
    expect(cache.matches(cache.get('k')!, sig)).toBe(true)
  })

  it('ignores trailing slashes and ordering so trivial differences do not trigger', () => {
    const a = sourceCatalogSignatureOf([source('https://a.com/'), source('https://b.com')])
    const b = sourceCatalogSignatureOf([source('https://b.com'), source('https://a.com')])
    expect(a).toBe(b)
  })

  it('clear() destroys the signal — which is why fetchSources must not call it', () => {
    // 直接演示「清空 = 失去判断依据」。这条用例存在的意义是让以后想再往
    // fetchSources 里加 clear() 的人先看到失败原因。
    const cache = createSourceCandidateCache({ storage: null })
    const before = sourceCatalogSignatureOf([source('https://a.com')])
    const after = sourceCatalogSignatureOf([source('https://a.com'), source('https://b.com')])

    cache.set('k', before, [book('书')])
    cache.clear()

    const cached = cache.get('k')
    // 清空后 cached 为 undefined，详情页的 `!!cached && ...` 直接短路成 false
    // → 判不出「书源变了」→ 不重搜。所以书源变化时**不能**清缓存。
    expect(cached).toBeUndefined()
    expect(Boolean(cached) && !cache.matches(cached!, after)).toBe(false)
  })

  it('keeps the signal when the entry is overwritten by a fresh search', () => {
    // 正常路径：重搜完成后用**新签名**覆盖，之后再打开就是「清单没变」。
    const cache = createSourceCandidateCache({ storage: null })
    const after = sourceCatalogSignatureOf([source('https://a.com'), source('https://b.com')])
    cache.set('k', after, [book('书'), book('书2')])

    const cached = cache.get('k')!
    expect(cache.matches(cached, after)).toBe(true)
    expect(cached.books).toHaveLength(2)
  })
})
