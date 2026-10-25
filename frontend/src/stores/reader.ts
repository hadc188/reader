import { defineStore } from 'pinia'
import { ref, computed, reactive, watch } from 'vue'
import { useAppStore } from './app'
import { useBookshelfStore } from './bookshelf'
import { isTauriRuntime } from '../api/transport'
import {
  getChapterList,
  getBookInfo,
  getBookContent,
  saveBookProgress,
  setBookSource as apiSetBookSource,
  type ContentLoadStage,
} from '../api/bookshelf'
import { invokeRaw } from '../api/invoke'
import {
  getBookmarks,
  saveBookmark,
  deleteBookmark as apiDeleteBookmark,
  deleteBookmarks as apiDeleteBookmarks,
} from '../api/bookmark'
import { applyReplaceRulesInSandbox, getReplaceRules, type SandboxReplaceRuleStep } from '../api/replaceRule'
import type { Book, BookChapter, Bookmark, ReplaceRule } from '../types'
import { isLocalBook } from '../utils/localBook'
import { PDF_PAGE_ATTR } from '../utils/pdfPages'
import { saveRecentReadBook } from '../utils/recentBooks'
import {
  DEFAULT_READER_BACKGROUND_OPACITY,
  DEFAULT_READER_PANEL_OPACITY,
  normalizeReaderBackgroundOpacity,
  normalizeReaderPanelOpacity,
} from '../utils/readerBackground'
import { normalizeReaderColor } from '../utils/readerColor'
import {
  HOTKEY_ACTION_MAP,
  bindActionToKey,
  isDefaultHotkeyBindings,
  listHotkeyConflicts,
  normalizeHotkeyBindings,
  unbindActionFromKey,
  type HotkeyActionId,
  type HotkeyBindings,
} from '../utils/readerHotkeys'
import {
  DEFAULT_OPENAI_BASE_URL,
  getSpeechApiFormatOption,
  inferSpeechApiFormat,
  requestOpenAISpeechAudio,
  type SpeechApiFormat,
  type SpeechAudioFormat,
} from '../utils/openaiSpeech'
import {
  syncLegadoBookProgress,
  uploadLegadoBookProgress,
  type LegadoBookProgress,
} from '../api/webdav'
import { deleteCustomFont, listCustomFonts, uploadCustomFont, type CustomFontEntry } from '../api/fonts'
import { deleteBackgroundImage } from '../api/background'
import { normalizeAudioContentType, synthesizeHttpTts } from '../api/httpTts'
import { synthesizeEdgeTts, type EdgeTtsFormat } from '../api/edgeTts'
import { synthesizeMsTts, type MsTtsFormat } from '../api/msTts'
import { splitSpeechSegments, type SpeechSegment, type SpeechSegmentType } from '../utils/speechSegments'
import {
  readSavedReadingPosition,
  resolveReadingPosition,
  toMillis,
  type ReadingPositionSnapshot,
} from '../utils/readingPosition'
import { compileReplaceRule, toSandboxRuleStep } from '../utils/replaceRuleRegex'

const READER_SESSION_KEY = 'reader-last-session'
const READER_READ_HISTORY_PREFIX = 'reader-read-history:'
const READER_BACKGROUND_IMAGE_KEY = 'reader-background-image'
/** 阅读页专属背景图: 与桌面背景图分开, 互不覆盖。 */
const READER_PAGE_BACKGROUND_IMAGE_KEY = 'reader-page-background-image'
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

/** 只含阅读进度的轻量快照。
 *
 *  背景: `setChapterScrollProgress` 由 handleScroll 在**每次滚动事件**里调用,
 *  而它过去直接走 saveReaderSession —— 后者每次都把整个 chapters 数组(上千章
 *  可达 500KB)JSON 序列化并同步写 localStorage。实测 3000 章时单次 ~10ms,
 *  一次 300ms 的翻页补间(约 18 帧)会因此阻塞主线程 ~180ms, 表现为掉帧与
 *  上一页残留; 章节越多越明显。
 *
 *  进度本身只是个 0~1 的小数与索引, 与「书 + 目录」这种极少变化的结构分开存,
 *  既避免重复序列化大对象, 也让高频写入的代价可以忽略(实测 0.006ms)。 */
interface PersistedReaderProgress {
  bookUrl: string
  currentIndex: number
  chapterScrollProgress: number
  updatedAt: number
}

const READER_PROGRESS_KEY = 'reader-last-progress'

/* ─── Reading config type ─── */
export const READER_PAGE_WIDTH_MIN = 400
export const READER_PAGE_WIDTH_MAX = 2000
export const READER_PAGE_VERTICAL_PADDING_MAX = 200

export interface ReadConfig {
  fontSize: number
  fontWeight: number
  fontFamily: string
  lineHeight: number
  paragraphSpacing: number
  firstLineIndent: boolean
  fontColor: string
  pageWidth: number
  /** 左右分页时每页正文的上下留白, 像素。 */
  pageVerticalPadding: number
  readMethod: '上下滑动' | '左右翻页' | '上下滚动' | '上下滚动2'
  animateDuration: number
  /** 自动阅读滚动速度, 像素/秒。 */
  autoScrollSpeed: number
  clickAction: 'next' | 'auto' | 'none'
  selectAction: 'popup' | 'ignore'
  /** 简繁转换档位。'off' 表示原文直出（与手机端 legado 的「关闭」一致）。 */
  chineseMode: 'off' | 'simplified' | 'traditional'
  enablePreload: boolean
  /** 桌面指针靠近阅读区边缘时自动展开工具栏/悬浮钮。
   *  默认关闭: 边缘热区会吃掉正文的点击(左右各 84px), 用户反馈鼠标停在边缘就弹按钮、
   *  可点区域变少且容易误触(issue #7 问题 1)。需要的人可在「更多」里开启。 */
  hoverRevealControls: boolean
  backgroundImage: string
  backgroundOpacity: number
  applyBackgroundToReader: boolean
  /** 阅读页背景色, '' 表示跟随主题。 */
  readerBackgroundColor: string
  /** 阅读页专属背景图(reader:// 的 URL), 与桌面背景图相互独立。 */
  readerBackgroundImage: string
  /** 阅读页背景图透明度。 */
  readerImageOpacity: number
  /** 阅读页浮层（目录/书签/设置抽屉、右键菜单等）的底板不透明度, 1 为实色。
   *  阅读页铺了背景图时, 半透明底板能让用户边调设置边看到正文变化。 */
  panelOpacity: number
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
  pageVerticalPadding: 24,
  readMethod: '上下滑动',
  animateDuration: 300,
  autoScrollSpeed: 30,
  clickAction: 'auto',
  selectAction: 'ignore',
  chineseMode: 'simplified',
  enablePreload: true,
  // 默认关闭: 边缘热区会吃掉正文点击, 用户反馈"可点区域变少、还要小心别误点按钮"
  // (issue #7 问题 1)。想用悬停呼出的人可在设置「更多」里开启。
  hoverRevealControls: false,
  backgroundImage: '',
  backgroundOpacity: DEFAULT_READER_BACKGROUND_OPACITY,
  applyBackgroundToReader: true,
  readerBackgroundColor: '',
  readerBackgroundImage: '',
  readerImageOpacity: DEFAULT_READER_BACKGROUND_OPACITY,
  panelOpacity: DEFAULT_READER_PANEL_OPACITY,
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
        // 旧存档只有简化/繁体两档; 脏值(手改 localStorage)一律回落默认,
        // 否则 convertContent 会走进一个不存在的分支。
        chineseMode: savedConfig.chineseMode === 'off'
          || savedConfig.chineseMode === 'traditional'
          || savedConfig.chineseMode === 'simplified'
          ? savedConfig.chineseMode
          : defaultConfig.chineseMode,
        pageWidth: typeof savedConfig.pageWidth === 'number'
          ? Math.min(
              READER_PAGE_WIDTH_MAX,
              Math.max(READER_PAGE_WIDTH_MIN, Math.round(savedConfig.pageWidth)),
            )
          : defaultConfig.pageWidth,
        pageVerticalPadding: typeof savedConfig.pageVerticalPadding === 'number'
          ? Math.min(
              READER_PAGE_VERTICAL_PADDING_MAX,
              Math.max(0, Math.round(savedConfig.pageVerticalPadding)),
            )
          : defaultConfig.pageVerticalPadding,
        autoScrollSpeed: typeof savedConfig.autoScrollSpeed === 'number' && savedConfig.autoScrollSpeed > 0
          ? Math.min(500, Math.max(2, Math.round(savedConfig.autoScrollSpeed)))
          : Math.min(500, Math.max(2, Math.round(legacyAutoScrollSpeed))),
        backgroundImage: localStorage.getItem(READER_BACKGROUND_IMAGE_KEY)
          || (typeof savedConfig.backgroundImage === 'string' ? savedConfig.backgroundImage : ''),
        backgroundOpacity: normalizeReaderBackgroundOpacity(savedConfig.backgroundOpacity),
        applyBackgroundToReader: typeof savedConfig.applyBackgroundToReader === 'boolean'
          ? savedConfig.applyBackgroundToReader
          : true,
        // 旧存档没有这个字段时按默认(关闭)处理, 非布尔值也回落默认, 避免脏数据开启悬停。
        hoverRevealControls: typeof savedConfig.hoverRevealControls === 'boolean'
          ? savedConfig.hoverRevealControls
          : defaultConfig.hoverRevealControls,
        // 自定义配色来自 localStorage, 必须收紧为合法色值再进 style。
        fontColor: normalizeReaderColor(savedConfig.fontColor),
        readerBackgroundColor: normalizeReaderColor(savedConfig.readerBackgroundColor),
        // 阅读页专属背景图与桌面背景图分开存(都是 reader:// 的 URL, 不进 readConfig)。
        readerBackgroundImage: localStorage.getItem(READER_PAGE_BACKGROUND_IMAGE_KEY)
          || (typeof savedConfig.readerBackgroundImage === 'string' ? savedConfig.readerBackgroundImage : ''),
        readerImageOpacity: normalizeReaderBackgroundOpacity(savedConfig.readerImageOpacity),
        panelOpacity: normalizeReaderPanelOpacity(savedConfig.panelOpacity),
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

/**
 * 缓冲流句柄：由 useReaderAutoPlayback 创建 SpeechStream 时注册进 store，
 * 让暂停/停止按钮能作用到流自己的 <audio>（而不是 store 里那套单段播放器）。
 */
export interface SpeechStreamControls {
  pause: () => void
  resume: () => void
  stop: () => void
}

const OPENAI_AUDIO_PRELOAD_LIMIT = 8

/** 复用在途预载时的加载动画延迟。命中预载本该是"瞬间换章", 200ms 内返回就完全
 *  不显示动画; 超过则说明书源确实慢, 亮出动画免得用户以为翻页没反应。 */
const CHAPTER_LOADING_INDICATOR_DELAY_MS = 200

/**
 * 空/纯空白的 URL 字段归一成 undefined。
 *
 * Legado 的 `Book.tocUrl` 是非空 `String`，从手机端备份恢复（或从手机端 WebDAV
 * 同步过来）的书架里它常常就是 `""`。前端若把这个空串当成"有目录地址"，
 * 就会带着空 URL 去请求目录，同时跳过"缺 tocUrl 就取详情"的分支 ——
 * 结果正是 issue #22 的「打开书一直转圈，切换一次书源才好」。
 */
function normalizeOptionalUrl(value?: string | null): string | undefined {
  const trimmed = (value ?? '').trim()
  return trimmed ? trimmed : undefined
}

/**
 * 朗读引擎。
 *
 * `'http'` 指 Legado 的「在线朗读引擎」（httpTTS）：URL 模板由后端展开，前端
 * 只把正文和语速交过去，收回来的就是一段完整音频 —— 所以它和 `'openai'`
 * 共用同一套 HTMLAudioElement 播放器，而不是复用系统语音。
 *
 * `'edge'` 是 Edge 朗读（edge-tts）：后端走微软的在线语音服务拿 mp3，同样
 * 一段文本一段音频，因此也复用 HTMLAudioElement 播放器与缓冲流。
 *
 * `'msdemo'` 是「微软朗读」（微软翻译客户端演示接口）：同样是一段文本一段 mp3，
 * 额外支持风格 style / 风格强度，也走 HTMLAudioElement 播放器与缓冲流。
 */
export type SpeechProvider = 'system' | 'openai' | 'http' | 'edge' | 'msdemo'
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
  /** 选中的在线朗读引擎 id，null 表示尚未选择。 */
  httpTtsId: number | string | null
  /** 选中引擎声明的 contentType，仅作音频 MIME 嗅探失败时的兜底。 */
  httpTtsContentType: string
  /** 旁白/对话分音色开关。关闭时下面 5 个字段仍保留（切换回来不用重选）。 */
  dualVoiceEnabled: boolean
  /** 系统语音的对话音色名；空串表示跟随旁白音色。 */
  dialogueVoiceName: string
  /** API 语音的对话音色；空串表示跟随旁白音色。 */
  dialogueOpenaiVoice: string
  /** 在线朗读引擎的对话引擎 id；null 表示跟随旁白引擎。 */
  dialogueHttpTtsId: number | string | null
  dialogueHttpTtsContentType: string
  /** Edge 朗读的旁白音色 shortName，默认云希（男声，适合旁白）。 */
  edgeVoice: string
  /** Edge 朗读的对话音色；空串表示跟随 `edgeVoice`。 */
  edgeDialogueVoice: string
  /** Edge 音调偏移（Hz），-100..100，默认 0（不偏移）。 */
  edgePitch: number
  /** Edge 音量偏移（%），-100..100，默认 0（不偏移）。 */
  edgeVolume: number
  /** Edge 音质：48k（默认，省流量）/ 96k（音质更好，字节数约为 2 倍）。 */
  edgeFormat: EdgeTtsFormat
  /** 微软朗读的旁白音色 shortName，默认云健（男声，适合旁白）。 */
  msVoice: string
  /** 微软朗读的对话音色；空串表示跟随 `msVoice`。 */
  msDialogueVoice: string
  /** 微软朗读的旁白风格（来自音色的 StyleList）；空串表示无风格。 */
  msStyle: string
  /** 微软朗读的对话风格；空串表示跟随旁白风格。 */
  msDialogueStyle: string
  /** 风格强度 0.5–2.0（默认 1.0，仅在有风格时生效）。 */
  msStyleDegree: number
  /** 音调偏移（%），-50..50，默认 0。注意这个通道的 pitch 是百分比而非 Hz。 */
  msPitch: number
  /** 音量偏移（%），-100..100，默认 0。 */
  msVolume: number
  /** 音质：48k / 96k（默认）/ 192k。 */
  msFormat: MsTtsFormat
  /**
   * 缓冲流的目标字符数（就绪未播 + 在途）：越大越不容易断档，代价是更早的
   * 网络请求与更多内存。只影响自动朗读的 http / openai / edge / msdemo 通道。
   */
  bufferChars: number
}

/** Edge 朗读的默认旁白音色：云希（zh-CN-YunxiNeural），男声，适合旁白。 */
export const DEFAULT_EDGE_VOICE = 'zh-CN-YunxiNeural'
/** Edge 音调 / 音量的取值范围（后端 SSML 支持 -100..100）。 */
export const EDGE_PITCH_MIN = -100
export const EDGE_PITCH_MAX = 100
export const EDGE_VOLUME_MIN = -100
export const EDGE_VOLUME_MAX = 100
/** Edge 边读端点只接受这两种 outputFormat；默认 48k（省流量）。 */
export const DEFAULT_EDGE_FORMAT: EdgeTtsFormat = '48k'

/** 微软朗读的默认旁白音色：云健（zh-CN-YunjianNeural），男声，适合旁白。 */
export const DEFAULT_MS_VOICE = 'zh-CN-YunjianNeural'
/** 微软朗读只支持三种音质档位；默认 96k（这个通道的音质档位是它的优势）。 */
export const DEFAULT_MS_FORMAT: MsTtsFormat = '96k'
export const MS_FORMATS: MsTtsFormat[] = ['48k', '96k', '192k']
/** 风格强度 / 音调 / 音量的取值范围。 */
export const MS_STYLE_DEGREE_MIN = 0.5
export const MS_STYLE_DEGREE_MAX = 2
export const MS_PITCH_MIN = -50
export const MS_PITCH_MAX = 50
export const MS_VOLUME_MIN = -100
export const MS_VOLUME_MAX = 100

/** 缓冲字符数的默认值与取值范围。 */
export const DEFAULT_BUFFER_CHARS = 500
export const BUFFER_CHARS_MIN = 200
export const BUFFER_CHARS_MAX = 2000

/**
 * 引擎 id 归一化：后端 id 是毫秒时间戳（number），旧配置/手写配置可能是字符串。
 * 0 与空串都表示「未选择」——保存时后端会给 0 补发新 id，拿 0 去请求只会找不到。
 */
function normalizeHttpTtsId(value: unknown): number | string | null {
  if (typeof value === 'number') {
    return Number.isFinite(value) && value !== 0 ? value : null
  }
  if (typeof value === 'string') {
    const trimmed = value.trim()
    return trimmed && trimmed !== '0' ? trimmed : null
  }
  return null
}

/** 缓冲字符数归一化：非数字回默认值，数字夹到 [200, 2000] 并取整。 */
function normalizeBufferChars(value: unknown): number {
  if (typeof value !== 'number' || !Number.isFinite(value)) return DEFAULT_BUFFER_CHARS
  return Math.min(BUFFER_CHARS_MAX, Math.max(BUFFER_CHARS_MIN, Math.round(value)))
}

/** Edge 旁白音色归一化：空/非字符串都回默认音色，避免下拉显示空白且发不出请求。 */
function normalizeEdgeVoice(value: unknown): string {
  return typeof value === 'string' && value.trim() ? value.trim() : DEFAULT_EDGE_VOICE
}

/** Edge 对话音色归一化：空串是合法值（表示跟随旁白），非字符串一律当空。 */
function normalizeEdgeDialogueVoice(value: unknown): string {
  return typeof value === 'string' && value.trim() ? value.trim() : ''
}

/** Edge 音调/音量归一化：非数字回 0，数字夹到 [-100, 100] 并取整。 */
function normalizeEdgeSignedLevel(value: unknown): number {
  if (typeof value !== 'number' || !Number.isFinite(value)) return 0
  return Math.min(100, Math.max(-100, Math.round(value)))
}

/**
 * Edge 音质归一化：服务端只接受 48k / 96k 两档，其它历史脏值一律回默认 48k
 * （宁可退回标准音质，也不要带着非法 outputFormat 让合成整个失败）。
 */
function normalizeEdgeFormat(value: unknown): EdgeTtsFormat {
  return value === '96k' ? '96k' : DEFAULT_EDGE_FORMAT
}

/** 微软朗读旁白音色归一化：空/非字符串回默认音色。 */
function normalizeMsVoice(value: unknown): string {
  return typeof value === 'string' && value.trim() ? value.trim() : DEFAULT_MS_VOICE
}

/** 微软朗读对话音色归一化：空串是合法值（跟随旁白）。 */
function normalizeMsDialogueVoice(value: unknown): string {
  return typeof value === 'string' && value.trim() ? value.trim() : ''
}

/** 风格名归一化：空/非字符串都当「无风格」。 */
function normalizeMsStyle(value: unknown): string {
  return typeof value === 'string' && value.trim() ? value.trim() : ''
}

/**
 * 风格强度归一化：夹到 0.5–2.0 并保留一位小数（服务端按 0.1 步进理解）。
 * 非数字回默认 1.0。
 */
function normalizeMsStyleDegree(value: unknown): number {
  if (typeof value !== 'number' || !Number.isFinite(value)) return 1
  const clamped = Math.min(MS_STYLE_DEGREE_MAX, Math.max(MS_STYLE_DEGREE_MIN, value))
  return Math.round(clamped * 10) / 10
}

/** 微软朗读音调/音量归一化：非数字回 0，按各自范围取整夹紧。 */
function normalizeMsLevel(value: unknown, min: number, max: number): number {
  if (typeof value !== 'number' || !Number.isFinite(value)) return 0
  return Math.min(max, Math.max(min, Math.round(value)))
}

/** 微软朗读音质归一化：只接受 48k/96k/192k，其它（含脏值）回默认 96k。 */
function normalizeMsFormat(value: unknown): MsTtsFormat {
  return MS_FORMATS.includes(value as MsTtsFormat) ? value as MsTtsFormat : DEFAULT_MS_FORMAT
}

/** 带符号百分比（微软朗读的 rate/pitch/volume 都是百分比串，0 写 `+0%`）。 */
export function formatMsPercent(percent: number): string {
  const value = Number.isFinite(percent) ? Math.round(percent) : 0
  return value >= 0 ? `+${value}%` : `${value}%`
}

/**
 * 倍速 → Edge 的 rate 参数（带符号百分比）。
 *
 * edge-tts 的 rate 是「相对默认语速的百分比」，如 `-20%`、`+15%`；我们的
 * `speechRate` 是倍速（1 = 正常）。必须显式带 `+`：后端按 `+N%`/`-N%` 解析，
 * 丢符号会被当成非法参数。0 也要写成 `+0%`。
 */
export function formatEdgeRate(speechRate: number): string {
  const rate = Number.isFinite(speechRate) ? speechRate : 1
  const delta = Math.round((rate - 1) * 100)
  return delta >= 0 ? `+${delta}%` : `${delta}%`
}

/**
 * 音调偏移 → Edge 的 pitch 参数（带符号 Hz）。
 *
 * 与 rate 同理：SSML 属性要求显式符号，0 必须写成 `+0Hz` 而不是 `0Hz`。
 */
export function formatEdgePitch(pitchHz: number): string {
  const value = Number.isFinite(pitchHz) ? Math.round(pitchHz) : 0
  return value >= 0 ? `+${value}Hz` : `${value}Hz`
}

/** 音量偏移 → Edge 的 volume 参数（带符号百分比）。 */
export function formatEdgeVolume(volumePercent: number): string {
  const value = Number.isFinite(volumePercent) ? Math.round(volumePercent) : 0
  return value >= 0 ? `+${value}%` : `${value}%`
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
  httpTtsId: null,
  httpTtsContentType: '',
  dualVoiceEnabled: false,
  dialogueVoiceName: '',
  dialogueOpenaiVoice: '',
  dialogueHttpTtsId: null,
  dialogueHttpTtsContentType: '',
  edgeVoice: DEFAULT_EDGE_VOICE,
  edgeDialogueVoice: '',
  edgePitch: 0,
  edgeVolume: 0,
  edgeFormat: DEFAULT_EDGE_FORMAT,
  msVoice: DEFAULT_MS_VOICE,
  msDialogueVoice: '',
  msStyle: '',
  msDialogueStyle: '',
  msStyleDegree: 1,
  msPitch: 0,
  msVolume: 0,
  msFormat: DEFAULT_MS_FORMAT,
  bufferChars: DEFAULT_BUFFER_CHARS,
}

function loadSpeechConfig(): SpeechConfig {
  try {
    const saved = localStorage.getItem('reader-speechConfig')
    if (saved) {
      const parsed = JSON.parse(saved) as Partial<SpeechConfig>
      const apiFormat = ['openai', 'fish', 'elevenlabs', 'azure'].includes(parsed.apiFormat || '')
        ? parsed.apiFormat as SpeechApiFormat
        : inferSpeechApiFormat(parsed.openaiBaseUrl || defaultSpeechConfig.openaiBaseUrl)
      // 必须逐字段显式归一化：`{ ...默认值, ...parsed }` 只能补「缺失的 key」，
      // 补不住「key 存在但值非法」。老配置只有两态 provider、没有 httpTtsId，
      // 手工改坏的 localStorage 也可能带脏值，这里统一收回合法取值域，
      // 否则 http 分支会带着不存在的 id 去请求后端，用户只看到一句「引擎不存在」。
      const provider: SpeechProvider = parsed.provider === 'openai'
        || parsed.provider === 'http'
        || parsed.provider === 'edge'
        || parsed.provider === 'msdemo'
        ? parsed.provider
        : 'system'
      const httpTtsId = normalizeHttpTtsId(parsed.httpTtsId)
      // 只接受音频类型：这个值最终会作为 Blob.type，脏值会让 <audio> 直接 fail。
      const httpTtsContentType = normalizeAudioContentType(parsed.httpTtsContentType)
      // 双音色字段同样是「值存在但可能非法」：布尔必须是 true 才算开启。
      const dualVoiceEnabled = parsed.dualVoiceEnabled === true
      const dialogueVoiceName = typeof parsed.dialogueVoiceName === 'string' ? parsed.dialogueVoiceName : ''
      const dialogueOpenaiVoice = typeof parsed.dialogueOpenaiVoice === 'string' ? parsed.dialogueOpenaiVoice : ''
      const dialogueHttpTtsId = normalizeHttpTtsId(parsed.dialogueHttpTtsId)
      const dialogueHttpTtsContentType = normalizeAudioContentType(parsed.dialogueHttpTtsContentType)
      // 缓冲字符数越界会让缓冲流要么白等、要么一次预取整章，必须夹回区间。
      const bufferChars = normalizeBufferChars(parsed.bufferChars)
      const edgeVoice = normalizeEdgeVoice(parsed.edgeVoice)
      const edgeDialogueVoice = normalizeEdgeDialogueVoice(parsed.edgeDialogueVoice)
      // 音调/音量是 SSML 参数：越界会让后端生成失败，脏值必须夹回 -100..100。
      const edgePitch = normalizeEdgeSignedLevel(parsed.edgePitch)
      const edgeVolume = normalizeEdgeSignedLevel(parsed.edgeVolume)
      const edgeFormat = normalizeEdgeFormat(parsed.edgeFormat)
      const msVoice = normalizeMsVoice(parsed.msVoice)
      const msDialogueVoice = normalizeMsDialogueVoice(parsed.msDialogueVoice)
      const msStyle = normalizeMsStyle(parsed.msStyle)
      const msDialogueStyle = normalizeMsStyle(parsed.msDialogueStyle)
      const msStyleDegree = normalizeMsStyleDegree(parsed.msStyleDegree)
      const msPitch = normalizeMsLevel(parsed.msPitch, MS_PITCH_MIN, MS_PITCH_MAX)
      const msVolume = normalizeMsLevel(parsed.msVolume, MS_VOLUME_MIN, MS_VOLUME_MAX)
      const msFormat = normalizeMsFormat(parsed.msFormat)
      return {
        ...defaultSpeechConfig,
        ...parsed,
        apiFormat,
        provider,
        httpTtsId,
        httpTtsContentType,
        dualVoiceEnabled,
        dialogueVoiceName,
        dialogueOpenaiVoice,
        dialogueHttpTtsId,
        dialogueHttpTtsContentType,
        edgeVoice,
        edgeDialogueVoice,
        edgePitch,
        edgeVolume,
        edgeFormat,
        msVoice,
        msDialogueVoice,
        msStyle,
        msDialogueStyle,
        msStyleDegree,
        msPitch,
        msVolume,
        msFormat,
        bufferChars,
      }
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
  /** 当前章节加载到哪一步（空串表示未在加载）；用于显示「正在解析段评」这类长耗时 */
  const loadStage = ref<ContentLoadStage | ''>('')
  // 阶段提示的持有者：并发加载（预加载 / 连续模式）时只有最后设置提示的那次调用
  // 能清掉它，避免旧请求提前清掉新章节的提示、或提示永久卡住。
  let chapterStageSeq = 0
  let chapterStageOwner = 0
  /** 原地刷新当前章(非切章加载)。连续阅读模式据此保留已渲染的章节 DOM。 */
  const refreshing = ref(false)
  /** 原地刷新替换正文后自增。阅读页据此在重排后把视口锚回当前章原进度。 */
  const contentRefreshToken = ref(0)
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
  /** 打开书失败时的原因（空串表示没有失败）。阅读页据此显示可重试的错误态，
   *  而不是把所有失败都表现成永久的「加载中…」。 */
  const bookLoadError = ref('')
  const bookmarks = ref<Bookmark[]>([])
  const replaceRules = ref<ReplaceRule[]>([])
  /**
   * 执行失败的净化规则: 规则名 → 错误信息。
   *
   * Legado 规则是 Java 正则, 少数写法在 JS 里没有等价物; 这些规则导入时是"成功"的,
   * 但每次应用都会失败。收集起来供规则管理面板标出, 免得用户以为规则没生效是阅读器的问题。
   */
  const failedReplaceRules = ref<Record<string, string>>({})
  const preloadedContent = ref<Map<number, string>>(new Map()) // index -> content
  const preloadingContent = new Map<number, Promise<string | null>>()
  let chapterPreloadGeneration = 0
  const isAutoScrolling = ref(false)
  /** 阅读页上/下工具栏是否呼出。由 ReaderView 维护, 标题栏据此决定是否实色
   *  —— 阅读时标题栏透明(沉浸), 呼出工具栏时转为实色(清晰)。 */
  const readerControlsVisible = ref(false)
  const chapterScrollProgress = ref(0)
  const readChapterKeys = ref<Set<string>>(new Set())
  const progressDirty = ref(false)
  const lastServerProgressKey = ref('')
  /** 正在路上的 keepalive 写入: 同一份 payload 不重复发送, 但**成功才**置
   *  `lastServerProgressKey`(见 flushProgressToServerKeepalive 的说明)。 */
  let keepaliveInFlightKey = ''
  let keepaliveInFlight: Promise<void> | null = null
  const pendingLegadoProgress = ref<{ index: number; position: number } | null>(null)
  const legadoPositionRestore = ref<{
    index: number
    position: number
    progress: number
    token: number
    ready: boolean
  } | null>(null)
  const legadoPositionRestoreCompleted = ref<{ index: number; token: number } | null>(null)
  const restoringLegadoProgress = ref(false)
  const customFonts = ref<CustomFontEntry[]>([])
  let chapterLoadRequestId = 0
  let bookLoadRequestId = 0
  let legadoPositionRestoreToken = 0

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
    // 两张背景图都是落盘的图片地址, 各自存在独立的 localStorage key 里, 不进 readConfig。
    delete persistedConfig.backgroundImage
    delete persistedConfig.readerBackgroundImage
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
    // 这些是「阅读页外观」偏好: 与背景图同等对待, 重置排版参数时一并保留,
    // 否则用户调一次字号就会把刚配好的阅读配色清掉。
    const backgroundPreferences = {
      backgroundImage: config.backgroundImage,
      backgroundOpacity: config.backgroundOpacity,
      applyBackgroundToReader: config.applyBackgroundToReader,
      readerBackgroundColor: config.readerBackgroundColor,
      readerBackgroundImage: config.readerBackgroundImage,
      readerImageOpacity: config.readerImageOpacity,
      panelOpacity: config.panelOpacity,
      fontColor: config.fontColor,
    }
    Object.assign(config, defaultConfig)
    Object.assign(config, backgroundPreferences)
    saveConfig()
  }

  /* ─── Hotkeys ─── */
  const HOTKEY_BINDINGS_KEY = 'reader-hotkeys'
  const hotkeyBindings = ref<HotkeyBindings>(normalizeHotkeyBindings(readStoredHotkeys()))

  function readStoredHotkeys(): unknown {
    try {
      const saved = localStorage.getItem(HOTKEY_BINDINGS_KEY)
      return saved ? JSON.parse(saved) : null
    } catch {
      return null
    }
  }

  function saveHotkeyBindings() {
    localStorage.setItem(HOTKEY_BINDINGS_KEY, JSON.stringify(hotkeyBindings.value))
  }

  /** 覆写某个按键的绑定(空数组表示该键不执行任何动作)。 */
  function setHotkeyBinding(keyId: string, actions: HotkeyActionId[]) {
    const normalized = normalizeHotkeyBindings({ ...hotkeyBindings.value, [keyId]: actions })
    hotkeyBindings.value = normalized
    saveHotkeyBindings()
  }

  /** 「功能→按键」界面的绑定入口: 追加到末尾, 保留已有功能的优先次序。 */
  function bindHotkeyKey(keyId: string, actionId: HotkeyActionId) {
    hotkeyBindings.value = bindActionToKey(hotkeyBindings.value, keyId, actionId)
    saveHotkeyBindings()
  }

  function unbindHotkeyKey(keyId: string, actionId: HotkeyActionId) {
    hotkeyBindings.value = unbindActionFromKey(hotkeyBindings.value, keyId, actionId)
    saveHotkeyBindings()
  }

  /** 当前所有「同模式多动作」冲突, 供设置界面提示。 */
  const hotkeyConflicts = computed(() => listHotkeyConflicts(hotkeyBindings.value))

  function resetHotkeyBindings() {
    hotkeyBindings.value = normalizeHotkeyBindings(null)
    saveHotkeyBindings()
  }

  const hotkeysAreDefault = computed(() => isDefaultHotkeyBindings(hotkeyBindings.value))

  /** 设置界面矩阵: 供 UI 直接读取动作元信息。 */
  const hotkeyActions = HOTKEY_ACTION_MAP

  function setBackgroundImage(url: string) {
    const previous = config.backgroundImage
    try {
      localStorage.setItem(READER_BACKGROUND_IMAGE_KEY, url)
      config.backgroundImage = url
    } catch {
      throw new Error('背景图片保存失败，请选择尺寸更小的图片')
    }
    // 换图后把上一张落盘文件删掉, 否则每换一次都会在 storage 里留一份孤儿图片。
    // 旧版存的是 dataURL, 后端只认自己生成的 URL, 传进去是空操作。
    if (previous && previous !== url) {
      void deleteBackgroundImage(previous).catch(() => undefined)
    }
  }

  function clearBackgroundImage() {
    const previous = config.backgroundImage
    localStorage.removeItem(READER_BACKGROUND_IMAGE_KEY)
    config.backgroundImage = ''
    if (previous) void deleteBackgroundImage(previous).catch(() => undefined)
  }

  function setReaderBackgroundImage(url: string) {
    const previous = config.readerBackgroundImage
    try {
      localStorage.setItem(READER_PAGE_BACKGROUND_IMAGE_KEY, url)
      config.readerBackgroundImage = url
    } catch {
      throw new Error('阅读页背景图片保存失败，请选择尺寸更小的图片')
    }
    if (previous && previous !== url) {
      void deleteBackgroundImage(previous).catch(() => undefined)
    }
  }

  function clearReaderBackgroundImage() {
    const previous = config.readerBackgroundImage
    localStorage.removeItem(READER_PAGE_BACKGROUND_IMAGE_KEY)
    config.readerBackgroundImage = ''
    if (previous) void deleteBackgroundImage(previous).catch(() => undefined)
  }

  /** 两个方向的转换函数。以前只加载 traditionalized, 「简体」档因此无从转换 ——
   *  繁体内容(常见于本地导入的书)选「简体」时毫无变化, 就是 issue #6 第 6 条。
   *
   *  词表与手机端 legado 一致(见 utils/chinese.js 的说明); 「关闭」档不转换,
   *  因此不需要为它加载这份几十万字的词典。 */
  const chineseConverters = ref<{
    traditional: (text: string) => string
    simplified: (text: string) => string
  } | null>(null)
  let chineseLoading: Promise<void> | null = null

  async function ensureChineseConverterLoaded() {
    if (config.chineseMode === 'off') return
    if (chineseConverters.value || chineseLoading) return chineseLoading || Promise.resolve()
    chineseLoading = import('../utils/chinese.js')
      .then((module) => {
        chineseConverters.value = {
          traditional: module.traditionalized,
          simplified: module.simplified,
        }
      })
      .catch(() => {
        chineseConverters.value = null
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
  /** 阅读页实际使用的配色: 自定义色覆盖主题预设的同名字段。
   *  只覆盖阅读页, 弹层等界面仍走 chromeTheme 的主题色。 */
  const readerTheme = computed<ThemePreset>(() => {
    const activeTheme = currentTheme.value
    const backgroundColor = normalizeReaderColor(config.readerBackgroundColor)
    const color = normalizeReaderColor(config.fontColor)
    if (!backgroundColor && !color) return activeTheme
    return {
      ...activeTheme,
      body: backgroundColor || activeTheme.body,
      content: backgroundColor || activeTheme.content,
      fontColor: color || activeTheme.fontColor,
      // popup 也要跟着走: 它是阅读页顶栏与工具栏的**实色**背景。若仍用预设的
      // popup, 设了自定义阅读背景色之后, 呼出工具栏会在正文底色上方贴一条
      // 预设色的栏, 两截颜色对不上。(与 backgroundColor 同色, 拼成连续一条。)
      popup: backgroundColor || activeTheme.popup,
    }
  })
  /** 阅读页背景色(已归一化)。空串表示跟随主题 / 让位给桌面背景图。 */
  const readerBackgroundColor = computed(() => normalizeReaderColor(config.readerBackgroundColor))
  /** 阅读页浮层底板配色。
   *  popup 同时是抽屉、右键菜单、划词菜单、搜索面板的底色, 因此这里统一按
   *  「面板不透明度」合成一次; 100% 时直接返回主题色, 与旧版实色完全一致。 */
  const chromeTheme = computed<ThemePreset>(() => {
    const activeTheme = currentTheme.value
    const opacity = normalizeReaderPanelOpacity(config.panelOpacity)
    if (opacity >= 1) return activeTheme
    return {
      ...activeTheme,
      popup: `color-mix(in srgb, ${activeTheme.popup} ${Math.round(opacity * 100)}%, transparent)`,
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

  /** 规则是否作用于当前这本书。 */
  function matchReplaceRuleScope(rule: ReplaceRule) {
    const currentBook = book.value
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

  /** 当前生效的规则, 已按 order 排序。 */
  const enabledReplaceRules = computed(() =>
    [...replaceRules.value]
      .filter(rule => rule.isEnabled && matchReplaceRuleScope(rule))
      .sort((a, b) => a.order - b.order),
  )

  function isScriptRule(rule: ReplaceRule) {
    return /^@js:/i.test((rule.replacement || '').trim())
  }

  /**
   * 同步执行规则链, **跳过 `@js:` 脚本规则**。
   *
   * 脚本规则只能在 Rust 沙盒里跑（渲染进程执行第三方脚本等于把 IPC 面交出去），
   * 所以这里先给出不含脚本的结果；`displayContent` 会在沙盒结果到达后替换成完整结果。
   * 纯正则/字面量规则不经过 IPC，保持原来的同步渲染速度。
   *
   * 失败原因写进 `failures` 而不是直接写 `failedReplaceRules`：这个函数会被
   * `displayContent`（computed）调用，而 computed 里改 ref 会在求值期间触发渲染/重算，
   * 并且会把沙盒那条路径刚写进去的原因冲掉。由调用方决定要不要落库。
   */
  function applyReplaceRules(text: string, failures?: Record<string, string>) {
    if (!text) return ''
    let result = text
    const currentBook = book.value
    if (currentBook?.readConfig?.useReplaceRule === false) return text

    for (const rule of enabledReplaceRules.value) {
      if (isScriptRule(rule)) continue
      const compiled = compileReplaceRule(rule.pattern, rule.isRegex, rule.replacement)
      if (compiled.kind === 'error') {
        // 收集而不是只 console.error：这些规则在导入时看起来是"成功"的, 但执行不了,
        // 必须让用户在规则管理里看到, 否则只会觉得"导入了却没效果"。
        if (failures) failures[rule.name] = compiled.error
        continue
      }
      if (compiled.kind === 'script') continue
      try {
        if (compiled.kind === 'regex') {
          result = result.replace(compiled.regex, compiled.replacement)
        } else {
          result = result.replaceAll(compiled.needle, compiled.replacement)
        }
      } catch (e) {
        if (failures) failures[rule.name] = (e as Error).message
        console.error(`Failed to apply rule: ${rule.name}`, e)
      }
    }
    return result
  }

  /** 按当前档位转换正文。
   *
   *  两档都真正转换(此前「简体」档直接返回原文, 繁体内容因此"无效"):
   *    'traditional' → 简转繁; 'simplified' → 繁转简。
   *  词表与手机端 legado 一致, 所以简体书选「简体」、繁体书选「繁体」时
   *  文本逐字不变（那套词表本身是幂等的）。
   *
   *  'off' 档直接返回原文 —— 转换器不加载也不会跑, 用户因此有一条
   *  「任何字都不想被改写」的退路(issue #12 的原始诉求)。 */
  function convertContent(text: string) {
    if (!text || config.chineseMode === 'off' || !chineseConverters.value) return text
    const convert = config.chineseMode === 'traditional'
      ? chineseConverters.value.traditional
      : chineseConverters.value.simplified
    return convert(text)
  }

  function processContentForDisplay(text: string) {
    // 本地 PDF 的「正文」是逐页占位元素 (页号写在 data-pdf-page 上), 页面内容
    // 由 pdf.js 绘制: 简繁转换与净化替换都无从作用于图片, 却可能改写占位标记
    // 本身, 因此原样返回。
    if (text.includes(PDF_PAGE_ATTR)) return text
    // 与手机端 ContentProcessor 一致: 先简繁转换, 再应用净化替换。
    // 旧顺序会把替换结果再次交给词表 (legado 词表方向相反, 例如「串行 → 序列」
    // 又会被改回「串行」), 表现为替换规则怎么设都无效(issue #12 的另一半)。
    const failures: Record<string, string> = {}
    const output = applyReplaceRules(convertContent(text), failures)
    failedReplaceRules.value = failures
    return output
  }

  /** 简繁转换结果。沙盒路径与同步路径都以它为输入，保证两边处理的是同一段文本。 */
  const convertedContent = computed(() => convertContent(content.value))

  /**
   * 沙盒（含 `@js:` 脚本规则）的处理结果缓存。
   *
   * 只有在「输入文本 + 生效规则集」都没变时才复用；任何一方变化都要重跑，否则改了
   * 规则或翻页后还会显示旧结果。
   */
  const sandboxedContent = ref<{ source: string; rulesKey: string; text: string } | null>(null)
  /**
   * 沙盒失败时记下「输入 + 规则集」，让 `displayContent` 知道当前正文是降级结果。
   *
   * 失败时正文会回落到不含脚本的同步结果 —— 那和「脚本本来就跑过了」在界面上完全
   * 一样，必须能区分，否则用户只会觉得规则没生效。
   */
  const sandboxFailureKey = ref<string | null>(null)
  let sandboxToken = 0

  function replaceRulesKey(rules: ReplaceRule[]) {
    return rules
      .map(rule => `${rule.name}\u0000${rule.pattern}\u0000${rule.replacement}\u0000${rule.isRegex}`)
      .join('\u0001')
  }

  // 脚本规则不能在前端执行，所以单独异步跑一次沙盒；同步路径先给出不含脚本的结果，
  // 等沙盒返回后 `displayContent` 自动切到完整结果。没有脚本规则时这个 watcher 直接返回，
  // 不会产生任何 IPC。
  watch(
    [convertedContent, enabledReplaceRules],
    async ([source, rules]) => {
      const scriptRules = rules.filter(isScriptRule)
      if (!scriptRules.length || book.value?.readConfig?.useReplaceRule === false) {
        sandboxedContent.value = null
        sandboxFailureKey.value = null
        return
      }

      const rulesKey = replaceRulesKey(rules)
      const currentKey = `${source}\u0002${rulesKey}`
      if (
        sandboxedContent.value?.source === source &&
        sandboxedContent.value.rulesKey === rulesKey
      ) {
        return
      }

      // 每轮重算: 上一章/上一套规则的失败不该留到这一章。
      failedReplaceRules.value = {}

      const steps: SandboxReplaceRuleStep[] = []
      for (const rule of rules) {
        const compiled = toSandboxRuleStep(rule.pattern, rule.isRegex, rule.replacement)
        if (!compiled.ok) {
          failedReplaceRules.value[rule.name] = compiled.error
          continue
        }
        steps.push(compiled.step)
      }

      const token = ++sandboxToken
      try {
        const text = await applyReplaceRulesInSandbox({ text: source, rules: steps })
        if (token !== sandboxToken) return
        sandboxedContent.value = { source, rulesKey, text }
        sandboxFailureKey.value = null
      } catch (error) {
        if (token !== sandboxToken) return
        sandboxedContent.value = null
        sandboxFailureKey.value = currentKey
        // 整批失败时无法定位到具体哪条规则，把原因挂到脚本规则名下，让规则管理里能看见。
        const message = (error as Error).message
        for (const rule of scriptRules) {
          failedReplaceRules.value[rule.name] = message
        }
      }
    },
    { immediate: true },
  )

  const displayContent = computed(() => {
    const source = convertedContent.value
    const rules = enabledReplaceRules.value
    if (!rules.some(isScriptRule)) return applyReplaceRules(source)

    const cached = sandboxedContent.value
    if (cached && cached.source === source && cached.rulesKey === replaceRulesKey(rules)) {
      return cached.text
    }
    // 沙盒结果还没到：先显示同步结果（脚本规则暂不生效），到达后自动刷新。
    // 注意这里不能改动任何 ref —— computed 求值期间写入会触发递归更新。
    return applyReplaceRules(source)
  })

  /** 当前正文是否是「脚本规则没跑成」的降级结果（给正文顶部提示用）。 */
  const replaceRulesDegraded = computed(() => {
    const rules = enabledReplaceRules.value
    if (!rules.some(isScriptRule)) return false
    return sandboxFailureKey.value === `${convertedContent.value}\u0002${replaceRulesKey(rules)}`
  })

  /**
   * 降级横幅里要说的「脚本规则未能执行」条数。
   *
   * 只数 `@js:` 规则：`failedReplaceRules` 里还可能有同步路径记录下来的**正则**规则
   * 失败，而横幅的文案是「净化脚本未能执行」，混在一起会多报。
   */
  const failedScriptRuleCount = computed(() =>
    enabledReplaceRules.value.filter(
      rule => isScriptRule(rule) && Boolean(failedReplaceRules.value[rule.name]),
    ).length,
  )

  // 两个档位都要加载转换器: 「简体」档现在也要真的做繁→简(此前它什么都不做)。
  // 「关闭」档不加载 —— 那份词表本身就有 60KB+1MB, 不需要为不转换的档位付费。
  watch(
    () => config.chineseMode,
    () => {
      void ensureChineseConverterLoaded()
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
    // 全量快照已包含最新进度, 轻量副本同步一次即可 —— 否则下次恢复
    // 可能读到比全量更旧的进度(两次调用之间存在插值中的中间值)。
    saveReaderProgress()
  }

  /** 只写进度: 供 handleScroll 等高频路径使用。
   *
   *  必须与 saveReaderSession 分开(见 PersistedReaderProgress 的说明) ——
   *  把整份目录塞进每次滚动事件是本项目最热的一条路径上最贵的操作。
   *  恢复时以本快照为**优先**来源(updatedAt 更新), 全量快照仅作兜底:
   *  章节列表只有全量快照里有, 进度则两者都有, 取新的那个。 */
  function saveReaderProgress() {
    if (!book.value) return
    const payload: PersistedReaderProgress = {
      bookUrl: book.value.bookUrl,
      currentIndex: currentIndex.value,
      chapterScrollProgress: chapterScrollProgress.value,
      updatedAt: Date.now(),
    }
    try {
      localStorage.setItem(READER_PROGRESS_KEY, JSON.stringify(payload))
    } catch {
      // 进度写失败不该中断阅读; 全量快照仍在, 只是可能略旧。
    }
  }

  function readReaderProgress(): PersistedReaderProgress | null {
    try {
      const raw = localStorage.getItem(READER_PROGRESS_KEY)
      if (!raw) return null
      const parsed = JSON.parse(raw) as Partial<PersistedReaderProgress>
      if (typeof parsed?.bookUrl !== 'string' || typeof parsed?.currentIndex !== 'number') return null
      return {
        bookUrl: parsed.bookUrl,
        currentIndex: parsed.currentIndex,
        chapterScrollProgress: typeof parsed.chapterScrollProgress === 'number' ? parsed.chapterScrollProgress : 0,
        updatedAt: typeof parsed.updatedAt === 'number' ? parsed.updatedAt : 0,
      }
    } catch {
      return null
    }
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

    // 进度有两个来源: 全量快照(saveReaderSession, 含目录)与轻量快照
    // (saveReaderProgress, 每次滚动都写)。两者都可能更新, 取 updatedAt 较新
    // 的那个, 并且必须是同一本书 —— 否则打开了 A 书却用 B 书的进度。
    const progress = readReaderProgress()
    const progressMatchesBook = progress?.bookUrl === session.book.bookUrl
    const sessionUpdatedAt = typeof session.updatedAt === 'number' ? session.updatedAt : 0
    const useProgress = Boolean(progressMatchesBook && progress && progress.updatedAt >= sessionUpdatedAt)
    const savedIndex = useProgress && progress ? progress.currentIndex : (session.currentIndex || 0)
    const savedScrollProgress = useProgress && progress
      ? progress.chapterScrollProgress
      : (session.chapterScrollProgress || 0)

    book.value = session.book
    snapshotOpenPosition(session.book)
    chapters.value = session.chapters
    loadReadChapterHistory(session.book)

    const nextIndex = Math.max(0, Math.min(savedIndex, session.chapters.length - 1))
    try {
      const chapterContent = await fetchChapterContent(nextIndex, false, true)
      if (chapterContent == null) return false
      cachePreloadedContent(nextIndex, chapterContent)
      if (config.enablePreload) void preloadAroundChapter(nextIndex)
      currentIndex.value = nextIndex
      const savedProgress = Math.max(0, Math.min(1, savedScrollProgress || 0))
      setActiveChapterState(nextIndex, chapterContent, savedProgress)
      if (book.value) {
        book.value.durChapterTime = session.book.durChapterTime || book.value.durChapterTime
        const shelfBook = shelfStore.books.find((item) => item.bookUrl === book.value?.bookUrl)
        if (shelfBook) shelfBook.durChapterTime = book.value.durChapterTime
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
  /** 在线朗读引擎是否可选：必须有选中的引擎 id；只切 provider 未选引擎时不算配置完成。 */
  const httpSpeechConfigured = computed(() => (
    speechConfig.httpTtsId != null && String(speechConfig.httpTtsId).trim() !== ''
  ))
  /**
   * Edge 朗读是否可用：有音色、且在线即可，不需要任何密钥。
   * 离线时 Edge 必然发不出请求，先在这里拦住，免得用户点朗读后只看到网络错误。
   */
  const edgeSpeechConfigured = computed(() => !!speechConfig.edgeVoice.trim() && appStore.isOnline)
  /**
   * 微软朗读是否可用：有音色且在线即可（演示接口不需要密钥）。
   * 风格是可选的，所以不算进「配置完成」的判断。
   */
  const msSpeechConfigured = computed(() => !!speechConfig.msVoice.trim() && appStore.isOnline)
  const speechProviderLabel = computed(() => {
    if (speechConfig.provider === 'openai') return 'API 语音'
    if (speechConfig.provider === 'http') return '在线朗读引擎'
    if (speechConfig.provider === 'edge') return 'Edge 朗读'
    if (speechConfig.provider === 'msdemo') return '微软朗读'
    return '系统语音'
  })
  const speechStopAt = ref(0)
  let speechStopTimer: number | null = null
  let synth: SpeechSynthesis | null = typeof window !== 'undefined' ? window.speechSynthesis : null
  let currentUtterance: SpeechSynthesisUtterance | null = null
  // 下面三个名字带 OpenAI 的播放器状态由 openai / http 两个分支共用：
  // 两者的音频都是一整段 Blob + HTMLAudioElement，各自持有会互相踩清理逻辑。
  let currentOpenAIAudio: HTMLAudioElement | null = null
  let currentOpenAIAudioUrl = ''
  let currentOpenAIAbortController: AbortController | null = null
  const preloadedOpenAIAudio = ref<PreloadedOpenAIAudio[]>([])
  let preloadGeneration = 0
  const inFlightPreloadKeys = new Set<string>()
  const inFlightOpenAIAudioRequests = new Map<string, Promise<Blob>>()
  let currentTTSSessionId = 0
  /**
   * 缓冲流的外部控制句柄。
   *
   * 自动朗读的 http/openai 通道由 useReaderAutoPlayback 里的 SpeechStream 播放
   * （它自己持有 <audio>），但暂停/停止按钮调的是 store.pauseTTS / store.stopTTS。
   * 这里保存一个句柄让这两个入口把操作转给流，避免两套播放器并存。
   */
  let speechStreamControls: SpeechStreamControls | null = null

  /**
   * 「朗读被停止」的监听器。
   *
   * 停止入口不止一个：听书面板的停止按钮、快捷键、定时停止、关闭面板、卸载页面。
   * 自动朗读需要在这些入口都取消「跳章后从头续读」的挂起任务，否则用户按了停止，
   * 几秒后正文加载完又会自己开始朗读。挂在 store 的停止路径上是唯一不漏的做法。
   */
  const speechStopListeners = new Set<() => void>()

  function addSpeechStopListener(listener: () => void) {
    speechStopListeners.add(listener)
    return () => {
      speechStopListeners.delete(listener)
    }
  }

  function notifySpeechStopped() {
    for (const listener of speechStopListeners) {
      try {
        listener()
      } catch {
        // 监听器只做清理，抛错不该反过来影响停止朗读。
      }
    }
  }

  function attachSpeechStreamControls(controls: SpeechStreamControls | null) {
    speechStreamControls = controls
  }

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

  async function syncCurrentBookProgressToLegado(
    allowUpload = true,
    contentOverride?: string,
    expectedBookUrl?: string,
    expectedBookLoadRequestId?: number,
    applyRemoteProgress = true,
  ) {
    if (!book.value) return null
    if (loading.value && !restoringLegadoProgress.value) return null
    if (expectedBookUrl && book.value.bookUrl !== expectedBookUrl) return null
    if (expectedBookLoadRequestId != null && expectedBookLoadRequestId !== bookLoadRequestId) return null
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
      durChapterTime: Date.now(),
      durChapterTitle: currentChapter.value?.title || book.value.durChapterTitle,
    }
    const result = await syncLegadoBookProgress(webdav, progress, allowUpload)
    if (expectedBookUrl && book.value?.bookUrl !== expectedBookUrl) return result
    if (expectedBookLoadRequestId != null && expectedBookLoadRequestId !== bookLoadRequestId) return result
    if (!result?.remote) return result

    const remote = result.remote
    // 与手机端保持一致：只按章节索引和章节内字符位置比较，
    // 不用电脑打开章节时自动更新的 durChapterTime 覆盖真实阅读进度。
    const remoteIsNewer = remote.durChapterIndex > progress.durChapterIndex
      || (remote.durChapterIndex === progress.durChapterIndex
        && remote.durChapterPos > progress.durChapterPos)
    if (!remoteIsNewer) return result
    if (!applyRemoteProgress) return result

    // 手机端直接使用 durChapterIndex。标题只用于显示，不能参与定位，
    // 否则章节标题重复或书源标题变化时会跳到错误章节。
    const targetIndex = remote.durChapterIndex
    if (targetIndex < 0 || targetIndex >= chapters.value.length) return result
    const title = chapters.value[targetIndex]?.title || remote.durChapterTitle || book.value.durChapterTitle
    const remoteProgress = targetIndex === currentIndex.value
      ? Math.max(0, Math.min(1, remote.durChapterPos / plainContentLength))
      : 0
    const encodedRemoteProgress = encodeServerProgress(remoteProgress)
    pendingLegadoProgress.value = { index: targetIndex, position: remote.durChapterPos }
    legadoPositionRestore.value = {
      index: targetIndex,
      position: Math.max(0, remote.durChapterPos),
      progress: remoteProgress,
      token: ++legadoPositionRestoreToken,
      ready: false,
    }
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
    // 跨章时正文还没有加载，先保存章节索引；目标章节正文加载后再按字符位置恢复。
    snapshotOpenPosition({
      ...book.value,
      durChapterPos: encodedRemoteProgress,
    })
    saveReaderSession()
    appStore.showToast('已读取手机端阅读进度', 'success')
    return result
  }

  async function uploadCurrentBookProgressToLegado() {
    if (!book.value || loading.value) return null
    if (!appStore.legadoSyncEnabled) return null
    const webdav = appStore.legadoWebdavConfig
    if (!webdav.url || !webdav.account || !webdav.password) return null
    const plainContentLength = Math.max(1, content.value.replace(/<[^>]+>/g, '').length)
    const progressPosition = Math.max(0, Math.min(plainContentLength,
      Math.round(chapterScrollProgress.value * plainContentLength)))
    const progress: LegadoBookProgress = {
      name: book.value.name,
      author: book.value.author,
      durChapterIndex: currentIndex.value,
      durChapterPos: progressPosition,
      durChapterTime: Date.now(),
      durChapterTitle: currentChapter.value?.title || book.value.durChapterTitle,
    }
    return uploadLegadoBookProgress(webdav, progress)
  }

  async function restoreCurrentBookProgressFromLegado(
    contentOverride?: string,
    expectedBookUrl?: string,
    expectedBookLoadRequestId?: number,
  ) {
    restoringLegadoProgress.value = true
    try {
      const result = await syncCurrentBookProgressToLegado(
        true,
        contentOverride,
        expectedBookUrl,
        expectedBookLoadRequestId,
      )
      const isExpectedBook = (!expectedBookUrl || book.value?.bookUrl === expectedBookUrl)
        && (expectedBookLoadRequestId == null || expectedBookLoadRequestId === bookLoadRequestId)
      if (isExpectedBook && pendingLegadoProgress.value) {
        await loadChapter(currentIndex.value)
      }
      return result
    } finally {
      restoringLegadoProgress.value = false
    }
  }

  /**
   * 离开阅读页时按手机端 uploadBookProgress 的规则执行：
   * 直接上传当前进度，不读取、不比较远端进度。
   */
  async function syncCurrentBookProgressFromLegadoOnExit() {
    if (!book.value || restoringLegadoProgress.value || loading.value) return null
    return uploadCurrentBookProgressToLegado()
  }

  function clearLegadoPositionRestore(token?: number) {
    if (token != null && legadoPositionRestore.value?.token !== token) return
    if (legadoPositionRestore.value) {
      legadoPositionRestoreCompleted.value = {
        index: legadoPositionRestore.value.index,
        token: legadoPositionRestore.value.token,
      }
    }
    legadoPositionRestore.value = null
  }

  function consumeLegadoPositionRestoreCompleted(index: number) {
    if (legadoPositionRestoreCompleted.value?.index !== index) return false
    legadoPositionRestoreCompleted.value = null
    return true
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

  /**
   * 选择在线朗读引擎。
   *
   * contentType 与 id 一起存：引擎换掉后旧的 MIME 兜底值就错了，必须跟着换。
   */
  function setHttpTtsId(id: number | string | null, contentType = '') {
    speechConfig.httpTtsId = id
    speechConfig.httpTtsContentType = id == null ? '' : normalizeAudioContentType(contentType)
    saveSpeechConfig()
  }

  /**
   * 旁白/对话分音色开关。
   *
   * 关闭时**不清空**已选的对话音色：用户往往是在「先关掉对比一下再打开」，
   * 每次都要重选一遍会很难用。
   */
  function setDualVoiceEnabled(enabled: boolean) {
    speechConfig.dualVoiceEnabled = enabled
    // 预载队列里是默认音色的音频：开关状态一变它们的用途就不成立了，直接丢弃。
    clearPreloadedOpenAIAudio()
    saveSpeechConfig()
  }

  function setDialogueVoiceName(name: string) {
    speechConfig.dialogueVoiceName = name
    saveSpeechConfig()
  }

  function setDialogueOpenaiVoice(voice: string) {
    speechConfig.dialogueOpenaiVoice = voice
    clearPreloadedOpenAIAudio()
    saveSpeechConfig()
  }

  /** 对话引擎。contentType 与 id 一起存，理由同 setHttpTtsId。 */
  function setDialogueHttpTtsId(id: number | string | null, contentType = '') {
    speechConfig.dialogueHttpTtsId = id
    speechConfig.dialogueHttpTtsContentType = id == null ? '' : normalizeAudioContentType(contentType)
    saveSpeechConfig()
  }

  /** 缓冲字符数：调大更抗网络抖动，调小更省请求。只影响自动朗读的缓冲流。 */
  function setBufferChars(chars: number) {
    speechConfig.bufferChars = normalizeBufferChars(chars)
    saveSpeechConfig()
  }

  /** Edge 旁白音色。空值回默认音色，避免配置成空后合成请求必然失败。 */
  function setEdgeVoice(voice: string) {
    speechConfig.edgeVoice = normalizeEdgeVoice(voice)
    saveSpeechConfig()
  }

  /** Edge 对话音色；空串表示跟随旁白音色（双音色开关关闭时保留）。 */
  function setEdgeDialogueVoice(voice: string) {
    speechConfig.edgeDialogueVoice = normalizeEdgeDialogueVoice(voice)
    saveSpeechConfig()
  }

  /** Edge 音调偏移（Hz），夹到 -100..100。 */
  function setEdgePitch(pitchHz: number) {
    speechConfig.edgePitch = normalizeEdgeSignedLevel(pitchHz)
    saveSpeechConfig()
  }

  /** Edge 音量偏移（%），夹到 -100..100。 */
  function setEdgeVolume(volumePercent: number) {
    speechConfig.edgeVolume = normalizeEdgeSignedLevel(volumePercent)
    saveSpeechConfig()
  }

  /** Edge 音质档位；只接受 48k / 96k，其它值回 48k。 */
  function setEdgeFormat(format: EdgeTtsFormat) {
    speechConfig.edgeFormat = normalizeEdgeFormat(format)
    saveSpeechConfig()
  }

  /* ─── 微软朗读 setters ─── */
  function setMsVoice(voice: string) {
    speechConfig.msVoice = normalizeMsVoice(voice)
    saveSpeechConfig()
  }

  function setMsDialogueVoice(voice: string) {
    speechConfig.msDialogueVoice = normalizeMsDialogueVoice(voice)
    saveSpeechConfig()
  }

  /** 旁白风格；空串表示无风格。 */
  function setMsStyle(style: string) {
    speechConfig.msStyle = normalizeMsStyle(style)
    saveSpeechConfig()
  }

  /** 对话风格；空串表示跟随旁白风格。 */
  function setMsDialogueStyle(style: string) {
    speechConfig.msDialogueStyle = normalizeMsStyle(style)
    saveSpeechConfig()
  }

  /** 风格强度 0.5–2.0，保留一位小数。 */
  function setMsStyleDegree(degree: number) {
    speechConfig.msStyleDegree = normalizeMsStyleDegree(degree)
    saveSpeechConfig()
  }

  /** 音调偏移（%），夹到 -50..50。 */
  function setMsPitch(pitch: number) {
    speechConfig.msPitch = normalizeMsLevel(pitch, MS_PITCH_MIN, MS_PITCH_MAX)
    saveSpeechConfig()
  }

  /** 音量偏移（%），夹到 -100..100。 */
  function setMsVolume(volume: number) {
    speechConfig.msVolume = normalizeMsLevel(volume, MS_VOLUME_MIN, MS_VOLUME_MAX)
    saveSpeechConfig()
  }

  /** 音质档位；只接受 48k / 96k / 192k。 */
  function setMsFormat(format: MsTtsFormat) {
    speechConfig.msFormat = normalizeMsFormat(format)
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

  /**
   * 合成一段文本的 API 语音 Blob。
   *
   * `voiceOverride` 供双音色路径逐段指定旁白/对话音色；缓存与预载仍只服务
   * 单音色的默认 `openaiVoice`（缓存 key 里没有音色维度，混用会串音色）。
   */
  async function fetchOpenAIAudioBlob(rawText: string, signal?: AbortSignal, voiceOverride?: string) {
    const voice = voiceOverride || speechConfig.openaiVoice
    const request = {
      apiFormat: speechConfig.apiFormat,
      baseUrl: speechConfig.openaiBaseUrl,
      proxyUrl: speechConfig.speechProxyUrl || undefined,
      apiKey: speechConfig.openaiApiKey || undefined,
      input: rawText.slice(0, 4096),
      model: speechConfig.openaiModel,
      voice,
      format: speechConfig.openaiFormat,
      speed: speechConfig.speechRate,
      signal,
    }
    if (!isTauriRuntime()) {
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
    // 双音色逐段合成不走单音色预载：缓存 key 里没有音色维度，且逐段播放要用
    // 各自 voice 重新合成，预载的默认音色音频根本不会被消费 —— 只会白发请求。
    if (speechConfig.dualVoiceEnabled) return
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

  /**
   * 播放一段已合成好的音频（openai / http 共用）。
   *
   * 两条分支拿到的都是完整音频 Blob（一个是 API 返回，一个是后端 IPC 返回），
   * 播放层完全一致：HTMLAudioElement + object URL。抽成一份而不是各写一遍，
   * 因为 onplay/onpause/onended/onerror、进度上报、session 校验、object URL
   * 回收任何一处漏改，都会表现为「第一段能播、后面没声」或内存泄漏。
   */
  function playTTSAudioBlob(
    blob: Blob,
    options: TTSOptions,
    sessionId: number,
    context: { logTag: string; errorMessage: string; textPreview?: string },
  ): Promise<void> {
    if (!isCurrentTTSSession(sessionId)) return Promise.resolve()
    isSpeechLoading.value = false
    currentOpenAIAudioUrl = URL.createObjectURL(blob)
    const audio = new Audio(currentOpenAIAudioUrl)
    currentOpenAIAudio = audio
    currentOpenAIAbortController = null
    const textPreview = context.textPreview || ''

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
      logTTS(`${context.logTag} onplay`, { sessionId, text: textPreview })
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
      logTTS(`${context.logTag} onended`, { sessionId, text: textPreview })
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
      const error = new Error(context.errorMessage)
      logTTS(`${context.logTag} onerror`, { sessionId, text: textPreview })
      options.onError?.(error)
    }

    return audio.play().catch((error: Error) => {
      if (!isCurrentTTSSession(sessionId)) return
      isSpeechLoading.value = false
      isSpeaking.value = false
      isPaused.value = false
      currentOpenAIAudio = null
      logTTS(`${context.logTag} play catch`, { sessionId, message: error.message, text: textPreview })
      options.onError?.(error)
    })
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
    // 播放器逻辑（含进度/回调/清理）与 http 分支共用，见 playTTSAudioBlob。
    const playBlob = (blob: Blob, controller: AbortController) => {
      if (controller.signal.aborted) return
      return playTTSAudioBlob(blob, options, sessionId, {
        logTag: 'openai',
        errorMessage: 'API 语音音频播放失败',
        textPreview: rawText.slice(0, 40),
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

  /** 在线朗读是否可用；不可用时给出一次明确提示并回调 onError。 */
  function requireHttpSpeechConfigured(options: TTSOptions): boolean {
    if (httpSpeechConfigured.value) return true
    const error = new Error('请先在「朗读引擎」里选择在线朗读引擎')
    isSpeechLoading.value = false
    appStore.showToast(error.message, 'warning')
    options.onError?.(error)
    return false
  }

  /** http 通道统一失败处理：session 已过期时静默丢弃，避免覆盖新一次朗读。 */
  function failHttpTts(error: unknown, sessionId: number, options: TTSOptions, textPreview: string, logTag: string) {
    if (!isCurrentTTSSession(sessionId)) return
    isSpeechLoading.value = false
    isSpeaking.value = false
    isPaused.value = false
    currentOpenAIAudio = null
    const message = (error as Error)?.message || '在线朗读请求失败'
    logTTS(`${logTag} request catch`, { sessionId, message, text: textPreview.slice(0, 40) })
    appStore.showToast(message, 'error')
    options.onError?.(error as Error)
  }

  /**
   * 在线朗读引擎（httpTTS）单段分支。
   *
   * 自动朗读不再走这里（缓冲流逐块合成，见 useReaderAutoPlayback），但手动
   * `startTTS(text)` / 系统外的直接调用仍以「一段文本 = 一次请求」工作。
   */
  async function startHttpTts(rawText: string, options: TTSOptions, sessionId: number) {
    if (!requireHttpSpeechConfigured(options)) return

    isSpeechLoading.value = true
    speechProgress.value = 0
    options.onProgress?.(0)
    logTTS('http speak queued', {
      sessionId,
      ttsId: speechConfig.httpTtsId,
      text: rawText.slice(0, 80),
    })

    try {
      const blob = await synthesizeHttpTts({
        ttsId: speechConfig.httpTtsId as number | string,
        text: rawText,
        speed: speechConfig.speechRate,
        contentType: speechConfig.httpTtsContentType,
      })
      if (!isCurrentTTSSession(sessionId)) return
      await playTTSAudioBlob(blob, options, sessionId, {
        logTag: 'http',
        errorMessage: '在线朗读音频播放失败',
        textPreview: rawText.slice(0, 40),
      })
    } catch (error) {
      failHttpTts(error, sessionId, options, rawText, 'http')
    }
  }

  /**
   * 双音色 · 系统语音：每片段一条 utterance，连续 speak() 交给浏览器队列串行。
   *
   * 刻意不在 onend 里 speak 下一条：那等于自己维护第二套状态机，还要处理
   * Safari 的 onend 时序/暂停恢复；浏览器的语音队列本来就保证串行。
   */
  function startSystemDualTTS(segments: SpeechSegment[], options: TTSOptions, sessionId: number) {
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
    speechProgress.value = 0
    options.onProgress?.(0)
    currentUtterance = null

    const narrationVoice = voiceList.value.find((voice) => voice.name === speechConfig.voiceName)
    // 对话音色留空时跟随旁白：开关打开但还没选第二音色是常见中间态。
    const dialogueVoice = voiceList.value.find((voice) => voice.name === speechConfig.dialogueVoiceName) || narrationVoice
    let started = false
    let failed = false

    segments.forEach((segment, index) => {
      const utterance = new SpeechSynthesisUtterance(segment.text)
      const voice = segment.type === 'dialogue' ? dialogueVoice : narrationVoice
      utterance.lang = voice?.lang || 'zh-CN'
      utterance.voice = voice || null
      utterance.rate = speechConfig.speechRate
      utterance.pitch = speechConfig.speechPitch

      utterance.onstart = () => {
        if (!isCurrentTTSSession(sessionId)) return
        currentUtterance = utterance
        isSpeaking.value = true
        isPaused.value = false
        if (!started) {
          started = true
          options.onStart?.()
        }
      }
      utterance.onboundary = (event) => {
        if (!isCurrentTTSSession(sessionId)) return
        const charIndex = Number.isFinite(event.charIndex) ? event.charIndex : 0
        const part = Math.max(0, Math.min(1, charIndex / Math.max(1, segment.text.length)))
        const progress = (index + part) / segments.length
        speechProgress.value = progress
        options.onProgress?.(progress)
      }
      utterance.onend = () => {
        if (currentUtterance === utterance) currentUtterance = null
        if (!isCurrentTTSSession(sessionId)) return
        if (index !== segments.length - 1) return
        speechProgress.value = 1
        options.onProgress?.(1)
        isSpeaking.value = false
        isPaused.value = false
        logTTS('system dual onend', { sessionId, segments: segments.length })
        options.onEnd?.()
      }
      utterance.onerror = (event) => {
        // stopTTS 走 cancel()，会以 interrupted/canceled 触发 onerror —— 那不是
        // 「播放失败」，不能把它回调给自动朗读（会误清高亮/状态）。
        if (event?.error === 'interrupted' || event?.error === 'canceled') return
        if (failed || !isCurrentTTSSession(sessionId)) return
        failed = true
        isSpeaking.value = false
        isPaused.value = false
        options.onError?.(event)
      }
      synth?.speak(utterance)
    })
  }

  /**
   * 双音色 · API 语音：逐段用各自的 voice 合成，再串行播放。
   *
   * 自动朗读的 http/openai 通道已改走 useReaderAutoPlayback 的缓冲流，这里保留
   * 给直接 `startTTS(text)` 的调用方。不走预载缓存：缓存 key 里没有音色维度
   * （见 buildOpenAIAudioCacheKey），双音色复用缓存会串音色。
   */
  async function startOpenAIDualTTS(segments: SpeechSegment[], options: TTSOptions, sessionId: number) {
    if (!openAISpeechConfigured.value) {
      const error = new Error('请先配置 API 语音服务')
      isSpeechLoading.value = false
      appStore.showToast(error.message, 'warning')
      options.onError?.(error)
      return
    }
    isSpeechLoading.value = true
    speechProgress.value = 0
    options.onProgress?.(0)
    const controller = new AbortController()
    currentOpenAIAbortController = controller
    const narrationVoice = speechConfig.openaiVoice
    const dialogueVoice = speechConfig.dialogueOpenaiVoice.trim() || narrationVoice
    let index = 0

    const playNext = () => {
      if (!isCurrentTTSSession(sessionId)) return
      if (index >= segments.length) {
        options.onEnd?.()
        return
      }
      const currentIndex = index
      const segment = segments[currentIndex]
      index += 1
      const isLast = index >= segments.length
      isSpeechLoading.value = true
      void fetchOpenAIAudioBlob(
        segment.text,
        controller.signal,
        segment.type === 'dialogue' ? dialogueVoice : narrationVoice,
      ).then((blob) => {
        if (!isCurrentTTSSession(sessionId)) return
        void playTTSAudioBlob(blob, {
          onStart: currentIndex === 0 ? () => options.onStart?.() : undefined,
          onProgress: (progress) => {
            const part = Math.max(0, Math.min(1, progress))
            const overall = (currentIndex + part) / segments.length
            speechProgress.value = overall
            options.onProgress?.(overall)
          },
          onEnd: () => {
            if (isLast) {
              options.onEnd?.()
              return
            }
            window.setTimeout(() => playNext(), 0)
          },
          onError: (error) => options.onError?.(error),
        }, sessionId, {
          logTag: 'openai-dual',
          errorMessage: 'API 语音音频播放失败',
          textPreview: segment.text.slice(0, 40),
        })
      }).catch((error) => {
        if (controller.signal.aborted || !isCurrentTTSSession(sessionId)) return
        isSpeechLoading.value = false
        isSpeaking.value = false
        isPaused.value = false
        currentOpenAIAbortController = null
        currentOpenAIAudio = null
        const message = (error as Error)?.message || 'API 语音请求失败'
        logTTS('openai dual request catch', { sessionId, message })
        appStore.showToast(message, 'error')
        options.onError?.(error as Error)
      })
    }

    playNext()
  }

  /**
   * 缓冲流用的单块合成：按 provider + 片段类型选引擎/音色。
   *
   * 这是「配置」与「播放」的分界：SpeechStream 只关心拿到 Blob，音色/引擎怎么
   * 选（旁白 vs 对话、http 引擎 id、openai voice）都属于 store 的知识。
   *
   * @param type `'dialogue'` 只在双音色开启时由调用方产生；关闭时一律 narration。
   * @param signal 停止朗读时中止在途请求。Tauri IPC 无法真正取消，因此返回前
   *               会再查一次 signal，保证停止后不会把音频交给播放器。
   */
  async function synthesizeSpeechChunkBlob(
    text: string,
    type: SpeechSegmentType,
    signal?: AbortSignal,
  ): Promise<Blob> {
    const rawText = text.trim()
    if (!rawText) throw new Error('朗读文本为空')
    if (signal?.aborted) throw new DOMException('The operation was aborted', 'AbortError')
    const useDialogue = type === 'dialogue' && speechConfig.dualVoiceEnabled

    if (speechConfig.provider === 'http') {
      const ttsId = (useDialogue ? speechConfig.dialogueHttpTtsId : null) ?? speechConfig.httpTtsId
      if (ttsId == null) {
        // 只抛错不弹 toast：调用方（缓冲流的 onError）统一提示，避免一次失败弹两条。
        throw new Error('请先在「朗读引擎」里选择在线朗读引擎')
      }
      // 对话引擎未选时退回旁白引擎：等同于单音色，而不是让整段朗读失败。
      const contentType = useDialogue
        ? speechConfig.dialogueHttpTtsContentType || speechConfig.httpTtsContentType
        : speechConfig.httpTtsContentType
      const blob = await synthesizeHttpTts({
        ttsId,
        text: rawText,
        speed: speechConfig.speechRate,
        contentType,
      })
      if (signal?.aborted) throw new DOMException('The operation was aborted', 'AbortError')
      return blob
    }

    if (speechConfig.provider === 'edge') {
      // 对话音色留空时跟随旁白音色（双音色开关打开但还没选第二个音色是常见中间态）。
      const voice = useDialogue
        ? speechConfig.edgeDialogueVoice.trim() || speechConfig.edgeVoice
        : speechConfig.edgeVoice
      if (!voice) throw new Error('请先选择 Edge 朗读音色')
      const blob = await synthesizeEdgeTts({
        text: rawText,
        voice,
        rate: formatEdgeRate(speechConfig.speechRate),
        pitch: formatEdgePitch(speechConfig.edgePitch),
        volume: formatEdgeVolume(speechConfig.edgeVolume),
        format: speechConfig.edgeFormat,
      })
      if (signal?.aborted) throw new DOMException('The operation was aborted', 'AbortError')
      return blob
    }

    if (speechConfig.provider === 'msdemo') {
      // 风格与音色同规则：对话优先用对话配置，留空跟随旁白。
      const voice = useDialogue
        ? speechConfig.msDialogueVoice.trim() || speechConfig.msVoice
        : speechConfig.msVoice
      if (!voice) throw new Error('请先选择微软朗读音色')
      const style = useDialogue
        ? speechConfig.msDialogueStyle.trim() || speechConfig.msStyle
        : speechConfig.msStyle
      const blob = await synthesizeMsTts({
        text: rawText,
        voice,
        style: style || undefined,
        styleDegree: style ? speechConfig.msStyleDegree : undefined,
        // 这个通道的 rate/pitch/volume 都是带符号百分比串（pitch 不是 Hz）。
        rate: formatEdgeRate(speechConfig.speechRate),
        pitch: formatMsPercent(speechConfig.msPitch),
        volume: formatMsPercent(speechConfig.msVolume),
        format: speechConfig.msFormat,
      })
      if (signal?.aborted) throw new DOMException('The operation was aborted', 'AbortError')
      return blob
    }

    if (!openAISpeechConfigured.value) {
      throw new Error('请先配置 API 语音服务')
    }
    const voice = useDialogue
      ? speechConfig.dialogueOpenaiVoice.trim() || speechConfig.openaiVoice
      : speechConfig.openaiVoice
    return fetchOpenAIAudioBlob(rawText, signal, voice)
  }

  /**
   * Edge 朗读的单次分支（自动朗读由缓冲流逐块合成，不走这里）。
   *
   * 直接调用 `startTTS(text)` 时按整段请求；双音色开启且确实切出多段时逐段
   * 合成/播放（每段自己的音色），保证这条兜底路径与自动朗读的听感一致。
   */
  function startEdgeTts(rawText: string, options: TTSOptions, sessionId: number) {
    if (!edgeSpeechConfigured.value) {
      const error = new Error(appStore.isOnline ? '请先选择 Edge 朗读音色' : '当前离线，无法使用 Edge 朗读')
      isSpeechLoading.value = false
      appStore.showToast(error.message, 'warning')
      options.onError?.(error)
      return
    }
    const split = speechConfig.dualVoiceEnabled ? splitSpeechSegments(rawText) : []
    const plan = split.length > 1 || (split.length === 1 && split[0].type === 'dialogue')
      ? split
      : [{ text: rawText, type: 'narration' as SpeechSegmentType }]
    let index = 0
    const playNext = () => {
      if (!isCurrentTTSSession(sessionId)) return
      if (index >= plan.length) {
        options.onEnd?.()
        return
      }
      const currentIndex = index
      const segment = plan[currentIndex]
      index += 1
      const isLast = index >= plan.length
      const voice = segment.type === 'dialogue' && speechConfig.dualVoiceEnabled
        ? speechConfig.edgeDialogueVoice.trim() || speechConfig.edgeVoice
        : speechConfig.edgeVoice
      isSpeechLoading.value = true
      void synthesizeEdgeTts({
        text: segment.text,
        voice,
        rate: formatEdgeRate(speechConfig.speechRate),
        pitch: formatEdgePitch(speechConfig.edgePitch),
        volume: formatEdgeVolume(speechConfig.edgeVolume),
        format: speechConfig.edgeFormat,
      })
        .then((blob) => {
          if (!isCurrentTTSSession(sessionId)) return
          void playTTSAudioBlob(blob, {
            onStart: currentIndex === 0 ? () => options.onStart?.() : undefined,
            onEnd: () => {
              if (isLast) {
                options.onEnd?.()
                return
              }
              playNext()
            },
            onError: (error) => options.onError?.(error),
          }, sessionId, {
            logTag: 'edge',
            errorMessage: 'Edge 朗读音频播放失败',
            textPreview: segment.text.slice(0, 40),
          })
        })
        .catch((error) => {
          if (!isCurrentTTSSession(sessionId)) return
          isSpeechLoading.value = false
          isSpeaking.value = false
          isPaused.value = false
          currentOpenAIAudio = null
          const message = (error as Error)?.message || 'Edge 朗读请求失败'
          logTTS('edge request catch', { sessionId, message })
          appStore.showToast(message, 'error')
          options.onError?.(error as Error)
        })
    }
    playNext()
  }

  /**
   * 微软朗读的单次分支（自动朗读由缓冲流逐块合成，不走这里）。
   *
   * 与 Edge 的单次分支同构：直接 `startTTS(text)` 时按整段请求；双音色开启且
   * 切出多段时逐段合成/播放（音色与风格都按旁白/对话规则选择）。
   */
  function startMsTts(rawText: string, options: TTSOptions, sessionId: number) {
    if (!msSpeechConfigured.value) {
      const error = new Error(appStore.isOnline ? '请先选择微软朗读音色' : '当前离线，无法使用微软朗读')
      isSpeechLoading.value = false
      appStore.showToast(error.message, 'warning')
      options.onError?.(error)
      return
    }
    const split = speechConfig.dualVoiceEnabled ? splitSpeechSegments(rawText) : []
    const plan = split.length > 1 || (split.length === 1 && split[0].type === 'dialogue')
      ? split
      : [{ text: rawText, type: 'narration' as SpeechSegmentType }]
    let index = 0
    const playNext = () => {
      if (!isCurrentTTSSession(sessionId)) return
      if (index >= plan.length) {
        options.onEnd?.()
        return
      }
      const currentIndex = index
      const segment = plan[currentIndex]
      index += 1
      const isLast = index >= plan.length
      const dialogue = segment.type === 'dialogue' && speechConfig.dualVoiceEnabled
      const voice = dialogue
        ? speechConfig.msDialogueVoice.trim() || speechConfig.msVoice
        : speechConfig.msVoice
      const style = dialogue
        ? speechConfig.msDialogueStyle.trim() || speechConfig.msStyle
        : speechConfig.msStyle
      isSpeechLoading.value = true
      void synthesizeMsTts({
        text: segment.text,
        voice,
        style: style || undefined,
        styleDegree: style ? speechConfig.msStyleDegree : undefined,
        rate: formatEdgeRate(speechConfig.speechRate),
        pitch: formatMsPercent(speechConfig.msPitch),
        volume: formatMsPercent(speechConfig.msVolume),
        format: speechConfig.msFormat,
      })
        .then((blob) => {
          if (!isCurrentTTSSession(sessionId)) return
          void playTTSAudioBlob(blob, {
            onStart: currentIndex === 0 ? () => options.onStart?.() : undefined,
            onEnd: () => {
              if (isLast) {
                options.onEnd?.()
                return
              }
              playNext()
            },
            onError: (error) => options.onError?.(error),
          }, sessionId, {
            logTag: 'msdemo',
            errorMessage: '微软朗读音频播放失败',
            textPreview: segment.text.slice(0, 40),
          })
        })
        .catch((error) => {
          if (!isCurrentTTSSession(sessionId)) return
          isSpeechLoading.value = false
          isSpeaking.value = false
          isPaused.value = false
          currentOpenAIAudio = null
          const message = (error as Error)?.message || '微软朗读请求失败'
          logTTS('msdemo request catch', { sessionId, message })
          appStore.showToast(message, 'error')
          options.onError?.(error as Error)
        })
    }
    playNext()
  }

  function startTTS(text?: string, options: TTSOptions = {}, interruptCurrent = true) {
    const hasActiveSystemSpeech = !!synth && (synth.speaking || synth.pending || !!currentUtterance)
    const hasActiveOpenAISpeech = !!currentOpenAIAudio || !!currentOpenAIAbortController

    // 缓冲流自己持有 <audio>：中断时也要把它停掉，否则新朗读会和旧流叠着播。
    if (interruptCurrent && (hasActiveSystemSpeech || hasActiveOpenAISpeech || speechStreamControls !== null || isSpeaking.value || isSpeechLoading.value)) {
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

    // 双音色：只有「开关打开 + 确实需要换音色」才走新路径。
    // - 多片段：旁白与对话交替，必须走；
    // - 单片段但整段都是对话（`“你来了。”` 这种一句对白一段的排版极常见）：
    //   也必须用对话音色，只按「片段数 > 1」判断会让这些段落全用旁白念。
    // - 单片段且是旁白：继续走原来的单音色逻辑，行为逐字节不变。
    // 注意：自动朗读的 http/openai 已在 useReaderAutoPlayback 的缓冲流里逐块按
    // 类型合成，不走这里；这里只有 system 与直接调用方会用。
    const dualSegments = speechConfig.dualVoiceEnabled ? splitSpeechSegments(rawText) : []
    const useDualVoice = dualSegments.length > 1
      || (dualSegments.length === 1 && dualSegments[0].type === 'dialogue')
    if (useDualVoice) {
      if (speechConfig.provider === 'openai') {
        void startOpenAIDualTTS(dualSegments, options, sessionId)
        return
      }
      if (speechConfig.provider === 'system') {
        startSystemDualTTS(dualSegments, options, sessionId)
        return
      }
      // http 的双音色由缓冲流负责；直接调用时退化为「整段用旁白引擎」。
    }

    if (speechConfig.provider === 'openai') {
      void startOpenAITTS(rawText, options, sessionId)
      return
    }

    if (speechConfig.provider === 'http') {
      void startHttpTts(rawText, options, sessionId)
      return
    }

    if (speechConfig.provider === 'edge') {
      startEdgeTts(rawText, options, sessionId)
      return
    }

    if (speechConfig.provider === 'msdemo') {
      startMsTts(rawText, options, sessionId)
      return
    }

    startSystemTTS(rawText, options, sessionId)
  }

  function pauseTTS() {
    // 缓冲流自己持有 <audio>：暂停/恢复必须转给它，否则按钮看起来没反应。
    if (speechStreamControls) {
      if (isPaused.value) {
        speechStreamControls.resume()
        isPaused.value = false
      } else {
        speechStreamControls.pause()
        isPaused.value = true
        isSpeaking.value = true
      }
      return
    }
    // http / openai / edge / msdemo 都是 HTMLAudioElement，暂停/恢复逻辑完全一致。
    if (speechConfig.provider === 'openai' || speechConfig.provider === 'http' || speechConfig.provider === 'edge' || speechConfig.provider === 'msdemo') {
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
    // 先停缓冲流并摘掉句柄，避免它的 onIdle/onError 回调再进 stopTTS 造成重入。
    const streamControls = speechStreamControls
    speechStreamControls = null
    streamControls?.stop()
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
      // 真正的「用户停止」才会走到这里：内部重启用的是 stopTTS(false)，
      // 不能通知，否则会把刚要续读的跳章跟随一起取消。
      notifySpeechStopped()
    }
  }

  /* ─── 语音试听（设置页「语音测试」） ───
   *
   * 试听必须与书内朗读完全隔离：
   * - 用的不是 isSpeaking / speechProgress 那套状态，而是独立的 refs，
   *   否则试听会让听书面板/进度条误以为正在朗读；
   * - 试听前先 stopTTS()，避免两条声音叠在一起；
   * - 独立 <audio> / utterance + 独立 URL，停止时自己回收，不碰主播放器的清理逻辑。
   */
  const isSpeechPreviewing = ref(false)
  const isSpeechPreviewLoading = ref(false)
  let previewSessionId = 0
  let previewUtterance: SpeechSynthesisUtterance | null = null
  let previewAudio: HTMLAudioElement | null = null
  let previewAudioUrl = ''
  let previewAbortController: AbortController | null = null

  function hasActiveSpeechPreview() {
    return isSpeechPreviewing.value || isSpeechPreviewLoading.value || !!previewUtterance || !!previewAudio
  }

  /** 释放当前试听 <audio> 与 objectURL（不推进 session、不动 utterance）。 */
  function releasePreviewAudio() {
    if (previewAudio) {
      previewAudio.onplay = null
      previewAudio.onended = null
      previewAudio.onerror = null
      try {
        previewAudio.pause()
      } catch {
        // 忽略：部分平台清空 src 后 pause 会抛
      }
      previewAudio.src = ''
      previewAudio = null
    }
    if (previewAudioUrl) {
      try {
        URL.revokeObjectURL(previewAudioUrl)
      } catch {
        // 测试环境可能没有 revokeObjectURL
      }
      previewAudioUrl = ''
    }
  }

  function cleanupSpeechPreview() {
    // 先推进 session：迟到的合成结果 / 事件回调据此丢弃，不能再开始播放。
    previewSessionId += 1
    previewAbortController?.abort()
    previewAbortController = null
    if (previewUtterance) {
      previewUtterance.onstart = null
      previewUtterance.onend = null
      previewUtterance.onerror = null
      previewUtterance = null
      // 只有试听用 utterance 时才 cancel，避免误伤书内朗读（试听前已 stopTTS，
      // 这里 cancel 只是清掉可能排队的试听语音）。
      try {
        synth?.cancel()
      } catch {
        // ignore platform-specific cancel errors
      }
    }
    releasePreviewAudio()
    isSpeechPreviewing.value = false
    isSpeechPreviewLoading.value = false
  }

  /**
   * 停止试听。没有在试听时什么都不做 —— 面板卸载/切通道会无条件调用它，
   * 若无条件 cancel synth 会把用户正在听的书内朗读一起掐掉。
   */
  function stopSpeechPreview() {
    if (!hasActiveSpeechPreview()) return
    cleanupSpeechPreview()
  }

  function failSpeechPreview(error: unknown) {
    cleanupSpeechPreview()
    const message = (error as Error)?.message || '试听失败'
    appStore.showToast(message, 'error')
  }

  /** 系统语音试听某一段时该用的音色：对话段用对话音色（未配置则跟随旁白）。 */
  function resolvePreviewSystemVoice(segmentType: SpeechSegmentType) {
    const narrationVoice = voiceList.value.find((voice) => voice.name === speechConfig.voiceName)
    if (segmentType !== 'dialogue' || !speechConfig.dualVoiceEnabled) return narrationVoice
    return voiceList.value.find((voice) => voice.name === speechConfig.dialogueVoiceName) || narrationVoice
  }

  /**
   * 系统语音逐段试听：一段播完（onend）再 speak 下一段。
   *
   * 不一次性把整串 utterance 丢进浏览器队列：那样 stop 时只能靠 cancel 清队列，
   * 而且无法在中途正确切换音色/判断结束。
   */
  function speakPreviewSegment(
    index: number,
    plan: SpeechSegment[],
    session: number,
    speech: SpeechSynthesis,
  ) {
    if (session !== previewSessionId) return
    if (index >= plan.length) {
      cleanupSpeechPreview()
      return
    }
    const segment = plan[index]
    const utterance = new SpeechSynthesisUtterance(segment.text)
    const voice = resolvePreviewSystemVoice(segment.type)
    utterance.lang = voice?.lang || 'zh-CN'
    utterance.voice = voice || null
    utterance.rate = speechConfig.speechRate
    utterance.pitch = speechConfig.speechPitch
    previewUtterance = utterance
    utterance.onstart = () => {
      if (session !== previewSessionId) return
      isSpeechPreviewLoading.value = false
      isSpeechPreviewing.value = true
    }
    utterance.onend = () => {
      if (session !== previewSessionId) return
      if (previewUtterance === utterance) previewUtterance = null
      speakPreviewSegment(index + 1, plan, session, speech)
    }
    utterance.onerror = (event) => {
      if (session !== previewSessionId) return
      // 用户点停止会以 interrupted/canceled 触发，不是错误。
      if (event.error === 'interrupted' || event.error === 'canceled') {
        cleanupSpeechPreview()
        return
      }
      failSpeechPreview(new Error('系统语音试听失败'))
    }
    speech.speak(utterance)
  }

  /**
   * 在线通道逐段试听：每段合成完播放，播完再合成下一段。
   *
   * 串行而不是并行预热：每段的音色/风格都可能不同，且必须严格按文本顺序出声；
   * 串行也让 stop 时最多只有一个在途请求需要丢弃。
   */
  function playPreviewSegment(
    index: number,
    plan: SpeechSegment[],
    session: number,
    controller: AbortController,
  ) {
    if (session !== previewSessionId) return
    if (index >= plan.length) {
      cleanupSpeechPreview()
      return
    }
    const segment = plan[index]
    void synthesizeSpeechChunkBlob(segment.text, segment.type, controller.signal)
      .then((blob) => {
        if (session !== previewSessionId) return
        const url = URL.createObjectURL(blob)
        previewAudioUrl = url
        const audio = new Audio(url)
        previewAudio = audio
        audio.onplay = () => {
          if (session !== previewSessionId) return
          isSpeechPreviewLoading.value = false
          isSpeechPreviewing.value = true
        }
        audio.onended = () => {
          if (session !== previewSessionId) return
          releasePreviewAudio()
          playPreviewSegment(index + 1, plan, session, controller)
        }
        audio.onerror = () => {
          if (session !== previewSessionId) return
          failSpeechPreview(new Error('试听音频播放失败'))
        }
        void audio.play().then(() => {
          if (session !== previewSessionId) return
          // 有的环境不触发 onplay（自动播放策略/测试替身），play() 成功即代表已在播。
          isSpeechPreviewLoading.value = false
          isSpeechPreviewing.value = true
        }).catch((error) => {
          if (session !== previewSessionId) return
          failSpeechPreview(error)
        })
      })
      .catch((error) => {
        if (session !== previewSessionId) return
        if ((error as Error)?.name === 'AbortError') {
          cleanupSpeechPreview()
          return
        }
        failSpeechPreview(error)
      })
  }

  async function previewSpeech(text: string) {
    stopSpeechPreview()
    // 试听前先停下书内朗读（含缓冲流），避免两条声音重叠、也避免抢占语音队列。
    stopTTS()
    const rawText = (text || '').trim()
    if (!rawText) {
      appStore.showToast('请先输入要试听的文本', 'warning')
      return
    }
    const session = ++previewSessionId
    isSpeechPreviewLoading.value = true
    isSpeechPreviewing.value = false
    // 双音色 +（多片段，或唯一片段是对白）才走分段路径 —— 与自动朗读的分流规则一致：
    // 试听框只填一句「“你好。”」时用户开双音色就是想听对话音色，不能仍按旁白合成。
    const segments = speechConfig.dualVoiceEnabled ? splitSpeechSegments(rawText) : []
    const useSegmented = segments.length > 1 || segments[0]?.type === 'dialogue'
    const plan = useSegmented
      ? segments
      : [{ text: rawText, type: 'narration' as SpeechSegmentType }]

    if (speechConfig.provider === 'system') {
      synth = resolveSpeechSynthesis()
      if (!synth || typeof SpeechSynthesisUtterance === 'undefined') {
        failSpeechPreview(new Error('当前系统不支持系统语音，请改用其它朗读通道'))
        return
      }
      if (!voiceList.value.length) {
        fetchVoices()
      }
      speakPreviewSegment(0, plan, session, synth)
      return
    }

    const controller = new AbortController()
    previewAbortController = controller
    playPreviewSegment(0, plan, session, controller)
  }

  /* ─── Book / chapter ops ─── */
  async function loadBook(b: Book, syncProgress = true) {
    loading.value = true
    bookLoadError.value = ''
    const requestId = ++bookLoadRequestId
    chapterLoadRequestId += 1
    pendingLegadoProgress.value = null
    legadoPositionRestore.value = null
    legadoPositionRestoreCompleted.value = null
    // 返回主页再打开同一本书时，当前会话的正文预载仍然有效。此前无条件
    // clear() 会让已缓存的章节也重新走一遍 loadChapter 加载态。
    if (book.value?.bookUrl !== b.bookUrl) {
      preloadedContent.value.clear()
      preloadingContent.clear()
    }
    chapterPreloadGeneration += 1
    book.value = b
    snapshotOpenPosition(b)
    appStore.setReadingSessionBook(b.bookUrl, b.name)
    chapters.value = []
    content.value = ''
    appStore.markBookOpened(b.bookUrl)
    currentIndex.value = b.durChapterIndex || 0
    const localPosition = readSavedReadingPosition(b.bookUrl)
    // 书架记录与 localStorage 段落快照经常互相矛盾: 书架那条由 10 秒自动保存 /
    // 换章 / 退出时各写一次, 既有落后的副本, 也有抢先写下的「新章 0」。
    // 统一交给 resolveReadingPosition 判: **章号相同用本地**(逐页写入, 带段落锚点),
    // **章号不同用书架**(只有主动跳章才会出现这种组合)。早先只按「章节号一致就用
    // 本地, 否则用书架进度」判断, 于是换章后重开会落到该章第一页。
    const shelfPosition: ReadingPositionSnapshot = {
      chapterIndex: b.durChapterIndex || 0,
      progress: decodeServerProgress(b.durChapterPos),
      updatedAt: toMillis(b.durChapterTime),
    }
    const resumed = resolveReadingPosition(localPosition, shelfPosition)
    if (resumed) {
      currentIndex.value = resumed.chapterIndex
      chapterScrollProgress.value = Math.max(0, Math.min(1, resumed.progress || 0))
      // 让 loadChapter 的 `isOpeningSavedChapter` 判定与恢复后的位置一致:
      // 它拿 book.durChapterIndex 当作"上次保存的章", 不一致就会把初始进度当 0。
      if (book.value) {
        book.value.durChapterIndex = resumed.chapterIndex
        book.value.durChapterPos = encodeServerProgress(resumed.progress)
      }
      const shelfBook = shelfStore.books.find((item) => item.bookUrl === b.bookUrl)
      if (shelfBook) {
        shelfBook.durChapterIndex = resumed.chapterIndex
        shelfBook.durChapterPos = encodeServerProgress(resumed.progress)
      }
    } else {
      chapterScrollProgress.value = 0
    }
    loadReadChapterHistory(b)
    progressDirty.value = false
    lastServerProgressKey.value = ''
    chaptersLoading.value = true
    try {
      // 空白目录地址必须当成"没有"。
      //
      // 从手机端(Legado)导入的书架里 `tocUrl` 常是空串(Kotlin 侧是非空 String)，
      // 前端原样带着它去请求目录，后端就会拿空 URL 抓页 —— 既不回落去取详情，
      // 也报不出可读原因。这里统一归一成 undefined，让下面"缺 tocUrl 就取详情"
      // 的分支真正生效。
      let tocUrl = normalizeOptionalUrl(b.tocUrl)
      if (!tocUrl && !isLocalBook(b)) {
        // 搜索结果还没有详情；取回书源脚本设置的每书配置，再用详情中的目录地址加载。
        const detail = await getBookInfo(b.bookUrl, b.origin).catch(() => null)
        if (requestId !== bookLoadRequestId || book.value?.bookUrl !== b.bookUrl) return
        if (detail) {
          tocUrl = normalizeOptionalUrl(detail.tocUrl) || tocUrl
          book.value = {
            ...b,
            tocUrl,
            readConfig: detail.readConfig
              ? { ...b.readConfig, ...detail.readConfig }
              : b.readConfig,
          }
        }
      }
      // 先取初始章正文并恢复手机端进度，再设置当前章节，
      // 避免 setActiveChapterState 更新阅读时间后遮蔽手机端进度。
      const chapterList = await getChapterList({
        bookUrl: b.bookUrl,
        bookSourceUrl: b.origin,
        tocUrl,
      })
      if (requestId !== bookLoadRequestId || book.value?.bookUrl !== b.bookUrl) return
      chapters.value = chapterList

      const initialIndex = chapters.value.length
        ? Math.max(0, Math.min(chapters.value.length - 1, currentIndex.value))
        : 0
      currentIndex.value = initialIndex
      if (!chapters.value.length) throw new Error('目录为空，无法加载正文')
      await loadChapter(
        initialIndex,
        false,
        localPosition?.chapterIndex === initialIndex ? localPosition.progress : undefined,
      )
      if (requestId !== bookLoadRequestId || book.value?.bookUrl !== b.bookUrl) return
      if (syncProgress) {
        await restoreCurrentBookProgressFromLegado(undefined, b.bookUrl, requestId)
          .catch((error) => {
            appStore.showToast((error as Error).message || '读取网盘阅读进度失败', 'warning')
          })
      }
      if (requestId !== bookLoadRequestId || book.value?.bookUrl !== b.bookUrl) return
      saveReaderSession()
      // 打开书后台静默检查目录更新(按书 30 分钟节流), 不阻塞首屏渲染
      void refreshTocFromSource()
    } catch (error) {
      // 只清掉自己那一次的加载态：loadBook 之后可能又发起了新的 loadBook/loadChapter,
      // 无条件置 false 会把它们的加载动画一起关掉。
      if (requestId === bookLoadRequestId) {
        loading.value = false
        bookLoadError.value = (error instanceof Error ? error.message : String(error ?? '')).trim()
          || '打开书籍失败'
      }
      // 失败必须让用户看见原因，并且把「这本书打不开」这件事告诉调用方。
      //
      // 早先这里只把错误往上抛，调用点(书架/最近/详情)都是 `void loadBook(...).catch(...)`，
      // 而阅读页在 store.book 已赋值的情况下不会做任何兜底 —— 于是目录抓失败
      // (书源地址对不上、tocUrl 为空、站点超时)就停在「加载中…」不动，
      // 连"为什么"都没有，用户只能靠反复换源摸索。
      throw error
    } finally {
      if (requestId === bookLoadRequestId) {
        chaptersLoading.value = false
        if (chapters.value.length === 0) loading.value = false
      }
    }
  }

  function setActiveChapterState(index: number, chapterContent: string, progress = 0) {
    content.value = chapterContent
    currentIndex.value = index
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

  function flushProgressToServerKeepalive(force = false): Promise<void> {
    const payload = currentServerProgressPayload()
    if (!payload) return Promise.resolve()
    const nextKey = `${payload.bookUrl}::${payload.index}::${payload.position}`
    if (!force && !progressDirty.value && lastServerProgressKey.value === nextKey) return Promise.resolve()
    // 同一份 payload 已经在路上时不重复发。
    if (keepaliveInFlightKey === nextKey) return keepaliveInFlight ?? Promise.resolve()

    // 关键: **不能在 IPC 返回之前**把进度标记为"已保存"。旧实现先清
    // `progressDirty` 再发 IPC, 且把错误吞掉 —— 一旦这次写入失败(书不在书架、
    // 磁盘忙、进程正在退出), 内存里却认为已经落盘, 后面所有自动保存都会被
    // `lastServerProgressKey === nextKey` 跳过, 书架永久停在更旧的位置。
    // 用户看到的就是"每次重开都回到上上次退出的地方"。
    const task = invokeRaw('save_book_progress', { req: payload })
      .then(() => {
        if (lastServerProgressKey.value !== nextKey) {
          progressDirty.value = false
          lastServerProgressKey.value = nextKey
        }
      })
      .catch(() => undefined)
      .finally(() => {
        if (keepaliveInFlightKey === nextKey) {
          keepaliveInFlightKey = ''
          keepaliveInFlight = null
        }
      })
    keepaliveInFlightKey = nextKey
    keepaliveInFlight = task
    return task
  }

  /**
   * 预载缓存容量。
   *
   * 必须容纳「当前章 + 后两章 + 上一章」(预载一批最多 3 章, 加上 loadChapter 自己
   * 要缓存的当前章共 4 个)再留一格余量。容量只有 3 时每写入一次就必然淘汰一个,
   * 而淘汰按插入序进行 —— 最早插入的往往正是**下一章**, 于是用户翻页那一刻缓存
   * 刚好被挤掉, 又得重新请求, 表现就是"开了预加载还是有加载动画"。
   */
  const PRELOAD_CACHE_LIMIT = 5

  /**
   * 写入缓存并按 LRU 淘汰。
   *
   * 用插入序而不是"离当前章的距离"来决定淘汰: 预载是**成批**写入的(一次 1~3 章),
   * 按距离排序会把刚写进来的相邻章判成"远处"当场淘汰, 等于白写。插入序天然保护
   * 了最近写入的整批。
   *
   * 当前章(`currentIndex`)永不淘汰。连续阅读/预载远端批次时, 用户正在读的那一章
   * 可能长时间没有再次写入而沦为"最久未用", 被后续预载挤出去 —— 而字号调整、
   * 窗口 resize、原地刷新都要重新取它, 一挤掉就又是一次网络请求。
   */
  function cachePreloadedContent(index: number, chapterContent: string) {
    preloadedContent.value.set(index, chapterContent)
    if (preloadedContent.value.size <= PRELOAD_CACHE_LIMIT) return

    const reading = currentIndex.value
    while (preloadedContent.value.size > PRELOAD_CACHE_LIMIT) {
      // 本次刚写入的章节是这次访问的直接结果, 与当前章一并保留。
      const oldestKey = Array.from(preloadedContent.value.keys())
        .find((key) => key !== index && key !== reading)
      if (oldestKey === undefined) break
      preloadedContent.value.delete(oldestKey)
    }
  }

  /**
   * 把已在缓存里的章节标记为"刚刚用过"。
   *
   * Map 的 set 对已存在的键不会改变插入序, 所以命中缓存必须显式 delete + set 才能
   * 移到队尾。缺了这一步, 一路预载下游会把**用户正在读的这一章**当成最久未用淘汰掉,
   * 回看或重排时又得重新请求。
   */
  function touchPreloadedContent(index: number) {
    const cached = preloadedContent.value.get(index)
    if (cached === undefined) return
    preloadedContent.value.delete(index)
    preloadedContent.value.set(index, cached)
  }

  async function fetchChapterContent(index: number, forceRefresh = false, reportStage = false) {
    if (!book.value || !chapters.value[index]) return null

    // 命中已预加载内容 / 在途的预加载请求时都不会发请求，也就没有阶段可言。
    // 这两个提前返回必须发生在设置 stage **之前**：否则设了提示却走不到 finally，
    // 提示会永久卡在「正在获取章节…」。
    if (!forceRefresh && preloadedContent.value.has(index)) {
      touchPreloadedContent(index)
      return preloadedContent.value.get(index) || null
    }

    if (!forceRefresh) {
      const pending = preloadingContent.get(index)
      if (pending) return pending
    }

    const chapter = chapters.value[index]
    // 本次请求发起时的代次。换书/关闭预载会递增它, 迟到的响应据此放弃写缓存。
    const generation = chapterPreloadGeneration

    // 立即给出可见反馈，不等后端的阶段事件到达（否则跳章后会有一段"无响应"观感，
    // 用户会以为点了没反应）。后端事件随后把阶段细化成 parsing / caching。
    // 用「所有者令牌」登记：并发加载时只有最后登记的那次能清掉提示。
    let ownsStage = false
    if (reportStage) {
      loadStage.value = 'fetching'
      chapterStageOwner = ++chapterStageSeq
      ownsStage = true
    }
    const stageOwner = chapterStageOwner

    try {
      const chapterContent = await getBookContent({
        bookUrl: book.value.bookUrl,
        chapterUrl: chapter.url,
        bookSourceUrl: book.value.origin,
        refresh: forceRefresh ? 1 : 0,
        // 只有仍持有提示的那次加载才能更新阶段（并发加载时避免互相覆盖）
        onStage: reportStage
          ? (stage) => {
              if (chapterStageOwner === stageOwner) loadStage.value = stage
            }
          : undefined,
      })
      if (typeof chapterContent !== 'string') {
        throw new Error('书源返回的正文格式无效')
      }
      if (!chapterContent.trim()) {
        throw new Error('书源正文解析为空，请检查正文规则')
      }
      // 任何拿到正文的路径都入缓存, 而不是只由 loadChapter 回写。
      // 连续阅读的正文是经 buildContinuousChapter 走这条路径取的, 只在 loadChapter
      // 里写缓存会让连续模式每滚过一次都要重新请求同一个章节。
      if (generation === chapterPreloadGeneration) {
        cachePreloadedContent(index, chapterContent)
      }
      return chapterContent
    } catch (error) {
      if (!appStore.isOnline) {
        throw new Error('当前处于离线状态，未缓存章节无法打开')
      }
      throw error
    } finally {
      // 只清自己登记的提示：并发加载时旧请求不能清掉新章节的提示，
      // 也不能用 requestId 判断（连续阅读/跳章不走 loadChapter，requestId 不变，
      // 会让提示永久卡住）。
      if (ownsStage && chapterStageOwner === stageOwner) loadStage.value = ''
    }
  }

  async function loadChapter(
    index: number,
    forceRefresh = false,
    initialProgressOverride?: number,
  ) {
    if (!book.value || !chapters.value[index]) return

    if (legadoPositionRestoreCompleted.value?.index !== index) {
      legadoPositionRestoreCompleted.value = null
    }

    const requestId = ++chapterLoadRequestId
    const bookUrl = book.value.bookUrl

    const cachedContent = !forceRefresh ? preloadedContent.value.get(index) : undefined
    // 在途的预载请求同样算命中。fetchChapterContent 本来就能复用它、不会重复发请求,
    // 但那次复用发生在 loading 赋值**之后** —— 只看已完成缓存的话, 预载已发出而尚未
    // 返回时翻章会先闪一次全屏加载动画, 再被同一份正文瞬间替换掉。
    const pendingPreload = cachedContent === undefined && !forceRefresh
      ? preloadingContent.get(index)
      : undefined
    loading.value = cachedContent === undefined && pendingPreload === undefined
    // 复用在途预载时同样不亮动画(否则就是那次闪烁), 但书源很慢的话用户会以为
    // 翻页没反应。改成延迟兜底: 200ms 内返回就完全不显示动画, 超过才亮出来。
    // 这个定时器只在"等一份已经在路上的正文"时存在, 与 loading 互斥。
    let pendingIndicatorTimer: number | null = null
    if (cachedContent === undefined && pendingPreload !== undefined) {
      pendingIndicatorTimer = window.setTimeout(() => {
        pendingIndicatorTimer = null
        if (requestId === chapterLoadRequestId) loading.value = true
      }, CHAPTER_LOADING_INDICATOR_DELAY_MS)
    }
    try {
      let chapterContent = cachedContent !== undefined
        ? cachedContent
        : await fetchChapterContent(index, forceRefresh, true)
      if (chapterContent == null && pendingPreload) {
        // 复用的在途预载失败了(preloadNextChapter 把错误吞成 null)。这里必须补一次
        // 真实请求: 否则用户只会看到"翻到下一章什么也没发生", 连错误提示都没有。
        // 补请求期间必须亮出加载态 —— 这次是真的要等网络了。
        if (pendingIndicatorTimer !== null) {
          clearTimeout(pendingIndicatorTimer)
          pendingIndicatorTimer = null
        }
        if (requestId === chapterLoadRequestId) loading.value = true
        chapterContent = await fetchChapterContent(index, forceRefresh, true)
      }
      if (chapterContent == null) return
      if (requestId !== chapterLoadRequestId || book.value?.bookUrl !== bookUrl) return

      const previousSavedIndex = book.value.durChapterIndex ?? 0
      const previousSavedProgress = decodeServerProgress(book.value.durChapterPos)
      const isOpeningSavedChapter = !forceRefresh && index === previousSavedIndex
      const pendingRemoteProgress = pendingLegadoProgress.value?.index === index
        ? pendingLegadoProgress.value
        : null
      const cloudProgress = pendingRemoteProgress
        ? Math.max(0, Math.min(1, pendingRemoteProgress.position
          / Math.max(1, chapterContent.replace(/<[^>]+>/g, '').length)))
        : null
      const initialProgress = initialProgressOverride
        ?? cloudProgress
        ?? (isOpeningSavedChapter ? previousSavedProgress : 0)

      cachePreloadedContent(index, chapterContent)
      if (cloudProgress != null && book.value) {
        snapshotOpenPosition({
          ...book.value,
          durChapterIndex: index,
          durChapterPos: encodeServerProgress(cloudProgress),
          durChapterTitle: chapters.value[index]?.title || book.value.durChapterTitle,
        })
      }
      setActiveChapterState(index, chapterContent, initialProgress)
      if (pendingRemoteProgress && cloudProgress != null) {
        const existingRestore = legadoPositionRestore.value
        legadoPositionRestore.value = {
          index,
          position: Math.max(0, pendingRemoteProgress.position),
          progress: cloudProgress,
          token: existingRestore?.index === index
            && existingRestore.position === pendingRemoteProgress.position
            ? existingRestore.token
            : ++legadoPositionRestoreToken,
          ready: true,
        }
        pendingLegadoProgress.value = null
      }
      markChapterAsRead(index)
      appStore.markChapterRead(book.value.bookUrl, index, chapters.value.length)
      loading.value = false

      if (!isOpeningSavedChapter) {
        void persistProgress(index, cloudProgress ?? 0)
      }

      if (config.enablePreload) {
        void preloadAroundChapter(index)
      }
    } catch (error) {
      // 章节加载失败必须让用户看到原因(如书源提示"VIP专属内容"、正文规则为空、
      // 网络失败)。此前错误只向上抛: 目录/跳章的调用点没有 catch, 变成未处理
      // 的 Promise 拒绝, 界面上什么提示都没有, 用户只觉得"点了没反应"。
      if (requestId === chapterLoadRequestId) {
        const message = error instanceof Error ? error.message : String(error ?? '')
        appStore.showToast(message.trim() || '章节加载失败，请稍后重试', 'error')
      }
    } finally {
      // 只收尾 loading：阶段提示由 fetchChapterContent 的持有者机制管理，
      // 这里再清一次会误清连续模式下并发加载刚设置的提示。
      if (pendingIndicatorTimer !== null) clearTimeout(pendingIndicatorTimer)
      if (requestId === chapterLoadRequestId) loading.value = false
    }
  }

  /**
   * 同步探测某章是否已经"到手或马上到手"(已缓存或预载在途)。
   *
   * 给连续阅读判断要不要显示「正在加载下一章…」用: 命中预载时取正文只是一个
   * 微任务, 提示却会被渲染出来闪一下 —— 那正是用户眼里"预加载没生效"的观感。
   */
  function isChapterReady(index: number) {
    return preloadedContent.value.has(index) || preloadingContent.has(index)
  }

  /**
   * 预载当前章周边。
   *
   * `options.forwardOnly` 用于「隐藏已读」形态: 身后章节会被立即裁掉, 预载它
   * 既不会被读到、还要占一格缓存并把真正要用的下一章挤出去。
   */
  async function preloadAroundChapter(index: number, options: { forwardOnly?: boolean } = {}) {
    if (!book.value || !config.enablePreload) return
    const candidates = options.forwardOnly ? [index + 1, index + 2] : [index + 1, index + 2, index - 1]
    const targets = candidates
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
    bookLoadError.value = ''
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

      await loadBook(updatedBook, false)
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

  /**
   * 重新抓取当前章正文。索引与章节内进度一律保持不变。
   * refreshing 供阅读页区分「切章加载」与「原地刷新」: 连续阅读模式下后者
   * 必须保留已渲染的章节列表, 否则容器内容塌缩会让 handleScroll 误判当前章。
   */
  async function refreshContent() {
    if (!book.value || !chapters.value[currentIndex.value]) return
    loading.value = true
    refreshing.value = true
    try {
      const chapterContent = await fetchChapterContent(currentIndex.value, true, true)
      if (chapterContent == null) return
      setActiveChapterState(currentIndex.value, chapterContent, chapterScrollProgress.value)
      contentRefreshToken.value += 1
      void preloadAroundChapter(currentIndex.value)
    } finally {
      loading.value = false
      refreshing.value = false
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
    // tocUrl 为空串时不要传 —— 后端会把它当成"没给目录地址"并回落到 bookUrl 取详情。
    const base = {
      bookUrl: b.bookUrl,
      bookSourceUrl: b.origin,
      ...(normalizeOptionalUrl(b.tocUrl) ? { tocUrl: b.tocUrl } : {}),
    }
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
    bookLoadError.value = ''
    try {
      preloadedContent.value.clear()
      preloadingContent.clear()
      chapterPreloadGeneration += 1
      markTocChecked(book.value.bookUrl)
      const fresh = await fetchTocListWithStabilizing()
      if (!fresh.length) {
        // 目录刷成空必须报出来。此前这里静默接受空数组：目录列表直接变空白，
        // 阅读页正文也不动，用户只会以为"刷新按钮坏了"。
        throw new Error('目录为空，请检查书源或稍后重试')
      }
      chapters.value = fresh
      const targetIndex = Math.max(0, Math.min(chapters.value.length - 1, currentIndex.value))
      if (chapters.value[targetIndex]) {
        await loadChapter(targetIndex, true)
      }
    } catch (error) {
      const message = (error instanceof Error ? error.message : String(error ?? '')).trim()
      appStore.showToast(message || '刷新目录失败', 'error')
      throw error
    } finally {
      chaptersLoading.value = false
    }
  }

  function setChapterScrollProgress(value: number) {
    chapterScrollProgress.value = Math.max(0, Math.min(1, value))
    syncLocalBookProgress(chapterScrollProgress.value)
    // 高频路径(每次滚动事件): 只写轻量进度, 不序列化整份目录。
    saveReaderProgress()
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
  /** 拉取替换规则。返回是否成功 —— 失败时调用方需要知道"本地状态不可信",
   *  例如开关失败后要靠这次拉取恢复真实值, 静默吞错会让 UI 停在错误状态。 */
  async function fetchReplaceRules(): Promise<boolean> {
    try {
      replaceRules.value = await getReplaceRules()
      return true
    } catch {
      return false
    }
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
    bookLoadRequestId += 1
    chapterLoadRequestId += 1
    restoringLegadoProgress.value = false
    legadoPositionRestore.value = null
    legadoPositionRestoreCompleted.value = null
    bookLoadError.value = ''
    book.value = null
    openPosition.value = null
    refreshing.value = false
    // 作废在飞请求的阶段提示持有者: 只清空 loadStage 的话, 旧请求后续的
    // onStage 回调仍持有所有权, 会把浮层重新点亮。
    chapterStageOwner = ++chapterStageSeq
    loadStage.value = ''
    contentRefreshToken.value = 0
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
    book, chapters, currentIndex, content, loading, loadStage, refreshing, contentRefreshToken, chaptersLoading, openPosition, restoringLegadoProgress,
    bookLoadError,
    legadoPositionRestore,
    legadoPositionRestoreCompleted,
    currentChapter, hasNext, hasPrev, readingProgress,
      loadBook, loadChapter, fetchChapterContent, setActiveChapterState, refreshContent, nextChapter, prevChapter, clear,
      chapterScrollProgress, setChapterScrollProgress,
      getPersistedReaderSession, restorePersistedSession, syncCurrentBookProgressToLegado,
      saveReaderProgress, readReaderProgress,
      restoreCurrentBookProgressFromLegado, uploadCurrentBookProgressToLegado,
      syncCurrentBookProgressFromLegadoOnExit,
      clearLegadoPositionRestore,
      consumeLegadoPositionRestoreCompleted,
      persistProgress, flushProgressToServer, flushProgressToServerKeepalive,
      config, updateConfig, resetConfig, saveConfig, setBackgroundImage, clearBackgroundImage,
      setReaderBackgroundImage, clearReaderBackgroundImage,
      customFonts, fetchCustomFonts, importCustomFont, removeCustomFont, customFontFamily,
    themeIndex, isNight, currentTheme, chromeTheme, readerTheme, readerBackgroundColor, setThemeIndex, toggleNight,
    autoReading, autoReadingTimer, toggleAutoReading, stopAutoReading,
    activePanel, openPanel, togglePanel, backPanel, closePanel,
    bookmarks, fetchBookmarks, addBookmark, removeBookmark, removeBookmarks,
    readChapterKeys, isChapterRead, markChapterAsRead,
    replaceRules, fetchReplaceRules, failedReplaceRules,
    switchSource, preloadNextChapter, preloadAroundChapter, isChapterReady,
    refreshChapters, refreshTocFromSource,
    isSpeaking, isSpeechLoading, isPaused, speechProgress, startTTS, pauseTTS, stopTTS,
    addSpeechStopListener,
    voiceList, speechConfig, speechStopAt, speechProviderLabel, openAISpeechConfigured, httpSpeechConfigured, edgeSpeechConfigured, msSpeechConfigured,
    systemTtsNativeEventsReliable, systemSpeechSupported,
    fetchVoices, setVoiceName, setSpeechProvider, setHttpTtsId, setSpeechRate, setSpeechPitch, setSpeechStopTimer, clearSpeechStopTimer,
    setDualVoiceEnabled, setDialogueVoiceName, setDialogueOpenaiVoice, setDialogueHttpTtsId, setBufferChars,
    setEdgeVoice, setEdgeDialogueVoice, setEdgePitch, setEdgeVolume, setEdgeFormat,
    setMsVoice, setMsDialogueVoice, setMsStyle, setMsDialogueStyle, setMsStyleDegree, setMsPitch, setMsVolume, setMsFormat,
    isSpeechPreviewing, isSpeechPreviewLoading, previewSpeech, stopSpeechPreview,
    attachSpeechStreamControls, synthesizeSpeechChunkBlob,
    setOpenAISpeechSource, setSpeechApiFormat, setOpenAISpeechBaseUrl, setSpeechProxyUrl, setOpenAISpeechApiKey, setOpenAISpeechModel, setOpenAISpeechVoice, setOpenAISpeechFormat, setOpenAISpeechRequestMode, preloadOpenAITTS,
    displayContent, processContentForDisplay, replaceRulesDegraded, failedScriptRuleCount,
    isAutoScrolling,
    readerControlsVisible,
    hotkeyBindings, hotkeyActions, hotkeysAreDefault, setHotkeyBinding, resetHotkeyBindings,
    bindHotkeyKey, unbindHotkeyKey, hotkeyConflicts,
  }
})
