// 旁白 / 对话切分。
//
// 朗读时「旁白一个音色、对话另一个音色」需要先把一段正文切成旁白与对话。这里
// 刻意做成纯函数（不碰 DOM / IPC / store）：切分规则很容易写错（未闭合、嵌套、
// 多种引号混用、没有引号的说话人冒号），单独可枚举的输入输出才能把这些边界钉死
// 在测试里。
//
// 两条识别途径：
// 1. **成对引号**（`“”` / `「」` / `『』`）—— 最可靠，优先判定；
// 2. **说话人冒号**（`张三：`、`他笑道：`）—— 网文里大量对话不带引号，只能靠冒号。
//    这条是启发式，故意做得保守：只有「短前缀 + 全角冒号」才算说话人，冒号后到
//    句末算对话；`时间：2024年`、`注意：请勿模仿` 这类标签不切（见
//    `NON_SPEAKER_LABELS`）。

export type SpeechSegmentType = 'narration' | 'dialogue'

export interface SpeechSegment {
  type: SpeechSegmentType
  text: string
}

/**
 * 引号开符 → 期望的闭符。
 *
 * 只认中文引号：西文 `"..."` 在中文正文里极少成对出现（半角引号还常被用作
 * 长度单位、英寸、代码片段），按它切分会把旁白整段误判成对话。Legado 的对话
 * 正文用的就是这三种成对引号。
 */
const QUOTE_PAIRS: Record<string, string> = {
  '\u201c': '\u201d', // “ ”
  '\u300c': '\u300d', // 「 」
  '\u300e': '\u300f', // 『 』
}

/**
 * 冒号对话真正的句末标点。
 *
 * 只认句号/问号/叹号：分号与省略号在台词中间很常见（`先别走；我还有话说`、
 * `等等……我还有话说`），把它们当句末会把一句台词从中间劈成「对话 + 旁白」。
 */
const COLON_DIALOGUE_ENDINGS = new Set(['。', '！', '？', '!', '?'])

/** 找「本句前缀」时的断句边界：分号/省略号也算，避免把上一句并进说话人标签。 */
const SENTENCE_BOUNDARIES = new Set(['。', '！', '？', '!', '?', '；', ';', '…'])

/**
 * 说话人前缀上限。
 *
 * `老道士叹了口气，说：` 这类最长的说话人标签是十几个字；放到 20 字仍能覆盖，
 * 又不会把整句话都当成标签（超过这个长度的前缀基本都是普通叙述里的冒号）。
 */
const SPEAKER_PREFIX_MAX_CHARS = 20

/**
 * 冒号前面不该算「说话人」的常见标签。
 *
 * 这些词后面跟冒号通常是解释/列举（`注意：请勿模仿`、`时间：2024年`），用对话
 * 音色读会很怪。词表刻意短：漏掉的宁可当旁白（保守优先），误伤的却是明显不对
 * 的叙述，所以只收高频标签词。
 */
const NON_SPEAKER_LABELS = new Set([
  '时间', '地点', '地址', '网址', '价格', '作者', '简介', '备注', '注意', '说明',
  '规则', '规则如下', '例如', '比如', '如下', '步骤', '提示', '答案', '来源',
  '日期', '电话', '邮箱', '原因', '结果', '条件', '示例', '参考', '目录', '正文',
  '系统', '公告', '通知', '警告', '任务', '奖励', '恭喜',
])

/**
 * 说话动作词：前缀里出现这些，才按「有人在说话」处理。
 *
 * 只靠「短前缀」判定的误伤太广：`他明白了：这一切都是骗局。`、`他只有一个
 * 念头：活下去。` 都会被当成台词。网文里真正的说话标签几乎都带这类动作词。
 * 单字的 `念`/`笑`/`叹` 不收（`念头`、`笑话` 会误伤），只用它们的双字形态。
 */
const SPEECH_MARKERS = [
  '说', '道', '问', '答', '喊', '叫', '吼', '嚷', '骂',
  '嘟囔', '嘀咕', '喃喃', '低语', '自语', '开口', '出声', '应声', '接口',
  '回答', '反问', '追问', '插嘴', '接话', '补充', '强调', '解释', '介绍',
  '招呼', '提醒', '嘱咐', '叮嘱', '吩咐', '命令', '宣布', '报告', '汇报',
  '表示', '回复', '写道', '咬牙', '点头', '摇头',
  '笑道', '冷笑', '嗤笑', '大笑', '苦笑', '微笑', '讥笑', '嘲弄',
  '叹道', '骂道', '吼道', '叫道', '喊道', '问道', '答道', '应道',
  '沉声', '低声', '冷声', '厉声', '柔声', '颤声', '嘶哑', '冷哼', '轻哼',
]

/**
 * 叙述/解释性短语：句子里带这些就不是「谁在说话」。
 *
 * `道` / `说` 这类单字也会出现在 `知道`、`据说` 里，先把这些短语排除掉；
 * 后半段是「短前缀 + 冒号」里常见的解释性名词（`根据规则：`、`他决定：`），
 * 只靠长度限制挡不住。
 */
const NON_SPEAKER_PATTERNS = new RegExp([
  '明白', '知道', '觉得', '清楚', '发现', '想到', '看到', '听到', '意识', '认为',
  '仿佛', '似乎', '原来', '结论', '念头', '道理', '难道', '味道', '街道',
  '根据', '按照', '依据', '规则', '规定', '标准', '要求', '方法', '步骤',
  '原因', '理由', '目的', '目标', '条件', '前提', '关键', '重点', '问题', '答案',
  '意思', '内容', '结果', '后果', '真相', '事实', '秘密', '计划', '办法', '选择',
  '决定', '据说', '听说', '虽说', '也就是说', '可以说', '应该说', '换句话说',
  '简单来说', '具体来说', '总的来说',
  // 系统流/游戏化网文的提示框：`系统提示：`、`任务奖励：` 后面不是台词。
  '系统', '提示', '公告', '警告', '任务', '奖励', '恭喜', '通知',
].join('|'))

/** 不含说话动作词时，只接受 4 字内、且不带叙述虚词的名字/称呼（`张三：`）。 */
const SPEAKER_NAME_MAX_CHARS = 4

/** 名字里几乎不会出现的叙述虚词。 */
const NARRATIVE_PARTICLES = /[了着过得很太更最没不]/

/**
 * 时间/顺序开头的前缀基本不是说话人（`第二天：`、`半小时后：`、`最后：`）。
 *
 * 用整词而不是单字：单字 `明`/`后`/`上` 会把 `明明：`、`后羿：` 这类人名一起
 * 挡掉；整词只命中真正的时间/顺序说法。
 */
const TIME_OR_ORDINAL_PREFIX = /^(今天|明天|昨天|后天|前天|上午|下午|晚上|早上|中午|半夜|凌晨|现在|当时|此时|此刻|次日|翌日|第二天|第二天早上|半小时|一会儿|片刻|随后|然后|接着|最后|首先|其次|同时|另外|总之|\d)/


/**
 * 冒号对话的结束哨兵：栈里出现它表示「等句末标点」，而不是等某个引号闭符。
 *
 * 用空串是因为它是唯一不会与真实闭符冲突的值。
 */
const SENTENCE_SENTINEL = ''

export interface SplitSpeechOptions {
  /**
   * 上一段结束时仍未闭合的引号（跨段对话）。
   *
   * 传期望的闭符（例如 `”`）即表示「本段从对话中间开始」，解析到该闭符为止都是
   * 对话。跨段对话在网文里很常见：上一段只剩一个开引号，下一段才出现闭符。
   */
  openCloser?: string | null
  /** 关闭说话人冒号规则（只想看引号切分时用）。默认开启。 */
  detectSpeakerColon?: boolean
}

export interface SpeechSegmentsResult {
  segments: SpeechSegment[]
  /**
   * 解析到段尾时仍未闭合的引号闭符；`null` 表示段尾不在引号对话里。
   *
   * 冒号切出来的对话只到句末为止，不会跨段，所以句末没落地的冒号对话也返回
   * `null` —— 下一段不该被它带着变成对话。
   */
  openCloser: string | null
}

function isEmptyText(text: string): boolean {
  // trim() 同时吃掉半角与全角空格（U+3000）—— 这正是「只 trim 两端」的语义：
  // 文本内部的缩进/空格保留，两端的排版空白不产出空片段。
  return !text.trim()
}

/**
 * 按中文引号与说话人冒号把正文切成旁白 / 对话片段。
 *
 * 规则：
 * - 引号（含开闭符本身）及其内容算对话，引号外算旁白；
 * - 未闭合引号：从该引号起到文本末尾都算对话（跨段对话的常见形态）；
 * - 同类引号嵌套按深度配对，第一个闭符不结束外层；
 * - 冒号后（`张三：`）到句末算对话；冒号本身留在旁白里，读起来是「旁白报人名、
 *   对话念台词」；冒号后紧跟引号开符时交给引号规则；
 * - 每个片段 trim 两端，空片段丢弃；连续同类片段不合并（保持与原文对应）。
 *
 * 返回顺序与原文一致；没有可切分内容时返回单个 narration 片段。
 */
export function splitSpeechSegments(text: string): SpeechSegment[] {
  return splitSpeechSegmentsDetailed(text).segments
}

/**
 * [`splitSpeechSegments`] 的完整版：额外返回「段尾是否还在引号对话里」。
 *
 * 跨段对话需要调用方把这个状态串到下一段（见 `buildSpeechChunkPieces`）；单段
 * 场景直接用 `splitSpeechSegments` 即可。
 */
export function splitSpeechSegmentsDetailed(
  text: string,
  options: SplitSpeechOptions = {},
): SpeechSegmentsResult {
  if (isEmptyText(text)) {
    return { segments: [], openCloser: options.openCloser ?? null }
  }

  const detectSpeakerColon = options.detectSpeakerColon !== false
  const chars = Array.from(text)
  const segments: SpeechSegment[] = []
  // 当前正在累积的片段类型。切入/切出对话时立刻 flush，保证顺序与原文一致。
  let type: SpeechSegmentType = options.openCloser ? 'dialogue' : 'narration'
  let buffer = ''
  // 嵌套引号的期望闭符栈：栈非空即「仍在对话里」；`SENTENCE_SENTINEL` 代表
  // 「冒号对话，等到句末标点结束」。
  const closerStack: string[] = options.openCloser ? [options.openCloser] : []

  const flush = () => {
    const value = buffer.trim()
    if (value) segments.push({ type, text: value })
    buffer = ''
  }

  for (let index = 0; index < chars.length; index += 1) {
    const char = chars[index]

    if (type === 'narration') {
      const closer = QUOTE_PAIRS[char]
      if (closer) {
        // 引号前若已有旁白，先收尾；开符本身属于对话片段。
        flush()
        type = 'dialogue'
        closerStack.push(closer)
        buffer = char
        continue
      }
      if (detectSpeakerColon && isSpeakerColon(chars, index, buffer)) {
        // 冒号跟在说话人后面：冒号本身留在旁白（「张三：」），台词另起对话段。
        buffer += char
        flush()
        type = 'dialogue'
        closerStack.push(SENTENCE_SENTINEL)
        continue
      }
      buffer += char
      continue
    }

    // ── 对话中 ──
    const top = closerStack.length ? closerStack[closerStack.length - 1] : null

    // 冒号对话：等到真正的句末标点才结束；期间出现的引号仍按引号规则嵌套配对。
    if (top === SENTENCE_SENTINEL) {
      if (COLON_DIALOGUE_ENDINGS.has(char)) {
        closerStack.pop()
        buffer += char
        if (!closerStack.length) {
          flush()
          type = 'narration'
        }
        continue
      }
      const nestedCloser = QUOTE_PAIRS[char]
      if (nestedCloser) closerStack.push(nestedCloser)
      buffer += char
      continue
    }

    // 只有「栈顶期望的闭符」才能收一层；其它闭符当普通字符保留，
    // 否则 `“他说「…」”` 会在内层的 」 处把外层也提前关掉。
    if (top !== null && char === top) {
      closerStack.pop()
      buffer += char
      if (!closerStack.length) {
        flush()
        type = 'narration'
      }
      continue
    }
    const nestedCloser = QUOTE_PAIRS[char]
    if (nestedCloser) {
      // 嵌套引号（同类或异类）一律押栈，等它自己的闭符出现；内容仍是同一段对话。
      closerStack.push(nestedCloser)
    }
    buffer += char
  }

  // 收尾：未闭合引号时 buffer 里就是「引号之后到段尾」的对话；正常结束时
  // 这里是最后一段旁白（或已 flush 过，buffer 为空）。
  flush()

  const top = closerStack.length ? closerStack[closerStack.length - 1] : null
  return {
    segments,
    // 冒号对话（句末哨兵）不跨段，只有真实引号才把状态带给下一段。
    openCloser: top === null || top === SENTENCE_SENTINEL ? null : top,
  }
}

/**
 * 当前位置的冒号是否算「说话人冒号」。
 *
 * 五个条件缺一不可：
 * 1. 全角 `：`（半角会和 `12:30`、`http://` 打架，不认）；
 * 2. 冒号后还有内容，且第一个非空白字符不是引号开符（那种交给引号规则）；
 * 3. 冒号后的第一个字符不是数字（`时间：2024年`、`价格：5元` 这类标签）；
 * 4. 冒号前的「本句前缀」非空、不超过 `SPEAKER_PREFIX_MAX_CHARS`，不在
 *    `NON_SPEAKER_LABELS` 里，也不以时间/序数词开头；
 * 5. 前缀要么带说话动词（`他笑道`），要么是 4 字内的名字/称呼（`张三`）。
 *    只满足「短」是不够的 —— 那样 `他明白了：`、`结论只有一个：` 都会被读成
 *    台词。
 */
function isSpeakerColon(chars: string[], index: number, buffer: string): boolean {
  if (chars[index] !== '：') return false
  const next = nextNonSpaceChar(chars, index + 1)
  if (!next) return false
  if (QUOTE_PAIRS[next]) return false
  if (/[0-9０-９]/.test(next)) return false
  return isSpeakerPrefix(buffer)
}

/** 冒号前最后一个句末标点（或换行）之后的部分，就是这句话的「前缀」。 */
function isSpeakerPrefix(buffer: string): boolean {
  let start = 0
  for (let index = 0; index < buffer.length; index += 1) {
    const char = buffer[index]
    if (SENTENCE_BOUNDARIES.has(char) || char === '\n') start = index + 1
  }
  const prefix = buffer.slice(start).trim()
  if (!prefix || prefix.length > SPEAKER_PREFIX_MAX_CHARS) return false
  if (NON_SPEAKER_LABELS.has(prefix)) return false
  if (TIME_OR_ORDINAL_PREFIX.test(prefix)) return false
  if (NON_SPEAKER_PATTERNS.test(prefix)) return false
  if (SPEECH_MARKERS.some((marker) => prefix.includes(marker))) return true
  // 没有说话动作词时只认极短的人名/称呼，且不能带叙述性虚词。
  return prefix.length <= SPEAKER_NAME_MAX_CHARS && !NARRATIVE_PARTICLES.test(prefix)
}

function nextNonSpaceChar(chars: string[], from: number): string {
  for (let index = from; index < chars.length; index += 1) {
    const char = chars[index]
    if (char !== ' ' && char !== '\t' && char !== '\u3000') return char
  }
  return ''
}
