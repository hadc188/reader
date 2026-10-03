import { invoke } from '@tauri-apps/api/core'
import type { ApiResponse } from '../types'

/** 书源 JS 调用 `java.showBrowser` 时捕获的面板请求（legado 的 BottomWebViewDialog）。 */
export interface ShowBrowserRequest {
  url: string
  html?: string | null
  preloadJs?: string | null
  /** 后端托管的面板 id：iframe 加载 `${readerOrigin}/sourcePanel?panelId=...`。 */
  panelId?: string | null
}

/** 正文图片 `click` 脚本的执行结果。 */
export interface SourceClickResult {
  result: string
  logs: string[]
  showBrowser?: ShowBrowserRequest | null
}

/**
 * 执行正文图片 `,{json}` 里的 `click` 脚本。
 * 脚本在书源 jsLib 作用域运行，因此插件定义的 `showCommentPanel(...)` 可直接调用。
 */
export async function evalSourceClick(params: {
  bookSourceUrl: string
  script: string
  result?: string
  bookUrl?: string
  chapterUrl?: string
}): Promise<SourceClickResult> {
  const res = await invoke<ApiResponse<SourceClickResult>>('eval_source_click', { req: params })
  if (!res.isSuccess) throw new Error(res.errorMsg || '执行点击脚本失败')
  return res.data as SourceClickResult
}
