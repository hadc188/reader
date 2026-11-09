/** 书签位置在 Legado 里按 0..10000 编码；旧数据也兼容 0..1 的小数。 */
export function normalizeBookmarkPosition(position?: number): number {
  if (typeof position !== 'number' || !Number.isFinite(position) || position <= 0) return 0
  if (position > 1) return Math.max(0, Math.min(1, position / 10000))
  return Math.max(0, Math.min(1, position))
}
