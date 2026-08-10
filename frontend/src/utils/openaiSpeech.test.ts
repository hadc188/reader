import { afterEach, describe, expect, it, vi } from 'vitest'
import {
  buildOpenAISpeechUrl,
  buildSpeechApiUrl,
  inferSpeechApiFormat,
  requestOpenAISpeechAudio,
} from './openaiSpeech'

describe('speech api compatibility', () => {
  afterEach(() => {
    vi.unstubAllGlobals()
  })

  it('migrates known saved endpoints to their explicit format', () => {
    expect(inferSpeechApiFormat('https://api.fish.audio')).toBe('fish')
    expect(inferSpeechApiFormat('https://api.elevenlabs.io')).toBe('elevenlabs')
    expect(inferSpeechApiFormat('https://example.cognitiveservices.azure.com')).toBe('azure')
    expect(inferSpeechApiFormat('https://eastus.tts.speech.microsoft.com')).toBe('azure')
  })

  it('does not duplicate the OpenAI v1 path', () => {
    expect(buildOpenAISpeechUrl('https://api.openai.com/v1')).toBe(
      'https://api.openai.com/v1/audio/speech',
    )
  })

  it('builds a voice endpoint for the ElevenLabs format', () => {
    expect(buildSpeechApiUrl('elevenlabs', 'https://api.elevenlabs.io', 'voice/id', 'opus')).toBe(
      'https://api.elevenlabs.io/v1/text-to-speech/voice%2Fid?output_format=opus_48000_128',
    )
  })

  it('maps generic settings to the Fish-compatible request format', async () => {
    const blob = new Blob(['audio'])
    const fetchMock = vi.fn().mockResolvedValue({
      ok: true,
      blob: vi.fn().mockResolvedValue(blob),
    })
    vi.stubGlobal('fetch', fetchMock)

    await requestOpenAISpeechAudio({
      apiFormat: 'fish',
      baseUrl: 'https://api.fish.audio',
      apiKey: 'test-key',
      input: '测试文本',
      model: 's2.1-pro',
      voice: 'voice-id',
      format: 'mp3',
      speed: 1.2,
    })

    expect(fetchMock).toHaveBeenCalledWith('https://api.fish.audio/v1/tts', expect.objectContaining({
      method: 'POST',
      headers: expect.objectContaining({
        Authorization: 'Bearer test-key',
        model: 's2.1-pro',
      }),
      body: JSON.stringify({
        text: '测试文本',
        format: 'mp3',
        reference_id: 'voice-id',
        prosody: { speed: 1.2 },
      }),
    }))
  })

  it('uses XML and subscription-key headers for the Azure format', async () => {
    const fetchMock = vi.fn().mockResolvedValue({
      ok: true,
      blob: vi.fn().mockResolvedValue(new Blob(['audio'])),
    })
    vi.stubGlobal('fetch', fetchMock)

    await requestOpenAISpeechAudio({
      apiFormat: 'azure',
      baseUrl: 'https://reader.cognitiveservices.azure.com',
      apiKey: 'test-key',
      input: '甲<&乙',
      model: 'zh-CN',
      voice: 'zh-CN-XiaoxiaoNeural',
      format: 'mp3',
      speed: 1,
    })

    expect(fetchMock).toHaveBeenCalledWith(
      'https://reader.cognitiveservices.azure.com/cognitiveservices/v1',
      expect.objectContaining({
        headers: expect.objectContaining({
          'Ocp-Apim-Subscription-Key': 'test-key',
          'Content-Type': 'application/ssml+xml',
        }),
        body: expect.stringContaining('甲&lt;&amp;乙'),
      }),
    )
  })
})
