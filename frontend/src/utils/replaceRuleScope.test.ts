import { describe, expect, it } from 'vitest'
import {
  appliesToReplaceTarget,
  matchesReplaceRuleScope,
  ruleAppliesTo,
  type ReplaceRuleScopeTarget,
} from './replaceRuleScope'

const book: ReplaceRuleScopeTarget = {
  name: '剑来',
  origin: 'https://source.example.test',
  bookUrl: 'https://source.example.test/book/1',
}

describe('matchesReplaceRuleScope', () => {
  it('空范围与 * 对全部书籍生效', () => {
    for (const scope of [undefined, null, '', '  ', '*']) {
      expect(matchesReplaceRuleScope({ scope }, book)).toBe(true)
    }
  })

  it('手机端导出的混合串按「包含」命中（书源地址在串里）', () => {
    // Legado 的 scope 常写成「书名,书源地址」这种混合串，精确相等会一条都不生效。
    expect(matchesReplaceRuleScope({ scope: '剑来,https://source.example.test' }, book)).toBe(true)
  })

  it('书名出现在任意位置即命中', () => {
    expect(matchesReplaceRuleScope({ scope: '随便什么剑来随便什么' }, book)).toBe(true)
  })

  it('书源地址出现在任意位置即命中', () => {
    expect(matchesReplaceRuleScope({ scope: '前缀 https://source.example.test 后缀' }, book)).toBe(true)
  })

  it('都不包含时不命中', () => {
    expect(matchesReplaceRuleScope({ scope: '别的书别处' }, book)).toBe(false)
  })

  it('桌面端两种前缀写法保持精确匹配，不被模糊扩展', () => {
    expect(matchesReplaceRuleScope({ scope: 'book:https://source.example.test/book/1' }, book)).toBe(true)
    // 前缀写法下「包含」不算命中：这是桌面端自己的约定，不该被放宽。
    expect(matchesReplaceRuleScope({ scope: 'book:https://source.example.test/book/1/extra' }, book))
      .toBe(false)
    expect(matchesReplaceRuleScope({ scope: 'source:https://source.example.test' }, book)).toBe(true)
    expect(matchesReplaceRuleScope({ scope: 'source:https://other.test' }, book)).toBe(false)
  })

  it('没开书时只有 * 生效', () => {
    expect(matchesReplaceRuleScope({ scope: '*' }, null)).toBe(true)
    expect(matchesReplaceRuleScope({}, null)).toBe(true)
    expect(matchesReplaceRuleScope({ scope: '剑来' }, null)).toBe(false)
  })

  it('排除范围命中即整体排除（书名或书源任一命中）', () => {
    expect(matchesReplaceRuleScope({ scope: '*', excludeScope: '剑来' }, book)).toBe(false)
    // 注意方向：手机端是 `excludeScope LIKE '%' || origin || '%'`，即**排除串里要出现
    // 完整的书源地址**，而不是「排除串是地址的子串」。
    expect(matchesReplaceRuleScope(
      { scope: '*', excludeScope: 'https://source.example.test' },
      book,
    )).toBe(false)
    // 只写域名片段匹配不到完整 origin，因此不会排除 —— 与手机端一致。
    expect(matchesReplaceRuleScope({ scope: '*', excludeScope: 'source.example.test' }, book))
      .toBe(true)
    expect(matchesReplaceRuleScope({ scope: '*', excludeScope: '别的书' }, book)).toBe(true)
  })

  it('排除范围在没开书时不会误触发', () => {
    expect(matchesReplaceRuleScope({ scope: '*', excludeScope: '剑来' }, null)).toBe(true)
  })

  it('排除优先于生效范围', () => {
    expect(matchesReplaceRuleScope({ scope: '剑来', excludeScope: '剑来' }, book)).toBe(false)
  })

  it('空白的排除串等同于没有排除', () => {
    expect(matchesReplaceRuleScope({ scope: '*', excludeScope: '   ' }, book)).toBe(true)
  })
})

describe('appliesToReplaceTarget', () => {
  it('两个开关都没写时按手机端默认：只作用于正文', () => {
    expect(appliesToReplaceTarget({}, 'content')).toBe(true)
    expect(appliesToReplaceTarget({}, 'title')).toBe(false)
  })

  it('只开正文', () => {
    const rule = { scopeContent: true, scopeTitle: false }
    expect(appliesToReplaceTarget(rule, 'content')).toBe(true)
    expect(appliesToReplaceTarget(rule, 'title')).toBe(false)
  })

  it('只开标题', () => {
    const rule = { scopeContent: false, scopeTitle: true }
    expect(appliesToReplaceTarget(rule, 'content')).toBe(false)
    expect(appliesToReplaceTarget(rule, 'title')).toBe(true)
  })

  it('两个都开时正文与标题都作用', () => {
    const rule = { scopeContent: true, scopeTitle: true }
    expect(appliesToReplaceTarget(rule, 'content')).toBe(true)
    expect(appliesToReplaceTarget(rule, 'title')).toBe(true)
  })

  it('两个都显式关掉时谁都不作用', () => {
    const rule = { scopeContent: false, scopeTitle: false }
    expect(appliesToReplaceTarget(rule, 'content')).toBe(false)
    expect(appliesToReplaceTarget(rule, 'title')).toBe(false)
  })

  it('只写了 scopeTitle:false（老数据缺 scopeContent）仍按正文处理', () => {
    expect(appliesToReplaceTarget({ scopeTitle: false }, 'content')).toBe(true)
    expect(appliesToReplaceTarget({ scopeTitle: false }, 'title')).toBe(false)
  })
})

describe('ruleAppliesTo', () => {
  it('停用的规则一律不作用', () => {
    const rule = { isEnabled: false, scopeContent: true }
    expect(ruleAppliesTo(rule, 'content', book)).toBe(false)
    expect(ruleAppliesTo(rule, 'title', book)).toBe(false)
  })

  it('作用目标、范围、启用状态三者都要满足', () => {
    const titleRule = { isEnabled: true, scopeContent: false, scopeTitle: true, scope: '剑来' }
    expect(ruleAppliesTo(titleRule, 'title', book)).toBe(true)
    expect(ruleAppliesTo(titleRule, 'content', book)).toBe(false)
    expect(ruleAppliesTo({ ...titleRule, scope: '别的书' }, 'title', book)).toBe(false)
    expect(ruleAppliesTo({ ...titleRule, isEnabled: false }, 'title', book)).toBe(false)
  })
})
