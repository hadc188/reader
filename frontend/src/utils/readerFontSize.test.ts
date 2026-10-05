import { describe, expect, it } from 'vitest'
import {
  READER_FONT_SIZE_MAX,
  READER_FONT_SIZE_MIN,
  READER_FONT_WEIGHT_MAX,
  READER_FONT_WEIGHT_MIN,
  getReaderFontSizeFromWheel,
  handleReaderFontSizeWheel,
  stepReaderFontSize,
  stepReaderFontWeight,
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

// 迷你模式的字号/字重加减走这两个函数, 边界必须与设置面板一致。
describe('stepReaderFontSize', () => {
  it('adds and subtracts by the given delta', () => {
    expect(stepReaderFontSize(18, 1)).toBe(19)
    expect(stepReaderFontSize(18, -1)).toBe(17)
  })

  it('clamps at both ends instead of running past them', () => {
    expect(stepReaderFontSize(READER_FONT_SIZE_MAX, 1)).toBe(READER_FONT_SIZE_MAX)
    expect(stepReaderFontSize(READER_FONT_SIZE_MIN, -1)).toBe(READER_FONT_SIZE_MIN)
  })

  it('clamps oversized jumps in one call', () => {
    expect(stepReaderFontSize(READER_FONT_SIZE_MIN, 999)).toBe(READER_FONT_SIZE_MAX)
    expect(stepReaderFontSize(READER_FONT_SIZE_MAX, -999)).toBe(READER_FONT_SIZE_MIN)
  })

  it('falls back to the minimum for non-finite input', () => {
    expect(stepReaderFontSize(Number.NaN, 1)).toBe(READER_FONT_SIZE_MIN)
    expect(stepReaderFontSize(18, Number.NaN)).toBe(READER_FONT_SIZE_MIN)
  })
})

describe('stepReaderFontWeight', () => {
  it('adds and subtracts by the 100 step used by the settings slider', () => {
    expect(stepReaderFontWeight(400, 100)).toBe(500)
    expect(stepReaderFontWeight(400, -100)).toBe(300)
  })

  it('clamps at 100 and 900', () => {
    expect(stepReaderFontWeight(READER_FONT_WEIGHT_MAX, 100)).toBe(READER_FONT_WEIGHT_MAX)
    expect(stepReaderFontWeight(READER_FONT_WEIGHT_MIN, -100)).toBe(READER_FONT_WEIGHT_MIN)
    expect(stepReaderFontWeight(400, 9999)).toBe(READER_FONT_WEIGHT_MAX)
  })

  it('falls back to the minimum for non-finite input', () => {
    expect(stepReaderFontWeight(Number.NaN, 100)).toBe(READER_FONT_WEIGHT_MIN)
  })
})
