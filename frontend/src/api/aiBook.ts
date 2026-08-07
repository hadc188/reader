import { post } from './invoke'
import type { AiBookMemory } from '../types'

export function getAiBookMemory(bookUrl: string) {
  return post<AiBookMemory | null>('/getAiBookMemory', { bookUrl }).then((r) => r.data)
}

export function saveAiBookMemory(memory: AiBookMemory) {
  return post<AiBookMemory>('/saveAiBookMemory', memory).then((r) => r.data)
}

export function deleteAiBookMemory(bookUrl: string) {
  return post<{ deleted: boolean }>('/deleteAiBookMemory', { bookUrl }).then((r) => r.data)
}
