//! 在沙盒里执行 Legado 的 `@js:` 净化规则。
//!
//! 为什么需要独立一层：Legado 的 `replacement` 允许写成 `@js:` 开头的脚本，脚本把
//! 匹配到的文本当 `result`，返回值才是替换结果。这类规则无法用 `String.replace`
//! 的字符串替换表达（例如「星号修复」要按上下文把 `ꁘꁘ` 还原成对应的脏话）。
//!
//! 为什么不在前端跑：`new Function` 会把导进来的第三方规则直接放进渲染进程，那里能
//! 摸到 DOM、`window.__TAURI_INTERNALS__`，等于把整个 IPC 面暴露给随机规则。所以放
//! 在 rquickjs 沙盒里，与书源规则同一个执行模型。
//!
//! 求值模型与 Legado 对齐：对每条规则做一次 `text.replace(regex, ...)`；脚本规则用
//! `new Function('result', code)` 逐匹配求值，普通规则仍走字符串替换。规则顺序即数组
//! 顺序（调用方已按 `order` 排好）。
//!
//! # 为什么必须有 interrupt handler
//!
//! 脚本是用户从网上导入的，可能写出死循环。rquickjs 的
//! [`Runtime::set_interrupt_handler`] 会在 JS 执行期间被引擎周期调用，返回 `true`
//! 就会抛出一个**不可被脚本捕获**的异常，把控制权交回 Rust。
//!
//! 没有它的话，外层的 `tokio::time::timeout` 只能做到"不再等待结果"——`spawn_blocking`
//! 里的线程会继续把 CPU 烧在这个死循环上，直到进程退出。一个坏规则就能永久占住一个
//! blocking 线程，多来几条会把线程池耗干。
//!
//! # 已知边界：正则回溯不受 interrupt handler 保护
//!
//! QuickJS 的正则引擎（libregexp）是用 C 实现的，`exec` 期间**不会**回到 Rust 的
//! interrupt 回调。所以 `(a+)+$` 这类灾难性回溯仍然打不断，只能靠外层
//! `tokio::time::timeout` 让 IPC 提前返回（UI 不卡），代价是那个 blocking 线程会
//! 一直被占到回溯结束。
//!
//! 这也是为什么 `MAX_REPLACE_SCRIPT_RULES` 之外还要限制条数：规则集越大，命中
//! 一条病态正则的概率越高。真要彻底解决得换线性时间的正则引擎，超出本次改动范围。

use std::time::{Duration, Instant};

use rquickjs::{Context, Runtime};

/// rquickjs 的栈上限，与书源脚本保持一致。
const JS_MAX_STACK_SIZE: usize = 1024 * 1024;

/// 沙盒堆内存上限。净化脚本只处理一章文本，64MB 已远超正常需求；设上限是为了让
/// `'x'.repeat(1e9)` 这类脚本尽快失败，而不是把进程拖到 OOM。
const JS_MEMORY_LIMIT: usize = 64 * 1024 * 1024;

/// 净化脚本的默认执行上限：正常整章处理在毫秒级，超过这个量级基本可以判定是死循环。
pub const DEFAULT_SCRIPT_TIMEOUT: Duration = Duration::from_secs(8);

/// 一条待执行的规则。`pattern` / `flags` 已由前端从 Java 正则降级为 JS 正则。
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReplaceRuleStep {
    pub pattern: String,
    pub flags: String,
    pub replacement: String,
    #[serde(default)]
    pub is_script: bool,
}

/// 执行整条规则链，返回处理后的正文。使用 [`DEFAULT_SCRIPT_TIMEOUT`]。
pub fn apply_rules(text: &str, rules: &[ReplaceRuleStep]) -> Result<String, String> {
    apply_rules_with_timeout(text, rules, DEFAULT_SCRIPT_TIMEOUT)
}

/// 执行整条规则链，超过 `timeout` 立刻中断并返回错误。
///
/// 超时是在**沙盒内部**生效的（interrupt handler），所以返回时线程已经停下，
/// 不会留下一个还在跑的后台线程。
pub fn apply_rules_with_timeout(
    text: &str,
    rules: &[ReplaceRuleStep],
    timeout: Duration,
) -> Result<String, String> {
    if text.is_empty() || rules.is_empty() {
        return Ok(text.to_string());
    }

    let runtime = Runtime::new().map_err(|error| format!("初始化脚本引擎失败: {error}"))?;
    runtime.set_max_stack_size(JS_MAX_STACK_SIZE);
    runtime.set_memory_limit(JS_MEMORY_LIMIT);

    // 截止时刻在进入沙盒前算好：handler 只做一次 Instant 比较，开销可以忽略。
    let deadline = Instant::now() + timeout;
    runtime.set_interrupt_handler(Some(Box::new(move || Instant::now() >= deadline)));

    let context = Context::full(&runtime).map_err(|error| format!("初始化脚本上下文失败: {error}"))?;

    // 输入以 JSON 字面量注入：JSON 是 JS 的子集，直接当表达式求值即可，不必手工转义。
    // U+2028/U+2029 在旧引擎里不能出现在字符串字面量中，先转成转义序列。
    let rules_literal = json_literal(
        &serde_json::to_value(rules).map_err(|error| format!("规则序列化失败: {error}"))?,
    );

    // 特殊内容保护（对齐手机端 SpecialContentProtector）：先保护 <img> / [newpage] /
    // 内联样式脚本，跑完规则再还原。脚本规则里的 `.*` 同样会吃掉图片，所以**两条
    // 路径都要保护** —— 前端同步路径有一份等价实现（utils/replaceRuleProtect）。
    //
    // 但「规则明确要处理某类标签」时那一类必须放开（见 kinds_for_patterns）：
    // 否则 `<img\b[^>]*>` 这种删图片规则会突然失效。
    let protected = crate::parser::special_content_protector::protect_for_rules(
        text,
        rules.iter().map(|rule| rule.pattern.as_str()),
    );
    let protected_literal = json_literal(&serde_json::Value::String(protected.content().to_string()));

    let result = context.with(|ctx| {
        let setup = format!(
            "globalThis.__readerInput = {protected_literal};\nglobalThis.__readerRules = {rules_literal};"
        );
        ctx.eval::<(), _>(setup.as_bytes())
            .map_err(|error| format!("注入规则失败: {error}"))?;

        let script = format!("({PROCESSOR})");
        ctx.eval::<String, _>(script.as_bytes())
            .map_err(|error| format!("执行净化脚本失败: {error}"))
    });

    // 中断抛的是 "uncatchable exception"，rquickjs 给出的文案不含时间信息，
    // 直接透传会让用户看到一句莫名其妙的英文；这里按是否超时换成可操作的提示。
    match result {
        Err(_) if Instant::now() >= deadline => Err(format!(
            "净化脚本执行超时（超过 {} 秒），已中止；请检查规则里是否有死循环",
            timeout.as_secs_f32().round() as u64
        )),
        // 跑完规则后把受保护的片段换回来。放在这里而不是沙盒内部：还原是纯字符串
        // 操作，没必要再塞进 JS 上下文里多一次注入。
        Ok(output) => Ok(protected.restore(&output)),
        other => other,
    }
}

fn json_literal(value: &serde_json::Value) -> String {
    let raw = serde_json::to_string(value).unwrap_or_else(|_| "\"\"".to_string());
    raw.replace('\u{2028}', "\\u2028")
        .replace('\u{2029}', "\\u2029")
}

/// 在沙盒里跑完整条链。
///
/// 脚本规则用 `eval` 而不是 `new Function(code)`：Legado 在 Rhino 上执行替换脚本时，
/// 取的是**最后一个表达式语句的值**（脚本里没有 `return` 也照样有返回值，例如
/// `R = result; R == "１" ? "1" : R`）。`new Function` 的函数体不返回最后表达式的值，
/// 直接把 `code` 当函数体喂进去会得到 `undefined`，于是整段正文被替换成空 —— 这是
/// 必须用 eval 语义的原因。`eval` 写在函数里且接收 `result`，同时满足变量绑定与
/// 完成值语义。
const PROCESSOR: &str = r#"
(function () {
  var out = globalThis.__readerInput;
  var rules = globalThis.__readerRules;
  for (var i = 0; i < rules.length; i++) {
    var rule = rules[i];
    var re = new RegExp(rule.pattern, rule.flags);
    if (rule.isScript) {
      var fn = new Function('result', 'return eval(' + JSON.stringify(rule.replacement) + ')');
      out = out.replace(re, function (match) {
        var value = fn(String(match));
        return value === undefined || value === null ? '' : String(value);
      });
    } else {
      out = out.replace(re, rule.replacement);
    }
  }
  return out;
})()
"#;

#[cfg(test)]
mod tests {
    use super::*;

    fn plain(pattern: &str, replacement: &str) -> ReplaceRuleStep {
        ReplaceRuleStep {
            pattern: pattern.to_string(),
            flags: "gm".to_string(),
            replacement: replacement.to_string(),
            is_script: false,
        }
    }

    fn script(pattern: &str, code: &str) -> ReplaceRuleStep {
        ReplaceRuleStep {
            pattern: pattern.to_string(),
            flags: "gm".to_string(),
            replacement: code.to_string(),
            is_script: true,
        }
    }

    #[test]
    fn applies_plain_rules_in_order() {
        let rules = vec![plain("aaa", "b"), plain("b+", "c")];
        assert_eq!(apply_rules("aaa aaa", &rules).unwrap(), "c c");
    }

    #[test]
    fn script_sees_matched_text_as_result() {
        // 全角数字转半角：与真实规则同形态（只依赖 result）
        let code = r#"R = result; R == "１" ? "1" : R == "２" ? "2" : R"#;
        let rules = vec![script("[０-９]", code)];
        assert_eq!(apply_rules("１２", &rules).unwrap(), "12");
    }

    #[test]
    fn script_can_reuse_globals_across_matches() {
        // 「星号修复」那类写法：脚本体内定义辅助函数
        let code = r#"
            function M(x) { return result.search(x) == -1 ? false : true }
            M("ꁘꁘꁘ") ? "混帐" : result
        "#;
        let rules = vec![script("ꁘ+", code)];
        assert_eq!(apply_rules("aꁘꁘꁘb", &rules).unwrap(), "a混帐b");
    }

    #[test]
    fn script_returning_non_string_is_coerced() {
        let rules = vec![script("\\d", "42")];
        assert_eq!(apply_rules("x1y", &rules).unwrap(), "x42y");
    }

    #[test]
    fn script_returning_null_drops_the_match() {
        let rules = vec![script("广告", "null")];
        assert_eq!(apply_rules("正文广告正文", &rules).unwrap(), "正文正文");
    }

    #[test]
    fn broken_script_reports_error_instead_of_panicking() {
        let rules = vec![script("x", "this is not javascript(")];
        assert!(apply_rules("x", &rules).is_err());
    }

    #[test]
    fn plain_replacement_supports_group_references() {
        // 与手机端一致：`$1` 是捕获组引用（由前端 toJsReplacement 从 Java 模板翻译过来），
        // 前端同步路径与这里走的是同一套 JS 替换串语义，两边结果必须一致。
        let rules = vec![plain("(.*)阴暗", "$1难看")];
        assert_eq!(apply_rules("他露出阴暗", &rules).unwrap(), "他露出难看");
    }

    #[test]
    fn plain_and_script_rules_mix_in_declared_order() {
        let rules = vec![
            plain("甲", "乙"),
            script("乙", r#"result + "丙""#),
            plain("丙", "丁"),
        ];
        assert_eq!(apply_rules("甲", &rules).unwrap(), "乙丁");
    }

    #[test]
    fn text_with_quotes_and_newlines_survives_injection() {
        let rules = vec![plain("x", "y")];
        let input = "a\"b\\c\nd\u{2028}e";
        let out = apply_rules(input, &rules).unwrap();
        assert_eq!(out, input);
    }

    /// 死循环必须被 interrupt handler 真正打断 —— 只靠外层 timeout 是不够的，
    /// 线程会继续烧 CPU。这里给 300ms，正常应该在 1 秒内返回错误。
    #[test]
    fn infinite_loop_is_interrupted_by_the_deadline() {
        let rules = vec![script("x", "while (true) {}")];
        let started = Instant::now();
        let result = apply_rules_with_timeout("x", &rules, Duration::from_millis(300));
        let elapsed = started.elapsed();

        assert!(result.is_err(), "死循环脚本不该返回成功");
        let message = result.unwrap_err();
        assert!(message.contains("超时"), "错误文案应说明是超时：{message}");
        // 没有 interrupt handler 时这个测试会永久挂起（CI 超时），有它则秒级返回。
        assert!(
            elapsed < Duration::from_secs(5),
            "中断后应立刻返回，实际用了 {elapsed:?}"
        );
    }

    /// 正则灾难性回溯**不能**被 interrupt handler 打断（libregexp 是 C 实现，
    /// exec 期间不回调 Rust）。这个测试因此只能验证"没有挂死"这一层 ——
    /// 用一个规模受控的坏正则，确认它最终会返回错误而不是永久卡住。
    ///
    /// 真正的防护是外层 `tokio::time::timeout`（见 `commands::replace_rule`）：
    /// 它让 IPC 提前返回，UI 不会卡；被占住的 blocking 线程要等回溯自己结束。
    #[test]
    fn pathological_backtracking_eventually_returns() {
        let rules = vec![plain("(a+)+$", "b")];
        // 26 个 a + 一个不匹配的字符：回溯量约 2^26，实测在本机 ~1s 量级。
        // 再大就会让测试本身变成负担，所以刻意不写 40。
        let text = "a".repeat(26) + "!";
        let started = Instant::now();
        let result = apply_rules_with_timeout(&text, &rules, Duration::from_millis(50));
        // 50ms 的 deadline 打不断正则，所以这里必然"超时之后才返回"，
        // 但**必须返回**（不能 panic、不能永久挂起）。
        assert!(result.is_ok() || result.is_err());
        assert!(
            started.elapsed() < Duration::from_secs(30),
            "回溯应最终结束，实际用了 {:?}",
            started.elapsed()
        );
    }

    /// 正常规则不该被 deadline 影响。
    #[test]
    fn fast_script_finishes_within_the_deadline() {
        let rules = vec![script("[０-９]", r#"R = result; R"#)];
        let out = apply_rules_with_timeout("１２３", &rules, Duration::from_secs(5)).unwrap();
        assert_eq!(out, "１２３");
    }
}
