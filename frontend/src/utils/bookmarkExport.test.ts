import { describe, expect, it } from 'vitest'
import type { Bookmark } from '../types'
import {
  bookmarkExportFilename,
  buildBookmarkExportJson,
  buildBookmarkExportMarkdown,
} from './bookmarkExport'

const bookmarks: Bookmark[] = [
  {
    time: 1711886400000,
    bookName: '山茶文具店',
    bookAuthor: '小川糸',
    chapterIndex: 1,
    chapterName: '第一封信',
    bookText: '原文一',
    content: '想法一',
  },
  {
    time: 1711886500000,
    bookName: '山茶文具店',
    bookAuthor: '小川糸',
    chapterIndex: 2,
    chapterName: '第二封信',
    bookText: '原文二',
    content: '',
  },
  {
    time: 1711886600000,
    bookName: '另一本书',
    bookAuthor: '作者',
    chapterIndex: 0,
    chapterName: '开头',
    bookText: '原文三',
    content: '想法三',
  },
]

describe('bookmarkExport', () => {
  it('JSON 导出保留书签完整字段', () => {
    expect(JSON.parse(buildBookmarkExportJson(bookmarks))).toEqual(bookmarks)
  })

  it('Markdown 导出与手机端格式一致并按书籍分组', () => {
    expect(buildBookmarkExportMarkdown(bookmarks)).toBe(
      '## 山茶文具店 小川糸\n\n'
      + '#### 第一封信\n\n'
      + '###### 原文\n 原文一\n\n'
      + '###### 摘要\n 想法一\n\n'
      + '#### 第二封信\n\n'
      + '###### 原文\n 原文二\n\n'
      + '###### 摘要\n \n\n'
      + '## 另一本书 作者\n\n'
      + '#### 开头\n\n'
      + '###### 原文\n 原文三\n\n'
      + '###### 摘要\n 想法三\n\n',
    )
  })

  it('文件名沿用手机端的 bookmark-yyMMddHHmmss 格式', () => {
    expect(bookmarkExportFilename('json', new Date(2026, 0, 2, 3, 4, 5))).toBe(
      'bookmark-260102030405.json',
    )
  })
})
