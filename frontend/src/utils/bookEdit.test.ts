import { describe, expect, it, vi } from 'vitest'
import {
  canRevertToOrigin,
  createBookEditDraft,
  editableFieldsForSave,
  isBookEditDirty,
  isBookOnShelf,
  isOwnBookCoverUrl,
  normalizeBookEditDraft,
  obsoleteCoverUrl,
  originPatchForSave,
  resolveRevertDraft,
  validateBookEditDraft,
  validateCoverFile,
} from './bookEdit'

// bookEdit 依赖 api/scheme(它会 import Tauri 的 convertFileSrc), 单测里拿不到
// 运行时; 用真实的探测结果——单测按 Windows 形态回退。
vi.mock('../api/invoke', () => ({
  get: vi.fn(),
  post: vi.fn(),
  invokeEnvelope: vi.fn(),
  invokeRaw: vi.fn(),
}))

const { readerOrigin } = await import('../api/scheme')

const ownCover = (name = 'deadbeef.png') => `${readerOrigin}/files?path=default/covers/${name}`
const remoteCover = 'https://example.com/cover.jpg'

describe('createBookEditDraft', () => {
  it('seeds the form from the book, filling missing fields with empty strings', () => {
    expect(createBookEditDraft({ name: '书名' })).toEqual({
      name: '书名', author: '', intro: '', customCoverUrl: '',
    })
  })

  it('carries over an existing custom cover and intro', () => {
    const draft = createBookEditDraft({
      name: '书名', author: '作者', intro: '简介', customCoverUrl: ownCover(),
    })

    expect(draft.author).toBe('作者')
    expect(draft.intro).toBe('简介')
    expect(draft.customCoverUrl).toBe(ownCover())
  })
})

describe('validateBookEditDraft', () => {
  const base = { name: '书名', author: '', intro: '', customCoverUrl: '' }

  it('requires a non-empty book name', () => {
    expect(validateBookEditDraft({ ...base, name: '' })).toBe('书名不能为空')
    expect(validateBookEditDraft({ ...base, name: '   ' })).toBe('书名不能为空')
    expect(validateBookEditDraft(base)).toBe('')
  })

  it('counts the trimmed name, so padding whitespace cannot trip the limit', () => {
    expect(validateBookEditDraft({ ...base, name: `  ${'字'.repeat(120)}  ` })).toBe('')
    expect(validateBookEditDraft({ ...base, name: '字'.repeat(121) })).toBe('书名不能超过 120 个字符')
  })

  it('bounds the author and the intro', () => {
    expect(validateBookEditDraft({ ...base, author: '字'.repeat(121) })).toBe('作者不能超过 120 个字符')
    expect(validateBookEditDraft({ ...base, intro: '字'.repeat(5001) })).toBe('简介不能超过 5000 个字符')
    expect(validateBookEditDraft({ ...base, author: '字'.repeat(120) })).toBe('')
    expect(validateBookEditDraft({ ...base, intro: '字'.repeat(5000) })).toBe('')
  })

  it('allows clearing author and intro, which are optional', () => {
    expect(validateBookEditDraft({ name: '书名', author: '', intro: '', customCoverUrl: '' })).toBe('')
  })
})

describe('normalizeBookEditDraft', () => {
  it('trims every text field', () => {
    const draft = normalizeBookEditDraft({
      name: '  书名  ', author: '  作者 ', intro: '  简介  ', customCoverUrl: `  ${ownCover()}  `,
    })

    expect(draft.name).toBe('书名')
    expect(draft.author).toBe('作者')
    expect(draft.intro).toBe('简介')
    expect(draft.customCoverUrl).toBe(ownCover())
  })
})

describe('isBookEditDirty', () => {
  const book = { name: '原书名', author: '原作者', intro: '原简介', customCoverUrl: '' }

  it('is clean for an untouched form', () => {
    expect(isBookEditDirty(book, createBookEditDraft(book))).toBe(false)
  })

  it('is clean when only surrounding whitespace was added', () => {
    expect(isBookEditDirty(book, { ...createBookEditDraft(book), name: '  原书名  ' })).toBe(false)
  })

  it('detects a change in each editable field', () => {
    const base = createBookEditDraft(book)
    expect(isBookEditDirty(book, { ...base, name: '新书名' })).toBe(true)
    expect(isBookEditDirty(book, { ...base, author: '新作者' })).toBe(true)
    expect(isBookEditDirty(book, { ...base, intro: '新简介' })).toBe(true)
    expect(isBookEditDirty(book, { ...base, customCoverUrl: ownCover() })).toBe(true)
  })

  it('treats clearing a field as a change', () => {
    const base = createBookEditDraft(book)
    expect(isBookEditDirty(book, { ...base, author: '' })).toBe(true)
  })
})

describe('validateCoverFile', () => {
  it('accepts every supported image type', () => {
    ['png', 'jpg', 'jpeg', 'gif', 'webp', 'avif'].forEach((extension) => {
      expect(validateCoverFile({ name: `cover.${extension}`, size: 1024 })).toBe('')
    })
  })

  it('is case-insensitive about the extension', () => {
    expect(validateCoverFile({ name: 'COVER.PNG', size: 1024 })).toBe('')
  })

  it('rejects unsupported and missing extensions', () => {
    expect(validateCoverFile({ name: 'cover.svg', size: 10 })).toBe('仅支持 PNG、JPG、GIF、WebP 和 AVIF 图片')
    expect(validateCoverFile({ name: 'cover.exe', size: 10 })).toBe('仅支持 PNG、JPG、GIF、WebP 和 AVIF 图片')
    expect(validateCoverFile({ name: 'cover', size: 10 })).toBe('仅支持 PNG、JPG、GIF、WebP 和 AVIF 图片')
  })

  it('rejects an empty file', () => {
    expect(validateCoverFile({ name: 'cover.png', size: 0 })).toBe('图片内容为空')
  })

  it('enforces the 8 MB ceiling at the boundary', () => {
    const limit = 8 * 1024 * 1024
    expect(validateCoverFile({ name: 'c.png', size: limit })).toBe('')
    expect(validateCoverFile({ name: 'c.png', size: limit + 1 })).toBe('封面图片不能超过 8 MB')
  })
})

describe('isOwnBookCoverUrl', () => {
  it('recognises covers this app stored', () => {
    expect(isOwnBookCoverUrl(ownCover())).toBe(true)
    expect(isOwnBookCoverUrl(ownCover('a1b2c3.jpeg'))).toBe(true)
  })

  it('rejects remote source covers', () => {
    expect(isOwnBookCoverUrl(remoteCover)).toBe(false)
  })

  it('rejects empty, missing and look-alike urls', () => {
    expect(isOwnBookCoverUrl('')).toBe(false)
    expect(isOwnBookCoverUrl(undefined)).toBe(false)
    expect(isOwnBookCoverUrl(null)).toBe(false)
    // 字体目录不是封面目录
    expect(isOwnBookCoverUrl('http://reader.localhost/files?path=default/fonts/x.png')).toBe(false)
  })
})

describe('obsoleteCoverUrl', () => {
  it('returns the old file when the cover was actually replaced', () => {
    expect(obsoleteCoverUrl(ownCover('old.png'), ownCover('new.png'))).toBe(ownCover('old.png'))
  })

  it('returns nothing when the cover did not change', () => {
    expect(obsoleteCoverUrl(ownCover(), ownCover())).toBe('')
  })

  it('returns nothing when reverting to the original source cover', () => {
    // 清空自定义封面(恢复书源封面)时, 旧的本应用封面确实成了孤儿, 应被删除。
    expect(obsoleteCoverUrl(ownCover(), '')).toBe(ownCover())
  })

  it('never targets a remote source cover', () => {
    // 书源封面不是本应用存的文件, 交给后端只会是空操作。
    expect(obsoleteCoverUrl(remoteCover, ownCover())).toBe('')
    expect(obsoleteCoverUrl(remoteCover, '')).toBe('')
  })

  it('handles a missing previous cover', () => {
    expect(obsoleteCoverUrl(undefined, ownCover())).toBe('')
    expect(obsoleteCoverUrl('', ownCover())).toBe('')
    expect(obsoleteCoverUrl('   ', ownCover())).toBe('')
  })

  it('ignores surrounding whitespace when comparing', () => {
    expect(obsoleteCoverUrl(`  ${ownCover()}  `, ownCover())).toBe('')
  })

  it('is origin-strict, matching the backend prefix check', () => {
    // 含相同路径片段的远程地址不算本应用封面 —— 后端也会拒绝, 前端先判掉。
    expect(isOwnBookCoverUrl(`https://evil.com/files?path=default/covers/deadbeef.png`)).toBe(false)
    expect(isOwnBookCoverUrl(`http://reader.localhost.evil.com/files?path=default/covers/x.png`)).toBe(false)
    expect(isOwnBookCoverUrl(ownCover())).toBe(true)
  })
})

describe('uploaded-but-unused cover cleanup', () => {
  /** 对应 BookDetailModal 的 pendingCoverUploads: 本次会话上传过、但最终没被
   *  采用的封面要在保存时删掉, 否则换了两次图或又点回原始封面都会留下孤儿。 */
  const unusedUploads = (pending: string[], finalUrl: string) =>
    pending.filter((url) => url !== finalUrl)

  it('deletes the earlier upload when the user uploaded twice', () => {
    const first = ownCover('aaaa.png')
    const second = ownCover('bbbb.png')

    expect(unusedUploads([first, second], second)).toEqual([first])
  })

  it('deletes every upload when the user reverted to the original cover', () => {
    const first = ownCover('aaaa.png')
    const second = ownCover('bbbb.png')

    expect(unusedUploads([first, second], '')).toEqual([first, second])
  })

  it('keeps the adopted cover untouched', () => {
    const adopted = ownCover('aaaa.png')

    expect(unusedUploads([adopted], adopted)).toEqual([])
  })

  it('has nothing to clean when the user never uploaded', () => {
    expect(unusedUploads([], ownCover('x.png'))).toEqual([])
  })
})

describe('resolveRevertDraft (还原到原始状态)', () => {
  const current = {
    name: '改过的书名', author: '改过的作者', intro: '改过的简介', customCoverUrl: ownCover(),
  }

  it('restores the recorded origin when the book was customised before', () => {
    // 这正是用户要的: 保存过自定义内容后, 仍能回到最初的样子。
    const reverted = resolveRevertDraft({
      name: '改过的书名',
      originalName: '剑来',
      originalAuthor: '烽火戏诸侯',
      originalIntro: '大千世界，无奇不有。',
    }, current)

    expect(reverted).toEqual({
      name: '剑来',
      author: '烽火戏诸侯',
      intro: '大千世界，无奇不有。',
      customCoverUrl: '',
    })
  })

  it('falls back to the current values when nothing was ever customised', () => {
    const reverted = resolveRevertDraft({ name: '剑来' }, current)

    expect(reverted.name).toBe('改过的书名')
    expect(reverted.author).toBe('改过的作者')
    expect(reverted.intro).toBe('改过的简介')
  })

  it('always clears the custom cover so the source cover comes back', () => {
    expect(resolveRevertDraft({ name: 'x' }, current).customCoverUrl).toBe('')
    expect(resolveRevertDraft({ name: 'x', originalName: 'y' }, current).customCoverUrl).toBe('')
  })

  it('keeps an empty origin meaningful instead of falling through', () => {
    // 原始简介本来就是空: 记成 '' 后还原必须回到空, 而不是保留改过的内容。
    const reverted = resolveRevertDraft({
      name: '改过的书名', originalName: '剑来', originalIntro: '',
    }, current)

    expect(reverted.intro).toBe('')
    expect(reverted.name).toBe('剑来')
  })

  it('restores fields independently', () => {
    const reverted = resolveRevertDraft({
      name: '改过的书名', originalAuthor: '原作者',
    }, current)

    expect(reverted.name).toBe('改过的书名')
    expect(reverted.author).toBe('原作者')
  })
})

describe('origin fields arriving as JSON null (Rust Option<String> → null)', () => {
  // 回归测试: 后端是 Rust `Option<String>` 且这三个字段没有 skip_serializing_if,
  // 因此「从未记录」经 JSON 往返后是 **null** 而不是 undefined。
  // 早先只判 `=== undefined`, 于是 null 被当成「已记录」→ 首次保存根本不记原始值,
  // 表现为「保存后就无法还原」。
  const fromBackend = {
    name: '剑来', author: '烽火戏诸侯', intro: '原文',
    originalName: null, originalAuthor: null, originalIntro: null,
  }

  it('records the origin on first save when the fields come back as null', () => {
    const patch = originPatchForSave(fromBackend, {
      name: '剑来', author: '烽火戏诸侯', intro: '原文', customCoverUrl: '',
    })

    expect(patch).toEqual({
      originalName: '剑来',
      originalAuthor: '烽火戏诸侯',
      originalIntro: '原文',
    })
  })

  it('reports nothing revertable while the origin is null', () => {
    expect(canRevertToOrigin(fromBackend)).toBe(false)
  })

  it('reports revertable once the origin was recorded as a real value', () => {
    expect(canRevertToOrigin({ ...fromBackend, originalIntro: '原文' })).toBe(true)
    expect(canRevertToOrigin({ ...fromBackend, originalIntro: '' })).toBe(true)
  })

  it('falls back to the current value when the origin is null', () => {
    const reverted = resolveRevertDraft(fromBackend, {
      name: '改过的', author: '改过的', intro: '改过的', customCoverUrl: ownCover(),
    })

    expect(reverted.name).toBe('改过的')
    expect(reverted.intro).toBe('改过的')
    expect(reverted.customCoverUrl).toBe('')
  })

  it('restores a real origin even when other fields are still null', () => {
    const reverted = resolveRevertDraft(
      { ...fromBackend, originalIntro: '最初的简介' },
      { name: '改过的', author: '改过的', intro: '改过的', customCoverUrl: '' },
    )

    expect(reverted.intro).toBe('最初的简介')
    // 其余字段仍是 null → 退回当前值, 不该变成 "null" 字符串。
    expect(reverted.name).toBe('改过的')
    expect(reverted.author).toBe('改过的')
  })

  it('completes the full cycle the user asked for: save then revert to the origin', () => {
    // 1) 首次保存: 记下原始值
    const first = originPatchForSave(fromBackend, createBookEditDraft(fromBackend))
    const saved = { ...fromBackend, ...first, name: '我改的书名', intro: '我改的简介' }

    // 2) 保存后应当可还原
    expect(canRevertToOrigin(saved)).toBe(true)

    // 3) 还原回到最初的样子
    const reverted = resolveRevertDraft(saved, createBookEditDraft(saved))
    expect(reverted.name).toBe('剑来')
    expect(reverted.intro).toBe('原文')
  })
})

describe('editableFieldsForSave (别把 None 写成 Some(""))', () => {
  // 回归测试: 后端 intro/author 是 Option<String> 且无 skip_serializing_if,
  // 而草稿会把「原本没有值」折成 ''。若照 '' 回传, 保存一次就把 None 写成
  // Some(""), 使 merge_book 的「书源有简介就回填」判据永远不成立。
  const next = { name: '剑来', author: '', intro: '', customCoverUrl: '' }

  it('omits author/intro that were absent and stayed empty', () => {
    const fields = editableFieldsForSave(
      { name: '剑来', author: undefined, intro: null },
      next,
    )

    expect('author' in fields).toBe(false)
    expect('intro' in fields).toBe(false)
    // name 必填、封面用 '' 表示回落书源封面
    expect(fields.name).toBe('剑来')
    expect(fields.customCoverUrl).toBe('')
  })

  it('submits the field once the user fills something in', () => {
    const fields = editableFieldsForSave(
      { name: '剑来', intro: undefined },
      { ...next, intro: '用户写的简介' },
    )

    expect(fields.intro).toBe('用户写的简介')
  })

  it('submits empty when the field already had a value', () => {
    // 原本有简介 → 用户清空 → 必须提交 '', 让清空生效, 而不是当作「没改」。
    const fields = editableFieldsForSave(
      { name: '剑来', intro: '原文' },
      { ...next, intro: '' },
    )

    expect(fields.intro).toBe('')
    expect('intro' in fields).toBe(true)
  })

  it('treats a recorded empty origin as having a value', () => {
    // originalIntro 是 '' 说明曾自定义过, 当前 intro 的 '' 是有效内容。
    const fields = editableFieldsForSave(
      { name: '剑来', intro: '', originalIntro: '' },
      { ...next, intro: '新简介' },
    )

    expect(fields.intro).toBe('新简介')
  })

  it('does not send keys the backend would read as null', () => {
    // 关键不变式: 省略键 → 后端保持 None; 发 '' → 后端变 Some("")。
    const fields = editableFieldsForSave({ name: 'x', intro: null }, next)
    expect(Object.values(fields)).not.toContain(undefined)
    expect('intro' in fields).toBe(false)
  })
})

describe('canRevertToOrigin', () => {
  it('is true once any origin value or a custom cover exists', () => {
    expect(canRevertToOrigin({ name: 'x', originalName: 'y' })).toBe(true)
    expect(canRevertToOrigin({ name: 'x', originalAuthor: 'y' })).toBe(true)
    expect(canRevertToOrigin({ name: 'x', originalIntro: 'y' })).toBe(true)
    expect(canRevertToOrigin({ name: 'x', customCoverUrl: ownCover() })).toBe(true)
  })

  it('is false for a book that was never customised', () => {
    // 没东西可回退时按钮应保持不可用, 而不是点了没反应。
    expect(canRevertToOrigin({ name: '剑来' })).toBe(false)
  })

  it('counts a recorded empty origin as revertable', () => {
    // 书源本来就没给简介/作者时, 原始值就是空字符串 —— 那是「记录过的原始状态」,
    // 不是「没有可还原内容」, 否则按钮会误禁用。
    expect(canRevertToOrigin({ name: '剑来', originalIntro: '' })).toBe(true)
    expect(canRevertToOrigin({ name: '剑来', originalName: '', originalAuthor: '' })).toBe(true)
  })

  it('treats an absent field as not recorded, unlike an empty one', () => {
    // undefined = 从未记录; '' = 记录过且原始为空。两者必须区分。
    expect(canRevertToOrigin({ name: '剑来' })).toBe(false)
    expect(canRevertToOrigin({ name: '剑来', originalIntro: '' })).toBe(true)
  })
})

describe('originPatchForSave', () => {
  const before = { name: '剑来', author: '烽火戏诸侯', intro: '原文', customCoverUrl: '' }

  it('records the pre-edit values on the very first save', () => {
    expect(originPatchForSave({ name: '剑来' }, before)).toEqual({
      originalName: '剑来',
      originalAuthor: '烽火戏诸侯',
      originalIntro: '原文',
    })
  })

  it('never overwrites an already recorded empty origin', () => {
    // 回归测试: 「原始简介为空 → 填入内容保存」之后, 原始空值必须保持为空。
    // 早先用 falsy 判断, '' 会被当成「尚未记录」, 于是每次保存都用上一次的值
    // 重写原始值 —— 原始空值永久丢失, 还原只能退回上一次保存的内容。
    const patch = originPatchForSave(
      { name: '剑来', originalName: '剑来', originalAuthor: '烽火戏诸侯', originalIntro: '' },
      { name: '剑来', author: '烽火戏诸侯', intro: '用户写的新简介', customCoverUrl: '' },
    )

    expect(patch).toEqual({})
  })

  it('round-trips an empty origin through save and revert', () => {
    // 原始简介为空, 用户填入内容并保存
    const book = { name: '剑来', author: '烽火戏诸侯' }
    const first = originPatchForSave(book, createBookEditDraft(book))
    expect(first.originalIntro).toBe('')

    const saved = { ...book, ...first, intro: '用户写的新简介' }
    // 再改一轮并保存: 原始值不能被这一轮覆盖
    const second = originPatchForSave(saved, createBookEditDraft(saved))
    expect(second).toEqual({})

    // 还原应该回到「原始为空」, 而不是上一次保存的内容
    const reverted = resolveRevertDraft(saved, createBookEditDraft(saved))
    expect(reverted.intro).toBe('')
    expect(canRevertToOrigin(saved)).toBe(true)
  })

  it('fills in only the fields that are still missing', () => {
    const patch = originPatchForSave({ name: 'x', originalName: '剑来' }, before)

    expect(patch.originalName).toBeUndefined()
    expect(patch.originalAuthor).toBe('烽火戏诸侯')
    expect(patch.originalIntro).toBe('原文')
  })

  it('round-trips: save then revert lands back on the original', () => {
    const book = { name: '剑来', author: '烽火戏诸侯', intro: '原文' }
    // 第一次编辑并保存
    const patch = originPatchForSave(book, createBookEditDraft(book))
    const saved = { ...book, ...patch, name: '新书名', intro: '新简介' }
    // 之后再还原
    const reverted = resolveRevertDraft(saved, createBookEditDraft(saved))

    expect(reverted.name).toBe('剑来')
    expect(reverted.author).toBe('烽火戏诸侯')
    expect(reverted.intro).toBe('原文')
  })
})

describe('isBookOnShelf (加入书架前的判重)', () => {
  // 回归测试: 发现页曾漏传 shelfBookUrls/shelfBookKeys, 导致点已在书架的书会
  // 再次 saveBook —— 那是一条只提交 5 个字段的路径, 后端整条覆盖, 用户自定义的
  // 封面/简介会被清空。挡住它的判据就是这里。
  const shelf = [
    { bookUrl: 'source-a://book/1', name: '剑来', author: '烽火戏诸侯' },
    { bookUrl: 'source-b://book/9', name: '遮天', author: '辰东' },
  ]

  it('matches the same book by bookUrl', () => {
    expect(isBookOnShelf({ bookUrl: 'source-a://book/1', name: '别的名字', author: '别人' }, shelf)).toBe(true)
  })

  it('matches across sources by normalized name+author', () => {
    // 同一本书在另一个书源的 URL 不同, 但书名/作者一致 —— 也必须判为已在书架,
    // 否则跨书源点「加入书架」同样会覆盖掉自定义内容。
    expect(isBookOnShelf(
      { bookUrl: 'source-c://book/777', name: '遮天', author: '辰东' },
      shelf,
    )).toBe(true)
  })

  it('ignores whitespace differences in name and author', () => {
    expect(isBookOnShelf(
      { bookUrl: 'source-c://other', name: ' 遮 天 ', author: '辰 东' },
      shelf,
    )).toBe(true)
  })

  it('treats a genuinely new book as not on the shelf', () => {
    expect(isBookOnShelf(
      { bookUrl: 'source-c://book/123', name: '新书', author: '新作者' },
      shelf,
    )).toBe(false)
  })

  it('is false for an empty shelf and for a book without a URL', () => {
    expect(isBookOnShelf({ bookUrl: 'source-a://book/1', name: '剑来' }, [])).toBe(false)
    // 没有 bookUrl 的结果无法可靠判重, 交给后端; 这里不误判成「已在书架」。
    expect(isBookOnShelf({ bookUrl: '', name: '剑来', author: '烽火戏诸侯' }, shelf)).toBe(false)
  })
})
