import { computed, ref } from 'vue'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { useReaderSelection } from './useReaderSelection'

describe('useReaderSelection mouse drag gate', () => {
  afterEach(() => {
    vi.unstubAllGlobals()
  })

  it('blocks selection created by clicking without dragging', () => {
    const { selection, container, runScheduledFrame } = setup()

    selection.handleMouseDownSelection(mouseEvent(100, 100))
    selection.handleMouseUpSelection(mouseEvent(100, 100))
    runScheduledFrame()

    expect(container.removeAllRanges).toHaveBeenCalledTimes(2)
  })

  it('clears click selection before starting a page turn', () => {
    const { selection, container } = setup()

    selection.handleMouseDownSelection(mouseEvent(100, 100))
    selection.handleMouseUpSelection(mouseEvent(100, 100))
    selection.clearSelectionBeforePageTurn()

    expect(container.removeAllRanges).toHaveBeenCalledTimes(2)
  })

  it('clears an existing selection before starting a page turn', () => {
    const { selection, container } = setup()

    selection.clearSelectionBeforePageTurn()

    expect(container.removeAllRanges).toHaveBeenCalledOnce()
  })

  it('clears a later click selection caused by the browser selectionchange path', () => {
    const { selection, container } = setup()

    selection.handleMouseDownSelection(mouseEvent(100, 100))
    selection.handleMouseUpSelection(mouseEvent(100, 100))
    selection.updateSelectionMenu()

    expect(container.removeAllRanges).toHaveBeenCalledTimes(2)
  })

  it('clears selection again after the page transition settles', () => {
    const { selection, container, runScheduledTimer } = setup()

    selection.clearSelectionBeforePageTurn()
    selection.clearSelectionAfterPageTurn(0)
    runScheduledTimer()

    expect(container.removeAllRanges).toHaveBeenCalledTimes(2)
  })

  it('allows selection after the pointer actually drags', () => {
    const { selection, container } = setup()

    selection.handleMouseDownSelection(mouseEvent(100, 100))
    selection.handleMouseMoveSelection(mouseEvent(104, 100))
    selection.handleMouseUpSelection(mouseEvent(104, 100))

    expect(container.removeAllRanges).not.toHaveBeenCalled()
    expect(selection.suppressNextReaderClick.value).toBe(true)
  })

  it('keeps a drag selection when the pointer returns to its start point', () => {
    const { selection, container } = setup()

    selection.handleMouseDownSelection(mouseEvent(100, 100))
    selection.handleMouseMoveSelection(mouseEvent(140, 100))
    selection.handleMouseMoveSelection(mouseEvent(100, 100))
    selection.handleMouseUpSelection(mouseEvent(100, 100))

    expect(container.removeAllRanges).not.toHaveBeenCalled()
  })

  it('keeps a real drag selection when turning the page', () => {
    const { selection, container, runScheduledFrame } = setup()
    container.rangeCount = 0

    selection.handleMouseDownSelection(mouseEvent(100, 100))
    selection.handleMouseMoveSelection(mouseEvent(140, 100))
    selection.handleMouseUpSelection(mouseEvent(140, 100))
    selection.clearSelectionBeforePageTurn()
    runScheduledFrame()

    expect(container.removeAllRanges).not.toHaveBeenCalled()
  })

  it('reports an active mouse drag so page transitions can stay selection-safe', () => {
    const { selection } = setup()

    expect(selection.isDraggingSelection()).toBe(false)
    selection.handleMouseDownSelection(mouseEvent(100, 100))
    selection.handleMouseMoveSelection(mouseEvent(104, 100))
    expect(selection.isDraggingSelection()).toBe(true)
  })

  it('keeps clicks non-selectable and opens selection only after a drag', () => {
    const { selection } = setup()

    selection.handleMouseDownSelection(mouseEvent(100, 100))
    expect(selection.mouseSelectionUnlocked.value).toBe(false)

    selection.handleMouseMoveSelection(mouseEvent(104, 100))
    expect(selection.mouseSelectionUnlocked.value).toBe(true)

    selection.handleMouseUpSelection(mouseEvent(104, 100))
    expect(selection.mouseSelectionUnlocked.value).toBe(true)
  })

  it('keeps the second rapid click non-selectable but allows it to become a drag', () => {
    const { selection, container } = setup()

    selection.handleMouseDownSelection(mouseEvent(100, 100))
    selection.handleMouseUpSelection(mouseEvent(100, 100))
    const clearsAfterFirstClick = container.removeAllRanges.mock.calls.length
    selection.handleMouseDownSelection(mouseEvent(100, 100))
    expect(selection.mouseSelectionUnlocked.value).toBe(false)

    selection.handleMouseMoveSelection(mouseEvent(130, 100))
    expect(selection.mouseSelectionUnlocked.value).toBe(true)
    selection.handleMouseUpSelection(mouseEvent(130, 100))
    expect(container.removeAllRanges).toHaveBeenCalledTimes(clearsAfterFirstClick)
  })

  it('leaves non-primary mouse buttons to the browser', () => {
    const { selection, container } = setup()

    selection.handleMouseDownSelection(mouseEvent(100, 100, 1))
    selection.handleMouseUpSelection(mouseEvent(100, 100, 1))

    expect(container.removeAllRanges).not.toHaveBeenCalled()
  })

  it('leaves touch-generated compatibility mouse events alone', () => {
    const { selection, container } = setup()

    selection.handleMouseDownSelection(mouseEvent(100, 100, 0, true))
    selection.handleMouseUpSelection(mouseEvent(100, 100))

    expect(container.removeAllRanges).not.toHaveBeenCalled()
  })
})

function setup() {
  const scheduledFrames: Array<() => void> = []
  const scheduledTimers: Array<() => void> = []
  const container = {
    contains: () => true,
    rangeCount: 1,
    removeAllRanges: vi.fn(),
  }
  vi.stubGlobal('window', {
    getSelection: () => container,
    setTimeout: (callback: () => void) => {
      scheduledTimers.push(callback)
      return scheduledTimers.length
    },
    clearTimeout: vi.fn(),
    requestAnimationFrame: (callback: () => void) => {
      scheduledFrames.push(callback)
      return scheduledFrames.length
    },
  })
  vi.stubGlobal('navigator', {
    maxTouchPoints: 0,
    userAgent: 'test',
  })

  const selection = useReaderSelection(
    {} as never,
    {} as never,
    computed(() => ({ selectAction: 'ignore' as const })),
    ref({} as HTMLElement),
  )
  return {
    selection,
    container,
    runScheduledFrame: () => scheduledFrames.shift()?.(),
    runScheduledTimer: () => scheduledTimers.shift()?.(),
  }
}

function mouseEvent(clientX: number, clientY: number, button = 0, touchGenerated = false) {
  return {
    clientX,
    clientY,
    button,
    defaultPrevented: false,
    sourceCapabilities: { firesTouchEvents: touchGenerated },
  } as unknown as MouseEvent
}
