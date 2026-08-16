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

export interface BookReadingStats {
  bookUrl: string
  bookName: string
  seconds: number
  characters: number
  lastReadDate: string
}

export function addReadingStats(params: {
  seconds?: number
  characters?: number
  date?: string
  bookUrl?: string
  bookName?: string
  bookAuthor?: string
}): Promise<{ saved: boolean }> {
  return invokeEnvelope('add_reading_stats', { req: params })
}

export function getReadingStatsDaily(start: string, end: string): Promise<DailyReadingStats[]> {
  return invokeEnvelope('get_reading_stats_daily', { start, end })
}

export function getReadingStatsSummary(): Promise<ReadingStatsSummary> {
  return invokeEnvelope('get_reading_stats_summary', {})
}

export function getReadingStatsByBook(start: string, end: string): Promise<BookReadingStats[]> {
  return invokeEnvelope('get_reading_stats_by_book', { start, end })
}

export function deleteReadingStatsByBook(bookUrl: string): Promise<{ deleted: number }> {
  return invokeEnvelope('delete_reading_stats_by_book', { bookUrl })
}
