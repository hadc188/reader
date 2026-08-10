import { summarizeHttpErrorBody } from './httpError'

export const DEFAULT_OPENAI_BASE_URL = 'http://localhost:8825'

export type SpeechApiFormat = 'openai' | 'fish' | 'elevenlabs' | 'azure'
export type SpeechAudioFormat = 'mp3' | 'wav' | 'opus' | 'flac' | 'pcm'

export interface SpeechApiFormatOption {
  value: SpeechApiFormat
  label: string
  baseUrlPlaceholder: string
  modelPlaceholder: string
  voicePlaceholder: string
  modelLabel: string
  supportedFormats: SpeechAudioFormat[]
}

export const speechApiFormatOptions: SpeechApiFormatOption[] = [
  {
    value: 'openai',
    label: 'OpenAI 兼容格式',
    baseUrlPlaceholder: 'https://api.openai.com',
    modelPlaceholder: 'gpt-4o-mini-tts',
    voicePlaceholder: 'alloy',
    modelLabel: '语音模型',
    supportedFormats: ['mp3', 'wav', 'opus', 'flac', 'pcm'],
  },
  {
    value: 'fish',
    label: 'Fish 兼容格式',
    baseUrlPlaceholder: 'https://api.fish.audio',
    modelPlaceholder: 's2.1-pro',
    voicePlaceholder: 'reference_id',
    modelLabel: '语音模型',
    supportedFormats: ['mp3', 'wav', 'opus', 'pcm'],
  },
  {
    value: 'elevenlabs',
    label: 'ElevenLabs 兼容格式',
    baseUrlPlaceholder: 'https://api.elevenlabs.io',
    modelPlaceholder: 'eleven_multilingual_v2',
    voicePlaceholder: 'voice_id',
    modelLabel: '语音模型',
    supportedFormats: ['mp3', 'opus'],
  },
  {
    value: 'azure',
    label: 'Azure 兼容格式',
    baseUrlPlaceholder: 'https://资源名.cognitiveservices.azure.com',
    modelPlaceholder: 'zh-CN',
    voicePlaceholder: 'zh-CN-XiaoxiaoNeural',
    modelLabel: '语言代码',
    supportedFormats: ['mp3', 'wav', 'opus', 'pcm'],
  },
]

export function normalizeOpenAIBaseUrl(url: string) {
  return url.trim().replace(/\/+$/, '')
}

export function inferSpeechApiFormat(baseUrl: string): SpeechApiFormat {
  try {
    const url = new URL(normalizeOpenAIBaseUrl(baseUrl))
    const host = url.hostname.toLowerCase()
    const path = url.pathname.replace(/\/+$/, '').toLowerCase()
    if (host === 'fish.audio' || host.endsWith('.fish.audio') || path.endsWith('/v1/tts')) {
      return 'fish'
    }
    if (host === 'elevenlabs.io' || host.endsWith('.elevenlabs.io') || path.includes('/v1/text-to-speech')) {
      return 'elevenlabs'
    }
    if (host.endsWith('.azure.com') || host.endsWith('.speech.microsoft.com') || path.endsWith('/cognitiveservices/v1')) {
      return 'azure'
    }
  } catch {
    // Invalid saved URLs remain editable and use the default compatible format.
  }
  return 'openai'
}

export function getSpeechApiFormatOption(format: SpeechApiFormat) {
  return speechApiFormatOptions.find((option) => option.value === format) || speechApiFormatOptions[0]
}

export function buildSpeechApiUrl(
  apiFormat: SpeechApiFormat,
  baseUrl: string,
  voice: string,
  format: SpeechAudioFormat = 'mp3',
) {
  const normalized = normalizeOpenAIBaseUrl(baseUrl)
  const lower = normalized.toLowerCase()
  let endpoint = normalized

  if (apiFormat === 'fish') {
    endpoint = lower.endsWith('/v1/tts')
      ? normalized
      : lower.endsWith('/v1')
        ? `${normalized}/tts`
        : `${normalized}/v1/tts`
  } else if (apiFormat === 'elevenlabs') {
    const encodedVoice = encodeURIComponent(voice.trim())
    if (!encodedVoice) throw new Error('请填写语音音色')
    endpoint = lower.includes('/v1/text-to-speech/')
      ? normalized
      : lower.endsWith('/v1/text-to-speech')
        ? `${normalized}/${encodedVoice}`
        : lower.endsWith('/v1')
          ? `${normalized}/text-to-speech/${encodedVoice}`
          : `${normalized}/v1/text-to-speech/${encodedVoice}`
    const url = new URL(endpoint)
    url.searchParams.set('output_format', format === 'opus' ? 'opus_48000_128' : 'mp3_44100_128')
    return url.toString()
  } else if (apiFormat === 'azure') {
    endpoint = lower.endsWith('/cognitiveservices/v1')
      ? normalized
      : `${normalized}/cognitiveservices/v1`
  } else {
    endpoint = lower.endsWith('/v1/audio/speech')
      ? normalized
      : lower.endsWith('/v1')
        ? `${normalized}/audio/speech`
        : `${normalized}/v1/audio/speech`
  }

  return endpoint
}

export function buildOpenAISpeechUrl(baseUrl: string) {
  return buildSpeechApiUrl('openai', baseUrl, '')
}

export interface OpenAISpeechRequest {
  apiFormat: SpeechApiFormat
  baseUrl: string
  proxyUrl?: string
  apiKey?: string
  input: string
  model: string
  voice: string
  format?: SpeechAudioFormat
  speed?: number
  signal?: AbortSignal
}

function buildAuthHeaders(apiFormat: SpeechApiFormat, apiKey?: string): Record<string, string> {
  const key = apiKey?.trim()
  if (!key) return {}
  if (apiFormat === 'elevenlabs') return { 'xi-api-key': key }
  if (apiFormat === 'azure') return { 'Ocp-Apim-Subscription-Key': key }
  return { Authorization: `Bearer ${key}` }
}

function escapeXml(value: string) {
  return value
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;')
    .replace(/'/g, '&apos;')
}

function azureOutputFormat(format: SpeechAudioFormat) {
  const formats: Partial<Record<SpeechAudioFormat, string>> = {
    mp3: 'audio-24khz-96kbitrate-mono-mp3',
    wav: 'riff-24khz-16bit-mono-pcm',
    opus: 'ogg-24khz-16bit-mono-opus',
    pcm: 'raw-24khz-16bit-mono-pcm',
  }
  return formats[format]
}

async function readSpeechError(response: Response) {
  const fallback = `语音请求失败 (${response.status})`
  const contentType = response.headers.get('content-type') || ''

  try {
    if (contentType.includes('application/json')) {
      const data = await response.json() as {
        error?: { message?: string } | string
        detail?: string
        message?: string
      }
      if (typeof data.error === 'string') return data.error
      return data.error?.message || data.detail || data.message || fallback
    }

    const text = (await response.text()).trim()
    return summarizeHttpErrorBody(text, { fallback, status: response.status })
  } catch {
    return fallback
  }
}

export async function requestOpenAISpeechAudio({
  apiFormat,
  baseUrl,
  apiKey,
  input,
  model,
  voice,
  format,
  speed,
  signal,
}: OpenAISpeechRequest) {
  const resolvedFormat = format || 'mp3'
  const formatOption = getSpeechApiFormatOption(apiFormat)
  if (!formatOption.supportedFormats.includes(resolvedFormat)) {
    throw new Error(`所选接口格式不支持 ${resolvedFormat} 音频格式`)
  }

  let body: string
  const headers: Record<string, string> = {
    ...buildAuthHeaders(apiFormat, apiKey),
  }

  if (apiFormat === 'fish') {
    headers['Content-Type'] = 'application/json'
    headers.model = model.trim() || 's2.1-pro'
    body = JSON.stringify({
      text: input,
      format: resolvedFormat,
      ...(voice.trim() ? { reference_id: voice.trim() } : {}),
      ...(speed == null ? {} : { prosody: { speed } }),
    })
  } else if (apiFormat === 'elevenlabs') {
    headers['Content-Type'] = 'application/json'
    body = JSON.stringify({
      text: input,
      ...(model.trim() ? { model_id: model.trim() } : {}),
      ...(speed == null ? {} : { voice_settings: { speed: Math.max(0.7, Math.min(1.2, speed)) } }),
    })
  } else if (apiFormat === 'azure') {
    const outputFormat = azureOutputFormat(resolvedFormat)
    if (!outputFormat) throw new Error(`所选接口格式不支持 ${resolvedFormat} 音频格式`)
    const language = model.trim() || 'zh-CN'
    const voiceName = voice.trim()
    if (!voiceName) throw new Error('请填写语音音色')
    const rate = Math.round(((speed ?? 1) - 1) * 100)
    headers['Content-Type'] = 'application/ssml+xml'
    headers['X-Microsoft-OutputFormat'] = outputFormat
    body = `<speak version="1.0" xml:lang="${escapeXml(language)}"><voice name="${escapeXml(voiceName)}"><prosody rate="${rate >= 0 ? '+' : ''}${rate}%">${escapeXml(input)}</prosody></voice></speak>`
  } else {
    headers['Content-Type'] = 'application/json'
    body = JSON.stringify({
      model,
      input,
      voice,
      response_format: resolvedFormat,
      speed,
    })
  }

  const response = await fetch(buildSpeechApiUrl(apiFormat, baseUrl, voice, resolvedFormat), {
    method: 'POST',
    headers,
    body,
    signal,
  })

  if (!response.ok) {
    throw new Error(await readSpeechError(response))
  }

  return response.blob()
}
