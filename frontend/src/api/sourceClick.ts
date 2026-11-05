import { invoke } from '@tauri-apps/api/core'
import type { ApiResponse } from '../types'

/** 书源 JS 调用 `java.showBrowser` 时捕获的面板请求（legado 的 BottomWebViewDialog）。 */
export interface ShowBrowserRequest {
  url: string
  html?: string | null
  preloadJs?: string | null
  options?: Record<string, unknown> | null
  title?: string | null
  awaitResult?: boolean
  /** 后端托管的面板 id：iframe 加载 `${readerOrigin}/sourcePanel?panelId=...`。 */
  panelId?: string | null
  /**
   * 用**独立窗口**打开，而不是 iframe 面板。
   *
   * 只用于「看一眼就走」的链接（注册页、用户后台、网页版阅读页）：这类页面通过
   * X-Frame-Options / CSP frame-ancestors，或用前端安全 SDK 检测「是否被 iframe
   * 嵌入」并整页替换成拦截提示。独立窗口里 window.top === window，这些检测都不触发。
   */
  externalWindow?: boolean
  /**
   * 发起这次打开操作的**书源地址**（`bookSourceUrl`）。
   *
   * 由后端随请求带回，前端要原样转给 `openExternalWindow` 的第三个参数：
   * 独立窗口据此套用书源自己的 User-Agent、按移动/桌面选窗口尺寸、并把已有
   * Cookie 注入窗口。缺了它，移动端 H5 登录页（如 `fanqienovel.com`）会按
   * 桌面宽度渲染，显示不全且无法下滑。
   */
  bookSourceUrl?: string | null
}

export function closeSourcePanel(panelId: string) {
  return invoke<void>('close_source_panel', { panelId })
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
/**
 * 把链接交给**独立窗口**打开（注册页、用户后台、网页版阅读等）。
 *
 * 这类页面常带 `X-Frame-Options: SAMEORIGIN` 或 CSP `frame-ancestors`，或用
 * 前端安全 SDK 主动检测「自己是否被 iframe 嵌入」，命中就把整页替换成
 * 「页面渲染安全策略拦截」。独立窗口里 `window.top === window`，检测不触发 ——
 * 与手机端 legado 直接 `loadUrl` 的行为一致。
 */
export function openExternalWindow(url: string, title?: string, bookSourceUrl?: string) {
  return invoke<void>('open_external_window', {
    url,
    title: title ?? null,
    // 传了书源地址才会在窗口关闭时把 Cookie 收进它的 jar —— 这是「窗口里登录
    // 成功、回到面板仍显示未登录」的解法。
    bookSourceUrl: bookSourceUrl ?? null,
  })
}