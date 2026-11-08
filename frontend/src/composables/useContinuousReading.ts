import { nextTick, ref } from 'vue'
import type { ComputedRef, Ref } from 'vue'
import type { useReaderStore } from '../stores/reader'

type ReaderStore = ReturnType<typeof useReaderStore>

/** 「正在加载下一章…」的延迟显示时长: 短到不显得迟钝, 长到足以让命中预载的
 *  微任务路径在提示渲染出来之前就结束。 */
const CONTINUOUS_LOADING_INDICATOR_DELAY_MS = 150

/**
 * 连续阅读常驻的章节窗口：当前章之前 / 之后各保留的章节数。
 *
 * 连续阅读会把章节一节节追加进 DOM，默认模式又没有「隐藏已读」那样的裁剪，
 * 读几百章后节点数、正文 HTML 与渲染层样式都会一直累积。这里只保留当前章
 * 附近一段窗口，向下滚动时旧章节被裁掉；向上回看时由 `loadContinuousPrev`
 * 重新取回（正文有缓存，代价很小）。
 *
 * 前后各 2 章足够覆盖：一次翻页/惯性滚动跨过的章节、以及返回上一章时的衔接。
 * 窗口太小会导致来回滚动时反复重建 DOM；太大则失去裁剪意义。
 */
const CONTINUOUS_KEEP_BEHIND = 2
const CONTINUOUS_KEEP_AHEAD = 2

export interface ContinuousChapterItem {
  index: number
  title: string
  content: string
  html: string
  /**
   * 卷标题章（`第一卷` 之类）。正文天然为空，连续阅读里只渲染居中的标题块，
   * 不画正文区也不占滚动高度 —— 否则用户会看到一个"空章节"卡在章节之间
   * （issue #33 截图里的重复标题就是这么来的：标题渲染一次，空正文区又把
   * 同一行带出来）。
   */
  isVolume: boolean
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
  // 「正在加载下一章…」的延迟显示: 命中预载时取正文只是一个微任务, 提示却会被
  // 渲染出来闪一下 —— 那正是用户眼里"预加载没生效"的观感。真正要等网络时,
  // 延迟远小于一次请求的耗时, 不会让人觉得没反应。
  let continuousNextIndicatorTimer: number | null = null
  // 用独立的布尔量守重入, 而不是 continuousLoadingNext —— 后者现在带 150ms
  // 延迟显示, 不能兼作"正在填充尾部"的同步标志(否则滚动事件会重复进入本函数)。
  let continuousNextInFlight = false

  function showContinuousNextIndicatorSoon() {
    if (continuousLoadingNext.value || continuousNextIndicatorTimer !== null) return
    continuousNextIndicatorTimer = window.setTimeout(() => {
      continuousNextIndicatorTimer = null
      continuousLoadingNext.value = true
    }, CONTINUOUS_LOADING_INDICATOR_DELAY_MS)
  }

  function hideContinuousNextIndicator() {
    if (continuousNextIndicatorTimer !== null) {
      clearTimeout(continuousNextIndicatorTimer)
      continuousNextIndicatorTimer = null
    }
    continuousLoadingNext.value = false
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

  /**
   * 把连续阅读的章节列表裁成「当前章前后各 N 章」的窗口。
   *
   * 与 `pruneReadChapters` 的区别：那个只在「隐藏已读」模式删除当前章之前的
   * 全部章节；这个在普通连续阅读里前后都裁，只保留窗口，避免长读后 DOM 无上限
   * 增长。裁剪前记录锚点章节的 offsetTop，裁剪后按位移补偿 scrollTop，避免
   * 用户正在读的段落因为上方章节被移除而整体上跳。
   *
   * 只在当前章附近确实超出窗口时动手；否则每个滚动事件都重排一次列表。
   */
  async function trimContinuousWindow(targetIndex = store.currentIndex) {
    if (!isContinuousMode.value || !continuousChapters.value.length) return
    // 滚动是高频路径: 列表还没超出窗口时直接返回，避免每个 scroll 事件都
    // 创建一次 async 调用与 nextTick。
    if (continuousChapters.value.length <= CONTINUOUS_KEEP_BEHIND + CONTINUOUS_KEEP_AHEAD + 1) return
    const first = continuousChapters.value[0]
    const last = continuousChapters.value[continuousChapters.value.length - 1]
    if (!first || !last) return

    const keepFrom = Math.max(0, targetIndex - CONTINUOUS_KEEP_BEHIND)
    const keepTo = targetIndex + CONTINUOUS_KEEP_AHEAD
    const kept = continuousChapters.value.filter(
      (chapter) => chapter.index >= keepFrom && chapter.index <= keepTo,
    )
    // 当前章必须始终留在列表里；否则说明索引异常，宁可放弃本次裁剪也
    // 不能把视口内容清空。
    if (!kept.some((chapter) => chapter.index === targetIndex)) return
    if (kept.length === continuousChapters.value.length) return

    const container = scrollContainerRef.value
    // 锚点取裁剪后仍在列表里的第一段：裁剪对它的 offsetTop 影响可由
    // 前后差值精确抵消。
    const anchorIndex = kept[0].index
    const anchorSelector = `.continuous-chapter[data-chapter-index="${anchorIndex}"]`
    const anchorBefore = container?.querySelector(anchorSelector) as HTMLElement | null
    const previousAnchorOffset = anchorBefore?.offsetTop ?? null

    continuousChapters.value = kept
    await nextTick()
    if (!container || previousAnchorOffset == null) return

    const anchor = container.querySelector(anchorSelector) as HTMLElement | null
    if (!anchor) return
    const anchorDelta = anchor.offsetTop - previousAnchorOffset
    if (anchorDelta === 0) return
    // 与 pruneReadChapters 同样的补偿：保持锚点章节在屏幕上的位置不变。
    container.scrollTo({
      top: Math.max(0, container.scrollTop + anchorDelta),
      behavior: 'instant',
    })
  }

  async function buildContinuousChapter(index: number, forceRefresh = false, reportStage = false) {
    const chapter = store.chapters[index]
    if (!chapter) return null
    const chapterContent = await store.fetchChapterContent(index, forceRefresh, reportStage)
    if (chapterContent == null) return null
    return {
      index,
      title: chapter.title,
      content: chapterContent,
      html: renderChapterHtml(chapterContent),
      isVolume: Boolean(chapter.isVolume),
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
    // 只让「当前章」上报加载阶段：前后章是并行预取，会互相覆盖界面提示
    const currentPromise = buildContinuousChapter(targetIndex, false, true)
    const previousPromise = includePrevious && !hideReadChaptersMode.value && previousIndex >= 0
      ? Promise.resolve(getContinuousChapter(previousIndex) ?? buildContinuousChapter(previousIndex).catch(() => null))
      : Promise.resolve(null)
    const [current, previous] = await Promise.all([currentPromise, previousPromise])
    if (!current) return
    if (generation !== continuousGeneration || !isContinuousMode.value) return

    continuousChapters.value = previous ? [previous, current] : [current]
    // 打开书/重建当前章节时保留已记录的章节进度(loadBook/loadChapter 设置的
    // 上次阅读位置); 仅主动跳章(includePrevious)落到章节开头。若这里硬编码 0,
    // 会把 durChapterPos 一并清零, 打开书时位置恢复读到清零后的假"服务器进度"。
    setContinuousActiveChapter(
      targetIndex,
      current.content,
      includePrevious ? 0 : store.chapterScrollProgress,
    )

    await nextTick()
    if (generation !== continuousGeneration || !isContinuousMode.value) return
    // 远端进度恢复期间不能把连续阅读视口重新定位到章节开头。
    // 先按已保存的章节进度定位，随后 ReaderView 再按字符位置校正。
    const legadoProgress = store.legadoPositionRestore?.ready
      && store.legadoPositionRestore.index === targetIndex
      ? store.legadoPositionRestore.progress
      : (includePrevious ? 0 : store.chapterScrollProgress)
    scrollToContinuousChapter(targetIndex, smooth, legadoProgress)

    // 向后追加紧邻的下一章, 不按已读状态跳章: 「隐藏已读」只隐藏身后翻过的
    // 章节(pruneReadChapters), 回看后再往下读应逐章推进而不是直接跳到未读章。
    const nextIndex = targetIndex + 1

    void (async () => {
      const next = await buildContinuousChapter(nextIndex).catch(() => null)
      if (!next || generation !== continuousGeneration || !isContinuousMode.value) return
      if (continuousChapters.value.some((chapter) => chapter.index === next.index)) return
      continuousChapters.value = [...continuousChapters.value, next]
      // 列表一变长就接着预载下游 —— 打开书/跳章之后用户往往直接往下读,
      // 等到滚动才预载就已经在等网络了。
      preloadContinuousAhead()
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

  /** 预载「已渲染列表之后」的两章。
   *
   *  必须用**尾部**章节作基准而不是 store.currentIndex: 用户滚到某一章中段时,
   *  列表尾部可能已领先当前章若干章, 按 currentIndex 预载只会重复拉取已经在
   *  DOM 里的章节, 并把真正要用的下一章挤出缓存。
   *
   *  隐藏已读形态下身后章节会被立即裁掉, 因此不预载上一章(forwardOnly)。 */
  function preloadContinuousAhead() {
    if (!store.config.enablePreload) return
    const tail = continuousChapters.value[continuousChapters.value.length - 1]
    if (!tail) return
    // 滚动是高频路径, 用"下游是否已就绪或已在路上"挡重复调用 —— 比记住上次的尾部
    // 索引更准: 预载失败时下游仍未就绪, 下一次滚动就会自然重试, 不会永久卡住。
    const downstream = [tail.index + 1, tail.index + 2]
      .filter((index) => index < store.chapters.length)
    // 已是最后几章: 没有下游可预载
    if (!downstream.length) return
    if (downstream.every((index) => store.isChapterReady(index))) return
    void store.preloadAroundChapter(tail.index, { forwardOnly: hideReadChaptersMode.value })
  }

  async function loadContinuousNext() {
    if (continuousNextInFlight || !continuousChapters.value.length) return
    const generation = continuousGeneration
    continuousNextInFlight = true
    try {
      // A fast scroll can reach the bottom while a chapter is being fetched.
      // Keep filling the tail until the viewport has enough content again;
      // adding a section does not reliably emit another scroll event.
      while (continuousChapters.value.length) {
        const last = continuousChapters.value[continuousChapters.value.length - 1]
        const nextIndex = last.index + 1
        if (nextIndex >= store.chapters.length) break

        // 命中预载(或预载在途)时取正文只是一个微任务, 不亮「正在加载下一章…」;
        // 真要从书源拉取才提示。这样预加载生效时用户完全看不到等待。
        if (store.isChapterReady(nextIndex)) {
          hideContinuousNextIndicator()
        } else {
          showContinuousNextIndicatorSoon()
        }

        const next = await buildContinuousChapter(nextIndex)
        if (generation !== continuousGeneration || !next || getContinuousChapter(next.index)) break
        continuousChapters.value = [...continuousChapters.value, next]
        // 追加一章就顺势预载它后面的两章, 让预载链跟着阅读推进接力,
        // 而不是等下一次滚动事件重新发起。
        preloadContinuousAhead()
        await nextTick()

        const container = scrollContainerRef.value
        const remaining = container
          ? container.scrollHeight - (container.scrollTop + container.clientHeight)
          : Number.POSITIVE_INFINITY
        if (remaining >= 480) break
      }
    } finally {
      continuousNextInFlight = false
      hideContinuousNextIndicator()
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
        // The scroll container disables native scroll anchoring, so pin the
        // viewport back to the content the user was reading with an explicit
        // instant jump: how far the anchor moved equals the height inserted
        // above it. Measured off the CURRENT scrollTop so scrolling during the
        // fetch is not fought. instant 也避开「动画时长」补间 —— 这是一次校正,
        // 不是翻页。翻页补间若在运行, 会在下一帧检测到本次赋值并自动让位。
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

  function scrollToContinuousChapter(index: number, smooth = true, progress?: number) {
    const container = scrollContainerRef.value
    if (!container) return
    const section = container.querySelector(`.continuous-chapter[data-chapter-index="${index}"]`) as HTMLElement | null
    if (!section) return
    const targetTop = progress == null
      ? section.offsetTop
      : (() => {
          const nextSection = section.nextElementSibling as HTMLElement | null
          const sectionHeight = Math.max(
            1,
            (nextSection ? nextSection.offsetTop : container.scrollHeight) - section.offsetTop,
          )
          const anchorOffset = container.clientHeight * 0.12
          return Math.max(
            section.offsetTop,
            section.offsetTop + sectionHeight * Math.max(0, Math.min(1, progress)) - anchorOffset,
          )
        })()
    container.scrollTo({
      top: Math.max(0, targetTop),
      // 跳章必须瞬时: 跨过前置章节的动画既慢又没意义。这里的 smooth 分支是
      // 打开书/原地刷新时的定位, 走原生平滑而非「动画时长」补间。
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
    if (continuousNextIndicatorTimer) {
      clearTimeout(continuousNextIndicatorTimer)
      continuousNextIndicatorTimer = null
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
    preloadContinuousAhead,
    armPreviousAutoLoad,
    ensureContinuousChapterLoaded,
    getContinuousSections,
    scrollToContinuousChapter,
    pruneReadChapters,
    trimContinuousWindow,
    clearContinuousChapters,
    disposeContinuousReading,
  }
}
