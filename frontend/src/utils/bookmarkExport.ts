import type { Bookmark } from '../types'

/** 与手机端「所有书签 -> 导出 JSON」一致：直接导出书签实体数组。 */
export function buildBookmarkExportJson(bookmarks: Bookmark[]): string {
  return JSON.stringify(bookmarks, null, 2)
}

/**
 * 与手机端「所有书签 -> 导出 Markdown」保持相同的标题层级和字段顺序。
 *
 * 不额外加导出时间或文件说明，方便同一个脚本继续处理两端的导出结果。
 */
export function buildBookmarkExportMarkdown(bookmarks: Bookmark[]): string {
  let output = ''
  let currentBookKey = ''

  for (const bookmark of bookmarks) {
    const bookKey = `${bookmark.bookName}\u0000${bookmark.bookAuthor}`
    if (bookKey !== currentBookKey) {
      currentBookKey = bookKey
      output += `## ${bookmark.bookName} ${bookmark.bookAuthor}\n\n`
    }
    output += `#### ${bookmark.chapterName || '未命名章节'}\n\n`
    output += `###### 原文\n ${bookmark.bookText || ''}\n\n`
    output += `###### 摘要\n ${bookmark.content || ''}\n\n`
  }

  return output
}

export function bookmarkExportFilename(format: 'json' | 'md', now = new Date()): string {
  const pad = (value: number) => String(value).padStart(2, '0')
  const stamp = [
    String(now.getFullYear()).slice(-2),
    pad(now.getMonth() + 1),
    pad(now.getDate()),
    pad(now.getHours()),
    pad(now.getMinutes()),
    pad(now.getSeconds()),
  ].join('')
  return `bookmark-${stamp}.${format}`
}
