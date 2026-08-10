import { describe, expect, it } from 'vitest'
import { getSpeechProgressItemIndex } from './useReaderAutoPlayback'

describe('merged speech paragraph progress', () => {
  it('moves the active paragraph according to each paragraph text length', () => {
    const lengths = [20, 60, 20]

    expect(getSpeechProgressItemIndex(lengths, 0)).toBe(0)
    expect(getSpeechProgressItemIndex(lengths, 0.19)).toBe(0)
    expect(getSpeechProgressItemIndex(lengths, 0.2)).toBe(1)
    expect(getSpeechProgressItemIndex(lengths, 0.79)).toBe(1)
    expect(getSpeechProgressItemIndex(lengths, 0.8)).toBe(2)
    expect(getSpeechProgressItemIndex(lengths, 1)).toBe(2)
  })
})
