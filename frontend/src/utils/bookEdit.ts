import { BOOK_COVER_EXTENSIONS, MAX_BOOK_COVER_BYTES } from '../api/bookCover'
import { readerOrigin } from '../api/scheme'
import { searchMergeKey } from './searchRank'

/** 书籍信息编辑的纯逻辑。
 *
 *  表单校验、封面来源判断、旧封面清理判定都放在这里, 不碰 DOM 也不发请求,
 *  便于单测 —— `.vue` 内部在当前的 node 测试环境里测不到。 */

export interface BookEditDraft {
  name: string
  author: string
  intro: string
  /** 自定义封面 URL; '' 表示用书源自带的 coverUrl。 */
  customCoverUrl: string
}

export interface BookEditSource {
  name: string
  /** `author`/`intro` 在后端也是 `Option<String>` 且无 skip_serializing_if,
   *  未设置时经 JSON 往返是 `null`(而非 `undefined`) —— 两者都要允许。 */
  author?: string | null
  intro?: string | null
  customCoverUrl?: string | null
  /** 首次自定义之前的原始值(见 model/book.rs)。
   *
   *  注意后端是 Rust `Option<String>`, 且这三个字段**没有** `skip_serializing_if`,
   *  所以「从未记录」经 JSON 往返后是 `null`, 不是 `undefined`。判空必须两者都认。 */
  originalName?: string | null
  originalAuthor?: string | null
  originalIntro?: string | null
}

/** 原始值是否「从未记录过」。
 *
 *  后端 Rust `Option<String>` 序列化 None → JSON `null`, 序列化后 key 是存在的,
 *  值为 `null`; 前端自建对象则可能是 `undefined`。两者都表示从未记录。
 *  `''` 不是未记录 —— 它代表「记录过, 且原始值为空」。 */
function isOriginUnrecorded(value: string | null | undefined): boolean {
  return value === undefined || value === null
}

/** 「还原」的目标: 首次自定义之前的样子。
 *
 *  三处来源按优先级取值:
 *  1. `original*` —— 曾自定义过, 已记下原始值;
 *  2. 没有 `original*` 时退回传入的 current —— 从未自定义过, 当前即原始;
 *  3. 封面用书源自带的 `coverUrl`(清空 customCoverUrl 即回落)。
 *
 *  注意封面: 若原始封面本身来自 `coverUrl`, 还原就是清空自定义封面, 因此这里
 *  一律返回 `customCoverUrl: ''`。 */
export function resolveRevertDraft(
  book: BookEditSource & { coverUrl?: string },
  current: BookEditDraft,
): BookEditDraft {
  return {
    name: isOriginUnrecorded(book.originalName) ? current.name : book.originalName!,
    author: isOriginUnrecorded(book.originalAuthor) ? current.author : book.originalAuthor!,
    intro: isOriginUnrecorded(book.originalIntro) ? current.intro : book.originalIntro!,
    customCoverUrl: '',
  }
}

/** 是否存在可还原的原始状态。
 *
 *  只有「曾经自定义过」(记下了 original*) 或「当前有自定义封面」时才有东西可回退;
 *  否则点还原等于什么都没做, 按钮应当保持不可用, 免得用户以为功能坏了。
 *
 *  判据同样用 `!== undefined`: 原始值为空字符串也是「记录过的原始状态」
 *  (例如书源本来就没给简介), 把它当成没有可还原内容会导致按钮误禁用。 */
export function canRevertToOrigin(book: BookEditSource): boolean {
  return !isOriginUnrecorded(book.originalName)
    || !isOriginUnrecorded(book.originalAuthor)
    || !isOriginUnrecorded(book.originalIntro)
    || Boolean(book.customCoverUrl)
}

/** 首次保存自定义内容时要补记的原始值。
 *
 *  只在字段**从未记录**时才写入, 之后永不覆盖 —— 这样用户改过多少轮, 都还能一键
 *  回到最初。返回的对象可直接合并进 `saveBook` 的参数。
 *
 *  注意判据必须是 `undefined` 而不是 falsy: 原始值本来就可能为空字符串
 *  (书源没给简介/作者)。用 `!existing.originalIntro` 会把「已记录的原始空值」
 *  当成「尚未记录」, 于是每次保存都用上一次的值重写原始值 —— 结果是「原始为空 →
 *  填入内容保存」之后, 原始空值永久丢失, 还原只能退回上一次保存的内容。 */
export function originPatchForSave(
  existing: BookEditSource,
  beforeEdit: BookEditDraft,
): Pick<BookEditSource, 'originalName' | 'originalAuthor' | 'originalIntro'> {
  const patch: Pick<BookEditSource, 'originalName' | 'originalAuthor' | 'originalIntro'> = {}
  if (isOriginUnrecorded(existing.originalName)) patch.originalName = beforeEdit.name
  if (isOriginUnrecorded(existing.originalAuthor)) patch.originalAuthor = beforeEdit.author
  if (isOriginUnrecorded(existing.originalIntro)) patch.originalIntro = beforeEdit.intro
  return patch
}

export function createBookEditDraft(book: BookEditSource): BookEditDraft {
  return {
    name: book.name || '',
    author: book.author || '',
    intro: book.intro || '',
    customCoverUrl: book.customCoverUrl || '',
  }
}

/** 校验表单。返回错误信息, 通过时返回 ''。 */
export function validateBookEditDraft(draft: BookEditDraft): string {
  if (!draft.name.trim()) return '书名不能为空'
  // 与书架既有约定一致: 书名/作者超长会让卡片与详情页排版崩掉。
  if (draft.name.trim().length > 120) return '书名不能超过 120 个字符'
  if (draft.author.trim().length > 120) return '作者不能超过 120 个字符'
  if (draft.intro.length > 5000) return '简介不能超过 5000 个字符'
  return ''
}

/** 保存前把草稿收敛成要写进书架的字段(去掉首尾空白)。 */
export function normalizeBookEditDraft(draft: BookEditDraft): BookEditDraft {
  return {
    name: draft.name.trim(),
    author: draft.author.trim(),
    intro: draft.intro.trim(),
    customCoverUrl: draft.customCoverUrl.trim(),
  }
}

/** 提交给后端的可编辑字段, 可能省略某些键。
 *
 *  后端的 `intro`/`author` 是 `Option<String>` 且没有 `skip_serializing_if`,
 *  而 `createBookEditDraft` 会把「原本没有值」折成 `''`。若照原样回传, 保存一次
 *  就会把 `None` 写成 `Some("")` —— 于是 `merge_book` 里
 *  `if target.intro.is_none()` 这条「书源有简介就回填」的判据永远不再成立,
 *  换源后新书源的简介再也填不进来。
 *
 *  所以: 原本就没有值(undefined/null)且用户也没填写时, 干脆**不提交这个键**,
 *  让后端保持 `None`。有值、或用户新填了内容, 才提交。
 *  `name` 必填(校验已保证非空), `customCoverUrl` 用 '' 表示回落到书源封面。 */
export function editableFieldsForSave(
  existing: BookEditSource,
  next: BookEditDraft,
): Partial<BookEditDraft> {
  const fields: Partial<BookEditDraft> = {
    name: next.name,
    customCoverUrl: next.customCoverUrl,
  }
  if (!isOriginUnrecorded(existing.author) || next.author) fields.author = next.author
  if (!isOriginUnrecorded(existing.intro) || next.intro) fields.intro = next.intro
  return fields
}

/** 书架判重: 这本书是否已在书架里。
 *
 *  发现页/搜索页的「加入书架」只提交 5 个字段(name/author/bookUrl/origin/coverUrl),
 *  而后端对同一本书是**整条覆盖** —— 一旦点在书架已有的书上, 用户自定义的封面、
 *  简介等就会被清空。两个入口(卡片按钮 + 右键菜单)都必须先用它挡一道。
 *
 *  先比 bookUrl, 再退回「书名+作者」归一化键, 以便跨书源识别同一本书。 */
export function isBookOnShelf(
  book: { bookUrl: string; name?: string; author?: string },
  shelf: { bookUrl: string; name?: string; author?: string }[],
): boolean {
  if (!book.bookUrl) return false
  if (shelf.some((item) => item.bookUrl === book.bookUrl)) return true
  const identity = searchMergeKey({ name: book.name || '', author: book.author || '' })
  return shelf.some((item) => (
    searchMergeKey({ name: item.name || '', author: item.author || '' }) === identity
  ))
}

/** 草稿相对原书是否有改动。
 *
 *  当前 UI 未直接调用它 —— 编辑页的「还原」已改为回退到原始值(见
 *  `resolveRevertDraft`), 不再依赖「是否改过」。保留此函数是因为它是
 *  「提交前判断能否跳过写入」的通用判据, 且已被单测覆盖; 若将来给保存按钮加
 *  「无改动则禁用」或关窗前的未保存提醒, 直接用这里即可。 */
export function isBookEditDirty(book: BookEditSource, draft: BookEditDraft): boolean {
  const before = normalizeBookEditDraft(createBookEditDraft(book))
  const after = normalizeBookEditDraft(draft)
  return before.name !== after.name
    || before.author !== after.author
    || before.intro !== after.intro
    || before.customCoverUrl !== after.customCoverUrl
}

/** 上传前的本地预检, 返回错误信息(通过时 ''）。
 *  后端也会校验, 这里只是为了让用户立刻看到原因而不是等一次失败的往返。 */
export function validateCoverFile(file: { name: string; size: number }): string {
  const extension = file.name.split('.').pop()?.toLowerCase() || ''
  if (!(BOOK_COVER_EXTENSIONS as readonly string[]).includes(extension)) {
    return '仅支持 PNG、JPG、GIF、WebP 和 AVIF 图片'
  }
  if (file.size <= 0) return '图片内容为空'
  if (file.size > MAX_BOOK_COVER_BYTES) return '封面图片不能超过 8 MB'
  return ''
}

/** 该 URL 是否是本应用存下的封面(而非书源自带的远程图)。
 *
 *  与后端 `book_cover_file_name_from_url` 的前缀校验保持一致: 必须是自己协议
 *  origin 下的 covers 路径。只判子串会放过「含该片段的远程地址」——后端虽会
 *  拒绝, 但前端先判掉可以少一次无谓的 IPC。
 *
 *  注意 origin 是平台相关的(http://reader.localhost / reader://localhost),
 *  这里按 scheme.ts 探测出的实际值判定, 不写死。 */
export function isOwnBookCoverUrl(url: string | undefined | null): boolean {
  if (!url) return false
  return url.startsWith(`${readerOrigin}/files?path=default/covers/`)
}

/** 保存后应当被删除的旧封面文件。
 *
 *  三条都不满足时返回 '':
 *  - 旧封面是本应用存的(远程图没有本地文件);
 *  - 新封面与旧封面不同(没换图就不该动文件);
 *  - 新封面不是旧的同一份(避免重复上传覆盖后又把自己删掉)。 */
export function obsoleteCoverUrl(
  previousCoverUrl: string | undefined | null,
  nextCoverUrl: string,
): string {
  const previous = (previousCoverUrl || '').trim()
  if (!previous) return ''
  if (previous === nextCoverUrl.trim()) return ''
  if (!isOwnBookCoverUrl(previous)) return ''
  return previous
}
