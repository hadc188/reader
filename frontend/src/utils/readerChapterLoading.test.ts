import { describe, expect, it } from 'vitest'
import {
  isChapterLoadingActive,
  shouldDeferChapterLoading,
  shouldShowChapterLoading,
} from './readerChapterLoading'

const base = {
  loading: true,
  continuousMode: false,
  refreshing: false,
  deferrable: false,
  localIndicatorVisible: false,
  hasRenderedChapter: true,
}

describe('reader chapter loading visibility', () => {
  it('shows the network book indicator immediately', () => {
    expect(shouldShowChapterLoading(base)).toBe(true)
    expect(shouldDeferChapterLoading(base)).toBe(false)
  })

  it('defers the local book indicator until the delay elapses', () => {
    const local = { ...base, deferrable: true }
    // 本地书读盘是毫秒级的: 延迟窗口内不画圈, 正文直接顶上
    expect(shouldShowChapterLoading(local)).toBe(false)
    expect(shouldDeferChapterLoading(local)).toBe(true)
    // 真的慢了(大 EPUB 解压/机械盘), 延迟结束仍要给反馈
    expect(shouldShowChapterLoading({ ...local, localIndicatorVisible: true })).toBe(true)
  })

  it('still shows the indicator on the very first open of a local book', () => {
    // 首次开书时屏上还没有正文, 压住动画会让用户对着空白页以为卡死
    const firstOpen = { ...base, deferrable: true, hasRenderedChapter: false }
    expect(shouldShowChapterLoading(firstOpen)).toBe(true)
    expect(shouldDeferChapterLoading(firstOpen)).toBe(false)
  })

  it('hides the indicator once loading finishes even for a local book', () => {
    expect(shouldShowChapterLoading({ ...base, deferrable: true, loading: false })).toBe(false)
    expect(shouldDeferChapterLoading({ ...base, deferrable: true, loading: false })).toBe(false)
  })

  it('keeps the continuous in-place refresh from unmounting the chapter list', () => {
    expect(isChapterLoadingActive({ loading: true, continuousMode: true, refreshing: true })).toBe(false)
    expect(shouldShowChapterLoading({ ...base, continuousMode: true, refreshing: true })).toBe(false)
    // 连续阅读下真正换章(非原地刷新)仍然要显示
    expect(shouldShowChapterLoading({ ...base, continuousMode: true, refreshing: false })).toBe(true)
  })
})
