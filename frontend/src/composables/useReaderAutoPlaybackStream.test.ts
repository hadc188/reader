import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { ref } from 'vue'
import { createPinia, setActivePinia } from 'pinia'
import { useReaderStore } from '../stores/reader'
import { useAppStore } from '../stores/app'
import { useReaderAutoPlayback } from './useReaderAutoPlayback'
import { getBookContent, getBookInfo, getChapterList, saveBookProgress } from '../api/bookshelf'
import { syncLegadoBookProgress, uploadLegadoBookProgress } from '../api/webdav'
import { requestOpenAISpeechAudio } from '../utils/openaiSpeech'
import { synthesizeHttpTts } from '../api/httpTts'
import { synthesizeEdgeTts } from '../api/edgeTts'
import { synthesizeMsTts } from '../api/msTts'

vi.mock('../api/bookshelf', () => ({
  getChapterList: vi.fn(),
  getBookInfo: vi.fn(),
  getBookContent: vi.fn(),
  saveBookProgress: vi.fn(),
  setBookSource: vi.fn(),
}))

vi.mock('../api/bookmark', () => ({
  getBookmarks: vi.fn(),
  saveBookmark: vi.fn(),
  deleteBookmark: vi.fn(),
  deleteBookmarks: vi.fn(),
}))

vi.mock('../api/replaceRule', () => ({
  getReplaceRules: vi.fn(),
  applyReplaceRulesInSandbox: vi.fn(),
}))

vi.mock('../api/webdav', () => ({
  syncLegadoBookProgress: vi.fn(),
  uploadLegadoBookProgress: vi.fn(),
}))

vi.mock('../utils/recentBooks', () => ({
  saveRecentReadBook: vi.fn(),
}))

vi.mock('../utils/openaiSpeech', () => ({
  DEFAULT_OPENAI_BASE_URL: 'https://api.openai.com/v1',
  inferSpeechApiFormat: vi.fn(() => 'openai'),
  getSpeechApiFormatOption: vi.fn(() => ({ supportedFormats: ['mp3', 'wav', 'opus', 'flac', 'pcm'] })),
  requestOpenAISpeechAudio: vi.fn(),
}))

vi.mock('../api/httpTts', async (importOriginal) => {
  const actual = await importOriginal<typeof import('../api/httpTts')>()
  return {
    ...actual,
    getHttpTtsList: vi.fn(),
    saveHttpTtsList: vi.fn(),
    deleteHttpTts: vi.fn(),
    readRemoteHttpTtsFile: vi.fn(),
    synthesizeHttpTts: vi.fn(),
  }
})

vi.mock('../api/edgeTts', async (importOriginal) => {
  const actual = await importOriginal<typeof import('../api/edgeTts')>()
  return {
    ...actual,
    listEdgeTtsVoices: vi.fn(),
    synthesizeEdgeTts: vi.fn(),
  }
})

vi.mock('../api/msTts', async (importOriginal) => {
  const actual = await importOriginal<typeof import('../api/msTts')>()
  return {
    ...actual,
    listMsTtsVoices: vi.fn(),
    synthesizeMsTts: vi.fn(),
  }
})

/** 只实现朗读链路用到的那部分 <audio>。 */
class FakeAudio {
  src = ''
  paused = true
  duration = 10
  currentTime = 0
  onended: (() => void) | null = null
  onerror: (() => void) | null = null
  ontimeupdate: (() => void) | null = null
  onloadedmetadata: (() => void) | null = null

  play() {
    this.paused = false
    return Promise.resolve()
  }

  pause() {
    this.paused = true
  }
}

/** 段落替身：朗读只用到 innerText / classList / isConnected / offsetTop。 */
class FakeParagraph {
  innerText: string
  isConnected = true
  offsetTop = 0
  offsetHeight = 20
  /** 连续阅读的段落属于哪一章（`paragraphChapterIndex` 靠 closest 取 dataset）。 */
  chapterIndex: number | null
  private classes = new Set<string>()
  classList = {
    add: (name: string) => {
      this.classes.add(name)
    },
    remove: (name: string) => {
      this.classes.delete(name)
    },
    contains: (name: string) => this.classes.has(name),
  }

  constructor(text: string, chapterIndex: number | null = null) {
    this.innerText = text
    this.chapterIndex = chapterIndex
  }

  closest(selector: string) {
    if (selector !== '.continuous-chapter' || this.chapterIndex === null) return null
    return { dataset: { chapterIndex: String(this.chapterIndex) } }
  }
}

let paragraphs: FakeParagraph[] = []
let audioInstances: FakeAudio[] = []
let chapterParagraphs = new Map<number, FakeParagraph[]>()

function stubStorage() {
  const storage = new Map<string, string>()
  vi.stubGlobal('localStorage', {
    getItem: vi.fn((key: string) => storage.get(key) ?? null),
    setItem: vi.fn((key: string, value: string) => storage.set(key, value)),
    removeItem: vi.fn((key: string) => storage.delete(key)),
    clear: vi.fn(() => storage.clear()),
  })
}

function stubGlobals() {
  audioInstances = []
  vi.stubGlobal('Audio', class extends FakeAudio {
    constructor() {
      super()
      audioInstances.push(this)
    }
  })
  vi.stubGlobal('URL', {
    createObjectURL: vi.fn(() => `blob:test-${audioInstances.length}-${Math.random()}`),
    revokeObjectURL: vi.fn(),
  })
  vi.stubGlobal('window', {
    setTimeout: (handler: () => void, ms?: number) => setTimeout(handler, ms),
    clearTimeout: (id: number) => clearTimeout(id),
  })
  vi.stubGlobal('requestAnimationFrame', vi.fn(() => 1))
  vi.stubGlobal('cancelAnimationFrame', vi.fn())
}

function setupComposable(options: { continuous?: boolean } = {}) {
  const store = useReaderStore()
  store.setSpeechProvider('http')
  store.setHttpTtsId(7, 'audio/wav')
  store.book = {
    name: '测试书籍',
    author: '测试作者',
    origin: 'test-source',
    bookUrl: 'https://example.test/stream-book',
  }
  store.chapters = [
    { title: '第一章', url: 'chapter-0', index: 0 },
    { title: '第二章', url: 'chapter-1', index: 1 },
  ]
  store.currentIndex = 0

  const chapterText = ref<HTMLElement | undefined>({
    querySelectorAll: (selector: string) => (selector === 'p' ? paragraphs : []),
  } as unknown as HTMLElement)
  const scrollContainer = ref<HTMLElement | undefined>({
    querySelectorAll: (selector: string) => (
      options.continuous && selector.includes('continuous')
        // 连续阅读：容器里是若干 .chapter-text 根节点，段落挂在根节点下。
        ? [{ querySelectorAll: () => paragraphs }]
        : []
    ),
    scrollTo: vi.fn(),
    scrollTop: 0,
    clientHeight: 600,
    scrollHeight: 1200,
  } as unknown as HTMLElement)

  const nextChapterMock = vi.fn(async () => {
    store.currentIndex += 1
    paragraphs = chapterParagraphs.get(store.currentIndex) || []
  })

  const playback = useReaderAutoPlayback(
    store,
    ref({ clickAction: 'next', autoScrollSpeed: 30 }) as never,
    ref(Boolean(options.continuous)) as never,
    scrollContainer,
    chapterText,
    nextChapterMock,
    vi.fn(),
    ref(false) as never,
    ref(0) as never,
  )
  return { store, playback, nextChapterMock }
}

function playingAudio(): FakeAudio | undefined {
  return audioInstances.find((audio) => !audio.paused)
}

describe('useReaderAutoPlayback buffered speech stream', () => {
  beforeEach(() => {
    setActivePinia(createPinia())
    stubStorage()
    stubGlobals()
    vi.mocked(getBookContent).mockReset()
    vi.mocked(getBookInfo).mockReset()
    vi.mocked(getChapterList).mockReset()
    vi.mocked(saveBookProgress).mockReset()
    vi.mocked(syncLegadoBookProgress).mockReset()
    vi.mocked(uploadLegadoBookProgress).mockReset()
    vi.mocked(requestOpenAISpeechAudio).mockReset()
    vi.mocked(synthesizeHttpTts).mockReset()
    vi.mocked(synthesizeEdgeTts).mockReset()
    vi.mocked(synthesizeMsTts).mockReset()

    paragraphs = [new FakeParagraph('第一段正文。'), new FakeParagraph('第二段正文。')]
    chapterParagraphs = new Map([
      [0, paragraphs],
      [1, [new FakeParagraph('下一章正文。')]],
    ])
    vi.mocked(getBookContent).mockImplementation(async ({ chapterUrl }) => {
      if (chapterUrl === 'chapter-1') return '下一章正文。'
      return '第一段正文。\n第二段正文。'
    })
    vi.mocked(synthesizeHttpTts).mockResolvedValue(new Blob(['audio'], { type: 'audio/wav' }))
  })

  afterEach(() => {
    vi.unstubAllGlobals()
  })

  it('plays paragraphs through the buffer stream and switches chapter', async () => {
    const { store, playback, nextChapterMock } = setupComposable()

    playback.startSpeech(paragraphs[0] as unknown as HTMLElement)

    // 首块合成完立即播（不等整章），高亮跟随当前段落。
    await vi.waitFor(() => expect(store.isSpeaking).toBe(true))
    expect(paragraphs[0].classList.contains('reading')).toBe(true)
    // 缓冲流会提前合成后续段落/下一章（配额 500 字足以容纳这三小块）。
    await vi.waitFor(() => expect(vi.mocked(synthesizeHttpTts).mock.calls.length).toBeGreaterThanOrEqual(2))

    playingAudio()?.onended?.()
    await vi.waitFor(() => expect(paragraphs[1].classList.contains('reading')).toBe(true))

    // 第一段读完时，下一章正文应已预取进缓存（跨章预取躲在这段播放时间里）。
    await vi.waitFor(() => {
      expect(vi.mocked(getBookContent).mock.calls.some(([params]) => params.chapterUrl === 'chapter-1')).toBe(true)
    })

    playingAudio()?.onended?.()
    await vi.waitFor(() => expect(nextChapterMock).toHaveBeenCalledTimes(1))
    await vi.waitFor(() => expect(store.currentIndex).toBe(1))
    await vi.waitFor(() => expect(paragraphs[0].classList.contains('reading')).toBe(true))

    store.stopTTS()
    expect(audioInstances.every((audio) => audio.paused)).toBe(true)
  })

  it('stops the stream when TTS is stopped mid-buffer', async () => {
    const { store, playback } = setupComposable()
    let release: ((blob: Blob) => void) | undefined
    vi.mocked(synthesizeHttpTts).mockImplementation(() => new Promise<Blob>((resolve) => {
      release = resolve
    }))

    playback.startSpeech(paragraphs[0] as unknown as HTMLElement)
    await vi.waitFor(() => expect(store.isSpeechLoading).toBe(true))

    store.stopTTS()
    release?.(new Blob(['audio']))

    // stop 之后迟到的合成结果不能再开始播放。
    await new Promise((resolve) => setTimeout(resolve, 30))
    expect(audioInstances.every((audio) => audio.paused)).toBe(true)
    expect(store.isSpeaking).toBe(false)
    expect(store.isSpeechLoading).toBe(false)
  })

  it('splits long paragraphs into sentence chunks for the edge channel', async () => {
    const { store, playback } = setupComposable()
    store.setSpeechProvider('edge')
    store.setEdgeVoice('zh-CN-XiaoxiaoNeural')
    vi.mocked(synthesizeEdgeTts).mockResolvedValue(new Blob(['audio'], { type: 'audio/mpeg' }))
    const longText = Array.from({ length: 12 }, (_, index) => `第${index + 1}句内容稍微长一点。`).join('')
    paragraphs = [new FakeParagraph(longText)]
    chapterParagraphs.set(0, paragraphs)

    playback.startSpeech(paragraphs[0] as unknown as HTMLElement)

    // edge 与 openai 一样按 ≤70 字切块：首播只等第一个小块。
    await vi.waitFor(() => expect(vi.mocked(synthesizeEdgeTts).mock.calls.length).toBeGreaterThanOrEqual(2))
    const calls = vi.mocked(synthesizeEdgeTts).mock.calls
    expect(calls.every(([params]) => params.text.length <= 70)).toBe(true)
    expect(calls.every(([params]) => params.voice === 'zh-CN-XiaoxiaoNeural')).toBe(true)
    expect(calls.every(([params]) => params.rate === '+0%')).toBe(true)
    // 本章长段落被切块后再拼回原文（跨章预取的「下一章正文。」不计入）。
    const chapterTexts = calls
      .map(([params]) => params.text)
      .filter((text) => text !== '下一章正文。')
    expect(chapterTexts.join('')).toBe(longText)

    store.stopTTS()
  })

  it('splits long paragraphs into sentence chunks for the msdemo channel', async () => {
    const { store, playback } = setupComposable()
    store.setSpeechProvider('msdemo')
    store.setMsVoice('zh-CN-YunjianNeural')
    store.setMsStyle('newscast')
    vi.mocked(synthesizeMsTts).mockResolvedValue(new Blob(['audio'], { type: 'audio/mpeg' }))
    const longText = Array.from({ length: 12 }, (_, index) => `第${index + 1}句内容稍微长一点。`).join('')
    paragraphs = [new FakeParagraph(longText)]
    chapterParagraphs.set(0, paragraphs)

    playback.startSpeech(paragraphs[0] as unknown as HTMLElement)

    // 微软朗读与 openai/edge 一样按 ≤70 字切块走缓冲流。
    await vi.waitFor(() => expect(vi.mocked(synthesizeMsTts).mock.calls.length).toBeGreaterThanOrEqual(2))
    const calls = vi.mocked(synthesizeMsTts).mock.calls
    expect(calls.every(([params]) => params.text.length <= 70)).toBe(true)
    expect(calls.every(([params]) => params.voice === 'zh-CN-YunjianNeural')).toBe(true)
    expect(calls.every(([params]) => params.style === 'newscast')).toBe(true)
    expect(calls.every(([params]) => params.format === '96k')).toBe(true)
    // 本章长段落被切块后再拼回原文（跨章预取不计入）。
    const chapterTexts = calls
      .map(([params]) => params.text)
      .filter((text) => text !== '下一章正文。')
    expect(chapterTexts.join('')).toBe(longText)

    store.stopTTS()
  })

  it('skips punctuation-only paragraphs instead of synthesizing them', async () => {
    const { store, playback } = setupComposable()
    paragraphs = [
      new FakeParagraph('第一段正文。'),
      new FakeParagraph('……'),
      new FakeParagraph('第三段正文。'),
    ]
    chapterParagraphs.set(0, paragraphs)

    playback.startSpeech(paragraphs[0] as unknown as HTMLElement)
    await vi.waitFor(() => expect(store.isSpeaking).toBe(true))

    // 纯标点段直接丢弃：既不发请求，也不报错。
    const texts = vi.mocked(synthesizeHttpTts).mock.calls.map(([params]) => params.text)
    expect(texts).not.toContain('……')
    expect(texts).toContain('第一段正文。')

    // 跳过标点段后继续播下一段，朗读不中断。
    playingAudio()?.onended?.()
    await vi.waitFor(() => expect(paragraphs[2].classList.contains('reading')).toBe(true))
    expect(store.isSpeaking).toBe(true)
    store.stopTTS()
  })

  it('drops quote-only dialogue segments when dual voice is on', async () => {
    const { store, playback } = setupComposable()
    store.setDualVoiceEnabled(true)
    paragraphs = [new FakeParagraph('他说：“……”')]
    chapterParagraphs.set(0, paragraphs)

    playback.startSpeech(paragraphs[0] as unknown as HTMLElement)

    await vi.waitFor(() => expect(vi.mocked(synthesizeHttpTts).mock.calls.length).toBeGreaterThanOrEqual(1))
    const chapterTexts = vi.mocked(synthesizeHttpTts).mock.calls
      .map(([params]) => params.text)
      .filter((text) => text !== '下一章正文。')
    // 对话段只剩两个引号（无可朗读内容）被丢弃，旁白段照常朗读。
    expect(chapterTexts).toEqual(['他说：'])
    store.stopTTS()
  })

  it('shows the skipped-chunk warning only once per session', async () => {
    const { store, playback } = setupComposable()
    const appStore = useAppStore()
    vi.mocked(synthesizeHttpTts).mockRejectedValue(new Error('朗读引擎请求失败'))

    playback.startSpeech(paragraphs[0] as unknown as HTMLElement)

    await vi.waitFor(() => expect(appStore.toasts.length).toBeGreaterThan(0), { timeout: 3000 })
    // 等重试+跳过两个段落都发生：同类错误只提示一次。
    await new Promise((resolve) => setTimeout(resolve, 900))
    expect(appStore.toasts.filter((toast) => toast.type === 'warning')).toHaveLength(1)
    expect(appStore.toasts.at(-1)?.message).toContain('已跳过朗读失败的片段')
    store.stopTTS()
  })

  it('双音色下说话人冒号后的句子走对话音色', async () => {
    const { store, playback } = setupDualVoiceEdge()

    paragraphs = [new FakeParagraph('他笑道：明天见。')]
    chapterParagraphs.set(0, paragraphs)

    playback.startSpeech(paragraphs[0] as unknown as HTMLElement)

    await vi.waitFor(() => expect(chapterSpeechCalls().length).toBeGreaterThanOrEqual(2))
    const calls = chapterSpeechCalls()
    // 「他笑道：」是旁白（说话人标签），冒号后的台词才是对话。
    expect(calls.filter((call) => call.voice === NARRATION_VOICE).map((call) => call.text))
      .toEqual(['他笑道：'])
    expect(calls.filter((call) => call.voice === DIALOGUE_VOICE).map((call) => call.text))
      .toEqual(['明天见。'])
    store.stopTTS()
  })

  it('双音色下一句长对话被切成多块时，每一块都还是对话音色', async () => {
    const { store, playback } = setupDualVoiceEdge()

    // 用户报的原始形态：一整段长对话（不是跨段），前半截对话音色、后半截变旁白。
    const body = '这是一句很长的话。'.repeat(8)
    paragraphs = [new FakeParagraph(`张三：“${body}”`)]
    chapterParagraphs.set(0, paragraphs)

    playback.startSpeech(paragraphs[0] as unknown as HTMLElement)

    // 长对话按 ≤70 字切成多块，等至少两块对话块出现。
    await vi.waitFor(() => {
      expect(chapterSpeechCalls().filter((call) => call.voice === DIALOGUE_VOICE).length)
        .toBeGreaterThanOrEqual(2)
    })
    const calls = chapterSpeechCalls()
    expect(calls.filter((call) => call.voice === NARRATION_VOICE).map((call) => call.text))
      .toEqual(['张三：'])
    expect(calls.filter((call) => call.voice === DIALOGUE_VOICE).map((call) => call.text).join(''))
      .toBe(`“${body}”`)
    store.stopTTS()
  })

  it('用户手动跳章时朗读跟到新章开头（不是继续播旧章）', async () => {
    const { store, playback } = setupComposable()
    const calls: Array<{ text: string; at: number }> = []
    vi.mocked(synthesizeHttpTts).mockImplementation(async ({ text }) => {
      calls.push({ text, at: Date.now() })
      return new Blob(['audio'], { type: 'audio/wav' })
    })

    playback.startSpeech(paragraphs[0] as unknown as HTMLElement)
    await vi.waitFor(() => expect(store.isSpeaking).toBe(true))
    // 先等跨章预取自己把「下一章正文。」合成掉，这样后面同名的请求就只能是跟随重启发出的。
    await vi.waitFor(
      () => expect(calls.some((call) => call.text === '下一章正文。')).toBe(true),
      { timeout: 3000 },
    )

    const jumpAt = Date.now()
    // 模拟目录/进度条跳章：只改索引与正文 DOM，不经过朗读自己的切章流程。
    store.currentIndex = 1
    paragraphs = chapterParagraphs.get(1) || []

    await vi.waitFor(
      () => expect(calls.some((call) => call.text === '下一章正文。' && call.at >= jumpAt)).toBe(true),
      { timeout: 4000 },
    )
    await vi.waitFor(() => expect(store.isSpeaking).toBe(true), { timeout: 4000 })
    await vi.waitFor(() => expect(paragraphs[0].classList.contains('reading')).toBe(true), { timeout: 4000 })
    store.stopTTS()
  })

  it('暂停中手动跳章只丢弃旧位置，不自动出声', async () => {
    const { store, playback } = setupComposable()

    playback.startSpeech(paragraphs[0] as unknown as HTMLElement)
    await vi.waitFor(() => expect(store.isSpeaking).toBe(true))
    store.pauseTTS()
    expect(store.isPaused).toBe(true)

    store.currentIndex = 1
    paragraphs = chapterParagraphs.get(1) || []

    // 等过防抖窗口：跟随只停流，不应把暂停态变成自动播放。
    await new Promise((resolve) => setTimeout(resolve, 800))
    expect(store.isPaused).toBe(false)
    expect(store.isSpeaking).toBe(false)
    expect(audioInstances.every((audio) => audio.paused)).toBe(true)
  })

  it('跳章跟随还没落地时按停止，不会过一会儿自己开始朗读', async () => {
    const { store, playback } = setupComposable()
    const calls: string[] = []
    vi.mocked(synthesizeHttpTts).mockImplementation(async ({ text }) => {
      calls.push(text)
      return new Blob(['audio'], { type: 'audio/wav' })
    })

    playback.startSpeech(paragraphs[0] as unknown as HTMLElement)
    await vi.waitFor(() => expect(store.isSpeaking).toBe(true))

    store.currentIndex = 1
    paragraphs = chapterParagraphs.get(1) || []
    // 用户跳章后立刻点停止/关闭听书：挂起的跟随必须作废。
    playback.cancelPendingChapterFollow()
    store.stopTTS()
    const countAfterStop = calls.length

    await new Promise((resolve) => setTimeout(resolve, 900))
    expect(calls.length).toBe(countAfterStop)
    expect(store.isSpeaking).toBe(false)
  })

  it('只调 store.stopTTS()（快捷键/定时停止）也会取消挂起的跳章跟随', async () => {
    const { store, playback } = setupComposable()
    const calls: string[] = []
    vi.mocked(synthesizeHttpTts).mockImplementation(async ({ text }) => {
      calls.push(text)
      return new Blob(['audio'], { type: 'audio/wav' })
    })

    playback.startSpeech(paragraphs[0] as unknown as HTMLElement)
    await vi.waitFor(() => expect(store.isSpeaking).toBe(true))

    // 用户跳章 → 正文还没回来时用快捷键停止朗读（不经过面板的停止按钮）。
    store.currentIndex = 1
    paragraphs = chapterParagraphs.get(1) || []
    store.stopTTS()
    const countAfterStop = calls.length

    await new Promise((resolve) => setTimeout(resolve, 900))
    expect(calls.length).toBe(countAfterStop)
    expect(store.isSpeaking).toBe(false)
  })

  it('连续阅读跳章从目标章第一段开始，而不是 DOM 里上一章的第一段', async () => {
    const { store, playback } = setupComposable({ continuous: true })
    const calls: string[] = []
    vi.mocked(synthesizeHttpTts).mockImplementation(async ({ text }) => {
      calls.push(text)
      return new Blob(['audio'], { type: 'audio/wav' })
    })

    // 上一章给足长度：起点选错时，跟随会逐段「跳过」上一章（每段约 120ms），
    // 40 段就是好几秒 —— 与用户实际遇到的「跳章后半天不出声」一致。
    const previousChapter = Array.from(
      { length: 40 },
      (_, index) => new FakeParagraph(`上一章第${index + 1}段。`, 0),
    )
    paragraphs = previousChapter
    chapterParagraphs.set(0, previousChapter)
    chapterParagraphs.set(1, [
      new FakeParagraph('新章第一段。', 1),
      new FakeParagraph('新章第二段。', 1),
    ])

    playback.startSpeech(previousChapter[0] as unknown as HTMLElement)
    await vi.waitFor(() => expect(store.isSpeaking).toBe(true))
    await new Promise((resolve) => setTimeout(resolve, 300))

    // 连续阅读切章后，DOM 里同时挂着上一章与目标章。
    const targetChapter = chapterParagraphs.get(1) || []
    paragraphs = [...previousChapter, ...targetChapter]
    const countBeforeJump = calls.length
    store.currentIndex = 1

    await vi.waitFor(
      () => expect(calls.slice(countBeforeJump)).toContain('新章第一段。'),
      { timeout: 2500, interval: 50 },
    )
    // 起点必须是目标章：bug 版本会先把上一章整章「空跳」一遍。
    expect(calls.slice(countBeforeJump)).not.toContain('上一章第1段。')
    store.stopTTS()
  })

  it('TTS 自己滚到章边界引起索引变化时不重启朗读', async () => {
    const { store, playback } = setupComposable({ continuous: true })
    const calls: string[] = []
    vi.mocked(synthesizeHttpTts).mockImplementation(async ({ text }) => {
      calls.push(text)
      return new Blob(['audio'], { type: 'audio/wav' })
    })

    paragraphs = [
      new FakeParagraph('本章第一段。', 0),
      new FakeParagraph('本章第二段。', 0),
    ]
    chapterParagraphs.set(0, paragraphs)
    chapterParagraphs.set(1, [new FakeParagraph('下一章第一段。', 1)])

    playback.startSpeech(paragraphs[0] as unknown as HTMLElement)
    await vi.waitFor(() => expect(store.isSpeaking).toBe(true))
    await new Promise((resolve) => setTimeout(resolve, 300))
    const countBeforeScroll = calls.length

    // 模拟 handleScroll 反推索引：TTS 自己滚过章边界。
    playback.markChapterChangeFromScroll()
    store.currentIndex = 1

    await new Promise((resolve) => setTimeout(resolve, 900))
    // 没有重启：不会重新合成新章；旧流仍在播。
    expect(calls.length).toBe(countBeforeScroll)
    expect(store.isSpeaking).toBe(true)
    store.stopTTS()
  })

  it('用户滚轮/触摸之后的自滚窗口内跳章，仍然跟随', async () => {
    const { store, playback } = setupComposable({ continuous: true })
    const calls: string[] = []
    vi.mocked(synthesizeHttpTts).mockImplementation(async ({ text }) => {
      calls.push(text)
      return new Blob(['audio'], { type: 'audio/wav' })
    })

    paragraphs = [new FakeParagraph('本章第一段。', 0)]
    chapterParagraphs.set(0, paragraphs)
    chapterParagraphs.set(1, [new FakeParagraph('新章第一段。', 1)])

    playback.startSpeech(paragraphs[0] as unknown as HTMLElement)
    await vi.waitFor(() => expect(store.isSpeaking).toBe(true))
    await new Promise((resolve) => setTimeout(resolve, 300))
    const countBeforeJump = calls.length

    // 用户在 TTS 刚滚过（1.5 秒保护窗口内）自己跳章：必须是真实跟随。
    paragraphs = [
      new FakeParagraph('本章第一段。', 0),
      new FakeParagraph('新章第一段。', 1),
    ]
    playback.markChapterChangeFromScroll()
    playback.markManualReadingInput()
    store.currentIndex = 1

    await vi.waitFor(
      () => expect(calls.slice(countBeforeJump)).toContain('新章第一段。'),
      { timeout: 4000 },
    )
    store.stopTTS()
  })
})

/* ─── 双音色 + edge 的运行时脚手架（上面的用例共用） ─── */

const NARRATION_VOICE = 'zh-CN-YunxiNeural'
const DIALOGUE_VOICE = 'zh-CN-XiaoxiaoNeural'

/** 切到 edge + 双音色，旁白与对话各用一套可区分的音色。 */
function setupDualVoiceEdge() {
  const result = setupComposable()
  result.store.setSpeechProvider('edge')
  result.store.setEdgeVoice(NARRATION_VOICE)
  result.store.setEdgeDialogueVoice(DIALOGUE_VOICE)
  result.store.setDualVoiceEnabled(true)
  vi.mocked(synthesizeEdgeTts).mockResolvedValue(new Blob(['audio'], { type: 'audio/mpeg' }))
  return result
}

/** 本章的 edge 合成请求（排除跨章预取的那一条）。 */
function chapterSpeechCalls() {
  return vi.mocked(synthesizeEdgeTts).mock.calls
    .map(([params]) => params)
    .filter((params) => params.text !== '下一章正文。')
}
