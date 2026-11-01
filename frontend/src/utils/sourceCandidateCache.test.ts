import { afterEach, describe, expect, it } from 'vitest'
import {
  DEFAULT_SOURCE_CANDIDATE_TTL_MS,
  SOURCE_CANDIDATE_STORAGE_KEY,
  createSourceCandidateCache,
  getSourceCandidateCache,
  mergeForCommit,
  resetSourceCandidateCacheForTests,
  sourceCandidateBookKey,
  sourceCatalogSignatureOf,
  type SourceCandidateStorage,
} from './sourceCandidateCache'
import type { SearchBook } from '../types'

function book(origin: string, bookUrl = '/1'): SearchBook {
  return { name: '书名', author: '作者', bookUrl, origin }
}

/** 用 origin 去重，等价于 searchRank 的 mergeSourceCandidates（简化版）。 */
function mergeByOrigin(a: readonly SearchBook[], b: readonly SearchBook[]): SearchBook[] {
  const map = new Map<string, SearchBook>()
  for (const item of [...a, ...b]) map.set(item.origin, item)
  return [...map.values()]
}

/** 内存版 localStorage：模拟「关掉应用再打开」。 */
function memoryStorage(seed: Record<string, string> = {}) {
  const data = new Map(Object.entries(seed))
  const storage: SourceCandidateStorage = {
    getItem: (key) => data.get(key) ?? null,
    setItem: (key, value) => { data.set(key, value) },
    removeItem: (key) => { data.delete(key) },
  }
  return { storage, data }
}

describe('sourceCandidateCache', () => {
  it('stores one entry per book and keeps the newest signature', () => {
    const cache = createSourceCandidateCache()
    cache.set('书A', 'a:1', [book('https://a.com')])

    // 同一本书用新签名再写一次：只剩一条，且是新签名
    cache.set('书A', 'a:1\nb:1', [book('https://a.com'), book('https://b.com')])

    expect(cache.size()).toBe(1)
    const entry = cache.get('书A')!
    expect(entry.books).toHaveLength(2)
    expect(cache.matches(entry, 'a:1\nb:1')).toBe(true)
    expect(cache.matches(entry, 'a:1')).toBe(false)
  })

  it('keeps the old entry when a refresh stores nothing new', () => {
    // 刷新时用户立刻按下停止 —— 一条新结果都没有，旧缓存必须原样留着。
    const cache = createSourceCandidateCache()
    cache.set('书A', 'sig', [book('https://a.com'), book('https://b.com')])
    cache.set('书A', 'sig', [])

    expect(cache.get('书A')!.books).toHaveLength(2)
  })

  it('replaces the snapshot only when the caller passes the full result', () => {
    // 刷新中断：调用方把「新旧并集」传进来，于是 8 个源被保留。
    const cache = createSourceCandidateCache()
    cache.set('书A', 'sig', [book('https://a.com')], true)
    cache.set('书A', 'sig', [
      book('https://a.com'),
      book('https://b.com'),
      book('https://c.com'),
    ], false)

    const entry = cache.get('书A')!
    expect(entry.books).toHaveLength(3)
    expect(entry.complete).toBe(false)
  })

  it('evicts least recently used entries past the limit', () => {
    const cache = createSourceCandidateCache({ limit: 2 })
    cache.set('k1', 'sig', [book('https://a.com')])
    cache.set('k2', 'sig', [book('https://a.com')])
    cache.set('k3', 'sig', [book('https://a.com')])

    expect(cache.size()).toBe(2)
    expect(cache.get('k1')).toBeUndefined()
    expect(cache.get('k3')).toBeDefined()
  })

  it('treats a read as recent so hot entries survive eviction', () => {
    const cache = createSourceCandidateCache({ limit: 2 })
    cache.set('k1', 'sig', [book('https://a.com')])
    cache.set('k2', 'sig', [book('https://a.com')])
    cache.get('k1')
    cache.set('k3', 'sig', [book('https://a.com')])

    expect(cache.get('k1')).toBeDefined()
    expect(cache.get('k2')).toBeUndefined()
  })

  it('reports staleness against the injected clock', () => {
    let clock = 1000
    const cache = createSourceCandidateCache({ ttlMs: 500, now: () => clock })
    cache.set('k', 'sig', [book('https://a.com')])

    const entry = cache.get('k')!
    expect(cache.isStale(entry)).toBe(false)

    clock += 501
    expect(cache.isStale(entry)).toBe(true)
  })

  it('clear() drops everything, which is how source edits invalidate the cache', () => {
    const cache = createSourceCandidateCache()
    cache.set('k1', 'sig', [book('https://a.com')])
    cache.set('k2', 'sig', [book('https://a.com')])
    cache.clear()
    expect(cache.size()).toBe(0)
  })

  it('defaults to a 30 minute ttl', () => {
    expect(DEFAULT_SOURCE_CANDIDATE_TTL_MS).toBe(30 * 60 * 1000)
  })

  it('survives an app restart through the storage backend', () => {
    // 回归：退出应用前刷出 6 个源，重开变回 3 个（书架里存的候选）。
    // 缓存原来只在内存里，重启即丢；现在写进 localStorage。
    const { storage } = memoryStorage()
    const six = ['a', 'b', 'c', 'd', 'e', 'f'].map((key) => book(`https://${key}.com`))

    const before = createSourceCandidateCache({ storage })
    before.set('书A', 'sig', six)
    expect(before.size()).toBe(1)

    // 模拟重启：新建一个实例，共用同一份存储
    const after = createSourceCandidateCache({ storage })
    const restored = after.get('书A')!
    expect(restored.books).toHaveLength(6)
    expect(restored.catalogSignature).toBe('sig')
    expect(restored.books.map((item) => item.origin)).toContain('https://f.com')
  })

  it('writes under the documented storage key', () => {
    const { storage, data } = memoryStorage()
    const cache = createSourceCandidateCache({ storage })
    cache.set('书A', 'sig', [book('https://a.com')])
    expect(data.has(SOURCE_CANDIDATE_STORAGE_KEY)).toBe(true)
  })

  it('ignores corrupted storage instead of throwing', () => {
    const { storage } = memoryStorage({ [SOURCE_CANDIDATE_STORAGE_KEY]: '{not json' })
    const cache = createSourceCandidateCache({ storage })
    expect(cache.size()).toBe(0)
  })

  it('drops records that do not look like entries', () => {
    const { storage } = memoryStorage({
      [SOURCE_CANDIDATE_STORAGE_KEY]: JSON.stringify({
        version: 1,
        entries: [
          { bookKey: 'ok', catalogSignature: 'sig', savedAt: 1, books: [book('https://a.com')] },
          { bookKey: 5, nonsense: true },
          null,
        ],
      }),
    })
    const cache = createSourceCandidateCache({ storage })
    expect(cache.size()).toBe(1)
    expect(cache.get('ok')!.books).toHaveLength(1)
  })

  it('clear() also wipes the storage entry', () => {
    const { storage, data } = memoryStorage()
    const cache = createSourceCandidateCache({ storage })
    cache.set('书A', 'sig', [book('https://a.com')])
    cache.clear()

    expect(data.has(SOURCE_CANDIDATE_STORAGE_KEY)).toBe(false)
    expect(createSourceCandidateCache({ storage }).size()).toBe(0)
  })

  it('works in memory when no storage is available', () => {
    const cache = createSourceCandidateCache({ storage: null })
    cache.set('书A', 'sig', [book('https://a.com')])
    expect(cache.get('书A')!.books).toHaveLength(1)
  })
})

describe('shared instance', () => {
  afterEach(() => {
    resetSourceCandidateCacheForTests()
  })

  it('hands the same object to the detail modal and the reader panel', () => {
    // 回归：两处各自 createSourceCandidateCache() 时，只有 localStorage 里的
    // 内容共享，会话内新写的对方看不见 —— 「详情页刷出 6 个源，进阅读页还是 3 个」。
    resetSourceCandidateCacheForTests()
    const first = getSourceCandidateCache()
    const second = getSourceCandidateCache()
    expect(second).toBe(first)

    const six = ['a', 'b', 'c', 'd', 'e', 'f'].map((key) => book(`https://${key}.com`))
    first.set('书A', 'sig', six)

    expect(second.get('书A')!.books).toHaveLength(6)
  })
})

describe('shared key helpers', () => {
  it('builds the same book key from any shape of book object', () => {
    const full = { name: '遮天', author: '辰东', bookUrl: '/b/1', origin: 'https://a.com' }
    expect(sourceCandidateBookKey(full)).toBe('遮天::辰东::/b/1::https://a.com')
    // author 缺失时也要稳定，不能出现 undefined 拼进键里
    expect(sourceCandidateBookKey({ ...full, author: undefined })).toBe('遮天::::/b/1::https://a.com')
  })

  it('signs the catalog by enabled sources only, ignoring order and trailing slashes', () => {
    const a = { bookSourceUrl: 'https://a.com/', enabled: true }
    const b = { bookSourceUrl: 'https://b.com', enabled: true }
    const disabled = { bookSourceUrl: 'https://c.com', enabled: false }

    const sig1 = sourceCatalogSignatureOf([a, b, disabled])
    const sig2 = sourceCatalogSignatureOf([{ ...b }, { ...a }])
    expect(sig1).toBe(sig2)
    expect(sig1).toBe('https://a.com\nhttps://b.com')
  })

  it('changes the signature when a source is added, removed or toggled', () => {
    const a = { bookSourceUrl: 'https://a.com', enabled: true }
    const b = { bookSourceUrl: 'https://b.com', enabled: true }
    const base = sourceCatalogSignatureOf([a, b])

    expect(sourceCatalogSignatureOf([a])).not.toBe(base)
    expect(sourceCatalogSignatureOf([a, b, { bookSourceUrl: 'https://c.com' }])).not.toBe(base)
    expect(sourceCatalogSignatureOf([a, { ...b, enabled: false }])).not.toBe(base)
  })
})

describe('mergeForCommit', () => {
  const original = [
    book('https://a.com'),
    book('https://b.com'),
    book('https://c.com'),
    book('https://d.com'),
  ]

  it('replaces the snapshot when the search completed', () => {
    const next = [book('https://a.com'), book('https://b.com')]
    expect(mergeForCommit(original, next, true, mergeByOrigin)).toEqual(next)
  })

  it('keeps both when the user stopped after finding more sources', () => {
    // 回归：刷新前 4 个源 → 刷新搜出 8 个 → 中途停止。
    // 旧实现只存本轮列表（或干脆不存），再打开退回 4 个。现在必须保存并集。
    const refreshed = [
      ...original,
      book('https://e.com'),
      book('https://f.com'),
      book('https://g.com'),
      book('https://h.com'),
    ]
    const merged = mergeForCommit(original, refreshed.slice(0, 6), false, mergeByOrigin)

    expect(merged).toHaveLength(6)
    expect(merged.map((item) => item.origin)).toEqual([
      'https://a.com',
      'https://b.com',
      'https://c.com',
      'https://d.com',
      'https://e.com',
      'https://f.com',
    ])
  })

  it('still keeps the old sources when the refresh returned nothing', () => {
    // 点刷新后立刻停止：本轮空，旧缓存不能因此丢掉。
    expect(mergeForCommit(original, [], false, mergeByOrigin)).toHaveLength(4)
  })

  it('lets a re-hit refresh the metadata of an existing source', () => {
    const updated = [{ ...book('https://a.com'), lastChapter: '第200章 新章' }]
    const merged = mergeForCommit(original, updated, false, mergeByOrigin)
    expect(merged.find((item) => item.origin === 'https://a.com')?.lastChapter)
      .toBe('第200章 新章')
  })
})
