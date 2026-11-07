import { describe, expect, it } from 'vitest'
import { protectSpecialContent } from './replaceRuleProtect'

describe('protectSpecialContent', () => {
  it('图片标签被保护，贪婪规则吃不到它', () => {
    const source = '开头<img src="a.png">中间<img src="b.png">结尾'
    const protectedContent = protectSpecialContent(source)
    expect(protectedContent.content).not.toContain('<img')
    expect(protectedContent.restore(protectedContent.content)).toBe(source)
  })

  it('模拟 `.*` 规则：命中后图片仍能还原', () => {
    const source = '广告文字<img src="a.png">正文'
    const { content, restore } = protectSpecialContent(source)
    // 规则把整行换成自己的文本 —— 占位符跟着被一起删掉也不会影响其它片段还原。
    expect(restore(content.replace(/.*/, '替换后'))).toBe('替换后')
  })

  it('单独成行的 [newpage] 被保护', () => {
    const source = '第一段\n  [newpage]  \n第二段'
    const { content, restore } = protectSpecialContent(source)
    expect(content).not.toContain('[newpage]')
    expect(restore(content)).toBe(source)
  })

  it('行内出现的 [newpage] 不是分页标记，不保护', () => {
    const source = '这里有 [newpage] 但不在独立行'
    expect(protectSpecialContent(source).content).toBe(source)
  })

  it('内联 style / script 被保护', () => {
    const source = '正文<style>p{color:red}</style>更多<script>var a=1;</script>结束'
    const { content, restore } = protectSpecialContent(source)
    expect(content).not.toContain('<style')
    expect(content).not.toContain('<script')
    expect(restore(content)).toBe(source)
  })

  it('未闭合的标签不会吞掉后面所有正文', () => {
    const source = '正文<img src="x.png" 后面还有字'
    expect(protectSpecialContent(source).content).toBe(source)
  })

  it('相似标签名不会被当成 img', () => {
    const source = '<images>不是图片标签</images>'
    expect(protectSpecialContent(source).content).toContain('<images>')
  })

  it('纯文本原样返回', () => {
    const source = '这是一段没有任何特殊标记的正文。'
    const { content, restore } = protectSpecialContent(source)
    expect(content).toBe(source)
    expect(restore(content)).toBe(source)
  })

  it('多类片段混排时保持原有顺序', () => {
    const source = '<img src="1.png">\n[newpage]\n<style>x{}</style>\n尾巴'
    const { content, restore } = protectSpecialContent(source)
    expect(restore(content)).toBe(source)
  })

  it('大小写不同的标签同样被保护', () => {
    const source = '正文<IMG SRC="a.png">尾巴'
    const { content, restore } = protectSpecialContent(source)
    expect(content).not.toContain('<IMG')
    expect(restore(content)).toBe(source)
  })
})
