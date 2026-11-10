import { readerOrigin } from '../api/scheme'
import type { Book } from '../types'
import { isLocalPdfBook } from './localBook'

/**
 * 本地 PDF 的页面渲染。
 *
 * 后端 (local_pdf_book) 只负责把导入的文件原样落盘并给出「页区间」目录,
 * 章节正文是每页一个占位 `<div data-pdf-page="N">`; 真正的页面内容由
 * pdf.js 在这里绘制 —— 与 Legado 的 `PdfFile` 分工一致 (它在 Android 上
 * 用 PdfRenderer 渲染, 项目里没有原生渲染器, 因此改用 WebView 自带的
 * pdf.js), 扫描件与复杂版式都能原样显示。
 */

/** 占位元素上的页号属性 */
export const PDF_PAGE_ATTR = 'data-pdf-page'

/** 渲染宽度上限 (CSS px): 超过这个宽度只会让整页滚动更费劲 */
const MAX_RENDER_WIDTH = 1400

type PdfModule = typeof import('pdfjs-dist')

let pdfModulePromise: Promise<PdfModule> | null = null

/**
 * 载入 pdf.js。worker 与 cmap/字体资源都随包分发 (vite 的 `?url`), 但 worker
 * 需要显式指定: 默认的相对地址在 `tauri.localhost` 下会解析到应用根目录。
 */
async function loadPdfjs(): Promise<PdfModule> {
  if (!pdfModulePromise) {
    pdfModulePromise = (async () => {
      const [pdfjs, workerUrl] = await Promise.all([
        import('pdfjs-dist'),
        import('pdfjs-dist/build/pdf.worker.min.mjs?url').then((m) => m.default as string),
      ])
      pdfjs.GlobalWorkerOptions.workerSrc = workerUrl
      return pdfjs
    })().catch((error) => {
      pdfModulePromise = null
      throw error
    })
  }
  return pdfModulePromise
}

/** 本地 PDF 原文地址: 走 `reader` 自定义协议, 后端校验 hash 后再读盘 */
export function localPdfFileUrl(book: Pick<Book, 'bookUrl'>) {
  return `${readerOrigin}/pdf?bookUrl=${encodeURIComponent(book.bookUrl.trim())}`
}

export function isPdfBook(book?: Pick<Book, 'origin' | 'bookUrl'> | null) {
  return isLocalPdfBook(book)
}

interface LoadedPdf {
  document: import('pdfjs-dist').PDFDocumentProxy
  loadingTask: import('pdfjs-dist').PDFDocumentLoadingTask
}

/**
 * 已解析的 PDF, 以 bookUrl 为键。
 *
 * 整本文档要留在内存里跨章复用 (每章都重新解析几十兆的 PDF 代价太大), 因此
 * 换书或删除本地书时必须显式调用 releasePdfBook 释放。
 */
const loadedPdfs = new Map<string, Promise<LoadedPdf>>()

/** 正在销毁中的任务: 释放后旧渲染不得再拿它画页 */
const destroyed = new WeakSet<import('pdfjs-dist').PDFDocumentLoadingTask>()

/** 取整本 PDF 的字节并解析成 document。同一本书只解析一次。 */
async function loadDocument(book: Book): Promise<LoadedPdf> {
  const key = book.bookUrl
  const cached = loadedPdfs.get(key)
  if (cached) {
    const loaded = await cached
    // 上一次释放刚好发生在本次取用之间: 重新解析, 而不是用已销毁的文档
    if (destroyed.has(loaded.loadingTask)) {
      await cached.catch(() => undefined)
      loadedPdfs.delete(key)
      return loadDocument(book)
    }
    return loaded
  }

  const task = (async () => {
    const response = await fetch(localPdfFileUrl(book))
    if (!response.ok) throw new Error('本地 PDF 文件读取失败')
    const bytes = new Uint8Array(await response.arrayBuffer())
    const pdfjs = await loadPdfjs()
    const loadingTask = pdfjs.getDocument({ data: bytes })
    const document = await loadingTask.promise
    return { document, loadingTask }
  })()

  loadedPdfs.set(key, task)
  task.catch(() => {
    if (loadedPdfs.get(key) === task) loadedPdfs.delete(key)
  })
  return task
}

/**
 * 把章节正文里的占位元素逐页绘制到 canvas。
 *
 * 返回一个释放函数: 换章时必须调用, 否则上一章的 PDF 文档与 canvas 会一直
 * 占着内存 (大书整本几十兆字节)。
 */
export async function renderPdfChapterPages(
  container: HTMLElement,
  book: Book,
  isCancelled: () => boolean = () => false,
): Promise<() => void> {
  const placeholders = Array.from(
    container.querySelectorAll<HTMLElement>(`[${PDF_PAGE_ATTR}]`),
  )
  if (!placeholders.length) return () => {}

  const { document: pdfDocument } = await loadDocument(book)
  let disposed = false
  /** 已画的 canvas: 取消时连同位图一起释放, 只置标志位会把显存留到 GC */
  const canvases: HTMLCanvasElement[] = []
  const dispose = () => {
    if (disposed) return
    disposed = true
    for (const canvas of canvases) {
      // 把宽高归零是让浏览器立刻回收 canvas 位图的标准做法
      canvas.width = 0
      canvas.height = 0
    }
    canvases.length = 0
  }

  for (const placeholder of placeholders) {
    if (disposed || isCancelled()) {
      dispose()
      return dispose
    }
    const pageNumber = Number(placeholder.getAttribute(PDF_PAGE_ATTR)) + 1
    if (
      !Number.isFinite(pageNumber) ||
      pageNumber < 1 ||
      pageNumber > pdfDocument.numPages
    ) {
      placeholder.classList.add('pdf-page-missing')
      continue
    }
    const canvas = document.createElement('canvas')
    canvas.className = 'pdf-page-canvas'
    canvases.push(canvas)
    placeholder.replaceChildren(canvas)
    try {
      await drawPage(pdfDocument, pageNumber, canvas)
      placeholder.classList.add('pdf-page-ready')
    } catch {
      // 单页失败 (损坏的页面对象) 只留空白, 不牵连整章
      placeholder.classList.add('pdf-page-missing')
    }
  }
  return dispose
}

async function drawPage(
  pdfDocument: import('pdfjs-dist').PDFDocumentProxy,
  pageNumber: number,
  canvas: HTMLCanvasElement,
) {
  const page = await pdfDocument.getPage(pageNumber)
  const baseViewport = page.getViewport({ scale: 1 })
  const available = Math.max(
    320,
    canvas.parentElement?.clientWidth || MAX_RENDER_WIDTH,
  )
  const cssWidth = Math.min(available, MAX_RENDER_WIDTH)
  const ratio = Math.min(window.devicePixelRatio || 1, 2)
  const viewport = page.getViewport({ scale: (cssWidth / baseViewport.width) * ratio })
  const context = canvas.getContext('2d')
  if (!context) throw new Error('canvas 不可用')
  canvas.width = Math.floor(viewport.width)
  canvas.height = Math.floor(viewport.height)
  canvas.style.width = `${Math.floor(viewport.width / ratio)}px`
  canvas.style.height = `${Math.floor(viewport.height / ratio)}px`
  await page.render({ canvasContext: context, viewport, canvas }).promise
  page.cleanup()
}

/** 释放一本书的 PDF 文档 (换书/离开阅读器时调用) */
export async function releasePdfBook(bookUrl: string) {
  const cached = loadedPdfs.get(bookUrl)
  if (!cached) return
  loadedPdfs.delete(bookUrl)
  await cached
    .then((loaded) => {
      // 先标记再销毁: 正在渲染的章节下一轮取用会重新解析, 而不是用死文档
      destroyed.add(loaded.loadingTask)
      return loaded.loadingTask.destroy()
    })
    .catch(() => undefined)
}
