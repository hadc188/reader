// SseLike shim over Tauri IPC channels. Preserves the EventSource surface the
// consuming components rely on (onmessage / onerror / addEventListener('end'|'error')
// / close), so the migration is a one-line change per consumer:
//   `JSON.parse(event.data)` → `event.data`  (the payload is already an object).
//
// The Rust side sends `{ event: "data"|"end"|"error", ...fields }`; this shim
// dispatches by the `event` discriminator to the matching listeners.

import { Channel, invoke } from '@tauri-apps/api/core'

export interface SseLike {
  onmessage: ((event: { data: unknown }) => void) | null
  onerror: ((event: unknown) => void) | null
  addEventListener(type: string, cb: (event: { data: unknown }) => void): void
  close(): void
}

type SsePayload = {
  event?: 'data' | 'end' | 'error'
  errorMsg?: string
  [key: string]: unknown
}

type SseEvent = { data: unknown }

export interface SseOpenOptions {
  /**
   * 主动关闭流时通知后端中断对应任务。
   *
   * 后端任务是在 `tokio::spawn` 里跑的, `close()` 只是让本地停止接收消息 ——
   * 不额外发这个命令的话, 用户离开搜索界面后剩余书源仍会被逐个请求完。
   * 只有在「未收到 end/error 就关闭」时才发送, 正常结束不会多发一次取消。
   */
  cancel?: { command: string; taskId: string }
}

/** 生成一个搜索任务的唯一 id(前端发起, 后端按它登记取消标志)。 */
export function newSseTaskId(prefix: string): string {
  const unique = globalThis.crypto?.randomUUID?.()
    ?? `${Date.now()}-${Math.random().toString(36).slice(2)}`
  return `${prefix}-${unique}`
}

export function openSse(
  command: string,
  args: Record<string, unknown> = {},
  options: SseOpenOptions = {},
): SseLike {
  const listeners: Record<string, Array<(event: SseEvent) => void>> = {}
  let closed = false
  // 收到 end/error 说明后端任务已经自己结束, 此时关闭不需要再发取消。
  let finished = false
  let onmessage: SseLike['onmessage'] = null
  let onerror: SseLike['onerror'] = null

  const channel = new Channel<SsePayload>()
  channel.onmessage = (payload: SsePayload) => {
    if (closed) return
    const kind = payload.event || 'data'
    const event: SseEvent = { data: payload }

    if (kind === 'error') {
      finished = true
      listeners['error']?.forEach((cb) => cb(event))
      onerror?.(event)
      return
    }
    if (kind === 'end') {
      finished = true
      listeners['end']?.forEach((cb) => cb(event))
      return
    }
    listeners['message']?.forEach((cb) => cb(event))
    onmessage?.(event)
  }

  // The Rust SSE commands take `req: XxxRequest` + `on_event: Channel`. Wrap
  // the args in `req` (Tauri 2 defaults command args to camelCase, so `onEvent`
  // matches `on_event`).
  invoke(command, { req: args, onEvent: channel }).catch((err) => {
    if (closed) return
    finished = true
    const event: SseEvent = { data: String(err) }
    listeners['error']?.forEach((cb) => cb(event))
    onerror?.(event)
  })

  return {
    get onmessage() {
      return onmessage
    },
    set onmessage(fn) {
      onmessage = fn
    },
    get onerror() {
      return onerror
    },
    set onerror(fn) {
      onerror = fn
    },
    addEventListener(type, cb) {
      if (!listeners[type]) listeners[type] = []
      listeners[type].push(cb)
    },
    close() {
      if (closed) return
      closed = true
      if (!finished && options.cancel) {
        invoke(options.cancel.command, { req: { taskId: options.cancel.taskId } }).catch((err) => {
          // 静默吞掉会让"取消命令失效"表现成"后端还在跑"且无任何线索: 留下日志便于排查。
          console.warn('[sse] 取消后端任务失败', options.cancel?.command, err)
        })
      }
    },
  }
}
