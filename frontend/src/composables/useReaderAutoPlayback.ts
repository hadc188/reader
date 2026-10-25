import { getCurrentScope, nextTick, onScopeDispose, watch, type ComputedRef, type Ref } from 'vue'
import type { useReaderStore } from '../stores/reader'
import { useAppStore } from '../stores/app'
import { cancelPageScrollAnimation } from '../utils/readerPaging'
import { buildSpeechChunkPieces, buildSpeechChunksFromText } from '../utils/speechChunks'
import { SpeechStream, type SpeechChunk, type SpeechChunkType } from '../utils/speechStream'

type ReaderStore = ReturnType<typeof useReaderStore>
const OPENAI_PRELOAD_CHUNK_LIMIT = 5
const OPENAI_MERGED_SEGMENT_CHAR_LIMIT = 260
/** 手动输入暂停自动滚动后, 静止多久恢复滚动。 */
const AUTO_SCROLL_MANUAL_RESUME_MS = 1200
/** 用户手动跳章后等多久再跟随：连续拖进度条会连续改索引，只跟随最后一次。 */
const CHAPTER_JUMP_FOLLOW_DEBOUNCE_MS = 320
/**
 * 换章后等正文渲染出来的轮询预算：换章要等书源返回正文，慢的时候十几秒都正常，
 * 所以按 ~30 秒给足（150 × 200ms），期间用户停止/再跳章会让这次跟随失效。
 */
const CHAPTER_JUMP_FOLLOW_RETRY = 150
const CHAPTER_JUMP_FOLLOW_RETRY_INTERVAL_MS = 200
/**
 * TTS 自己滚动正文后的保护窗口。
 *
 * 连续阅读模式下正文是一整条，滚动到章边界会让 `currentIndex` 跟着变；TTS 推进
 * 段落时也会滚动，所以刚滚过的那一下不能当成「用户跳章」，否则朗读会被自己的
 * 自动滚动反复重启。窗口要覆盖平滑滚动的整段（几百毫秒），只在「这次索引变化
 * 由滚动事件反推出来」时才生效 —— 目录/上下一章/进度条这类显式跳章不看它。
 */
const SPEECH_SELF_SCROLL_GUARD_MS = 1500

/**
 * 这次 `currentIndex` 变化是不是 TTS 自己滚出来的。
 *
 * 三个条件同时成立才算：索引变化来自滚动事件反推（`scrollDriven`）、距 TTS 上次
 * 自己滚动还在保护窗口内、且这期间用户没有新的滚动/按键输入。只看时间窗口会把
 * 用户在朗读推进后 1.5 秒内的真实跳章一起吞掉 —— 那正是「连续阅读里刚滚两下就
 * 点下一章，朗读不跟」的原因。
 */
export function isSpeechSelfScroll(state: {
  scrollDriven: boolean
  now: number
  lastSpeechScrollAt: number
  lastUserInputAt: number
}): boolean {
  if (!state.scrollDriven) return false
  if (state.now - state.lastSpeechScrollAt >= SPEECH_SELF_SCROLL_GUARD_MS) return false
  return state.lastUserInputAt <= state.lastSpeechScrollAt
}

/**
 * 用户手动跳章时要不要让朗读跟随（抽成纯函数便于单测）。
 *
 * 不跟随的情况：变化本来就是 TTS 自己发起的；当前没有朗读会话；正在切段/
 * 切章的过渡里（再来一次会互相打架）；以及这次索引变化是 TTS 自动滚动反推的
 * （见 `isSpeechSelfScroll`）。
 */
export function shouldFollowChapterJump(state: {
  speechDriven: boolean
  hasSpeechSession: boolean
  transitioning: boolean
  selfScroll: boolean
}): boolean {
  if (state.speechDriven) return false
  if (!state.hasSpeechSession) return false
  if (state.transitioning) return false
  if (state.selfScroll) return false
  return true
}

interface AutoPlaybackConfig {
  clickAction: string
  /** 自动滚动速度, 像素/秒。 */
  autoScrollSpeed: number
}

interface SpeechSegment {
  text: string
  nextParagraph: HTMLElement | null
  paragraphs: HTMLElement[]
}

export function getSpeechProgressItemIndex(lengths: number[], progress: number) {
  if (!lengths.length) return -1
  const normalizedLengths = lengths.map((length) => Math.max(1, length))
  const totalLength = normalizedLengths.reduce((sum, length) => sum + length, 0)
  const position = Math.max(0, Math.min(1, progress)) * totalLength
  let accumulated = 0
  for (let index = 0; index < normalizedLengths.length; index += 1) {
    accumulated += normalizedLengths[index]
    if (position < accumulated || index === normalizedLengths.length - 1) return index
  }
  return normalizedLengths.length - 1
}

/** 分页模式下"该段落属于第几页"：返回页容器的索引，找不到返回 -1。
 *
 *  独立成纯函数是为了可测: 页与页之间只靠 transform 位移, `offsetTop` 在分页布局里
 *  没有跨页含义, 只能靠最近祖先页容器来判断归属。
 *
 *  只要求 `contains` 这一种能力, 不依赖真实 DOM —— 单元测试环境没有 DOM 实现。 */
export function findParagraphPageIndex(
  pages: readonly { contains(node: unknown): boolean }[],
  paragraph: unknown,
): number {
  if (!paragraph) return -1
  for (let index = 0; index < pages.length; index += 1) {
    if (pages[index].contains(paragraph)) return index
  }
  return -1
}

export function useReaderAutoPlayback(
  store: ReaderStore,
  config: ComputedRef<AutoPlaybackConfig>,
  isContinuousMode: ComputedRef<boolean>,
  scrollContainerRef: Ref<HTMLElement | undefined>,
  chapterTextRef: Ref<HTMLElement | undefined>,
  nextChapter: () => void | Promise<void>,
  prevChapter: () => void | Promise<void>,
  /** 左右分页模式: 正文按页渲染, 跟随朗读要靠页索引而不是纵向滚动。 */
  isHorizontalPageMode: ComputedRef<boolean>,
  horizontalPageIndex: Ref<number>,
) {
  let autoScrollId: number | null = null
  let autoScrollResumeTimer: number | null = null
  let lastAutoScrollTime = 0
  let autoScrollRemainder = 0
  let speechRestartTimer: number | null = null
  let isSpeechTransitioning = false
  let currentSpeechParagraph: HTMLElement | null = null
  let currentSpeechSegments: SpeechSegment[] = []
  let currentSpeechSegmentIndex = 0
  /** 当前这次 `currentIndex` 变化是 TTS 自己发起的（见 `runSpeechDrivenChapterChange`）。 */
  let speechDrivenChapterChange = false
  /** 当前这次 `currentIndex` 变化是「滚动反推」出来的（连续阅读 / 自动滚动）。 */
  let scrollDrivenChapterChange = false
  /** TTS 自己滚动正文的时刻：连续模式下滚动到章边界也会改 `currentIndex`。 */
  let lastSpeechOwnScrollAt = 0
  /** 用户最后一次手动输入（滚轮/触摸/按键/点击）的时刻，用于区分自滚与用户滚动。 */
  let lastUserInputAt = 0
  /** 用户跳章后的跟随定时器（连续拖动进度条时不断重置）。 */
  let chapterJumpFollowTimer: number | null = null
  /** 跳章跟随的序号：用户停止/再次跳章时递增，让挂起的「等正文」循环失效。 */
  let chapterFollowSeq = 0

  function isSafariSpeechDelayBrowser() {
    if (typeof navigator === 'undefined') return false
    const ua = navigator.userAgent || ''
    return /Safari/i.test(ua) && !/Chrome|Chromium|CriOS|Edg|EdgiOS|Android/i.test(ua)
  }

  function paragraphPreview(paragraph: HTMLElement | null) {
    return paragraph?.innerText.trim().slice(0, 40) || ''
  }

  function logSpeech(message: string, payload?: unknown) {
    void message
    void payload
  }

  function getFilteredParagraphs() {
    const roots = isContinuousMode.value
      ? Array.from(scrollContainerRef.value?.querySelectorAll('.chapter-text[data-role="continuous"]') || []) as HTMLElement[]
      : (chapterTextRef.value ? [chapterTextRef.value] : [])
    if (!roots.length) return [] as HTMLElement[]
    const allElements = roots.flatMap((root) => Array.from(root.querySelectorAll('p')) as HTMLElement[])
    const list: HTMLElement[] = []
    let lastText = ''
    allElements.forEach((el) => {
      const text = el.innerText.trim()
      if (text && text !== lastText) {
        list.push(el)
        lastText = text
      }
    })
    return list
  }

  function getCurrentParagraph() {
    const reading = chapterTextRef.value?.querySelector('.reading') as HTMLElement | null
    if (reading) return reading

    const container = scrollContainerRef.value
    if (!container) return null

    const list = getFilteredParagraphs()

    // 分页模式: 纵向滚动不生效(容器 overflow-y: hidden), 改用「当前页里第一个段落」。
    // 若仍按偏移量算, 每一页的 offsetTop 都会落在同一套纵向坐标系里, 结果与当前页无关。
    if (isHorizontalPageMode.value) {
      const pages = Array.from(
        container.querySelectorAll('.horizontal-page') || [],
      ) as HTMLElement[]
      const page = pages[horizontalPageIndex.value]
      if (page) {
        const inPage = list.find((paragraph) => page.contains(paragraph))
        if (inPage) return inPage
      }
      return list[0] || null
    }

    for (const paragraph of list) {
      const top = paragraph.offsetTop - container.scrollTop
      const bottom = top + paragraph.offsetHeight
      if (bottom > 40) {
        return paragraph
      }
    }

    return list[0] || null
  }

  function getPrevParagraph() {
    const current = getCurrentParagraph()
    return getPrevParagraphFrom(current)
  }

  function getPrevParagraphFrom(current: HTMLElement | null) {
    const list = getFilteredParagraphs()
    const index = current ? list.indexOf(current) : -1
    if (index > 0) return list[index - 1]
    return null
  }

  function getNextParagraph() {
    const current = getCurrentParagraph()
    return getNextParagraphFrom(current)
  }

  function getNextParagraphFrom(current: HTMLElement | null) {
    const list = getFilteredParagraphs()
    const index = current ? list.indexOf(current) : -1
    if (index >= 0 && index < list.length - 1) return list[index + 1]
    return null
  }

  function buildParagraphSpeechChunks(paragraph: HTMLElement | null) {
    return buildSpeechChunksFromText(paragraph?.innerText.trim() || '')
  }

  function buildMergedSpeechSegment(paragraph: HTMLElement | null): SpeechSegment {
    const currentText = paragraph?.innerText.trim() || ''
    if (!currentText) {
      return {
        text: '',
        nextParagraph: getNextParagraph(),
        paragraphs: [],
      }
    }

    const list = getFilteredParagraphs()
    const startIndex = paragraph ? list.indexOf(paragraph) : -1
    if (!paragraph || startIndex < 0) {
      return {
        text: currentText,
        nextParagraph: getNextParagraph(),
        paragraphs: [],
      }
    }

    const mergedTexts: string[] = [currentText]
    const mergedParagraphs: HTMLElement[] = [paragraph]
    let mergedLength = currentText.length
    let cursorIndex = startIndex + 1

    while (cursorIndex < list.length && mergedLength < OPENAI_MERGED_SEGMENT_CHAR_LIMIT) {
      const nextText = list[cursorIndex]?.innerText.trim() || ''
      if (!nextText) {
        cursorIndex += 1
        continue
      }
      if (mergedLength + nextText.length > OPENAI_MERGED_SEGMENT_CHAR_LIMIT) {
        break
      }
      mergedTexts.push(nextText)
      mergedParagraphs.push(list[cursorIndex])
      mergedLength += nextText.length
      cursorIndex += 1
    }

    return {
      text: mergedTexts.join('\n'),
      nextParagraph: list[cursorIndex] || null,
      paragraphs: mergedParagraphs,
    }
  }

  function resetSpeechChunkState() {
    currentSpeechParagraph = null
    currentSpeechSegments = []
    currentSpeechSegmentIndex = 0
  }

  function buildOpenAISpeechSegments(paragraph: HTMLElement): SpeechSegment[] {
    if (store.speechConfig.openaiRequestMode === 'merged') {
      const merged = buildMergedSpeechSegment(paragraph)
      return merged.text ? [merged] : []
    }

    const paragraphChunks = buildParagraphSpeechChunks(paragraph)
    const nextParagraph = getNextParagraph()
    return paragraphChunks.map((text, index) => ({
      text,
      nextParagraph: index < paragraphChunks.length - 1 ? paragraph : nextParagraph,
      paragraphs: [paragraph],
    }))
  }

  function ensureSpeechChunkState(paragraph: HTMLElement): SpeechSegment {
    if (store.speechConfig.provider !== 'openai') {
      return {
        text: paragraph.innerText.trim(),
        nextParagraph: getNextParagraphFrom(paragraph),
        paragraphs: [paragraph],
      }
    }

    if (currentSpeechParagraph !== paragraph) {
      currentSpeechParagraph = paragraph
      currentSpeechSegments = buildOpenAISpeechSegments(paragraph)
      currentSpeechSegmentIndex = 0
    }

    return currentSpeechSegments[currentSpeechSegmentIndex] || {
      text: '',
      nextParagraph: getNextParagraphFrom(paragraph),
      paragraphs: [paragraph],
    }
  }

  function getUpcomingSpeechChunks(startParagraph: HTMLElement | null) {
    const chunks: string[] = []

    if (store.speechConfig.provider !== 'openai') {
      return chunks
    }

    if (store.speechConfig.openaiRequestMode === 'merged') {
      const merged = buildMergedSpeechSegment(startParagraph)
      return merged.text ? [merged.text] : []
    }

    if (currentSpeechParagraph && currentSpeechSegments.length) {
      for (let i = currentSpeechSegmentIndex + 1; i < currentSpeechSegments.length && chunks.length < OPENAI_PRELOAD_CHUNK_LIMIT; i += 1) {
        if (currentSpeechSegments[i]?.text) {
          chunks.push(currentSpeechSegments[i].text)
        }
      }
    }

    let cursor = startParagraph
    while (cursor && chunks.length < OPENAI_PRELOAD_CHUNK_LIMIT) {
      const paragraphChunks = buildParagraphSpeechChunks(cursor)
      for (const chunk of paragraphChunks) {
        if (chunks.length >= OPENAI_PRELOAD_CHUNK_LIMIT) break
        chunks.push(chunk)
      }
      const list = getFilteredParagraphs()
      const index = list.indexOf(cursor)
      cursor = index >= 0 ? (list[index + 1] || null) : null
    }

    return chunks
  }

  function clearReadingClass() {
    scrollContainerRef.value?.querySelectorAll('.reading').forEach((el) => {
      el.classList.remove('reading')
    })
  }

  function showParagraph(paragraph: HTMLElement | null, smooth = true) {
    const container = scrollContainerRef.value
    if (!container || !paragraph) return

    // 记下「这次滚动是 TTS 自己发起的」：连续阅读模式下滚动到章边界会改
    // `currentIndex`，跟随 watcher 不能把它当成用户跳章（见 `lastSpeechOwnScrollAt`）。
    lastSpeechOwnScrollAt = Date.now()

    // 左右分页模式: 正文被切成 horizontalPages 逐页渲染, 容器是 overflow-y: hidden,
    // 纵向滚不动。翻页靠页索引 + transform, 所以这里必须改走翻页而不是 scrollTo。
    if (isHorizontalPageMode.value) {
      showParagraphOnHorizontalPage(paragraph)
      return
    }

    const targetTop = Math.max(0, paragraph.offsetTop - 24)
    // 朗读推进是用户意图, 先让翻页补间让位, 否则补间每帧写回的 scrollTop
    // 会把本次滚动顶掉。
    cancelPageScrollAnimation(container)
    container.scrollTo({
      top: targetTop,
      behavior: smooth ? 'smooth' : 'auto',
    })
  }

  /**
   * 分页模式下把朗读段落所在页翻到眼前。
   *
   * 段落 DOM 分布在各自的 `.horizontal-page` 里, 所以"段落属于第几页"直接问最近的
   * 页容器即可 —— 比用 offsetTop 反推页索引可靠: 页与页之间是 transform 位移,
   * offsetTop 在分页布局下没有跨页含义。
   */
  function showParagraphOnHorizontalPage(paragraph: HTMLElement) {
    const pages = Array.from(
      scrollContainerRef.value?.querySelectorAll('.horizontal-page') || [],
    ) as HTMLElement[]
    const index = findParagraphPageIndex(pages, paragraph)
    if (index < 0 || index === horizontalPageIndex.value) return
    horizontalPageIndex.value = index
  }

  function markReadingParagraph(paragraph: HTMLElement | null) {
    clearReadingClass()
    if (paragraph) {
      paragraph.classList.add('reading')
    }
  }

  function updateMergedReadingParagraph(segment: SpeechSegment, progress: number) {
    if (store.speechConfig.provider !== 'openai') return
    if (store.speechConfig.openaiRequestMode !== 'merged') return
    if (segment.paragraphs.length <= 1) return
    const lengths = segment.paragraphs.map((paragraph) => paragraph.innerText.trim().length)
    const index = getSpeechProgressItemIndex(lengths, progress)
    const paragraph = segment.paragraphs[index] || null
    if (!paragraph || paragraph.classList.contains('reading')) return
    markReadingParagraph(paragraph)
    showParagraph(paragraph)
  }

  function runAutoScroll(timestamp?: number) {
    if (!store.isAutoScrolling || !scrollContainerRef.value) return

    const container = scrollContainerRef.value
    // 按真实帧间隔折算位移, 高刷新率屏幕下速度不变。
    const now = timestamp ?? performance.now()
    const deltaMs = lastAutoScrollTime ? Math.min(100, now - lastAutoScrollTime) : 16
    lastAutoScrollTime = now

    // 必须显式 instant: 直接赋 scrollTop 会被 CSS 平滑转成慢启动动画, 每帧重启
    // 动画的起步段, 步长越大实际越慢(容器 scroll-behavior 现为 auto, 但这条
    // 防御对未来的样式改动仍成立)。位移按整像素滚动, 不足 1px 的部分累积到
    // 下一帧, 低速(如 2px/秒)也能匀速前进。
    const raw = (config.value.autoScrollSpeed * deltaMs) / 1000 + autoScrollRemainder
    const whole = Math.floor(raw)
    autoScrollRemainder = raw - whole
    if (whole >= 1) {
      container.scrollBy({ top: whole, behavior: 'instant' })
    }

    if (container.scrollTop + container.clientHeight >= container.scrollHeight - 2) {
      if (config.value.clickAction === 'auto' && store.hasNext) {
        void nextChapter()
      } else {
        stopAutoScroll()
      }
    } else {
      autoScrollId = requestAnimationFrame(runAutoScroll)
    }
  }

  function startAutoScroll() {
    if (autoScrollId) return
    lastAutoScrollTime = 0
    autoScrollRemainder = 0
    runAutoScroll()
  }

  /**
   * 手动输入(滚轮/触摸/点击/按键)时暂停自动滚动, 静止片刻后自动恢复。
   * 自动滚动期间用户仍可自由拖动阅读位置。
   */
  function pauseAutoScrollForManualInput() {
    if (!store.isAutoScrolling) return
    if (autoScrollId) {
      cancelAnimationFrame(autoScrollId)
      autoScrollId = null
    }
    if (autoScrollResumeTimer) clearTimeout(autoScrollResumeTimer)
    autoScrollResumeTimer = window.setTimeout(() => {
      autoScrollResumeTimer = null
      if (store.isAutoScrolling) startAutoScroll()
    }, AUTO_SCROLL_MANUAL_RESUME_MS)
  }

  function stopAutoScroll() {
    store.isAutoScrolling = false
    if (autoScrollId) {
      cancelAnimationFrame(autoScrollId)
      autoScrollId = null
    }
    if (autoScrollResumeTimer) {
      clearTimeout(autoScrollResumeTimer)
      autoScrollResumeTimer = null
    }
    if (!store.isSpeaking) {
      clearReadingClass()
    }
  }

  /* ─── 缓冲流朗读（http / openai） ─────────────────────────────────────────
   *
   * system 通道继续走 store.startTTS 的浏览器语音队列（本身就是缓冲的）；
   * http/openai 的请求-播放是严格串行的，每块之间都有网络空档，所以改用
   * SpeechStream：边播边补，双 <audio> 无缝切换。
   *
   * 文本供给按优先级：当前段落剩余块（initialUnitQueue）→ 本章后续段落（也在
   * initialUnitQueue 里一次给出，由流按缓冲配额自行节制）→ 下一章正文（先用
   * store.fetchChapterContent 取回缓存，切章时 loadChapter 命中缓存）。 */

  interface SpeechStreamUnit extends SpeechChunk {
    /** 该块对应本章第几个 DOM 段落，用于高亮/跟随滚动。 */
    paragraphIndex: number
  }

  interface SpeechStreamSession {
    bookUrl: string
    /** 已经为哪一章规划过块（含跨章预取），预取下一章时以它 +1 为准。 */
    lastChapterPlanned: number
    unitQueue: SpeechStreamUnit[]
    /** 当前章的 DOM 段落快照；切章后刷新。 */
    domParagraphs: HTMLElement[]
    stopped: boolean
  }

  let speechStreamSession: SpeechStreamSession | null = null
  let activeSpeechStream: SpeechStream | null = null
  /**
   * 本次朗读已提示过的「单块失败」错误消息。
   *
   * SpeechStream 只负责单个流的去重；连续朗读跨段/跨章会创建新的流实例，
   * 所以这里按「用户本次开始朗读」为范围再兜一层，避免换段后同一提示反复弹。
   * 用户重新开始朗读（interruptCurrent=true）时清空。
   */
  const reportedSpeechChunkErrors = new Set<string>()
  const appStore = useAppStore()

  /** 正文 → 段落文本。与 ReaderView.formatChapterHtml 的纯文本分支保持一致的切分。 */
  function splitChapterContentIntoParagraphs(content: string): string[] {
    let text = content
    if (/<[a-z][\s\S]*>/i.test(content)) {
      // 书源正文可能是 HTML：把块级标签换成换行，再去标签/解常见实体，得到可朗读文本。
      text = content
        .replace(/<br\s*\/?>/gi, '\n')
        .replace(/<\/(p|div|li|h\d|tr)>/gi, '\n')
        .replace(/<[^>]+>/g, '')
        .replace(/&nbsp;/gi, ' ')
        .replace(/&lt;/gi, '<')
        .replace(/&gt;/gi, '>')
        .replace(/&quot;/gi, '"')
        .replace(/&#39;/gi, "'")
        .replace(/&amp;/gi, '&')
    }
    return text
      .split(/\n/)
      .map((line) => line.replace(/^[\u3000\u00A0 \t]+/, '').trim())
      .filter(Boolean)
  }

  /**
   * 一段文本 → 缓冲流的块（含双音色按引号 / 说话人冒号标注旁白/对话）。
   *
   * 必须**先切旁白/对话段、再按长度切块**（见 `buildSpeechChunkPieces` 的说明）：
   * 反过来先按句号切块再逐块认引号，一段长对话的后续小块会丢掉引号上下文，
   * 于是前半句用对话音色、后半句突然变旁白。
   *
   * `openCloser` / 返回值里的 `openCloser` 用于跨段对话：上一段只有开引号时，
   * 下一段要从对话中续读。
   */
  function buildChunkPieces(
    text: string,
    openCloser: string | null = null,
  ): { pieces: Array<{ text: string; type: SpeechChunkType }>; openCloser: string | null } {
    // openai / edge / msdemo 都是「一次网络合成一段音频」：按句切成 ≤70 字的块，
    // 首播只需等第一小块；http 引擎的请求按整段模板展开，切碎反而增加请求数，故不切。
    const sentenceChunked = store.speechConfig.provider === 'openai'
      || store.speechConfig.provider === 'edge'
      || store.speechConfig.provider === 'msdemo'
    // 纯标点的块（`……`、只剩引号的 `“……”`）在线服务不会返回音频，发给后端只会
    // 换来错误并可能打断整段朗读，这里直接丢弃；整段都被丢弃时上层会跳过该段。
    const { pieces, openCloser: nextOpenCloser } = buildSpeechChunkPieces(text, {
      dualVoice: store.speechConfig.dualVoiceEnabled,
      sentenceChunked,
      openCloser,
    })
    return { pieces, openCloser: nextOpenCloser }
  }

  /** 段落属于哪一章：连续阅读模式下本章之外还有别的章节 DOM。 */
  function paragraphChapterIndex(paragraph: HTMLElement): number {
    if (!isContinuousMode.value) return store.currentIndex
    const section = paragraph.closest?.('.continuous-chapter') as HTMLElement | null
    const raw = section?.dataset?.chapterIndex
    const parsed = raw == null ? Number.NaN : Number(raw)
    return Number.isFinite(parsed) ? parsed : store.currentIndex
  }

  /**
   * 收集从 startParagraph 开始的本章朗读块。
   *
   * 连续阅读模式只收「同一章」的段落：跨章时 DOM 结构会在切章时重建，提前把
   * 别的章节混进来会让高亮映射错位；那里改由 idle → continueSpeechTarget 走
   * 现有切章流程（连续模式本身已经预渲染了后续章节，空档很小）。
   */
  function collectDomSpeechUnits(startParagraph: HTMLElement, chapterIndex: number) {
    const list = getFilteredParagraphs()
    const startIndex = Math.max(0, list.indexOf(startParagraph))
    const domParagraphs: HTMLElement[] = []
    const units: SpeechStreamUnit[] = []
    // 跨段对话：上一段结束时还在引号里，下一段开头就仍算对话。
    let openCloser: string | null = null
    for (let index = startIndex; index < list.length; index += 1) {
      const paragraph = list[index]
      if (paragraphChapterIndex(paragraph) !== chapterIndex) break
      const text = paragraph.innerText.trim()
      if (!text) continue
      const paragraphIndex = domParagraphs.length
      domParagraphs.push(paragraph)
      const built = buildChunkPieces(text, openCloser)
      openCloser = built.openCloser
      for (const piece of built.pieces) {
        units.push({ ...piece, chapterIndex, paragraphIndex })
      }
    }
    return { units, domParagraphs }
  }

  /**
   * 跨章预取：取下一章正文并切成块。
   *
   * 这里只取正文（会进 store 的正文缓存），不切换当前章；播放到这些块时由
   * onChapterChange 调 nextChapter()，届时 loadChapter 命中缓存，几乎瞬间完成。
   */
  async function prefetchNextChapterUnits(session: SpeechStreamSession): Promise<SpeechStreamUnit[]> {
    if (isContinuousMode.value) return []
    const nextChapter = session.lastChapterPlanned + 1
    if (nextChapter >= store.chapters.length) return []
    const content = await store.fetchChapterContent(nextChapter).catch(() => null)
    if (session.stopped || !content) return []
    const processed = store.processContentForDisplay(content)
    const paragraphs = splitChapterContentIntoParagraphs(processed)
    if (!paragraphs.length) return []
    const units: SpeechStreamUnit[] = []
    // 跨段对话状态只在同一章内传递：换章时对话本来就会被章节切开。
    let openCloser: string | null = null
    paragraphs.forEach((paragraphText, paragraphIndex) => {
      const built = buildChunkPieces(paragraphText, openCloser)
      openCloser = built.openCloser
      for (const piece of built.pieces) {
        units.push({ ...piece, chapterIndex: nextChapter, paragraphIndex })
      }
    })
    session.lastChapterPlanned = nextChapter
    return units
  }

  async function supplySpeechUnits(session: SpeechStreamSession): Promise<SpeechChunk[]> {
    if (session.stopped) return []
    // 本章（含开头一次性收集的剩余段落）优先。
    if (session.unitQueue.length) return session.unitQueue.splice(0, session.unitQueue.length)
    // 书末 / 连续模式：交给 onIdle → continueSpeechTarget 走原切章流程。
    if (!store.hasNext) return []
    const units = await prefetchNextChapterUnits(session)
    if (session.stopped || !units.length) return []
    return units
  }

  /** 找到块对应的 DOM 段落（切章后按段落序号优先，文本匹配兜底）。 */
  function resolveUnitParagraph(session: SpeechStreamSession, unit: SpeechStreamUnit): HTMLElement | null {
    const direct = session.domParagraphs[unit.paragraphIndex]
    if (direct && direct.isConnected) return direct
    const probe = unit.text.slice(0, 12)
    return session.domParagraphs.find((paragraph) => paragraph.isConnected && paragraph.innerText.includes(probe)) || null
  }

  function stopSpeechStream() {
    const session = speechStreamSession
    const stream = activeSpeechStream
    speechStreamSession = null
    activeSpeechStream = null
    if (session) session.stopped = true
    if (stream) {
      // 先摘句柄再停流：stop() 不再回到 store，避免 store.stopTTS 重入。
      store.attachSpeechStreamControls(null)
      stream.stop()
    }
  }

  /** 流自然结束（onIdle）或出错后的清理：停流并摘掉句柄。 */
  function releaseSpeechStream(session: SpeechStreamSession) {
    session.stopped = true
    if (speechStreamSession === session) speechStreamSession = null
    if (activeSpeechStream) {
      activeSpeechStream.stop()
      activeSpeechStream = null
    }
    store.attachSpeechStreamControls(null)
  }

  /**
   * TTS 自己发起的切章：期间的 `currentIndex` 变化不能被当成「用户跳章」。
   *
   * 朗读跨章要调 `nextChapter()`，它和用户按「下一章」走的是同一条路 —— 不包这一层
   * 的话，跟随 watcher 会把 TTS 自己的推进当成用户操作，反复重启朗读。
   */
  async function runSpeechDrivenChapterChange<T>(action: () => T | Promise<T>): Promise<T> {
    speechDrivenChapterChange = true
    try {
      return await action()
    } finally {
      speechDrivenChapterChange = false
    }
  }

  /**
   * 取消「跳章跟随」：既清掉防抖定时器，也让已经进入「等正文」循环的那一次失效。
   *
   * 用递增序号而不是布尔量：用户可能连续拖进度条（多次跟随）或中途按停止，
   * 序号一变，之前挂起的等待就会自行退出，不会在几秒后突然开始朗读。
   */
  function cancelChapterJumpFollow() {
    chapterFollowSeq += 1
    if (chapterJumpFollowTimer !== null) {
      clearTimeout(chapterJumpFollowTimer)
      chapterJumpFollowTimer = null
    }
  }

  /**
   * 用户手动换了章节（目录 / 上下一章 / 拖进度条）而朗读还在进行时，朗读跟着换到
   * 新章开头。
   *
   * 不做这件事的后果：缓冲流继续播旧章的音频，高亮因为找不到对应 DOM 而静默不动，
   * 直到旧章剩余块播完、流想跨章时才发现索引不符而停止 —— 用户体感是「朗读卡在
   * 原地不跟」。
   */
  function followUserChapterJump() {
    cancelChapterJumpFollow()
    const seq = chapterFollowSeq
    chapterJumpFollowTimer = window.setTimeout(() => {
      chapterJumpFollowTimer = null
      if (speechDrivenChapterChange || seq !== chapterFollowSeq) return
      // 暂停状态要在防抖之后再读：用户可能在这 320ms 里按了暂停/继续。
      const wasPaused = store.isPaused
      // 先把旧流彻底停掉（在途合成 + 两个 <audio> + objectURL 都释放）。
      cancelSpeechTransition()
      stopSpeechStream()
      store.stopTTS(false)
      resetSpeechChunkState()
      clearReadingClass()
      // 暂停中跳章：只丢掉旧位置，不自动出声；下次点播放从新章开头开始。
      if (wasPaused) return
      void startSpeechAtChapterStart(seq)
    }, CHAPTER_JUMP_FOLLOW_DEBOUNCE_MS)
  }

  /**
   * 等新章正文渲染出来后，从第一段开始朗读。
   *
   * 预算按「网络章节」给足：换章要等书源返回正文，慢的时候好几秒。等待期间用户
   * 按停止/又跳了别的章会让序号变化（见 `cancelChapterJumpFollow`），这里立即放弃。
   *
   * 起点必须是**目标章**的第一段：连续阅读的 DOM 里同时挂着上一章/下一章，直接取
   * `getFilteredParagraphs()[0]` 会从上一章开头读起（`collectDomSpeechUnits` 只收
   * 当前章，于是立刻返回空、掉进逐段跳过的慢路径，新章迟迟不开始）。
   */
  async function startSpeechAtChapterStart(seq: number) {
    const targetChapter = store.currentIndex
    for (let attempt = 0; attempt < CHAPTER_JUMP_FOLLOW_RETRY; attempt += 1) {
      if (seq !== chapterFollowSeq) return
      await nextTick()
      if (seq !== chapterFollowSeq) return
      if (store.currentIndex !== targetChapter) return
      if (!store.loading) {
        const first = getFilteredParagraphs()
          .find((paragraph) => paragraphChapterIndex(paragraph) === targetChapter) || null
        if (first?.isConnected && first.innerText.trim()) {
          startSpeech(first, true)
          return
        }
      }
      await new Promise((resolve) => setTimeout(resolve, CHAPTER_JUMP_FOLLOW_RETRY_INTERVAL_MS))
    }
  }

  function startBufferedSpeech(paragraph: HTMLElement, interruptCurrent: boolean) {
    // 新会话先彻底停掉旧流（它的在途请求与 audio 都要释放）。
    stopSpeechStream()
    if (interruptCurrent) {
      // 用户主动开始新一轮朗读：单块失败提示的去重范围随之重置。
      reportedSpeechChunkErrors.clear()
      // 停掉可能还在跑的系统语音 / 单段播放（它们不走流）。
      store.stopTTS(false)
    }
    resetSpeechChunkState()
    markReadingParagraph(paragraph)
    showParagraph(paragraph)

    const chapterIndex = store.currentIndex
    const bookUrl = store.book?.bookUrl || ''
    const { units, domParagraphs } = collectDomSpeechUnits(paragraph, chapterIndex)
    if (!units.length) {
      continueSpeechTarget(getNextParagraphFrom(paragraph))
      return
    }

    const session: SpeechStreamSession = {
      bookUrl,
      lastChapterPlanned: chapterIndex,
      unitQueue: units,
      domParagraphs,
      stopped: false,
    }
    speechStreamSession = session

    const stream = new SpeechStream({
      supply: () => supplySpeechUnits(session),
      synthesize: (chunk, signal) => store.synthesizeSpeechChunkBlob(chunk.text, chunk.type, signal),
      onChunkStart: (chunk) => {
        if (session.stopped) return
        const unit = chunk as SpeechStreamUnit
        store.isSpeechLoading = false
        store.isSpeaking = true
        store.isPaused = false
        const target = resolveUnitParagraph(session, unit)
        if (target) {
          markReadingParagraph(target)
          showParagraph(target)
        }
      },
      onChunkProgress: (_chunk, progress) => {
        if (session.stopped) return
        store.speechProgress = progress
      },
      onChapterChange: async (nextChapterIndex) => {
        if (session.stopped) return
        if (session.bookUrl !== (store.book?.bookUrl || '')) {
          stopSpeechStream()
          return
        }
        // 预取可能跑得比播放快，一次要跨多章时逐章推进。
        while (store.currentIndex < nextChapterIndex) {
          if (!store.hasNext) {
            stopSpeechStream()
            return
          }
          await runSpeechDrivenChapterChange(() => nextChapter())
          if (session.stopped) return
        }
        if (store.currentIndex !== nextChapterIndex) {
          // 用户手动跳到了别的章，流已经对不上文本，直接停掉。
          stopSpeechStream()
          return
        }
        await nextTick()
        if (session.stopped) return
        session.domParagraphs = getFilteredParagraphs()
        clearReadingClass()
        const first = session.domParagraphs[0]
        if (first) {
          markReadingParagraph(first)
          showParagraph(first, false)
        }
      },
      onIdle: () => {
        if (session.stopped) return
        releaseSpeechStream(session)
        store.isSpeechLoading = false
        store.isSpeaking = false
        store.speechProgress = 0
        // 本章读完：沿用既有「继续下一段/下一章」的推进逻辑。
        continueSpeechTarget(null)
      },
      onChunkError: (error) => {
        if (session.stopped) return
        const message = error.message || '朗读失败'
        logSpeech('speech chunk skipped', { message })
        // 单块失败已在流内重试一次并跳过，不中断朗读。提示措辞刻意不提「通道不可用」：
        // 纯标点片段服务端本来就不返回音频，让用户换通道只会误导。同一错误在本次朗读中
        // 只提示一次（跨流实例也去重），SpeechStream 内部另有单流去重作为第一道。
        if (reportedSpeechChunkErrors.has(message)) return
        reportedSpeechChunkErrors.add(message)
        appStore.showToast(`已跳过朗读失败的片段：${message}`, 'warning')
      },
      onError: (error) => {
        if (session.stopped) return
        releaseSpeechStream(session)
        const message = error.message || '朗读失败'
        logSpeech('speech stream error', { message })
        appStore.showToast(message, 'error')
        store.isSpeechLoading = false
        store.isSpeaking = false
        store.isPaused = false
        store.speechProgress = 0
        resetSpeechChunkState()
        clearReadingClass()
        // 摘掉流句柄后让 store 复位状态（不会再次 stop 流）。
        store.stopTTS()
      },
    }, {
      bufferChars: store.speechConfig.bufferChars,
      maxConcurrent: 2,
    })
    activeSpeechStream = stream
    store.attachSpeechStreamControls({
      pause: () => stream.pause(),
      resume: () => stream.resume(),
      stop: () => {
        session.stopped = true
        if (speechStreamSession === session) speechStreamSession = null
        if (activeSpeechStream === stream) activeSpeechStream = null
        stream.stop()
      },
    })
    store.isSpeechLoading = true
    store.isSpeaking = false
    store.isPaused = false
    store.speechProgress = 0
    logSpeech('start buffered speech', {
      provider: store.speechConfig.provider,
      chapterIndex,
      chunks: units.length,
      bufferChars: store.speechConfig.bufferChars,
    })
    stream.start()
  }

  function restartSpeechTarget(paragraph: HTMLElement | null, interruptCurrent = true) {
    logSpeech('restartSpeechTarget', {
      interruptCurrent,
      paragraph: paragraphPreview(paragraph),
      isSpeechTransitioning,
    })
    if (!paragraph) {
      store.stopTTS()
      resetSpeechChunkState()
      return
    }
    if (isSpeechTransitioning) return
    isSpeechTransitioning = true
    resetSpeechChunkState()
    if (interruptCurrent) {
      store.stopTTS(false)
    }
    if (speechRestartTimer) {
      clearTimeout(speechRestartTimer)
    }
    const restartDelay = !interruptCurrent && store.speechConfig.provider === 'system'
      ? ((isSafariSpeechDelayBrowser() && !store.systemTtsNativeEventsReliable) ? 160 : 40)
      : 150
    speechRestartTimer = window.setTimeout(() => {
      if (store.isPaused) {
        isSpeechTransitioning = false
        return
      }
      isSpeechTransitioning = false
      startSpeech(paragraph, interruptCurrent)
    }, restartDelay)
  }

  function continueSpeechTarget(paragraph: HTMLElement | null, resetChunks = true) {
    logSpeech('continueSpeechTarget', {
      resetChunks,
      paragraph: paragraphPreview(paragraph),
      hasNextChapter: store.hasNext,
    })
    if (speechRestartTimer) {
      clearTimeout(speechRestartTimer)
    }

    const continueDelay = store.speechConfig.provider === 'system'
      ? ((isSafariSpeechDelayBrowser() && !store.systemTtsNativeEventsReliable) ? 160 : 40)
      : 120

    if (paragraph) {
      isSpeechTransitioning = true
      if (resetChunks) {
        resetSpeechChunkState()
      }
      speechRestartTimer = window.setTimeout(() => {
        if (store.isPaused) {
          isSpeechTransitioning = false
          return
        }
        isSpeechTransitioning = false
        startSpeech(paragraph, false)
      }, continueDelay)
      return
    }

    if (!store.hasNext) {
      store.stopTTS()
      clearReadingClass()
      return
    }

    isSpeechTransitioning = true
    if (resetChunks) {
      resetSpeechChunkState()
    }
    Promise.resolve(runSpeechDrivenChapterChange(() => nextChapter()))
      .then(() => {
        speechRestartTimer = window.setTimeout(() => {
          if (store.isPaused) {
            isSpeechTransitioning = false
            return
          }
          isSpeechTransitioning = false
          startSpeech(getFilteredParagraphs()[0] || null, false)
        }, continueDelay)
      })
      .catch(() => {
        isSpeechTransitioning = false
      })
  }

  /**
   * 标记「这次 `currentIndex` 变化是滚动反推出来的」。
   *
   * 连续阅读模式下 `handleScroll` 按视口位置反推当前章，改索引的有可能是 TTS 自己
   * 的自动滚动（滚到章边界），也有可能是用户滚动。只靠时间窗口分不开，所以由
   * 滚动方显式标记来源，watcher 消费一次即复位（见 `isSpeechSelfScroll`）。
   */
  function markChapterChangeFromScroll() {
    scrollDrivenChapterChange = true
  }

  /** 用户手动输入（滚轮/触摸/按键/点击）：此后的滚动不再算 TTS 自动滚动。 */
  function markManualReadingInput() {
    lastUserInputAt = Date.now()
  }

  function startSpeech(paragraph?: HTMLElement | null, interruptCurrent = true) {
    const current = paragraph || getCurrentParagraph()
    logSpeech('startSpeech', {
      interruptCurrent,
      paragraph: paragraphPreview(current),
      currentIndex: store.currentIndex,
    })
    if (!current?.innerText.trim()) {
      if (interruptCurrent) {
        speechNext()
      } else {
        continueSpeechTarget(getNextParagraph())
      }
      return
    }

    // http / openai / edge / msdemo 的请求-播放是串行的，走缓冲流把网络等待藏到
    // 播放后面；system 的浏览器语音队列本身就是缓冲，保持原逻辑。
    const provider = store.speechConfig.provider
    if (provider === 'http' || provider === 'openai' || provider === 'edge' || provider === 'msdemo') {
      startBufferedSpeech(current, interruptCurrent)
      return
    }

    markReadingParagraph(current)
    showParagraph(current)
    const chunk = ensureSpeechChunkState(current)
    if (!chunk.text.trim()) {
      if (interruptCurrent) {
        speechNext(chunk.nextParagraph)
      } else {
        continueSpeechTarget(chunk.nextParagraph)
      }
      return
    }
    const nextParagraph = chunk.nextParagraph
    logSpeech('speak chunk', {
      interruptCurrent,
      provider: store.speechConfig.provider,
      text: chunk.text.slice(0, 60),
      nextParagraph: paragraphPreview(nextParagraph),
      chunkIndex: currentSpeechSegmentIndex,
      chunkCount: currentSpeechSegments.length,
    })
    store.startTTS(chunk.text, {
      onProgress: (progress) => {
        updateMergedReadingParagraph(chunk, progress)
      },
      onEnd: () => {
        logSpeech('chunk onEnd', {
          provider: store.speechConfig.provider,
          currentParagraph: paragraphPreview(current),
          nextParagraph: paragraphPreview(nextParagraph),
          chunkIndex: currentSpeechSegmentIndex,
          chunkCount: currentSpeechSegments.length,
        })
        if (store.speechConfig.provider === 'openai' && currentSpeechParagraph === current && currentSpeechSegmentIndex < currentSpeechSegments.length - 1) {
          currentSpeechSegmentIndex += 1
          continueSpeechTarget(current, false)
          return
        }
        continueSpeechTarget(nextParagraph)
      },
      onError: () => {
        logSpeech('chunk onError', {
          currentParagraph: paragraphPreview(current),
          nextParagraph: paragraphPreview(nextParagraph),
        })
        resetSpeechChunkState()
        clearReadingClass()
      },
    }, interruptCurrent)
    const preloadTexts = getUpcomingSpeechChunks(nextParagraph)
    if (preloadTexts.length) {
      window.setTimeout(() => {
        void store.preloadOpenAITTS(preloadTexts)
      }, 0)
    }
  }

  function speechPrev() {
    logSpeech('speechPrev', {
      currentParagraph: paragraphPreview(getCurrentParagraph()),
      hasPrevChapter: store.hasPrev,
    })
    resetSpeechChunkState()
    const prev = getPrevParagraph()
    if (prev) {
      restartSpeechTarget(prev)
      return
    }
    if (!store.hasPrev) {
      store.stopTTS()
      return
    }
    store.stopTTS(false)
    Promise.resolve(runSpeechDrivenChapterChange(() => prevChapter())).then(() => {
      window.setTimeout(() => {
        const list = getFilteredParagraphs()
        restartSpeechTarget(list[list.length - 1] || null)
      }, 120)
    })
  }

  function speechNext(forcedNext?: HTMLElement | null, interruptCurrent = true) {
    logSpeech('speechNext', {
      interruptCurrent,
      forcedNext: paragraphPreview(forcedNext || null),
      currentParagraph: paragraphPreview(getCurrentParagraph()),
      hasNextChapter: store.hasNext,
    })
    resetSpeechChunkState()
    const next = forcedNext ?? getNextParagraph()
    if (next) {
      restartSpeechTarget(next, interruptCurrent)
      return
    }
    if (!store.hasNext) {
      store.stopTTS()
      clearReadingClass()
      return
    }
    if (interruptCurrent) {
      store.stopTTS(false)
    }
    Promise.resolve(runSpeechDrivenChapterChange(() => nextChapter())).then(() => {
      window.setTimeout(() => {
        restartSpeechTarget(getFilteredParagraphs()[0] || null)
      }, 120)
    })
  }

  function restartSpeechFromCurrentParagraph() {
    logSpeech('restartSpeechFromCurrentParagraph', {
      currentParagraph: paragraphPreview(getCurrentParagraph()),
      isSpeechTransitioning,
    })
    if (isSpeechTransitioning) return
    isSpeechTransitioning = true
    resetSpeechChunkState()
    store.stopTTS(false)
    if (speechRestartTimer) {
      clearTimeout(speechRestartTimer)
    }
    speechRestartTimer = window.setTimeout(() => {
      if (store.isPaused) {
        isSpeechTransitioning = false
        return
      }
      isSpeechTransitioning = false
      startSpeech()
    }, 150)
  }

  function cancelSpeechTransition() {
    if (speechRestartTimer) {
      clearTimeout(speechRestartTimer)
      speechRestartTimer = null
    }
    isSpeechTransitioning = false
  }

  function disposeAutoPlayback() {
    cancelSpeechTransition()
    cancelChapterJumpFollow()
    stopAutoScroll()
    // 组件卸载（离开阅读页）时必须把缓冲流停掉：它持有 audio 与在途请求，
    // 不清理会继续出声，而用户已经没有任何入口关掉它。
    stopSpeechStream()
  }

  // 换书/关闭书籍时文本与 DOM 都变了，仍在跑的流（含跨章预取）必须终止。
  watch(() => store.book?.bookUrl, (bookUrl, previous) => {
    if (previous && bookUrl !== previous) {
      cancelChapterJumpFollow()
      stopSpeechStream()
    }
  })

  // 任何「真正停止朗读」的入口（面板停止按钮 / 快捷键 / 定时停止 / 关闭听书面板）
  // 都要取消还没落地的跳章跟随：那条跟随会在正文加载完之后自己开始朗读，
  // 用户按了停止却过几秒又出声，是明显的行为倒退。
  const removeSpeechStopListener = store.addSpeechStopListener(() => {
    cancelChapterJumpFollow()
  })
  // 组件卸载时摘掉监听：store 是全局单例，不摘会一直持有已销毁组件的闭包。
  if (getCurrentScope()) onScopeDispose(removeSpeechStopListener)

  /**
   * 朗读进行中用户手动换章 → 让朗读跟到新章开头。
   *
   * 覆盖所有会改 `currentIndex` 的用户入口（目录、上一章/下一章、进度条拖拽、
   * 连续阅读里滚过章边界）：它们最终都只改索引，不经过朗读 —— 所以盯索引是唯一
   * 不漏的做法。
   */
  watch(() => store.currentIndex, (next, previous) => {
    if (next === previous) return
    // 暂停时流还在（只是没在出声），同样要停掉旧位置，否则恢复播放会接着播旧章。
    const hasSpeechSession = !!speechStreamSession
      || !!activeSpeechStream
      || store.isSpeaking
      || store.isPaused
    // 「TTS 自己滚动会改索引」只发生在连续阅读模式：其它模式里 TTS 滚动正文不会
    // 动 currentIndex，不必让这个保护窗口把用户的真实跳章也挡掉。
    const selfScroll = isContinuousMode.value && isSpeechSelfScroll({
      scrollDriven: scrollDrivenChapterChange,
      now: Date.now(),
      lastSpeechScrollAt: lastSpeechOwnScrollAt,
      lastUserInputAt,
    })
    // 来源标记是一次性的：无论这次跟不跟，都别留在下一次索引变化上。
    scrollDrivenChapterChange = false
    if (!shouldFollowChapterJump({
      speechDriven: speechDrivenChapterChange,
      hasSpeechSession,
      transitioning: isSpeechTransitioning,
      selfScroll,
    })) return
    followUserChapterJump()
  })

  return {
    getCurrentParagraph,
    clearReadingClass,
    startAutoScroll,
    stopAutoScroll,
    pauseAutoScrollForManualInput,
    startSpeech,
    speechPrev,
    speechNext,
    restartSpeechFromCurrentParagraph,
    cancelSpeechTransition,
    /** 用户按停止/关闭听书时调用：取消还没落地的跳章跟随，避免几秒后自己开始朗读。 */
    cancelPendingChapterFollow: cancelChapterJumpFollow,
    /** 供 ReaderView 的滚动处理标记「这次索引变化来自滚动」与用户手动输入。 */
    markChapterChangeFromScroll,
    markManualReadingInput,
    disposeAutoPlayback,
  }
}
