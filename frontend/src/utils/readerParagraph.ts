/** 书源正文的段落间距处理。
 *
 *  背景: ReaderView 的 formatChapterHtml 只给 <p> 写内联 margin-bottom,
 *  于是「有 HTML 但没有 <p>」的正文(书源常见 <br> 分段)拿不到「段落间距」
 *  与首行缩进。这里把 <br> 分段的正文重组成独立 <p>。
 *
 *  重组是字符串级操作, 必须有严格的安全闸: 只有「全部标签都在行内白名单内」
 *  且「每个段落自身标签平衡」时才敢拆。任一不满足就返回 null, 由调用方退回
 *  「只补 margin、不动结构」的安全路径 —— 结构重组出错(悬空标签)的代价远大于
 *  段落间距不生效。
 *
 *  纯字符串逻辑, 不碰 DOM, 便于单测。 */

export interface ParagraphSpacingOptions {
  /** 段落间距, 单位 em(与设置面板一致)。 */
  spacingEm: number
  /** 是否首行缩进 2 字符。 */
  firstLineIndent: boolean
}

/** 允许出现在段落内部的行内标签。名单之外(div/table/img/hr/h1 等)一律不拆:
 *  黑名单式的排除总会漏新标签, 白名单只会更保守。 */
const INLINE_TAGS = new Set([
  'a', 'abbr', 'b', 'bdi', 'bdo', 'br', 'cite', 'code', 'data', 'dfn', 'em',
  'font', 'i', 'kbd', 'mark', 'q', 'rp', 'rt', 'ruby', 's', 'samp', 'small',
  'span', 'strong', 'sub', 'sup', 'time', 'u', 'var', 'wbr',
])

/** 无需闭合的行内空元素。 */
const VOID_INLINE_TAGS = new Set(['br', 'wbr'])

const TAG_PATTERN = /<\/?([a-zA-Z][a-zA-Z0-9-]*)\b[^>]*?(\/?)>/g

/** 扫描标签, 校验「白名单 + 平衡」。返回 false 表示这段 HTML 不能安全重组。 */
function isSplittableMarkup(html: string): boolean {
  const stack: string[] = []
  TAG_PATTERN.lastIndex = 0
  let match: RegExpExecArray | null
  while ((match = TAG_PATTERN.exec(html)) !== null) {
    const tagName = match[1].toLowerCase()
    if (!INLINE_TAGS.has(tagName)) return false
    if (VOID_INLINE_TAGS.has(tagName) || match[2] === '/') continue
    if (match[0].startsWith('</')) {
      if (stack.pop() !== tagName) return false
    } else {
      stack.push(tagName)
    }
  }
  return stack.length === 0
}

/**
 * 把以 <br> 分段的 HTML 拆成独立 <p>, 并附上间距与缩进。
 *
 * @returns 拆好的 HTML; 空串表示没有可渲染内容;
 *          **null 表示这段内容不能安全重组**(含结构化标签, 或有标签跨 <br>,
 *          例如 `<b>甲<br>乙</b>` 拆开会留下悬空标签), 调用方应放弃重组。
 */
export function splitBrParagraphs(
  html: string,
  options: ParagraphSpacingOptions,
): string | null {
  // 整体先过一遍: 拦掉结构化标签与源本身就不平衡的 HTML。
  if (!isSplittableMarkup(html)) return null

  const paragraphs = html
    .split(/<br\s*\/?>/i)
    .map((line) => line.replace(/^[\u3000\u00A0 \t]+/, '').trimEnd())
    .filter((line) => line.trim())
  if (!paragraphs.length) return ''

  // 逐段再校验: 整体平衡不代表每段平衡(`<b>甲<br>乙</b>` 整体是平的)。
  // 属性值里混入 "<br" 造成的误切也会在这里被拦下。
  if (!paragraphs.every((paragraph) => isSplittableMarkup(paragraph))) return null

  const spacing = `${options.spacingEm}em`
  const indentClass = options.firstLineIndent ? ' class="reader-indent"' : ''
  return paragraphs
    .map((paragraph) => `<p${indentClass} style="margin-top: 0; margin-bottom: ${spacing};">${paragraph}</p>`)
    .join('')
}
