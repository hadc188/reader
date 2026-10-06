import { invokeEnvelope } from './invoke'

/** 书籍自定义封面。
 *
 *  图片真实文件由后端写入应用数据目录(`storage/assets/default/covers/`),
 *  书架里只保存返回的 URL —— 与自定义字体同一思路, 避免把 base64 塞进书架 JSON。 */

/** 与后端 `MAX_BOOK_COVER_BYTES` 保持一致, 用于上传前的本地预检。 */
export const MAX_BOOK_COVER_BYTES = 8 * 1024 * 1024

export const BOOK_COVER_EXTENSIONS = ['png', 'jpg', 'jpeg', 'gif', 'webp', 'avif'] as const

/** 上传封面, 返回可直接存进 `Book.customCoverUrl` 的 URL。 */
export async function uploadBookCover(file: File) {
  return invokeEnvelope<string>('upload_book_cover', {
    fileName: file.name,
    file: new Uint8Array(await file.arrayBuffer()),
  })
}

/** 删除本应用存下的封面文件。
 *  传入书源自带的远程地址是安全的 —— 后端只认自己生成的 URL, 其余直接返回成功。 */
export function deleteBookCover(url: string) {
  return invokeEnvelope<null>('delete_book_cover', { url })
}
