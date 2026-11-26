import { readSourceLiteral } from './sourceLiteral'

/**
 * legado `img` 的 `,{json}` 参数协议。
 *
 * 正文里的图片可以是 `src='data:image/svg+xml;base64,...==,{"style":"text","type":"qd","click":"showConfigPanel()"}'`，
 * 逗号后的 JSON 是渲染选项而非 URL 的一部分（legado `AnalyzeUrl.paramPattern` 为 `\s*,\s*(?=\{)`）。
 * 不剥离这段后缀，浏览器会把整串当 URL 加载，图标必然显示为破损图片。
 */

/** 与 legado `AnalyzeUrl.paramPattern` 一致：逗号 + 可空白 + 左花括号。 */
export const SOURCE_IMG_OPTION_RE = /\s*,\s*(?=\{)/

export interface SourceImageOption {
  /** 剥离参数后的真实图片地址。 */
  url: string
  /** `style`，如 `text`。 */
  style: string
  /** `click` 脚本（在书源 jsLib 作用域执行）。 */
  click: string
  /** `width`。 */
  width: string
  /** 用于提示的显示文本（`displayText` / `num` / `text`）。 */
  label: string
}

const SVG_BASE64_MARKER = ';base64,'

/**
 * 给缺少命名空间的 SVG data URI 补上 `xmlns`。
 *
 * 插件的图标有两种：段评气泡自带 `xmlns="http://www.w3.org/2000/svg"`，
 * 而"打开控制面板"的齿轮图标没有。作为 `data:image/svg+xml` 独立文档加载时，
 * 缺少命名空间声明的 SVG 会解析失败，浏览器只能显示破损图片占位符。
 * 这里在本地补上命名空间，不改动远程 jsLib。
 */
export function ensureSvgNamespace(url: string): string {
  if (!url.startsWith('data:image/svg+xml')) return url
  const markerIndex = url.indexOf(SVG_BASE64_MARKER)
  if (markerIndex < 0) return url

  const header = url.slice(0, markerIndex + SVG_BASE64_MARKER.length)
  const payload = url.slice(markerIndex + SVG_BASE64_MARKER.length)
  try {
    // base64 → UTF-8 文本（不能直接用 atob 的二进制串，图标里有中文会被破坏）
    const binary = atob(payload)
    const bytes = Uint8Array.from(binary, (char) => char.charCodeAt(0))
    const svg = new TextDecoder('utf-8').decode(bytes)

    if (/\sxmlns\s*=/.test(svg)) return url

    const fixed = svg.replace(/<svg\b/i, '<svg xmlns="http://www.w3.org/2000/svg"')
    if (fixed === svg) return url

    const outBytes = new TextEncoder().encode(fixed)
    let outBinary = ''
    outBytes.forEach((byte) => {
      outBinary += String.fromCharCode(byte)
    })
    return header + btoa(outBinary)
  } catch {
    // 解码/编码失败时保持原样（宁可显示破损，也不要弄坏能用的图标）
    return url
  }
}

/**
 * 拆分图片 `src` 与其 `,{json}` 选项。
 * 没有选项或 JSON 非法时返回 `null`（保持原始 src 不动，避免误伤普通图片）。
 */
export function parseSourceImageOptions(rawSrc: string): SourceImageOption | null {
  if (!rawSrc) return null
  const separators = [...rawSrc.matchAll(new RegExp(SOURCE_IMG_OPTION_RE.source, 'g'))].reverse()
  for (const separator of separators) {
    const parsed = readSourceLiteral(rawSrc, separator.index + separator[0].length)
    if (!parsed || rawSrc.slice(parsed.end).trim()) continue
    const option = parsed.value
    if (!option || typeof option !== 'object' || Array.isArray(option)) continue
    const asText = (key: string) => {
      const value = option[key]
      return typeof value === 'string' || typeof value === 'number' ? String(value) : ''
    }
    return {
      url: rawSrc.slice(0, separator.index).trim(),
      style: asText('style').toLowerCase(),
      click: asText('click'),
      width: asText('width'),
      label: asText('displayText') || asText('num') || asText('text'),
    }
  }
  return null
}

/** 给正文插图预留宽度, 只处理没有可信尺寸的普通插图。
 *
 *  不能给未知图片套固定宽高比: 在线漫画常见 720x9000 的整页长图, 16:9 会把它
 *  永久压成一条横带, `height:auto` 也覆盖不了显式的 aspect-ratio。这里只负责铺满
 *  正文列和显示骨架, 加载完成后由 settleReaderImageLayout 按自然尺寸结算。
 *  与文字同段的插图、来源已给明确尺寸的图片、以及带 `,{json}` 选项的图标
 *  都不参与, 保留它们的原始排版。 */
const FALLBACK_IMAGE_CLASS = 'reader-image-pending'
const READER_BLOCK_IMAGE_CLASS = 'reader-image-block'
const TEXT_CONTAINER_SELECTOR = 'p, div, td, th, li, dd, blockquote'

export function reserveImageSpace(root: HTMLElement) {
  const images = Array.from(root.querySelectorAll('img')) as HTMLImageElement[]
  images.forEach((image) => {
    if (image.classList.contains('source-inline-bubble')) return
    // `,{json}` 图片是插件图标或带自定义尺寸的展示图, 不参与正文占位。
    if (parseSourceImageOptions(image.getAttribute('src') || '')) return
    if (image.hasAttribute('width') && image.hasAttribute('height')) return
    if (image.style.width || image.style.height || image.style.aspectRatio) return
    // 与文字同容器的图片通常是正文插图, 不铺满整列。
    const container = image.closest(TEXT_CONTAINER_SELECTOR)
    if (container && (container.textContent || '').trim()) return
    image.style.width = '100%'
    image.style.height = 'auto'
    image.classList.add(READER_BLOCK_IMAGE_CLASS)
    image.classList.add(FALLBACK_IMAGE_CLASS)
  })
}

/** 图片加载完成后按自然尺寸结算, 清除来源和占位留下的固定比例。
 *
 *  返回 true 表示布局确实发生了变化, 调用方可以据此决定是否重建横向分页。
 *  缓存命中时 `load` 可能早于阅读页挂载, 此时 natural 尺寸仍可直接读取。 */
export function settleReaderImageLayout(image: HTMLImageElement): boolean {
  if (!(image?.naturalWidth > 0) || !(image.naturalHeight > 0)) {
    return false
  }
  // 段评气泡是行内小图标, 不能按正文插图清掉它自己的尺寸。
  if (image.classList.contains('source-inline-bubble')) {
    return false
  }
  // 只结算本次占位逻辑标记的图片。横向分页写下的 max-height 属于它自己的约束,
  // 这里不能动, 否则重建后同一尺寸命中缓存、不会再触发下一次重建。
  const isBlockImage = image.classList.contains(READER_BLOCK_IMAGE_CLASS)
  if (!isBlockImage) {
    return false
  }
  const hadPendingState = image.classList.contains(FALLBACK_IMAGE_CLASS)
  const hadLayoutOverride = Boolean(
    image.style.aspectRatio
    || image.style.minHeight
    || image.style.height,
  )
  image.classList.remove(FALLBACK_IMAGE_CLASS)
  image.classList.remove(READER_BLOCK_IMAGE_CLASS)
  image.style.removeProperty('aspect-ratio')
  image.style.removeProperty('min-height')
  image.style.width = '100%'
  image.style.height = 'auto'
  return hadPendingState || hadLayoutOverride
}

/**
 * 处理正文容器里的所有图片：剥离 `,{json}`、标记可点击、按 `style:text` 呈现为行内气泡。
 * 直接修改传入的 DOM（与 `formatChapterHtml` 的其余后处理一致）。
 */
export function processSourceImageOptions(root: HTMLElement) {
  const images = Array.from(root.querySelectorAll('img')) as HTMLImageElement[]
  images.forEach((image) => {
    const rawSrc = image.getAttribute('src') || ''
    const parsed = parseSourceImageOptions(rawSrc)
    if (!parsed) return

    image.dataset.sourceRawSrc = rawSrc
    image.setAttribute('src', ensureSvgNamespace(parsed.url))

    if (parsed.click) {
      image.dataset.sourceClick = parsed.click
      image.classList.add('source-clickable')
    }
    // style:text 的图标按行内文本气泡呈现（段评图标通常就是这个形态）
    if (parsed.style === 'text') {
      image.classList.add('source-inline-bubble')
      if (parsed.label) {
        image.setAttribute('title', parsed.label)
        image.setAttribute('alt', parsed.label)
      }
    }
    if (parsed.width) {
      image.style.width = parsed.width
    }
  })
}
