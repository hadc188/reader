import { describe, expect, it } from 'vitest'
import { shouldResearchForSourceChange } from './searchSourceChange'

describe('shouldResearchForSourceChange', () => {
  it('does not re-search when the source list is first loaded on mount', () => {
    // 回归：这是搜索缓存被抵消的主因 —— onMounted 每次都 fetchSources(true)，
    // 首次挂载签名从空串变完整清单，若无条件重搜，刚命中缓存的页面立刻又搜一遍。
    expect(shouldResearchForSourceChange({
      hasSearchKey: true,
      previous: '',
      next: 'https://a.com\nhttps://b.com',
      baseline: '',
    })).toBe(false)
  })

  it('does not re-search when refetching the same list', () => {
    const sig = 'https://a.com\nhttps://b.com'
    expect(shouldResearchForSourceChange({
      hasSearchKey: true,
      previous: sig,
      next: sig,
      baseline: sig,
    })).toBe(false)
  })

  it('does not re-search when the new list matches the mount baseline', () => {
    const baseline = 'https://a.com'
    expect(shouldResearchForSourceChange({
      hasSearchKey: true,
      previous: 'https://a.com\nhttps://b.com',
      next: baseline,
      baseline,
    })).toBe(false)
  })

  it('re-searches when a source is added', () => {
    expect(shouldResearchForSourceChange({
      hasSearchKey: true,
      previous: 'https://a.com',
      next: 'https://a.com\nhttps://b.com',
      baseline: 'https://a.com',
    })).toBe(true)
  })

  it('re-searches when a source is removed or disabled', () => {
    expect(shouldResearchForSourceChange({
      hasSearchKey: true,
      previous: 'https://a.com\nhttps://b.com',
      next: 'https://a.com',
      baseline: 'https://a.com\nhttps://b.com',
    })).toBe(true)
  })

  it('never re-searches without an active keyword', () => {
    expect(shouldResearchForSourceChange({
      hasSearchKey: false,
      previous: 'https://a.com',
      next: 'https://a.com\nhttps://b.com',
      baseline: 'https://a.com',
    })).toBe(false)
  })

  it('ignores an empty next signature (source list not loaded yet)', () => {
    expect(shouldResearchForSourceChange({
      hasSearchKey: true,
      previous: 'https://a.com',
      next: '',
      baseline: 'https://a.com',
    })).toBe(false)
  })
})
