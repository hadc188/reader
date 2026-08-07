import type { BookChapter } from '../types'

const NON_STORY_TITLE_PATTERNS = [
  /番外/,
  /外传/,
  /新书/,
  /新番外/,
  /公告/,
  /通知/,
  /说明/,
  /感言/,
  /请假/,
  /断更/,
  /恢复更新/,
  /更新时间/,
  /求票/,
  /月票/,
  /推荐票/,
  /打赏/,
  /推书/,
  /书友群/,
  /群号/,
  /活动/,
  /抽奖/,
  /实体书/,
  /出版/,
  /完本/,
  /上架/,
]

const PROLOGUE_PATTERNS = [
  /^序章$/,
  /^楔子$/,
  /^引子$/,
  /^序$/,
  /^前言$/,
]

export function shouldSkipAiBookChapter(chapter: Pick<BookChapter, 'title' | 'index'>, chapters: Pick<BookChapter, 'title' | 'index'>[] = []) {
  const title = normalizeChapterTitle(chapter.title)
  if (!title) return true
  if (PROLOGUE_PATTERNS.some((pattern) => pattern.test(title))) return false
  if (NON_STORY_TITLE_PATTERNS.some((pattern) => pattern.test(title))) return true
  return isFrontLoadedLatestChapter(chapter, chapters)
}

function isFrontLoadedLatestChapter(chapter: Pick<BookChapter, 'title' | 'index'>, chapters: Pick<BookChapter, 'title' | 'index'>[]) {
  if (!chapters.length || chapter.index > 30) return false

  const ordinal = extractChapterOrdinal(chapter.title)
  if (!ordinal || ordinal < 50) return false

  const firstMainIndex = chapters.findIndex((item) => {
    const itemTitle = normalizeChapterTitle(item.title)
    if (PROLOGUE_PATTERNS.some((pattern) => pattern.test(itemTitle))) return false
    return extractChapterOrdinal(item.title) === 1
  })

  if (firstMainIndex >= 0 && chapter.index < firstMainIndex) return true

  const laterLowOrdinal = chapters
    .slice(chapter.index + 1, Math.min(chapters.length, chapter.index + 20))
    .some((item) => {
      const nextOrdinal = extractChapterOrdinal(item.title)
      return typeof nextOrdinal === 'number' && nextOrdinal <= 10
    })
  return laterLowOrdinal
}

function extractChapterOrdinal(title: string) {
  const normalized = normalizeChapterTitle(title)
  if (!normalized) return null

  const arabic = normalized.match(/^(?:第)?\s*(\d{1,5})\s*(?:[章章节回话集部卷篇\.、:：\s]|$)/)
  if (arabic) return Number(arabic[1])

  const chinese = normalized.match(/^第?\s*([零〇一二两三四五六七八九十百千万]+)\s*(?:章|节|回|话|集|部|卷|篇)/)
  if (chinese) return parseChineseNumber(chinese[1])

  return null
}

function normalizeChapterTitle(title: string) {
  return title
    .replace(/\s+/g, '')
    .replace(/[【】《》（）()]/g, '')
    .trim()
}

function parseChineseNumber(value: string) {
  const digits: Record<string, number> = {
    零: 0,
    '〇': 0,
    一: 1,
    二: 2,
    两: 2,
    三: 3,
    四: 4,
    五: 5,
    六: 6,
    七: 7,
    八: 8,
    九: 9,
  }
  const units: Record<string, number> = {
    十: 10,
    百: 100,
    千: 1000,
    万: 10000,
  }

  let total = 0
  let section = 0
  let number = 0
  for (const char of value) {
    if (char in digits) {
      number = digits[char]!
      continue
    }
    const unit = units[char]
    if (!unit) return null
    if (unit === 10000) {
      section = (section + number) * unit
      total += section
      section = 0
    } else {
      section += (number || 1) * unit
    }
    number = 0
  }

  return total + section + number
}
