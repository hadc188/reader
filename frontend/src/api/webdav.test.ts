import { describe, expect, it } from 'vitest'
import { createWebdavFileBlob, decodeWebdavFileText } from './webdav'

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
})
