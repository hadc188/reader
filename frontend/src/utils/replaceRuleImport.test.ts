import { describe, expect, it } from 'vitest'
import {
  buildReplaceRuleImportMessage,
  normalizeLegadoReplaceRules,
} from './replaceRuleImport'

/** Legado 净化规则条目的典型形态（字段齐全）。
 *  名字与数字都是占位, 不指代任何真实站点或书源 —— 仓库约定 fixture 不指名真实源站。 */
function legadoRule(overrides: Record<string, unknown> = {}) {
  return {
    group: '格式',
    id: 1,
    isEnabled: true,
    isRegex: true,
    name: '示例规则',
    order: 1,
    pattern: '\\s+',
    replacement: '',
    scopeContent: true,
    scopeTitle: false,
    ...overrides,
  }
}

/** 一份「实测形态」的导出样本: 16 条正文规则 + 4 条 @js: 脚本规则。 */
function realisticExport() {
  const contentRules = Array.from({ length: 16 }, (_, i) => legadoRule({
    id: i + 5,
    name: `#${String(i + 5).padStart(2, '0')} 正文规则`,
    order: i + 5,
  }))
  const jsRules = [
    // 标题型规则, 恰好也是脚本替换
    legadoRule({
      id: 1,
      name: '#01 数字标题#JS',
      order: 1,
      replacement: '@js:if (result.slice(0,1)=="第") { result.replace(/一/g,"1") }',
      scopeContent: false,
      scopeTitle: true,
    }),
    legadoRule({ id: 2, name: '#02 全角字符#JS', order: 2, replacement: '@JS:R=result' }),
    // 实测存在以换行开头的写法
    legadoRule({ id: 3, name: '#03 其他字符#JS', order: 3, replacement: '\n@js:R=result' }),
    // pattern 侧也可能是脚本
    legadoRule({ id: 4, name: '#04 脚本匹配', order: 4, pattern: ' @js:return pattern' }),
  ]
  return [...contentRules, ...jsRules]
}

describe('normalizeLegadoReplaceRules', () => {
  it('把 Legado 字段映射为 Reader 规则, 范围与作用目标原样保留', () => {
    const result = normalizeLegadoReplaceRules([legadoRule()])
    expect(result.rules).toEqual([
      {
        id: 1,
        name: '示例规则',
        group: '格式',
        pattern: '\\s+',
        replacement: '',
        scope: '*',
        excludeScope: undefined,
        scopeContent: true,
        scopeTitle: false,
        isEnabled: true,
        isRegex: true,
        timeoutMillisecond: 3000,
        order: 1,
      },
    ])
    expect(result.skipped).toEqual({ total: 0, jsRules: 0, invalid: 0, samples: [] })
    expect(result.titleOnly).toBe(0)
  })

  it('实测形态的 20 条样本: 19 条导入, 仅跳过「pattern 本身是脚本」的 1 条', () => {
    const result = normalizeLegadoReplaceRules(realisticExport())
    expect(result.rules).toHaveLength(19)
    expect(result.skipped).toEqual({
      total: 1,
      jsRules: 1,
      invalid: 0,
      samples: ['#04 脚本匹配'],
    })
    // replacement 侧是 @js: 的规则现在可以导入, 但要统计出来供提示使用
    expect(result.scriptRules).toBe(3)
    // #01 是「仅作用于标题」, 导入后计入 titleOnly
    expect(result.titleOnly).toBe(1)
    expect(result.rules.every((rule) => rule.scope === '*')).toBe(true)
  })

  it('replacement 侧 @js: 前缀大小写不敏感且允许前导空白, 这类规则现在照常导入', () => {
    const result = normalizeLegadoReplaceRules([
      legadoRule({ name: '小写', replacement: '@js:x' }),
      legadoRule({ name: '大写', replacement: '@JS:x' }),
      legadoRule({ name: '前导换行', replacement: '\n  \t@js:x' }),
      legadoRule({ name: '匹配侧', pattern: '  @js:return p' }),
    ])
    // 前三条导入（替换脚本交给后端沙盒）, 第四条 pattern 是脚本, 无通道只能跳过
    expect(result.rules.map((rule) => rule.name)).toEqual(['小写', '大写', '前导换行'])
    expect(result.scriptRules).toBe(3)
    expect(result.skipped).toMatchObject({ total: 1, jsRules: 1, invalid: 0 })
  })

  it('结构无效优先于 @js: 判定, 同一只计一次', () => {
    const result = normalizeLegadoReplaceRules([
      { name: '', pattern: 'p', replacement: '@js:x' },
    ])
    // 连 name 都没有的规则算「无效」, 不能借 @js: 混进脚本统计里
    expect(result.skipped).toMatchObject({ total: 1, jsRules: 0, invalid: 1 })
  })

  it('单条字段缺失/类型不符只跳过该条, 不影响其余条目', () => {
    const result = normalizeLegadoReplaceRules([
      null,
      42,
      '不是对象',
      {},
      { name: '缺 pattern' },
      { pattern: '缺 name' },
      { name: '   ', pattern: '空名字' },
      legadoRule({ name: '可导入', pattern: 'ok' }),
    ])
    expect(result.rules.map((rule) => rule.name)).toEqual(['可导入'])
    expect(result.skipped).toMatchObject({ total: 7, jsRules: 0, invalid: 7 })
  })

  it('name 去掉两端空白, pattern 原样保留（正则里的空白有意义）', () => {
    const rule = normalizeLegadoReplaceRules([
      legadoRule({ name: '  带空格的规则  ', pattern: '  \\s+  ' }),
    ]).rules[0]
    expect(rule.name).toBe('带空格的规则')
    expect(rule.pattern).toBe('  \\s+  ')
  })

  it('缺少 isEnabled/isRegex/order/group 时给出安全默认值', () => {
    const rules = normalizeLegadoReplaceRules([
      { name: '一条', pattern: 'p' },
      { name: '二条', pattern: 'p' },
    ]).rules
    expect(rules[0]).toMatchObject({ isEnabled: true, isRegex: true, order: 1 })
    expect(rules[1]).toMatchObject({ order: 2 })
    expect(rules[0].group).toBeUndefined()
    // 空/非字符串 group 同样忽略, 而不是写成空串
    expect(normalizeLegadoReplaceRules([legadoRule({ group: '   ' })]).rules[0].group).toBeUndefined()
  })

  it('replacement 不是字符串时按「替换为空」(删除匹配) 处理', () => {
    const rules = normalizeLegadoReplaceRules([
      legadoRule({ replacement: 42 }),
      legadoRule({ replacement: null }),
    ]).rules
    expect(rules.every((rule) => rule.replacement === '')).toBe(true)
  })

  it('「仅作用于标题」的规则照样导入, 但单独计入 titleOnly', () => {
    const result = normalizeLegadoReplaceRules([
      // 只作用于标题
      legadoRule({ name: '仅标题', scopeContent: false, scopeTitle: true }),
      // 正文+标题都作用: 不算「仅」, titleOnly 不计
      legadoRule({ name: '两者都管', scopeContent: true, scopeTitle: true }),
      // 只作用于正文
      legadoRule({ name: '仅正文', scopeContent: true, scopeTitle: false }),
    ])
    expect(result.rules).toHaveLength(3)
    expect(result.titleOnly).toBe(1)
    expect(result.rules.every((rule) => rule.scope === '*')).toBe(true)
  })

  it('保留 id; 缺失或非法 id 用负数占位, 避免与手工规则 id=0 撞 key', () => {
    const rules = normalizeLegadoReplaceRules([
      legadoRule({ name: 'a', id: 7 }),
      legadoRule({ name: 'b', id: undefined }),
      legadoRule({ name: 'c', id: 'oops' }),
      legadoRule({ name: 'd', id: -3 }),
    ]).rules
    expect(rules.map((rule) => rule.id)).toEqual([7, -2, -3, -4])
    expect(new Set(rules.map((rule) => rule.id)).size).toBe(4)
  })

  it('数字 id 允许以字符串形式给出', () => {
    const rules = normalizeLegadoReplaceRules([legadoRule({ name: 'a', id: '12' })]).rules
    expect(rules[0].id).toBe(12)
  })

  it('顶层不是数组时直接报错, 而不是静默导入 0 条', () => {
    for (const raw of [null, undefined, 42, '{"rules":[]}', {}, { data: [legadoRule()] }]) {
      expect(() => normalizeLegadoReplaceRules(raw)).toThrow(/顶层应为数组/)
    }
  })

  it('空数组返回空结果, 由调用方决定提示「没有可导入的规则」', () => {
    expect(normalizeLegadoReplaceRules([])).toEqual({
      rules: [],
      skipped: { total: 0, jsRules: 0, invalid: 0, samples: [] },
      titleOnly: 0,
      scriptRules: 0,
    })
  })

  it('样例名最多 3 条', () => {
    const result = normalizeLegadoReplaceRules(
      // 用 pattern 侧的脚本触发跳过（replacement 侧的脚本现在会正常导入）
      Array.from({ length: 5 }, (_, i) => legadoRule({ name: `脚本${i}`, pattern: '@js:x' })),
    )
    expect(result.skipped.samples).toEqual(['脚本0', '脚本1', '脚本2'])
  })

  it('无效条目没有名字时, 样例回退为条目序号', () => {
    const result = normalizeLegadoReplaceRules([null, 42])
    expect(result.skipped.samples).toEqual(['第 1 条', '第 2 条'])
  })

  it('不修改传入的原始数据', () => {
    const raw = realisticExport()
    const snapshot = JSON.parse(JSON.stringify(raw))
    normalizeLegadoReplaceRules(raw)
    expect(raw).toEqual(snapshot)
  })
})

describe('buildReplaceRuleImportMessage', () => {
  it('全部成功时给出 success 提示', () => {
    const feedback = buildReplaceRuleImportMessage(normalizeLegadoReplaceRules(
      Array.from({ length: 3 }, (_, i) => legadoRule({ name: `规则${i}` })),
    ))
    expect(feedback).toEqual({ text: '导入 3 条', type: 'success' })
  })

  it('有跳过时用 warning, 并带跳过明细与脚本规则提示（实测文案）', () => {
    const feedback = buildReplaceRuleImportMessage(normalizeLegadoReplaceRules(realisticExport()))
    expect(feedback).toEqual({
      text:
        '导入 19 条（跳过 1 条：1 条用脚本生成匹配式的规则；3 条脚本规则在后端沙盒执行；' +
        '1 条只作用于章节标题）',
      type: 'warning',
    })
  })

  it('脚本替换规则与无效条目分别计数', () => {
    const feedback = buildReplaceRuleImportMessage(normalizeLegadoReplaceRules([
      legadoRule({ name: '甲' }),
      legadoRule({ name: '乙' }),
      legadoRule({ name: '丙' }),
      legadoRule({ name: '脚本', replacement: '@js:x' }),
      { name: '坏条目' },
    ]))
    expect(feedback).toEqual({
      text: '导入 4 条（跳过 1 条：1 条无效；1 条脚本规则在后端沙盒执行）',
      type: 'warning',
    })
  })

  it('全部被跳过时是 error, 带明细与样例名（不提示「导入 0 条」）', () => {
    const feedback = buildReplaceRuleImportMessage(normalizeLegadoReplaceRules([
      // pattern 侧是脚本才没有通道; replacement 侧是脚本现在能导入
      legadoRule({ name: '甲', pattern: '@js:x' }),
      legadoRule({ name: '乙', pattern: '@js:y' }),
    ]))
    expect(feedback.type).toBe('error')
    expect(feedback.text).toBe(
      '未导入任何规则（跳过 2 条：2 条用脚本生成匹配式的规则）；例如：甲、乙',
    )
    expect(feedback.text).not.toContain('导入 0 条')
  })

  it('空数组时提示没有可导入的规则', () => {
    const feedback = buildReplaceRuleImportMessage(normalizeLegadoReplaceRules([]))
    expect(feedback).toEqual({
      text: '没有可导入的净化规则（JSON 数组为空）',
      type: 'error',
    })
  })

  it('成功时说明哪些规则只管标题', () => {
    const feedback = buildReplaceRuleImportMessage(normalizeLegadoReplaceRules([
      legadoRule({ name: '仅标题', scopeContent: false, scopeTitle: true }),
      legadoRule({ name: '正文' }),
    ]))
    expect(feedback).toEqual({
      text: '导入 2 条（1 条只作用于章节标题）',
      type: 'success',
    })
  })

  it('仅标题规则的作用目标被保留，不会被改写成正文规则', () => {
    const result = normalizeLegadoReplaceRules([
      legadoRule({ name: '仅标题', scopeContent: false, scopeTitle: true }),
    ])
    expect(result.rules[0].scopeContent).toBe(false)
    expect(result.rules[0].scopeTitle).toBe(true)
  })

  it('缺 scopeContent / scopeTitle 时按手机端默认补齐：正文开、标题关', () => {
    const bare = { name: '老 JSON', pattern: 'x', replacement: '' }
    const result = normalizeLegadoReplaceRules([bare])
    expect(result.rules[0].scopeContent).toBe(true)
    expect(result.rules[0].scopeTitle).toBe(false)
  })
})
