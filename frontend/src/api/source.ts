import { get, post, invokeEnvelope, invokeUpload } from './invoke'
import { FILE_NAME_HEADER, encodeFileName } from './uploadHeader'
import type { BookSource, BookSourceTestResponse } from '../types'
import { MAX_SOURCE_TEST_BATCH_SIZE } from '../utils/sourceTesting'
import { Channel } from '@tauri-apps/api/core'
import type { ShowBrowserRequest } from './sourceClick'

export function getBookSources() {
  return get<BookSource[]>('/getBookSources').then((r) => r.data)
}

export interface SourceLoginField {
  name: string
  /** `label` 是纯展示控件（legado 的 `RowUi.Type` 之外的扩展类型，只当说明文字渲染）。 */
  type: 'text' | 'password' | 'button' | 'toggle' | 'select' | 'label'
  action?: string | null
  viewName?: string | null
  placeholder?: string | null
  chars?: unknown[] | null
  style?: Record<string, unknown> | null
  default?: unknown
}

export interface SourceLoginView {
  mode: 'form' | 'web' | 'cookie'
  fields: SourceLoginField[]
  values: Record<string, string>
  url?: string | null
  openUrl?: string | null
  loginSession: string
  messages: string[]
  actionCompleted: boolean
  showBrowser?: ShowBrowserRequest | null
  cancelled?: boolean
  /**
   * `mode: "web"` 时后端建议用**独立窗口**打开登录页。
   *
   * 这类页面常带 `X-Frame-Options: SAMEORIGIN` / CSP `frame-ancestors`，或用
   * 前端安全 SDK 检测「是否被 iframe 嵌入」并整页替换成拦截提示。独立窗口里
   * `window.top === window`，检测不触发 —— 与手机端 legado 直接 `loadUrl` 一致。
   */
  externalWindow?: boolean
}

export function loginBookSource(bookSourceUrl: string, action?: string, values?: Record<string, string>, onBrowser?: (request: ShowBrowserRequest) => void) {
  const channel = new Channel<ShowBrowserRequest>()
  if (onBrowser) channel.onmessage = onBrowser
  return invokeEnvelope<SourceLoginView>('login_book_source', {
    req: { bookSourceUrl, action, values }, onBrowser: channel,
  })
}

/** 手动导入书源登录 Cookie(如通过抓包获取的会话)。 */
export function setBookSourceCookie(bookSourceUrl: string, cookie: string) {
  return post<{ success: boolean; saved: boolean }>('/setBookSourceCookie', {
    bookSourceUrl,
    cookie,
  }).then((r) => r.data)
}

export function saveBookSource(source: BookSource) {
  return post<{ saved: boolean }>('/saveBookSource', source).then((r) => r.data)
}

export function saveBookSources(sources: BookSource[]) {
  return post<{ saved: boolean; count: number }>('/saveBookSources', sources).then((r) => r.data)
}

export function deleteBookSource(bookSourceUrl: string) {
  return post<{ deleted: boolean }>('/deleteBookSource', { bookSourceUrl }).then((r) => r.data)
}

export function deleteBookSources(sources: { bookSourceUrl: string }[]) {
  return post<{ deleted: boolean }>('/deleteBookSources', sources).then((r) => r.data)
}

export function deleteAllBookSources() {
  return post<{ deleted: boolean }>('/deleteAllBookSources').then((r) => r.data)
}

export function pinBookSource(bookSourceUrl: string) {
  return post<{ success: boolean }>('/pinBookSource', { bookSourceUrl }).then((r) => r.data)
}

export function unpinBookSource(bookSourceUrl: string) {
  return post<{ success: boolean }>('/unpinBookSource', { bookSourceUrl }).then((r) => r.data)
}

export function testBookSources(params: {
  bookSourceUrls?: string[]
  keyword?: string
  markInvalid?: boolean
  concurrent?: number
  taskId?: string
}) {
  if ((params.bookSourceUrls?.length || 0) > MAX_SOURCE_TEST_BATCH_SIZE) {
    throw new Error(`单次最多测试 ${MAX_SOURCE_TEST_BATCH_SIZE} 个书源`)
  }
  return post<BookSourceTestResponse>('/testBookSources', params).then((r) => r.data)
}

export function cancelBookSourceTest(taskId: string) {
  return post<{ cancelled: boolean }>('/cancelBookSourceTest', { taskId }).then((r) => r.data)
}

export function deleteInvalidBookSources() {
  return post<{ deleted: number }>('/deleteInvalidBookSources').then((r) => r.data)
}

export function readRemoteSourceFile(url: string) {
  return post<string[]>('/readRemoteSourceFile', { url }).then((r) => r.data)
}

/** 读取本地书源 JSON。字节走 octet-stream 快速通道, 文件名走 IPC header。 */
export async function readSourceFile(file: File) {
  return invokeUpload<BookSource[]>(
    'read_source_file',
    { [FILE_NAME_HEADER]: encodeFileName(file.name) },
    file,
  )
}

export function exportBookSources(sources: BookSource[]) {
  return invokeEnvelope<{ saved: boolean; cancelled?: boolean; path?: string }>(
    'export_book_sources_to_file',
    { sources },
  )
}
