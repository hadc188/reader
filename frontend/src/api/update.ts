import { Channel } from '@tauri-apps/api/core'
import { get, invokeEnvelope, post } from './invoke'
import type { DesktopUpdateProgress, DesktopUpdateResult, VersionUpdateInfo } from '../types'

export function getVersionUpdate(force = false) {
  return get<VersionUpdateInfo>('/getVersionUpdate', {
    params: force ? { force: true } : undefined,
  }).then((r) => r.data)
}

export function dismissVersionUpdate(version: string) {
  return post<VersionUpdateInfo>('/dismissVersionUpdate', { version }).then((r) => r.data)
}

export function applyDesktopVersionUpdate(onProgress?: (progress: DesktopUpdateProgress) => void) {
  const onEvent = new Channel<DesktopUpdateProgress>()
  onEvent.onmessage = (progress) => onProgress?.(progress)
  return invokeEnvelope<DesktopUpdateResult>('apply_desktop_update', { onEvent })
}
