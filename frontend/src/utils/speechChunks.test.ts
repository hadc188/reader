import { describe, expect, it } from 'vitest'
import { buildSpeechChunkPieces } from './speechChunks'

const onlineDual = { dualVoice: true, sentenceChunked: true }

describe('buildSpeechChunkPieces', () => {
  it('一整段长对话被切成多块时，每一块都保持对话类型', () => {
    // 用户报的原始形态：没有跨段，就是一段比较长的对话，前半截用对话音色、
    // 后半截突然变旁白。根因是先按句切块再逐块认引号：后续小块丢了引号上下文。
    const body = '这是一句很长的话。'.repeat(8)
    const text = `张三：“${body}”`
    const { pieces } = buildSpeechChunkPieces(text, onlineDual)

    expect(pieces[0]).toEqual({ text: '张三：', type: 'narration' })
    const dialogue = pieces.slice(1)
    expect(dialogue.length).toBeGreaterThan(1)
    expect(dialogue.every((piece) => piece.type === 'dialogue')).toBe(true)
    expect(dialogue.map((piece) => piece.text).join('')).toBe(`“${body}”`)
  })

  it('短对话只切成一块，类型同样正确', () => {
    const { pieces } = buildSpeechChunkPieces('张三：“第一句话。第二句话。”', onlineDual)
    expect(pieces).toEqual([
      { text: '张三：', type: 'narration' },
      { text: '“第一句话。第二句话。”', type: 'dialogue' },
    ])
  })

  it('超过单块上限的长对话按逗号拆开后，子块仍是对话', () => {
    const body = `${'很长的第一小句'.repeat(8)}，${'很长的第二小句'.repeat(8)}`
    const text = `“${body}”`
    expect(text.length).toBeGreaterThan(70)
    const { pieces } = buildSpeechChunkPieces(text, onlineDual)

    expect(pieces.length).toBeGreaterThan(1)
    expect(pieces.every((piece) => piece.type === 'dialogue')).toBe(true)
    expect(pieces.map((piece) => piece.text).join('')).toBe(text)
  })

  it('旁白里夹对话时按位置切分，各块类型与原文一致', () => {
    const { pieces } = buildSpeechChunkPieces('他推门进来。“你来了。”他点点头。', onlineDual)

    expect(pieces).toEqual([
      { text: '他推门进来。', type: 'narration' },
      { text: '“你来了。”', type: 'dialogue' },
      { text: '他点点头。', type: 'narration' },
    ])
  })

  it('跨段对话：上一段只有开引号，下一段开头仍算对话', () => {
    const first = buildSpeechChunkPieces('他说：“今天', onlineDual)
    expect(first.pieces).toEqual([
      { text: '他说：', type: 'narration' },
      { text: '“今天', type: 'dialogue' },
    ])
    expect(first.openCloser).toBe('”')

    const second = buildSpeechChunkPieces('不走了。”然后他走了。', {
      ...onlineDual,
      openCloser: first.openCloser,
    })
    expect(second.pieces).toEqual([
      { text: '不走了。”', type: 'dialogue' },
      { text: '然后他走了。', type: 'narration' },
    ])
    expect(second.openCloser).toBeNull()
  })

  it('不开双音色时整段都是旁白（保持单音色老行为）', () => {
    const { pieces } = buildSpeechChunkPieces('张三：“你好。再见。”', {
      dualVoice: false,
      sentenceChunked: true,
    })

    expect(pieces).toEqual([{ text: '张三：“你好。再见。”', type: 'narration' }])
  })

  it('开双音色但通道不按句切块时，每个旁白/对话段各成一块', () => {
    const { pieces } = buildSpeechChunkPieces('他喊：“先走「左边」那条路”', {
      dualVoice: true,
      sentenceChunked: false,
    })

    expect(pieces).toEqual([
      { text: '他喊：', type: 'narration' },
      { text: '“先走「左边」那条路”', type: 'dialogue' },
    ])
  })

  it('丢掉纯标点/纯引号块', () => {
    // 纯引号内容在线服务不会返回音频，必须在这里就丢掉而不是发出去换错误。
    expect(buildSpeechChunkPieces('“……”。', onlineDual).pieces).toEqual([])
  })

  it('不切块通道下，只有说话人标签的旁白并进前一段旁白（少一次请求）', () => {
    // http 引擎一段一次请求：`张三：` 单独发出去等于白花一次合成。
    const { pieces } = buildSpeechChunkPieces('他推门进来。张三：你好啊。', {
      dualVoice: true,
      sentenceChunked: false,
    })
    expect(pieces).toEqual([
      { text: '他推门进来。张三：', type: 'narration' },
      { text: '你好啊。', type: 'dialogue' },
    ])
  })

  it('没有前一段旁白可并时，说话人标签仍单独成块', () => {
    const { pieces } = buildSpeechChunkPieces('张三：你好啊。', {
      dualVoice: true,
      sentenceChunked: false,
    })
    expect(pieces).toEqual([
      { text: '张三：', type: 'narration' },
      { text: '你好啊。', type: 'dialogue' },
    ])
  })
})
