import { describe, expect, it } from 'vitest'
import {
  READER_FONT_SIZE_MAX,
  READER_FONT_SIZE_MIN,
  getReaderFontSizeFromWheel,
  handleReaderFontSizeWheel,
} from './readerFontSize'

describe('readerFontSize', () => {
  it('increases and decreases the font size with Ctrl and the mouse wheel', () => {
    expect(getReaderFontSizeFromWheel(18, { ctrlKey: true, deltaY: -100 })).toBe(19)
    expect(getReaderFontSizeFromWheel(18, { ctrlKey: true, deltaY: 100 })).toBe(17)
  })

  it('ignores ordinary scrolling and empty wheel movement', () => {
    expect(getReaderFontSizeFromWheel(18, { ctrlKey: false, deltaY: -100 })).toBeNull()
    expect(getReaderFontSizeFromWheel(18, { ctrlKey: true, deltaY: 0 })).toBeNull()
  })

  it('keeps the configured font size within the existing limits', () => {
    expect(getReaderFontSizeFromWheel(READER_FONT_SIZE_MAX, { ctrlKey: true, deltaY: -100 }))
      .toBe(READER_FONT_SIZE_MAX)
    expect(getReaderFontSizeFromWheel(READER_FONT_SIZE_MIN, { ctrlKey: true, deltaY: 100 }))
      .toBe(READER_FONT_SIZE_MIN)
  })

  it('prevents browser zoom and persists a Ctrl-wheel font change', () => {
    let prevented = false
    let updatedSize = 0

    const handled = handleReaderFontSizeWheel(18, {
      ctrlKey: true,
      deltaY: -100,
      preventDefault: () => { prevented = true },
    }, (fontSize) => { updatedSize = fontSize })

    expect(handled).toBe(true)
    expect(prevented).toBe(true)
    expect(updatedSize).toBe(19)
  })

  it('leaves ordinary scrolling untouched', () => {
    let prevented = false

    const handled = handleReaderFontSizeWheel(18, {
      ctrlKey: false,
      deltaY: -100,
      preventDefault: () => { prevented = true },
    }, () => undefined)

    expect(handled).toBe(false)
    expect(prevented).toBe(false)
  })
})
