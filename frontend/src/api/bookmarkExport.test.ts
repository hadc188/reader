import { beforeEach, describe, expect, it, vi } from 'vitest'
import { invokeEnvelope } from './invoke'
import { isTauriRuntime } from './transport'
import { saveBookmarkExportAs } from './bookmark'

vi.mock('./invoke', () => ({
  get: vi.fn(),
  post: vi.fn(),
  invokeEnvelope: vi.fn(),
}))

vi.mock('./transport', () => ({
  isTauriRuntime: vi.fn(() => true),
}))

describe('saveBookmarkExportAs', () => {
  beforeEach(() => {
    vi.mocked(invokeEnvelope).mockReset()
    vi.mocked(isTauriRuntime).mockReturnValue(true)
  })

  it('uses the native save dialog command on desktop', async () => {
    vi.mocked(invokeEnvelope).mockResolvedValue({ saved: true, path: 'C:\\bookmarks.json' })

    await expect(saveBookmarkExportAs('bookmarks.json', '[{"id":1}]', 'json')).resolves.toEqual({
      saved: true,
      path: 'C:\\bookmarks.json',
    })
    expect(invokeEnvelope).toHaveBeenCalledWith('save_bookmark_export_as', {
      req: {
        fileName: 'bookmarks.json',
        content: '[{"id":1}]',
        format: 'json',
      },
    })
  })

  it('reports a cancelled native save dialog', async () => {
    vi.mocked(invokeEnvelope).mockResolvedValue({ saved: false, cancelled: true })

    await expect(saveBookmarkExportAs('bookmarks.md', '# Bookmarks', 'md')).resolves.toEqual({
      saved: false,
      cancelled: true,
    })
  })
})
