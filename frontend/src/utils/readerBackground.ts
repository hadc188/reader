const SUPPORTED_IMAGE_TYPES = new Set([
  'image/jpeg',
  'image/png',
  'image/webp',
  'image/bmp',
])

/** 原图上限。落盘后不再受 localStorage 5MB 配额约束, 但仍要挡掉异常巨大的文件;
 *  与后端 `MAX_BACKGROUND_IMAGE_BYTES` 保持一致。 */
export const MAX_BACKGROUND_SOURCE_BYTES = 48 * 1024 * 1024

/**
 * WebView 的位图尺寸上限是 16384px，超过后整张图解码失败（背景直接不显示）。
 * 8192 足够覆盖 4K / 5K 屏的双倍图，又留出安全余量。
 */
export const MAX_BACKGROUND_DIMENSION = 8192

export const DEFAULT_READER_BACKGROUND_OPACITY = 0.35

/** 阅读页浮层底板默认实色: 面板不透明度是可选装饰, 默认不改变旧版观感。 */
export const DEFAULT_READER_PANEL_OPACITY = 1

/** 浮层底板不透明度下限: 再低会让抽屉里的文字失去可读底色。
 *  设置面板的滑块只跑这一段（50%-100%），拖到最左就是这一档。 */
export const READER_PANEL_OPACITY_MIN = 0.5

/** 阅读页背景不透明度下限。0 表示完全透明，只保留正文与控件。 */
export const READER_WINDOW_OPACITY_MIN = 0

/** 整窗不透明度默认值: 1 = 不透明，保持旧版观感。 */
export const DEFAULT_READER_WINDOW_OPACITY = 1

/** 正文文字不透明度默认值: 1 = 主题文字原色，保持旧版观感。 */
export const DEFAULT_READER_TEXT_OPACITY = 1

/** 透明度滑块的步长(1%)。旧版是 5%, 用户反馈太粗。 */
export const READER_BACKGROUND_OPACITY_STEP = 0.01

export function normalizeReaderBackgroundOpacity(value: unknown) {
  const numeric = typeof value === 'number' ? value : Number(value)
  if (!Number.isFinite(numeric)) return DEFAULT_READER_BACKGROUND_OPACITY
  return Math.max(0, Math.min(1, numeric))
}

/** 浮层底板不透明度归一化。
 *  非法输入回落默认实色; 有效值钳到 [READER_PANEL_OPACITY_MIN, 1] ——
 *  低于 0.5 时抽屉内的章节标题与徽标会透出正文, 已经读不清。 */
export function normalizeReaderPanelOpacity(value: unknown): number {
  const numeric = typeof value === 'number' ? value : Number(value)
  if (!Number.isFinite(numeric)) return DEFAULT_READER_PANEL_OPACITY
  return Math.max(READER_PANEL_OPACITY_MIN, Math.min(1, numeric))
}

/** 阅读页背景不透明度归一化。非法值回落 1(不透明)，有效值钳到 [0, 1]。 */
export function normalizeReaderWindowOpacity(value: unknown): number {
  const numeric = typeof value === 'number' ? value : Number(value)
  if (!Number.isFinite(numeric)) return DEFAULT_READER_WINDOW_OPACITY
  return Math.max(READER_WINDOW_OPACITY_MIN, Math.min(1, numeric))
}

/** 正文文字不透明度归一化。
 *  允许到 0：用户可能只想让背景图透出来，不显示文字（例如临时遮住正文）。
 *  非法输入回落 1，避免旧存档缺字段时文字突然变淡。 */
export function normalizeReaderTextOpacity(value: unknown): number {
  const numeric = typeof value === 'number' ? value : Number(value)
  if (!Number.isFinite(numeric)) return DEFAULT_READER_TEXT_OPACITY
  return Math.max(0, Math.min(1, numeric))
}

/** 生成阅读页标题栏/正文底板共用的半透明背景。 */
export function readerWindowBackground(color: string, opacity: unknown): string {
  const percent = Math.round(normalizeReaderWindowOpacity(opacity) * 100)
  return `color-mix(in srgb, ${color} ${percent}%, transparent)`
}

/** 存储值(0~1) → 界面上的百分比整数。 */
export function readerOpacityToPercent(opacity: unknown): number {
  return Math.round(normalizeReaderBackgroundOpacity(opacity) * 100)
}

/** 界面上的百分比 → 存储值(0~1, 保留两位小数)。
 *
 *  百分比输入框允许用户键入非法内容(空串、字母、"abc%"), 因此非数字一律返回
 *  null 由调用方保持原值, 而不是回落到默认透明度 —— 那会让用户只是误敲一个
 *  字符, 背景就突然变成 35%。 */
export function percentToReaderOpacity(percent: unknown): number | null {
  // 空串与纯空白必须先挡掉: Number('') === 0, 否则用户清空输入框时背景会
  // 突然变成全透明, 而不是保持原值等用户重新输入。
  if (typeof percent === 'string' && !percent.trim()) return null
  if (percent === null || percent === undefined) return null
  const numeric = typeof percent === 'number' ? percent : Number(String(percent).trim())
  if (!Number.isFinite(numeric)) return null
  const clamped = Math.max(0, Math.min(100, numeric))
  return Math.round(clamped) / 100
}

/** 背景图要落盘的像素尺寸。
 *
 *  背景图**不做有损压缩**: 以前这里会等比缩到 1600px 再按 0.58 质量编码成
 *  webp, 3840x2160 的图存进来只剩 1600x900, 铺到 4K 屏上自然发糊(issue #20
 *  第 1 条)。现在只在超过 WebView 位图上限时等比缩小(无损编码), 其余情况原样
 *  上传, 由 `background-size: cover` 在渲染期缩放。
 */
export function backgroundOutputSize(
  width: number,
  height: number,
  maxDimension = MAX_BACKGROUND_DIMENSION,
): { width: number; height: number } {
  const longest = Math.max(width, height)
  if (!Number.isFinite(longest) || longest <= 0) {
    throw new Error('无法读取该图片')
  }
  if (longest <= maxDimension) {
    return { width: Math.round(width), height: Math.round(height) }
  }
  const scale = maxDimension / longest
  return {
    width: Math.max(1, Math.round(width * scale)),
    height: Math.max(1, Math.round(height * scale)),
  }
}

export interface PreparedBackgroundImage {
  /** 要上传的字节。非 BMP、尺寸未超限时就是用户选的原文件。 */
  blob: Blob
  /** 上传时用于推导扩展名的文件名。 */
  fileName: string
}

/** 校验并准备要保存到磁盘的背景图。
 *
 *  必须保持**原始分辨率**: 存进 localStorage 的时代受 5MB 配额限制, 只能把图
 *  缩到 1600px 再压 webp; 现在字节直接落盘(`storage/assets/default/backgrounds`),
 *  只在两种情况下重新编码:
 *  - BMP: 浏览器能解码但体积巨大(BMP 无压缩), 无损转成 PNG;
 *  - 边长超过 `MAX_BACKGROUND_DIMENSION`: 等比缩小, 否则 WebView 解码失败。
 */
export async function prepareReaderBackgroundImage(file: File): Promise<PreparedBackgroundImage> {
  if (!SUPPORTED_IMAGE_TYPES.has(file.type)) {
    throw new Error('请选择 JPG、PNG、WebP 或 BMP 图片')
  }
  if (file.size > MAX_BACKGROUND_SOURCE_BYTES) {
    throw new Error('背景图片不能超过 48 MB')
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
    const target = backgroundOutputSize(bitmap.width, bitmap.height)
    const mustReencode = file.type === 'image/bmp'
      || target.width !== bitmap.width
      || target.height !== bitmap.height
    if (!mustReencode) {
      // 扩展名按 MIME 类型给, 不用原文件名: 有的 JPG 存成 `.jfif`/`.jpe`,
      // 后端只接受白名单扩展名, 直接沿用原名字会被拒。
      return {
        blob: file,
        fileName: backgroundUploadFileName(file.name, file.type),
      }
    }
    // PNG 是无损的: 重新编码不会引入压缩劣化, 只是把 BMP 的 24bit 原始像素
    // 换成压缩后的容器。缩小过的图同样走 PNG。
    const blob = await renderBackgroundImage(bitmap, target.width, target.height)
    return { blob, fileName: replaceExtension(file.name, 'png') }
  } finally {
    bitmap.close()
  }
}

function extensionForType(type: string) {
  if (type === 'image/png') return 'png'
  if (type === 'image/webp') return 'webp'
  return 'jpg'
}

/** 上传用的文件名: 原文件名的可读部分 + 后端白名单里的扩展名。 */
export function backgroundUploadFileName(name: string, type: string) {
  return replaceExtension(name, extensionForType(type))
}

function replaceExtension(name: string, extension: string) {
  const base = name.replace(/\.[^./\\]+$/, '') || 'background'
  return `${base}.${extension}`
}

function renderBackgroundImage(bitmap: ImageBitmap, width: number, height: number) {
  const canvas = document.createElement('canvas')
  canvas.width = width
  canvas.height = height
  const context = canvas.getContext('2d')
  if (!context) throw new Error('当前环境无法处理图片')
  context.drawImage(bitmap, 0, 0, width, height)
  return new Promise<Blob>((resolve, reject) => {
    canvas.toBlob((blob) => {
      if (blob) resolve(blob)
      else reject(new Error('图片编码失败，请更换图片后重试'))
    }, 'image/png')
  })
}
