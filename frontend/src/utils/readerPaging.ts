/** 阅读页翻页的滚动动画与步长计算。
 *
 *  背景：容器的 CSS scroll-behavior 已改为 auto —— 直接赋 scrollTop 是瞬时的，
 *  平滑效果必须由这里显式补间，翻页动画时长才能被「动画时长」设置真正控制。
 *  反过来，若容器保留 scroll-behavior: smooth，赋值 scrollTop 会被浏览器转成
 *  时长不可控的原生平滑动画，设置项就永远不起作用。 */

/** 「动画时长」设置的上限，避免异常配置让一次翻页久久不结束。 */
export const READER_PAGE_ANIMATION_MAX_MS = 1000

/** 无有效行高时的翻页步长回退比例（略小于整屏，避免漏读最后一行）。 */
const PAGE_STEP_FALLBACK_RATIO = 0.9

const animationFrames = new WeakMap<HTMLElement, number>()

/** 把设置里的动画时长归一化为可用的毫秒数；<= 0 表示关闭动画。 */
export function normalizePageAnimationDuration(duration: number): number {
  if (!Number.isFinite(duration) || duration <= 0) return 0
  return Math.min(READER_PAGE_ANIMATION_MAX_MS, Math.round(duration))
}

/** 翻页步长：按正文行高取整，保证翻页后视口顶部落在一行的开头。
 *
 *  原实现是 clientHeight × 0.88，每翻一页都会把上一页末尾的几行重新显示一遍
 *  （连续阅读下尤其明显）。取整到整行后，重叠最多不到一行，同时不会像整屏
 *  滚动那样把末尾半行切掉造成漏读。 */
export function getReaderPageStep(viewportHeight: number, lineHeightPx: number): number {
  if (!Number.isFinite(viewportHeight) || viewportHeight <= 0) return 0

  if (!Number.isFinite(lineHeightPx) || lineHeightPx <= 0) {
    return Math.max(1, Math.floor(viewportHeight * PAGE_STEP_FALLBACK_RATIO))
  }

  const lines = Math.floor(viewportHeight / lineHeightPx)
  // 行高不小于视口时退化为整屏，否则会一步只走一行。
  const step = lines >= 1 ? lines * lineHeightPx : viewportHeight
  return Math.max(1, Math.floor(step))
}

/** 取消该容器上尚未结束的翻页动画。 */
export function cancelPageScrollAnimation(container: HTMLElement | undefined | null) {
  if (!container) return
  const frame = animationFrames.get(container)
  if (frame == null) return
  cancelAnimationFrame(frame)
  animationFrames.delete(container)
}

function easeOutCubic(progress: number) {
  const inverted = 1 - progress
  return 1 - inverted * inverted * inverted
}

function maxScrollTop(container: HTMLElement) {
  return Math.max(0, container.scrollHeight - container.clientHeight)
}

/** 把容器平滑滚动到指定位置；duration <= 0 时立即到位。 */
export function animatePageScrollTo(
  container: HTMLElement,
  targetTop: number,
  duration: number,
) {
  cancelPageScrollAnimation(container)

  if (!Number.isFinite(targetTop)) return
  const target = Math.max(0, Math.min(maxScrollTop(container), targetTop))
  const start = container.scrollTop
  const delta = target - start
  if (delta === 0) return

  const ms = normalizePageAnimationDuration(duration)
  if (ms <= 0) {
    container.scrollTop = target
    return
  }

  const startTime = performance.now()
  // 记录本补间最后一次写入的值。每帧先比对实际位置：连续阅读插入/删除章节后
  // 会用 offsetTop 差值校正 scrollTop，位置恢复也会直接赋值。原生平滑滚动会被
  // 这类程序化滚动取代，而 rAF 补间不会——不主动让位就会把校正覆盖掉。
  let lastWritten = start
  const step = (now: number) => {
    // 容差 1.5px: 浏览器会把 scrollTop 取整/按 DPR 量化, 小于一帧位移的差异
    // 不能当成外部改动。
    if (Math.abs(container.scrollTop - lastWritten) > 1.5) {
      animationFrames.delete(container)
      return
    }
    const progress = Math.min(1, (now - startTime) / ms)
    const next = start + delta * easeOutCubic(progress)
    container.scrollTop = next
    lastWritten = container.scrollTop
    if (progress < 1) {
      animationFrames.set(container, requestAnimationFrame(step))
    } else {
      animationFrames.delete(container)
    }
  }
  animationFrames.set(container, requestAnimationFrame(step))
}

/** 把容器平滑滚动一段距离；duration <= 0 时立即到位。 */
export function animatePageScrollBy(
  container: HTMLElement,
  delta: number,
  duration: number,
) {
  if (!Number.isFinite(delta) || delta === 0) {
    cancelPageScrollAnimation(container)
    return
  }
  animatePageScrollTo(container, container.scrollTop + delta, duration)
}
