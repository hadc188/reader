import { get, post, invokeEnvelope } from './invoke'
import type { BookSource, BookSourceTestResponse } from '../types'
import { MAX_SOURCE_TEST_BATCH_SIZE } from '../utils/sourceTesting'

export function getBookSources() {
  return get<BookSource[]>('/getBookSources').then((r) => r.data)
}

export function loginBookSource(bookSourceUrl: string) {
  return post<{
    success: boolean
    status: number
    url: string
    checkResult?: string | null
    bodyPreview?: string
    bodyHtml?: string
  }>('/loginBookSource', { bookSourceUrl }).then((r) => r.data)
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

export async function readSourceFile(file: File) {
  return invokeEnvelope<BookSource[]>('read_source_file', {
    fileName: file.name,
    file: new Uint8Array(await file.arrayBuffer()),
  })
}

export function exportBookSources(sources: BookSource[]) {
  return invokeEnvelope<{ saved: boolean; cancelled?: boolean; path?: string }>(
    'export_book_sources_to_file',
    { sources },
  )
}
