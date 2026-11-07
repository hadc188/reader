/**
 * Legado 净化规则 → Reader 净化规则 的导入归一化。
 *
 * 这里刻意做成纯函数、不碰 DOM / fetch / Tauri IPC:
 * 「直链怎么拉、本地文件怎么读」是调用方(面板)的事, 本模块只负责
 * 「宽容解析 → 字段映射 → 跳过统计」, 于是可以直接在 node 环境下单测。
 *
 * 为什么需要映射:
 *   Legado 的 ReplaceRule 用 `scopeContent`(bool) + `scopeTitle`(bool) 两个开关
 *   表达作用目标, 另有 `scope` / `excludeScope` 两个字符串表达生效与排除范围。
 *   Reader 现在把这些字段**原样保留**: 目标开关直接落到同名字段, 两个范围串
 *   照搬, 匹配语义由 utils/replaceRuleScope 按手机端的「包含」规则实现。
 *   老 JSON 缺字段时按手机端实体默认值补齐(正文开、标题关)。
 */
import type { ReplaceRule } from '../types'

/** 跳过/失败时的样例名上限: toast 只有 3 秒, 名字太长会把关键数字挤没。 */
const SAMPLE_LIMIT = 3

/** Legado 的 JS 脚本替换前缀(大小写不敏感, 允许前导空白/换行)。 */
const JS_RULE_PREFIX = '@js:'

export interface ReplaceRuleImportSkipped {
  /** 被跳过的总条数 = jsRules + invalid。 */
  total: number
  /** pattern 或 replacement 以 @js: 开头的脚本规则 —— Reader 无等价能力。 */
  jsRules: number
  /** 缺 name / 缺 pattern / 根本不是对象等结构无效的条目。 */
  invalid: number
  /** 被跳过条目的名字(无效条目无名时回退为「第 N 条」), 最多 3 个。 */
  samples: string[]
}

export interface ReplaceRuleImportResult {
  rules: ReplaceRule[]
  skipped: ReplaceRuleImportSkipped
  /** 导入的规则里「Legado 仅作用于标题」的条数 —— Reader 只能按正文范围导入。 */
  titleOnly: number
  /** 导入的规则里用 `@js:` 脚本做替换的条数（在 Rust 沙盒里执行）。 */
  scriptRules: number
}

export type ReplaceRuleImportFeedbackType = 'success' | 'warning' | 'error'

export interface ReplaceRuleImportFeedback {
  text: string
  type: ReplaceRuleImportFeedbackType
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null && !Array.isArray(value)
}

/** 只接受 number 或非空数字字符串。`Number(null)` / `Number('')` 都会得到 0,
 *  直接把 null 当 0 会让「字段缺失」和「显式 0」混为一谈, 所以先判类型。 */
function toFiniteNumber(value: unknown): number | null {
  if (typeof value === 'number') return Number.isFinite(value) ? value : null
  if (typeof value === 'string' && value.trim()) {
    const parsed = Number(value)
    return Number.isFinite(parsed) ? parsed : null
  }
  return null
}

/** 判断一段规则文本是不是 Legado 的 JS 脚本。
 *
 *  必须 `trim()`: 实测样本里有 replacement 以换行/空格开头再接 `@js:` 的写法,
 *  只判首字符会漏掉它们。大小写也不敏感(`@JS:` 同样可能出现)。 */
function isJsRuleText(value: string) {
  return value.trim().toLowerCase().startsWith(JS_RULE_PREFIX)
}

function describeEntry(index: number, name: string) {
  return name || `第 ${index + 1} 条`
}

/**
 * 把 Legado 净化规则 JSON(任意 `unknown`)归一化为 Reader 的 `ReplaceRule[]`。
 *
 * 宽容策略: 单条字段缺失/类型不符只把该条计入 `skipped.invalid`, 不影响其余条目。
 * 顶层不是数组则直接抛错 —— 那是「文件选错了」而不是「少数条目坏了」,
 * 静默吞掉会让用户以为导入成功却什么都没发生。
 *
 * @throws 顶层不是数组时抛出带中文说明的 `Error`。
 */
export function normalizeLegadoReplaceRules(raw: unknown): ReplaceRuleImportResult {
  if (!Array.isArray(raw)) {
    throw new Error('净化规则 JSON 格式不正确：顶层应为数组（[...]）')
  }

  const rules: ReplaceRule[] = []
  const skipped: ReplaceRuleImportSkipped = { total: 0, jsRules: 0, invalid: 0, samples: [] }
  let titleOnly = 0
  let scriptRules = 0

  const pushSample = (name: string) => {
    if (skipped.samples.length < SAMPLE_LIMIT) skipped.samples.push(name)
  }
  const markInvalid = (name: string) => {
    skipped.total += 1
    skipped.invalid += 1
    pushSample(name)
  }

  raw.forEach((entry, index) => {
    if (!isRecord(entry)) {
      markInvalid(describeEntry(index, ''))
      return
    }

    const name = typeof entry.name === 'string' ? entry.name.trim() : ''
    const pattern = typeof entry.pattern === 'string' ? entry.pattern : ''
    // name / pattern 是 Reader 侧的最低要求(后端 save_replace_rules 也会过滤掉),
    // 所以结构无效优先判定: 一条连名字都没有的 @js: 规则算「无效」而不是「脚本」。
    if (!name || !pattern.trim()) {
      markInvalid(describeEntry(index, name))
      return
    }

    const replacement = typeof entry.replacement === 'string' ? entry.replacement : ''
    // 只跳过「pattern 本身是脚本」的规则：那类规则要用 JS 生成匹配式，沙盒里也没有
    // 对应通道。`replacement` 是 `@js:` 的规则**可以导入** —— 替换脚本在 Rust 沙盒里
    // 执行（见 parser::replace_rule_js），这里只统计条数用于提示。
    if (isJsRuleText(pattern)) {
      skipped.total += 1
      skipped.jsRules += 1
      pushSample(name)
      return
    }
    if (isJsRuleText(replacement)) scriptRules += 1

    // 作用范围按手机端原样保留：
    // - `scopeContent` / `scopeTitle` 决定这条规则管正文还是管标题（以前一律丢弃，
    //   于是「仅标题」的规则导入后完全无效）。
    // - 字符串 `scope` / `excludeScope` 直接照搬，匹配语义由 utils/replaceRuleScope
    //   按 LIKE %…% 的包含规则实现。
    // - 老 JSON 可能两个开关都没有：按手机端实体默认值补（正文开、标题关）。
    const scopeContent = typeof entry.scopeContent === 'boolean' ? entry.scopeContent : true
    const scopeTitle = typeof entry.scopeTitle === 'boolean' ? entry.scopeTitle : false
    if (scopeTitle && !scopeContent) titleOnly += 1

    const id = toFiniteNumber(entry.id)
    rules.push({
      // 缺 id 时用负数占位: 后端 save_replace_rules 按 name upsert, id 只是列表 key;
      // 用负数可避免与「手工新增规则」的 id=0 撞 key, 同时保证本批内唯一。
      id: id !== null && id >= 0 ? id : -(index + 1),
      name,
      group: typeof entry.group === 'string' && entry.group.trim() ? entry.group.trim() : undefined,
      // pattern 保留原样(不 trim): 正则里的前导/尾随空白是有意义的匹配条件。
      pattern,
      replacement,
      scope: typeof entry.scope === 'string' && entry.scope.trim() ? entry.scope.trim() : '*',
      excludeScope: typeof entry.excludeScope === 'string' && entry.excludeScope.trim()
        ? entry.excludeScope.trim()
        : undefined,
      scopeContent,
      scopeTitle,
      isEnabled: typeof entry.isEnabled === 'boolean' ? entry.isEnabled : true,
      // 缺 isRegex 按正则处理, 与 webdavBackup 里 Legado→Reader 的既有映射保持一致。
      isRegex: typeof entry.isRegex === 'boolean' ? entry.isRegex : true,
      timeoutMillisecond: toFiniteNumber(entry.timeoutMillisecond) ?? 3000,
      order: toFiniteNumber(entry.order) ?? index + 1,
    })
  })

  return { rules, skipped, titleOnly, scriptRules }
}

/**
 * 把归一化结果翻译成给用户看的一句话。
 *
 * 单独抽出来的原因: 「跳过明细」「全部被跳过」「仅标题」的措辞是行为约定的一部分,
 * 放在组件模板里就只能在浏览器里人工验证; 放这里可以单测。
 * 全部被跳过时必须是 error 且带样例名 —— 提示「导入 0 条」等于什么都没说。
 */
export function buildReplaceRuleImportMessage(result: ReplaceRuleImportResult): ReplaceRuleImportFeedback {
  const { rules, skipped, titleOnly, scriptRules } = result

  const skipParts: string[] = []
  if (skipped.jsRules) skipParts.push(`${skipped.jsRules} 条用脚本生成匹配式的规则`)
  if (skipped.invalid) skipParts.push(`${skipped.invalid} 条无效`)
  const skipDetail = skipParts.length ? `跳过 ${skipped.total} 条：${skipParts.join('、')}` : ''

  if (!rules.length) {
    const reason = skipped.total
      ? `未导入任何规则（${skipDetail}）`
      : '没有可导入的净化规则（JSON 数组为空）'
    const samples = skipped.samples.length ? `；例如：${skipped.samples.join('、')}` : ''
    return { text: reason + samples, type: 'error' }
  }

  const notes: string[] = []
  if (skipDetail) notes.push(skipDetail)
  if (scriptRules) notes.push(`${scriptRules} 条脚本规则在后端沙盒执行`)
  if (titleOnly) notes.push(`${titleOnly} 条只作用于章节标题`)

  return {
    text: notes.length ? `导入 ${rules.length} 条（${notes.join('；')}）` : `导入 ${rules.length} 条`,
    // 有跳过就意味着「成功但结果不是你给的全部」, 用 warning 而不是 success 提醒看一眼。
    type: skipped.total ? 'warning' : 'success',
  }
}
