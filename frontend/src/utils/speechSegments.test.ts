import { describe, expect, it } from 'vitest'
import { splitSpeechSegments, splitSpeechSegmentsDetailed } from './speechSegments'

describe('splitSpeechSegments', () => {
  it('returns a single narration segment when there are no quotes', () => {
    expect(splitSpeechSegments('他推开门，屋里没有人。')).toEqual([
      { type: 'narration', text: '他推开门，屋里没有人。' },
    ])
  })

  it('keeps a single quoted dialogue as one dialogue segment', () => {
    expect(splitSpeechSegments('“你好。”')).toEqual([
      { type: 'dialogue', text: '“你好。”' },
    ])
  })

  it('separates narration, dialogue and trailing narration in order', () => {
    expect(splitSpeechSegments('他说：“你好。”然后走了。')).toEqual([
      { type: 'narration', text: '他说：' },
      { type: 'dialogue', text: '“你好。”' },
      { type: 'narration', text: '然后走了。' },
    ])
  })

  it('treats an unclosed quote as dialogue until the end of the text', () => {
    // 跨段对话在正文里很常见：整段只有一个开引号。
    expect(splitSpeechSegments('他说：“今天不走了')).toEqual([
      { type: 'narration', text: '他说：' },
      { type: 'dialogue', text: '“今天不走了' },
    ])
  })

  it('pairs nested same-kind quotes by depth instead of closing at the first closer', () => {
    expect(splitSpeechSegments('“他说“快走”，然后跑了。”')).toEqual([
      { type: 'dialogue', text: '“他说“快走”，然后跑了。”' },
    ])
  })

  it('keeps nested different-kind quotes inside one dialogue segment', () => {
    expect(splitSpeechSegments('他喊：“先走「左边」那条路”')).toEqual([
      { type: 'narration', text: '他喊：' },
      { type: 'dialogue', text: '“先走「左边」那条路”' },
    ])
  })

  it('recognises the corner-bracket quote forms', () => {
    expect(splitSpeechSegments('「走吧」他挥挥手。')).toEqual([
      { type: 'dialogue', text: '「走吧」' },
      { type: 'narration', text: '他挥挥手。' },
    ])
    expect(splitSpeechSegments('『别出声』')).toEqual([
      { type: 'dialogue', text: '『别出声』' },
    ])
  })

  it('keeps consecutive dialogue segments separate (no merging)', () => {
    expect(splitSpeechSegments('“第一句。”“第二句。”')).toEqual([
      { type: 'dialogue', text: '“第一句。”' },
      { type: 'dialogue', text: '“第二句。”' },
    ])
  })

  it('drops whitespace-only segments between quotes', () => {
    expect(splitSpeechSegments('“甲”   “乙”')).toEqual([
      { type: 'dialogue', text: '“甲”' },
      { type: 'dialogue', text: '“乙”' },
    ])
  })

  it('trims only the ends of each segment', () => {
    // 全角空格在正文中间必须保留：它常被用来做中文排版缩进/停顿。
    const raw = '  旁白开头\u3000\u3000内部停顿  “对\u3000话”  尾巴  '
    expect(splitSpeechSegments(raw)).toEqual([
      { type: 'narration', text: '旁白开头\u3000\u3000内部停顿' },
      { type: 'dialogue', text: '“对\u3000话”' },
      { type: 'narration', text: '尾巴' },
    ])
  })

  it('returns an empty list for blank input', () => {
    expect(splitSpeechSegments('')).toEqual([])
    expect(splitSpeechSegments('   \n\t\u3000')).toEqual([])
  })

  it('treats a stray closing quote as ordinary narration text', () => {
    // 半截数据/OCR 失败时可能只剩闭符：不能因为它把后半段吞成对话。
    expect(splitSpeechSegments('旁白”仍在旁白')).toEqual([
      { type: 'narration', text: '旁白”仍在旁白' },
    ])
  })

  it('handles a dialogue that runs to the end after an inner quote closes', () => {
    expect(splitSpeechSegments('“他说「走」')).toEqual([
      { type: 'dialogue', text: '“他说「走」' },
    ])
  })
})

describe('splitSpeechSegments：说话人冒号', () => {
  it('把「短前缀 + 冒号」后的句子切成对话，冒号本身留在旁白里', () => {
    expect(splitSpeechSegments('张三：你好啊。李四：我也好。')).toEqual([
      { type: 'narration', text: '张三：' },
      { type: 'dialogue', text: '你好啊。' },
      { type: 'narration', text: '李四：' },
      { type: 'dialogue', text: '我也好。' },
    ])
  })

  it('带逗号的说话人标签也认', () => {
    expect(splitSpeechSegments('老道士叹了口气，说：走吧。')).toEqual([
      { type: 'narration', text: '老道士叹了口气，说：' },
      { type: 'dialogue', text: '走吧。' },
    ])
  })

  it('冒号后紧跟引号时交给引号规则，不把说话人标签读成对话', () => {
    expect(splitSpeechSegments('张三：“你好。”')).toEqual([
      { type: 'narration', text: '张三：' },
      { type: 'dialogue', text: '“你好。”' },
    ])
  })

  it('前缀过长时不当说话人（普通叙述里的冒号）', () => {
    const text = '他站在门口想了很久很久很久才终于明白了一件事：原来如此。'
    expect(splitSpeechSegments(text)).toEqual([{ type: 'narration', text }])
  })

  it('数字开头与常见标签词后面的冒号不算说话人', () => {
    for (const text of ['时间：2024年。', '价格：5元。', '注意：请勿模仿。', '规则如下：第一，别跑。']) {
      expect(splitSpeechSegments(text), text).toEqual([{ type: 'narration', text }])
    }
  })

  it('冒号对话在句末结束，后面的叙述回到旁白', () => {
    expect(splitSpeechSegments('他笑道：明天见。然后转身走了。')).toEqual([
      { type: 'narration', text: '他笑道：' },
      { type: 'dialogue', text: '明天见。' },
      { type: 'narration', text: '然后转身走了。' },
    ])
  })

  it('冒号对话里仍能嵌套引号', () => {
    expect(splitSpeechSegments('他说：快走，别管我“那件事”了。')).toEqual([
      { type: 'narration', text: '他说：' },
      { type: 'dialogue', text: '快走，别管我“那件事”了。' },
    ])
  })

  it('关掉冒号规则时只认引号', () => {
    expect(
      splitSpeechSegmentsDetailed('张三：你好啊。', { detectSpeakerColon: false }).segments,
    ).toEqual([{ type: 'narration', text: '张三：你好啊。' }])
  })

  it('台词里的分号与省略号不结束对话', () => {
    // 分号/省略号在台词中间很常见，当成句末会把一句台词劈成「对话 + 旁白」。
    expect(splitSpeechSegments('他笑道：先别走；我还有话说。')).toEqual([
      { type: 'narration', text: '他笑道：' },
      { type: 'dialogue', text: '先别走；我还有话说。' },
    ])
    expect(splitSpeechSegments('他说：等等……我还有话说。')).toEqual([
      { type: 'narration', text: '他说：' },
      { type: 'dialogue', text: '等等……我还有话说。' },
    ])
  })

  it('叙述里的「短句 + 冒号」不当台词', () => {
    // 只按「短前缀」判定会把心理活动/判断句读成对话音色。
    for (const text of [
      '他明白了：这一切都是骗局。',
      '他只有一个念头：活下去。',
      '他心里清楚：这次躲不过去了。',
      '结论只有一个：撤退。',
      '他想起一句话：时间会证明一切。',
      '根据规则：每人只能带一件。',
      '他决定：明天就走。',
    ]) {
      expect(splitSpeechSegments(text), text).toEqual([{ type: 'narration', text }])
    }
  })

  it('带说话动作的标签仍照常切（包括不写「说」的动作）', () => {
    expect(splitSpeechSegments('张三冷笑：你也有今天。')).toEqual([
      { type: 'narration', text: '张三冷笑：' },
      { type: 'dialogue', text: '你也有今天。' },
    ])
    expect(splitSpeechSegments('老道士淡淡道：随缘吧。')).toEqual([
      { type: 'narration', text: '老道士淡淡道：' },
      { type: 'dialogue', text: '随缘吧。' },
    ])
  })
})

describe('splitSpeechSegmentsDetailed：跨段对话', () => {
  it('上一段只有开引号时，把未闭合的闭符交给下一段', () => {
    const first = splitSpeechSegmentsDetailed('他说：“今天')
    expect(first.segments).toEqual([
      { type: 'narration', text: '他说：' },
      { type: 'dialogue', text: '“今天' },
    ])
    expect(first.openCloser).toBe('”')
  })

  it('下一段从对话续读，闭符之后回到旁白', () => {
    const second = splitSpeechSegmentsDetailed('不走了。”然后他走了。', { openCloser: '”' })
    expect(second.segments).toEqual([
      { type: 'dialogue', text: '不走了。”' },
      { type: 'narration', text: '然后他走了。' },
    ])
    expect(second.openCloser).toBeNull()
  })

  it('冒号对话只到句末，不跨段', () => {
    const result = splitSpeechSegmentsDetailed('张三：今天不走了')
    expect(result.segments).toEqual([
      { type: 'narration', text: '张三：' },
      { type: 'dialogue', text: '今天不走了' },
    ])
    expect(result.openCloser).toBeNull()
  })
})
