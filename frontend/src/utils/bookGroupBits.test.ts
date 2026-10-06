import { describe, expect, it } from 'vitest'
import { bookInAnyGroup, clearGroupsFromBook, groupMask } from './bookGroupBits'

describe('groupMask', () => {
  it('ORs the given ids together', () => {
    // Legado 用 1/2/4/8… 作位; 传入的是位值本身, 不是位序号。
    expect(groupMask([1, 2, 4])).toBe(7)
    expect(groupMask([1])).toBe(1)
    expect(groupMask([8])).toBe(8)
  })

  it('is 0 for an empty selection', () => {
    expect(groupMask([])).toBe(0)
  })

  it('ignores non-positive ids, which cannot be represented as bits', () => {
    // Legado 的负数分组(IdLocal=-2 等)是虚拟分组, 不参与位域。
    expect(groupMask([-2, -1, 0])).toBe(0)
    expect(groupMask([-2, 4])).toBe(4)
  })

  it('ignores non-integers and out-of-range values instead of wrapping', () => {
    // JS 位运算是 32 位有符号: 超出范围会回绕, 宁可当作无效也不要算出错误掩码。
    expect(groupMask([1.5, 2])).toBe(2)
    expect(groupMask([0x8000_0001, 2])).toBe(2)
    expect(groupMask([Number.NaN, 2])).toBe(2)
  })
})

describe('clearGroupsFromBook', () => {
  it('clears only the removed groups, keeping the others', () => {
    // 1 | 2 | 4 = 7, 删掉 2 之后应剩 5。
    const book = { name: '书', group: 7 }

    expect(clearGroupsFromBook(book, [2]).group).toBe(5)
  })

  it('leaves a book that is not in any removed group untouched', () => {
    const book = { name: '书', group: 4 }

    // 返回同一个对象(引用相等), 调用方据此判断有没有必要写回。
    expect(clearGroupsFromBook(book, [1, 2])).toBe(book)
  })

  it('handles a book without a group field', () => {
    const book: { name: string; group?: number } = { name: '书' }

    expect(clearGroupsFromBook(book, [1])).toBe(book)
  })

  it('treats group 0 as no membership', () => {
    const book = { name: '书', group: 0 }

    expect(clearGroupsFromBook(book, [1, 2])).toBe(book)
  })

  it('returns a new object when it does change', () => {
    const book = { name: '书', group: 3 }
    const next = clearGroupsFromBook(book, [1])

    expect(next).not.toBe(book)
    expect(next.group).toBe(2)
  })

  it('does nothing for an empty removal list', () => {
    const book = { name: '书', group: 7 }

    expect(clearGroupsFromBook(book, [])).toBe(book)
  })

  it('clears several groups at once', () => {
    const book = { name: '书', group: 1 | 2 | 4 | 8 }

    expect(clearGroupsFromBook(book, [2, 8]).group).toBe(1 | 4)
  })

  it('clears everything when all groups are removed', () => {
    const book = { name: '书', group: 1 | 2 }

    expect(clearGroupsFromBook(book, [1, 2]).group).toBe(0)
  })

  it('preserves unrelated fields', () => {
    const book = { name: '书', author: '作者', group: 3 }

    expect(clearGroupsFromBook(book, [1])).toMatchObject({ name: '书', author: '作者' })
  })

  it('ignores negative ids that cannot be bits', () => {
    // 删除一个负数虚拟分组不该动到书的任何位。
    const book = { name: '书', group: 3 }

    expect(clearGroupsFromBook(book, [-2, -1])).toBe(book)
  })
})

describe('bookInAnyGroup', () => {
  it('detects membership in any listed group', () => {
    expect(bookInAnyGroup({ group: 2 }, [1, 2])).toBe(true)
    expect(bookInAnyGroup({ group: 4 }, [1, 2])).toBe(false)
  })

  it('is false for an empty selection or a book with no group', () => {
    expect(bookInAnyGroup({ group: 7 }, [])).toBe(false)
    expect(bookInAnyGroup({}, [1])).toBe(false)
  })
})
