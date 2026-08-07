import { get, post } from './invoke'
import type { VersionUpdateInfo } from '../types'

export function getVersionUpdate(force = false) {
  return get<VersionUpdateInfo>('/getVersionUpdate', {
    params: force ? { force: true } : undefined,
  }).then((r) => r.data)
}

export function dismissVersionUpdate(version: string) {
  return post<VersionUpdateInfo>('/dismissVersionUpdate', { version }).then((r) => r.data)
}
