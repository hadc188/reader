import { describe, expect, it } from 'vitest'
import {
  DEFAULT_READER_BACKGROUND_OPACITY,
  normalizeReaderBackgroundOpacity,
} from './readerBackground'

describe('reader background opacity', () => {
  it('uses the default for invalid saved values', () => {
    expect(normalizeReaderBackgroundOpacity(undefined)).toBe(DEFAULT_READER_BACKGROUND_OPACITY)
    expect(normalizeReaderBackgroundOpacity('invalid')).toBe(DEFAULT_READER_BACKGROUND_OPACITY)
  })

  it('clamps opacity to the visible range', () => {
    expect(normalizeReaderBackgroundOpacity(-0.5)).toBe(0)
    expect(normalizeReaderBackgroundOpacity(0.65)).toBe(0.65)
    expect(normalizeReaderBackgroundOpacity(2)).toBe(1)
  })
})
