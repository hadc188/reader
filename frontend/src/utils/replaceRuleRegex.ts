/**
 * Legado(阅读3.0，Java) 净化规则 → JavaScript 正则。
 *
 * 为什么需要这一层：Legado 的 `pattern` 是 **Java 正则**，而这里用 `new RegExp` 执行。
 * 两者语法大同小异，但实测导入的 16 条规则里有 **11 条直接构造失败** —— 全部是
 * Java 专有写法，`new RegExp` 会抛异常；旧实现用 try/catch 把它吞成一行
 * `console.error`，于是用户看到的只是"规则导入了但完全没效果"。
 *
 * 已处理的差异（都是实测数据里真实出现的）：
 * - **内联旗标** `(?i)` / `(?m)` / `(?s)` / `(?x)`：Java 允许写在任意位置；JS 不支持
 *   内联旗标，只能提成整体 flags。绝大多数规则把它写在开头，但也有写在中间的。
 * - **`\h`**：Java 的水平空白，JS 没有。展开成等价的字符类。
 * - **占有量词** `++` / `*+` / `?+` / `{n,m}+`：JS 不支持，退化成贪婪匹配。
 * - **`\R`**：Java 的任意换行，展开成 `(?:\r\n|[\r\n])`。
 *
 * 刻意**不**支持的东西（没有可靠等价物）：`\p{...}`（需要 u 旗标，而 u 会让 `\-`
 * 之类旧写法变语法错误）、原子组 `(?>...)`、`\G`、`\Q...\E`。
 *
 * 这类写法**必须在 [`findUnsupportedSyntax`] 里显式拦下来**。原因：JS 的 `RegExp`
 * 对它们大多不报错，而是把 `\p{L}` 当字面量 `p{L}`、把 `\G` 当 `G` —— 编译"成功"
 * 但永远匹配不到正文，用户只会看到「规则导入了却没效果」。宁可判成「无法执行」。
 */

/** Java 的水平空白，展开成字符类**内容**（不带外层方括号，由调用方按上下文补）。 */
const HORIZONTAL_SPACE = ' \\t\\u00a0\\u1680\\u2000-\\u200a\\u202f\\u205f\\u3000'

/**
 * Java 专有、JS 里没有可靠等价物、必须判成「无法执行」的写法。
 *
 * 这些在 JS 里**不会**抛异常（`\p{L}` 会被当成字面量 `p{L}`、`\G` 当成 `G`、
 * `\Q..\E` 当成字面量 `Q..E`），编译"成功"但永远匹配不到正文，所以只能自己扫。
 */
const UNSUPPORTED_SYNTAX: Array<{ probe: RegExp; message: string }> = [
  { probe: /\\[pP]\{/, message: '不支持 \\p{...} / \\P{...}（Unicode 属性，JS 无等价写法）' },
  { probe: /\(\?>/, message: '不支持原子组 (?>...)' },
  { probe: /\\G/, message: '不支持 \\G（上次匹配结束位置锚点）' },
  { probe: /\\Q|\\E/, message: '不支持 \\Q...\\E（字面量引用段）' },
  { probe: /\\H/, message: '不支持 \\H（水平空白补集，JS 无等价写法）' },
  { probe: /\\[zZ]/, message: '不支持 \\z / \\Z（JS 无等价写法，可用 $ 替代）' },
]

/**
 * 扫出无法降级的 Java 语法；返回 `null` 表示可以降级。
 *
 * 必须在**替换之前**扫原始 pattern：`\h` 变成字符类之后原文就不在了。
 */
export function findUnsupportedSyntax(pattern: string): string | null {
  for (const { probe, message } of UNSUPPORTED_SYNTAX) {
    if (probe.test(pattern)) return message
  }
  return null
}

/** 内联旗标组 `(?i)`、`(?ms)`。带冒号的 `(?i:...)` 是作用域内旗标，不动它。 */
const INLINE_FLAGS = /\(\?([imsx]+)\)/g

/** 占有量词 `++`、`*+`、`?+`、`{2,3}+`。 */
const POSSESSIVE_QUANTIFIER = /([+*?]|\{\d+(?:,\d*)?\})\+/g

/**
 * `\w` 的 Unicode 版本。
 *
 * 这是**必须**对齐的语义差异，不是可选的优化：Java 的 `\w` 默认是 ASCII，而中文规则
 * 几乎都按"`\w` 包含汉字"来写。实测规则 `#07 标点——` 里有一条
 * `[^\n\wꁘ“”【】（）…]{4,}`，本意是"匹配 4 个以上的标点/符号"；若 `\w` 只认 ASCII，
 * 汉字就落在被排除集合里，于是**整段中文正文都会命中**，被替换成「——」。
 * 展开成显式字符类后，行为与规则作者的意图一致（只吃标点，不动汉字）。
 */
const UNICODE_WORD = [
  '0-9A-Za-z_',
  '\\u00C0-\\u024F', // 拉丁扩展
  '\\u0370-\\u03FF', // 希腊
  '\\u0400-\\u04FF', // 西里尔
  '\\u3040-\\u30FF', // 平假名/片假名
  '\\u3400-\\u4DBF', // CJK 扩展 A
  '\\u4E00-\\u9FFF', // CJK 基本区
  '\\uF900-\\uFAFF', // CJK 兼容表意
  '\\uFF10-\\uFF19', // 全角数字
  '\\uFF21-\\uFF3A', // 全角大写
  '\\uFF41-\\uFF5A', // 全角小写
].join('')

/**
 * `\w` 的 Unicode 补集（BMP 范围内）。
 *
 * 为什么 `\W` 也必须展开：规则里常见 `(?<=\n)\W+$`（"整行都是符号"），若 `\W` 仍是
 * JS 默认的 ASCII 语义，汉字会被算作 `\W`，于是整段中文正文都满足条件被删掉 —— 实测
 * `#15 净化词语` 正是这样把正文清空的。
 */
const UNICODE_NON_WORD = [
  '\\u0000-\\u002F',
  '\\u003A-\\u0040',
  '\\u005B-\\u005E',
  '\\u0060',
  '\\u007B-\\u00BF',
  '\\u0250-\\u036F',
  '\\u0500-\\u303F',
  '\\u3100-\\u33FF',
  '\\u4DC0-\\u4DFF',
  '\\uA000-\\uF8FF',
  '\\uFB00-\\uFF0F',
  '\\uFF1A-\\uFF20',
  '\\uFF3B-\\uFF40',
  '\\uFF5B-\\uFFFF',
].join('')

/**
 * 把 `\w` / `\W` / `\h` 展开成 Unicode 字符类。
 *
 * `\h` 必须走这里而不是字符串替换：Java 允许把 `\h` 写在字符类里（`[\h]+`），
 * 而 `HORIZONTAL_SPACE` 自带范围分隔符，直接拼会变成 `[[ ... ]]` —— 一个「包含
 * 方括号的字符类」，能编译但永远匹配不到空白。
 */
function expandShorthandClasses(pattern: string): string {
  let output = ''
  let inClass = false
  for (let index = 0; index < pattern.length; index += 1) {
    const char = pattern[index]
    if (char === '\\') {
      const next = pattern[index + 1]
      if (next === 'w' || next === 'W' || next === 'h') {
        const body = next === 'w'
          ? UNICODE_WORD
          : next === 'W'
            ? UNICODE_NON_WORD
            : HORIZONTAL_SPACE
        output += inClass ? body : `[${body}]`
        index += 1
        continue
      }
      // 其它转义原样保留（`\\w` 这种连续反斜杠也会走到这里，保持原意）。
      output += char + (next ?? '')
      index += 1
      continue
    }
    if (char === '[' && !inClass) inClass = true
    else if (char === ']' && inClass) inClass = false
    output += char
  }
  return output
}

export type CompiledReplaceRule =
  | { kind: 'regex'; regex: RegExp; replacement: string }
  | { kind: 'literal'; needle: string; replacement: string }
  /**
   * `@js:` 脚本替换：不能在前端执行（会拿到 DOM 与 IPC），要交给后端 rquickjs 沙盒。
   * 这里只做 Java→JS 正则转换，把可送后端的三元组准备好。
   */
  | { kind: 'script'; source: string; flags: string; code: string }
  | { kind: 'error'; error: string }

/**
 * 把一条规则编译成可执行形态。
 *
 * 基础 flags 保持 `gm`：老实现就是 `new RegExp(pattern, 'gm')`，换成别的会让用户
 * 现有规则（尤其依赖 `^`/`$` 按行锚定的）行为突变。这里只在此基础上追加规则自己
 * 声明的 `i` / `s`。
 */
export function compileReplaceRule(
  pattern: string,
  isRegex: boolean,
  replacement: string,
): CompiledReplaceRule {
  // `@js:` 是 Legado 的脚本替换：脚本以 `result` 为输入、返回值为替换结果。
  // 桌面版在 Rust 沙盒里执行（见 src/parser/replace_rule_js.rs），这里只负责把
  // 正则部分降级成 JS 语法，脚本原文保持不动。
  if (/^@js:/i.test(replacement.trim())) {
    const converted = convertJavaRegexToJs(pattern)
    if (converted.error) return { kind: 'error', error: converted.error }
    return {
      kind: 'script',
      source: converted.source,
      flags: converted.flags,
      code: replacement.trim().slice('@js:'.length),
    }
  }

  if (!isRegex) {
    // 字面量替换：`String.replaceAll(str, str)` 仍会解释 `$&`/`$1`，而"字面量"语义
    // 要求原样插入，所以把 `$` 转义掉。
    // 注意必须用**函数形式**：`replaceAll('$', '$$')` 里的 `$$` 自身会被当成转义，
    // 等于什么都没做（实测结果仍是 `$100`）。
    return {
      kind: 'literal',
      needle: pattern,
      replacement: replacement.replaceAll('$', () => '$$'),
    }
  }

  const converted = convertJavaRegexToJs(pattern)
  if (converted.error) return { kind: 'error', error: converted.error }
  try {
    const regex = new RegExp(converted.source, converted.flags)
    return {
      kind: 'regex',
      regex,
      replacement: toJsReplacement(replacement, captureGroups(converted.source, converted.flags)),
    }
  } catch (error) {
    return {
      kind: 'error',
      error: (error as Error).message.replace(/^Invalid regular expression:\s*/, ''),
    }
  }
}

/** 正则里可用的捕获组：编号 1..count，外加命名分组名。 */
export interface ReplacementGroups {
  count: number
  names: Set<string>
}

/**
 * 数出正则里的捕获组，供替换串解析用。
 *
 * 为什么不在源码上数左括号：`\(`、`(?:`、`(?=`、字符类里的 `(` 都不是捕获组，
 * 手写扫描器很容易数错。这里用引擎自己报数 —— `(?:源码)|` 一定能匹配空串，
 * 匹配结果数组的长度减一就是捕获组个数，`groups` 的键就是命名分组。
 * `g` / `y` 必须去掉：带 `g` 的 `exec` 会记忆 `lastIndex`，第二次调用结果就错了。
 */
export function captureGroups(source: string, flags: string): ReplacementGroups {
  const probe = new RegExp(`(?:${source})|`, flags.replace(/[gy]/g, ''))
  const match = probe.exec('')
  return {
    count: match ? match.length - 1 : 0,
    names: new Set(match?.groups ? Object.keys(match.groups) : []),
  }
}

function isAsciiDigit(char: string | undefined): boolean {
  return char !== undefined && char >= '0' && char <= '9'
}

/** 替换串里的字面量：JS 只把 `$` 当特殊字符，`\` 原样输出。 */
function literalText(text: string): string {
  return text.replaceAll('$', () => '$$')
}

/** 拆出 Java 正则里的内联旗标并做语法降级。导出以便单测直接断言转换结果。 */
export interface ConvertedJavaRegex {
  source: string
  flags: string
  /** 无法降级时为中文原因；此时 `source` 不可用，调用方必须当成「无法执行」。 */
  error: string | null
}

export function convertJavaRegexToJs(pattern: string): ConvertedJavaRegex {
  // 先在原文上扫不支持的语法：`\h` 等替换之后原文就没了。
  const unsupported = findUnsupportedSyntax(pattern)
  if (unsupported) return { source: pattern, flags: 'gm', error: unsupported }

  let flags = 'gm'
  let source = pattern.replace(INLINE_FLAGS, (_match, letters: string) => {
    for (const letter of letters) {
      // x(verbose) 在 JS 没有等价物：只能丢弃，否则整条规则直接不可用。
      if ('is'.includes(letter) && !flags.includes(letter)) flags += letter
    }
    return ''
  })

  source = source.replace(POSSESSIVE_QUANTIFIER, '$1')
  source = source.replaceAll('\\R', '(?:\\r\\n|[\\r\\n])')
  source = expandShorthandClasses(source)

  return { source, flags, error: null }
}

/**
 * 把 Legado（Java）的**替换串**翻译成 JS 的 `String.replace` 替换串。
 *
 * 手机端走的是 `Matcher.appendReplacement(buf, replacement)`，规则里「替换为」那段
 * 是 Java 的替换模板，不是纯文本；桌面版原先几乎原样丢给 `String.replace`，于是
 * 两边行为分叉（issue #33 第 2 条：`$1`/`${name}`/`\$` 全都不认）。
 *
 * 逐条对齐 Java 语义（已用 JDK 1.8 实测）：
 * - `$n`（1..99）：第 n 个捕获组。两位数超范围时**退一位**：只有 12 个组时
 *   `$99` → 组 9 + 字面量 `9`，`$10` → 组 10，`$123` → 组 12 + `3`。
 *   `$0` / `$00` 是整个匹配。组号超过实际组数 → Java 抛
 *   `IndexOutOfBoundsException: No group N`，这里同样判成「无法执行」。
 * - `${name}`：具名分组。JS 的写法是 `$<name>`，这里翻译过去；没有这个组名则报错
 *   （Java 也是 `No group with name`）。
 * - `\x`：转义，输出字面量 `x`。所以 `\1` 在手机端得到的是字符 `1`，**不是**分组
 *   引用；`\$` 得到字面量 `$`，`\\` 得到 `\`。结尾单个 `\` 在 Java 里直接报错。
 * - 其余 `$`（`$&`、`$$`、行尾 `$`、`$<x>`）在 Java 里都是 `Illegal group reference`，
 *   这里一律判成「无法执行」，免得静默插入一段错误的正文。
 *
 * 输出侧还要防两件事：
 * - JS 的 `$` 本身是特殊字符，字面量 `$` 必须写成 `$$`；
 * - 组号后面紧跟数字会被 JS 吃成两位数分组（`$1` + `0` 变成组 10）。1..9 号组
 *   后面接数字时改用 `$0n` 写法，JS 与 Java 都按「第 n 组」解释。
 */
export function toJsReplacement(replacement: string, groups: ReplacementGroups): string {
  let output = ''
  for (let index = 0; index < replacement.length; index += 1) {
    const char = replacement[index]
    if (char === '\\') {
      const escaped = replacement[index + 1]
      if (escaped === undefined) {
        throw new Error('替换为以单个 \\ 结尾，Java 端会报错；字面量反斜杠要写成 \\\\')
      }
      output += literalText(escaped)
      index += 1
      continue
    }
    if (char !== '$') {
      output += literalText(char)
      continue
    }

    const next = replacement[index + 1]
    if (next === '{') {
      const end = replacement.indexOf('}', index + 2)
      if (end < 0) throw new Error('替换为里的 ${ 没有配对的 }')
      const name = replacement.slice(index + 2, end)
      if (!groups.names.has(name)) {
        throw new Error(`替换为引用了不存在的命名分组 \${${name}}`)
      }
      output += `$<${name}>`
      index = end
      continue
    }
    if (!isAsciiDigit(next)) {
      throw new Error(
        `替换为里的 $ 后面不是分组编号（第 ${index + 1} 个字符）；`
        + '引用捕获组写 $1，字面量 $ 要写成 \\$',
      )
    }

    let end = index + 1
    while (isAsciiDigit(replacement[end])) end += 1
    // 复刻 JDK `Matcher.appendReplacement` 的解析：从第一位起累加，
    // 一旦超过捕获组数量就停在上一轮的数字上，剩下的位当字面量。
    // 于是 `$99`（12 个组）→ 组 9 + `9`，`$0005`（5 个组）→ 组 5，`$009` → 组 0 + `9`。
    let digits = replacement.slice(index + 1, index + 2)
    let cursor = index + 2
    while (cursor < end) {
      const candidate = Number(digits + replacement[cursor])
      if (candidate > groups.count) break
      digits = digits + replacement[cursor]
      cursor += 1
    }
    const group = Number(digits)
    if (group > groups.count) {
      throw new Error(
        `替换为引用了不存在的分组 $${digits}（本规则只有 ${groups.count} 个捕获组）`,
      )
    }

    // 没被吃进组号的数字是字面量（`$99` → 组 9 + `9`）。
    const rest = replacement.slice(cursor, end)
    if (group === 0) {
      output += '$&'
      output += literalText(rest)
    } else {
      // 后面紧跟数字时必须写 `$0n`，否则 JS 会把它吃成两位数分组。
      const following = rest[0] ?? replacement[end]
      output += group < 10 && isAsciiDigit(following) ? `$0${group}` : `$${group}`
      output += literalText(rest)
    }
    index = end - 1
  }
  return output
}

/** 送后端沙盒的一步：字符串替换在沙盒里也走 `String.replace`，所以统一用正则表达。 */
export interface SandboxRuleStep {
  pattern: string
  flags: string
  replacement: string
  isScript: boolean
}

export type SandboxStepResult = { ok: true; step: SandboxRuleStep } | { ok: false; error: string }

/** 把字面量串转成等价正则源，让后端可以统一用 `RegExp` 处理。 */
function escapeRegExpSource(text: string): string {
  return text.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')
}

/**
 * 把一条规则编译成"可以整批发给 Rust 沙盒"的形式。
 *
 * 之所以不直接复用 `compileReplaceRule` 的结果：后端需要的是 `RegExp` 能直接吃的
 * pattern/flags，而前端 `literal` 分支返回的是原始 needle（`replaceAll` 用）。这里把
 * 字面量转义成等价正则，保证两条路径对同一条规则的处理结果一致。
 */
export function toSandboxRuleStep(
  pattern: string,
  isRegex: boolean,
  replacement: string,
): SandboxStepResult {
  if (/^@js:/i.test(replacement.trim())) {
    const converted = convertJavaRegexToJs(pattern)
    if (converted.error) return { ok: false, error: converted.error }
    try {
      new RegExp(converted.source, converted.flags)
    } catch (error) {
      return { ok: false, error: (error as Error).message }
    }
    return {
      ok: true,
      step: {
        pattern: converted.source,
        flags: converted.flags,
        replacement: replacement.trim().slice('@js:'.length),
        isScript: true,
      },
    }
  }

  if (!isRegex) {
    return {
      ok: true,
      step: {
        pattern: escapeRegExpSource(pattern),
        flags: 'g',
        // 同 `compileReplaceRule`：必须用函数形式转义，否则 `$$` 会被再解释一次。
        replacement: replacement.replaceAll('$', () => '$$'),
        isScript: false,
      },
    }
  }

  const converted = convertJavaRegexToJs(pattern)
  if (converted.error) return { ok: false, error: converted.error }
  try {
    new RegExp(converted.source, converted.flags)
  } catch (error) {
    return { ok: false, error: (error as Error).message }
  }
  let stepReplacement: string
  try {
    stepReplacement = toJsReplacement(replacement, captureGroups(converted.source, converted.flags))
  } catch (error) {
    return { ok: false, error: (error as Error).message }
  }
  return {
    ok: true,
    step: {
      pattern: converted.source,
      flags: converted.flags,
      replacement: stepReplacement,
      isScript: false,
    },
  }
}
