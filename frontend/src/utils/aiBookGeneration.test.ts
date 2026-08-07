import { afterEach, describe, expect, it, vi } from 'vitest'
import { invoke } from '@tauri-apps/api/core'
import type { AiBookConfig, AiBookMemory } from '../types'
import {
  applyMapFallbackToMemory,
  buildAiBookPromptMessages,
  requestAiBookMemoryUpdate,
  requestAiBookMapImage,
  shouldRunAiBookAutoUpdate,
  uploadGeneratedMap,
} from './aiBookGeneration'

vi.mock('@tauri-apps/api/core', () => ({
  invoke: vi.fn(),
}))

afterEach(() => {
  vi.clearAllMocks()
})

const readyConfig: AiBookConfig = {
  modelSource: 'browser',
  textBaseUrl: 'http://localhost:8825',
  textApiKey: '',
  textModel: 'gpt-4o-mini',
  textUseFullUrl: false,
  imageBaseUrl: 'http://localhost:8826',
  imageApiKey: 'image-key',
  imageModel: 'gpt-image-1',
  imageSize: '1024x1024',
  imageUseFullUrl: false,
  useBackendProxy: false,
}

describe('aiBookGeneration', () => {
  it('skips auto update without config, disabled memory, or already processed chapter', () => {
    const memory: AiBookMemory = {
      bookUrl: 'book-1',
      enabled: true,
      processedChapterIndex: 4,
      worldview: [],
      characters: [],
      relationships: [],
      locations: [],
      updatedAt: 0,
    }

    expect(shouldRunAiBookAutoUpdate(memory, 5, { ...readyConfig, textBaseUrl: '' })).toBe(false)
    expect(shouldRunAiBookAutoUpdate({ ...memory, enabled: false }, 5, readyConfig)).toBe(false)
    expect(shouldRunAiBookAutoUpdate(memory, 4, readyConfig)).toBe(false)
    expect(shouldRunAiBookAutoUpdate(memory, 5, readyConfig)).toBe(true)
  })

  it('builds spoiler-safe incremental memory prompts', () => {
    const messages = buildAiBookPromptMessages({
      bookName: '山海旧事',
      chapterTitle: '第八章 北境',
      chapterIndex: 7,
      chapterContent: '主角抵达北境，只知道旧神传说真假未明。',
      memory: {
        bookUrl: 'book-1',
        enabled: true,
        processedChapterIndex: 6,
        summary: '主角离开帝都。',
        worldview: [],
        characters: [],
        relationships: [],
        locations: [],
        updatedAt: 0,
      },
    })

    const serialized = JSON.stringify(messages)
    expect(serialized).toContain('不得使用未读章节')
    expect(serialized).toContain('必须通过工具按需读取当前资料和本次已完成章节')
    expect(serialized).toContain('save_memory_patch')
    expect(serialized).toContain('第八章 北境')
    expect(serialized).not.toContain('主角抵达北境，只知道旧神传说真假未明。')
    expect(serialized).not.toContain('previousMemory')
    expect(serialized).toContain('累计剧情摘要')
    expect(serialized).toContain('300-800 字')
    expect(serialized).toContain('不要逐章累加')
    expect(serialized).toContain('禁止以“本章”“第X章”“章节名：”开头')
    expect(serialized).toContain('mapPrompt')
    expect(serialized).toContain('俯视地图')
    expect(serialized).toContain('不要写成场景照片')
    expect(serialized).toContain('category')
    expect(serialized).toContain('worldview 不是章节简介')
    expect(serialized).toContain('worldview 必须输出 []')
    expect(serialized).toContain('宁可为空')
    expect(serialized).toContain('parentName')
    expect(serialized).toContain('国家 > 区域/郡 > 城市')
    expect(serialized).toContain('禁止把国家挂在城市下面')
    expect(serialized).toContain('importance')
    expect(serialized).toContain('不要输出不重要')
    expect(serialized).toContain('人物关系必须去重')
  })

  it('runs memory updates through OpenAI-compatible tool calling', async () => {
    const fetchMock = vi
      .fn()
      .mockResolvedValueOnce({
        ok: true,
        json: async () => ({
          choices: [{
            message: {
              tool_calls: [
                {
                  id: 'call-memory',
                  type: 'function',
                  function: { name: 'get_current_memory', arguments: '{}' },
                },
                {
                  id: 'call-chapter',
                  type: 'function',
                  function: { name: 'get_completed_chapter', arguments: '{}' },
                },
              ],
            },
          }],
        }),
      })
      .mockResolvedValueOnce({
        ok: true,
        json: async () => ({
          choices: [{
            message: {
              tool_calls: [
                {
                  id: 'call-save',
                  type: 'function',
                  function: {
                    name: 'save_memory_patch',
                    arguments: JSON.stringify({
                      memory: {
                        summary: '主角确认超凡领域存在。',
                        worldview: [
                          {
                            category: '基础规则',
                            title: '超凡领域',
                            content: '存在以特殊能力影响现实的超凡领域，细节仍未公开。',
                            confidence: '已知',
                            importance: 'high',
                          },
                        ],
                        characters: [],
                        relationships: [],
                        locations: [],
                      },
                      shouldRegenerateMap: false,
                    }),
                  },
                },
              ],
            },
          }],
        }),
      })

    const update = await requestAiBookMemoryUpdate({
      config: readyConfig,
      book: { name: '诡秘之主', author: '爱潜水的乌贼', bookUrl: 'book-1', origin: 'source-1' },
      chapter: { title: '第十一章', url: 'chapter-11', index: 10 },
      chapterContent: '刘隆指出李皓已经接触超凡领域。',
      memory: {
        bookUrl: 'book-1',
        enabled: true,
        updatedAt: 0,
        summary: '旧资料摘要',
        worldview: [],
        characters: [],
        relationships: [],
        locations: [],
      },
      fetchImpl: fetchMock as unknown as typeof fetch,
    })

    expect(fetchMock).toHaveBeenCalledTimes(2)
    const firstBody = JSON.parse(String((fetchMock.mock.calls[0]?.[1] as RequestInit).body))
    expect(firstBody.tools.map((tool: { function: { name: string } }) => tool.function.name)).toEqual([
      'get_current_memory',
      'get_completed_chapter',
      'save_memory_patch',
    ])
    expect(JSON.stringify(firstBody.messages)).not.toContain('旧资料摘要')
    expect(JSON.stringify(firstBody.messages)).not.toContain('刘隆指出李皓已经接触超凡领域')

    const secondBody = JSON.parse(String((fetchMock.mock.calls[1]?.[1] as RequestInit).body))
    expect(JSON.stringify(secondBody.messages)).toContain('旧资料摘要')
    expect(JSON.stringify(secondBody.messages)).toContain('刘隆指出李皓已经接触超凡领域')
    expect(update.memory.summary).toBe('主角确认超凡领域存在。')
    expect(update.memory.worldview[0]).toMatchObject({
      category: '基础规则',
      title: '超凡领域',
    })
  })

  it('accepts model JSON content with trailing explanation text', async () => {
    const modelPayload = {
      memory: {
        summary: '主角抵达北境。',
        worldview: [],
        characters: [],
        relationships: [],
        locations: [],
      },
      shouldRegenerateMap: false,
    }
    const fetchMock = vi.fn(async (_url: RequestInfo | URL, _init?: RequestInit) => ({
      ok: true,
      json: async () => ({
        choices: [{
          message: {
            content: `${JSON.stringify(modelPayload, null, 2)}\n\n说明：已按当前章节更新。`,
          },
        }],
      }),
    }))

    const update = await requestAiBookMemoryUpdate({
      config: readyConfig,
      book: { name: '山海旧事', author: '佚名', bookUrl: 'book-1', origin: 'source-1' },
      chapter: { title: '第八章', url: 'chapter-8', index: 7 },
      chapterContent: '主角抵达北境。',
      memory: {
        bookUrl: 'book-1',
        enabled: true,
        updatedAt: 0,
        worldview: [],
        characters: [],
        relationships: [],
        locations: [],
      },
      fetchImpl: fetchMock as unknown as typeof fetch,
    })

    expect(update.memory.summary).toBe('主角抵达北境。')
    expect(update.memory.processedChapterIndex).toBe(7)
  })

  it('normalizes important model memory and removes duplicate relationships', async () => {
    const modelPayload = {
      memory: {
        summary: '克莱恩开始熟悉廷根。',
        worldview: [
          { title: '非凡力量', content: '存在超凡能力但细节未明。', confidence: '已知' },
          { category: '基础规则', title: '非凡力量', content: '存在超凡能力，来源仍未确认。', confidence: '推断' },
        ],
        characters: [
          { name: '克莱恩', status: '正在适应新身份', description: '主角', importance: 'high' },
          { name: '克莱恩', status: '正在适应新身份并调查线索', aliases: ['周明瑞'], importance: 'high' },
          { name: '路人店员', status: '卖过面包', importance: 'low' },
        ],
        relationships: [
          { source: '克莱恩', target: '梅丽莎', relation: '兄妹', description: '共同生活，互相关心。', importance: 'high' },
          { source: '梅丽莎', target: '克莱恩', relation: '兄妹', description: '梅丽莎关心哥哥的异常。', importance: 'high' },
          { source: '克莱恩', target: '路人店员', relation: '认识', description: '买过东西', importance: 'low' },
        ],
        locations: [
          { name: '廷根市', kind: '城市', description: '北大陆城市。', importance: 'high' },
          { name: '莫雷蒂公寓', parentName: '廷根市', kind: '住宅', description: '莫雷蒂一家居住地。', relatedCharacters: ['克莱恩'] },
          { name: '莫雷蒂公寓', parentName: '廷根市', kind: '住宅', description: '包含书桌和卧室的两居室公寓。', relatedCharacters: ['梅丽莎'] },
        ],
      },
      shouldRegenerateMap: false,
    }
    const fetchMock = vi.fn(async (_url: RequestInfo | URL, _init?: RequestInit) => ({
      ok: true,
      json: async () => ({
        choices: [{
          message: {
            content: JSON.stringify(modelPayload),
          },
        }],
      }),
    }))

    const update = await requestAiBookMemoryUpdate({
      config: readyConfig,
      book: { name: '诡秘之主', author: '爱潜水的乌贼', bookUrl: 'book-1', origin: 'source-1' },
      chapter: { title: '第九章', url: 'chapter-9', index: 8 },
      chapterContent: '克莱恩回到廷根市的莫雷蒂公寓。',
      memory: {
        bookUrl: 'book-1',
        enabled: true,
        updatedAt: 0,
        worldview: [],
        characters: [],
        relationships: [],
        locations: [],
      },
      fetchImpl: fetchMock as unknown as typeof fetch,
    })

    expect(update.memory.worldview.map((item) => item.category)).toEqual(['基础设定', '基础规则'])
    expect(update.memory.characters.map((item) => item.name)).toEqual(['克莱恩'])
    expect(update.memory.characters[0].aliases).toEqual(['周明瑞'])
    expect(update.memory.characters[0].status).toBe('正在适应新身份并调查线索')
    expect(update.memory.relationships).toHaveLength(1)
    expect(update.memory.relationships[0]).toMatchObject({
      source: '克莱恩',
      target: '梅丽莎',
      relation: '兄妹',
    })
    expect(update.memory.locations).toHaveLength(2)
    expect(update.memory.locations.find((item) => item.name === '莫雷蒂公寓')).toMatchObject({
      parentName: '廷根市',
      description: '包含书桌和卧室的两居室公寓。',
      relatedCharacters: ['克莱恩', '梅丽莎'],
    })
  })

  it('filters chapter recap content out of worldview notes', async () => {
    const fetchMock = vi.fn(async (_url: RequestInfo | URL, _init?: RequestInit) => ({
      ok: true,
      json: async () => ({
        choices: [{
          message: {
            content: JSON.stringify({
              memory: {
                summary: '执法队搜查无果，李皓开始接触超凡线索。',
                worldview: [
                  {
                    category: '基础设定',
                    title: '本章（第11章）执法队搜查',
                    content: '本章执法队搜查张家老屋一无所获，刘隆下令拆屋烧屋以引出幕后势力。',
                    confidence: '已知',
                    importance: 'high',
                  },
                  {
                    category: '基础设定',
                    title: '梦境与巫师线索',
                    content: '卢米安坐在屋顶沉思，他一直渴望获得超凡力量但奥萝尔拒绝教他，称这条路危险痛苦。回到房间后看到奥萝尔在用香槟金色钢笔给笔友写信，奥萝尔解释笔友是通过报纸专栏等认识的书信朋友，其中有厉害的人，电池灯就是笔友送的。卢米安躺在床上担心奥萝尔的秘密带来危险。随后卢米安反复做灰色雾气的梦，无论往哪走都会回到自己的卧室，频率越来越高几乎每天都会做。清晨卢米安告诉奥萝尔又做那个梦了，奥萝尔说之前的方案没用，考虑给他找一个真正的催眠师。卢米安想成为巫师解开梦境秘密，奥萝尔拒绝并说这个世界变得越来越危险，催促他准备考试。',
                    confidence: '已知',
                    importance: 'high',
                  },
                  {
                    category: '基础规则',
                    title: '超凡领域',
                    content: '存在普通执法体系之外的超凡领域，接触者可能成为重点目标。',
                    confidence: '推断',
                    importance: 'high',
                  },
                ],
                characters: [],
                relationships: [],
                locations: [],
              },
              shouldRegenerateMap: false,
            }),
          },
        }],
      }),
    }))

    const update = await requestAiBookMemoryUpdate({
      config: readyConfig,
      book: { name: '星门', author: '老鹰吃小鸡', bookUrl: 'book-1', origin: 'source-1' },
      chapter: { title: '第十一章', url: 'chapter-11', index: 10 },
      chapterContent: '执法队搜查张家老屋。',
      memory: {
        bookUrl: 'book-1',
        enabled: true,
        updatedAt: 0,
        worldview: [],
        characters: [],
        relationships: [],
        locations: [],
      },
      fetchImpl: fetchMock as unknown as typeof fetch,
    })

    expect(update.memory.worldview.map((item) => item.title)).toEqual(['超凡领域'])
    expect(update.memory.summary).toBe('执法队搜查无果，李皓开始接触超凡线索。')
  })

  it('merges incremental model memory with existing book memory', async () => {
    const fetchMock = vi.fn(async (_url: RequestInfo | URL, _init?: RequestInit) => ({
      ok: true,
      json: async () => ({
        choices: [{
          message: {
            content: JSON.stringify({
              memory: {
                summary: '第十章「北境」：林舟离开旧村，抵达北境。',
                worldview: [
                  { category: '地理环境', title: '北境', content: '北境是寒冷边境区域，已出现新的线索。', confidence: '已知' },
                ],
                characters: [
                  { name: '林舟', status: '已离开旧村', location: '北境', importance: 'high' },
                  { name: '沈月', status: '在北境提供帮助', importance: 'medium' },
                ],
                relationships: [
                  { source: '林舟', target: '沈月', relation: '临时同伴', description: '两人在北境同行。', importance: 'medium' },
                ],
                locations: [
                  { name: '北境', kind: '区域', description: '寒冷边境。', importance: 'high' },
                ],
              },
              shouldRegenerateMap: false,
            }),
          },
        }],
      }),
    }))

    const update = await requestAiBookMemoryUpdate({
      config: readyConfig,
      book: { name: '山海旧事', author: '佚名', bookUrl: 'book-1', origin: 'source-1' },
      chapter: { title: '第十章', url: 'chapter-10', index: 9 },
      chapterContent: '林舟离开旧村抵达北境。',
      memory: {
        bookUrl: 'book-1',
        enabled: true,
        updatedAt: 0,
        summary: '主角仍在旧村。',
        worldview: [
          { category: '基础设定', title: '灵脉', content: '灵脉会影响修行。', confidence: '已知' },
        ],
        characters: [
          { name: '林舟', status: '停留在旧村', location: '旧村', importance: 'high' },
        ],
        relationships: [
          { source: '林舟', target: '村长', relation: '师徒', description: '村长曾指导林舟。', importance: 'medium' },
        ],
        locations: [
          { name: '旧村', kind: '村落', description: '故事开始的村落。', importance: 'high' },
        ],
      },
      fetchImpl: fetchMock as unknown as typeof fetch,
    })

    expect(update.memory.summary).toBe('主角仍在旧村。')
    expect(update.memory.worldview.map((item) => item.title)).toEqual(['灵脉', '北境'])
    expect(update.memory.characters.map((item) => item.name)).toEqual(['林舟', '沈月'])
    expect(update.memory.characters.find((item) => item.name === '林舟')).toMatchObject({
      status: '已离开旧村',
      location: '北境',
    })
    expect(update.memory.relationships.map((item) => `${item.source}-${item.relation}-${item.target}`)).toEqual([
      '林舟-师徒-村长',
      '林舟-临时同伴-沈月',
    ])
    expect(update.memory.locations.map((item) => item.name)).toEqual(['旧村', '北境'])
  })

  it('keeps cumulative summaries bounded instead of growing by chapter', async () => {
    const longSummary = `主线开端。${'主角持续调查线索，局势逐步升级。'.repeat(120)}当前进展仍集中在北境。`
    const fetchMock = vi.fn(async (_url: RequestInfo | URL, _init?: RequestInit) => ({
      ok: true,
      json: async () => ({
        choices: [{
          message: {
            content: JSON.stringify({
              memory: {
                summary: longSummary,
                worldview: [],
                characters: [],
                relationships: [],
                locations: [],
              },
              shouldRegenerateMap: false,
            }),
          },
        }],
      }),
    }))

    const update = await requestAiBookMemoryUpdate({
      config: readyConfig,
      book: { name: '山海旧事', author: '佚名', bookUrl: 'book-1', origin: 'source-1' },
      chapter: { title: '第一千章', url: 'chapter-1000', index: 999 },
      chapterContent: '主角继续调查。',
      memory: {
        bookUrl: 'book-1',
        enabled: true,
        updatedAt: 0,
        summary: '旧累计摘要。',
        worldview: [],
        characters: [],
        relationships: [],
        locations: [],
      },
      fetchImpl: fetchMock as unknown as typeof fetch,
    })

    expect([...update.memory.summary || '']).toHaveLength(1200)
    expect(update.memory.summary).toContain('……')
    expect(update.memory.summary).toContain('主线开端')
    expect(update.memory.summary).toContain('当前进展仍集中在北境')
  })

  it('does not regenerate the world map when requested without location changes', async () => {
    const fetchMock = vi.fn(async (_url: RequestInfo | URL, _init?: RequestInit) => ({
      ok: true,
      json: async () => ({
        choices: [{
          message: {
            content: JSON.stringify({
              memory: {
                summary: '角色关系发生变化，但地点没有变化。',
                worldview: [],
                characters: [
                  { name: '林舟', status: '继续调查', importance: 'high' },
                ],
                relationships: [
                  { source: '林舟', target: '沈月', relation: '同伴', description: '关系更稳定。', importance: 'medium' },
                ],
                locations: [
                  { name: '旧村', kind: '村落', description: '故事开始的村落。', importance: 'high' },
                ],
              },
              shouldRegenerateMap: true,
              mapPrompt: '重新绘制旧村地图。',
            }),
          },
        }],
      }),
    }))

    const update = await requestAiBookMemoryUpdate({
      config: readyConfig,
      book: { name: '山海旧事', author: '佚名', bookUrl: 'book-1', origin: 'source-1' },
      chapter: { title: '第十一章', url: 'chapter-11', index: 10 },
      chapterContent: '人物关系变化。',
      memory: {
        bookUrl: 'book-1',
        enabled: true,
        updatedAt: 0,
        worldview: [],
        characters: [
          { name: '林舟', status: '调查中', importance: 'high' },
        ],
        relationships: [],
        locations: [
          { name: '旧村', kind: '村落', description: '故事开始的村落。', importance: 'high' },
        ],
        map: {
          imageUrl: '/assets/ai-maps/old-map.png',
          prompt: '绘制旧村地图。',
          updatedAt: 100,
          sourceChapterIndex: 9,
        },
      },
      fetchImpl: fetchMock as unknown as typeof fetch,
    })

    expect(update.shouldRegenerateMap).toBe(false)
    expect(update.mapPrompt).toBeUndefined()
    expect(update.memory.mapDirty).toBe(false)
  })

  it('regenerates the world map when requested with new location changes', async () => {
    const fetchMock = vi.fn(async (_url: RequestInfo | URL, _init?: RequestInit) => ({
      ok: true,
      json: async () => ({
        choices: [{
          message: {
            content: JSON.stringify({
              memory: {
                summary: '主角发现北境。',
                worldview: [],
                characters: [],
                relationships: [],
                locations: [
                  { name: '旧村', kind: '村落', description: '故事开始的村落。', importance: 'high' },
                  { name: '北境', kind: '区域', description: '新出现的寒冷边境。', importance: 'high' },
                ],
              },
              shouldRegenerateMap: true,
              mapPrompt: '把旧村与北境画在同一张区域地图上。',
            }),
          },
        }],
      }),
    }))

    const update = await requestAiBookMemoryUpdate({
      config: readyConfig,
      book: { name: '山海旧事', author: '佚名', bookUrl: 'book-1', origin: 'source-1' },
      chapter: { title: '第十二章', url: 'chapter-12', index: 11 },
      chapterContent: '主角抵达北境。',
      memory: {
        bookUrl: 'book-1',
        enabled: true,
        updatedAt: 0,
        worldview: [],
        characters: [],
        relationships: [],
        locations: [
          { name: '旧村', kind: '村落', description: '故事开始的村落。', importance: 'high' },
        ],
        map: {
          imageUrl: '/assets/ai-maps/old-map.png',
          prompt: '绘制旧村地图。',
          updatedAt: 100,
          sourceChapterIndex: 9,
        },
      },
      fetchImpl: fetchMock as unknown as typeof fetch,
    })

    expect(update.shouldRegenerateMap).toBe(true)
    expect(update.mapPrompt).toBe('把旧村与北境画在同一张区域地图上。')
    expect(update.memory.mapDirty).toBe(true)
  })

  it('uploads generated base64 maps through reader asset endpoint', async () => {
    const fetchMock = vi.fn(async (_url: RequestInfo | URL, _init?: RequestInit) => ({
      ok: true,
      json: async () => ({
        isSuccess: true,
        data: ['/assets/alice/ai-maps/map.png'],
      }),
    })) as unknown as typeof fetch

    vi.mocked(invoke).mockImplementation(async (cmd: string, _args?: unknown) => {
      if (cmd === 'upload_file') return { isSuccess: true, errorMsg: '', data: ['/assets/alice/ai-maps/map.png'] }
      throw new Error(`unexpected command ${cmd}`)
    })

    const url = await uploadGeneratedMap({
      b64Json: btoa('fake-png'),
      filename: 'map.png',
      fetchImpl: fetchMock,
    })

    expect(url).toBe('/assets/alice/ai-maps/map.png')
    expect(invoke).toHaveBeenCalledWith(
      'upload_file',
      expect.objectContaining({ fileName: 'map.png', fileType: 'ai-maps' }),
    )
  })

  it('downloads generated map URLs through the backend proxy when enabled', async () => {
    const fetchMock = vi.fn(async (_url: RequestInfo | URL) => ({
      ok: true,
      json: async () => ({
        isSuccess: true,
        data: ['/assets/alice/ai-maps/map.png'],
      }),
    })) as unknown as typeof fetch

    vi.mocked(invoke).mockImplementation(async (cmd: string, _args?: unknown) => {
      if (cmd === 'ai_proxy_image') {
        return {
          status: 200,
          contentType: 'image/png',
          body: new TextEncoder().encode('fake-png'),
        }
      }
      if (cmd === 'upload_file') return { isSuccess: true, errorMsg: '', data: ['/assets/alice/ai-maps/map.png'] }
      throw new Error(`unexpected command ${cmd}`)
    })

    const url = await uploadGeneratedMap({
      imageUrl: 'https://cdn.example.test/map.png',
      filename: 'map.png',
      useBackendProxy: true,
      fetchImpl: fetchMock,
    })

    expect(url).toBe('/assets/alice/ai-maps/map.png')
    expect(invoke).toHaveBeenCalledWith(
      'ai_proxy_image',
      expect.objectContaining({ req: { url: 'https://cdn.example.test/map.png' } }),
    )
  })

  it('uploads generated data image URLs without backend proxy download', async () => {
    const fetchMock = vi.fn(async (_url: RequestInfo | URL) => ({
      ok: true,
      json: async () => ({
        isSuccess: true,
        data: ['/assets/alice/ai-maps/map.png'],
      }),
    })) as unknown as typeof fetch

    vi.mocked(invoke).mockImplementation(async (cmd: string, _args?: unknown) => {
      if (cmd === 'upload_file') return { isSuccess: true, errorMsg: '', data: ['/assets/alice/ai-maps/map.png'] }
      throw new Error(`unexpected command ${cmd}`)
    })

    const url = await uploadGeneratedMap({
      imageUrl: `data:image/png;base64,${btoa('fake-png')}`,
      filename: 'map.png',
      useBackendProxy: true,
      fetchImpl: fetchMock,
    })

    expect(url).toBe('/assets/alice/ai-maps/map.png')
    expect(invoke).toHaveBeenCalledTimes(1)
    expect(invoke).toHaveBeenCalledWith(
      'upload_file',
      expect.objectContaining({ fileName: 'map.png', fileType: 'ai-maps' }),
    )
    expect(invoke).not.toHaveBeenCalledWith('ai_proxy_image', expect.anything())
  })

  it('routes text model calls through the backend proxy when enabled', async () => {
    const fetchMock = vi.fn(async (_url: RequestInfo | URL, _init?: RequestInit) => ({
      ok: true,
      json: async () => ({
        choices: [{
          message: {
            content: JSON.stringify({
              memory: {
                summary: '主角抵达北境。',
                worldview: [],
                characters: [],
                relationships: [],
                locations: [],
              },
            }),
          },
        }],
      }),
    }))

    vi.mocked(invoke).mockImplementation(async (cmd: string, _args?: unknown) => {
      if (cmd === 'ai_proxy') {
        return {
          status: 200,
          contentType: 'application/json',
          body: new TextEncoder().encode(JSON.stringify({
            choices: [{
              message: {
                content: JSON.stringify({
                  memory: {
                    summary: '主角抵达北境。',
                    worldview: [],
                    characters: [],
                    relationships: [],
                    locations: [],
                  },
                }),
              },
            }],
          })),
        }
      }
      throw new Error(`unexpected command ${cmd}`)
    })

    await requestAiBookMemoryUpdate({
      config: { ...readyConfig, useBackendProxy: true, textApiKey: 'text-key' },
      book: { name: '山海旧事', author: '佚名', bookUrl: 'book-1', origin: 'source-1' },
      chapter: { title: '第八章', url: 'chapter-8', index: 7 },
      chapterContent: '主角抵达北境。',
      memory: {
        bookUrl: 'book-1',
        enabled: true,
        updatedAt: 0,
        worldview: [],
        characters: [],
        relationships: [],
        locations: [],
      },
      fetchImpl: fetchMock as unknown as typeof fetch,
    })

    expect(invoke).toHaveBeenCalledWith(
      'ai_proxy',
      expect.objectContaining({
        req: expect.objectContaining({
          baseUrl: 'http://localhost:8825',
          apiKey: 'text-key',
          path: '/v1/chat/completions',
        }),
      }),
    )
  })

  it('uses backend configured text model without browser credentials when selected', async () => {
    const fetchMock = vi.fn(async (_url: RequestInfo | URL, _init?: RequestInit) => ({
      ok: true,
      json: async () => ({
        choices: [{
          message: {
            content: JSON.stringify({
              memory: {
                summary: '主角抵达北境。',
                worldview: [],
                characters: [],
                relationships: [],
                locations: [],
              },
            }),
          },
        }],
      }),
    }))

    vi.mocked(invoke).mockImplementation(async (cmd: string, _args?: unknown) => {
      if (cmd === 'ai_proxy') {
        return {
          status: 200,
          contentType: 'application/json',
          body: new TextEncoder().encode(JSON.stringify({
            choices: [{
              message: {
                content: JSON.stringify({
                  memory: {
                    summary: '主角抵达北境。',
                    worldview: [],
                    characters: [],
                    relationships: [],
                    locations: [],
                  },
                }),
              },
            }],
          })),
        }
      }
      throw new Error(`unexpected command ${cmd}`)
    })

    await requestAiBookMemoryUpdate({
      config: {
        ...readyConfig,
        modelSource: 'server',
        textBaseUrl: '',
        textApiKey: '',
      },
      book: { name: '山海旧事', author: '佚名', bookUrl: 'book-1', origin: 'source-1' },
      chapter: { title: '第八章', url: 'chapter-8', index: 7 },
      chapterContent: '主角抵达北境。',
      memory: {
        bookUrl: 'book-1',
        enabled: true,
        updatedAt: 0,
        worldview: [],
        characters: [],
        relationships: [],
        locations: [],
      },
      fetchImpl: fetchMock as unknown as typeof fetch,
    })

    expect(invoke).toHaveBeenCalledWith(
      'ai_proxy',
      expect.objectContaining({
        req: expect.objectContaining({
          useServerConfig: true,
          kind: 'text',
          path: '/v1/chat/completions',
        }),
      }),
    )
    const payload = JSON.stringify(vi.mocked(invoke).mock.calls[0]?.[1])
    expect(payload).not.toContain('text-key')
  })

  it('uses the configured full text endpoint without appending chat completions', async () => {
    const fetchMock = vi.fn(async (_url: RequestInfo | URL, _init?: RequestInit) => ({
      ok: true,
      json: async () => ({
        choices: [{
          message: {
            content: JSON.stringify({
              memory: {
                summary: '主角抵达北境。',
                worldview: [],
                characters: [],
                relationships: [],
                locations: [],
              },
            }),
          },
        }],
      }),
    }))

    await requestAiBookMemoryUpdate({
      config: {
        ...readyConfig,
        textBaseUrl: 'https://gateway.example.test/custom/chat',
        textUseFullUrl: true,
      },
      book: { name: '山海旧事', author: '佚名', bookUrl: 'book-1', origin: 'source-1' },
      chapter: { title: '第八章', url: 'chapter-8', index: 7 },
      chapterContent: '主角抵达北境。',
      memory: {
        bookUrl: 'book-1',
        enabled: true,
        updatedAt: 0,
        worldview: [],
        characters: [],
        relationships: [],
        locations: [],
      },
      fetchImpl: fetchMock as unknown as typeof fetch,
    })

    expect(fetchMock).toHaveBeenCalledWith(
      'https://gateway.example.test/custom/chat',
      expect.objectContaining({ method: 'POST' }),
    )
  })

  it('uses the separated image endpoint and key for map requests', async () => {
    const fetchMock = vi.fn(async (_url: RequestInfo | URL, _init?: RequestInit) => ({
      ok: true,
      json: async () => ({
        data: [{ b64_json: btoa('fake-png') }],
      }),
    })) as unknown as typeof fetch

    await requestAiBookMapImage({
      config: readyConfig,
      prompt: '绘制关系地图',
      fetchImpl: fetchMock,
    })

    expect(fetchMock).toHaveBeenCalledWith(
      'http://localhost:8826/v1/images/generations',
      expect.objectContaining({
        method: 'POST',
        headers: expect.objectContaining({
          Authorization: 'Bearer image-key',
        }),
      }),
    )
  })

  it('wraps image prompts with cartographic constraints before map generation', async () => {
    const rawPrompt = '绘制一张包含两个独立区域的地图：左侧为现代化的地球大学机房，右侧为荒凉废土中的404号避难所，两者之间以虚线连接。'
    const fetchMock = vi.fn(async (_url: RequestInfo | URL, _init?: RequestInit) => ({
      ok: true,
      json: async () => ({
        data: [{ b64_json: btoa('fake-png') }],
      }),
    }))

    await requestAiBookMapImage({
      config: readyConfig,
      prompt: rawPrompt,
      fetchImpl: fetchMock as unknown as typeof fetch,
    })

    const init = fetchMock.mock.calls[0]?.[1] as RequestInit
    const body = JSON.parse(String(init.body)) as { prompt: string }
    expect(body.prompt).not.toBe(rawPrompt)
    expect(body.prompt).toContain(rawPrompt)
    expect(body.prompt).toContain('俯视地图')
    expect(body.prompt).toContain('地图符号')
    expect(body.prompt).toContain('不要生成写实照片')
    expect(body.prompt).toContain('不要画人物')
    expect(body.prompt).toContain('机房、避难所等室内或建筑地点只能表现为地图上的标注区域')
  })

  it('routes image model calls through the backend proxy when enabled', async () => {
    const fetchMock = vi.fn(async () => ({
      ok: true,
      json: async () => ({
        data: [{ b64_json: btoa('fake-png') }],
      }),
    })) as unknown as typeof fetch

    vi.mocked(invoke).mockImplementation(async (cmd: string, _args?: unknown) => {
      if (cmd === 'ai_proxy') {
        return {
          status: 200,
          contentType: 'application/json',
          body: new TextEncoder().encode(JSON.stringify({
            data: [{ b64_json: btoa('fake-png') }],
          })),
        }
      }
      throw new Error(`unexpected command ${cmd}`)
    })

    await requestAiBookMapImage({
      config: { ...readyConfig, useBackendProxy: true },
      prompt: '绘制关系地图',
      fetchImpl: fetchMock,
    })

    expect(invoke).toHaveBeenCalledWith(
      'ai_proxy',
      expect.objectContaining({
        req: expect.objectContaining({ path: '/v1/images/generations' }),
      }),
    )
  })

  it('uses backend configured image model without browser credentials when selected', async () => {
    const fetchMock = vi.fn(async (_url: RequestInfo | URL, _init?: RequestInit) => ({
      ok: true,
      json: async () => ({
        data: [{ b64_json: btoa('fake-png') }],
      }),
    }))

    vi.mocked(invoke).mockImplementation(async (cmd: string, _args?: unknown) => {
      if (cmd === 'ai_proxy') {
        return {
          status: 200,
          contentType: 'application/json',
          body: new TextEncoder().encode(JSON.stringify({
            data: [{ b64_json: btoa('fake-png') }],
          })),
        }
      }
      throw new Error(`unexpected command ${cmd}`)
    })

    await requestAiBookMapImage({
      config: {
        ...readyConfig,
        modelSource: 'server',
        imageBaseUrl: '',
        imageApiKey: '',
      },
      prompt: '绘制关系地图',
      fetchImpl: fetchMock as unknown as typeof fetch,
    })

    expect(invoke).toHaveBeenCalledWith(
      'ai_proxy',
      expect.objectContaining({
        req: expect.objectContaining({
          useServerConfig: true,
          kind: 'image',
          path: '/v1/images/generations',
        }),
      }),
    )
  })

  it('uses the configured full image endpoint in backend proxy requests', async () => {
    const fetchMock = vi.fn(async (_url: RequestInfo | URL, _init?: RequestInit) => ({
      ok: true,
      json: async () => ({
        data: [{ b64_json: btoa('fake-png') }],
      }),
    }))

    vi.mocked(invoke).mockImplementation(async (cmd: string, _args?: unknown) => {
      if (cmd === 'ai_proxy') {
        return {
          status: 200,
          contentType: 'application/json',
          body: new TextEncoder().encode(JSON.stringify({
            data: [{ b64_json: btoa('fake-png') }],
          })),
        }
      }
      throw new Error(`unexpected command ${cmd}`)
    })

    await requestAiBookMapImage({
      config: {
        ...readyConfig,
        useBackendProxy: true,
        imageBaseUrl: 'https://gateway.example.test/custom/image',
        imageUseFullUrl: true,
      },
      prompt: '绘制关系地图',
      fetchImpl: fetchMock as unknown as typeof fetch,
    })

    expect(invoke).toHaveBeenCalledWith(
      'ai_proxy',
      expect.objectContaining({
        req: expect.objectContaining({
          baseUrl: 'https://gateway.example.test/custom/image',
          path: '/v1/images/generations',
          fullUrl: true,
        }),
      }),
    )
  })

  it('can downgrade a failed map image into an interactive relationship graph fallback', () => {
    const memory: AiBookMemory = {
      bookUrl: 'book-1',
      enabled: true,
      processedChapterIndex: 3,
      worldview: [],
      characters: [],
      relationships: [],
      locations: [],
      updatedAt: 0,
      mapDirty: true,
    }

    const next = applyMapFallbackToMemory(memory, {
      prompt: '绘制世界地图',
      reason: '图片模型未配置',
      sourceChapterIndex: 3,
      updatedAt: 100,
    })

    expect(next.mapDirty).toBe(true)
    expect(next.map).toMatchObject({
      fallback: 'relationship-graph',
      fallbackReason: '图片模型未配置',
      prompt: '绘制世界地图',
      sourceChapterIndex: 3,
      updatedAt: 100,
    })
  })
})
