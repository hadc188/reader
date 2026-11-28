import { invokeEnvelope } from './invoke'

export interface TxtTocRule {
  id: number
  name: string
  rule: string
  example: string
  enable: boolean
  order: number
}

export interface RebuildLocalTxtTocsResult {
  rebuilt: number
  failed: number
  metadataFailed: number
}

function normalizeRule(raw: Record<string, unknown>): TxtTocRule {
  return {
    id: Number(raw.id) || 0,
    name: String(raw.name || ''),
    rule: String(raw.rule || ''),
    example: String(raw.example || ''),
    enable: raw.enable !== false,
    order: Number(raw.order) || 0,
  }
}

export async function getTxtTocRules(): Promise<TxtTocRule[]> {
  const rules = await invokeEnvelope<Record<string, unknown>[]>('get_txt_toc_rules', {})
  return (rules || []).map(normalizeRule)
}

export async function saveTxtTocRule(rule: TxtTocRule): Promise<TxtTocRule> {
  const saved = await invokeEnvelope<Record<string, unknown>>('save_txt_toc_rule', { rule })
  return normalizeRule(saved)
}

export async function deleteTxtTocRule(id: number): Promise<void> {
  await invokeEnvelope('delete_txt_toc_rule', { id })
}

/** 仅用于阅读页刷新单本 TXT；规则增删改和启停只保存规则表。 */
export async function rebuildLocalTxtTocs(bookUrl: string): Promise<RebuildLocalTxtTocsResult> {
  return invokeEnvelope<RebuildLocalTxtTocsResult>('rebuild_local_txt_tocs', { bookUrl })
}
