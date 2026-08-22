// reader:// 自定义协议的 origin 因 webview 后端而异:
//   WebView2 (Windows/Android)  → http://reader.localhost
//   WebKitGTK (Linux) / WKWebView (macOS) → reader://localhost
// Tauri 注入的 convertFileSrc 用的正是这套映射(默认 protocol 为 asset),
// 借它探测而不是嗅探 window.location —— dev 模式下所有平台的页面都跑在
// http://localhost:5173, 无法据此区分。单测环境没有 Tauri, 退回 Windows 形式。
import { convertFileSrc } from '@tauri-apps/api/core'

export const readerOrigin: string = (() => {
  try {
    const probe = convertFileSrc('probe', 'reader')
    if (probe.endsWith('/probe')) return probe.slice(0, -'/probe'.length)
  } catch {
    /* 不在 Tauri 内运行(单元测试) */
  }
  return 'http://reader.localhost'
})()
