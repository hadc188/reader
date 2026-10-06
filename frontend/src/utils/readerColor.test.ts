import { describe, expect, it } from 'vitest'
import {
  normalizeReaderColor,
  resolveReaderBackground,
  resolveShellBackgroundImage,
  shouldUseDesktopBackground,
} from './readerColor'

describe('normalizeReaderColor', () => {
  it('accepts a six digit hex colour', () => {
    expect(normalizeReaderColor('#1A2B3C')).toBe('#1a2b3c')
  })

  it('expands the three digit shorthand', () => {
    expect(normalizeReaderColor('#abc')).toBe('#aabbcc')
  })

  it('treats empty input as follow-the-theme', () => {
    expect(normalizeReaderColor('')).toBe('')
    expect(normalizeReaderColor('   ')).toBe('')
  })

  it('rejects anything that could not be written into a style safely', () => {
    expect(normalizeReaderColor('red')).toBe('')
    expect(normalizeReaderColor('rgb(1,2,3)')).toBe('')
    expect(normalizeReaderColor('#12345')).toBe('')
    expect(normalizeReaderColor('url(1.png)')).toBe('')
    expect(normalizeReaderColor('var(--x)')).toBe('')
    expect(normalizeReaderColor('expression(alert(1))')).toBe('')
  })

  it('rejects non-string values', () => {
    expect(normalizeReaderColor(null)).toBe('')
    expect(normalizeReaderColor(undefined)).toBe('')
    expect(normalizeReaderColor(123)).toBe('')
    expect(normalizeReaderColor({})).toBe('')
  })
})

describe('resolveReaderBackground', () => {
  const image = 'data:image/webp;base64,dGVzdA=='
  const base = {
    customBackgroundColor: '',
    readerBackgroundImage: '',
    backgroundImage: '',
    applyBackgroundToReader: true,
  }

  it('follows the theme when nothing is configured', () => {
    expect(resolveReaderBackground(base)).toBe('theme')
  })

  it('prefers the reader-specific image above everything else', () => {
    expect(resolveReaderBackground({
      ...base,
      readerBackgroundImage: image,
      customBackgroundColor: '#112233',
      backgroundImage: image,
    })).toBe('readerImage')
  })

  it('prefers a custom colour over the desktop image', () => {
    expect(resolveReaderBackground({
      ...base,
      customBackgroundColor: '#112233',
      backgroundImage: image,
    })).toBe('readerColor')
  })

  it('falls back to the desktop image when the reader has no own background', () => {
    expect(resolveReaderBackground({ ...base, backgroundImage: image })).toBe('desktopImage')
  })

  it('returns to the theme when the desktop image is not applied to the reader', () => {
    expect(resolveReaderBackground({
      ...base,
      backgroundImage: image,
      applyBackgroundToReader: false,
    })).toBe('theme')
  })
})

describe('shouldUseDesktopBackground', () => {
  const image = 'data:image/webp;base64,dGVzdA=='
  const base = {
    customBackgroundColor: '',
    readerBackgroundImage: '',
    backgroundImage: image,
    applyBackgroundToReader: true,
  }

  it('shows the desktop image when it is the only background source', () => {
    expect(shouldUseDesktopBackground(base)).toBe(true)
  })

  it('lets a custom reader background win over the desktop image', () => {
    // 这是诉求 2 的核心: 自定义阅读页背景必须不被桌面图盖住。
    expect(shouldUseDesktopBackground({ ...base, customBackgroundColor: '#112233' })).toBe(false)
    expect(shouldUseDesktopBackground({ ...base, readerBackgroundImage: image })).toBe(false)
  })

  it('respects the apply-to-reader switch', () => {
    expect(shouldUseDesktopBackground({ ...base, applyBackgroundToReader: false })).toBe(false)
  })

  it('falls back to the theme when no image is configured', () => {
    expect(shouldUseDesktopBackground({ ...base, backgroundImage: '' })).toBe(false)
  })
})

describe('resolveShellBackgroundImage (外壳最底层铺哪张图)', () => {
  const desktop = 'https://example.com/desktop.jpg'
  const readerImg = 'https://example.com/reader.jpg'
  const base = {
    customBackgroundColor: '',
    readerBackgroundImage: '',
    backgroundImage: desktop,
    applyBackgroundToReader: true,
  }

  it('shows the desktop image on non-reader pages', () => {
    // 书架/设置等页面: 有桌面图就铺。
    expect(resolveShellBackgroundImage(base, false)).toBe(desktop)
  })

  it('still shows the desktop image on the shelf when the reader is opted out', () => {
    // 回归测试: 「应用到阅读页」关掉后, 桌面自己的背景图必须照常显示。
    // 早先对所有页面套用阅读页优先级, 于是关掉开关后桌面背景也变空 ——
    // 用户看到的是「必须进一次阅读页才看得到背景图」。
    expect(resolveShellBackgroundImage({ ...base, applyBackgroundToReader: false }, false)).toBe(desktop)
  })

  it('keeps the reader on its theme when the reader is opted out', () => {
    // 该开关的语义: 阅读页不用桌面图, 让位给阅读主题。
    expect(resolveShellBackgroundImage({ ...base, applyBackgroundToReader: false }, true)).toBe('')
  })

  it('prefers the reader-only image while on the reader page', () => {
    expect(resolveShellBackgroundImage({ ...base, readerBackgroundImage: readerImg }, true)).toBe(readerImg)
  })

  it('uses the desktop image on the reader page only when applied', () => {
    expect(resolveShellBackgroundImage(base, true)).toBe(desktop)
    expect(resolveShellBackgroundImage({ ...base, backgroundImage: '' }, true)).toBe('')
    expect(resolveShellBackgroundImage({ ...base, backgroundImage: '' }, false)).toBe('')
  })

  it('ignores the reader-only image and reader colour outside the reader', () => {
    // 外壳页铺的是桌面图, 不该被阅读页专属设置影响。
    const input = { ...base, readerBackgroundImage: readerImg, customBackgroundColor: '#112233' }
    expect(resolveShellBackgroundImage(input, false)).toBe(desktop)
  })

  it('still hides the desktop image on the reader when a reader colour is set', () => {
    // 自定义阅读色优先于桌面图 —— 否则「单独自定义阅读页背景」会被桌面图盖住。
    expect(resolveShellBackgroundImage({ ...base, customBackgroundColor: '#112233' }, true)).toBe('')
  })
})
