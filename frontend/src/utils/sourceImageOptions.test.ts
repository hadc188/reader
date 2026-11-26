import { describe, expect, it } from 'vitest'
import {
  ensureSvgNamespace,
  parseSourceImageOptions,
  processSourceImageOptions,
  reserveImageSpace,
  settleReaderImageLayout,
} from './sourceImageOptions'

// 取自真实插件(addComment)产生的 src: 段评图标 + 配置面板 click
const REAL_SRC =
  'data:image/svg+xml;base64,PHN2ZyB2aWV3Qm94PSIwIDAgMjQgMjQiPjwvc3ZnPg==,' +
  '{"style":"text","type":"qd","click":"showConfigPanel()"}'

/** 齿轮图标(打开控制面板的入口): 真实数据, 根标签缺少 xmlns。 */
const GEAR_SVG =
  '<svg viewBox="0 0 24 24" width="800" height="800" stroke="#757575" stroke-width="2" fill="none">' +
  '<circle cx="12" cy="12" r="3"></circle><path d="M19.4 15a1.65 1.65 0 0 0 .33 1.82z"></path></svg>'

/** 段评气泡图标: 真实数据, 自带 xmlns(所以它一直显示正常)。 */
const COMMENT_SVG =
  '<svg class="icon" viewBox="-150 0 1224 1224" version="1.1" xmlns="http://www.w3.org/2000/svg" width="800" height="800">' +
  '<path d="M512 938.667z"></path></svg>'

function toDataUri(svg: string) {
  const bytes = new TextEncoder().encode(svg)
  let binary = ''
  bytes.forEach((byte) => {
    binary += String.fromCharCode(byte)
  })
  return `data:image/svg+xml;base64,${btoa(binary)}`
}

function decodeDataUri(url: string) {
  const payload = url.slice(url.indexOf(';base64,') + ';base64,'.length)
  const binary = atob(payload)
  const bytes = Uint8Array.from(binary, (char) => char.charCodeAt(0))
  return new TextDecoder('utf-8').decode(bytes)
}

describe('parseSourceImageOptions', () => {
  it('剥离 ,{json} 后缀并取出 click（修复破损图标的关键）', () => {
    const parsed = parseSourceImageOptions(REAL_SRC)
    expect(parsed).not.toBeNull()
    // 剥离后的地址不能残留 JSON 部分，否则图片会 404 成破损图标
    expect(parsed!.url).toBe('data:image/svg+xml;base64,PHN2ZyB2aWV3Qm94PSIwIDAgMjQgMjQiPjwvc3ZnPg==')
    expect(parsed!.url).not.toContain('{')
    expect(parsed!.style).toBe('text')
    expect(parsed!.click).toBe('showConfigPanel()')
  })

  it('普通图片（无参数）原样返回 null', () => {
    expect(parseSourceImageOptions('https://cdn.example/a.jpg')).toBeNull()
    expect(parseSourceImageOptions('')).toBeNull()
  })

  it('JSON 非法时不剥离，避免误伤普通图片', () => {
    expect(parseSourceImageOptions('https://cdn.example/a.jpg,{bad json')).toBeNull()
  })

  it('逗号后不是花括号时不当作参数（legado 语义）', () => {
    // URL 自身含逗号是合法的
    expect(parseSourceImageOptions('https://cdn.example/a,b.jpg')).toBeNull()
  })

  it('支持 width 与 displayText', () => {
    const parsed = parseSourceImageOptions(
      'https://cdn.example/a.jpg,{"style":"TEXT","displayText":"12","width":"2em"}',
    )
    expect(parsed!.url).toBe('https://cdn.example/a.jpg')
    expect(parsed!.style).toBe('text')
    expect(parsed!.label).toBe('12')
    expect(parsed!.width).toBe('2em')
  })
})

describe('processSourceImageOptions（用最小 DOM 桩，不依赖 jsdom）', () => {
  /** 只实现被测代码用到的那几个 DOM 接口，避免为一个测试引入 jsdom 依赖。 */
  function makeFakeImage(rawSrc: string) {
    const classes = new Set<string>()
    const attrs = new Map<string, string>([['src', rawSrc]])
    const image = {
      dataset: {} as Record<string, string>,
      style: {} as Record<string, string>,
      classList: {
        add: (name: string) => classes.add(name),
        contains: (name: string) => classes.has(name),
      },
      getAttribute: (name: string) => attrs.get(name) ?? null,
      setAttribute: (name: string, value: string) => attrs.set(name, value),
    }
    return { image, classes, attrs }
  }

  function runOn(rawSrc: string) {
    const { image, classes, attrs } = makeFakeImage(rawSrc)
    const root = {
      querySelectorAll: () => [image],
    } as unknown as HTMLElement
    processSourceImageOptions(root)
    return { image, classes, attrs }
  }

  it('把破损图标修成可点击气泡并保留原始 src', () => {
    const { image, classes, attrs } = runOn(REAL_SRC)

    // 关键：src 里的 JSON 部分必须被剥离，否则图片加载失败显示破损图标
    const produced = attrs.get('src') || ''
    expect(produced).not.toContain('{')
    expect(produced.startsWith('data:image/svg+xml;base64,')).toBe(true)
    // 缺 xmlns 的 SVG 会被补上命名空间(这正是齿轮图标破损的原因)
    expect(decodeDataUri(produced)).toContain('xmlns="http://www.w3.org/2000/svg"')
    expect(image.dataset.sourceClick).toBe('showConfigPanel()')
    expect(classes.has('source-clickable')).toBe(true)
    expect(classes.has('source-inline-bubble')).toBe(true)
    // 原始 src 保留给 click 脚本的 `result`
    expect(image.dataset.sourceRawSrc).toBe(REAL_SRC)
  })

  it('不影响没有参数的普通图片', () => {
    const { classes, attrs, image } = runOn('https://cdn.example/a.jpg')

    expect(attrs.get('src')).toBe('https://cdn.example/a.jpg')
    expect(image.dataset.sourceClick).toBeUndefined()
    expect(classes.has('source-clickable')).toBe(false)
  })

  it('JSON 非法时保持原样', () => {
    const bad = 'https://cdn.example/a.jpg,{oops'
    const { classes, attrs } = runOn(bad)
    expect(attrs.get('src')).toBe(bad)
    expect(classes.has('source-clickable')).toBe(false)
  })
})

describe('reserveImageSpace（用最小 DOM 桩，不依赖 jsdom）', () => {
  function makeImage(opts: {
    attrs?: Record<string, string>
    classes?: string[]
    style?: Record<string, string>
    paragraphText?: string
    containerSelector?: string
  }) {
    const attrs = new Map<string, string>(Object.entries(opts.attrs ?? {}))
    const classes = new Set(opts.classes ?? [])
    const style: Record<string, string> = { ...(opts.style ?? {}) }
    const paragraph = opts.paragraphText === undefined
      ? null
      : { textContent: opts.paragraphText }
    const image = {
      style,
      closest: (selector: string) => {
        if (!paragraph) return null
        return opts.containerSelector && selector.includes(opts.containerSelector)
          ? paragraph
          : null
      },
      classList: {
        add: (name: string) => classes.add(name),
        contains: (name: string) => classes.has(name),
        remove: (name: string) => classes.delete(name),
      },
      hasAttribute: (name: string) => attrs.has(name),
      getAttribute: (name: string) => attrs.get(name) ?? null,
    }
    return { image, classes }
  }

  function runOn(images: ReturnType<typeof makeImage>[]) {
    const root = { querySelectorAll: () => images.map(({ image }) => image) } as unknown as HTMLElement
    reserveImageSpace(root)
    return images
  }

  it('leaves an image that already declares width and height alone', () => {
    // 书源插图常带这两个属性, 浏览器据此即可在加载前按比例占位。
    const [{ image }] = runOn([makeImage({ attrs: { width: '1024', height: '576' } })])

    expect(image.style.aspectRatio).toBeUndefined()
    expect(image.classList.contains('reader-image-pending')).toBe(false)
    expect(image.classList.contains('reader-image-block')).toBe(false)
  })

  it('reserves width and pending state without a misleading aspect ratio', () => {
    const [{ image }] = runOn([makeImage({ attrs: { src: 'a.jpg' } })])

    expect(image.style.width).toBe('100%')
    expect(image.style.aspectRatio).toBeUndefined()
    expect(image.style.height).toBe('auto')
    expect(image.classList.contains('reader-image-pending')).toBe(true)
    expect(image.classList.contains('reader-image-block')).toBe(true)
  })

  it('does not trust a single declared dimension', () => {
    const [{ image }] = runOn([makeImage({ attrs: { width: '1024' } })])

    expect(image.style.width).toBe('100%')
    expect(image.style.aspectRatio).toBeUndefined()
    expect(image.classList.contains('reader-image-pending')).toBe(true)
    expect(image.classList.contains('reader-image-block')).toBe(true)
  })

  it('leaves commentary bubbles alone', () => {
    // 气泡有自己固定的行内尺寸, 再套宽高比会把它撑成整段高图。
    const [{ image }] = runOn([makeImage({ classes: ['source-inline-bubble'] })])

    expect(image.style.aspectRatio).toBeUndefined()
    expect(image.classList.contains('reader-image-pending')).toBe(false)
    expect(image.classList.contains('reader-image-block')).toBe(false)
  })

  it('leaves source images with ,{json} options alone', () => {
    const [{ image }] = runOn([makeImage({ attrs: { src: REAL_SRC } })])

    expect(image.style.width).toBeUndefined()
    expect(image.style.aspectRatio).toBeUndefined()
    expect(image.classList.contains('reader-image-pending')).toBe(false)
    expect(image.classList.contains('reader-image-block')).toBe(false)
  })

  it('does not override a size the source already set', () => {
    const [{ image }] = runOn([makeImage({ style: { width: '110px' } })])

    expect(image.style.aspectRatio).toBeUndefined()
    expect(image.classList.contains('reader-image-pending')).toBe(false)
    expect(image.classList.contains('reader-image-block')).toBe(false)
  })

  it('keeps body text illustrations inline', () => {
    const [{ image }] = runOn([makeImage({
      attrs: { src: 'inline.jpg' },
      paragraphText: '正文和插图',
      containerSelector: 'p',
    })])

    expect(image.style.width).toBeUndefined()
    expect(image.style.aspectRatio).toBeUndefined()
    expect(image.classList.contains('reader-image-pending')).toBe(false)
    expect(image.classList.contains('reader-image-block')).toBe(false)
  })

  it('keeps mixed div illustrations inline', () => {
    const [{ image }] = runOn([makeImage({
      attrs: { src: 'inline-div.jpg' },
      paragraphText: '正文和插图',
      containerSelector: 'div',
    })])

    expect(image.style.width).toBeUndefined()
    expect(image.classList.contains('reader-image-pending')).toBe(false)
    expect(image.classList.contains('reader-image-block')).toBe(false)
  })
})

describe('settleReaderImageLayout', () => {
  function loadedImage(classes: string[] = ['reader-image-pending', 'reader-image-block']) {
    const style: Record<string, string> = {
      width: '100%',
      height: 'auto',
      aspectRatio: '16 / 9',
      minHeight: '200px',
      maxHeight: '720px',
    }
    const imageClasses = new Set(classes)
    const image = {
      naturalWidth: 720,
      naturalHeight: 9000,
      style: {
        get aspectRatio() { return style.aspectRatio },
        get minHeight() { return style.minHeight },
        get maxHeight() { return style.maxHeight },
        get height() { return style.height },
        set height(value: string) { style.height = value },
        removeProperty: (name: string) => {
          const key = name === 'aspect-ratio'
            ? 'aspectRatio'
            : name === 'min-height'
              ? 'minHeight'
              : name === 'max-height'
                ? 'maxHeight'
                : name
          delete style[key]
        },
      },
      classList: {
        contains: (name: string) => imageClasses.has(name),
        remove: (name: string) => imageClasses.delete(name),
      },
    }
    return { image, imageClasses, style }
  }

  it('uses natural dimensions and clears the fixed ratio after load', () => {
    const { image, imageClasses, style } = loadedImage()

    expect(settleReaderImageLayout(image as unknown as HTMLImageElement)).toBe(true)
    expect(imageClasses.has('reader-image-pending')).toBe(false)
    expect(imageClasses.has('reader-image-block')).toBe(false)
    expect(style.aspectRatio).toBeUndefined()
    expect(style.height).toBe('auto')
    expect(style.minHeight).toBeUndefined()
    // 横向分页器会写 max-height 限制长图高度, 加载结算不能把它删掉。
    expect(style.maxHeight).toBe('720px')
  })

  it('does not settle an image before natural dimensions are available', () => {
    const { image, imageClasses, style } = loadedImage()
    image.naturalWidth = 0
    image.naturalHeight = 0

    expect(settleReaderImageLayout(image as unknown as HTMLImageElement)).toBe(false)
    expect(imageClasses.has('reader-image-pending')).toBe(true)
    expect(style.aspectRatio).toBe('16 / 9')
  })

  it('does not touch a source-sized image that was never marked as a block illustration', () => {
    const { image, imageClasses, style } = loadedImage([])

    expect(settleReaderImageLayout(image as unknown as HTMLImageElement)).toBe(false)
    expect(imageClasses.size).toBe(0)
    expect(style.aspectRatio).toBe('16 / 9')
  })
})

describe('ensureSvgNamespace', () => {
  it('给缺少 xmlns 的 SVG 补上命名空间(修复控制面板齿轮图标)', () => {
    const fixed = ensureSvgNamespace(toDataUri(GEAR_SVG))
    const svg = decodeDataUri(fixed)

    expect(svg).toContain('xmlns="http://www.w3.org/2000/svg"')
    // 其余内容必须原样保留
    expect(svg).toContain('<circle cx="12" cy="12" r="3">')
    expect(svg).toContain('#757575')
  })

  it('已带 xmlns 的 SVG 不动(段评气泡一直正常)', () => {
    const original = toDataUri(COMMENT_SVG)
    expect(ensureSvgNamespace(original)).toBe(original)
  })

  it('非 SVG 与非法 base64 原样返回', () => {
    const jpg = 'data:image/jpeg;base64,AAAA'
    expect(ensureSvgNamespace(jpg)).toBe(jpg)
    const broken = 'data:image/svg+xml;base64,!!!not-base64!!!'
    expect(ensureSvgNamespace(broken)).toBe(broken)
    const httpUrl = 'https://cdn.example/a.svg'
    expect(ensureSvgNamespace(httpUrl)).toBe(httpUrl)
  })

  it('中文内容编码往返不丢失', () => {
    const svgWithCjk =
      '<svg viewBox="0 0 10 10"><text>段评</text></svg>'
    const fixed = decodeDataUri(ensureSvgNamespace(toDataUri(svgWithCjk)))
    expect(fixed).toContain('段评')
    expect(fixed).toContain('xmlns=')
  })
})
