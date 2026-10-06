import { describe, expect, it, vi } from 'vitest'
import { readerOrigin } from './scheme'
import { getCoverUrl } from './bookshelf'

// bookshelf.ts 依赖 invoke 层, 这里只测纯函数 getCoverUrl, 不需要真的发请求。
vi.mock('./invoke', () => ({
  get: vi.fn(),
  post: vi.fn(),
  invokeEnvelope: vi.fn(),
  invokeRaw: vi.fn(),
}))

describe('getCoverUrl', () => {
  it('returns empty for a missing cover', () => {
    expect(getCoverUrl(undefined)).toBe('')
    expect(getCoverUrl('')).toBe('')
  })

  /** 本应用自存的封面(书籍信息编辑功能上传的)已经是可直接访问的地址。
   *  它同样以 http 开头 —— 若被当成远端地址再包一层 /cover, 封面必然 404。 */
  it('passes through a cover this app stored, without wrapping it again', () => {
    const stored = `${readerOrigin}/files?path=default/covers/deadbeef.png`

    expect(getCoverUrl(stored)).toBe(stored)
  })

  it('still proxies a remote source cover', () => {
    expect(getCoverUrl('https://example.com/c.jpg'))
      .toBe(`${readerOrigin}/cover?path=${encodeURIComponent('https://example.com/c.jpg')}`)
  })

  it('proxies a root-relative source path', () => {
    expect(getCoverUrl('/covers/1.jpg'))
      .toBe(`${readerOrigin}/cover?path=${encodeURIComponent('/covers/1.jpg')}`)
  })

  it('maps a local epub asset to the epub route', () => {
    const url = getCoverUrl('/reader3/localEpubAsset?bookUrl=book%3A%2F%2Fx&path=img%2Fa.png')

    expect(url).toBe(`${readerOrigin}/epub?bookUrl=${encodeURIComponent('book://x')}&path=${encodeURIComponent('img/a.png')}`)
  })

  it('returns anything else untouched', () => {
    expect(getCoverUrl('data:image/png;base64,AAA')).toBe('data:image/png;base64,AAA')
  })

  /** 字体等别的本地目录不该被误当成封面透传 —— 那条透传只针对本应用协议前缀,
   *  这里确认前缀判断没有宽松到吞掉任意相对路径。 */
  it('does not treat an arbitrary relative path as a local asset', () => {
    expect(getCoverUrl('covers/local.png')).toBe('covers/local.png')
  })
})
