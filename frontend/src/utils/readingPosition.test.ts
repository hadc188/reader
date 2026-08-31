import { describe, expect, it } from 'vitest'
import { shouldPreferServerReadingPosition } from './readingPosition'

describe('readingPosition', () => {
  it('prefers a different server position even when its timestamp is older', () => {
    const local = { chapterIndex: 47, progress: 0, updatedAt: 2000 }
    const server = { chapterIndex: 47, progress: 0.6, updatedAt: 1000 }

    expect(shouldPreferServerReadingPosition(local, server)).toBe(true)
  })

  it('keeps the local paragraph-level position when both positions agree', () => {
    const local = { chapterIndex: 47, progress: 0.61, paragraphIndex: 8, updatedAt: 2000 }
    const server = { chapterIndex: 47, progress: 0.6, updatedAt: 3000 }

    expect(shouldPreferServerReadingPosition(local, server)).toBe(false)
  })
})
