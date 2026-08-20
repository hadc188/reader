import { defineStore } from 'pinia'
import { ref, computed, reactive, watch } from 'vue'
import { isTauri } from '@tauri-apps/api/core'
import { useAppStore } from './app'
import { useBookshelfStore } from './bookshelf'
import {
  getChapterList,
  getBookContent,
  saveBookProgress,
  setBookSource as apiSetBookSource,
} from '../api/bookshelf'
import { invokeRaw } from '../api/invoke'
import {
  getBookmarks,
  saveBookmark,
  deleteBookmark as apiDeleteBookmark,
  deleteBookmarks as apiDeleteBookmarks,
} from '../api/bookmark'
import { getReplaceRules } from '../api/replaceRule'
import type { Book, BookChapter, Bookmark, ReplaceRule } from '../types'
import { isLocalBook } from '../utils/localBook'
import { saveRecentReadBook } from '../utils/recentBooks'
import {
  DEFAULT_READER_BACKGROUND_OPACITY,
  normalizeReaderBackgroundOpacity,
} from '../utils/readerBackground'
import {
  DEFAULT_OPENAI_BASE_URL,
  getSpeechApiFormatOption,
  inferSpeechApiFormat,
  requestOpenAISpeechAudio,
  type SpeechApiFormat,
  type SpeechAudioFormat,
} from '../utils/openaiSpeech'
import { syncLegadoBookProgress, type LegadoBookProgress } from '../api/webdav'
import { deleteCustomFont, listCustomFonts, uploadCustomFont, type CustomFontEntry } from '../api/fonts'

const READER_SESSION_KEY = 'reader-last-session'
const READER_READ_HISTORY_PREFIX = 'reader-read-history:'
const READER_BACKGROUND_IMAGE_KEY = 'reader-background-image'
const SERVER_PROGRESS_SCALE = 10000
// 目录自动检查: 打开书后台静默检查的节流间隔 / 读到末章再翻页触发的检查节流间隔
const TOC_OPEN_CHECK_INTERVAL_MS = 30 * 60 * 1000
export const TOC_END_CHECK_INTERVAL_MS = 60 * 1000
const TOC_CHECK_TIMES_KEY = 'reader-toc-check-times'
const TOC_CHECK_TIMES_LIMIT = 200

interface PersistedReaderSession {
  book: Book
  chapters: BookChapter[]
  currentIndex: number
  chapterScrollProgress: number
  updatedAt: number
}

/* ─── Reading config type ─── */
export interface ReadConfig {
  fontSize: number
  fontWeight: number
  fontFamily: string
  lineHeight: number
  paragraphSpacing: number
  firstLineIndent: boolean
  fontColor: string
  pageWidth: number
  readMethod: '上下滑动' | '左右翻页' | '上下滚动' | '上下滚动2'
  animateDuration: number
  /** 自动阅读滚动速度, 像素/秒。 */
  autoScrollSpeed: number
  clickAction: 'next' | 'auto' | 'none'
  selectAction: 'popup' | 'ignore'
  chineseMode: 'simplified' | 'traditional'
  enablePreload: boolean
  backgroundImage: string
  backgroundOpacity: number
  applyBackgroundToReader: boolean
}

const defaultConfig: ReadConfig = {
  fontSize: 18,
  fontWeight: 400,
  fontFamily: 'system',
  lineHeight: 1.8,
  paragraphSpacing: 0.2,
  firstLineIndent: true,
  fontColor: '',
  pageWidth: 800,
  readMethod: '上下滑动',
  animateDuration: 300,
  autoScrollSpeed: 30,
  clickAction: 'auto',
  selectAction: 'ignore',
  chineseMode: 'simplified',
  enablePreload: false,
  backgroundImage: '',
  backgroundOpacity: DEFAULT_READER_BACKGROUND_OPACITY,
  applyBackgroundToReader: true,
}

function loadConfig(): ReadConfig {
  try {
    const saved = localStorage.getItem('readConfig')
    if (saved) {
      const parsed = JSON.parse(saved) as Partial<ReadConfig> & {
        specialMode?: unknown
        autoPageMode?: unknown
        scrollPixel?: number
        pageSpeed?: number
      }
      const {
        specialMode: _legacySpecialMode,
        autoPageMode: _legacyAutoPageMode,
        scrollPixel: legacyScrollPixel,
        pageSpeed: legacyPageSpeed,
        ...savedConfig
      } = parsed
      // 旧版像素滚动速度 = scrollPixel × pageSpeed/1000 × 0.5 像素/帧(60fps),
      // 折算成像素/秒 = scrollPixel × pageSpeed × 0.03, 默认组合恰为 30。
      const legacyAutoScrollSpeed = typeof legacyScrollPixel === 'number' && typeof legacyPageSpeed === 'number'
        ? legacyScrollPixel * legacyPageSpeed * 0.03
        : defaultConfig.autoScrollSpeed
      return {
        ...defaultConfig,
        ...savedConfig,
        autoScrollSpeed: typeof savedConfig.autoScrollSpeed === 'number' && savedConfig.autoScrollSpeed > 0
          ? Math.min(500, Math.max(2, Math.round(savedConfig.autoScrollSpeed)))
          : Math.min(500, Math.max(2, Math.round(legacyAutoScrollSpeed))),
        backgroundImage: localStorage.getItem(READER_BACKGROUND_IMAGE_KEY)
          || (typeof savedConfig.backgroundImage === 'string' ? savedConfig.backgroundImage : ''),
        backgroundOpacity: normalizeReaderBackgroundOpacity(savedConfig.backgroundOpacity),
        applyBackgroundToReader: typeof savedConfig.applyBackgroundToReader === 'boolean'
          ? savedConfig.applyBackgroundToReader
          : true,
      }
    }
  } catch { /* ignore */ }
  return { ...defaultConfig }
}

/* ─── Theme presets ─── */
export interface ThemePreset {
  name: string
  body: string
  content: string
  fontColor: string
  popup: string
}

export const themePresets: ThemePreset[] = [
  { name: '默认', body: '#f5ede4', content: '#fff9f0', fontColor: '#333', popup: '#fff' },
  { name: '纯白', body: '#ffffff', content: '#ffffff', fontColor: '#333', popup: '#fff' },
  { name: '琥珀', body: '#f5e6ce', content: '#faf0e4', fontColor: '#5b4636', popup: '#faf0e4' },
  { name: '薄荷', body: '#e0f0e8', content: '#eaf5ef', fontColor: '#2d4a3e', popup: '#eaf5ef' },
  { name: '天蓝', body: '#dce8f0', content: '#e8f0f6', fontColor: '#2c3e50', popup: '#e8f0f6' },
  { name: '粉白', body: '#f5e4e8', content: '#faf0f3', fontColor: '#4a2d36', popup: '#faf0f3' },
  { name: '浅灰', body: '#eaeaea', content: '#f5f5f5', fontColor: '#333', popup: '#f5f5f5' },
  { name: '暗灰', body: '#808080', content: '#999', fontColor: '#eee', popup: '#888' },
  { name: '暗夜', body: '#141414', content: '#16213e', fontColor: '#c8c8c8', popup: '#141414' },
]
export const nightThemeIndex = themePresets.length - 1

/* ─── Font presets ─── */
export const fontPresets = [
  { label: '系统', value: 'system', family: '' },
  { label: '黑体', value: 'heiti', family: '"SimHei", "STHeiti", "Heiti SC", sans-serif' },
  { label: '楷体', value: 'kaiti', family: '"KaiTi", "STKaiti", "BiauKai", serif' },
  { label: '宋体', value: 'songti', family: '"SimSun", "STSong", "Songti SC", serif' },
  { label: '仿宋', value: 'fangsong', family: '"FangSong", "STFangsong", serif' },
]

const CUSTOM_FONT_PREFIX = 'custom:'

interface TTSOptions {
  onStart?: () => void
  onProgress?: (progress: number) => void
  onEnd?: () => void
  onError?: (event?: SpeechSynthesisErrorEvent | Error) => void
}

interface PreloadedOpenAIAudio {
  key: string
  blob: Blob
}

const OPENAI_AUDIO_PRELOAD_LIMIT = 8

export type SpeechProvider = 'system' | 'openai'
export type OpenAISpeechSource = 'browser' | 'server'
export type OpenAISpeechFormat = SpeechAudioFormat
export type OpenAISpeechRequestMode = 'chunked' | 'merged'

interface SpeechConfig {
  provider: SpeechProvider
  voiceName: string
  speechRate: number
  speechPitch: number
  stopAfterMinutes: number
  openaiSource: OpenAISpeechSource
  apiFormat: SpeechApiFormat
  openaiBaseUrl: string
  speechProxyUrl: string
  openaiApiKey: string
  openaiModel: string
  openaiVoice: string
  openaiFormat: OpenAISpeechFormat
  openaiRequestMode: OpenAISpeechRequestMode
}

const defaultSpeechConfig: SpeechConfig = {
  provider: 'system',
  voiceName: '',
  speechRate: 1,
  speechPitch: 1,
  stopAfterMinutes: 0,
  openaiSource: 'browser',
  apiFormat: 'openai',
  openaiBaseUrl: DEFAULT_OPENAI_BASE_URL,
  speechProxyUrl: '',
  openaiApiKey: '',
  openaiModel: 'qwen-tts',
  openaiVoice: 'vivian',
  openaiFormat: 'mp3',
  openaiRequestMode: 'chunked',
}

function loadSpeechConfig(): SpeechConfig {
  try {
    const saved = localStorage.getItem('reader-speechConfig')
    if (saved) {
      const parsed = JSON.parse(saved) as Partial<SpeechConfig>
      const apiFormat = ['openai', 'fish', 'elevenlabs', 'azure'].includes(parsed.apiFormat || '')
        ? parsed.apiFormat as SpeechApiFormat
        : inferSpeechApiFormat(parsed.openaiBaseUrl || defaultSpeechConfig.openaiBaseUrl)
      return { ...defaultSpeechConfig, ...parsed, apiFormat }
    }
  } catch { /* ignore */ }
  return { ...defaultSpeechConfig }
}

function isSafariSpeechFallbackMode() {
  if (typeof navigator === 'undefined') return false
  const ua = navigator.userAgent || ''
  const vendor = navigator.vendor || ''
  const isAppleEngine = /Apple/i.test(vendor) || /iPhone|iPad|iPod/i.test(ua)
  return isAppleEngine && /Safari/i.test(ua) && !/Chrome|Chromium|CriOS|Edg|EdgiOS|Firefox|FxiOS|OPR|OPT|SamsungBrowser|Android/i.test(ua)
}

export const useReaderStore = defineStore('reader', () => {
  type ReaderPanel = 'catalog' | 'settings' | 'bookshelf' | 'source' | 'bookmark' | 'rule' | 'cache' | null
  const appStore = useAppStore()
  const shelfStore = useBookshelfStore()
  const book = ref<Book | null>(null)
  const chapters = ref<BookChapter[]>([])
  const currentIndex = ref(0)
  const content = ref('')
  const loading = ref(false)
  /** 打开书时的入口位置快照(书架记录的章节/进度/时间)。
   *  位置恢复只能用稳定来源(localStorage + 本快照); 活状态(durChapterPos 等)
   *  会被初始化滚动等 UI 过程改写, 读它做恢复会拿到清零后的假数据。 */
  const openPosition = ref<{ index: number; position: number; time: number } | null>(null)

  function snapshotOpenPosition(source: Book) {
    openPosition.value = {
      index: source.durChapterIndex ?? 0,
      position: typeof source.durChapterPos === 'number' ? source.durChapterPos : 0,
      time: source.durChapterTime ?? 0,
    }
  }
  const chaptersLoading = ref(false)
  const bookmarks = ref<Bookmark[]>([])
  const replaceRules = ref<ReplaceRule[]>([])
  const preloadedContent = ref<Map<number, string>>(new Map()) // index -> content
  const preloadingContent = new Map<number, Promise<string | null>>()
  let chapterPreloadGeneration = 0
  const isAutoScrolling = ref(false)
  const chapterScrollProgress = ref(0)
  const readChapterKeys = ref<Set<string>>(new Set())
  const progressDirty = ref(false)
  const lastServerProgressKey = ref('')
  const pendingLegadoProgress = ref<{ index: number; position: number } | null>(null)
  const customFonts = ref<CustomFontEntry[]>([])

  const currentChapter = computed(() => chapters.value[currentIndex.value] || null)
  const hasNext = computed(() => currentIndex.value < chapters.value.length - 1)
  const hasPrev = computed(() => currentIndex.value > 0)

  const readingProgress = computed(() => {
    if (chapters.value.length === 0) return '0%'
    const progress = ((currentIndex.value + chapterScrollProgress.value) / chapters.value.length) * 100
    const normalized = Math.max(0, Math.min(100, progress))
    return `${normalized < 10 ? normalized.toFixed(1) : Math.round(normalized)}%`
  })

  /* ─── Reading config ─── */
  const config = reactive<ReadConfig>(loadConfig())

  function customFontFamily(id: string) {
    return `ReaderCustom_${id.replace(/[^a-zA-Z0-9_]/g, '')}`
  }

  async function installCustomFontFace(font: CustomFontEntry) {
    const family = customFontFamily(font.id)
    // Load through a blob URL so the font does not depend on the webview's
    // cross-origin font handling. This also makes malformed files fail before
    // the font is offered as an active reading option.
    const response = await fetch(font.url, { cache: 'no-store' })
    if (!response.ok) throw new Error(`字体资源请求失败（${response.status}）`)
    const blobUrl = URL.createObjectURL(await response.blob())
    const face = new FontFace(family, `url("${blobUrl}")`)
    try {
      const loaded = await face.load()
      document.fonts.add(loaded)
      return true
    } catch (error) {
      throw new Error(`字体文件加载失败：${error instanceof Error ? error.message : '格式不受支持'}`)
    } finally {
      URL.revokeObjectURL(blobUrl)
    }
  }

  async function fetchCustomFonts() {
    customFonts.value = await listCustomFonts()
    const failedIds = new Set<string>()
    for (const font of customFonts.value) {
      try {
        await installCustomFontFace(font)
      } catch {
        // Ignore a stale or corrupt font during startup. It remains listed so
        // the user can remove it from the settings panel.
        failedIds.add(font.id)
      }
    }
    if (config.fontFamily.startsWith(CUSTOM_FONT_PREFIX)) {
      const id = config.fontFamily.slice(CUSTOM_FONT_PREFIX.length)
      if (!customFonts.value.some((font) => font.id === id) || failedIds.has(id)) {
        updateConfig('fontFamily', 'system')
      }
    }
    return customFonts.value
  }

  async function importCustomFont(file: File) {
    const font = await uploadCustomFont(file)
    try {
      await installCustomFontFace(font)
    } catch (error) {
      await deleteCustomFont(font.id).catch(() => undefined)
      throw error
    }
    customFonts.value.push(font)
    updateConfig('fontFamily', `${CUSTOM_FONT_PREFIX}${font.id}`)
    return font
  }

  async function removeCustomFont(id: string) {
    await deleteCustomFont(id)
    customFonts.value = customFonts.value.filter((font) => font.id !== id)
    if (config.fontFamily === `${CUSTOM_FONT_PREFIX}${id}`) {
      updateConfig('fontFamily', 'system')
    }
  }

  function saveConfig() {
    const persistedConfig: Partial<ReadConfig> = { ...config }
    delete persistedConfig.backgroundImage
    localStorage.setItem('readConfig', JSON.stringify(persistedConfig))
  }

  function updateConfig<K extends keyof ReadConfig>(key: K, value: ReadConfig[K]) {
    config[key] = value
    if (key === 'enablePreload' && !value) {
      preloadedContent.value.clear()
      preloadingContent.clear()
      chapterPreloadGeneration += 1
    }
    saveConfig()
  }

  function resetConfig() {
    const backgroundPreferences = {
      backgroundImage: config.backgroundImage,
      backgroundOpacity: config.backgroundOpacity,
      applyBackgroundToReader: config.applyBackgroundToReader,
    }
    Object.assign(config, defaultConfig)
    Object.assign(config, backgroundPreferences)
    saveConfig()
  }

  function setBackgroundImage(dataUrl: string) {
    try {
      localStorage.setItem(READER_BACKGROUND_IMAGE_KEY, dataUrl)
      config.backgroundImage = dataUrl
    } catch {
      throw new Error('背景图片保存失败，请选择尺寸更小的图片')
    }
  }

  function clearBackgroundImage() {
    localStorage.removeItem(READER_BACKGROUND_IMAGE_KEY)
    config.backgroundImage = ''
  }

  const chineseConverter = ref<((text: string) => string) | null>(null)
  let chineseLoading: Promise<void> | null = null

  async function ensureChineseConverterLoaded() {
    if (chineseConverter.value || chineseLoading) return chineseLoading || Promise.resolve()
    chineseLoading = import('../utils/chinese.js')
      .then((module) => {
        chineseConverter.value = module.traditionalized
      })
      .catch(() => {
        chineseConverter.value = null
      })
      .finally(() => {
        chineseLoading = null
      })
    return chineseLoading
  }

  /* ─── Theme ─── */
  const storedThemeIndex = Number.parseInt(localStorage.getItem('reader-themeIndex') || '0', 10)
  const themeIndex = ref(
    Number.isInteger(storedThemeIndex) && storedThemeIndex >= 0 && storedThemeIndex < nightThemeIndex
      ? storedThemeIndex
      : 0,
  )
  const isNight = ref(
    localStorage.getItem('reader-isNight') === 'true' || storedThemeIndex === nightThemeIndex,
  )

  const currentTheme = computed(() => {
    if (isNight.value) return themePresets[nightThemeIndex]
    return themePresets[themeIndex.value] || themePresets[0]
  })
  const chromeTheme = computed<ThemePreset>(() => {
    const activeTheme = currentTheme.value
    if (!config.backgroundImage || !config.applyBackgroundToReader) return activeTheme
    return {
      ...activeTheme,
      popup: `color-mix(in srgb, ${activeTheme.popup} 84%, transparent)`,
    }
  })

  function setThemeIndex(idx: number) {
    if (!Number.isInteger(idx) || idx < 0 || idx >= themePresets.length) return
    if (idx === nightThemeIndex) {
      isNight.value = true
      localStorage.setItem('reader-isNight', 'true')
      return
    }
    themeIndex.value = idx
    isNight.value = false
    localStorage.setItem('reader-themeIndex', String(idx))
    localStorage.setItem('reader-isNight', 'false')
  }

  function toggleNight() {
    isNight.value = !isNight.value
    localStorage.setItem('reader-isNight', String(isNight.value))
    if (!isNight.value) {
      localStorage.setItem('reader-themeIndex', String(themeIndex.value))
    }
  }

  /* ─── Chinese Conversion (OpenCC) ─── */
  /* ─── Content Filtering (Replace Rules) ─── */
  function applyReplaceRules(text: string) {
    if (!text) return ''
    let result = text
    const currentBook = book.value

    function matchRuleScope(rule: ReplaceRule) {
      const scope = (rule.scope || '').trim()
      if (!scope || scope === '*') return true
      if (!currentBook) return false

      if (scope.startsWith('source:')) {
        return scope.slice('source:'.length) === currentBook.origin
      }

      if (scope.startsWith('book:')) {
        return scope.slice('book:'.length) === currentBook.bookUrl
      }

      const scopeParts = scope.split(';')
      if (scopeParts[0] !== '*' && scopeParts[0] !== currentBook.name) {
        return false
      }
      return scopeParts.length === 1 || scopeParts[1] === currentBook.bookUrl
    }

    // Sort by order and apply enabled rules
    const enabledRules = [...replaceRules.value]
      .filter(r => r.isEnabled && matchRuleScope(r))
      .sort((a, b) => a.order - b.order)

    for (const rule of enabledRules) {
      try {
        if (rule.isRegex) {
          const re = new RegExp(rule.pattern, 'gm')
          result = result.replace(re, rule.replacement)
        } else {
          result = result.replaceAll(rule.pattern, rule.replacement)
        }
      } catch (e) {
        console.error(`Failed to apply rule: ${rule.name}`, e)
      }
    }
    return result
  }

  function convertContent(text: string) {
    if (!text || config.chineseMode !== 'traditional' || !chineseConverter.value) return text
    return chineseConverter.value(text)
  }

  function processContentForDisplay(text: string) {
    return convertContent(applyReplaceRules(text))
  }

  const displayContent = computed(() => {
    return processContentForDisplay(content.value)
  })

  watch(
    () => config.chineseMode,
    (mode) => {
      if (mode === 'traditional') {
        void ensureChineseConverterLoaded()
      }
    },
    { immediate: true },
  )

  function saveReaderSession() {
    if (!book.value || !chapters.value.length) return
    const payload: PersistedReaderSession = {
      book: book.value,
      chapters: chapters.value,
      currentIndex: currentIndex.value,
      chapterScrollProgress: chapterScrollProgress.value,
      updatedAt: Date.now(),
    }
    localStorage.setItem(READER_SESSION_KEY, JSON.stringify(payload))
  }

  function encodeServerProgress(progress = chapterScrollProgress.value) {
    return Math.max(
      0,
      Math.min(SERVER_PROGRESS_SCALE, Math.round(Math.max(0, Math.min(1, progress)) * SERVER_PROGRESS_SCALE)),
    )
  }

  function decodeServerProgress(position?: number | null) {
    if (typeof position !== 'number' || Number.isNaN(position)) return 0
    const normalized = position > 1 ? position / SERVER_PROGRESS_SCALE : position
    return Math.max(0, Math.min(1, normalized))
  }

  function currentServerProgressPayload(index = currentIndex.value, progress = chapterScrollProgress.value) {
    if (!book.value) return null
    return {
      bookUrl: book.value.bookUrl,
      index,
      position: encodeServerProgress(progress),
    }
  }

  function markProgressDirty() {
    progressDirty.value = true
  }

  function syncLocalBookProgress(progress = chapterScrollProgress.value) {
    if (!book.value) return
    const encodedProgress = encodeServerProgress(progress)
    book.value.durChapterPos = encodedProgress
    const shelfBook = shelfStore.books.find((item) => item.bookUrl === book.value?.bookUrl)
    if (shelfBook) {
      shelfBook.durChapterPos = encodedProgress
    }
  }

  function getPersistedReaderSession(): PersistedReaderSession | null {
    try {
      const raw = localStorage.getItem(READER_SESSION_KEY)
      if (!raw) return null
      return JSON.parse(raw) as PersistedReaderSession
    } catch {
      return null
    }
  }

  async function restorePersistedSession() {
    const session = getPersistedReaderSession()
    if (!session?.book || !session.chapters?.length) return false

    book.value = session.book
    snapshotOpenPosition(session.book)
    chapters.value = session.chapters
    loadReadChapterHistory(session.book)

    const nextIndex = Math.max(0, Math.min(session.currentIndex || 0, session.chapters.length - 1))
    try {
      const chapterContent = await fetchChapterContent(nextIndex)
      if (chapterContent == null) return false
      cachePreloadedContent(nextIndex, chapterContent)
      if (config.enablePreload) void preloadAroundChapter(nextIndex)
      const persistedProgressTime = session.book.durChapterTime || 0
      setActiveChapterState(nextIndex, chapterContent, session.chapterScrollProgress || 0)
      if (book.value) {
        book.value.durChapterTime = persistedProgressTime
        const shelfBook = shelfStore.books.find((item) => item.bookUrl === book.value?.bookUrl)
        if (shelfBook) shelfBook.durChapterTime = persistedProgressTime
      }
      markChapterAsRead(nextIndex)
      await restoreCurrentBookProgressFromLegado(chapterContent).catch((error) => {
        appStore.showToast((error as Error).message || '读取网盘阅读进度失败', 'warning')
      })
      return true
    } catch {
      return false
    }
  }

  function getReadHistoryStorageKey(currentBook?: Book | null) {
    if (!currentBook?.bookUrl) return ''
    return `${READER_READ_HISTORY_PREFIX}${currentBook.bookUrl}`
  }

  function buildReadChapterKey(index: number, chapter?: BookChapter | null, currentBook?: Book | null) {
    if (!currentBook?.bookUrl) return ''
    const sourceKey = currentBook.origin || 'default'
    if (chapter?.url) {
      return `${currentBook.bookUrl}::${sourceKey}::${chapter.url}`
    }
    return `${currentBook.bookUrl}::${sourceKey}::index:${index}`
  }

  function loadReadChapterHistory(currentBook?: Book | null) {
    const storageKey = getReadHistoryStorageKey(currentBook)
    if (!storageKey) {
      readChapterKeys.value = new Set()
      return
    }
    try {
      const raw = localStorage.getItem(storageKey)
      if (!raw) {
        readChapterKeys.value = new Set()
        return
      }
      const parsed = JSON.parse(raw)
      readChapterKeys.value = new Set(Array.isArray(parsed) ? parsed.filter((item) => typeof item === 'string') : [])
    } catch {
      readChapterKeys.value = new Set()
    }
  }

  function persistReadChapterHistory(currentBook?: Book | null) {
    const storageKey = getReadHistoryStorageKey(currentBook)
    if (!storageKey) return
    localStorage.setItem(storageKey, JSON.stringify(Array.from(readChapterKeys.value)))
  }

  function markChapterAsRead(index: number) {
    const key = buildReadChapterKey(index, chapters.value[index], book.value)
    if (!key || readChapterKeys.value.has(key)) return
    const next = new Set(readChapterKeys.value)
    next.add(key)
    readChapterKeys.value = next
    persistReadChapterHistory(book.value)
  }

  function isChapterRead(index: number) {
    return readChapterKeys.value.has(buildReadChapterKey(index, chapters.value[index], book.value))
  }

  /* ─── Auto reading ─── */
  const autoReading = ref(false)
  const autoReadingTimer = ref<number | null>(null)

  function toggleAutoReading() {
    isAutoScrolling.value = !isAutoScrolling.value
    autoReading.value = isAutoScrolling.value
  }

  function stopAutoReading() {
    isAutoScrolling.value = false
    autoReading.value = false
    if (autoReadingTimer.value) {
      clearInterval(autoReadingTimer.value)
      autoReadingTimer.value = null
    }
  }

  /* ─── TTS (Text To Speech) ─── */
  const isSpeaking = ref(false)
  const isSpeechLoading = ref(false)
  const isPaused = ref(false)
  const speechProgress = ref(0)
  const systemTtsNativeEventsReliable = ref(false)
  const voiceList = ref<SpeechSynthesisVoice[]>([])
  const speechConfig = reactive<SpeechConfig>(loadSpeechConfig())
  const openAISpeechConfigured = computed(() => !!speechConfig.openaiBaseUrl.trim())
  const speechProviderLabel = computed(() => speechConfig.provider === 'openai' ? 'API 语音' : '系统语音')
  const speechStopAt = ref(0)
  let speechStopTimer: number | null = null
  let synth: SpeechSynthesis | null = typeof window !== 'undefined' ? window.speechSynthesis : null
  let currentUtterance: SpeechSynthesisUtterance | null = null
  let currentOpenAIAudio: HTMLAudioElement | null = null
  let currentOpenAIAudioUrl = ''
  let currentOpenAIAbortController: AbortController | null = null
  const preloadedOpenAIAudio = ref<PreloadedOpenAIAudio[]>([])
  let preloadGeneration = 0
  const inFlightPreloadKeys = new Set<string>()
  const inFlightOpenAIAudioRequests = new Map<string, Promise<Blob>>()
  let currentTTSSessionId = 0

  function logTTS(message: string, payload?: unknown) {
    void message
    void payload
  }

  function captureTTSCaller() {
    try {
      const stack = new Error().stack || ''
      return stack
        .split('\n')
        .slice(2, 5)
        .map((line) => line.trim())
        .join(' | ')
    } catch {
      return ''
    }
  }

  function beginTTSSession() {
    currentTTSSessionId += 1
    logTTS('begin session', { sessionId: currentTTSSessionId })
    return currentTTSSessionId
  }

  function isCurrentTTSSession(sessionId: number) {
    return sessionId === currentTTSSessionId
  }

  function saveSpeechConfig() {
    localStorage.setItem('reader-speechConfig', JSON.stringify(speechConfig))
  }

  function normalizeLegadoProgressTime(value?: number | null) {
    if (typeof value !== 'number' || Number.isNaN(value) || value <= 0) return 0
    return value < 1_000_000_000_000 ? value * 1000 : value
  }

  async function syncCurrentBookProgressToLegado(
    allowUpload = true,
    contentOverride?: string,
    forceUpload = false,
  ) {
    if (!book.value) return null
    if (!appStore.legadoSyncEnabled) return null
    const webdav = appStore.legadoWebdavConfig
    if (!webdav.url || !webdav.account || !webdav.password) return null
    const plainContentLength = Math.max(1, (contentOverride ?? content.value).replace(/<[^>]+>/g, '').length)
    const progressPosition = Math.max(0, Math.min(plainContentLength,
      Math.round(chapterScrollProgress.value * plainContentLength)))
    const progress: LegadoBookProgress = {
      name: book.value.name,
      author: book.value.author,
      durChapterIndex: currentIndex.value,
      durChapterPos: progressPosition,
      durChapterTime: forceUpload ? Date.now() : (book.value.durChapterTime || 0),
      durChapterTitle: currentChapter.value?.title || book.value.durChapterTitle,
    }
    const result = await syncLegadoBookProgress(webdav, progress, allowUpload, forceUpload)
    if (!result.remote) return result

    const remote = result.remote
    const remoteTime = normalizeLegadoProgressTime(remote.durChapterTime)
    const localTime = normalizeLegadoProgressTime(progress.durChapterTime)
    const remotePositionIsAhead = remote.durChapterIndex > currentIndex.value
      || (remote.durChapterIndex === currentIndex.value && remote.durChapterPos > progress.durChapterPos)
    const remoteIsNewer = remoteTime > localTime
      || (remoteTime === localTime && remotePositionIsAhead)
    if (!remoteIsNewer) return result

    const targetIndex = Math.max(0, Math.min(chapters.value.length - 1, remote.durChapterIndex))
    const title = chapters.value[targetIndex]?.title || remote.durChapterTitle || book.value.durChapterTitle
    const remoteProgress = targetIndex === currentIndex.value
      ? Math.max(0, Math.min(1, remote.durChapterPos / plainContentLength))
      : 0
    const encodedRemoteProgress = encodeServerProgress(remoteProgress)
    pendingLegadoProgress.value = { index: targetIndex, position: remote.durChapterPos }
    currentIndex.value = targetIndex
    Object.assign(book.value, {
      durChapterIndex: targetIndex,
      durChapterPos: encodedRemoteProgress,
      durChapterTitle: title,
      durChapterTime: remote.durChapterTime,
    })
    chapterScrollProgress.value = remoteProgress
    const shelfBook = shelfStore.books.find((item) => item.bookUrl === book.value?.bookUrl)
    if (shelfBook) Object.assign(shelfBook, {
      durChapterIndex: targetIndex,
      durChapterPos: encodedRemoteProgress,
      durChapterTitle: title,
      durChapterTime: remote.durChapterTime,
    })
    // 云端进度是更权威的入口位置: 同步更新快照, 保证随后 loadSavedReadingPosition
    // 把远端位置作为"服务器来源"参与比较(否则快照停留书架旧值, 云端进度被丢弃)。
    // 跨章时 remoteProgress 编码为 0, 快照须用云端原始字数位置(pendingLegadoProgress)。
    snapshotOpenPosition({
      ...book.value,
      durChapterPos: pendingLegadoProgress.value?.position ?? encodedRemoteProgress,
    })
    saveReaderSession()
    appStore.showToast('已读取手机端阅读进度', 'success')
    return result
  }

  function uploadCurrentBookProgressToLegado() {
    return syncCurrentBookProgressToLegado(true, undefined, true)
  }

  async function restoreCurrentBookProgressFromLegado(contentOverride?: string) {
    const result = await syncCurrentBookProgressToLegado(false, contentOverride)
    if (pendingLegadoProgress.value) {
      await loadChapter(currentIndex.value)
    }
    return result
  }

  const systemSpeechSupported = computed(() => (
    typeof window !== 'undefined'
      && typeof window.speechSynthesis !== 'undefined'
      && typeof SpeechSynthesisUtterance !== 'undefined'
  ))

  function resolveSpeechSynthesis() {
    if (!synth && typeof window !== 'undefined') {
      synth = window.speechSynthesis
    }
    return synth
  }

  function fetchVoices() {
    const speech = resolveSpeechSynthesis()
    if (!speech) return
    voiceList.value = speech.getVoices().slice().sort((a, b) => {
      const aZh = a.lang.startsWith('zh-')
      const bZh = b.lang.startsWith('zh-')
      if (aZh && !bZh) return -1
      if (!aZh && bZh) return 1
      return a.lang.localeCompare(b.lang)
    })
    if (!speechConfig.voiceName && voiceList.value.length > 0) {
      const zhVoice = voiceList.value.find((v) => v.lang.startsWith('zh-'))
      speechConfig.voiceName = (zhVoice || voiceList.value[0]).name
      saveSpeechConfig()
    }
  }

  function setVoiceName(name: string) {
    speechConfig.voiceName = name
    saveSpeechConfig()
  }

  function setSpeechProvider(provider: SpeechProvider) {
    speechConfig.provider = provider
    clearPreloadedOpenAIAudio()
    saveSpeechConfig()
  }

  function setOpenAISpeechBaseUrl(url: string) {
    speechConfig.openaiBaseUrl = url.trim()
    clearPreloadedOpenAIAudio()
    saveSpeechConfig()
  }

  function setSpeechProxyUrl(url: string) {
    speechConfig.speechProxyUrl = url.trim()
    clearPreloadedOpenAIAudio()
    saveSpeechConfig()
  }

  function setSpeechApiFormat(format: SpeechApiFormat) {
    speechConfig.apiFormat = format
    const supportedFormats = getSpeechApiFormatOption(format).supportedFormats
    if (!supportedFormats.includes(speechConfig.openaiFormat)) {
      speechConfig.openaiFormat = 'mp3'
    }
    clearPreloadedOpenAIAudio()
    saveSpeechConfig()
  }

  function setOpenAISpeechSource(source: OpenAISpeechSource) {
    speechConfig.openaiSource = source
    clearPreloadedOpenAIAudio()
    saveSpeechConfig()
  }

  function setOpenAISpeechApiKey(apiKey: string) {
    speechConfig.openaiApiKey = apiKey.trim()
    clearPreloadedOpenAIAudio()
    saveSpeechConfig()
  }

  function setOpenAISpeechModel(model: string) {
    speechConfig.openaiModel = model
    clearPreloadedOpenAIAudio()
    saveSpeechConfig()
  }

  function setOpenAISpeechVoice(voice: string) {
    speechConfig.openaiVoice = voice
    clearPreloadedOpenAIAudio()
    saveSpeechConfig()
  }

  function setOpenAISpeechFormat(format: OpenAISpeechFormat) {
    speechConfig.openaiFormat = format
    clearPreloadedOpenAIAudio()
    saveSpeechConfig()
  }

  function setOpenAISpeechRequestMode(mode: OpenAISpeechRequestMode) {
    speechConfig.openaiRequestMode = mode
    clearPreloadedOpenAIAudio()
    saveSpeechConfig()
  }

  function setSpeechRate(rate: number) {
    speechConfig.speechRate = rate
    clearPreloadedOpenAIAudio()
    saveSpeechConfig()
  }

  function setSpeechPitch(pitch: number) {
    speechConfig.speechPitch = pitch
    saveSpeechConfig()
  }

  function buildOpenAIAudioCacheKey(rawText: string) {
    return [
      speechConfig.apiFormat,
      speechConfig.openaiBaseUrl.trim(),
      speechConfig.speechProxyUrl.trim(),
      speechConfig.openaiApiKey.trim(),
      speechConfig.openaiModel,
      speechConfig.openaiVoice,
      speechConfig.openaiFormat,
      speechConfig.speechRate.toFixed(1),
      rawText,
    ].join('::')
  }

  async function fetchOpenAIAudioBlob(rawText: string, signal?: AbortSignal) {
    const request = {
      apiFormat: speechConfig.apiFormat,
      baseUrl: speechConfig.openaiBaseUrl,
      proxyUrl: speechConfig.speechProxyUrl || undefined,
      apiKey: speechConfig.openaiApiKey || undefined,
      input: rawText.slice(0, 4096),
      model: speechConfig.openaiModel,
      voice: speechConfig.openaiVoice,
      format: speechConfig.openaiFormat,
      speed: speechConfig.speechRate,
      signal,
    }
    if (!isTauri()) {
      return requestOpenAISpeechAudio(request)
    }

    const payload = await invokeRaw<ArrayBuffer | Uint8Array | number[]>('request_speech_audio', {
      req: {
        apiFormat: request.apiFormat,
        baseUrl: request.baseUrl,
        proxyUrl: request.proxyUrl,
        apiKey: request.apiKey,
        input: request.input,
        model: request.model,
        voice: request.voice,
        format: request.format,
        speed: request.speed,
      },
    })
    if (signal?.aborted) {
      throw new DOMException('The operation was aborted', 'AbortError')
    }
    const bytes = payload instanceof ArrayBuffer
      ? payload
      : payload instanceof Uint8Array
        ? payload.buffer.slice(payload.byteOffset, payload.byteOffset + payload.byteLength) as ArrayBuffer
        : new Uint8Array(payload).buffer
    const mimeType = request.format === 'opus' ? 'audio/ogg' : `audio/${request.format}`
    return new Blob([bytes], { type: mimeType })
  }

  function getOrStartOpenAIAudioRequest(rawText: string, signal?: AbortSignal) {
    const key = buildOpenAIAudioCacheKey(rawText)
    const existing = inFlightOpenAIAudioRequests.get(key)
    if (existing) {
      return { key, promise: existing }
    }

    const promise = fetchOpenAIAudioBlob(rawText, signal).finally(() => {
      if (inFlightOpenAIAudioRequests.get(key) === promise) {
        inFlightOpenAIAudioRequests.delete(key)
      }
    })
    inFlightOpenAIAudioRequests.set(key, promise)
    return { key, promise }
  }

  function clearPreloadedOpenAIAudio() {
    preloadGeneration += 1
    inFlightPreloadKeys.clear()
    inFlightOpenAIAudioRequests.clear()
    preloadedOpenAIAudio.value = []
  }

  async function preloadOpenAITTS(rawText?: string | string[] | null) {
    if (speechConfig.provider !== 'openai' || !openAISpeechConfigured.value) return
    const texts = Array.isArray(rawText) ? rawText : [rawText || '']
    const normalizedTexts = texts.map((item) => item.trim()).filter(Boolean)
    if (!normalizedTexts.length) return
    const pendingTexts = normalizedTexts.filter((item) => {
      const key = buildOpenAIAudioCacheKey(item)
      return !preloadedOpenAIAudio.value.some((entry) => entry.key === key) && !inFlightPreloadKeys.has(key)
    })
    if (!pendingTexts.length) return

    const generation = preloadGeneration
    for (const text of pendingTexts.slice(0, OPENAI_AUDIO_PRELOAD_LIMIT)) {
      const key = buildOpenAIAudioCacheKey(text)
      inFlightPreloadKeys.add(key)
      const { promise } = getOrStartOpenAIAudioRequest(text)
      void promise
        .then((blob) => {
          if (generation !== preloadGeneration) return
          const nextQueue = preloadedOpenAIAudio.value.filter((entry) => entry.key !== key)
          nextQueue.push({ key, blob })
          preloadedOpenAIAudio.value = nextQueue
        })
        .catch(() => undefined)
        .finally(() => {
          inFlightPreloadKeys.delete(key)
        })
    }
  }

  function stopOpenAIAudioPlayback() {
    if (currentOpenAIAbortController) {
      currentOpenAIAbortController.abort()
      currentOpenAIAbortController = null
    }
    if (currentOpenAIAudio) {
      currentOpenAIAudio.onplay = null
      currentOpenAIAudio.onpause = null
      currentOpenAIAudio.onloadedmetadata = null
      currentOpenAIAudio.ontimeupdate = null
      currentOpenAIAudio.onended = null
      currentOpenAIAudio.onerror = null
      currentOpenAIAudio.pause()
      currentOpenAIAudio.src = ''
      currentOpenAIAudio = null
    }
    if (currentOpenAIAudioUrl) {
      URL.revokeObjectURL(currentOpenAIAudioUrl)
      currentOpenAIAudioUrl = ''
    }
  }

  function clearSpeechStopTimer(resetConfig = true) {
    if (speechStopTimer) {
      clearTimeout(speechStopTimer)
      speechStopTimer = null
    }
    speechStopAt.value = 0
    if (resetConfig) {
      speechConfig.stopAfterMinutes = 0
      saveSpeechConfig()
    }
  }

  function setSpeechStopTimer(minutes: number) {
    clearSpeechStopTimer(false)
    const normalized = Math.max(0, Math.min(180, Math.round(minutes)))
    speechConfig.stopAfterMinutes = normalized
    saveSpeechConfig()
    if (!normalized) {
      speechStopAt.value = 0
      return
    }
    speechStopAt.value = Date.now() + normalized * 60 * 1000
    speechStopTimer = window.setTimeout(() => {
      stopTTS()
      clearSpeechStopTimer(false)
      speechConfig.stopAfterMinutes = 0
      saveSpeechConfig()
      appStore.showToast('朗读已按定时设置停止', 'success')
    }, normalized * 60 * 1000)
  }

  function startSystemTTS(rawText: string, options: TTSOptions, sessionId: number) {
    synth = resolveSpeechSynthesis()
    if (!synth || typeof SpeechSynthesisUtterance === 'undefined') {
      const error = new Error('当前系统不支持系统语音，请改用 API 语音')
      isSpeechLoading.value = false
      isSpeaking.value = false
      isPaused.value = false
      appStore.showToast(error.message, 'warning')
      options.onError?.(error)
      return
    }
    isSpeechLoading.value = false
    if (!voiceList.value.length) {
      fetchVoices()
    }

    const utterance = new SpeechSynthesisUtterance(rawText)
    speechProgress.value = 0
    options.onProgress?.(0)
    currentUtterance = utterance
    const safariSpeechFallback = isSafariSpeechFallbackMode() && !systemTtsNativeEventsReliable.value

    const selectedVoice = voiceList.value.find((voice) => voice.name === speechConfig.voiceName)
    utterance.lang = selectedVoice?.lang || 'zh-CN'
    utterance.voice = selectedVoice || null
    utterance.rate = speechConfig.speechRate
    utterance.pitch = speechConfig.speechPitch
    logTTS('system speak queued', {
      sessionId,
      voice: utterance.voice?.name || utterance.lang,
      rate: utterance.rate,
      pitch: utterance.pitch,
      text: rawText.slice(0, 80),
    })

    let completed = false
    let finishWatchdog: number | null = null
    const startedAt = Date.now()
    let lastProgressAt = startedAt
    let sawStart = false
    let sawBoundary = false
    let pausedStartedAt: number | null = null
    let pausedAccumulatedMs = 0

    const clearFinishWatchdog = () => {
      if (finishWatchdog) {
        clearTimeout(finishWatchdog)
        finishWatchdog = null
      }
    }

    const effectiveElapsed = () => {
      const now = Date.now()
      const currentPaused = pausedStartedAt ? now - pausedStartedAt : 0
      return now - startedAt - pausedAccumulatedMs - currentPaused
    }

    const finalizePlayback = (kind: 'end' | 'error' | 'interrupted', event?: SpeechSynthesisErrorEvent) => {
      if (completed) return
      completed = true
      clearFinishWatchdog()
      if (currentUtterance === utterance) {
        currentUtterance = null
      }
      if (!isCurrentTTSSession(sessionId)) return
      isSpeaking.value = false
      isPaused.value = false
      logTTS('system finalize', {
        sessionId,
        kind,
        error: event?.error,
        speaking: synth?.speaking,
        pending: synth?.pending,
      })
      if (kind === 'end') {
        options.onEnd?.()
        return
      }
      if (kind === 'error') {
        options.onError?.(event)
      }
    }

    const forceFinalizeEnd = (reason: string) => {
      logTTS('system watchdog force end', {
        sessionId,
        reason,
        speaking: synth?.speaking,
        pending: synth?.pending,
        elapsed: effectiveElapsed(),
        text: rawText.slice(0, 40),
      })
      finalizePlayback('end')
      window.setTimeout(() => {
        if (!isCurrentTTSSession(sessionId)) return
        try {
          synth?.cancel()
        } catch {
          // ignore platform-specific cancel errors
        }
      }, 0)
    }

    const scheduleFinishWatchdog = () => {
      clearFinishWatchdog()
      const estimatedMs = safariSpeechFallback
        ? Math.max(2400, Math.ceil((rawText.length / Math.max(0.6, speechConfig.speechRate)) * 235))
        : Math.max(2800, Math.ceil((rawText.length / Math.max(0.6, speechConfig.speechRate)) * 280))
      const noStartTimeoutMs = safariSpeechFallback
        ? estimatedMs + Math.max(400, Math.ceil(rawText.length * 22))
        : 0
      const hardTimeoutMs = safariSpeechFallback
        ? estimatedMs + Math.max(1800, Math.ceil(rawText.length * 80))
        : Math.min(120000, estimatedMs + Math.max(4000, Math.ceil(rawText.length * 120)))
      logTTS('system watchdog scheduled', {
        sessionId,
        estimatedMs,
        noStartTimeoutMs,
        hardTimeoutMs,
        safariSpeechFallback,
        text: rawText.slice(0, 40),
      })
      const checkFinish = () => {
        if (completed || !isCurrentTTSSession(sessionId) || currentUtterance !== utterance) return
        if (synth?.paused || isPaused.value) {
          if (pausedStartedAt == null) {
            pausedStartedAt = Date.now()
          }
          lastProgressAt = Date.now()
          finishWatchdog = window.setTimeout(checkFinish, 600)
          return
        }
        if (pausedStartedAt != null) {
          pausedAccumulatedMs += Date.now() - pausedStartedAt
          pausedStartedAt = null
        }
        const elapsed = effectiveElapsed()
        const idleMs = Date.now() - lastProgressAt
        if (!synth?.speaking && !synth?.pending) {
          logTTS('system watchdog finalize end', { sessionId })
          finalizePlayback('end')
          return
        }
        if (sawBoundary && idleMs > 1800 && elapsed > Math.max(2200, estimatedMs * 0.75)) {
          forceFinalizeEnd('boundary-idle')
          return
        }
        if (safariSpeechFallback && !sawStart && elapsed > noStartTimeoutMs) {
          forceFinalizeEnd('no-start-timeout')
          return
        }
        if (elapsed > hardTimeoutMs) {
          forceFinalizeEnd('hard-timeout')
          return
        }
        finishWatchdog = window.setTimeout(checkFinish, 600)
      }
      finishWatchdog = window.setTimeout(checkFinish, safariSpeechFallback ? Math.min(estimatedMs, 1200) : estimatedMs)
    }

    utterance.onstart = () => {
      if (!isCurrentTTSSession(sessionId) || currentUtterance !== utterance) return
      isSpeaking.value = true
      isPaused.value = false
      sawStart = true
      systemTtsNativeEventsReliable.value = true
      lastProgressAt = Date.now()
      logTTS('system onstart', { sessionId, text: rawText.slice(0, 40) })
      options.onStart?.()
    }
    utterance.onboundary = (event) => {
      if (!isCurrentTTSSession(sessionId) || currentUtterance !== utterance) return
      sawBoundary = true
      lastProgressAt = Date.now()
      const charIndex = Number.isFinite(event.charIndex) ? event.charIndex : 0
      const progress = Math.max(0, Math.min(1, charIndex / Math.max(1, rawText.length)))
      speechProgress.value = progress
      options.onProgress?.(progress)
    }
    utterance.onend = () => {
      logTTS('system onend', { sessionId, text: rawText.slice(0, 40) })
      speechProgress.value = 1
      options.onProgress?.(1)
      finalizePlayback('end')
    }
    utterance.onerror = (event) => {
      const interrupted = event.error === 'interrupted' || event.error === 'canceled'
      logTTS('system onerror', { sessionId, error: event.error, interrupted, text: rawText.slice(0, 40) })
      finalizePlayback(interrupted ? 'interrupted' : 'error', event)
    }

    synth.speak(utterance)
    logTTS('system speak invoked', {
      sessionId,
      speaking: synth.speaking,
      pending: synth.pending,
      text: rawText.slice(0, 40),
    })
    scheduleFinishWatchdog()
  }

  async function startOpenAITTS(rawText: string, options: TTSOptions, sessionId: number) {
    if (!openAISpeechConfigured.value) {
      const error = new Error('请先配置 API 语音服务')
      appStore.showToast(error.message, 'warning')
      options.onError?.(error)
      return
    }

    isSpeechLoading.value = true
    speechProgress.value = 0
    options.onProgress?.(0)
    logTTS('openai speak queued', {
      sessionId,
      model: speechConfig.openaiModel,
      voice: speechConfig.openaiVoice,
      text: rawText.slice(0, 80),
    })
    const playBlob = (blob: Blob, controller: AbortController) => {
      if (controller.signal.aborted) return
      if (!isCurrentTTSSession(sessionId)) return
      isSpeechLoading.value = false
      currentOpenAIAudioUrl = URL.createObjectURL(blob)
      const audio = new Audio(currentOpenAIAudioUrl)
      currentOpenAIAudio = audio
      currentOpenAIAbortController = null

      const updateProgress = () => {
        if (!isCurrentTTSSession(sessionId) || currentOpenAIAudio !== audio) return
        if (!Number.isFinite(audio.duration) || audio.duration <= 0) return
        const progress = Math.max(0, Math.min(1, audio.currentTime / audio.duration))
        speechProgress.value = progress
        options.onProgress?.(progress)
      }

      audio.onplay = () => {
        if (!isCurrentTTSSession(sessionId) || currentOpenAIAudio !== audio) return
        isSpeaking.value = true
        isPaused.value = false
        logTTS('openai onplay', { sessionId, text: rawText.slice(0, 40) })
        options.onStart?.()
      }

      audio.onpause = () => {
        if (!isCurrentTTSSession(sessionId) || currentOpenAIAudio !== audio) return
        if (!audio.ended) {
          isPaused.value = true
          isSpeaking.value = true
        }
      }

      audio.onloadedmetadata = updateProgress
      audio.ontimeupdate = updateProgress

      audio.onended = () => {
        if (currentOpenAIAudio === audio) {
          currentOpenAIAudio = null
        }
        if (!isCurrentTTSSession(sessionId)) return
        speechProgress.value = 1
        options.onProgress?.(1)
        isSpeaking.value = false
        isPaused.value = false
        logTTS('openai onended', { sessionId, text: rawText.slice(0, 40) })
        if (currentOpenAIAudioUrl) {
          URL.revokeObjectURL(currentOpenAIAudioUrl)
          currentOpenAIAudioUrl = ''
        }
        options.onEnd?.()
      }

      audio.onerror = () => {
        if (currentOpenAIAudio === audio) {
          currentOpenAIAudio = null
        }
        if (!isCurrentTTSSession(sessionId)) return
        isSpeaking.value = false
        isPaused.value = false
        const error = new Error('API 语音音频播放失败')
        logTTS('openai onerror', { sessionId, text: rawText.slice(0, 40) })
        options.onError?.(error)
      }

      return audio.play().catch((error: Error) => {
        if (!isCurrentTTSSession(sessionId)) return
        isSpeechLoading.value = false
        isSpeaking.value = false
        isPaused.value = false
        currentOpenAIAudio = null
        logTTS('openai play catch', { sessionId, message: error.message, text: rawText.slice(0, 40) })
        options.onError?.(error)
      })
    }

    const controller = new AbortController()
    currentOpenAIAbortController = controller

    const key = buildOpenAIAudioCacheKey(rawText)
    const cached = preloadedOpenAIAudio.value.find((entry) => entry.key === key)
    if (cached) {
      void Promise.resolve(playBlob(cached.blob, controller))
      return
    }

    const inFlight = inFlightOpenAIAudioRequests.get(key)
    if (inFlight) {
      void inFlight.then((blob) => {
        return playBlob(blob, controller)
      }).catch((error: Error) => {
        if (controller.signal.aborted || !isCurrentTTSSession(sessionId)) return
        isSpeechLoading.value = false
        isSpeaking.value = false
        isPaused.value = false
        currentOpenAIAbortController = null
        currentOpenAIAudio = null
        logTTS('openai inflight catch', { sessionId, message: error.message, text: rawText.slice(0, 40) })
        options.onError?.(error)
      })
      return
    }

    const started = getOrStartOpenAIAudioRequest(rawText, controller.signal)
    void started.promise.then((blob) => {
      return playBlob(blob, controller)
    }).catch((error: Error) => {
      if (controller.signal.aborted || !isCurrentTTSSession(sessionId)) return
      isSpeechLoading.value = false
      isSpeaking.value = false
      isPaused.value = false
      currentOpenAIAbortController = null
      currentOpenAIAudio = null
      logTTS('openai request catch', { sessionId, message: error.message, text: rawText.slice(0, 40) })
      appStore.showToast(error.message || 'API 语音请求失败', 'error')
      options.onError?.(error)
    })
  }

  function startTTS(text?: string, options: TTSOptions = {}, interruptCurrent = true) {
    const hasActiveSystemSpeech = !!synth && (synth.speaking || synth.pending || !!currentUtterance)
    const hasActiveOpenAISpeech = !!currentOpenAIAudio || !!currentOpenAIAbortController

    if (interruptCurrent && (hasActiveSystemSpeech || hasActiveOpenAISpeech || isSpeaking.value || isSpeechLoading.value)) {
      stopTTS(false)
    }

    const rawText = (text || content.value.replace(/<[^>]+>/g, '')).trim()
    if (!rawText) return

    const sessionId = beginTTSSession()
    logTTS('startTTS', {
      sessionId,
      provider: speechConfig.provider,
      interruptCurrent,
      text: rawText.slice(0, 80),
    })

    if (
      !interruptCurrent &&
      speechConfig.provider === 'system' &&
      synth &&
      !synth.speaking &&
      isSafariSpeechFallbackMode() &&
      !systemTtsNativeEventsReliable.value
    ) {
      try {
        logTTS('startTTS cleanup idle system synth', { sessionId })
        synth.cancel()
      } catch {
        // ignore platform-specific cancel errors
      }
    }

    if (speechConfig.provider === 'openai') {
      void startOpenAITTS(rawText, options, sessionId)
      return
    }

    startSystemTTS(rawText, options, sessionId)
  }

  function pauseTTS() {
    if (speechConfig.provider === 'openai') {
      if (!currentOpenAIAudio) return
      if (currentOpenAIAudio.paused) {
        const audio = currentOpenAIAudio
        void audio.play().catch((error: Error) => {
          if (currentOpenAIAudio !== audio) return
          isPaused.value = true
          isSpeaking.value = true
          appStore.showToast(error.message || '恢复播放失败', 'error')
        })
      } else {
        currentOpenAIAudio.pause()
        isPaused.value = true
        isSpeaking.value = true
      }
      return
    }

    if (!synth) return
    if (synth.speaking && !synth.paused) {
      synth.pause()
      isPaused.value = true
    } else if (synth.paused) {
      synth.resume()
      isPaused.value = false
    }
  }

  function stopTTS(resetCallbacks = true) {
    const sessionId = beginTTSSession()
    logTTS('stopTTS', {
      sessionId,
      resetCallbacks,
      provider: speechConfig.provider,
      caller: captureTTSCaller(),
    })
    if (synth) {
      synth.cancel()
      currentUtterance = null
    }
    stopOpenAIAudioPlayback()
    isSpeechLoading.value = false
    isSpeaking.value = false
    isPaused.value = false
    speechProgress.value = 0
    if (resetCallbacks) {
      clearSpeechStopTimer()
    }
  }

  /* ─── Book / chapter ops ─── */
  async function loadBook(b: Book) {
    loading.value = true
    preloadedContent.value.clear()
    preloadingContent.clear()
    chapterPreloadGeneration += 1
    book.value = b
    snapshotOpenPosition(b)
    appStore.setReadingSessionBook(b.bookUrl, b.name)
    chapters.value = []
    content.value = ''
    appStore.markBookOpened(b.bookUrl)
    currentIndex.value = b.durChapterIndex || 0
    chapterScrollProgress.value = decodeServerProgress(b.durChapterPos)
    loadReadChapterHistory(b)
    progressDirty.value = false
    lastServerProgressKey.value = ''
    chaptersLoading.value = true
    try {
      // 目录必须先拿到才能决定章节, 但正文请求不与目录串行等待:
      // 目录到达后立即发正文请求, 二者在网络上并行; 网盘进度读取也不阻塞渲染。
      const chapterListPromise = getChapterList({
        bookUrl: b.bookUrl,
        bookSourceUrl: b.origin,
        tocUrl: b.tocUrl,
      })
      chapters.value = await chapterListPromise

      const contentLoad = fetchChapterContent(currentIndex.value).catch(() => null)
      const initialChapterContent = await contentLoad
      if (initialChapterContent) {
        cachePreloadedContent(currentIndex.value, initialChapterContent)
        if (config.enablePreload) void preloadAroundChapter(currentIndex.value)
      }
      // 网盘进度同步放后台, 不拖慢正文首屏
      void restoreCurrentBookProgressFromLegado(initialChapterContent || undefined)
        .catch((error) => {
          appStore.showToast((error as Error).message || '读取网盘阅读进度失败', 'warning')
        })
      saveReaderSession()
      // 打开书后台静默检查目录更新(按书 30 分钟节流), 不阻塞首屏渲染
      void refreshTocFromSource()
    } catch (error) {
      loading.value = false
      throw error
    } finally {
      chaptersLoading.value = false
    }
  }

  function setActiveChapterState(index: number, chapterContent: string, progress = 0) {
    currentIndex.value = index
    content.value = chapterContent
    chapterScrollProgress.value = Math.max(0, Math.min(1, progress))
    if (book.value) {
      book.value.durChapterIndex = index
      book.value.durChapterTitle = chapters.value[index]?.title || book.value.durChapterTitle
      book.value.durChapterTime = Date.now()
      const shelfBook = shelfStore.books.find((item) => item.bookUrl === book.value?.bookUrl)
      if (shelfBook) {
        shelfBook.durChapterIndex = book.value.durChapterIndex
        shelfBook.durChapterTitle = book.value.durChapterTitle
        shelfBook.durChapterTime = book.value.durChapterTime
      }
    }
    syncLocalBookProgress(chapterScrollProgress.value)
    if (book.value) {
      saveRecentReadBook(book.value)
    }
    localStorage.setItem('reader-currentIndex', String(index))
    saveReaderSession()
    markProgressDirty()
  }

  async function persistProgress(index = currentIndex.value, progress = chapterScrollProgress.value) {
    const payload = currentServerProgressPayload(index, progress)
    if (!payload) return
    await saveBookProgress(payload).then(() => {
      progressDirty.value = false
      lastServerProgressKey.value = `${payload.bookUrl}::${payload.index}::${payload.position}`
    }).catch(() => undefined)
  }

  async function flushProgressToServer(force = false) {
    const payload = currentServerProgressPayload()
    if (!payload) return
    const nextKey = `${payload.bookUrl}::${payload.index}::${payload.position}`
    if (!force && !progressDirty.value && lastServerProgressKey.value === nextKey) return
    await persistProgress(payload.index, chapterScrollProgress.value)
  }

  function flushProgressToServerKeepalive(force = false) {
    const payload = currentServerProgressPayload()
    if (!payload) return
    const nextKey = `${payload.bookUrl}::${payload.index}::${payload.position}`
    if (!force && !progressDirty.value && lastServerProgressKey.value === nextKey) return

    void invokeRaw('save_book_progress', { req: payload }).catch(() => undefined)
    progressDirty.value = false
    lastServerProgressKey.value = nextKey
  }

  function cachePreloadedContent(index: number, chapterContent: string) {
    preloadedContent.value.set(index, chapterContent)
    while (preloadedContent.value.size > 3) {
      const oldestKey = Array.from(preloadedContent.value.keys()).find((key) => key !== index)
      if (oldestKey === undefined) break
      preloadedContent.value.delete(oldestKey)
    }
  }

  async function fetchChapterContent(index: number, forceRefresh = false) {
    if (!book.value || !chapters.value[index]) return null

    if (!forceRefresh && preloadedContent.value.has(index)) {
      return preloadedContent.value.get(index) || null
    }

    if (!forceRefresh) {
      const pending = preloadingContent.get(index)
      if (pending) return pending
    }

    const chapter = chapters.value[index]

    try {
      return await getBookContent({
        bookUrl: book.value.bookUrl,
        chapterUrl: chapter.url,
        bookSourceUrl: book.value.origin,
        refresh: forceRefresh ? 1 : 0,
      })
    } catch (error) {
      if (!appStore.isOnline) {
        throw new Error('当前处于离线状态，未缓存章节无法打开')
      }
      throw error
    }
  }

  async function loadChapter(index: number, forceRefresh = false) {
    if (!book.value || !chapters.value[index]) return

    const cachedContent = !forceRefresh ? preloadedContent.value.get(index) : undefined
    loading.value = cachedContent === undefined
    try {
      const chapterContent = cachedContent !== undefined
        ? cachedContent
        : await fetchChapterContent(index, forceRefresh)
      if (chapterContent == null) return

      const previousSavedIndex = book.value.durChapterIndex ?? 0
      const previousSavedProgress = decodeServerProgress(book.value.durChapterPos)
      const isOpeningSavedChapter = !forceRefresh && index === previousSavedIndex
      const cloudProgress = pendingLegadoProgress.value?.index === index
        ? Math.max(0, Math.min(1, pendingLegadoProgress.value.position
          / Math.max(1, chapterContent.replace(/<[^>]+>/g, '').length)))
        : null
      const initialProgress = cloudProgress ?? (isOpeningSavedChapter ? previousSavedProgress : 0)
      if (cloudProgress != null) pendingLegadoProgress.value = null

      cachePreloadedContent(index, chapterContent)
      setActiveChapterState(index, chapterContent, initialProgress)
      markChapterAsRead(index)
      appStore.markChapterRead(book.value.bookUrl, index, chapters.value.length)
      loading.value = false

      if (!isOpeningSavedChapter) {
        void persistProgress(index, cloudProgress ?? 0)
      }

      if (config.enablePreload) {
        void preloadAroundChapter(index)
      }
    } finally {
      loading.value = false
    }
  }

  async function preloadAroundChapter(index: number) {
    if (!book.value || !config.enablePreload) return
    const targets = [index + 1, index + 2, index - 1]
      .filter((target, pos, list) => target >= 0 && target < chapters.value.length && list.indexOf(target) === pos)
    // 并发预载, 不等彼此
    await Promise.allSettled(targets.map((target) => preloadNextChapter(target)))
  }

  async function preloadNextChapter(index: number) {
    if (!book.value || !config.enablePreload || index < 0 || index >= chapters.value.length || preloadedContent.value.has(index)) return

    const pending = preloadingContent.get(index)
    if (pending) return pending

    const generation = chapterPreloadGeneration
    const bookUrl = book.value.bookUrl
    let request: Promise<string | null>
    request = fetchChapterContent(index)
      .then((res) => {
        if (res && generation === chapterPreloadGeneration && config.enablePreload && book.value?.bookUrl === bookUrl) {
          cachePreloadedContent(index, res)
        }
        return res
      })
      .catch(() => null)
      .finally(() => {
        if (preloadingContent.get(index) === request) {
          preloadingContent.delete(index)
        }
      })
    preloadingContent.set(index, request)
    return request
  }

  function normalizeChapterTitle(title?: string) {
    return (title || '')
      .replace(/\s+/g, '')
      .replace(/[^\p{L}\p{N}]/gu, '')
      .toLowerCase()
  }

  function resolveChapterIndexByTitle(list: BookChapter[], targetTitle?: string, fallbackIndex = 0) {
    if (!list.length) return 0
    const normalizedTarget = normalizeChapterTitle(targetTitle)
    if (!normalizedTarget) {
      return Math.max(0, Math.min(list.length - 1, fallbackIndex))
    }

    const exactIndex = list.findIndex((chapter) => normalizeChapterTitle(chapter.title) === normalizedTarget)
    if (exactIndex >= 0) return exactIndex

    const partialIndex = list.findIndex((chapter) => {
      const title = normalizeChapterTitle(chapter.title)
      return title.includes(normalizedTarget) || normalizedTarget.includes(title)
    })
    if (partialIndex >= 0) return partialIndex

    return Math.max(0, Math.min(list.length - 1, fallbackIndex))
  }

  /* ─── Switch Source ─── */
  async function switchSource(newUrl: string, sourceUrl: string) {
    if (!book.value) return
    const previousChapterTitle = currentChapter.value?.title || book.value.durChapterTitle
    const previousIndex = currentIndex.value
    const previousProgress = chapterScrollProgress.value
    loading.value = true
    try {
      const updatedBook = await apiSetBookSource({
        bookUrl: book.value.bookUrl,
        newUrl,
        bookSourceUrl: sourceUrl,
        name: book.value.name,
        author: book.value.author,
        coverUrl: book.value.coverUrl,
        intro: book.value.intro,
        kind: book.value.kind,
        latestChapterTitle: book.value.latestChapterTitle,
        durChapterIndex: book.value.durChapterIndex,
        durChapterTitle: book.value.durChapterTitle,
        durChapterPos: book.value.durChapterPos,
        durChapterTime: book.value.durChapterTime,
      })
      if (!updatedBook) return null

      await loadBook(updatedBook)
      const targetIndex = resolveChapterIndexByTitle(
        chapters.value,
        previousChapterTitle,
        typeof updatedBook.durChapterIndex === 'number' ? updatedBook.durChapterIndex : previousIndex,
      )
      await loadChapter(targetIndex)
      setChapterScrollProgress(previousProgress)
      await shelfStore.fetchBooks().catch(() => undefined)
      return updatedBook
    } finally {
      loading.value = false
    }
  }

  async function refreshContent() {
    if (!book.value || !chapters.value[currentIndex.value]) return
    loading.value = true
    try {
      const chapterContent = await fetchChapterContent(currentIndex.value, true)
      if (chapterContent == null) return
      setActiveChapterState(currentIndex.value, chapterContent, chapterScrollProgress.value)
      void preloadAroundChapter(currentIndex.value)
    } finally {
      loading.value = false
    }
  }

  /* ─── 目录自动更新 ─── */
  let tocCheckTimes: Record<string, number> | null = null
  const tocRefreshInFlight = new Map<string, Promise<number>>()

  function loadTocCheckTimes(): Record<string, number> {
    if (tocCheckTimes) return tocCheckTimes
    let parsed: Record<string, number> = {}
    try {
      const raw = localStorage.getItem(TOC_CHECK_TIMES_KEY)
      const value = raw ? JSON.parse(raw) : {}
      if (value && typeof value === 'object') parsed = value
    } catch { /* 解析失败按空记录处理 */ }
    tocCheckTimes = parsed
    return parsed
  }

  function markTocChecked(bookUrl: string) {
    const times = loadTocCheckTimes()
    times[bookUrl] = Date.now()
    const entries = Object.entries(times)
    if (entries.length > TOC_CHECK_TIMES_LIMIT) {
      const kept = entries.sort((a, b) => b[1] - a[1]).slice(0, TOC_CHECK_TIMES_LIMIT)
      tocCheckTimes = Object.fromEntries(kept)
    }
    try {
      localStorage.setItem(TOC_CHECK_TIMES_KEY, JSON.stringify(tocCheckTimes))
    } catch { /* 存储写入失败时静默放弃节流记录 */ }
  }

  /** 强制重抓目录; 分页目录余下页由后端后台写入缓存, 轮询读取直到长度稳定。 */
  async function fetchTocListWithStabilizing(): Promise<BookChapter[]> {
    const b = book.value
    if (!b) return []
    const base = { bookUrl: b.bookUrl, bookSourceUrl: b.origin, tocUrl: b.tocUrl }
    let list = await getChapterList({ ...base, refresh: 1 })
    for (let attempt = 0; attempt < 3; attempt += 1) {
      if (!list.length) break
      await new Promise((resolve) => setTimeout(resolve, 1200))
      const cached = await getChapterList({ ...base })
      if (cached.length <= list.length) break
      list = cached
    }
    return list
  }

  async function performRefreshToc(minIntervalMs: number): Promise<number> {
    const b = book.value
    if (!b || isLocalBook(b) || !appStore.isOnline) return 0
    const bookUrl = b.bookUrl
    if (Date.now() - (loadTocCheckTimes()[bookUrl] || 0) < minIntervalMs) return 0
    markTocChecked(bookUrl)
    try {
      const fresh = await fetchTocListWithStabilizing()
      // 等待期间可能已切换书籍
      if (book.value?.bookUrl !== bookUrl) return 0
      const current = chapters.value
      if (!current.length || fresh.length <= current.length) return 0
      // 已有章节需完全对齐(末章 URL 一致)才静默替换, 避免源站目录重排错位阅读进度
      if (fresh[current.length - 1]?.url !== current[current.length - 1]?.url) return 0
      const added = fresh.length - current.length
      chapters.value = fresh
      saveReaderSession()
      appStore.showToast(`目录已更新，新增 ${added} 章`, 'success')
      return added
    } catch {
      return 0
    }
  }

  /**
   * 后台静默刷新书源目录, 返回新增章节数(0 = 无更新/被节流/失败)。
   * 打开书时按 TOC_OPEN_CHECK_INTERVAL_MS 节流; 读到末章再翻页时按更短间隔触发。
   */
  function refreshTocFromSource(minIntervalMs = TOC_OPEN_CHECK_INTERVAL_MS): Promise<number> {
    const bookUrl = book.value?.bookUrl
    if (!bookUrl) return Promise.resolve(0)
    const existing = tocRefreshInFlight.get(bookUrl)
    if (existing) return existing
    const task = performRefreshToc(minIntervalMs).finally(() => {
      if (tocRefreshInFlight.get(bookUrl) === task) tocRefreshInFlight.delete(bookUrl)
    })
    tocRefreshInFlight.set(bookUrl, task)
    return task
  }

  async function refreshChapters() {
    if (!book.value) return
    chaptersLoading.value = true
    try {
      preloadedContent.value.clear()
      preloadingContent.clear()
      chapterPreloadGeneration += 1
      markTocChecked(book.value.bookUrl)
      chapters.value = await fetchTocListWithStabilizing()
      const targetIndex = Math.max(0, Math.min(chapters.value.length - 1, currentIndex.value))
      if (chapters.value[targetIndex]) {
        await loadChapter(targetIndex, true)
      }
    } finally {
      chaptersLoading.value = false
    }
  }

  function setChapterScrollProgress(value: number) {
    chapterScrollProgress.value = Math.max(0, Math.min(1, value))
    syncLocalBookProgress(chapterScrollProgress.value)
    saveReaderSession()
    markProgressDirty()
  }

  async function nextChapter() {
    if (hasNext.value) {
      await loadChapter(currentIndex.value + 1)
    }
  }

  async function prevChapter() {
    if (hasPrev.value) {
      await loadChapter(currentIndex.value - 1)
    }
  }

  /* ─── Replace Rules ─── */
  async function fetchReplaceRules() {
    try {
      replaceRules.value = await getReplaceRules()
    } catch { /* ignore */ }
  }

  /* ─── Bookmarks ─── */
  async function fetchBookmarks() {
    try {
      const all = await getBookmarks()
      // Filter for current book
      if (book.value) {
        bookmarks.value = all.filter(b => b.bookName === book.value?.name && b.bookAuthor === book.value?.author)
      } else {
        bookmarks.value = all
      }
    } catch { /* ignore */ }
  }

  async function addBookmark(pos: number = 0, snippet: string = '') {
    if (!book.value || !currentChapter.value) return
    const b: Bookmark = {
      bookName: book.value.name,
      bookAuthor: book.value.author,
      chapterIndex: currentIndex.value,
      chapterName: currentChapter.value.title,
      chapterPos: pos,
      bookText: snippet || content.value.slice(0, 50).replace(/<[^>]+>/g, ''),
      time: Date.now(),
      content: '',
    }
    await saveBookmark(b)
    await fetchBookmarks()
  }

  async function removeBookmark(b: Bookmark) {
    await apiDeleteBookmark(b)
    await fetchBookmarks()
  }

  async function removeBookmarks(items: Bookmark[]) {
    if (!items.length) return
    await apiDeleteBookmarks(items)
    await fetchBookmarks()
  }

  function clear() {
    book.value = null
    openPosition.value = null
    chapters.value = []
    content.value = ''
    currentIndex.value = 0
    chapterScrollProgress.value = 0
    preloadedContent.value.clear()
    preloadingContent.clear()
    chapterPreloadGeneration += 1
    readChapterKeys.value = new Set()
    stopAutoReading()
  }

  /* ─── Panel visibility ─── */
  const activePanel = ref<ReaderPanel>(null)
  const panelParent = ref<ReaderPanel>(null)

  function openPanel(panel: ReaderPanel, parent: ReaderPanel = null) {
    activePanel.value = panel
    panelParent.value = parent
  }

  function togglePanel(panel: ReaderPanel, parent: ReaderPanel = null) {
    if (activePanel.value === panel) {
      closePanel()
      return
    }
    openPanel(panel, parent)
  }

  function backPanel() {
    if (panelParent.value) {
      activePanel.value = panelParent.value
      panelParent.value = null
      return
    }
    activePanel.value = null
  }

  function closePanel() {
    activePanel.value = null
    panelParent.value = null
  }

  return {
    book, chapters, currentIndex, content, loading, chaptersLoading, openPosition,
    currentChapter, hasNext, hasPrev, readingProgress,
      loadBook, loadChapter, fetchChapterContent, setActiveChapterState, refreshContent, nextChapter, prevChapter, clear,
      chapterScrollProgress, setChapterScrollProgress,
      getPersistedReaderSession, restorePersistedSession, syncCurrentBookProgressToLegado,
      restoreCurrentBookProgressFromLegado,
      uploadCurrentBookProgressToLegado,
      persistProgress, flushProgressToServer, flushProgressToServerKeepalive,
      config, updateConfig, resetConfig, saveConfig, setBackgroundImage, clearBackgroundImage,
      customFonts, fetchCustomFonts, importCustomFont, removeCustomFont, customFontFamily,
    themeIndex, isNight, currentTheme, chromeTheme, setThemeIndex, toggleNight,
    autoReading, autoReadingTimer, toggleAutoReading, stopAutoReading,
    activePanel, openPanel, togglePanel, backPanel, closePanel,
    bookmarks, fetchBookmarks, addBookmark, removeBookmark, removeBookmarks,
    readChapterKeys, isChapterRead, markChapterAsRead,
    replaceRules, fetchReplaceRules,
    switchSource, preloadNextChapter, preloadAroundChapter,
    refreshChapters, refreshTocFromSource,
    isSpeaking, isSpeechLoading, isPaused, speechProgress, startTTS, pauseTTS, stopTTS,
    voiceList, speechConfig, speechStopAt, speechProviderLabel, openAISpeechConfigured,
    systemTtsNativeEventsReliable, systemSpeechSupported,
    fetchVoices, setVoiceName, setSpeechProvider, setSpeechRate, setSpeechPitch, setSpeechStopTimer, clearSpeechStopTimer,
    setOpenAISpeechSource, setSpeechApiFormat, setOpenAISpeechBaseUrl, setSpeechProxyUrl, setOpenAISpeechApiKey, setOpenAISpeechModel, setOpenAISpeechVoice, setOpenAISpeechFormat, setOpenAISpeechRequestMode, preloadOpenAITTS,
    displayContent, processContentForDisplay,
    isAutoScrolling,
  }
})
