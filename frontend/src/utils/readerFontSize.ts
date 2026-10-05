export const READER_FONT_SIZE_MIN = 12
export const READER_FONT_SIZE_MAX = 50
export const READER_FONT_SIZE_STEP = 1

/** 字重范围与设置面板的滑块一致(100/200/.../900)。 */
export const READER_FONT_WEIGHT_MIN = 100
export const READER_FONT_WEIGHT_MAX = 900
export const READER_FONT_WEIGHT_STEP = 100

/** 把数值夹到 [min, max]。 */
function clamp(value: number, min: number, max: number) {
  return Math.max(min, Math.min(max, value))
}

/**
 * 按步长加/减字号并夹在合法范围内。
 *
 * 迷你模式的字号加减与设置面板共用这套边界; 抽出来是为了能被单测覆盖,
 * 避免"到界后还能继续加减"这类边界问题只靠界面观察。
 */
export function stepReaderFontSize(current: number, delta: number): number {
  if (!Number.isFinite(current) || !Number.isFinite(delta)) return READER_FONT_SIZE_MIN
  return clamp(current + delta, READER_FONT_SIZE_MIN, READER_FONT_SIZE_MAX)
}

/** 按步长加/减字重并夹在合法范围内。 */
export function stepReaderFontWeight(current: number, delta: number): number {
  if (!Number.isFinite(current) || !Number.isFinite(delta)) return READER_FONT_WEIGHT_MIN
  return clamp(current + delta, READER_FONT_WEIGHT_MIN, READER_FONT_WEIGHT_MAX)
}

interface ReaderFontSizeWheelInput {
  ctrlKey: boolean
  deltaY: number
}

interface ReaderFontSizeWheelEvent extends ReaderFontSizeWheelInput {
  preventDefault: () => void
}

export function getReaderFontSizeFromWheel(
  currentSize: number,
  input: ReaderFontSizeWheelInput,
): number | null {
  if (!input.ctrlKey || !Number.isFinite(input.deltaY) || input.deltaY === 0) {
    return null
  }

  const direction = input.deltaY < 0 ? 1 : -1
  return Math.max(
    READER_FONT_SIZE_MIN,
    Math.min(READER_FONT_SIZE_MAX, currentSize + direction * READER_FONT_SIZE_STEP),
  )
}

export function handleReaderFontSizeWheel(
  currentSize: number,
  event: ReaderFontSizeWheelEvent,
  updateFontSize: (fontSize: number) => void,
): boolean {
  const nextFontSize = getReaderFontSizeFromWheel(currentSize, event)
  if (nextFontSize === null) return false

  event.preventDefault()
  if (nextFontSize !== currentSize) {
    updateFontSize(nextFontSize)
  }
  return true
}
