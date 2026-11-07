/**
 * 净化替换前的特殊内容保护（前端同步路径）。
 *
 * 与 Rust 侧 `parser::special_content_protector` 是同一套规则的两份实现 —— 两条
 * 执行路径（前端同步快路径、Rust 沙盒路径）必须给出一致结果，所以两边的保护范围
 * 与占位符形态都刻意保持相同。改其中一边时另一边要一起改。
 *
 * 为什么需要它：净化规则是用户从网上抄来的，常见 `.*`、`[^\n]+$` 这类宽匹配。
 * 直接作用在正文 HTML 上会把 `<img>` 一起吃掉，表现为"开了净化后插图全没了"。
 * 手机端用 `SpecialContentProtector` 做同样的事（保护 img / newpage / 内联样式脚本）。
 */

const MARK_LEFT = '\uE000'
const MARK_RIGHT = '\uE001'

export interface ProtectedContent {
  /** 保护后（可直接跑替换规则）的正文。 */
  content: string
  /** 把占位符换回原片段。 */
  restore: (value: string) => string
}

/** 自闭合标签：第一个 `>` 就是结尾。 */
const SELF_CLOSING = new Set(['img'])

/** 需要保护的片段类别。 */
export interface ContentKinds {
  img: boolean
  script: boolean
  style: boolean
  newpage: boolean
}

export const ALL_CONTENT_KINDS: ContentKinds = {
  img: true,
  script: true,
  style: true,
  newpage: true,
}

/**
 * 按**规则集**决定保护哪些类别。
 *
 * 规则里明确提到某类标签（例如 pattern 写 `<img\b[^>]*>`）时，那条规则的目的就是
 * 处理它 —— 这种情况必须把这一类放开，否则「删除评论图」这类既有规则会突然失效。
 * 其余类别一律保护，避免 `.*` 这类宽匹配顺手把图片吃掉。
 *
 * 判定只看「是否提到」，不追求精确解析正则：`img` 出现在 pattern 里几乎只会是
 * 「要处理图片」，误判的代价是"这一类退回原来的不保护行为"，不会更糟。
 *
 * 与 Rust 侧 `special_content_protector::kinds_for_patterns` 是同一套判定 ——
 * 两条执行路径必须一致，改一边时另一边一起改。
 */
export function kindsForPatterns(patterns: Iterable<string>): ContentKinds {
  let mentionsImg = false
  let mentionsScript = false
  let mentionsStyle = false
  let mentionsNewpage = false
  for (const pattern of patterns) {
    const lower = pattern.toLowerCase()
    if (lower.includes('img')) mentionsImg = true
    if (lower.includes('script')) mentionsScript = true
    if (lower.includes('style')) mentionsStyle = true
    if (lower.includes('newpage')) mentionsNewpage = true
  }
  return {
    img: !mentionsImg,
    script: !mentionsScript,
    style: !mentionsStyle,
    newpage: !mentionsNewpage,
  }
}

/**
 * 找出 `open` 在 `content` 里所有「标签起点」位置。
 *
 * 必须校验边界：`<images>` 不是 `<img>`，否则会把无关标签整段保护起来。
 */
function findTagStart(lower: string, needle: string, from: number): number {
  let index = lower.indexOf(needle, from)
  while (index >= 0) {
    const next = lower[index + needle.length]
    const boundaryOk = next !== undefined && !/[a-zA-Z0-9_-]/.test(next)
    if (boundaryOk) return index
    index = lower.indexOf(needle, index + needle.length)
  }
  return -1
}

/**
 * 保护 `open … close` 形式的标签块。
 *
 * 用「找开标签 → 找对应闭标签」而不是正则：`<img>` 自闭合没有 `</img>`，
 * `<script>` / `<style>` 内部可能含 `>`，正则容易切歪。
 */
function protectTagBlocks(
  content: string,
  open: string,
  close: string,
  reserve: (value: string) => string,
): string {
  const lower = content.toLowerCase()
  let out = ''
  let cursor = 0

  for (;;) {
    const start = findTagStart(lower, open, cursor)
    if (start < 0) break

    let end: number
    if (SELF_CLOSING.has(open.slice(1))) {
      const closeIndex = lower.indexOf('>', start)
      if (closeIndex < 0) break // 没闭合：剩下的原样保留，不吞内容
      end = closeIndex + 1
    } else {
      const closeIndex = lower.indexOf(close, start)
      if (closeIndex < 0) break
      end = closeIndex + close.length
    }

    out += content.slice(cursor, start)
    out += reserve(content.slice(start, end))
    cursor = end
  }

  return out + content.slice(cursor)
}

/** 保护单独成行的 `[newpage]`（行内出现不算，与手机端正则一致）。 */
function protectNewpage(content: string, reserve: (value: string) => string): string {
  return content
    .split('\n')
    .map((line) => (line.trim().toLowerCase() === '[newpage]' ? reserve(line) : line))
    .join('\n')
}

/** 把正文里的特殊片段换成占位符，返回保护结果与还原函数。 */
export function protectSpecialContent(
  content: string,
  kinds: ContentKinds = ALL_CONTENT_KINDS,
): ProtectedContent {
  const placeholders: Array<[string, string]> = []
  const reserve = (value: string) => {
    const placeholder = `${MARK_LEFT}READER_SPECIAL_${placeholders.length}${MARK_RIGHT}`
    placeholders.push([placeholder, value])
    return placeholder
  }

  let protectedText = content
  for (const [enabled, open, close] of [
    [kinds.img, '<img', '>'],
    [kinds.script, '<script', '</script>'],
    [kinds.style, '<style', '</style>'],
  ] as const) {
    if (enabled) {
      protectedText = protectTagBlocks(protectedText, open, close, reserve)
    }
  }
  if (kinds.newpage) {
    protectedText = protectNewpage(protectedText, reserve)
  }

  return {
    content: protectedText,
    // 逐个 replacen：占位符是我们生成的、各出现一次，既不漏也不重复。
    restore: (value: string) => {
      let restored = value
      for (const [placeholder, original] of placeholders) {
        if (restored.includes(placeholder)) {
          restored = restored.replace(placeholder, original)
        }
      }
      return restored
    },
  }
}
