import { summarizeHttpErrorBody } from './httpError'

export const DEFAULT_OPENAI_BASE_URL = 'http://localhost:8825'

export function normalizeOpenAIBaseUrl(url: string) {
  return url.trim().replace(/\/+$/, '')
}

export function buildOpenAISpeechUrl(baseUrl: string) {
  return `${normalizeOpenAIBaseUrl(baseUrl)}/v1/audio/speech`
}

export interface OpenAISpeechRequest {
  baseUrl: string
  apiKey?: string
  input: string
  model: string
  voice: string
  format?: string
  speed?: number
  signal?: AbortSignal
}

function buildAuthHeaders(apiKey?: string) {
  const headers: Record<string, string> = {}
  if (!apiKey?.trim()) return headers
  headers.Authorization = `Bearer ${apiKey.trim()}`
  return headers
}

async function readSpeechError(response: Response) {
  const fallback = `语音请求失败 (${response.status})`
  const contentType = response.headers.get('content-type') || ''

  try {
    if (contentType.includes('application/json')) {
      const data = await response.json() as {
        error?: {
          message?: string
        }
      }
      return data.error?.message || fallback
    }

    const text = (await response.text()).trim()
    return summarizeHttpErrorBody(text, { fallback, status: response.status })
  } catch {
    return fallback
  }
}

export async function requestOpenAISpeechAudio({
  baseUrl,
  apiKey,
  input,
  model,
  voice,
  format,
  speed,
  signal,
}: OpenAISpeechRequest) {
  const body = {
    model,
    input,
    voice,
    response_format: format || 'mp3',
    speed,
  }
  const response = await fetch(buildOpenAISpeechUrl(baseUrl), {
    method: 'POST',
    headers: {
      'Content-Type': 'application/json',
      ...buildAuthHeaders(apiKey),
    },
    body: JSON.stringify(body),
    signal,
  })

  if (!response.ok) {
    throw new Error(await readSpeechError(response))
  }

  return response.blob()
}
