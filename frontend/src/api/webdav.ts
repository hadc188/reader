import { get, post, invokeEnvelope, invokeRaw } from './invoke'

export interface WebdavFileEntry {
  name: string
  size: number
  path: string
  lastModified: number
  isDirectory: boolean
}

export interface BackupArchiveFile {
  name: string
  content: string
}

export interface WebdavBinaryResponse {
  bytes: number[] | Uint8Array | ArrayBuffer
  content_type?: string | null
}

export interface LegadoWebdavConfig {
  url: string
  account: string
  password: string
  directory?: string
}

export interface LegadoBookProgress {
  name: string
  author: string
  durChapterIndex: number
  durChapterPos: number
  durChapterTime: number
  durChapterTitle?: string | null
}

export interface LegadoProgressResponse {
  configured: boolean
  remote?: LegadoBookProgress | null
  uploaded: boolean
}

export interface LegadoWebdavBackupEntry {
  name: string
  size: number
  lastModified: number
}

export interface SaveBackupResult {
  saved: boolean
  cancelled?: boolean
  path?: string
}

export function testLegadoWebdav(config: LegadoWebdavConfig) {
  return invokeEnvelope<{ connected: boolean }>('test_legado_webdav', { config })
}

export function syncLegadoBookProgress(
  config: LegadoWebdavConfig,
  progress: LegadoBookProgress,
  allowUpload = true,
  forceUpload = false,
) {
  return invokeEnvelope<LegadoProgressResponse>('sync_legado_book_progress', {
    req: {
      config,
      progress,
      allowUpload,
      forceUpload,
    },
  })
}

export function listLegadoWebdavBackups(config: LegadoWebdavConfig) {
  return invokeEnvelope<LegadoWebdavBackupEntry[]>('list_legado_webdav_backups', { config })
}

export function uploadLegadoWebdavBackup(
  config: LegadoWebdavConfig,
  filename: string,
  files: BackupArchiveFile[],
) {
  return invokeEnvelope<LegadoWebdavBackupEntry>('upload_legado_webdav_backup', {
    req: { config, filename, files },
  })
}

export function downloadLegadoWebdavBackup(config: LegadoWebdavConfig, filename: string) {
  return invokeRaw<WebdavBinaryResponse>('download_legado_webdav_backup', {
    req: { config, filename },
  })
}

export function saveLegadoWebdavBackupAs(config: LegadoWebdavConfig, filename: string) {
  return invokeEnvelope<SaveBackupResult>('save_legado_webdav_backup_as', {
    req: { config, filename },
  })
}

export function getLegadoWebdavBackupArchive(config: LegadoWebdavConfig, filename: string) {
  return invokeEnvelope<Record<string, string>>('get_legado_webdav_backup_archive', {
    req: { config, filename },
  })
}

export function deleteLegadoWebdavBackup(config: LegadoWebdavConfig, filename: string) {
  return invokeEnvelope<string>('delete_legado_webdav_backup', {
    req: { config, filename },
  })
}

export function getWebdavFileList(path = '/') {
  return get<WebdavFileEntry[]>('/getWebdavFileList', {
    params: { path },
  }).then((r) => r.data)
}

export function getWebdavFileText(path: string) {
  return get<WebdavBinaryResponse>('/getWebdavFile', {
    params: { path },
  }).then((r) => decodeWebdavFileText(r.data))
}

export function getWebdavFileBlob(path: string) {
  return get<WebdavBinaryResponse>('/getWebdavFile', {
    params: { path },
  }).then((r) => createWebdavFileBlob(r.data))
}

export function saveWebdavFileAs(path: string) {
  return invokeEnvelope<SaveBackupResult>('save_webdav_file_as', { req: { path } })
}

export function decodeWebdavFileText(response: WebdavBinaryResponse) {
  return new TextDecoder('utf-8', { fatal: true }).decode(toUint8Array(response.bytes))
}

export function createWebdavFileBlob(response: WebdavBinaryResponse) {
  const bytes = toUint8Array(response.bytes)
  const copy = new Uint8Array(bytes.byteLength)
  copy.set(bytes)
  return new Blob([copy.buffer], {
    type: response.content_type || 'application/octet-stream',
  })
}

function toUint8Array(bytes: WebdavBinaryResponse['bytes']) {
  if (bytes instanceof Uint8Array) return bytes
  if (bytes instanceof ArrayBuffer) return new Uint8Array(bytes)
  if (Array.isArray(bytes)) return Uint8Array.from(bytes)
  throw new Error('文件数据格式无效')
}

export async function uploadFilesToWebdav(files: Array<{ file: Blob; name: string }>, path = '/') {
  const uploadFiles = await Promise.all(
    files.map(async (item) => ({
      name: item.name,
      file: new Uint8Array(await item.file.arrayBuffer()),
    })),
  )
  return invokeEnvelope<WebdavFileEntry[]>('upload_file_to_webdav', { path, files: uploadFiles })
}

export function createWebdavBackupArchive(
  files: BackupArchiveFile[],
  filename: string,
  path = '/backups',
) {
  return invokeEnvelope<WebdavFileEntry>('create_webdav_backup_archive', {
    files,
    filename,
    path,
  })
}

export function getWebdavBackupArchive(path: string) {
  return get<Record<string, string>>('/getWebdavBackupArchive', {
    params: { path },
  }).then((r) => r.data)
}

export function deleteWebdavFile(path: string) {
  return post<string>('/deleteWebdavFile', { path }).then((r) => r.data)
}

export function deleteWebdavFileList(paths: string[]) {
  return post<string>('/deleteWebdavFileList', { path: paths }).then((r) => r.data)
}

export function getWebdavHome() {
  return get<{ path: string }>('/getWebdavHome').then((r) => r.data)
}

export function openWebdavFolder() {
  return post<{ opened: boolean }>('/openWebdavFolder').then((r) => r.data)
}
