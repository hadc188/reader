import type {
  AiBookConfig,
  AiBookCharacter,
  AiBookLocation,
  AiBookMap,
  AiBookMemory,
  AiBookModelUpdate,
  AiBookNote,
  AiBookRelationship,
  Book,
  BookChapter,
} from '../types'
import { invokeRaw, invokeEnvelope } from '../api/invoke'
import { isAiBookConfigReady, isAiBookImageConfigReady } from './aiBookConfig'
import { summarizeHttpErrorBody } from './httpError'

export type AiBookChatMessage = {
  role: 'system' | 'user' | 'assistant' | 'tool'
  content?: string | null
  tool_calls?: AiBookToolCall[]
  tool_call_id?: string
  name?: string
}

export type AiBookToolCall = {
  id: string
  type?: 'function'
  function?: {
    name?: string
    arguments?: string
  }
}

export interface BuildPromptParams {
  bookName: string
  chapterTitle: string
  chapterIndex: number
  chapterContent: string
  memory: AiBookMemory
}

export interface GenerateMemoryParams {
  config: AiBookConfig
  book: Book
  chapter: BookChapter
  chapterContent: string
  memory: AiBookMemory
  fetchImpl?: typeof fetch
}

export interface GenerateMapParams {
  config: AiBookConfig
  prompt: string
  fetchImpl?: typeof fetch
}

export interface UploadGeneratedMapParams {
  b64Json?: string
  imageUrl?: string
  filename: string
  useBackendProxy?: boolean
  fetchImpl?: typeof fetch
}

export interface ApplyMapFallbackParams {
  prompt: string
  reason: string
  sourceChapterIndex?: number
  updatedAt?: number
}

interface OpenAIChatResponse {
  choices?: Array<{
    message?: {
      content?: string | null
      tool_calls?: AiBookToolCall[]
    }
  }>
}

interface OpenAIImageResponse {
  data?: Array<{
    b64_json?: string
    url?: string
  }>
}

interface AiProxyResult {
  status: number
  contentType?: string
  body: Uint8Array
}

function aiProxyResponse(result: AiProxyResult): Response {
  return new Response(result.body as BlobPart, {
    status: result.status,
    headers: result.contentType ? { 'content-type': result.contentType } : undefined,
  })
}

interface AiProxyRequestParams {
  config: AiBookConfig
  baseUrl: string
  apiKey: string
  fullUrl: boolean
  path: '/v1/chat/completions' | '/v1/images/generations'
  body: Record<string, unknown>
  fetchImpl: typeof fetch
}

interface AiBookRawModelUpdate {
  memory?: Partial<AiBookMemory>
  memoryPatch?: Partial<AiBookMemory>
  patch?: Partial<AiBookMemory>
  shouldRegenerateMap?: boolean
  mapPrompt?: string
  summary?: string
  worldview?: unknown
  characters?: unknown
  relationships?: unknown
  locations?: unknown
  mapDirty?: boolean
}

type AiBookToolResult = {
  content: Record<string, unknown>
  final?: boolean
  raw?: AiBookRawModelUpdate
}

const MAX_AI_BOOK_AGENT_STEPS = 6
const MAX_AI_BOOK_SUMMARY_CHARS = 1200
const AI_BOOK_TOOL_GET_MEMORY = 'get_current_memory'
const AI_BOOK_TOOL_GET_CHAPTER = 'get_completed_chapter'
const AI_BOOK_TOOL_SAVE_PATCH = 'save_memory_patch'

const AI_BOOK_AGENT_TOOLS = [
  {
    type: 'function',
    function: {
      name: AI_BOOK_TOOL_GET_MEMORY,
      description: '读取当前已保存的小说 AI 资料。只返回已读进度内的结构化记忆。',
      parameters: {
        type: 'object',
        properties: {},
        additionalProperties: false,
      },
    },
  },
  {
    type: 'function',
    function: {
      name: AI_BOOK_TOOL_GET_CHAPTER,
      description: '读取本次需要处理的已完成章节正文。不会返回未读章节。',
      parameters: {
        type: 'object',
        properties: {},
        additionalProperties: false,
      },
    },
  },
  {
    type: 'function',
    function: {
      name: AI_BOOK_TOOL_SAVE_PATCH,
      description: '提交本章带来的结构化资料增量。必须在已读取当前资料和章节后调用一次作为最终结果。',
      parameters: {
        type: 'object',
        additionalProperties: false,
        properties: {
          memory: {
            type: 'object',
            additionalProperties: true,
            description: '增量资料，不要整包覆盖。可包含 summary、worldview、characters、relationships、locations。',
          },
          shouldRegenerateMap: {
            type: 'boolean',
            description: '只有重要地点、层级、路线或地图结构变化时为 true。',
          },
          mapPrompt: {
            type: 'string',
            description: '需要重绘地图时的俯视二维制图提示词。',
          },
        },
        required: ['memory', 'shouldRegenerateMap'],
      },
    },
  },
]

export function shouldRunAiBookAutoUpdate(
  memory: AiBookMemory | null | undefined,
  completedChapterIndex: number,
  config: AiBookConfig,
) {
  if (!memory?.enabled) return false
  if (!isAiBookConfigReady(config)) return false
  if (typeof memory.processedChapterIndex === 'number' && memory.processedChapterIndex >= completedChapterIndex) {
    return false
  }
  return true
}

export function buildAiBookPromptMessages({
  bookName,
  chapterTitle,
  chapterIndex,
}: BuildPromptParams): AiBookChatMessage[] {
  return [
    {
      role: 'system',
      content: [
        '你是小说阅读资料维护 agent。',
        '不得使用未读章节，不得补充未来剧情，不得剧透。',
        '必须通过工具按需读取当前资料和本次已完成章节，然后只提交增量 patch。',
        `必须先调用 ${AI_BOOK_TOOL_GET_MEMORY} 和 ${AI_BOOK_TOOL_GET_CHAPTER}，最后调用 ${AI_BOOK_TOOL_SAVE_PATCH} 完成更新。`,
        '不要在普通文本中输出最终 JSON；最终结果必须放在 save_memory_patch 工具参数里。',
        '无法确认的信息必须标记为“推断”或“未知”。',
        'summary 是截至当前已处理章节的累计剧情摘要，不是单章摘要；必须保留并压缩已有 summary，再融入当前章节新增进展。',
        'summary 必须持续压缩，建议 300-800 字；章节很多时只保留主线、重大转折、核心谜团和当前状态，不要逐章累加。',
        'summary 禁止以“本章”“第X章”“章节名：”开头；不要只复述当前章节，也不要丢弃前面章节的关键进展。',
        'summary 是唯一可以记录章节剧情进展的位置；worldview 不是章节简介。',
        'worldview 必须是跨章节可复用的设定集条目，只记录规则、制度、势力、历史、技术/魔法、社会文化、地理环境、组织体系、未确认设定。',
        'worldview 禁止写成本章剧情复述、人物行动流水账、案件经过、章节摘要；不要使用“本章”“这一章”“第X章”“第三章《标题》”作为设定标题或内容主体。',
        '如果当前章节没有新增稳定设定，worldview 必须输出 []；不要为了凑条目把章节内容改写成长段概述。',
        '世界观必须按 category 分类，例如：基础规则、势力制度、历史传说、技术/魔法、社会文化、地理环境、组织体系、未确认信息。',
        '角色和关系必须填写 importance: high|medium|low；只保留推动剧情、反复出现或明确影响主角行动的 high/medium 项。',
        '不要输出不重要、路人、一次性提及、无状态变化的角色；不要输出寒暄、同村、路过、单纯“认识”等低价值关系。',
        '人物关系必须去重：同一对人物的同类关系只输出一条，不要再输出反向重复项；保留信息量更高的描述。',
        '地点必须填写 parentName 表示层级归属；父级必须比子级尺度更大：国家 > 区域/郡 > 城市 > 街区/村镇 > 学校/建筑/住宅 > 房间/设施。',
        '禁止把国家挂在城市下面，禁止把城市挂在学校、建筑、住宅、房间等子地点下面；无法确认父级时 parentName 留空。',
        '只有新增重要地点、地点层级、路线、区域边界或地图结构变化时，shouldRegenerateMap 才能为 true；单纯角色状态或人物关系变化必须为 false。',
        '生成 mapPrompt 时必须写成俯视地图/二维制图提示词，强调区域边界、路线、图例、地图符号和地点标签。',
        'mapPrompt 不要写成场景照片、建筑照片、室内渲染或人物插画；机房、避难所等地点只能作为地图上的标注区域、平面轮廓或图标。',
      ].join('\n'),
    },
    {
      role: 'user',
      content: JSON.stringify({
        task: 'tool-calling-ai-book-memory-update',
        finalTool: AI_BOOK_TOOL_SAVE_PATCH,
        patchSchema: {
          summary: 'string，300-800 字累计已读剧情摘要，必须压缩旧 summary + 当前章节新增进展；禁止写成单章摘要或逐章流水账',
          worldview: [{
            category: '基础规则|势力制度|历史传说|技术/魔法|社会文化|地理环境|组织体系|未确认信息',
            title: 'string，设定名，只能是概念/规则/组织/地点体系名，不要写“本章/第X章/剧情/章节名”',
            content: 'string，稳定设定说明；禁止以章节号、章节名、时间顺序或角色行动复述开头',
            confidence: '已知|推断|未知',
            importance: 'high|medium|low',
          }],
          characters: [{
            name: 'string',
            aliases: ['string'],
            status: 'string',
            faction: 'string',
            location: 'string',
            description: 'string',
            lastSeenChapter: 'string',
            importance: 'high|medium|low',
          }],
          relationships: [{
            source: 'string',
            target: 'string',
            relation: 'string',
            status: 'string',
            description: 'string',
            importance: 'high|medium|low',
          }],
          locations: [{
            name: 'string',
            parentName: 'string or empty for top-level places',
            kind: 'string',
            description: 'string',
            status: 'string',
            relatedCharacters: ['string'],
            firstSeenChapter: 'string',
            importance: 'high|medium|low',
          }],
          shouldRegenerateMap: 'boolean',
          mapPrompt: 'string when map should be regenerated; must describe a top-down cartographic world map, not a scene/photo/building illustration',
        },
        qualityRules: [
          'worldview 必须有 category；同一 category 下不要重复 title；只写设定，不写本章简介。',
          'summary 必须是累计压缩摘要；如果已有 summary，先保留旧摘要中的主线，再合并当前章节新增变化，全篇控制在 300-800 字。',
          '剧情经过、角色行动、调查过程、战斗过程写入 summary 或角色状态，不要写入 worldview。',
          'worldview 宁可为空，也不要输出“第X章《标题》：角色先做A、随后做B”的单章总结。',
          'characters 只输出重要角色；背景人物、一次性称呼、无独立状态者不要输出。',
          'relationships 只输出重要关系；同一 source/target/relation 只保留一条，不要反向重复。',
          'locations 必须尽量给 parentName 形成正确层级，父级尺度必须大于子级；无法确认父级时留空。',
          'shouldRegenerateMap 只在地图相关地点信息发生重要变化时为 true。',
          '所有信息只来自工具返回的当前资料和当前章节；不确定就写 推断/未知。',
        ],
        bookName,
        chapter: {
          index: chapterIndex,
          title: chapterTitle,
        },
      }),
    },
  ]
}

export async function requestAiBookMemoryUpdate({
  config,
  book,
  chapter,
  chapterContent,
  memory,
  fetchImpl = fetch,
}: GenerateMemoryParams): Promise<AiBookModelUpdate> {
  const messages: AiBookChatMessage[] = buildAiBookPromptMessages({
    bookName: book.name,
    chapterTitle: chapter.title,
    chapterIndex: chapter.index,
    chapterContent,
    memory,
  })

  for (let step = 0; step < MAX_AI_BOOK_AGENT_STEPS; step += 1) {
    const response = await requestModelJson({
      config,
      baseUrl: config.textBaseUrl,
      apiKey: config.textApiKey,
      fullUrl: config.textUseFullUrl,
      path: '/v1/chat/completions',
      fetchImpl,
      body: {
        model: config.textModel,
        messages,
        tools: AI_BOOK_AGENT_TOOLS,
        tool_choice: 'auto',
        temperature: 0.2,
      },
    })

    if (!response.ok) {
      throw new Error(await readModelError(response, 'AI 资料生成失败'))
    }

    const data = await response.json() as OpenAIChatResponse
    const message = data.choices?.[0]?.message
    const toolCalls = Array.isArray(message?.tool_calls) ? message.tool_calls : []
    if (toolCalls.length) {
      messages.push({
        role: 'assistant',
        content: message?.content || null,
        tool_calls: toolCalls,
      })

      let finalUpdate: AiBookModelUpdate | null = null
      for (const toolCall of toolCalls) {
        const result = executeAiBookToolCall(toolCall, {
          book,
          chapter,
          chapterContent,
          memory,
        })
        messages.push({
          role: 'tool',
          tool_call_id: toolCall.id,
          name: toolCall.function?.name || '',
          content: JSON.stringify(result.content),
        })
        if (result.final && result.raw) {
          finalUpdate = coerceModelUpdate(result.raw, memory, book, chapter)
        }
      }
      if (finalUpdate) return finalUpdate
      continue
    }

    const content = message?.content
    if (content) {
      return coerceModelUpdate(parseJsonContent(content), memory, book, chapter)
    }
  }

  throw new Error('AI 资料生成超过工具调用轮次限制')
}

function executeAiBookToolCall(
  toolCall: AiBookToolCall,
  {
    book,
    chapter,
    chapterContent,
    memory,
  }: {
    book: Book
    chapter: BookChapter
    chapterContent: string
    memory: AiBookMemory
  },
): AiBookToolResult {
  const name = toolCall.function?.name || ''
  const args = parseToolArguments(toolCall.function?.arguments || '{}')
  if (!args.ok) {
    return {
      content: {
        ok: false,
        error: args.error,
      },
    }
  }

  if (name === AI_BOOK_TOOL_GET_MEMORY) {
    return {
      content: {
        ok: true,
        memory: buildAgentMemoryContext(memory),
      },
    }
  }

  if (name === AI_BOOK_TOOL_GET_CHAPTER) {
    return {
      content: {
        ok: true,
        book: {
          name: book.name,
          author: book.author,
          bookUrl: book.bookUrl,
        },
        chapter: {
          index: chapter.index,
          title: chapter.title,
          content: chapterContent.slice(0, 24000),
        },
      },
    }
  }

  if (name === AI_BOOK_TOOL_SAVE_PATCH) {
    const raw = normalizeToolPatch(args.value)
    return {
      final: true,
      raw,
      content: {
        ok: true,
        accepted: true,
      },
    }
  }

  return {
    content: {
      ok: false,
      error: `未知工具：${name}`,
    },
  }
}

function buildAgentMemoryContext(memory: AiBookMemory) {
  return {
    bookUrl: memory.bookUrl,
    bookName: memory.bookName,
    author: memory.author,
    processedChapterIndex: memory.processedChapterIndex,
    processedChapterTitle: memory.processedChapterTitle,
    summary: memory.summary || '',
    worldview: normalizeWorldview(memory.worldview || []),
    characters: normalizeCharacters(memory.characters || []),
    relationships: normalizeRelationships(memory.relationships || []),
    locations: normalizeLocations(memory.locations || []),
    map: memory.map
      ? {
        prompt: memory.map.prompt,
        sourceChapterIndex: memory.map.sourceChapterIndex,
        fallback: memory.map.fallback,
        fallbackReason: memory.map.fallbackReason,
      }
      : null,
    mapDirty: Boolean(memory.mapDirty),
  }
}

function parseToolArguments(input: string): { ok: true; value: UnknownRecord } | { ok: false; error: string } {
  try {
    const parsed = JSON.parse(input || '{}')
    if (!isRecord(parsed)) {
      return { ok: false, error: '工具参数必须是 JSON 对象' }
    }
    return { ok: true, value: parsed }
  } catch (error) {
    return { ok: false, error: `工具参数不是有效 JSON：${(error as Error).message}` }
  }
}

function normalizeToolPatch(args: UnknownRecord): AiBookRawModelUpdate {
  const memory = isRecord(args.memory)
    ? args.memory
    : isRecord(args.memoryPatch)
      ? args.memoryPatch
      : isRecord(args.patch)
        ? args.patch
        : args
  return {
    memory: memory as Partial<AiBookMemory>,
    shouldRegenerateMap: readBoolean(args, 'shouldRegenerateMap') || readBoolean(args, 'mapDirty'),
    mapPrompt: readString(args, 'mapPrompt'),
    mapDirty: readBoolean(args, 'mapDirty'),
  }
}

export async function requestAiBookMapImage({
  config,
  prompt,
  fetchImpl = fetch,
}: GenerateMapParams) {
  if (!isAiBookImageConfigReady(config)) {
    throw new Error('图片模型未配置')
  }

  const response = await requestModelJson({
    config,
    baseUrl: config.imageBaseUrl,
    apiKey: config.imageApiKey,
    fullUrl: config.imageUseFullUrl,
    path: '/v1/images/generations',
    fetchImpl,
    body: {
      model: config.imageModel,
      prompt: buildMapImagePrompt(prompt),
      size: config.imageSize || '1024x1024',
      response_format: 'b64_json',
      n: 1,
    },
  })

  if (!response.ok) {
    throw new Error(await readModelError(response, '地图生成失败'))
  }

  const data = await response.json() as OpenAIImageResponse
  const first = data.data?.[0]
  if (!first?.b64_json && !first?.url) {
    throw new Error('地图生成结果为空')
  }
  return {
    b64Json: first.b64_json,
    imageUrl: first.url,
  }
}

export async function uploadGeneratedMap({
  b64Json,
  imageUrl,
  filename,
  useBackendProxy = false,
  fetchImpl = fetch,
}: UploadGeneratedMapParams) {
  const imageSource = imageUrl || ''
  let blob: Blob
  if (b64Json) {
    blob = base64ToBlob(b64Json, 'image/png')
  } else if (isDataImageUrl(imageSource)) {
    blob = dataUrlToBlob(imageSource)
  } else {
    blob = await fetchImageBlob(imageSource, fetchImpl, useBackendProxy)
  }

  const bytes = new Uint8Array(await blob.arrayBuffer())
  const urls = await invokeEnvelope<string[]>('upload_file', {
    file: bytes,
    fileName: filename,
    fileType: 'ai-maps',
  })
  const url = Array.isArray(urls) ? urls[0] : ''
  if (!url) {
    throw new Error('地图上传结果为空')
  }
  return url
}

export function createEmptyAiBookMemory(book: Book): AiBookMemory {
  return {
    bookUrl: book.bookUrl,
    bookName: book.name,
    author: book.author,
    enabled: false,
    processedChapterIndex: undefined,
    processedChapterTitle: undefined,
    updatedAt: Date.now(),
    summary: '',
    worldview: [],
    characters: [],
    relationships: [],
    locations: [],
    map: null,
    mapDirty: false,
    lastError: undefined,
  }
}

export function applyMapToMemory(memory: AiBookMemory, map: AiBookMap): AiBookMemory {
  return {
    ...memory,
    map,
    mapDirty: false,
    updatedAt: Date.now(),
  }
}

export function applyMapFallbackToMemory(
  memory: AiBookMemory,
  {
    prompt,
    reason,
    sourceChapterIndex,
    updatedAt = Date.now(),
  }: ApplyMapFallbackParams,
): AiBookMemory {
  return {
    ...memory,
    map: {
      prompt,
      updatedAt,
      sourceChapterIndex,
      fallback: 'relationship-graph',
      fallbackReason: reason,
    },
    mapDirty: true,
    updatedAt,
    lastError: undefined,
  }
}

function coerceModelUpdate(raw: AiBookRawModelUpdate, previous: AiBookMemory, book: Book, chapter: BookChapter): AiBookModelUpdate {
  const rawMemory = raw.memory || raw
  const worldviewSource = mergeIncrementalItems(previous.worldview, rawMemory.worldview)
  const characterSource = mergeIncrementalItems(previous.characters, rawMemory.characters)
  const relationshipSource = mergeIncrementalItems(previous.relationships, rawMemory.relationships)
  const locationSource = mergeIncrementalItems(previous.locations, rawMemory.locations)
  const worldview = normalizeWorldview(worldviewSource)
  const characters = normalizeCharacters(characterSource)
  const relationships = normalizeRelationships(relationshipSource)
  const locations = normalizeLocations(locationSource)
  const mapPrompt = typeof raw.mapPrompt === 'string' ? raw.mapPrompt.trim() : ''
  const shouldRegenerateMap = shouldAcceptMapRegeneration({
    requested: Boolean(raw.shouldRegenerateMap || raw.mapDirty),
    mapPrompt,
    previous,
    locations,
  })
  const memory: AiBookMemory = {
    ...previous,
    ...rawMemory,
    bookUrl: book.bookUrl,
    bookName: book.name,
    author: book.author,
    enabled: previous.enabled,
    processedChapterIndex: chapter.index,
    processedChapterTitle: chapter.title,
    updatedAt: Date.now(),
    summary: normalizeSummary(rawMemory.summary, previous.summary),
    worldview,
    characters,
    relationships,
    locations,
    map: previous.map || null,
    mapDirty: shouldRegenerateMap,
    lastError: undefined,
  }

  return {
    memory,
    shouldRegenerateMap,
    mapPrompt: shouldRegenerateMap ? mapPrompt : undefined,
  }
}

type UnknownRecord = Record<string, unknown>

function mergeIncrementalItems(previousItems: unknown[] | undefined, nextItems: unknown) {
  const previousArray = Array.isArray(previousItems) ? previousItems : []
  return Array.isArray(nextItems) ? [...previousArray, ...nextItems] : previousArray
}

function normalizeSummary(nextSummary: unknown, previousSummary: string | undefined) {
  const previous = (previousSummary || '').trim()
  if (typeof nextSummary !== 'string') return previous

  const next = nextSummary.trim()
  if (!next) return limitSummaryLength(previous)
  if (!previous) return limitSummaryLength(stripSingleChapterSummaryHeading(next))
  if (startsWithSingleChapterSummary(next)) return limitSummaryLength(previous)
  return limitSummaryLength(next)
}

function startsWithSingleChapterSummary(value: string) {
  return /^(?:本章|本节|这一章)[：:，,]/.test(value.trim())
    || /^第\s*(?:\d+|[零〇一二两三四五六七八九十百千万]+)\s*[章节回话卷篇][^。！？；]{0,40}[：:]/.test(value.trim())
}

function stripSingleChapterSummaryHeading(value: string) {
  return value
    .trim()
    .replace(/^(?:本章|本节|这一章)[：:，,]\s*/, '')
    .replace(/^第\s*(?:\d+|[零〇一二两三四五六七八九十百千万]+)\s*[章节回话卷篇][^。！？；]{0,40}[：:]\s*/, '')
    .trim()
}

function limitSummaryLength(value: string) {
  const chars = [...value.trim()]
  if (chars.length <= MAX_AI_BOOK_SUMMARY_CHARS) return value.trim()

  const headLength = Math.floor((MAX_AI_BOOK_SUMMARY_CHARS - 2) * 0.55)
  const tailLength = MAX_AI_BOOK_SUMMARY_CHARS - 2 - headLength
  return `${chars.slice(0, headLength).join('')}……${chars.slice(-tailLength).join('')}`
}

function shouldAcceptMapRegeneration({
  requested,
  mapPrompt,
  previous,
  locations,
}: {
  requested: boolean
  mapPrompt: string
  previous: AiBookMemory
  locations: AiBookLocation[]
}) {
  if (!requested || !mapPrompt) return false
  if (!previous.map) return true
  return locationSignature(normalizeLocations(previous.locations || [])) !== locationSignature(locations)
}

function locationSignature(locations: AiBookLocation[]) {
  return locations
    .map((location) => [
      normalizeKey(location.name),
      normalizeKey(location.parentName),
      normalizeKey(location.kind),
      normalizeKey(location.status),
      normalizeKey(location.description),
    ].join(':'))
    .sort()
    .join('|')
}

function normalizeWorldview(items: unknown[]): AiBookNote[] {
  const notes = new Map<string, AiBookNote>()
  for (const item of items) {
    if (!isRecord(item) || isLowImportance(readString(item, 'importance'))) continue
    const title = readString(item, 'title')
    const content = readString(item, 'content')
    if (!title || !content) continue
    const category = readString(item, 'category') || '基础设定'
    if (isChapterSummaryWorldview(title, content, category)) continue
    const note: AiBookNote = {
      title,
      content,
      category,
      confidence: readString(item, 'confidence') || undefined,
      importance: readString(item, 'importance') || undefined,
    }
    const key = `${normalizeKey(category)}::${normalizeKey(title)}`
    const existing = notes.get(key)
    notes.set(key, existing ? mergeNote(existing, note) : note)
  }
  return [...notes.values()]
}

function isChapterSummaryWorldview(title: string, content: string, category: string) {
  const categoryKey = normalizeKey(category)
  const contentKey = normalizeKey(content)
  if (/(本章|章节|剧情|简介|概要|经过|第\d+章|第[一二三四五六七八九十百千万]+章)/.test(title)) {
    return true
  }
  if (['当前事件', '章节摘要', '剧情进展', '本章剧情'].some((term) => categoryKey.includes(normalizeKey(term)))) {
    return true
  }
  if (/^(本章|本节|这一章|此章|第.+章)/.test(content.trim())) {
    return true
  }
  if (isNarrativeRecapText(title, content)) {
    return true
  }
  const plotVerbs = ['搜查', '担心', '登上', '指出', '加入', '透露', '引出', '随后']
  const plotHits = plotVerbs.filter((term) => contentKey.includes(normalizeKey(term))).length
  return content.length > 80 && plotHits >= 3 && !isSettingCategory(category)
}

function isNarrativeRecapText(title: string, content: string) {
  const trimmed = content.trim()
  const combined = `${title} ${trimmed}`
  const normalized = normalizeKey(combined)
  const sentenceCount = trimmed
    .split(/[。！？；]/)
    .filter((part) => part.trim().length > 0)
    .length
  const chapterReference = /第\s*(?:\d+|[零〇一二两三四五六七八九十百千万]+)\s*[章节回话卷篇]/.test(combined)
    || ['本章', '这一章', '当前章节', '章节内容'].some((term) => normalized.includes(normalizeKey(term)))
  const narrativeTerms = [
    '随后',
    '然后',
    '接着',
    '回到',
    '看到',
    '告诉',
    '解释',
    '拒绝',
    '催促',
    '等待',
    '躺在',
    '坐在',
    '担心',
    '考虑',
    '讲述',
    '寻找',
    '清晨',
    '下午',
    '晚上',
    '第二天',
  ]
  const narrativeHits = narrativeTerms.filter((term) => normalized.includes(normalizeKey(term))).length
  return (chapterReference && trimmed.length > 60 && narrativeHits >= 2)
    || (trimmed.length > 140 && sentenceCount >= 4 && narrativeHits >= 4)
}

function isSettingCategory(category: string) {
  const key = normalizeKey(category)
  return [
    '基础规则',
    '基础设定',
    '势力制度',
    '历史传说',
    '技术魔法',
    '社会文化',
    '地理环境',
    '组织体系',
    '未确认信息',
  ].some((term) => key.includes(normalizeKey(term)))
}

function normalizeCharacters(items: unknown[]): AiBookCharacter[] {
  const characters = new Map<string, AiBookCharacter>()
  for (const item of items) {
    if (!isRecord(item) || isLowImportance(readString(item, 'importance'))) continue
    const name = readString(item, 'name')
    if (!name) continue
    const character: AiBookCharacter = {
      name,
      aliases: uniqueStrings(readStringArray(item, 'aliases')),
      status: readString(item, 'status') || readString(item, 'description') || '状态未知',
      faction: readString(item, 'faction') || undefined,
      location: readString(item, 'location') || undefined,
      description: readString(item, 'description') || undefined,
      lastSeenChapter: readString(item, 'lastSeenChapter') || undefined,
      importance: readString(item, 'importance') || undefined,
    }
    const key = normalizeKey(name)
    const existing = characters.get(key)
    characters.set(key, existing ? mergeCharacter(existing, character) : character)
  }
  return [...characters.values()]
}

function normalizeRelationships(items: unknown[]): AiBookRelationship[] {
  const relationships = new Map<string, AiBookRelationship>()
  for (const item of items) {
    if (!isRecord(item) || isLowImportance(readString(item, 'importance'))) continue
    const source = readString(item, 'source')
    const target = readString(item, 'target')
    const relation = readString(item, 'relation')
    const description = readString(item, 'description')
    const status = readString(item, 'status')
    const importance = readString(item, 'importance')
    if (!source || !target || !relation) continue
    if (normalizeKey(source) === normalizeKey(target)) continue
    if (isLowValueRelationship(relation, description || status, importance)) continue
    const relationship: AiBookRelationship = {
      source,
      target,
      relation,
      status: status || undefined,
      description: description || undefined,
      importance: importance || undefined,
    }
    const key = relationshipKey(source, target, relation)
    const existing = relationships.get(key)
    relationships.set(key, existing ? mergeRelationship(existing, relationship) : relationship)
  }
  return [...relationships.values()]
}

function normalizeLocations(items: unknown[]): AiBookLocation[] {
  const locations = new Map<string, AiBookLocation>()
  for (const item of items) {
    if (!isRecord(item)) continue
    const name = readString(item, 'name')
    if (!name) continue
    const parentName = readString(item, 'parentName')
    const location: AiBookLocation = {
      name,
      kind: readString(item, 'kind') || undefined,
      parentName: parentName && normalizeKey(parentName) !== normalizeKey(name) ? parentName : undefined,
      description: readString(item, 'description') || readString(item, 'status') || '',
      status: readString(item, 'status') || undefined,
      relatedCharacters: uniqueStrings(readStringArray(item, 'relatedCharacters')),
      firstSeenChapter: readString(item, 'firstSeenChapter') || undefined,
      importance: readString(item, 'importance') || undefined,
    }
    const key = normalizeKey(name)
    const existing = locations.get(key)
    locations.set(key, existing ? mergeLocation(existing, location) : location)
  }
  return [...locations.values()]
}

function mergeNote(current: AiBookNote, next: AiBookNote): AiBookNote {
  return {
    ...current,
    content: richerString(current.content, next.content),
    confidence: preferString(current.confidence, next.confidence),
    importance: preferImportance(current.importance, next.importance),
  }
}

function mergeCharacter(current: AiBookCharacter, next: AiBookCharacter): AiBookCharacter {
  return {
    ...current,
    aliases: uniqueStrings([...(current.aliases || []), ...(next.aliases || [])]),
    status: next.status || current.status,
    faction: next.faction || current.faction,
    location: next.location || current.location,
    description: richerString(current.description, next.description),
    lastSeenChapter: next.lastSeenChapter || current.lastSeenChapter,
    importance: preferImportance(current.importance, next.importance),
  }
}

function mergeRelationship(current: AiBookRelationship, next: AiBookRelationship): AiBookRelationship {
  return {
    ...current,
    status: next.status || current.status,
    description: richerString(current.description, next.description),
    importance: preferImportance(current.importance, next.importance),
  }
}

function mergeLocation(current: AiBookLocation, next: AiBookLocation): AiBookLocation {
  return {
    ...current,
    kind: next.kind || current.kind,
    parentName: next.parentName || current.parentName,
    description: richerString(current.description, next.description),
    status: next.status || current.status,
    relatedCharacters: uniqueStrings([...(current.relatedCharacters || []), ...(next.relatedCharacters || [])]),
    firstSeenChapter: current.firstSeenChapter || next.firstSeenChapter,
    importance: preferImportance(current.importance, next.importance),
  }
}

function isRecord(value: unknown): value is UnknownRecord {
  return Boolean(value && typeof value === 'object' && !Array.isArray(value))
}

function readString(record: UnknownRecord, key: string) {
  const value = record[key]
  return typeof value === 'string' ? value.trim() : ''
}

function readStringArray(record: UnknownRecord, key: string) {
  const value = record[key]
  if (!Array.isArray(value)) return []
  return value
    .filter((item): item is string => typeof item === 'string')
    .map((item) => item.trim())
    .filter(Boolean)
}

function readBoolean(record: UnknownRecord, key: string) {
  return record[key] === true
}

function uniqueStrings(values: string[]) {
  const seen = new Set<string>()
  const result: string[] = []
  for (const value of values) {
    const key = normalizeKey(value)
    if (!key || seen.has(key)) continue
    seen.add(key)
    result.push(value)
  }
  return result
}

function normalizeKey(value: string | undefined) {
  return (value || '')
    .trim()
    .toLowerCase()
    .replace(/[·•・]/g, '.')
    .replace(/\s+/g, '')
}

function isLowImportance(value: string | undefined) {
  const normalized = normalizeKey(value || '')
  if (!normalized) return false
  return [
    'low',
    '低',
    '低重要性',
    '不重要',
    '路人',
    '背景',
    'minor',
    'background',
    'oneoff',
    '一次性',
  ].some((term) => normalized.includes(term))
}

function isLowValueRelationship(relation: string, detail: string, importance: string) {
  const normalizedImportance = normalizeKey(importance)
  if (normalizedImportance.includes('high') || normalizedImportance.includes('medium') || normalizedImportance.includes('高') || normalizedImportance.includes('中')) {
    return false
  }
  const normalizedRelation = normalizeKey(relation)
  if (!['认识', '见过', '路过', '同村', '同校', '位于', '相关'].includes(normalizedRelation)) {
    return false
  }
  return normalizeKey(detail).length < 18
}

function relationshipKey(source: string, target: string, relation: string) {
  const pair = [normalizeKey(source), normalizeKey(target)].sort().join('::')
  return `${pair}::${normalizeKey(relation)}`
}

function richerString(current: string | undefined, next: string | undefined) {
  if (!current) return next || ''
  if (!next) return current
  return next.length > current.length ? next : current
}

function preferString(current: string | undefined, next: string | undefined) {
  return current || next || undefined
}

function preferImportance(current: string | undefined, next: string | undefined) {
  return importanceRank(next) > importanceRank(current) ? next : current || next
}

function importanceRank(value: string | undefined) {
  const normalized = normalizeKey(value || '')
  if (normalized.includes('high') || normalized.includes('高')) return 3
  if (normalized.includes('medium') || normalized.includes('中')) return 2
  if (isLowImportance(value)) return 1
  return 0
}

function buildModelHeaders(apiKey: string) {
  const headers: Record<string, string> = {
    'Content-Type': 'application/json',
  }
  if (apiKey.trim()) {
    headers.Authorization = `Bearer ${apiKey.trim()}`
  }
  return headers
}

async function requestModelJson({
  config,
  baseUrl,
  apiKey,
  fullUrl,
  path,
  body,
  fetchImpl,
}: AiProxyRequestParams) {
  if (config.modelSource === 'server') {
    return aiProxyResponse(await invokeRaw<AiProxyResult>('ai_proxy', {
      req: {
        useServerConfig: true,
        kind: path === '/v1/images/generations' ? 'image' : 'text',
        path,
        body,
      },
    }))
  }

  const endpointUrl = fullUrl ? normalizeBaseUrl(baseUrl) : `${normalizeBaseUrl(baseUrl)}${path}`
  if (config.useBackendProxy) {
    return aiProxyResponse(await invokeRaw<AiProxyResult>('ai_proxy', {
      req: {
        baseUrl: normalizeBaseUrl(baseUrl),
        apiKey: apiKey.trim(),
        path,
        fullUrl,
        body,
      },
    }))
  }

  return fetchImpl(endpointUrl, {
    method: 'POST',
    headers: buildModelHeaders(apiKey),
    body: JSON.stringify(body),
  })
}

function normalizeBaseUrl(url: string) {
  return url.trim().replace(/\/+$/, '')
}

function buildMapImagePrompt(prompt: string) {
  const sourcePrompt = prompt.trim() || '根据已读进度中的已知地点绘制小说世界地图。'
  return [
    '请生成一张小说世界地图，而不是场景插画。',
    '画面类型：俯视地图（top-down / orthographic map）、二维制图、设定集地图。',
    '必须表现：区域边界、道路或虚线连接、地形/空间分区、地图符号、地点标签、图例、罗盘或比例尺感。',
    '地点呈现方式：机房、避难所等室内或建筑地点只能表现为地图上的标注区域、平面轮廓或小图标。',
    '禁止内容：不要生成写实照片、电影截图、建筑外观特写、室内房间透视图、服务器机柜照片、避难所入口照片。',
    '不要画人物，不要把地点画成可进入的真实建筑场景，不要用巨大门牌或数字替代地图标注。',
    '构图要求：清晰分区，路线关系可读，整体像游戏世界地图、桌面 RPG 区域地图或小说设定集地图。',
    `原始地图信息：${sourcePrompt}`,
  ].join('\n')
}

function parseJsonContent(content: string): AiBookRawModelUpdate {
  const trimmed = content.trim()
  const json = extractFirstJsonObject(trimmed)
  try {
    return JSON.parse(json) as AiBookRawModelUpdate
  } catch (error) {
    throw new Error(`AI 资料生成结果不是有效 JSON：${(error as Error).message}`)
  }
}

function extractFirstJsonObject(content: string) {
  const text = content
    .replace(/^```(?:json)?\s*/i, '')
    .trim()
  const start = text.indexOf('{')
  if (start < 0) {
    throw new Error('AI 资料生成结果未包含 JSON 对象')
  }

  let depth = 0
  let inString = false
  let escaped = false
  for (let index = start; index < text.length; index += 1) {
    const char = text[index]
    if (inString) {
      if (escaped) {
        escaped = false
      } else if (char === '\\') {
        escaped = true
      } else if (char === '"') {
        inString = false
      }
      continue
    }

    if (char === '"') {
      inString = true
    } else if (char === '{') {
      depth += 1
    } else if (char === '}') {
      depth -= 1
      if (depth === 0) {
        return text.slice(start, index + 1)
      }
    }
  }

  throw new Error('AI 资料生成结果 JSON 对象不完整')
}

async function readModelError(response: Response, fallback: string) {
  try {
    const contentType = response.headers.get('content-type') || ''
    if (contentType.includes('application/json')) {
      const data = await response.json() as {
        error?: { message?: string }
        errorMsg?: string
      }
      return data.error?.message || data.errorMsg || `${fallback} (${response.status})`
    }
    const text = await response.text()
    return summarizeHttpErrorBody(text, { fallback, status: response.status })
  } catch {
    return `${fallback} (${response.status})`
  }
}

function base64ToBlob(value: string, contentType: string) {
  const binary = atob(value)
  const bytes = new Uint8Array(binary.length)
  for (let index = 0; index < binary.length; index += 1) {
    bytes[index] = binary.charCodeAt(index)
  }
  return new Blob([bytes], { type: contentType })
}

function isDataImageUrl(value: string) {
  return value.trim().toLowerCase().startsWith('data:image/')
}

function dataUrlToBlob(value: string) {
  const dataUrl = value.trim()
  const commaIndex = dataUrl.indexOf(',')
  if (commaIndex < 0 || !dataUrl.toLowerCase().startsWith('data:')) {
    throw new Error('地图图片 data URL 无效')
  }

  const metadata = dataUrl.slice(5, commaIndex)
  const data = dataUrl.slice(commaIndex + 1)
  const parts = metadata.split(';').filter(Boolean)
  const contentType = parts[0] || 'text/plain'
  if (!contentType.toLowerCase().startsWith('image/')) {
    throw new Error('地图图片 data URL 不是图片')
  }
  if (parts.some((part) => part.toLowerCase() === 'base64')) {
    return base64ToBlob(data.trim(), contentType)
  }
  return new Blob([decodeURIComponent(data)], { type: contentType })
}

async function fetchImageBlob(imageUrl: string, fetchImpl: typeof fetch, useBackendProxy: boolean) {
  if (!imageUrl) {
    throw new Error('地图图片地址为空')
  }
  const response = useBackendProxy
    ? aiProxyResponse(await invokeRaw<AiProxyResult>('ai_proxy_image', {
      req: { url: imageUrl },
    }))
    : await fetchImpl(imageUrl)
  if (!response.ok) {
    throw new Error(await readModelError(response, '地图图片下载失败'))
  }
  return response.blob()
}
