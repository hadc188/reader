import { invoke } from '@tauri-apps/api/core'
import type { ApiResponse, DebugTrace } from '../types'

export type DebugStep = 'search' | 'bookInfo' | 'toc' | 'content'

export async function debugSourceStep(params: {
  bookSourceUrl: string
  step: DebugStep
  keyword?: string
  bookUrl?: string
  chapterUrl?: string
}): Promise<DebugTrace> {
  const res = await invoke<ApiResponse<DebugTrace>>('debug_source_step', { req: params })
  if (!res.isSuccess) throw new Error(res.errorMsg || '调试失败')
  return res.data as DebugTrace
}
