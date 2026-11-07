/**
 * 净化规则的「这条规则该不该跑」判定。
 *
 * 对齐手机端 `ReplaceRuleDao.findEnabledByContentScope` / `findEnabledByTitleScope`：
 *
 * ```sql
 * WHERE isEnabled = 1 and scopeContent = 1
 *   AND (scope LIKE '%' || :name || '%' or scope LIKE '%' || :origin || '%'
 *        or scope is null or scope = '')
 *   AND (excludeScope is null
 *        or (excludeScope not LIKE '%' || :name || '%'
 *            and excludeScope not LIKE '%' || :origin || '%'))
 * ```
 *
 * 三个关键点，缺一条就会和手机端对不上：
 * 1. **子串匹配**，不是相等。手机端导出的 `scope` 常写成 `我的书,https://a.com` 这种
 *    混合串，精确相等会让规则一条都不生效。
 * 2. 书名与书源地址**任一**命中即可。
 * 3. `excludeScope` 命中的是「任一」，就整体排除（SQL 里两个 `not LIKE` 用 and）。
 *
 * 桌面端另有 `book:` / `source:` 前缀写法（阅读页「按本书替换 / 按书源替换」写入的），
 * 这两类保持精确匹配 —— 它们是桌面端自己的约定，不该被模糊匹配扩大范围。
 */

export interface ReplaceRuleScopeTarget {
  /** 书名。 */
  name: string
  /** 书源地址（`book.origin`）。 */
  origin: string
  /** 书籍地址（`book.bookUrl`）。 */
  bookUrl: string
}

/** 规则里与范围相关的字段子集。 */
export interface ReplaceRuleScopeFields {
  scope?: string | null
  excludeScope?: string | null
  scopeTitle?: boolean
  scopeContent?: boolean
  isEnabled?: boolean
}

/** 桌面端自己的两种前缀写法：精确匹配，不做子串扩展。 */
function matchesPrefixedScope(scope: string, target: ReplaceRuleScopeTarget): boolean {
  if (scope.startsWith('source:')) {
    return scope.slice('source:'.length) === target.origin
  }
  if (scope.startsWith('book:')) {
    return scope.slice('book:'.length) === target.bookUrl
  }
  return false
}

function isPrefixedScope(scope: string): boolean {
  return scope.startsWith('source:') || scope.startsWith('book:')
}

/**
 * `scope LIKE '%关键词%'` 的等价判定：只要 scope 串里包含书名或书源地址就算命中。
 *
 * 手机端拿 `name` 和 `origin` 两个值去 LIKE，不是把 scope 拆成多个关键词 —— 所以
 * 用户写 `A;B` 时，只有「整个串里含书名/书源」才算，不是「A 或 B 任一命中」。
 */
function matchesLooseScope(scope: string, target: ReplaceRuleScopeTarget): boolean {
  const name = target.name.trim()
  const origin = target.origin.trim()
  if (name && scope.includes(name)) return true
  if (origin && scope.includes(origin)) return true
  return false
}

/** 生效范围判定。空 / `*` 表示全部书籍。 */
export function matchesReplaceRuleScope(
  rule: ReplaceRuleScopeFields,
  target: ReplaceRuleScopeTarget | null,
): boolean {
  const scope = (rule.scope || '').trim()
  const excluded = (rule.excludeScope || '').trim()

  // 排除范围优先：命中的是「任一」就整体排除，与手机端一致。
  if (excluded && target) {
    if (matchesLooseScope(excluded, target)) return false
  }

  if (!scope || scope === '*') return true
  if (!target) return false

  if (isPrefixedScope(scope)) return matchesPrefixedScope(scope, target)
  return matchesLooseScope(scope, target)
}

/** 这条规则是否作用于当前场景（正文 / 标题）。 */
export function appliesToReplaceTarget(
  rule: ReplaceRuleScopeFields,
  target: 'content' | 'title',
): boolean {
  // 两个开关都缺省时按「正文」处理：老规则与手机端默认值（content=1/title=0）一致。
  const hasExplicit = rule.scopeTitle === true || rule.scopeContent === true
  if (!hasExplicit) {
    // 显式都关掉（scopeTitle:false + scopeContent:false）时谁都不作用；
    // 只有「两个字段都没写」才走默认。
    if (rule.scopeTitle === false && rule.scopeContent === false) {
      return false
    }
    return target === 'content'
  }
  return target === 'content' ? rule.scopeContent === true : rule.scopeTitle === true
}

/** 标题净化与正文净化共用同一条范围判定，只是场景不同。 */
export function ruleAppliesTo(
  rule: ReplaceRuleScopeFields,
  target: 'content' | 'title',
  bookTarget: ReplaceRuleScopeTarget | null,
): boolean {
  if (rule.isEnabled === false) return false
  if (!appliesToReplaceTarget(rule, target)) return false
  return matchesReplaceRuleScope(rule, bookTarget)
}
