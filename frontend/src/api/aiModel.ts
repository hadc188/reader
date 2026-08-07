import { get, post } from './invoke'
import type { AiServerModelConfig, AiServerModelConfigResponse } from '../types'

export function getAiModelConfig() {
  return get<AiServerModelConfigResponse>('/getAiModelConfig').then((r) => r.data)
}

export function saveAiModelConfig(config: AiServerModelConfig) {
  return post<AiServerModelConfigResponse>('/saveAiModelConfig', config).then((r) => r.data)
}
