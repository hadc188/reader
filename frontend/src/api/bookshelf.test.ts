import { describe, expect, it, vi } from 'vitest'
import { readerOrigin } from './scheme'
import { get, invokeEnvelope } from './invoke'
import { getBookshelfWithCacheInfo, getCoverUrl } from './bookshelf'

// bookshelf.ts 依赖 invoke 层, 这里只测纯函数 getCoverUrl, 不需要真的发请求。
vi.mock('./invoke', () => ({
  get: vi.fn(),
  post: vi.fn(),
  invokeEnvelope: vi.fn(),
  invokeRaw: vi.fn(),
  invokeUpload: vi.fn(),
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

  /** 加密封面要靠书源反查 `coverDecodeJs`, 所以书籍所属书源必须随请求带上
   *  (issue #23)。少了它, 后端只能退化成按封面地址翻书架。 */
  it('carries the book source so encrypted covers can be decoded', () => {
    expect(getCoverUrl('https://example.com/c.jpg', 'https://18mh.net/novel'))
      .toBe(
        `${readerOrigin}/cover?path=${encodeURIComponent('https://example.com/c.jpg')}` +
          `&source=${encodeURIComponent('https://18mh.net/novel')}`,
      )
  })

  /** 本地 PDF 封面与书源无关, 不该多带一个用不上的参数。 */
  it('does not attach a source to a local pdf cover', () => {
    expect(getCoverUrl('/local-pdf/abc/cover.jpg', 'https://18mh.net/novel'))
      .toBe(`${readerOrigin}/cover?path=${encodeURIComponent('/local-pdf/abc/cover.jpg')}`)
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

describe('getBookshelfWithCacheInfo', () => {
  it('passes prefetch=false in the browser so the reader skips the source sweep', async () => {
    // 单测环境不是 Tauri, 走 HTTP 分支: prefetch 必须作为 query 参数发出。
    vi.mocked(get).mockResolvedValue({ data: [] } as never)

    await getBookshelfWithCacheInfo({ prefetch: false })

    expect(get).toHaveBeenCalledWith('/getShelfBookWithCacheInfo', { params: { prefetch: false } })
  })

  it('defaults to prefetch=true so the shelf page still warms the toc cache', async () => {
    vi.mocked(get).mockResolvedValue({ data: [] } as never)

    await getBookshelfWithCacheInfo()

    expect(get).toHaveBeenCalledWith('/getShelfBookWithCacheInfo', { params: { prefetch: true } })
  })

  it('uses the top-level prefetch argument on desktop, not the generic req wrapper', async () => {
    // 桌面端的命令签名是 `prefetch: Option<bool>`(顶层参数)。塞进 `{req: …}`
    // 会让 invoke 反序列化失败, 所以这里断言参数形状。
    vi.stubGlobal('window', { __TAURI_INTERNALS__: {} })
    vi.mocked(invokeEnvelope).mockResolvedValue([] as never)
    try {
      await getBookshelfWithCacheInfo({ prefetch: false })
      expect(invokeEnvelope).toHaveBeenCalledWith('get_shelf_book_with_cache_info', { prefetch: false })
    } finally {
      vi.unstubAllGlobals()
    }
  })
})
