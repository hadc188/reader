export const READER_FONT_SIZE_MIN = 12
export const READER_FONT_SIZE_MAX = 50
export const READER_FONT_SIZE_STEP = 1

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
