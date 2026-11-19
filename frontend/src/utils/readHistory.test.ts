import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { clearReadHistory, readHistoryStorageKey } from './readHistory'

/** 内存版 localStorage：与真实浏览器行为一致的最小实现。 */
function createStorage() {
  const map = new Map<string, string>()
  return {
    getItem: (key: string) => map.get(key) ?? null,
    setItem: (key: string, value: string) => void map.set(key, value),
    removeItem: (key: string) => void map.delete(key),
    clear: () => map.clear(),
  }
}

describe('readHistory', () => {
  beforeEach(() => {
    vi.stubGlobal('localStorage', createStorage())
  })

  afterEach(() => {
    vi.unstubAllGlobals()
  })

  it('按书 URL 生成独立的存储键', () => {
    expect(readHistoryStorageKey('https://a.test/book/1')).toBe('reader-read-history:https://a.test/book/1')
    expect(readHistoryStorageKey('  https://a.test/book/1  ')).toBe('reader-read-history:https://a.test/book/1')
    expect(readHistoryStorageKey('')).toBe('')
    expect(readHistoryStorageKey(null)).toBe('')
  })

  it('删除书籍时清掉它的已读历史，其它书不受影响', () => {
    localStorage.setItem(readHistoryStorageKey('https://a.test/book/1'), '["k1"]')
    localStorage.setItem(readHistoryStorageKey('https://a.test/book/2'), '["k2"]')

    expect(clearReadHistory('https://a.test/book/1')).toBe(true)

    expect(localStorage.getItem(readHistoryStorageKey('https://a.test/book/1'))).toBeNull()
    expect(localStorage.getItem(readHistoryStorageKey('https://a.test/book/2'))).toBe('["k2"]')
  })

  it('没有记录时删除是空操作', () => {
    expect(clearReadHistory('https://a.test/book/3')).toBe(false)
    expect(clearReadHistory('')).toBe(false)
  })

  it('localStorage 抛错时不影响删书流程', () => {
    vi.stubGlobal('localStorage', {
      getItem: () => { throw new Error('quota') },
      setItem: () => { throw new Error('quota') },
      removeItem: () => { throw new Error('quota') },
      clear: () => { throw new Error('quota') },
    })
    expect(clearReadHistory('https://a.test/book/4')).toBe(false)
  })
})
