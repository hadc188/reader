// Drop-in replacement for the old axios `http` client. The 14 api/*.ts modules
// keep their exact signatures — `http.get<T>('/x').then(r => r.data)` becomes
// `get<T>('/x').then(r => r.data)` — only the import changes.
//
// Each call maps a `/reader3/*` path to a Tauri command name and unwraps the
// ApiResponse envelope, rejecting with `new Error(errorMsg)` exactly like the
// old axios response interceptor.

import { invoke } from '@tauri-apps/api/core'
import type { ApiResponse } from '../types'

const COMMAND_BY_PATH: Record<string, string> = {
  '/reader3/getBookSources': 'get_book_sources',
  '/reader3/loginBookSource': 'login_book_source',
  '/reader3/setBookSourceCookie': 'set_book_source_cookie',
  '/reader3/testBookSources': 'test_book_sources',
  '/reader3/cancelBookSourceTest': 'cancel_book_source_test',
  '/reader3/cancelCacheBook': 'cancel_cache_book',
  '/reader3/deleteInvalidBookSources': 'delete_invalid_book_sources',
  '/reader3/saveBookSource': 'save_book_source',
  '/reader3/saveBookSources': 'save_book_sources',
  '/reader3/deleteBookSource': 'delete_book_source',
  '/reader3/deleteBookSources': 'delete_book_sources',
  '/reader3/deleteAllBookSources': 'delete_all_book_sources',
  '/reader3/pinBookSource': 'pin_book_source',
  '/reader3/unpinBookSource': 'unpin_book_source',
  '/reader3/readRemoteSourceFile': 'read_remote_source_file',
  '/reader3/exploreBook': 'explore_book',
  '/reader3/searchBookMulti': 'search_book_multi',
  '/reader3/getBookshelf': 'get_bookshelf',
  '/reader3/getShelfBookWithCacheInfo': 'get_shelf_book_with_cache_info',
  '/reader3/getBookGroups': 'get_book_groups',
  '/reader3/saveBookGroup': 'save_book_group',
  '/reader3/deleteBookGroup': 'delete_book_group',
  '/reader3/saveBookGroupId': 'save_book_group_id',
  '/reader3/saveBook': 'save_book',
  '/reader3/saveBooks': 'save_books',
  '/reader3/setBookSource': 'set_book_source',
  '/reader3/deleteBook': 'delete_book',
  '/reader3/deleteBooks': 'delete_books',
  '/reader3/saveBookProgress': 'save_book_progress',
  '/reader3/getBookInfo': 'get_book_info',
  '/reader3/getChapterList': 'get_chapter_list',
  '/reader3/getBookContent': 'get_book_content',
  '/reader3/deleteBookCache': 'delete_book_cache',
  '/reader3/getCachedChapterUrls': 'get_cached_chapter_urls',
  '/reader3/getAvailableBookSource': 'get_available_book_source',
  '/reader3/getRssSources': 'get_rss_sources',
  '/reader3/saveRssSource': 'save_rss_source',
  '/reader3/saveRssSources': 'save_rss_sources',
  '/reader3/deleteRssSource': 'delete_rss_source',
  '/reader3/deleteRssSources': 'delete_rss_sources',
  '/reader3/readRemoteRssSourceFile': 'read_remote_rss_source_file',
  '/reader3/getRssArticles': 'get_rss_articles',
  '/reader3/getRssContent': 'get_rss_content',
  '/reader3/getBookmarks': 'get_bookmarks',
  '/reader3/saveBookmark': 'save_bookmark',
  '/reader3/saveBookmarks': 'save_bookmarks',
  '/reader3/deleteBookmark': 'delete_bookmark',
  '/reader3/deleteBookmarks': 'delete_bookmarks',
  '/reader3/getReplaceRules': 'get_replace_rules',
  '/reader3/saveReplaceRule': 'save_replace_rule',
  '/reader3/saveReplaceRules': 'save_replace_rules',
  '/reader3/deleteReplaceRule': 'delete_replace_rule',
  '/reader3/deleteReplaceRules': 'delete_replace_rules',
  '/reader3/getWebdavFileList': 'get_webdav_file_list',
  '/reader3/getWebdavFile': 'get_webdav_file',
  '/reader3/getWebdavBackupArchive': 'get_webdav_backup_archive',
  '/reader3/deleteWebdavFile': 'delete_webdav_file',
  '/reader3/deleteWebdavFileList': 'delete_webdav_file_list',
  '/reader3/getWebdavHome': 'get_webdav_home',
  '/reader3/openWebdavFolder': 'open_webdav_folder',
  '/reader3/getVersionUpdate': 'get_version_update',
  '/reader3/dismissVersionUpdate': 'dismiss_version_update',
}

let lastNeedLoginDispatchAt = 0

function dispatchNeedLogin() {
  const now = Date.now()
  if (now - lastNeedLoginDispatchAt < 1500) return
  lastNeedLoginDispatchAt = now
  window.dispatchEvent(new CustomEvent('need-login'))
}

async function call<T>(path: string, payload: unknown): Promise<T> {
  // The api/*.ts modules pass relative paths (e.g. `/getBookSources`) because
  // the old axios client had `baseURL: '/reader3'`. Normalize to the full path
  // the command map is keyed on.
  const fullPath = path.startsWith('/reader3') ? path : `/reader3${path}`
  const command = COMMAND_BY_PATH[fullPath]
  if (!command) throw new Error(`未知接口: ${path}`)
  try {
    const res = (await invoke(command, payload === undefined ? {} : { req: payload })) as
      | ApiResponse<T>
      | T
    if (typeof res === 'object' && res !== null && 'isSuccess' in res) {
      const box = res as ApiResponse<T>
      if (!box.isSuccess) {
        if (box.errorMsg === 'NEED_LOGIN' || box.data === 'NEED_LOGIN') {
          dispatchNeedLogin()
        }
        throw new Error(box.errorMsg || '请求失败')
      }
      return box.data as T
    }
    return res as T // raw payload (binary endpoints that bypass the envelope)
  } catch (err) {
    if (err instanceof Error) throw err
    throw new Error(String(err))
  }
}

/** GET with query params → command args. */
export function get<T>(path: string, opts?: { params?: Record<string, unknown> }): Promise<{ data: T }> {
  return call<T>(path, opts?.params).then((data) => ({ data }))
}

/** POST with a JSON body → command `req` arg. */
export function post<T>(path: string, body?: unknown, _config?: unknown): Promise<{ data: T }> {
  return call<T>(path, body).then((data) => ({ data }))
}

/** Direct invoke without the axios-shaped wrapper (for binary/SSE helpers). */
export function invokeRaw<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  return invoke<T>(command, args)
}

/**
 * Invoke a command that takes top-level args (not wrapped in `req`) and returns
 * an ApiResponse envelope. Used by upload commands whose Rust params are
 * `file`/`file_name`/`path` etc. rather than a single `req` struct.
 */
export async function invokeEnvelope<T>(
  command: string,
  args: Record<string, unknown>,
): Promise<T> {
  const res = await invoke<ApiResponse<T>>(command, args)
  if (!res.isSuccess) throw new Error(res.errorMsg || '请求失败')
  return res.data as T
}

export default { get, post, invokeRaw, invokeEnvelope }
