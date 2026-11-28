export interface ExistingRuleLike {
  id: number
  name: string
}

export interface ImportedRuleLike extends ExistingRuleLike {
  selected: boolean
  importState: 'new' | 'update' | 'same'
}

export function parseImportedRuleList<T>(raw: unknown): T[] {
  if (Array.isArray(raw)) {
    return raw.filter((item): item is T => typeof item === 'object' && item !== null)
  }
  if (raw && typeof raw === 'object') {
    return [raw as T]
  }
  throw new Error('规则内容需要是 JSON 对象或数组')
}

export function markImportedRules<T extends ExistingRuleLike>(
  incoming: T[],
  existing: ExistingRuleLike[],
): Array<T & ImportedRuleLike> {
  const currentById = new Map(existing.map((rule) => [String(rule.id), rule]))
  const currentByName = new Map(existing.map((rule) => [rule.name.trim(), rule]))
  return incoming.map((rule) => {
    const current = currentById.get(String(rule.id)) || currentByName.get(rule.name.trim())
    const importState = current
      ? JSON.stringify(current) === JSON.stringify(rule)
        ? 'same'
        : 'update'
      : 'new'
    return {
      ...rule,
      selected: importState !== 'same',
      importState,
    } as T & ImportedRuleLike
  })
}

export async function readRuleImportText(
  source: string,
  file?: File | null,
): Promise<string> {
  if (file) return file.text()
  const trimmed = source.trim()
  if (!trimmed) throw new Error('请输入规则地址或选择本地文件')
  if (!/^https?:\/\//i.test(trimmed)) {
    return trimmed
  }
  const response = await fetch(trimmed, { cache: 'no-store' })
  if (!response.ok) throw new Error(`获取规则失败：${response.status}`)
  return response.text()
}
