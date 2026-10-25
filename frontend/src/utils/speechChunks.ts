// 正文 → 朗读块。
//
// 两个职责，顺序不能反：
// 1. 先把一段正文按引号 / 说话人冒号切成「旁白段 / 对话段」（`speechSegments`）；
// 2. 再按长度把每个段切成适合单次合成的短块，**子块沿用父段的类型**。
//
// 早期的实现是反的：先按句号把整段切成 ≤70 字，再对每个小块单独识别引号。于是
// 一段长对话被句号切碎后，只有含开引号的第一块算对话，后面几块（不带引号 / 只剩
// 闭符）全被判成旁白 —— 听感上就是「对话读一半突然换旁白音色」。抽到这里是为了
// 让这个顺序能被单测钉死。

import { hasSpeakableContent } from './speakableText'
import {
  splitSpeechSegmentsDetailed,
  type SpeechSegmentType,
} from './speechSegments'

/** 单块字符上限。openai / edge / msdemo 都是「一次请求一段音频」，块小首播更快。 */
export const SPEECH_CHUNK_CHAR_LIMIT = 70

/**
 * 「只说人名」的旁白标签上限（`张三：`、`老道士叹了口气，说：`）。
 *
 * 这类段落在双音色下会单独占一次合成请求，而它往往只有几个字。紧跟在一段旁白
 * 之后时直接并进那段旁白：音色不变（都是旁白），请求数少一次。
 */
const SPEAKER_LABEL_MAX_CHARS = 24

export interface SpeechChunkPiece {
  text: string
  type: SpeechSegmentType
}

export interface BuildSpeechChunkPiecesOptions {
  /**
   * 双音色：先切旁白/对话段，再切长度。
   *
   * 关闭时整段一块、类型恒为 narration —— 与单音色时代的行为完全一致，避免不
   * 开双音色的用户因为这次改动多出几段请求。
   */
  dualVoice: boolean
  /**
   * 是否按句切成 ≤`SPEECH_CHUNK_CHAR_LIMIT` 的块。
   *
   * 在线引擎（openai / edge / msdemo）按块合成，切碎能显著降低首播延迟；`http`
   * 引擎的请求按整段模板展开，切碎只会成倍增加请求数，故不切。
   */
  sentenceChunked: boolean
  /** 上一段结束时仍未闭合的引号闭符（跨段对话），见 `splitSpeechSegmentsDetailed`。 */
  openCloser?: string | null
}

export interface SpeechChunkPiecesResult {
  pieces: SpeechChunkPiece[]
  /** 本段结束仍未闭合的引号闭符，调用方原样喂给下一段；null = 回到旁白。 */
  openCloser: string | null
}

/**
 * 一段正文 → 朗读块（含旁白 / 对话类型与跨段对话状态）。
 *
 * 纯标点/纯引号的块会被丢弃：在线服务对这类片段不会返回音频，发出去只会换来错误，
 * 而错误又可能打断整段朗读（见 `utils/speakableText`）。
 */
export function buildSpeechChunkPieces(
  text: string,
  options: BuildSpeechChunkPiecesOptions,
): SpeechChunkPiecesResult {
  const trimmed = (text || '').trim()
  if (!trimmed) {
    return { pieces: [], openCloser: options.openCloser ?? null }
  }

  if (!options.dualVoice) {
    const texts = options.sentenceChunked ? buildSpeechChunksFromText(trimmed) : [trimmed]
    const pieces = texts
      .filter((piece) => hasSpeakableContent(piece))
      .map((piece) => ({ text: piece, type: 'narration' as SpeechSegmentType }))
    // 单音色不解析引号，跨段对话状态对本段无意义；交回 null 让下一段从头判定。
    return { pieces, openCloser: null }
  }

  // 关键顺序：先按引号/冒号切段，再对每个段切长度。这样长对话的所有子块都带着
  // 父段的 dialogue，不会因为中间某一小块没有引号就被打回旁白。
  const { segments, openCloser } = splitSpeechSegmentsDetailed(trimmed, {
    openCloser: options.openCloser ?? null,
  })
  const pieces: SpeechChunkPiece[] = []
  for (const segment of segments) {
    const texts = options.sentenceChunked
      ? buildSpeechChunksFromText(segment.text)
      : [segment.text.trim()]
    for (const pieceText of texts) {
      if (!hasSpeakableContent(pieceText)) continue
      // 只有一个说话人标签的旁白段（`张三：`）紧跟在一段旁白后可并进去：
      // 类型相同、朗读听感不变，但少一次合成请求。
      const previous = pieces[pieces.length - 1]
      if (
        previous
        && previous.type === 'narration'
        && segment.type === 'narration'
        && isSpeakerLabelOnly(pieceText)
        && !options.sentenceChunked
      ) {
        previous.text += pieceText
        continue
      }
      pieces.push({ text: pieceText, type: segment.type })
    }
  }
  return { pieces, openCloser }
}

/** 这一段是不是「只有说话人标签、没有说话内容」（`张三：`、`他笑道：`）。 */
function isSpeakerLabelOnly(text: string): boolean {
  const trimmed = text.trim()
  if (!trimmed || trimmed.length > SPEAKER_LABEL_MAX_CHARS) return false
  if (!trimmed.endsWith('：')) return false
  return !/[。！？!?；;…]/.test(trimmed.slice(0, -1))
}

/**
 * 把一段文本按句号切成适合一次合成的短块。
 *
 * 拆成接受纯文本而不是 HTMLElement：缓冲流还要为**尚未渲染到 DOM 的下一章**
 * 预切块（跨章预取），那些文本只能从 store 的正文里来。
 */
export function buildSpeechChunksFromText(rawText: string): string[] {
  const text = rawText.trim()
  if (!text) return [] as string[]

  const sentences = text
    .replace(/\n+/g, '\n')
    .split(/(?<=[。！？!?；;])/)
    .map((item) => item.trim())
    .filter(Boolean)

  const chunks: string[] = []
  let current = ''

  const pushCurrent = () => {
    const normalized = current.trim()
    if (normalized) chunks.push(normalized)
    current = ''
  }

  for (const sentence of (sentences.length ? sentences : [text])) {
    if (sentence.length > SPEECH_CHUNK_CHAR_LIMIT) {
      pushCurrent()
      chunks.push(...splitLongSentence(sentence))
      continue
    }
    const next = current ? `${current}${sentence}` : sentence
    if (next.length > SPEECH_CHUNK_CHAR_LIMIT) {
      pushCurrent()
      current = sentence
    } else {
      current = next
    }
  }

  pushCurrent()
  return chunks.length ? chunks : [text]
}

/** 单句超过块上限时按逗号/顿号/空格就近断开；正文里没有可断点时硬切。 */
export function splitLongSentence(sentence: string): string[] {
  const chunks: string[] = []
  let remaining = sentence.trim()
  while (remaining.length > SPEECH_CHUNK_CHAR_LIMIT) {
    let splitIndex = Math.max(
      remaining.lastIndexOf('，', SPEECH_CHUNK_CHAR_LIMIT),
      remaining.lastIndexOf('、', SPEECH_CHUNK_CHAR_LIMIT),
      remaining.lastIndexOf(',', SPEECH_CHUNK_CHAR_LIMIT),
      remaining.lastIndexOf(' ', SPEECH_CHUNK_CHAR_LIMIT),
    )
    if (splitIndex <= 0) {
      splitIndex = SPEECH_CHUNK_CHAR_LIMIT
    }
    chunks.push(remaining.slice(0, splitIndex).trim())
    remaining = remaining.slice(splitIndex).trim()
  }
  if (remaining) chunks.push(remaining)
  return chunks
}
