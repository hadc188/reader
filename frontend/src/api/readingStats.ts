import { invokeEnvelope } from './invoke'

export interface DailyReadingStats {
  date: string
  seconds: number
  characters: number
}

export interface ReadingStatsSummary {
  totalSeconds: number
  totalCharacters: number
  activeDays: number
}

export function addReadingStats(params: {
  seconds?: number
  characters?: number
  date?: string
}): Promise<{ saved: boolean }> {
  return invokeEnvelope('add_reading_stats', { req: params })
}

export function getReadingStatsDaily(start: string, end: string): Promise<DailyReadingStats[]> {
  return invokeEnvelope('get_reading_stats_daily', { start, end })
}

export function getReadingStatsSummary(): Promise<ReadingStatsSummary> {
  return invokeEnvelope('get_reading_stats_summary', {})
}
