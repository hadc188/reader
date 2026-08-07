import { describe, expect, it } from 'vitest'
import { isReaderInteractiveClickTarget } from './readerClick'

describe('readerClick', () => {
  it('treats reader controls as interactive so clicks do not trigger page turning', () => {
    const target = fakeTarget((selector) => selector.includes('button'))

    expect(isReaderInteractiveClickTarget(target)).toBe(true)
  })

  it('treats mobile controls as interactive so touch pagination does not steal toolbar taps', () => {
    const target = fakeTarget((selector) => selector.includes('.mobile-controls'))

    expect(isReaderInteractiveClickTarget(target)).toBe(true)
  })

  it('treats plain chapter text as non-interactive', () => {
    const target = fakeTarget(() => false)

    expect(isReaderInteractiveClickTarget(target)).toBe(false)
  })
})

function fakeTarget(matches: (selector: string) => boolean) {
  return {
    closest: (selector: string) => matches(selector) ? {} : null,
  }
}
