import { get, post, invokeEnvelope } from './invoke'

export interface WebdavFileEntry {
  name: string
  size: number
  path: string
  lastModified: number
  isDirectory: boolean
}

export function getWebdavFileList(path = '/') {
  return get<WebdavFileEntry[]>('/getWebdavFileList', {
    params: { path },
  }).then((r) => r.data)
}

export function getWebdavFileText(path: string) {
  return get<string>('/getWebdavFile', {
    params: { path },
  }).then((r) => r.data as unknown as string)
}

export function getWebdavFileBlob(path: string) {
  return get<Blob>('/getWebdavFile', {
    params: { path },
  }).then((r) => r.data)
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

export function uploadTextToWebdav(content: string, filename: string, path = '/') {
  const blob = new Blob([content], { type: 'application/json;charset=utf-8' })
  return uploadFilesToWebdav([{ file: blob, name: filename }], path)
}

export function deleteWebdavFile(path: string) {
  return post<string>('/deleteWebdavFile', { path }).then((r) => r.data)
}

export function deleteWebdavFileList(paths: string[]) {
  return post<string>('/deleteWebdavFileList', { path: paths }).then((r) => r.data)
}
