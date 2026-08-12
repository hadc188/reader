import { describe, expect, it, vi } from 'vitest'
import { createWebdavFileBlob, decodeWebdavFileText, syncLegadoBookProgress } from './webdav'
import { invokeEnvelope } from './invoke'

vi.mock('./invoke', () => ({
  get: vi.fn(),
  post: vi.fn(),
  invokeEnvelope: vi.fn(),
  invokeRaw: vi.fn(),
}))

describe('webdav binary file conversion', () => {
  it('decodes an old JSON backup with UTF-8 text intact', () => {
    const raw = '{"version":1,"bookName":"神通者"}'
    const bytes = Array.from(new TextEncoder().encode(raw))

    expect(decodeWebdavFileText({ bytes })).toBe(raw)
  })

  it('creates a downloadable blob from the binary response', async () => {
    const raw = '备份内容'
    const bytes = new TextEncoder().encode(raw)
    const blob = createWebdavFileBlob({ bytes, content_type: 'application/json' })

    expect(blob.type).toBe('application/json')
    expect(await blob.text()).toBe(raw)
  })

  it('wraps Legado progress in the request object expected by Tauri', async () => {
    vi.mocked(invokeEnvelope).mockResolvedValue({ configured: true, uploaded: true })
    const config = {
      url: 'https://dav.example.test/',
      account: 'reader',
      password: 'secret',
      directory: 'legado',
    }
    const progress = {
      name: '测试书籍',
      author: '测试作者',
      durChapterIndex: 3,
      durChapterPos: 120,
      durChapterTime: 123456,
      durChapterTitle: '第四章',
    }

    await syncLegadoBookProgress(config, progress, true, true)

    expect(invokeEnvelope).toHaveBeenCalledWith('sync_legado_book_progress', {
      req: { config, progress, allowUpload: true, forceUpload: true },
    })
  })
})
