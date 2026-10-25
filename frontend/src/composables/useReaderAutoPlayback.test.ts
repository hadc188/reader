import { describe, expect, it } from 'vitest'
import {
  findParagraphPageIndex,
  getSpeechProgressItemIndex,
  isSpeechSelfScroll,
  shouldFollowChapterJump,
} from './useReaderAutoPlayback'

describe('merged speech paragraph progress', () => {
  it('moves the active paragraph according to each paragraph text length', () => {
    const lengths = [20, 60, 20]

    expect(getSpeechProgressItemIndex(lengths, 0)).toBe(0)
    expect(getSpeechProgressItemIndex(lengths, 0.19)).toBe(0)
    expect(getSpeechProgressItemIndex(lengths, 0.2)).toBe(1)
    expect(getSpeechProgressItemIndex(lengths, 0.79)).toBe(1)
    expect(getSpeechProgressItemIndex(lengths, 0.8)).toBe(2)
    expect(getSpeechProgressItemIndex(lengths, 1)).toBe(2)
  })
})

describe('horizontal page lookup for speech following', () => {
  /** 假页容器: 只需要 `contains`, 与运行时用到的能力一致。 */
  function fakePage(...members: object[]): { contains(node: unknown): boolean } {
    return { contains: (node: unknown) => members.includes(node as object) }
  }

  it('finds the page that contains the spoken paragraph', () => {
    const secondParagraph = { text: '第三段' }
    const pages = [fakePage({ text: '第一段' }, { text: '第二段' }), fakePage(secondParagraph)]

    expect(findParagraphPageIndex(pages, secondParagraph)).toBe(1)
  })

  it('returns -1 when the paragraph is not rendered yet', () => {
    // 换章/重建分页期间段落可能已被卸载: 此时不能翻页, 也不能越界。
    const detached = { text: '不在任何页里' }
    expect(findParagraphPageIndex([fakePage({ text: '正文' })], detached)).toBe(-1)
    expect(findParagraphPageIndex([], detached)).toBe(-1)
    expect(findParagraphPageIndex([fakePage({ text: '正文' })], null)).toBe(-1)
  })
})

describe('manual chapter jump following', () => {
  /** 用户手动跳章、朗读进行中、没有过渡、TTS 也没有刚自己滚过正文。 */
  const userJump = {
    speechDriven: false,
    hasSpeechSession: true,
    transitioning: false,
    selfScroll: false,
  }

  it('用户跳章且朗读进行中时跟随', () => {
    expect(shouldFollowChapterJump(userJump)).toBe(true)
  })

  it('TTS 自己切章时不跟随（否则会自己打断自己）', () => {
    expect(shouldFollowChapterJump({ ...userJump, speechDriven: true })).toBe(false)
  })

  it('没有朗读会话时不跟随（纯阅读换章）', () => {
    expect(shouldFollowChapterJump({ ...userJump, hasSpeechSession: false })).toBe(false)
  })

  it('正在切段/切章过渡时不跟随', () => {
    expect(shouldFollowChapterJump({ ...userJump, transitioning: true })).toBe(false)
  })

  it('TTS 刚自己滚过正文时不跟随（连续模式下滚动到章边界也会改索引）', () => {
    expect(shouldFollowChapterJump({ ...userJump, selfScroll: true })).toBe(false)
  })
})

describe('区分「TTS 自动滚动」与「用户滚动」', () => {
  const now = 10_000

  it('滚动反推 + 保护窗口内 + 无用户输入 → TTS 自滚', () => {
    expect(isSpeechSelfScroll({
      scrollDriven: true,
      now,
      lastSpeechScrollAt: now - 100,
      lastUserInputAt: now - 5_000,
    })).toBe(true)
  })

  it('用户刚滚过（输入晚于 TTS 滚动）→ 不算自滚，跳章要被跟随', () => {
    // 用户点的跳章/滚轮发生在 TTS 上次滚动之后：这是真实操作，不能被窗口吞掉。
    expect(isSpeechSelfScroll({
      scrollDriven: true,
      now,
      lastSpeechScrollAt: now - 100,
      lastUserInputAt: now - 50,
    })).toBe(false)
  })

  it('超出保护窗口 → 不算自滚', () => {
    expect(isSpeechSelfScroll({
      scrollDriven: true,
      now,
      lastSpeechScrollAt: now - 5_000,
      lastUserInputAt: 0,
    })).toBe(false)
  })

  it('不是滚动反推（目录/上下一章/进度条）→ 一律不算自滚', () => {
    expect(isSpeechSelfScroll({
      scrollDriven: false,
      now,
      lastSpeechScrollAt: now - 10,
      lastUserInputAt: 0,
    })).toBe(false)
  })
})
