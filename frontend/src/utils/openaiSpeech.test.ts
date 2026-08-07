import { afterEach, describe, expect, it, vi } from 'vitest'
import { invoke } from '@tauri-apps/api/core'
import { requestOpenAISpeechAudio } from './openaiSpeech'

vi.mock('@tauri-apps/api/core', () => ({
  invoke: vi.fn(),
}))

afterEach(() => {
  vi.clearAllMocks()
})

describe('openaiSpeech', () => {
  it('routes server configured speech through aiProxy without browser credentials', async () => {
    vi.mocked(invoke).mockResolvedValue({
      status: 200,
      contentType: 'audio/mpeg',
      body: new Uint8Array([97, 117, 100, 105, 111]), // "audio"
    })

    const blob = await requestOpenAISpeechAudio({
      source: 'server',
      baseUrl: '',
      apiKey: 'browser-key',
      input: '你好',
      model: 'browser-model',
      voice: 'browser-voice',
      format: 'mp3',
      speed: 1,
    })

    expect(blob.type).toBe('audio/mpeg')
    expect(invoke).toHaveBeenCalledWith(
      'ai_proxy',
      expect.objectContaining({
        req: expect.objectContaining({
          useServerConfig: true,
          kind: 'speech',
          path: '/v1/audio/speech',
          body: expect.objectContaining({
            input: '你好',
            response_format: 'mp3',
            speed: 1,
          }),
        }),
      }),
    )
    const payload = JSON.stringify(vi.mocked(invoke).mock.calls[0]?.[1])
    expect(payload).not.toContain('browser-key')
  })
})
