/**
 * 阅读页「全屏加载圈」的显示判定。
 *
 * 书源正文要经过「抓页面 → 跑规则/脚本 → 请求段评」几秒级的过程, 立刻亮圈是对的;
 * 本地 TXT/EPUB/PDF 的正文就在磁盘上, 一章是毫秒级的读盘 + 清洗。可目录跳章、
 * 跨章翻页时目标章通常不在预载缓存里（预载只覆盖相邻几章）, `store.loading`
 * 会在请求发起的同一刻置 true, 阅读页于是把旧正文整段卸载、画一个大圈, 下一帧
 * 又被新正文顶掉 —— 这就是「本地书翻页闪一下加载动画」。
 *
 * 所以本地书的圈延后 `LOCAL_CHAPTER_LOADING_DELAY_MS` 再显示: 毫秒级读完就完全
 * 不出现, 真的慢（大 EPUB 解压、机械盘、网络盘）才给反馈。
 *
 * 注意只影响**可见动画**, 不改 `store.loading` 本身: 分页落位
 * （readerLandingPageIndexAfterPrevChapter）、进度保存都拿它判断"正文是否还在路上"。
 */

/** 本地书加载圈延迟显示的时长; 与书源「复用在途预载」那套延迟保持同一量级。 */
export const LOCAL_CHAPTER_LOADING_DELAY_MS = 200

/** 加载态本身该不该成立: 连续阅读的原地刷新必须保留已渲染的章节 DOM。 */
export function isChapterLoadingActive(options: {
  loading: boolean
  continuousMode: boolean
  refreshing: boolean
}): boolean {
  return options.loading && !(options.continuousMode && options.refreshing)
}

/** 这次加载态要不要先压住不显示(等延迟计时器)。 */
export function shouldDeferChapterLoading(options: {
  loading: boolean
  /** 这套延迟是否适用于当前书(本地 TXT/EPUB 是, 书源与本地 PDF 不是)。 */
  deferrable: boolean
  localIndicatorVisible: boolean
  /** 页面上是否已经挂着上一章的正文。首次打开书时没有, 那时该立刻显示加载态。 */
  hasRenderedChapter: boolean
}): boolean {
  return options.loading
    && options.deferrable
    && !options.localIndicatorVisible
    && options.hasRenderedChapter
}

/** 最终是否画全屏加载圈。 */
export function shouldShowChapterLoading(options: {
  loading: boolean
  continuousMode: boolean
  refreshing: boolean
  deferrable: boolean
  localIndicatorVisible: boolean
  hasRenderedChapter: boolean
}): boolean {
  if (!isChapterLoadingActive(options)) return false
  return !shouldDeferChapterLoading(options)
}
