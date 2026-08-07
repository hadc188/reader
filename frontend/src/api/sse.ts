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

export function openSse(command: string, args: Record<string, unknown> = {}): SseLike {
  const listeners: Record<string, Array<(event: SseEvent) => void>> = {}
  let closed = false
  let onmessage: SseLike['onmessage'] = null
  let onerror: SseLike['onerror'] = null

  const channel = new Channel<SsePayload>()
  channel.onmessage = (payload: SsePayload) => {
    if (closed) return
    const kind = payload.event || 'data'
    const event: SseEvent = { data: payload }

    if (kind === 'error') {
      listeners['error']?.forEach((cb) => cb(event))
      onerror?.(event)
      return
    }
    if (kind === 'end') {
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
      closed = true
    },
  }
}
