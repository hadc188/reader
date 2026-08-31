export interface ReadingPositionSnapshot {
  chapterIndex: number
  progress: number
  paragraphIndex?: number
  paragraphProgress?: number
  updatedAt?: number
}

export function shouldPreferServerReadingPosition(
  local: ReadingPositionSnapshot | null,
  server: ReadingPositionSnapshot | null,
) {
  if (!server) return false
  if (!local) return true
  return Math.abs((server.progress || 0) - (local.progress || 0)) >= 0.02
}
