import { describe, expect, it } from 'vitest'
import type { BookChapter } from '../types'
import { shouldSkipAiBookChapter } from './aiBookChapterFilter'

const chapters: BookChapter[] = [
  { index: 0, title: '12月1日《诡秘之主》新番外发布', url: '0' },
  { index: 1, title: '1417.新书已发', url: '1' },
  { index: 2, title: '1416.不是诈尸，不是遭了阿蒙', url: '2' },
  { index: 3, title: '1415.一个普通人的日常（八）', url: '3' },
  { index: 4, title: '1414.一个普通人的日常（七）', url: '4' },
  { index: 5, title: '1.第1章 绯红', url: '5' },
  { index: 6, title: '2.第2章 情况', url: '6' },
]

describe('aiBookChapterFilter', () => {
  it('skips source-prefixed latest chapters before the real first chapter', () => {
    expect(shouldSkipAiBookChapter(chapters[0]!, chapters)).toBe(true)
    expect(shouldSkipAiBookChapter(chapters[1]!, chapters)).toBe(true)
    expect(shouldSkipAiBookChapter(chapters[4]!, chapters)).toBe(true)
    expect(shouldSkipAiBookChapter(chapters[5]!, chapters)).toBe(false)
  })

  it('skips non-story extras and announcements', () => {
    expect(shouldSkipAiBookChapter({ index: 22, title: '番外：普通人的日常' })).toBe(true)
    expect(shouldSkipAiBookChapter({ index: 23, title: '上架感言' })).toBe(true)
    expect(shouldSkipAiBookChapter({ index: 24, title: '新书已发，求支持' })).toBe(true)
  })

  it('keeps prologue-like story chapters', () => {
    expect(shouldSkipAiBookChapter({ index: 0, title: '序章' }, [
      { index: 0, title: '序章' },
      { index: 1, title: '第一章 风起' },
    ])).toBe(false)
  })
})
