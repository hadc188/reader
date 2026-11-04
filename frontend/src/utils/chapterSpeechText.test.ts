import { describe, expect, it } from 'vitest'
import { splitChapterContentIntoParagraphs, stripInvisibleMarkup } from './chapterSpeechText'

/** 本地 EPUB 的章节形态：后端把整本书的作用域化 CSS 内联进章节容器。 */
const EPUB_CHAPTER =
  '<div class="epub-chapter-content"><style>.epub-chapter-content .calibre2 '
  + '{ display: block; font-size: 1em; margin: 1em 0 }</style>'
  + '<h1 class="calibre1">第一章 冬——春</h1>'
  + '<p class="calibre2">睁开眼，窗外已经大白。</p>'
  + '<p class="calibre2">看了一眼手机，六点整。</p></div>'

describe('splitChapterContentIntoParagraphs', () => {
  it('drops the EPUB scoped stylesheet instead of reading CSS as body text', () => {
    const paragraphs = splitChapterContentIntoParagraphs(EPUB_CHAPTER)
    // issue #30：CSS 规则被当成段落朗读，用户听到「下一章开始念英文」。
    expect(paragraphs.some((line) => line.includes('display'))).toBe(false)
    expect(paragraphs.some((line) => line.includes('font-size'))).toBe(false)
    expect(paragraphs.some((line) => line.includes('.epub-chapter-content'))).toBe(false)
    expect(paragraphs).toEqual(['第一章 冬——春', '睁开眼，窗外已经大白。', '看了一眼手机，六点整。'])
  })

  it('drops script and head blocks as well', () => {
    const html = '<head><title>未知</title><style>p{margin:0}</style></head>'
      + '<body><script>var a = 1;</script><p>正文</p></body>'
    expect(splitChapterContentIntoParagraphs(html)).toEqual(['正文'])
  })

  it('keeps chapter text that merely mentions style words', () => {
    // 只有标签内部才算「不可见」；正文里出现 style / display 这类词必须照常朗读。
    const paragraphs = splitChapterContentIntoParagraphs('<p>这个 display 挺清楚。</p>')
    expect(paragraphs).toEqual(['这个 display 挺清楚。'])
  })

  it('splits plain text by lines and trims indentation', () => {
    expect(splitChapterContentIntoParagraphs('第一段。\n\u3000\u3000第二段。\n\n')).toEqual([
      '第一段。',
      '第二段。',
    ])
  })

  it('handles an unclosed style tag without swallowing the body', () => {
    // 畸形 HTML（缺 </style>）不能把后面的正文整段吃掉：宁可多留一段也不吞正文。
    const paragraphs = splitChapterContentIntoParagraphs('<style>p{margin:0}<p>正文。</p>')
    expect(paragraphs.join('')).toContain('正文')
  })

  it('splits headings into their own paragraphs like p/div', () => {
    const html = '<h1>第一章 起点</h1><div>段一</div><h2>小标题</h2><div>段二</div>'
    expect(splitChapterContentIntoParagraphs(html)).toEqual(['第一章 起点', '段一', '小标题', '段二'])
  })
})

describe('stripInvisibleMarkup', () => {
  it('removes style/script/comment content but keeps the rest', () => {
    const html = '<!--note--><style>.a{color:red}</style><p>正文</p><script>1</script>'
    expect(stripInvisibleMarkup(html)).not.toContain('color')
    expect(stripInvisibleMarkup(html)).toContain('正文')
  })

  it('leaves plain text without markup untouched', () => {
    expect(stripInvisibleMarkup('纯文本正文，没有任何标签。')).toBe('纯文本正文，没有任何标签。')
  })
})
