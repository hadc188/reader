export const SEARCH_CONCURRENCY_DEFAULT = 24
export const SEARCH_CONCURRENCY_MIN = 1
export const SEARCH_CONCURRENCY_MAX = 999

export function normalizeSearchConcurrency(value: unknown): number {
  if (value === null || value === undefined || value === '') {
    return SEARCH_CONCURRENCY_DEFAULT
  }
  const parsed = typeof value === 'number' ? value : Number(value)
  if (!Number.isFinite(parsed)) return SEARCH_CONCURRENCY_DEFAULT
  return Math.min(
    SEARCH_CONCURRENCY_MAX,
    Math.max(SEARCH_CONCURRENCY_MIN, Math.trunc(parsed)),
  )
}
