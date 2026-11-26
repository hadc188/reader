// Tauri IPC 传输实现。逻辑从原 `invoke.ts` 搬来, 保持逐字节等价。

import { invoke } from '@tauri-apps/api/core'
import type { Transport } from './index'

/** 与 Rust 侧 `crate::api::commands::FILE_NAME_HEADER` 必须一致。 */
const FILE_NAME_HEADER = 'x-reader-file-name'

export const tauriTransport: Transport = {
  // IPC 没有方法之分, `method` 只为满足接口 —— GET 与 POST 走同一条 invoke。
  async call<T>(path: string, payload: unknown, _method: 'GET' | 'POST'): Promise<T> {
    const command = COMMAND_BY_PATH[path]
    if (!command) throw new Error(`未知接口: ${path}`)
    return invoke<T>(command, payload === undefined ? {} : { req: payload })
  },

  async upload<T>(
    command: string,
    file: Blob,
    fileName: string,
    headers: Record<string, string> = {},
  ): Promise<T> {
    // 文件名走 header: header 值只能是 ASCII, 中文名先 percent-encode。
    // 字节作为**顶层**载荷, 否则会落到 JSON 分支被展开成十进制数字数组
    // (一本 50MB 的 TXT 会涨到约 178MB 字符串)。
    return invoke<T>(command, new Uint8Array(await file.arrayBuffer()), {
      headers: { ...headers, [FILE_NAME_HEADER]: encodeURIComponent(fileName) },
    })
  },
}

/**
 * `/reader3/*` → command 名。与原 `invoke.ts` 的 `COMMAND_BY_PATH` 完全一致。
 *
 * 注意: `getBookContent` 不在表里 —— 它需要额外的 Channel 参数, 由
 * `api/bookshelf.ts` 直接 invoke, 不走通用通道。
 */
export const COMMAND_BY_PATH: Record<string, string> = {
  '/reader3/getBookSources': 'get_book_sources',
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
  '/reader3/getExploreKinds': 'get_explore_kinds',
  '/reader3/runExploreAction': 'run_explore_action',
  '/reader3/searchBookMulti': 'search_book_multi',
  '/reader3/getBookshelf': 'get_bookshelf',
  '/reader3/getShelfBookWithCacheInfo': 'get_shelf_book_with_cache_info',
  '/reader3/importLocalBookFolder': 'import_local_book_folder',
  '/reader3/getBookGroups': 'get_book_groups',
  '/reader3/saveBookGroup': 'save_book_group',
  '/reader3/saveBookGroupOrder': 'save_book_group_order',
  '/reader3/deleteBookGroup': 'delete_book_group',
  '/reader3/deleteBookGroups': 'delete_book_groups',
  '/reader3/saveBookGroupId': 'save_book_group_id',
  '/reader3/setBooksGroupMask': 'set_books_group_mask',
  '/reader3/saveBook': 'save_book',
  '/reader3/saveBooks': 'save_books',
  '/reader3/setBookSource': 'set_book_source',
  '/reader3/deleteBook': 'delete_book',
  '/reader3/deleteBooks': 'delete_books',
  '/reader3/saveBookProgress': 'save_book_progress',
  '/reader3/getBookInfo': 'get_book_info',
  '/reader3/getChapterList': 'get_chapter_list',
  '/reader3/getCacheOverview': 'get_cache_overview',
  '/reader3/getCacheManifest': 'get_cache_manifest',
  '/reader3/deleteBookCache': 'delete_book_cache',
  '/reader3/clearSearchCoverCache': 'clear_search_cover_cache',
  '/reader3/getCachedChapterUrls': 'get_cached_chapter_urls',
  '/reader3/getChapterWordCounts': 'get_chapter_word_counts',
  '/reader3/getAvailableBookSource': 'get_available_book_source',
  '/reader3/getRssSources': 'get_rss_sources',
  '/reader3/saveRssSource': 'save_rss_source',
  '/reader3/saveRssSources': 'save_rss_sources',
  '/reader3/deleteRssSource': 'delete_rss_source',
  '/reader3/deleteRssSources': 'delete_rss_sources',
  '/reader3/readRemoteRssSourceFile': 'read_remote_rss_source_file',
  '/reader3/getRssArticles': 'get_rss_articles',
  '/reader3/getRssContent': 'get_rss_content',
  '/reader3/openRssSourcePanel': 'open_rss_source_panel',
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
  // 净化脚本规则：只在前端存在 @js: 规则时才会走到（普通规则走前端同步快路径）。
  '/reader3/applyReplaceRules': 'apply_replace_rules',
  // Edge 朗读：音色列表走信封，合成结果是二进制 mp3（api/edgeTts.ts 用 invokeRaw）。
  '/reader3/edgeTtsListVoices': 'edge_tts_list_voices',
  '/reader3/synthesizeEdgeTts': 'edge_tts_synthesize',
  // 微软朗读（演示接口）：同样支持风格与多档音质。
  '/reader3/msTtsListVoices': 'ms_tts_list_voices',
  '/reader3/synthesizeMsTts': 'ms_tts_synthesize',
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


