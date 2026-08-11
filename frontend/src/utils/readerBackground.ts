const SUPPORTED_IMAGE_TYPES = new Set([
  'image/jpeg',
  'image/png',
  'image/webp',
  'image/bmp',
])

const MAX_SOURCE_BYTES = 20 * 1024 * 1024
const MAX_STORED_LENGTH = 2_000_000

export const DEFAULT_READER_BACKGROUND_OPACITY = 0.35

export function normalizeReaderBackgroundOpacity(value: unknown) {
  const numeric = typeof value === 'number' ? value : Number(value)
  if (!Number.isFinite(numeric)) return DEFAULT_READER_BACKGROUND_OPACITY
  return Math.max(0, Math.min(1, numeric))
}

export async function prepareReaderBackgroundImage(file: File) {
  if (!SUPPORTED_IMAGE_TYPES.has(file.type)) {
    throw new Error('请选择 JPG、PNG、WebP 或 BMP 图片')
  }
  if (file.size > MAX_SOURCE_BYTES) {
    throw new Error('背景图片不能超过 20 MB')
  }

  let bitmap: ImageBitmap
  try {
    bitmap = await createImageBitmap(file)
  } catch {
    throw new Error('无法读取该图片，请更换图片后重试')
  }
  try {
    if (!bitmap.width || !bitmap.height) {
      throw new Error('无法读取该图片')
    }

    const attempts = [
      { maxDimension: 2400, quality: 0.86 },
      { maxDimension: 1920, quality: 0.72 },
      { maxDimension: 1600, quality: 0.58 },
    ]
    for (const attempt of attempts) {
      const dataUrl = renderBackgroundImage(bitmap, attempt.maxDimension, attempt.quality)
      if (dataUrl.length <= MAX_STORED_LENGTH) return dataUrl
    }
  } finally {
    bitmap.close()
  }

  throw new Error('图片内容过大，请选择更简单或尺寸更小的图片')
}

function renderBackgroundImage(bitmap: ImageBitmap, maxDimension: number, quality: number) {
  const scale = Math.min(1, maxDimension / Math.max(bitmap.width, bitmap.height))
  const width = Math.max(1, Math.round(bitmap.width * scale))
  const height = Math.max(1, Math.round(bitmap.height * scale))
  const canvas = document.createElement('canvas')
  canvas.width = width
  canvas.height = height
  const context = canvas.getContext('2d')
  if (!context) throw new Error('当前环境无法处理图片')
  context.drawImage(bitmap, 0, 0, width, height)
  return canvas.toDataURL('image/webp', quality)
}
