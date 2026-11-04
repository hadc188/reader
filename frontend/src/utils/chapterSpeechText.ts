// 章节正文（可能含 HTML）→ 可朗读的纯文本段落。
//
// 为什么需要它：朗读的「跨章预取」拿不到 DOM，只能从 store 的正文缓存里取字符串。
// 本地 EPUB 的章节是后端拼出来的 `<div class="epub-chapter-content"><style>…</style>
// 正文</div>`（见 `local_epub_book.rs::get_content`），那份 `<style>` 里是整本
// 书作用域化后的 CSS。按「块级标签换行 + 去标签」直接剥，CSS 规则本体是文本节点，
// 会被当成正文段落：`display: block;`、`font-size: 1.2em` 这些英文一行一段地
// 送进朗读引擎。用户听到的就是「第一章中文正常，下一章开始念英文」（issue #30）。
//
// 第一章之所以正常：当前章走的是已渲染 DOM 的 `innerText`，浏览器本来就不把
// `<style>` 当可见文本。只有预取/切章这条字符串路径会漏。因此在字符串路径上
// 必须显式丢掉 `<style>` / `<script>` / `<head>` 整段，而不是只匹配标签名。

/** 整段丢弃：内容不是正文，但会被标签正则原样留下。`head` 里还有 `<title>`。 */
const NON_CONTENT_BLOCK = /<(script|style|head)\b[^>]*>[\s\S]*?<\/\1\s*>/gi
/** 未闭合的同类标签：只摘掉开标签，内容留着（宁可多念一段，也不能吞掉正文）。 */
const NON_CONTENT_OPEN_TAG = /<\/?(?:script|style|head)\b[^>]*>/gi
const HTML_COMMENT = /<!--[\s\S]*?-->/g

/**
 * 去掉「看起来像 HTML 但读者看不到」的部分，并保留原文（不做换行/分段）。
 *
 * 供只需要一段纯文本的调用方使用（例如 `startTTS()` 的兜底取正文）。
 */
export function stripInvisibleMarkup(html: string): string {
  if (!html.includes('<')) return html
  return html
    .replace(HTML_COMMENT, ' ')
    .replace(NON_CONTENT_BLOCK, ' ')
    .replace(NON_CONTENT_OPEN_TAG, ' ')
}

/**
 * 章节正文 → 段落文本。
 *
 * 纯文本（不含标签）按行切；HTML 则先把块级标签换成换行再去标签、解常见实体。
 * 与 `ReaderView.formatChapterHtml` 的纯文本分支保持一致的切分方式。
 *
 * 块级标签名单刻意沿用修复前的 `p/div/li/h*​/tr`：加标签会改变段落序号，而
 * `prefetchNextChapterUnits` 生成的 `paragraphIndex` 要和切章后的 DOM 段落一一对应。
 */
export function splitChapterContentIntoParagraphs(content: string): string[] {
  let text = content
  if (/<[a-z][\s\S]*>/i.test(content)) {
    text = stripInvisibleMarkup(content)
      .replace(/<br\s*\/?>/gi, '\n')
      .replace(/<\/(p|div|li|h\d|tr)>/gi, '\n')
      .replace(/<[^>]+>/g, '')
      .replace(/&nbsp;/gi, ' ')
      .replace(/&lt;/gi, '<')
      .replace(/&gt;/gi, '>')
      .replace(/&quot;/gi, '"')
      .replace(/&#39;/gi, "'")
      .replace(/&amp;/gi, '&')
  }
  return text
    .split(/\n/)
    .map((line) => line.replace(/^[\u3000\u00A0 \t]+/, '').trim())
    .filter(Boolean)
}
