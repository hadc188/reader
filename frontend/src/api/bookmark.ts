import { get, post, invokeEnvelope } from './invoke'
import { isTauriRuntime } from './transport'
import type { Bookmark } from '../types'

/**
 * 获取所有书签
 */
export function getBookmarks() {
  return get<Bookmark[]>('/getBookmarks').then((r) => r.data)
}

/**
 * 保存单个书签
 */
export function saveBookmark(bookmark: Bookmark) {
  return post<string>('/saveBookmark', bookmark).then((r) => r.data)
}

/**
 * 批量保存书签
 */
export function saveBookmarks(bookmarks: Bookmark[]) {
  return post<string>('/saveBookmarks', bookmarks).then((r) => r.data)
}

/**
 * 删除单个书签
 */
export function deleteBookmark(bookmark: Bookmark) {
  return post<string>('/deleteBookmark', bookmark).then((r) => r.data)
}

/**
 * 批量删除书签
 */
export function deleteBookmarks(bookmarks: Bookmark[]) {
  return post<string>('/deleteBookmarks', bookmarks).then((r) => r.data)
}

export interface SaveBookmarkExportResult {
  saved: boolean
  cancelled?: boolean
  path?: string
}

export async function saveBookmarkExportAs(
  fileName: string,
  content: string,
  format: 'json' | 'md',
): Promise<SaveBookmarkExportResult> {
  if (isTauriRuntime()) {
    return invokeEnvelope<SaveBookmarkExportResult>('save_bookmark_export_as', {
      req: { fileName, content, format },
    })
  }

  const type = format === 'json' ? 'application/json;charset=utf-8' : 'text/markdown;charset=utf-8'
  const blob = new Blob([content], { type })
  const picker = (window as Window & {
    showSaveFilePicker?: (options: {
      suggestedName: string
      types: Array<{ description: string; accept: Record<string, string[]> }>
    }) => Promise<{ createWritable: () => Promise<{ write: (value: Blob) => Promise<void>; close: () => Promise<void> }> }>
  }).showSaveFilePicker

  if (picker) {
    try {
      const handle = await picker({
        suggestedName: fileName,
        types: [{
          description: format === 'json' ? 'JSON 文件' : 'Markdown 文件',
          accept: {
            [type]: [`.${format}`],
          },
        }],
      })
      const writable = await handle.createWritable()
      await writable.write(blob)
      await writable.close()
      return { saved: true }
    } catch (error) {
      if ((error as Error).name === 'AbortError') return { saved: false, cancelled: true }
      throw error
    }
  }

  const url = URL.createObjectURL(blob)
  const anchor = document.createElement('a')
  anchor.href = url
  anchor.download = fileName
  document.body.appendChild(anchor)
  anchor.click()
  anchor.remove()
  window.setTimeout(() => URL.revokeObjectURL(url), 1000)
  return { saved: true }
}
