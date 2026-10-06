import { describe, expect, it } from 'vitest'
import { splitBrParagraphs } from './readerParagraph'

const options = { spacingEm: 0.5, firstLineIndent: true }

describe('splitBrParagraphs', () => {
  it('splits each <br> into its own paragraph with the configured spacing', () => {
    const html = splitBrParagraphs('第一段<br>第二段<br>第三段', options)

    expect(html?.match(/<p/g)).toHaveLength(3)
    expect(html).toContain('margin-bottom: 0.5em')
    expect(html).toContain('第一段')
    expect(html).toContain('第三段')
  })

  it('honours the indent setting', () => {
    expect(splitBrParagraphs('正文<br>下一段', options)).toContain('class="reader-indent"')
    expect(splitBrParagraphs('正文<br>下一段', { ...options, firstLineIndent: false }))
      .not.toContain('reader-indent')
  })

  it('accepts self-closing and spaced <br> variants', () => {
    expect(splitBrParagraphs('甲<br/>乙<br />丙', options)?.match(/<p/g)).toHaveLength(3)
  })

  it('drops empty lines produced by consecutive breaks', () => {
    const html = splitBrParagraphs('甲<br><br>乙<br>   <br>', options)

    expect(html?.match(/<p/g)).toHaveLength(2)
    expect(html).not.toContain('margin-bottom: 0.5em;">   <')
  })

  it('strips leading full-width and ASCII indentation inside each paragraph', () => {
    const html = splitBrParagraphs('\u3000\u3000首行缩进<br>  半角缩进', options)

    expect(html).toContain('>首行缩进</p>')
    expect(html).toContain('>半角缩进</p>')
  })

  it('keeps inline markup that stays balanced inside one paragraph', () => {
    expect(splitBrParagraphs('<b>加粗</b>文字<br>普通', options))
      .toContain('<b>加粗</b>文字')
  })

  it('returns an empty string when there is nothing to render', () => {
    expect(splitBrParagraphs('<br><br>', options)).toBe('')
  })

  // 以下为安全闸: 重组会破坏结构时必须返回 null, 让调用方保留原 HTML。
  it('refuses to split when an inline tag spans across a <br>', () => {
    // <b> 跨段会导致每个 <p> 里都有悬空标签。
    expect(splitBrParagraphs('<b>甲<br>乙</b>', options)).toBeNull()
    expect(splitBrParagraphs('<span>甲<br>乙', options)).toBeNull()
  })

  it('refuses to split when structured markup is present', () => {
    expect(splitBrParagraphs('<img src="a.png"><br>正文', options)).toBeNull()
    expect(splitBrParagraphs('<div>甲</div><br>乙', options)).toBeNull()
    expect(splitBrParagraphs('<table><tr><td>甲</td></tr></table><br>乙', options)).toBeNull()
    expect(splitBrParagraphs('甲<br><h1>标题</h1>', options)).toBeNull()
    expect(splitBrParagraphs('甲<br><hr>', options)).toBeNull()
    expect(splitBrParagraphs('甲<br><iframe src="x"></iframe>', options)).toBeNull()
    expect(splitBrParagraphs('甲<br><svg><circle /></svg>', options)).toBeNull()
  })

  it('refuses to split unbalanced markup', () => {
    expect(splitBrParagraphs('甲<br><b>未闭合', options)).toBeNull()
  })

  it('is not fooled by a tag name that merely appears inside text', () => {
    // 属性值里的 "<br" 会被 split 误切, 逐段校验会拦下它, 退回安全路径。
    expect(splitBrParagraphs('<a href="x<br>y">链接</a>', options)).toBeNull()
  })
})
