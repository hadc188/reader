import { describe, expect, it } from 'vitest'
import {
  SEARCH_CONCURRENCY_DEFAULT,
  SEARCH_CONCURRENCY_MAX,
  SEARCH_CONCURRENCY_MIN,
  normalizeSearchConcurrency,
} from './searchConcurrency'

describe('normalizeSearchConcurrency', () => {
  it('uses the Reader default when no valid value is provided', () => {
    expect(normalizeSearchConcurrency(undefined)).toBe(SEARCH_CONCURRENCY_DEFAULT)
    expect(normalizeSearchConcurrency('')).toBe(SEARCH_CONCURRENCY_DEFAULT)
    expect(normalizeSearchConcurrency('not-a-number')).toBe(SEARCH_CONCURRENCY_DEFAULT)
  })

  it('clamps values to the phone-side range', () => {
    expect(normalizeSearchConcurrency(0)).toBe(SEARCH_CONCURRENCY_MIN)
    expect(normalizeSearchConcurrency(-3)).toBe(SEARCH_CONCURRENCY_MIN)
    expect(normalizeSearchConcurrency(1000)).toBe(SEARCH_CONCURRENCY_MAX)
  })

  it('truncates fractional values and reads numeric strings', () => {
    expect(normalizeSearchConcurrency(12.9)).toBe(12)
    expect(normalizeSearchConcurrency('24')).toBe(24)
  })
})
