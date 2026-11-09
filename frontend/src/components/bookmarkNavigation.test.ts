import { describe, expect, it } from 'vitest'
import { normalizeBookmarkPosition } from './bookmarkNavigation'

describe('normalizeBookmarkPosition', () => {
  it('keeps fractional progress in the 0..1 form', () => {
    expect(normalizeBookmarkPosition(0.42)).toBe(0.42)
  })

  it('decodes Legado bookmark positions stored on a 0..10000 scale', () => {
    expect(normalizeBookmarkPosition(5000)).toBe(0.5)
  })

  it('clamps invalid positions to the chapter start or end', () => {
    expect(normalizeBookmarkPosition(undefined)).toBe(0)
    expect(normalizeBookmarkPosition(Number.NaN)).toBe(0)
    expect(normalizeBookmarkPosition(-2)).toBe(0)
    expect(normalizeBookmarkPosition(12000)).toBe(1)
  })
})
