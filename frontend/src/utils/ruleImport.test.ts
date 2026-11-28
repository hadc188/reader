import { describe, expect, it } from 'vitest'
import { markImportedRules, parseImportedRuleList } from './ruleImport'

describe('rule import helpers', () => {
  it('accepts a single rule object', () => {
    expect(parseImportedRuleList<{ name: string }>({ name: '单条规则' })).toEqual([
      { name: '单条规则' },
    ])
  })

  it('filters non-object entries from a rule array', () => {
    expect(parseImportedRuleList<{ name: string }>([
      { name: '有效规则' },
      null,
      '无效规则',
    ])).toEqual([{ name: '有效规则' }])
  })

  it('marks new, update, and same rules', () => {
    const incoming = [
      { id: 0, name: '新增规则' },
      { id: 2, name: '更新规则', value: 'new' },
      { id: 3, name: '已有规则', value: 'same' },
    ]
    const existing = [
      { id: 2, name: '更新规则', value: 'old' },
      { id: 3, name: '已有规则', value: 'same' },
    ]

    expect(markImportedRules(incoming, existing).map((rule) => rule.importState)).toEqual([
      'new',
      'update',
      'same',
    ])
  })
})
