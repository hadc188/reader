import { describe, expect, it } from 'vitest'
import { isLocalBook, isLocalEpubBook, isLocalTxtBook } from './localBook'

describe('isLocalTxtBook', () => {
  it('detects uploaded local txt books by origin or url', () => {
    expect(isLocalTxtBook({ origin: 'local-txt', bookUrl: 'anything' })).toBe(true)
    expect(isLocalTxtBook({ origin: 'remote', bookUrl: 'local-txt:abc' })).toBe(true)
    expect(isLocalTxtBook({ origin: 'remote', bookUrl: 'https://example.test/book' })).toBe(false)
    expect(isLocalTxtBook(null)).toBe(false)
  })

  it('detects uploaded local epub books and all local books', () => {
    expect(isLocalEpubBook({ origin: 'local-epub', bookUrl: 'anything' })).toBe(true)
    expect(isLocalEpubBook({ origin: 'remote', bookUrl: 'local-epub:abc' })).toBe(true)
    expect(isLocalBook({ origin: 'local-txt', bookUrl: 'anything' })).toBe(true)
    expect(isLocalBook({ origin: 'remote', bookUrl: 'local-epub:abc' })).toBe(true)
    expect(isLocalBook({ origin: 'remote', bookUrl: 'https://example.test/book' })).toBe(false)
  })
})
