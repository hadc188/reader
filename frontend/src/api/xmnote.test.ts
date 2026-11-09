import { beforeEach, describe, expect, it, vi } from 'vitest'
import { invokeEnvelope } from './invoke'
import { isTauriRuntime } from './transport'
import { sendXmnoteBooks } from './xmnote'

vi.mock('./invoke', () => ({
  invokeEnvelope: vi.fn(),
}))

vi.mock('./transport', () => ({
  isTauriRuntime: vi.fn(() => true),
}))

describe('xmnote api', () => {
  beforeEach(() => {
    vi.mocked(invokeEnvelope).mockReset()
    vi.mocked(isTauriRuntime).mockReturnValue(true)
  })

  it('sends books through the envelope command with a req wrapper', async () => {
    const result = { sent: 1, failed: 0, errors: [] }
    const books = [{ title: '书名', type: 1 as const, locationUnit: 0 as const }]
    vi.mocked(invokeEnvelope).mockResolvedValue(result)

    await expect(sendXmnoteBooks('http://192.168.1.8:8080/send', books)).resolves.toEqual(result)
    expect(invokeEnvelope).toHaveBeenCalledWith('send_xmnote_books', {
      req: {
        endpoint: 'http://192.168.1.8:8080/send',
        books,
      },
    })
  })

  it('uses browser fetch when the app is running in web mode', async () => {
    vi.mocked(isTauriRuntime).mockReturnValue(false)
    const fetchMock = vi.fn().mockResolvedValue({
      ok: true,
      status: 200,
      text: vi.fn().mockResolvedValue('{"code":200,"message":"success"}'),
    })
    vi.stubGlobal('fetch', fetchMock)

    const books = [{ title: '书名', type: 1 as const, locationUnit: 0 as const }]
    await expect(sendXmnoteBooks('http://192.168.1.8:8080/send', books)).resolves.toEqual({
      sent: 1,
      failed: 0,
      errors: [],
    })
    expect(fetchMock).toHaveBeenCalledWith('http://192.168.1.8:8080/send', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify(books[0]),
    })
    vi.unstubAllGlobals()
  })
})
