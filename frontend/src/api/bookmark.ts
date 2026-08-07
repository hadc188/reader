import { get, post } from './invoke'
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
