import { invokeEnvelope } from './invoke'

/** 本地书内容文件(存储目录 data/default/local_books/<id>/ 下按相对路径打包)。 */
export interface LocalBookFileEntry {
  path: string
  base64: string
}

export interface LocalBookExportItem {
  id: string
  sizeBytes?: number
  files: LocalBookFileEntry[]
}

export interface CustomFontExportItem {
  fileName: string
  base64: string
}

export interface ReadingStatsExport {
  daily: Array<{ date: string; seconds: number; characters: number }>
  byBook: Array<{
    date: string
    bookUrl: string
    bookName: string
    bookAuthor: string
    seconds: number
    characters: number
  }>
}

export interface LocalBooksExport {
  books: LocalBookExportItem[]
  skipped: Array<{ id: string; sizeBytes: number }>
  totalBytes: number
}

export interface CustomFontsExport {
  fonts: CustomFontExportItem[]
  skipped: string[]
  totalBytes: number
}

export function exportLocalBooks() {
  return invokeEnvelope<LocalBooksExport>('export_local_books')
}

export function importLocalBooks(books: LocalBookExportItem[]) {
  return invokeEnvelope<{ imported: number }>('import_local_books', { req: { books } })
}

export function exportCustomFonts() {
  return invokeEnvelope<CustomFontsExport>('export_custom_fonts')
}

export function importCustomFonts(fonts: CustomFontExportItem[]) {
  return invokeEnvelope<{ imported: number }>('import_custom_fonts', { req: { fonts } })
}

export function exportReadingStats() {
  return invokeEnvelope<ReadingStatsExport>('export_reading_stats')
}

export function importReadingStats(stats: ReadingStatsExport) {
  return invokeEnvelope<{ daily: number; byBook: number }>('import_reading_stats', {
    req: { daily: stats.daily, byBook: stats.byBook },
  })
}
