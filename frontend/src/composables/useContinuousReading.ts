import { nextTick, ref } from 'vue'
import type { ComputedRef, Ref } from 'vue'
import type { useReaderStore } from '../stores/reader'

type ReaderStore = ReturnType<typeof useReaderStore>

export interface ContinuousChapterItem {
  index: number
  title: string
  content: string
  html: string
}

export function useContinuousReading(
  store: ReaderStore,
  renderChapterHtml: (rawText: string) => string,
  isContinuousMode: ComputedRef<boolean>,
  hideReadChaptersMode: ComputedRef<boolean>,
  scrollContainerRef: Ref<HTMLElement | undefined>,
) {
  const continuousChapters = ref<ContinuousChapterItem[]>([])
  const continuousLoadingNext = ref(false)
  const continuousLoadingPrev = ref(false)
  const suppressContinuousSync = ref(false)
  const previousAutoLoadArmed = ref(false)
  let continuousStateSyncTimer: number | null = null
  let continuousGeneration = 0

  function shouldHideChapter(index: number, keepIndex?: number) {
    if (!hideReadChaptersMode.value) return false
    if (typeof keepIndex === 'number' && index === keepIndex) return false
    return store.isChapterRead(index)
  }

  function findNextVisibleIndex(startIndex: number, keepIndex?: number) {
    for (let index = startIndex; index < store.chapters.length; index += 1) {
      if (!shouldHideChapter(index, keepIndex)) {
        return index
      }
    }
    return -1
  }

  async function pruneReadChapters(targetIndex = store.currentIndex) {
    if (!hideReadChaptersMode.value) return
    const kept = continuousChapters.value.filter((chapter) => chapter.index >= targetIndex)
    if (kept.length === continuousChapters.value.length) return

    const firstKept = kept[0]
    const container = scrollContainerRef.value
    const anchorSelector = firstKept
      ? `.continuous-chapter[data-chapter-index="${firstKept.index}"]`
      : ''
    const anchorBefore = anchorSelector
      ? (container?.querySelector(anchorSelector) as HTMLElement | null)
      : null
    const previousAnchorOffset = anchorBefore?.offsetTop ?? null

    continuousChapters.value = kept
    await nextTick()
    if (previousAnchorOffset == null || !container || !anchorSelector) return
    // overflow-anchor: none disabled native anchoring, so pin the first kept
    // chapter when the read sections above it are pruned. Without this the
    // viewport drops toward the chapter tail and the position restore then
    // visibly scrolls it back to the top.
    const anchor = container.querySelector(anchorSelector) as HTMLElement | null
    if (!anchor) return
    const anchorDelta = anchor.offsetTop - previousAnchorOffset
    if (anchorDelta === 0) return
    container.scrollTo({
      top: Math.max(0, container.scrollTop + anchorDelta),
      behavior: 'instant',
    })
  }

  async function buildContinuousChapter(index: number, forceRefresh = false) {
    const chapter = store.chapters[index]
    if (!chapter) return null
    const chapterContent = await store.fetchChapterContent(index, forceRefresh)
    if (chapterContent == null) return null
    return {
      index,
      title: chapter.title,
      content: chapterContent,
      html: renderChapterHtml(chapterContent),
    } satisfies ContinuousChapterItem
  }

  function syncContinuousChapterHtml() {
    continuousChapters.value = continuousChapters.value.map((chapter) => ({
      ...chapter,
      html: renderChapterHtml(chapter.content),
    }))
  }

  function getContinuousChapter(index: number) {
    return continuousChapters.value.find((chapter) => chapter.index === index) || null
  }

  function setContinuousActiveChapter(index: number, chapterContent: string, progress: number) {
    suppressContinuousSync.value = true
    store.setActiveChapterState(index, chapterContent, progress)
    store.markChapterAsRead(index)
    void store.persistProgress(index)
    if (continuousStateSyncTimer) {
      clearTimeout(continuousStateSyncTimer)
    }
    continuousStateSyncTimer = window.setTimeout(() => {
      suppressContinuousSync.value = false
    }, 0)
  }

  async function initializeContinuousChapters(
    targetIndex = store.currentIndex,
    smooth = false,
    includePrevious = false,
  ) {
    if (!isContinuousMode.value || !store.chapters[targetIndex]) return

    const generation = ++continuousGeneration
    previousAutoLoadArmed.value = false
    const previousIndex = targetIndex - 1
    const currentPromise = buildContinuousChapter(targetIndex)
    const previousPromise = includePrevious && !hideReadChaptersMode.value && previousIndex >= 0
      ? Promise.resolve(getContinuousChapter(previousIndex) ?? buildContinuousChapter(previousIndex).catch(() => null))
      : Promise.resolve(null)
    const [current, previous] = await Promise.all([currentPromise, previousPromise])
    if (!current) return
    if (generation !== continuousGeneration || !isContinuousMode.value) return

    continuousChapters.value = previous ? [previous, current] : [current]
    setContinuousActiveChapter(targetIndex, current.content, 0)

    await nextTick()
    if (generation !== continuousGeneration || !isContinuousMode.value) return
    scrollToContinuousChapter(targetIndex, smooth)

    const nextIndex = hideReadChaptersMode.value
      ? findNextVisibleIndex(targetIndex + 1, targetIndex)
      : targetIndex + 1
    if (nextIndex < 0) return

    void (async () => {
      const next = await buildContinuousChapter(nextIndex).catch(() => null)
      if (!next || generation !== continuousGeneration || !isContinuousMode.value) return
      if (continuousChapters.value.some((chapter) => chapter.index === next.index)) return
      continuousChapters.value = [...continuousChapters.value, next]
    })()
  }

  async function syncContinuousToStoreState() {
    if (!isContinuousMode.value || suppressContinuousSync.value || store.loading || !store.chapters[store.currentIndex]) return

    pruneReadChapters(store.currentIndex)
    const current = getContinuousChapter(store.currentIndex)
    if (current) {
      if (current.content !== store.content) {
        current.content = store.content
        current.html = renderChapterHtml(store.content)
      }
      return
    }

    await initializeContinuousChapters(store.currentIndex, false)
  }

  async function loadContinuousNext() {
    if (continuousLoadingNext.value || !continuousChapters.value.length) return
    const generation = continuousGeneration
    continuousLoadingNext.value = true
    try {
      // A fast scroll can reach the bottom while a chapter is being fetched.
      // Keep filling the tail until the viewport has enough content again;
      // adding a section does not reliably emit another scroll event.
      while (continuousChapters.value.length) {
        const last = continuousChapters.value[continuousChapters.value.length - 1]
        const nextIndex = hideReadChaptersMode.value
          ? findNextVisibleIndex(last.index + 1, store.currentIndex)
          : last.index + 1
        if (nextIndex < 0 || nextIndex >= store.chapters.length) break

        const next = await buildContinuousChapter(nextIndex)
        if (generation !== continuousGeneration || !next || getContinuousChapter(next.index)) break
        continuousChapters.value = [...continuousChapters.value, next]
        await nextTick()

        const container = scrollContainerRef.value
        const remaining = container
          ? container.scrollHeight - (container.scrollTop + container.clientHeight)
          : Number.POSITIVE_INFINITY
        if (remaining >= 480) break
      }
    } finally {
      continuousLoadingNext.value = false
    }
  }

  async function loadContinuousPrev() {
    if (hideReadChaptersMode.value) return
    if (continuousLoadingPrev.value || !continuousChapters.value.length) return
    previousAutoLoadArmed.value = false
    const generation = continuousGeneration
    const first = continuousChapters.value[0]
    const prevIndex = first.index - 1
    if (prevIndex < 0) return

    const container = scrollContainerRef.value
    const anchorSelector = `.continuous-chapter[data-chapter-index="${first.index}"]`
    const anchorBefore = container?.querySelector(anchorSelector) as HTMLElement | null
    const previousAnchorOffset = anchorBefore?.offsetTop ?? 0

    continuousLoadingPrev.value = true
    try {
      const prev = await buildContinuousChapter(prevIndex).catch(() => null)
      if (generation !== continuousGeneration || !prev || getContinuousChapter(prev.index)) return

      continuousChapters.value = [prev, ...continuousChapters.value]
      await nextTick()
      if (typeof window !== 'undefined') {
        await new Promise<void>((resolve) => window.requestAnimationFrame(() => resolve()))
      }
      if (generation !== continuousGeneration || !isContinuousMode.value) return
      if (container) {
        const anchor = container.querySelector(anchorSelector) as HTMLElement | null
        // The scroll container disables native scroll anchoring and sets CSS
        // scroll-behavior: smooth, so pin the viewport back to the content the
        // user was reading with an explicit instant jump: how far the anchor
        // moved equals the height inserted above it. Measured off the CURRENT
        // scrollTop so scrolling during the fetch is not fought.
        const anchorDelta = anchor ? anchor.offsetTop - previousAnchorOffset : 0
        container.scrollTo({
          top: Math.max(0, container.scrollTop + anchorDelta),
          behavior: 'instant',
        })
      }
    } finally {
      continuousLoadingPrev.value = false
    }
  }

  async function ensureContinuousChapterLoaded(index: number) {
    if (getContinuousChapter(index)) return
    if (!continuousChapters.value.length) {
      await initializeContinuousChapters(index, false)
      return
    }

    // loadContinuousPrev/Next swallow fetch failures without mutating the
    // list, so stop as soon as an iteration stops making progress.
    while (continuousChapters.value[0] && index < continuousChapters.value[0].index) {
      const firstIndexBefore = continuousChapters.value[0].index
      await loadContinuousPrev()
      if ((continuousChapters.value[0]?.index ?? firstIndexBefore) === firstIndexBefore) break
    }

    while (
      continuousChapters.value[continuousChapters.value.length - 1]
      && index > continuousChapters.value[continuousChapters.value.length - 1].index
    ) {
      const lastIndexBefore = continuousChapters.value[continuousChapters.value.length - 1].index
      await loadContinuousNext()
      if (
        (continuousChapters.value[continuousChapters.value.length - 1]?.index ?? lastIndexBefore)
        === lastIndexBefore
      ) break
    }
  }

  function getContinuousSections() {
    const container = scrollContainerRef.value
    if (!container) return [] as HTMLElement[]
    return Array.from(container.querySelectorAll('.continuous-chapter')) as HTMLElement[]
  }

  function scrollToContinuousChapter(index: number, smooth = true) {
    const container = scrollContainerRef.value
    if (!container) return
    const section = container.querySelector(`.continuous-chapter[data-chapter-index="${index}"]`) as HTMLElement | null
    if (!section) return
    container.scrollTo({
      top: Math.max(0, section.offsetTop),
      // 'auto' would follow the container's CSS scroll-behavior: smooth and
      // animate the jump across the prepended chapter.
      behavior: smooth ? 'smooth' : 'instant',
    })
  }

  function clearContinuousChapters() {
    continuousGeneration += 1
    previousAutoLoadArmed.value = false
    continuousChapters.value = []
  }

  function armPreviousAutoLoad() {
    previousAutoLoadArmed.value = true
  }

  function disposeContinuousReading() {
    if (continuousStateSyncTimer) {
      clearTimeout(continuousStateSyncTimer)
      continuousStateSyncTimer = null
    }
  }

  return {
    continuousChapters,
    continuousLoadingNext,
    continuousLoadingPrev,
    previousAutoLoadArmed,
    suppressContinuousSync,
    syncContinuousChapterHtml,
    getContinuousChapter,
    setContinuousActiveChapter,
    initializeContinuousChapters,
    syncContinuousToStoreState,
    loadContinuousNext,
    loadContinuousPrev,
    armPreviousAutoLoad,
    ensureContinuousChapterLoaded,
    getContinuousSections,
    scrollToContinuousChapter,
    pruneReadChapters,
    clearContinuousChapters,
    disposeContinuousReading,
  }
}
