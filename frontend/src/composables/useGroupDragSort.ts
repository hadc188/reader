import { computed, nextTick, onBeforeUnmount, ref } from 'vue'

type Direction = 'horizontal' | 'vertical'

interface DragItem {
  groupId: number
  groupName: string
  draggable?: boolean
}

interface DragOrigin {
  pointerId: number
  pointerX: number
  pointerY: number
  startX: number
  startY: number
  scrollLeft: number
  sourceElement: HTMLElement
}

interface ItemRect {
  groupId: number
  left: number
  top: number
  right: number
  bottom: number
}

interface UseGroupDragSortOptions {
  items: () => DragItem[]
  direction: Direction
  disabled: () => boolean
  longPressOnTouch: boolean
  onTap?: (groupId: number) => void
  onReorder: (draggedId: number, targetId: number, placement: 'before' | 'after') => Promise<void> | void
}

const TOUCH_LONG_PRESS_MS = 240
const MOUSE_START_DISTANCE = 6
const EDGE_SCROLL_PX = 42
const MAX_EDGE_SCROLL_STEP = 14

/** 分组拖拽排序。桌面按下即待命, 触屏长按后拖动; 松手前只在本地
 *  显示插入位, 不修改分组顺序或写盘。 */
export function useGroupDragSort(options: UseGroupDragSortOptions) {
  const containerRef = ref<HTMLElement | null>(null)
  const draggingGroupId = ref(0)
  const dragActive = ref(false)
  const draggedName = ref('')
  const dropIndex = ref(-1)
  const ghost = ref({ x: 0, y: 0, width: 0, height: 0 })
  const itemRects = ref<ItemRect[]>([])
  let startRects: ItemRect[] = []

  let origin: DragOrigin | null = null
  let longPressTimer: number | null = null
  let autoScrollFrame: number | null = null
  let autoScrollDirection = 0

  const sortedItems = computed(() => options.items())
  const dropMarker = computed(() => {
    if (!dragActive.value || dropIndex.value < 0) return null
    const rects = itemRects.value
    if (!rects.length) return null
    const from = rects.findIndex((rect) => rect.groupId === draggingGroupId.value)
    const index = Math.min(dropIndex.value, rects.length)
    if (from < 0) return null
    if (index === from || index === from + 1) {
      const rect = rects[from]!
      return options.direction === 'horizontal'
        ? { left: rect.left, top: rect.top, height: rect.bottom - rect.top }
        : { top: rect.top, left: rect.left, width: rect.right - rect.left }
    }
    if (options.direction === 'horizontal') {
      if (index <= 0) return { left: rects[0]!.left, top: rects[0]!.top, height: rects[0]!.bottom - rects[0]!.top }
      const previous = rects[index - 1]!
      return { left: previous.right, top: previous.top, height: previous.bottom - previous.top }
    }
    if (index <= 0) return { top: rects[0]!.top, left: rects[0]!.left, width: rects[0]!.right - rects[0]!.left }
    const previous = rects[index - 1]!
    return { top: previous.bottom, left: previous.left, width: previous.right - previous.left }
  })
  const dropMarkerStyle = computed(() => {
    const marker = dropMarker.value
    const container = containerRef.value
    if (!marker || !container) return {}
    const containerRect = container.getBoundingClientRect()
    const offset = dragActive.value && draggingGroupId.value
      ? 14
      : 0
    return options.direction === 'horizontal'
      ? {
          left: `${marker.left - containerRect.left + container.scrollLeft + offset}px`,
          top: '8px',
          height: `${Math.max(18, marker.height! - 16)}px`,
        }
      : {
          top: `${marker.top - containerRect.top + container.scrollTop + offset}px`,
          left: '8px',
          width: `${Math.max(18, marker.width! - 16)}px`,
        }
  })

  function freezeRects() {
    const container = containerRef.value
    if (!container) {
      itemRects.value = []
      return
    }
    itemRects.value = [...container.querySelectorAll<HTMLElement>('[data-group-id]')]
      .map((element) => {
        const groupId = Number(element.dataset.groupId)
        const rect = element.getBoundingClientRect()
        return {
          groupId,
          left: rect.left,
          top: rect.top,
          right: rect.right,
          bottom: rect.bottom,
        }
      })
      .filter((rect) => Number.isFinite(rect.groupId))
      .sort((a, b) => (
        options.direction === 'horizontal'
          ? a.left - b.left
          : a.top - b.top
      ))
    startRects = itemRects.value
  }

  function findDropIndex(clientX: number, clientY: number) {
    const rects = itemRects.value
    if (options.direction === 'horizontal') {
      const first = rects[0]
      if (first && clientX < first.left) return 0
      for (let index = 0; index < rects.length; index += 1) {
        const rect = rects[index]!
        if (clientX < rect.left + (rect.right - rect.left) / 2) return index
      }
      return rects.length
    }
    for (let index = 0; index < rects.length; index += 1) {
      const rect = rects[index]!
      if (clientY < rect.top + (rect.bottom - rect.top) / 2) return index
    }
    return rects.length
  }

  function beginDrag() {
    const container = containerRef.value
    if (!origin || !container) return
    freezeRects()
    const startRect = itemRects.value.find((item) => item.groupId === draggingGroupId.value)
    if (!startRect) return
    dropIndex.value = itemRects.value.findIndex((item) => item.groupId === draggingGroupId.value)
    dropIndex.value = dropIndex.value < 0 ? -1 : dropIndex.value + 1
    ghost.value = {
      x: startRect.left,
      y: startRect.top,
      width: startRect.right - startRect.left,
      height: startRect.bottom - startRect.top,
    }
    dragActive.value = true
    placeGhost()
  }

  function placeGhost() {
    if (!origin) return
    const width = Math.min(
      options.direction === 'vertical'
        ? Math.min(ghost.value.width, 360)
        : ghost.value.width,
      window.innerWidth - 16,
    )
    ghost.value = {
      ...ghost.value,
      width: width || ghost.value.width,
      x: Math.max(8, Math.min(
        origin.pointerX - (options.direction === 'vertical' ? 120 : 24),
        window.innerWidth - width - 8,
      )),
      y: origin.pointerY - ghost.value.height / 2,
    }
  }

  function clearTimers() {
    if (longPressTimer !== null) {
      window.clearTimeout(longPressTimer)
      longPressTimer = null
    }
    if (autoScrollFrame !== null) {
      window.cancelAnimationFrame(autoScrollFrame)
      autoScrollFrame = null
    }
    autoScrollDirection = 0
  }

  function resetDrag() {
    clearTimers()
    origin = null
    draggingGroupId.value = 0
    dragActive.value = false
    draggedName.value = ''
    dropIndex.value = -1
    itemRects.value = []
    startRects = []
  }

  function captureDropTarget() {
    const draggedId = draggingGroupId.value
    const target = dropTarget()
    resetDrag()
    if (draggedId && target && draggedId !== target.targetId) {
      void options.onReorder(draggedId, target.targetId, target.placement)
    }
  }

  function edgeScroll() {
    const container = containerRef.value
    if (!container || !dragActive.value || !autoScrollDirection) {
      autoScrollFrame = null
      return
    }
    container.scrollLeft += autoScrollDirection * MAX_EDGE_SCROLL_STEP
    if (origin) updateDrop(origin.pointerX, origin.pointerY)
    autoScrollFrame = window.requestAnimationFrame(edgeScroll)
  }

  function updateEdgeScroll(clientX: number) {
    const container = containerRef.value
    if (!container || options.direction !== 'horizontal') {
      autoScrollDirection = 0
      return
    }
    const rect = container.getBoundingClientRect()
    autoScrollDirection = clientX < rect.left + EDGE_SCROLL_PX
      ? -1
      : clientX > rect.right - EDGE_SCROLL_PX
        ? 1
        : 0
    if (autoScrollDirection && autoScrollFrame === null) {
      autoScrollFrame = window.requestAnimationFrame(edgeScroll)
    }
  }

  function updateDrop(clientX: number, clientY: number) {
    if (!dragActive.value) return
    if (!itemRects.value.length) freezeRects()
    const container = containerRef.value
    if (container && options.direction === 'horizontal' && origin) {
      const scrollDelta = container.scrollLeft - origin.scrollLeft
      itemRects.value = startRects.map((rect) => ({
        ...rect,
        left: rect.left - scrollDelta,
        right: rect.right - scrollDelta,
      }))
    }
    dropIndex.value = findDropIndex(clientX, clientY)
    updateEdgeScroll(clientX)
  }

  function dropTarget() {
    const items = sortedItems.value
    const draggableItems = items.filter((item) => item.groupId > 0 && item.draggable !== false)
    const from = draggableItems.findIndex((item) => item.groupId === draggingGroupId.value)
    if (from < 0 || dropIndex.value < 0 || !draggableItems.length) return null
    const slot = Math.max(0, Math.min(dropIndex.value, items.length))
    if (slot <= 0) {
      const first = draggableItems[0]
      return first && first.groupId !== draggingGroupId.value
        ? { targetId: first.groupId, placement: 'before' as const }
        : null
    }
    if (slot >= items.length) {
      const last = draggableItems[draggableItems.length - 1]
      return last && last.groupId !== draggingGroupId.value
        ? { targetId: last.groupId, placement: 'after' as const }
        : null
    }
    const after = items[slot]
    if (after && after.groupId > 0 && after.draggable !== false && after.groupId !== draggingGroupId.value) {
      return { targetId: after.groupId, placement: 'before' as const }
    }
    const before = items[slot - 1]
    if (before && before.groupId > 0 && before.draggable !== false && before.groupId !== draggingGroupId.value) {
      return { targetId: before.groupId, placement: 'after' as const }
    }
    return null
  }

  function handlePointerDown(event: PointerEvent, groupId: number, groupName: string) {
    if (options.disabled() || origin) return
    if (event.pointerType === 'mouse' && event.button !== 0) return
    const item = sortedItems.value.find((entry) => entry.groupId === groupId)
    if (!item || item.draggable === false) return
    if (options.longPressOnTouch && event.pointerType === 'touch') {
      event.preventDefault()
    }
    const target = event.currentTarget as HTMLElement
    if ((event.target as Element | null)?.closest('input')) return
    target.setPointerCapture(event.pointerId)
    origin = {
      pointerId: event.pointerId,
      pointerX: event.clientX,
      pointerY: event.clientY,
      startX: event.clientX,
      startY: event.clientY,
      scrollLeft: containerRef.value?.scrollLeft ?? 0,
      sourceElement: target,
    }
    draggingGroupId.value = groupId
    draggedName.value = groupName
    clearTimers()
    if (event.pointerType === 'touch' && options.longPressOnTouch) {
      longPressTimer = window.setTimeout(() => {
        longPressTimer = null
        if (!origin) return
        beginDrag()
      }, TOUCH_LONG_PRESS_MS)
    }
  }

  function handleClick(groupId: number) {
    if (dragActive.value) return
    options.onTap?.(groupId)
  }

  function handlePointerMove(event: PointerEvent) {
    if (!origin || origin.pointerId !== event.pointerId) return
    if (event.cancelable) event.preventDefault()
    origin.pointerX = event.clientX
    origin.pointerY = event.clientY
    const distance = Math.hypot(
      event.clientX - origin.startX,
      event.clientY - origin.startY,
    )
    if (!dragActive.value) {
      if (event.pointerType !== 'touch' && distance >= MOUSE_START_DISTANCE) {
        beginDrag()
      }
    }
    if (!dragActive.value) return
    if (!origin.sourceElement.hasPointerCapture(event.pointerId)) {
      origin.sourceElement.setPointerCapture(event.pointerId)
    }
    updateDrop(event.clientX, event.clientY)
    placeGhost()
  }

  function handleTouchMove(event: TouchEvent) {
    if (!origin) return
    const touch = event.touches[0]
    if (!touch) return
    origin.pointerX = touch.clientX
    origin.pointerY = touch.clientY
    if (dragActive.value && event.cancelable) event.preventDefault()
    if (!dragActive.value) return
    updateDrop(touch.clientX, touch.clientY)
    placeGhost()
  }

  function handleTouchEnd(event: TouchEvent) {
    if (!origin) return
    if (!dragActive.value) {
      resetDrag()
      return
    }
    const touch = event.changedTouches[0]
    if (touch) {
      origin.pointerX = touch.clientX
      origin.pointerY = touch.clientY
      updateDrop(touch.clientX, touch.clientY)
    }
    captureDropTarget()
  }

  async function handlePointerUp(event: PointerEvent) {
    if (!origin || origin.pointerId !== event.pointerId) return
    if (event.pointerType === 'touch') return
    origin.pointerX = event.clientX
    origin.pointerY = event.clientY
    if (dragActive.value) updateDrop(event.clientX, event.clientY)
    const shouldReorder = dragActive.value
    const target = dropTarget()
    const draggedId = draggingGroupId.value
    const source = event.currentTarget as HTMLElement
    if (source.hasPointerCapture(event.pointerId)) source.releasePointerCapture(event.pointerId)
    resetDrag()
    if (shouldReorder && draggedId && target && draggedId !== target.targetId) {
      await nextTick()
      await options.onReorder(draggedId, target.targetId, target.placement)
    }
  }

  function handlePointerCancel(event: PointerEvent) {
    if (!origin || origin.pointerId !== event.pointerId) return
    resetDrag()
  }

  onBeforeUnmount(resetDrag)


  return {
    containerRef,
    draggingGroupId,
    dragActive,
    draggedName,
    dropMarker,
    dropMarkerStyle,
    ghost,
    handlePointerDown,
    handleClick,
    handlePointerMove,
    handleTouchMove,
    handleTouchEnd,
    handlePointerUp,
    handlePointerCancel,
    resetDrag,
  }
}
