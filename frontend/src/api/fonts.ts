import { invokeEnvelope } from './invoke'

export interface CustomFontEntry {
  id: string
  name: string
  url: string
}

export function listCustomFonts() {
  return invokeEnvelope<CustomFontEntry[]>('list_custom_fonts', {})
}

export async function uploadCustomFont(file: File) {
  return invokeEnvelope<CustomFontEntry>('upload_custom_font', {
    fileName: file.name,
    file: new Uint8Array(await file.arrayBuffer()),
  })
}

export function deleteCustomFont(id: string) {
  return invokeEnvelope<null>('delete_custom_font', { id })
}
