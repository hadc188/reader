export interface HttpErrorSummaryOptions {
  fallback: string
  status?: number
  maxLength?: number
}

export function summarizeHttpErrorBody(raw: string, {
  fallback,
  status,
  maxLength = 260,
}: HttpErrorSummaryOptions) {
  const statusText = status ? ` (${status})` : ''
  const trimmed = raw.trim()
  if (!trimmed) return `${fallback}${statusText}`

  if (looksLikeHtml(trimmed)) {
    const title = extractHtmlTitle(trimmed)
    const code = extractHtmlErrorCode(trimmed) || (status ? String(status) : '')
    return [
      `${fallback}${statusText}`,
      `服务返回 HTML 错误页${code ? `，错误码 ${code}` : ''}${title ? `：${title}` : ''}`,
    ].join('，')
  }

  const text = collapseWhitespace(trimmed)
  if (text.length > maxLength) {
    return `${fallback}${statusText}：${text.slice(0, maxLength)}...`
  }
  return text || `${fallback}${statusText}`
}

export function summarizeDisplayError(raw: string, maxLength = 180) {
  const trimmed = raw.trim()
  if (!trimmed) return ''

  if (looksLikeHtml(trimmed)) {
    const title = extractHtmlTitle(trimmed)
    const code = extractHtmlErrorCode(trimmed)
    return `服务返回 HTML 错误页${code ? `，错误码 ${code}` : ''}${title ? `：${title}` : ''}`
  }

  const text = collapseWhitespace(trimmed)
  return text.length > maxLength ? `${text.slice(0, maxLength)}...` : text
}

export function collapseWhitespace(value: string) {
  return decodeHtmlEntities(value).replace(/\s+/g, ' ').trim()
}

function looksLikeHtml(value: string) {
  return /^\s*<(?:!doctype\s+html|html|head|body|div|span|p|h1)\b/i.test(value)
    || /<html[\s>]/i.test(value)
    || /<\/(?:html|body|head)>/i.test(value)
}

function extractHtmlTitle(value: string) {
  const match = value.match(/<title[^>]*>([\s\S]*?)<\/title>/i)
  return match ? collapseWhitespace(stripTags(match[1])) : ''
}

function extractHtmlErrorCode(value: string) {
  const match = value.match(/(?:Error code|errorcode[_-])\s*(\d{3})/i)
    || value.match(/\b([45]\d{2})\b/)
  return match?.[1] || ''
}

function stripTags(value: string) {
  return value.replace(/<[^>]*>/g, ' ')
}

function decodeHtmlEntities(value: string) {
  return value
    .replace(/&nbsp;/gi, ' ')
    .replace(/&amp;/gi, '&')
    .replace(/&lt;/gi, '<')
    .replace(/&gt;/gi, '>')
    .replace(/&quot;/gi, '"')
    .replace(/&#39;/gi, "'")
    .replace(/&#(\d+);/g, (_match, code) => {
      const value = Number(code)
      return Number.isFinite(value) ? String.fromCharCode(value) : ''
    })
}
