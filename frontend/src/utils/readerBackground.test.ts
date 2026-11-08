import { describe, expect, it } from 'vitest'
import {
  DEFAULT_READER_BACKGROUND_OPACITY,
  DEFAULT_READER_PANEL_OPACITY,
  DEFAULT_READER_WINDOW_OPACITY,
  MAX_BACKGROUND_DIMENSION,
  READER_BACKGROUND_OPACITY_STEP,
  READER_PANEL_OPACITY_MIN,
  READER_WINDOW_OPACITY_MIN,
  backgroundOutputSize,
  backgroundUploadFileName,
  normalizeReaderBackgroundOpacity,
  normalizeReaderPanelOpacity,
  normalizeReaderWindowOpacity,
  percentToReaderOpacity,
  readerOpacityToPercent,
} from './readerBackground'

describe('reader panel opacity', () => {
  it('defaults to opaque so panels look as before', () => {
    expect(DEFAULT_READER_PANEL_OPACITY).toBe(1)
    expect(normalizeReaderPanelOpacity(undefined)).toBe(1)
    expect(normalizeReaderPanelOpacity('invalid')).toBe(1)
  })

  it('keeps a user-chosen translucency', () => {
    expect(normalizeReaderPanelOpacity(0.84)).toBe(0.84)
    expect(normalizeReaderPanelOpacity(1)).toBe(1)
  })

  it('refuses to go below the readable floor', () => {
    // 再低会让抽屉里的章节标题透出正文, 读不清。
    expect(normalizeReaderPanelOpacity(0.2)).toBe(READER_PANEL_OPACITY_MIN)
    expect(normalizeReaderPanelOpacity(0)).toBe(READER_PANEL_OPACITY_MIN)
    expect(normalizeReaderPanelOpacity(5)).toBe(1)
  })
})

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

  it('steps the slider by 1% so fine adjustments are possible', () => {
    expect(READER_BACKGROUND_OPACITY_STEP).toBe(0.01)
  })
})

describe('window opacity', () => {
  it('defaults to fully opaque so upgrading users see no change', () => {
    expect(DEFAULT_READER_WINDOW_OPACITY).toBe(1)
    expect(normalizeReaderWindowOpacity(undefined)).toBe(1)
    expect(normalizeReaderWindowOpacity('invalid')).toBe(1)
  })

  it('keeps a user-chosen translucency', () => {
    expect(normalizeReaderWindowOpacity(0.8)).toBe(0.8)
    expect(normalizeReaderWindowOpacity(1)).toBe(1)
  })

  it('clamps to the readable floor shared with the Rust command', () => {
    // 与 src/api/commands/window.rs 的 WINDOW_OPACITY_MIN 必须一致，
    // 否则滑块位置会与原生窗口的实际 alpha 脱节。
    expect(READER_WINDOW_OPACITY_MIN).toBe(0.1)
    expect(normalizeReaderWindowOpacity(0.1)).toBe(READER_WINDOW_OPACITY_MIN)
    expect(normalizeReaderWindowOpacity(0)).toBe(READER_WINDOW_OPACITY_MIN)
    expect(normalizeReaderWindowOpacity(2)).toBe(1)
  })
})

describe('opacity percentage input', () => {
  it('renders the stored opacity as a whole percentage', () => {
    expect(readerOpacityToPercent(0.35)).toBe(35)
    expect(readerOpacityToPercent(0.01)).toBe(1)
    expect(readerOpacityToPercent(1)).toBe(100)
    expect(readerOpacityToPercent(0)).toBe(0)
  })

  it('falls back to the default percentage for invalid stored values', () => {
    expect(readerOpacityToPercent(undefined)).toBe(
      Math.round(DEFAULT_READER_BACKGROUND_OPACITY * 100),
    )
  })

  it('parses typed percentages, including whitespace and a trailing unit', () => {
    expect(percentToReaderOpacity('42')).toBe(0.42)
    expect(percentToReaderOpacity(' 7 ')).toBe(0.07)
    expect(percentToReaderOpacity(0)).toBe(0)
    expect(percentToReaderOpacity(100)).toBe(1)
  })

  it('clamps out-of-range percentages instead of storing them', () => {
    expect(percentToReaderOpacity('-20')).toBe(0)
    expect(percentToReaderOpacity('250')).toBe(1)
  })

  it('returns null for non-numeric input so the caller keeps the old value', () => {
    // 关键: 用户误敲一个字符时背景不能突然跳回默认透明度。
    expect(percentToReaderOpacity('')).toBeNull()
    expect(percentToReaderOpacity('abc')).toBeNull()
    expect(percentToReaderOpacity(undefined)).toBeNull()
    expect(percentToReaderOpacity(Number.NaN)).toBeNull()
  })

  it('round-trips percentage → stored → percentage', () => {
    for (const percent of [0, 1, 17, 35, 50, 99, 100]) {
      expect(readerOpacityToPercent(percentToReaderOpacity(percent))).toBe(percent)
    }
  })
})

describe('background image output size', () => {
  it('keeps a 4K image at its original resolution', () => {
    // issue #20 第 1 条: 以前一律缩到 1600px 再压 0.58 质量 webp, 4K 图必然发糊。
    expect(backgroundOutputSize(3840, 2160)).toEqual({ width: 3840, height: 2160 })
    expect(backgroundOutputSize(1920, 1080)).toEqual({ width: 1920, height: 1080 })
  })

  it('only shrinks what the webview could not decode', () => {
    const size = backgroundOutputSize(20000, 10000)
    expect(size.width).toBe(MAX_BACKGROUND_DIMENSION)
    expect(size.height).toBe(MAX_BACKGROUND_DIMENSION / 2)
  })

  it('preserves the aspect ratio when shrinking', () => {
    const size = backgroundOutputSize(10000, 5000, 4000)
    expect(size).toEqual({ width: 4000, height: 2000 })
  })

  it('rejects images without usable dimensions', () => {
    expect(() => backgroundOutputSize(0, 0)).toThrow()
  })
})

describe('background upload file name', () => {
  it('rewrites the extension from the MIME type', () => {
    // 后端只收 png/jpg/jpeg/webp 扩展名, `.jfif` 这类会被拒。
    expect(backgroundUploadFileName('壁纸.jfif', 'image/jpeg')).toBe('壁纸.jpg')
    expect(backgroundUploadFileName('a.b.png', 'image/png')).toBe('a.b.png')
    expect(backgroundUploadFileName('shot.WEBP', 'image/webp')).toBe('shot.webp')
  })

  it('keeps a readable name when the file has none', () => {
    expect(backgroundUploadFileName('', 'image/jpeg')).toBe('background.jpg')
  })
})
