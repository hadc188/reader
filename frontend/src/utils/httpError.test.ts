import { describe, expect, it } from 'vitest'
import { summarizeDisplayError, summarizeHttpErrorBody } from './httpError'

const cloudflareTimeoutHtml = `<!DOCTYPE html>
<html><head><title>grandy.fun | 524: A timeout occurred</title></head>
<body><span class="code-label">Error code 524</span><p>${'x'.repeat(1000)}</p></body></html>`

describe('httpError', () => {
  it('summarizes HTML error pages for model requests', () => {
    expect(summarizeHttpErrorBody(cloudflareTimeoutHtml, {
      fallback: 'AI 资料生成失败',
      status: 524,
    })).toBe('AI 资料生成失败 (524)，服务返回 HTML 错误页，错误码 524：grandy.fun | 524: A timeout occurred')
  })

  it('summarizes saved display errors without leaking full HTML into the page', () => {
    const summary = summarizeDisplayError(cloudflareTimeoutHtml)
    expect(summary).toBe('服务返回 HTML 错误页，错误码 524：grandy.fun | 524: A timeout occurred')
    expect(summary).not.toContain('<!DOCTYPE')
    expect(summary.length).toBeLessThan(100)
  })

  it('truncates long plain text errors', () => {
    const summary = summarizeDisplayError(`失败：${'网络超时'.repeat(100)}`, 40)
    expect(summary.endsWith('...')).toBe(true)
    expect(summary.length).toBeLessThanOrEqual(43)
  })
})
