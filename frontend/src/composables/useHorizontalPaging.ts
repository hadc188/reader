import { computed, nextTick, ref } from 'vue'
import type { ComputedRef, Ref } from 'vue'
import type { useReaderStore } from '../stores/reader'

type ReaderStore = ReturnType<typeof useReaderStore>
const HORIZONTAL_PAGE_SIDE_PADDING = 24
const HORIZONTAL_PAGE_VERTICAL_PADDING = 24
const HORIZONTAL_PAGE_MIN_BOTTOM_GUARD = 18
const HORIZONTAL_PAGE_BOTTOM_GUARD_LINES = 0.5
const NON_STARTING_PUNCTUATION = new Set([
  '，',
  '。',
  '、',
  '：',
  '；',
  '！',
  '？',
  ',',
  '.',
  ':',
  ';',
  '!',
  '?',
  '”',
  '’',
  '）',
  '】',
  '》',
  '」',
  '』',
])
const SPLIT_BACKTRACK_BREAKERS = new Set(['，', '。', '；', '！', '？', ',', '.', ';', '!', '?', ' '])
const PUNCTUATION_ONLY_CHARS = new Set([...NON_STARTING_PUNCTUATION, '“', '‘', '（', '【', '《', '「', '『', '…', '—'])
const MAX_PUNCTUATION_BACKTRACK = 12

export function useHorizontalPaging(
  store: ReaderStore,
  config: ComputedRef<{ fontSize: number; fontWeight: number; lineHeight: number }>,
  currentFontFamily: ComputedRef<string>,
  formattedContent: ComputedRef<string>,
  isHorizontalPageMode: ComputedRef<boolean>,
  scrollContainerRef: Ref<HTMLElement | undefined>,
) {
  const horizontalPageIndex = ref(0)
  const horizontalPageStep = ref(1)
  const horizontalPageStepStyle = computed(() => `${Math.max(1, horizontalPageStep.value)}px`)
  const horizontalPages = ref<string[]>([])
  const isHorizontalAtEnd = ref(false)

  function escapeHtml(input: string) {
    return input
      .replaceAll('&', '&amp;')
      .replaceAll('<', '&lt;')
      .replaceAll('>', '&gt;')
  }

  function parseInlineStyle(styleText: string) {
    const entries = styleText
      .split(';')
      .map((item) => item.trim())
      .filter(Boolean)
      .map((item) => {
        const idx = item.indexOf(':')
        if (idx <= 0) return null
        const key = item.slice(0, idx).trim().toLowerCase()
        const value = item.slice(idx + 1).trim()
        if (!key || !value) return null
        return [key, value] as const
      })
      .filter(Boolean) as Array<readonly [string, string]>
    return Object.fromEntries(entries) as Record<string, string>
  }

  function buildInlineStyle(styleObj: Record<string, string>) {
    return Object.entries(styleObj)
      .map(([key, value]) => `${key}: ${value}`)
      .join('; ')
  }

  function removeIndentClass(className: string) {
    return className
      .split(/\s+/)
      .filter(Boolean)
      .filter((name) => name !== 'reader-indent')
      .join(' ')
  }

  function normalizeParagraphHtml(paragraphHtml: string) {
    const wrapper = document.createElement('div')
    wrapper.innerHTML = paragraphHtml
    const paragraph = wrapper.querySelector('p')
    if (!paragraph) return paragraphHtml
    const style = paragraph.getAttribute('style') || ''
    const styleObj = parseInlineStyle(style)
    if (paragraph.classList.contains('reader-indent') && !styleObj['text-indent']) {
      styleObj['text-indent'] = '2em'
    }
    const styleText = buildInlineStyle(styleObj)
    if (styleText) paragraph.setAttribute('style', styleText)
    return paragraph.outerHTML
  }

  function parseParagraphHtml(paragraphHtml: string) {
    const wrapper = document.createElement('div')
    wrapper.innerHTML = paragraphHtml
    const paragraph = wrapper.querySelector('p')
    if (!paragraph) return null
    return {
      style: paragraph.getAttribute('style') || '',
      className: paragraph.getAttribute('class') || '',
      text: (paragraph.textContent || '').trimEnd(),
    }
  }

  function buildParagraphHtml(style: string, text: string, className = '') {
    const stylePart = style ? ` style="${style}"` : ''
    const classPart = className ? ` class="${className}"` : ''
    return `<p${classPart}${stylePart}>${escapeHtml(text)}</p>`
  }

  function firstVisibleChar(text: string) {
    return text.trimStart().charAt(0)
  }

  function isPunctuationOnlyText(text: string) {
    const value = text.trim()
    return value.length > 0 && value.length <= 3 && Array.from(value).every((char) => PUNCTUATION_ONLY_CHARS.has(char))
  }

  function adjustFitCountForReadableStart(text: string, fitCount: number) {
    if (fitCount <= 0 || fitCount >= text.length) return fitCount
    if (!NON_STARTING_PUNCTUATION.has(firstVisibleChar(text.slice(fitCount)))) return fitCount

    const minIndex = Math.max(1, fitCount - MAX_PUNCTUATION_BACKTRACK)
    for (let idx = fitCount - 1; idx >= minIndex; idx -= 1) {
      if (SPLIT_BACKTRACK_BREAKERS.has(text[idx])) {
        return idx + 1
      }
    }
    return Math.max(1, fitCount - Math.min(4, fitCount - 1))
  }

  function extractTextFromHtml(html: string) {
    const wrapper = document.createElement('div')
    wrapper.innerHTML = html
    return wrapper.textContent || ''
  }

  function prependTextToFirstParagraph(html: string, text: string) {
    const wrapper = document.createElement('div')
    wrapper.innerHTML = html
    const paragraph = wrapper.querySelector('p')
    if (!paragraph) return `${escapeHtml(text)}${html}`
    paragraph.insertBefore(document.createTextNode(text), paragraph.firstChild)
    return wrapper.innerHTML
  }

  function appendTextToLastParagraph(html: string, text: string) {
    const wrapper = document.createElement('div')
    wrapper.innerHTML = html
    const paragraphs = wrapper.querySelectorAll('p')
    const paragraph = paragraphs[paragraphs.length - 1]
    if (!paragraph) return `${html}${escapeHtml(text)}`
    paragraph.appendChild(document.createTextNode(text))
    return wrapper.innerHTML
  }

  function mergeOrphanPunctuationPages(sourcePages: string[]) {
    const pendingPages = [...sourcePages]
    const mergedPages: string[] = []
    for (let idx = 0; idx < pendingPages.length; idx += 1) {
      const page = pendingPages[idx]
      const text = extractTextFromHtml(page)
      if (isPunctuationOnlyText(text)) {
        const punctuation = text.trim()
        if (mergedPages.length) {
          mergedPages[mergedPages.length - 1] = appendTextToLastParagraph(mergedPages[mergedPages.length - 1], punctuation)
          continue
        }
        if (idx + 1 < pendingPages.length) {
          pendingPages[idx + 1] = prependTextToFirstParagraph(pendingPages[idx + 1], punctuation)
          continue
        }
      }
      mergedPages.push(page)
    }
    return mergedPages
  }

  function buildHorizontalParagraphs() {
    const root = document.createElement('div')
    root.innerHTML = formattedContent.value
    return Array.from(root.querySelectorAll('p')).map((node) => normalizeParagraphHtml(node.outerHTML))
  }

  function updateHorizontalMetrics() {
    const container = scrollContainerRef.value
    if (!container || !isHorizontalPageMode.value) return
    horizontalPageStep.value = Math.max(1, container.clientWidth)
  }

  function getHorizontalPageMeasure(container: HTMLElement) {
    const lineHeightPx = config.value.fontSize * config.value.lineHeight
    const bottomGuard = Math.max(
      HORIZONTAL_PAGE_MIN_BOTTOM_GUARD,
      Math.ceil(lineHeightPx * HORIZONTAL_PAGE_BOTTOM_GUARD_LINES),
    )
    return {
      innerWidth: Math.max(120, horizontalPageStep.value - HORIZONTAL_PAGE_SIDE_PADDING * 2),
      pageHeight: Math.max(160, container.clientHeight - HORIZONTAL_PAGE_VERTICAL_PADDING * 2 - bottomGuard),
    }
  }

  function updateHorizontalEndState() {
    const container = scrollContainerRef.value
    if (!container || !isHorizontalPageMode.value) {
      isHorizontalAtEnd.value = false
      return
    }
    updateHorizontalMetrics()
    const maxPage = Math.max(0, horizontalPages.value.length - 1)
    isHorizontalAtEnd.value = horizontalPageIndex.value >= maxPage
  }

  async function rebuildHorizontalPages() {
    if (!isHorizontalPageMode.value) {
      horizontalPages.value = []
      return
    }
    await nextTick()
    const container = scrollContainerRef.value
    if (!container) return

    updateHorizontalMetrics()

    const { innerWidth, pageHeight } = getHorizontalPageMeasure(container)
    const title = (store.currentChapter?.title || '加载中...').trim()
    const titleHtml = `<h1 class="horizontal-flow-title">${escapeHtml(title)}</h1>`
    const paragraphs = buildHorizontalParagraphs()

    const measurer = document.createElement('div')
    measurer.style.position = 'fixed'
    measurer.style.left = '-99999px'
    measurer.style.top = '0'
    measurer.style.width = `${innerWidth}px`
    measurer.style.height = `${pageHeight}px`
    measurer.style.overflow = 'hidden'
    measurer.style.visibility = 'hidden'
    measurer.style.pointerEvents = 'none'
    measurer.style.boxSizing = 'border-box'
    measurer.style.fontSize = `${config.value.fontSize}px`
    measurer.style.fontWeight = String(config.value.fontWeight)
    measurer.style.lineHeight = String(config.value.lineHeight)
    measurer.style.fontFamily = currentFontFamily.value || ''
    measurer.style.wordBreak = 'normal'
    measurer.style.overflowWrap = 'break-word'
    measurer.style.textAlign = 'left'
    document.body.appendChild(measurer)

    const pages: string[] = []
    let currentParts: string[] = [titleHtml]

    const measureContentHeight = (parts: string[]) => {
      measurer.innerHTML = parts.join('')
      const children = Array.from(measurer.children) as HTMLElement[]
      if (!children.length) return 0
      const top = measurer.getBoundingClientRect().top
      return children.reduce((bottom, child, idx) => {
        const rect = child.getBoundingClientRect()
        const style = window.getComputedStyle(child)
        const isLastParagraph = idx === children.length - 1 && child.tagName.toLowerCase() === 'p'
        const marginBottom = isLastParagraph ? 0 : parseFloat(style.marginBottom) || 0
        return Math.max(bottom, rect.bottom - top + marginBottom)
      }, 0)
    }

    const overflows = (parts: string[]) => {
      return measureContentHeight(parts) > measurer.clientHeight
    }

    const flushPage = () => {
      if (!currentParts.length) return
      pages.push(currentParts.join(''))
      currentParts = []
    }

    const buildSegmentStyle = (style: string, isContinuation: boolean, hasMoreText: boolean) => {
      const styleObj = parseInlineStyle(style)
      if (hasMoreText) delete styleObj['margin-bottom']
      if (isContinuation) delete styleObj['text-indent']
      return buildInlineStyle(styleObj)
    }

    const fitParagraphSegment = (
      blockHtml: string,
      options: { isContinuation: boolean; minRemainingLines?: number },
    ) => {
      const parsed = parseParagraphHtml(blockHtml)
      if (!parsed || parsed.text.length <= 1) return null

      const { style, text, className } = parsed
      const currentHeight = measureContentHeight(currentParts)
      const remainingHeight = measurer.clientHeight - currentHeight
      const minRemainingHeight = (options.minRemainingLines || 0) * config.value.fontSize * config.value.lineHeight
      if (remainingHeight < minRemainingHeight) return null

      const segmentClassName = options.isContinuation ? removeIndentClass(className) : className
      let left = 1
      let right = text.length
      let fitCount = 0
      while (left <= right) {
        const mid = Math.floor((left + right) / 2)
        const tryStyle = buildSegmentStyle(style, options.isContinuation, mid < text.length)
        const tryHtml = buildParagraphHtml(tryStyle, text.slice(0, mid), segmentClassName)
        if (!overflows([...currentParts, tryHtml])) {
          fitCount = mid
          left = mid + 1
        } else {
          right = mid - 1
        }
      }

      if (fitCount <= 0) return null

      fitCount = adjustFitCountForReadableStart(text, fitCount)
      if (fitCount <= 0 || (fitCount < text.length && isPunctuationOnlyText(text.slice(0, fitCount)))) return null

      const hasMoreText = fitCount < text.length
      const fitStyle = buildSegmentStyle(style, options.isContinuation, hasMoreText)
      return {
        html: buildParagraphHtml(fitStyle, text.slice(0, fitCount), segmentClassName),
        remainingHtml: hasMoreText
          ? buildParagraphHtml(style, text.slice(fitCount), removeIndentClass(className))
          : '',
      }
    }

    const appendOversizedParagraph = (blockHtml: string, isContinuation = false) => {
      const parsed = parseParagraphHtml(blockHtml)
      if (!parsed || parsed.text.length <= 1) {
        pages.push(blockHtml)
        return
      }

      let pending = blockHtml
      let continuation = isContinuation
      while (pending) {
        const fitted = fitParagraphSegment(pending, { isContinuation: continuation })
        if (fitted) {
          currentParts = [...currentParts, fitted.html]
          pending = fitted.remainingHtml
          continuation = true
          if (pending) flushPage()
          continue
        }

        if (currentParts.length) {
          flushPage()
          continue
        }

        pages.push(pending)
        pending = ''
      }
    }

    const appendBlock = (blockHtml: string) => {
      const withBlock = [...currentParts, blockHtml]
      if (!overflows(withBlock)) {
        currentParts = withBlock
        return
      }

      if (currentParts.length) {
        const fitted = fitParagraphSegment(blockHtml, { isContinuation: false })
        if (fitted) {
          currentParts = [...currentParts, fitted.html]
          if (fitted.remainingHtml) {
            flushPage()
            appendOversizedParagraph(fitted.remainingHtml, true)
          }
          return
        }

        flushPage()
        if (!overflows([blockHtml])) {
          currentParts = [blockHtml]
          return
        }
      }

      appendOversizedParagraph(blockHtml)
    }

    for (const paragraph of paragraphs) {
      appendBlock(paragraph)
    }

    if (currentParts.length) {
      flushPage()
    }

    if (!pages.length) {
      pages.push(titleHtml)
    } else if (pages.length === 1 && !pages[0].includes('horizontal-flow-title')) {
      pages[0] = `${titleHtml}${pages[0]}`
    }

    const mergedPages = mergeOrphanPunctuationPages(pages)

    document.body.removeChild(measurer)
    horizontalPages.value = mergedPages
    horizontalPageIndex.value = Math.min(horizontalPageIndex.value, mergedPages.length - 1)

    container.scrollTo({ left: 0, behavior: 'auto' })
    updateHorizontalEndState()
  }

  function alignHorizontalToNearestPage(touchMoving: boolean) {
    const container = scrollContainerRef.value
    if (!container || !isHorizontalPageMode.value || touchMoving) return
    updateHorizontalMetrics()
    const maxPage = Math.max(0, horizontalPages.value.length - 1)
    horizontalPageIndex.value = Math.max(0, Math.min(maxPage, horizontalPageIndex.value))
    if (container.scrollLeft !== 0) {
      container.scrollTo({ left: 0, behavior: 'auto' })
    }
  }

  function resetHorizontalPagePosition() {
    horizontalPageIndex.value = 0
    const container = scrollContainerRef.value
    if (!container) return
    updateHorizontalMetrics()
    container.scrollTo({ left: 0, behavior: 'auto' })
  }

  return {
    horizontalPageIndex,
    horizontalPageStep,
    horizontalPageStepStyle,
    horizontalPages,
    isHorizontalAtEnd,
    rebuildHorizontalPages,
    updateHorizontalMetrics,
    updateHorizontalEndState,
    alignHorizontalToNearestPage,
    resetHorizontalPagePosition,
  }
}
