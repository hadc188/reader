/**
 * 应用级全屏切换。
 *
 * 必须走 Tauri 的**窗口全屏**（setFullscreen），而不是 HTML5 的
 * document.documentElement.requestFullscreen()：窗口是无边框的
 * （decorations: false），自定义标题栏由前端绘制 —— HTML5 全屏只把 webview
 * 铺满屏幕，窗口本身仍是普通窗口，--app-height 与真实视口在切换期间不一致，
 * 顶部标题栏与底部就会露出错位/黑条（用户反馈的 F11 BUG）。
 *
 * 浏览器里（开发调试 / Web 阅读模式）没有 Tauri API，回退到 HTML5 全屏。
 *
 * 「最大化状态下进全屏会露出任务栏高度的黑边」由 tao 修好
 * （tauri-apps/tao#1353，tao 0.37.2 起包含）。这里刻意**不再**用
 * 「进全屏前先 unmaximize、退出后再 maximize」的变通：那虽也能绕开黑边，
 * 但 unmaximize/maximize 各对应一次 ShowWindow，且都由 tao 异步派发到事件
 * 循环线程 —— 退出全屏时必然先看到一次窗口化动画、再来一次最大化动画。
 *
 * ## 为什么住在 composable 而不是阅读页
 *
 * 全屏是**窗口级**状态，不随路由变化。早先它只存在于 ReaderView 的 keydown
 * 监听里，用户按 F11 进全屏、返回书架后监听器随组件卸载一起消失，于是再也
 * 按不出全屏（issue #33 补充反馈）。现在 App.vue 始终挂一个兜底的 F11 监听，
 * 阅读页仍可为了「弹层面板打开时也放行」等场景自己处理，两边共用这里的状态。
 */

/** 全屏切换后布局会重排：立刻同步一次视口尺寸，并让位置恢复重新对齐。
 *
 *  Tauri 的全屏事件也会触发 window resize，这里只是把首次同步提前，
 *  避免中间帧按旧高度渲染。 */
function afterFullscreenToggle() {
  // 动态导入避免在非浏览器环境（单元测试）里解析 DOM 相关模块。
  void import('../utils/viewport')
    .then(({ syncViewportSize }) => {
      syncViewportSize()
      window.requestAnimationFrame(() => {
        syncViewportSize()
      })
    })
    .catch(() => undefined)
}

/** 本端追踪的全屏期望状态，仅用于连续触发时不错乱。
 *
 *  不能只依赖 `setFullscreen(!(await isFullscreen()))`：两次 IPC 往返之间用户
 *  再按一次 F11，第二次会读到**过期**的 isFullscreen，于是把同一个值又设一遍 ——
 *  用户连按两下期望「进→出」，实际停在「进」。本地记一份期望值，并在切换后
 *  再用真实状态校准，就能让连按变成两次真切换。 */
let fullscreenToggleInFlight: Promise<void> | null = null
let desiredFullscreen: boolean | null = null

function isTauri() {
  return typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window
}

async function toggleTauriFullscreen() {
  const { getCurrentWindow } = await import('@tauri-apps/api/window')
  const appWindow = getCurrentWindow()
  const base = desiredFullscreen ?? (await appWindow.isFullscreen())
  desiredFullscreen = !base
  await appWindow.setFullscreen(desiredFullscreen)
  // 用真实状态校准：万一设置没生效（被系统拒、窗口已销毁），下一次仍能正确取反。
  try {
    desiredFullscreen = await appWindow.isFullscreen()
  } catch {
    desiredFullscreen = null
  }
  afterFullscreenToggle()
}

/**
 * 切换当前窗口的全屏状态。
 *
 * 返回的 Promise 在切换完成后 resolve；调用方不需要 await（快捷键路径直接丢弃）。
 * 失败时抛出，由调用方决定是否提示。
 */
export function toggleAppFullscreen(): Promise<void> {
  if (isTauri()) {
    // 串行化：快速连按 F11 时不并发发 IPC，否则两次 setFullscreen 的到达顺序
    // 无法保证，结果可能是用户不想要的那个。
    const run = (fullscreenToggleInFlight ?? Promise.resolve()).then(toggleTauriFullscreen)
    // 队列本身必须"永不拒绝"，否则下一次 `.then` 会跳过 —— 但**返回给调用方的
    // 是 `run`**，失败要能被调用方 catch 到（不能在这里吞掉，否则快捷键路径
    // 永远弹不出失败提示）。
    fullscreenToggleInFlight = run.catch(() => undefined).finally(() => {
      if (fullscreenToggleInFlight === run) fullscreenToggleInFlight = null
    })
    return run
  }
  if (typeof document === 'undefined') return Promise.resolve()
  if (document.fullscreenElement) {
    return document
      .exitFullscreen()
      .then(() => afterFullscreenToggle())
      .catch(() => undefined)
  }
  return document.documentElement
    .requestFullscreen()
    .then(() => afterFullscreenToggle())
    .catch(() => undefined)
}

/** 切换失败后清掉期望值，让下一次取反基于真实状态重新判断。 */
export function resetFullscreenIntent() {
  desiredFullscreen = null
}

/** 仅测试用：重置模块级状态，避免用例之间互相影响。 */
export function __resetFullscreenStateForTest() {
  fullscreenToggleInFlight = null
  desiredFullscreen = null
}
