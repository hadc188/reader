//! 替换净化前的「特殊内容保护」。
//!
//! 对齐手机端 `SpecialContentProtector`：净化规则是用户从网上抄来的，其中常见
//! `.*`、`[^\n]+$` 这类宽匹配。如果直接作用在正文 HTML 上，**图片标签会被整段吃掉**，
//! 表现为"开了净化之后插图全没了"，而规则作者本来只想删广告文字。
//!
//! 做法与手机端一致：替换前把这几类片段换成占位符，跑完整条规则链再换回来。
//! - `<img …>`：图片。这是最要紧的一类，规则写 `.*` 就会命中它。
//! - `[newpage]`：分页标记（单独成行），删掉会让分页乱掉。
//! - `<js>…</js>` / `<style>…</style>`：书源正文里内联的脚本与样式。
//! - EPUB 的 native 段标记：`textReader` 直读模式用来标识原书样式段。
//!
//! 占位符用 Unicode 私用区（U+E000/U+E001）包裹，正文里几乎不可能自然出现；
//! 万一用户正文真的含有同名串，`restore` 只按**自己登记过的**占位符还原，
//! 不会误伤原文。

/// 占位符的左右边界字符（私用区，正文里不会自然出现）。
const MARK_LEFT: char = '\u{E000}';
const MARK_RIGHT: char = '\u{E001}';

/// 保护后的正文 + 待还原的片段表。
pub struct ProtectedContent {
    content: String,
    placeholders: Vec<(String, String)>,
}

impl ProtectedContent {
    /// 把占位符换回原片段。
    ///
    /// 用 `replacen` 逐个替换而不是 `String::replace`：用户正文里可能恰好出现
    /// 同样形状的串（极端情况），但占位符是我们自己生成的、每个只出现一次，
    /// 逐个替换能保证既不漏也不重复。
    pub fn restore(&self, value: &str) -> String {
        let mut restored = value.to_string();
        for (placeholder, original) in &self.placeholders {
            if restored.contains(placeholder.as_str()) {
                restored = restored.replacen(placeholder.as_str(), original, 1);
            }
        }
        restored
    }

    /// 保护后（可直接跑替换规则）的正文。
    pub fn content(&self) -> &str {
        &self.content
    }
}

/// 把正文里的特殊片段换成占位符（不做取舍，全部保护）。
///
/// 只给「不知道该不该保护」的场景用（例如没有规则上下文）。真正的执行链走
/// [`protect_for_rules`]，它会先看规则是否明确要处理某类标签。
pub fn protect(content: &str) -> ProtectedContent {
    protect_kinds(content, &ContentKinds::all())
}

/// 需要保护的片段类别。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ContentKinds {
    pub img: bool,
    pub script: bool,
    pub style: bool,
    pub newpage: bool,
}

impl ContentKinds {
    pub fn all() -> Self {
        Self {
            img: true,
            script: true,
            style: true,
            newpage: true,
        }
    }
}

/// 按**规则集**决定保护哪些类别。
///
/// 规则里明确提到某类标签（例如 pattern 写 `<img\b[^>]*>`）时，那条规则的目的
/// 就是处理它 —— 这种情况必须把这一类放开，否则「删除评论图」这类既有规则会
/// 突然全部失效。其余类别一律保护，避免 `.*` 这类宽匹配顺手把图片吃掉。
///
/// 判定只看**是否提到**，不追求精确解析正则：`img` 出现在 pattern 里几乎只会是
/// 「要处理图片」，误判的代价是"这一类退回原来的不保护行为"，不会更糟。
pub fn kinds_for_patterns<'a>(patterns: impl IntoIterator<Item = &'a str>) -> ContentKinds {
    let mut mentions_img = false;
    let mut mentions_script = false;
    let mut mentions_style = false;
    let mut mentions_newpage = false;
    for pattern in patterns {
        let lower = pattern.to_ascii_lowercase();
        if lower.contains("img") {
            mentions_img = true;
        }
        if lower.contains("script") {
            mentions_script = true;
        }
        if lower.contains("style") {
            mentions_style = true;
        }
        if lower.contains("newpage") {
            mentions_newpage = true;
        }
    }
    ContentKinds {
        img: !mentions_img,
        script: !mentions_script,
        style: !mentions_style,
        newpage: !mentions_newpage,
    }
}

/// 执行链用的入口：按规则集决定保护范围。
pub fn protect_for_rules<'a>(
    content: &str,
    patterns: impl IntoIterator<Item = &'a str>,
) -> ProtectedContent {
    protect_kinds(content, &kinds_for_patterns(patterns))
}

/// 把正文里的特殊片段换成占位符。
fn protect_kinds(content: &str, kinds: &ContentKinds) -> ProtectedContent {
    let mut placeholders: Vec<(String, String)> = Vec::new();
    let mut protected = content.to_string();

    for (enabled, open, close) in [
        (kinds.img, "<img", ">"),
        (kinds.script, "<script", "</script>"),
        (kinds.style, "<style", "</style>"),
    ] {
        if enabled {
            protected = protect_tag_blocks(&protected, open, close, &mut placeholders);
        }
    }
    if kinds.newpage {
        protected = protect_newpage(&protected, &mut placeholders);
    }

    ProtectedContent {
        content: protected,
        placeholders,
    }
}

/// 保护 `open … close` 形式的标签块（含跨行）。
///
/// 用「找开标签 → 找对应闭标签」而不是正则：`<img>` 是自闭合的（没有 `</img>`），
/// `<script>` / `<style>` 则可能内部含 `>`，正则容易切歪。
fn protect_tag_blocks(
    content: &str,
    open: &str,
    close: &str,
    placeholders: &mut Vec<(String, String)>,
) -> String {
    let lowercase = content.to_ascii_lowercase();
    let mut out = String::with_capacity(content.len());
    let mut cursor = 0usize;

    while let Some(rel_start) = lowercase[cursor..].find(open) {
        let start = cursor + rel_start;
        // 必须落在标签边界上：`<images>` 不算 `<img>`。
        let after = lowercase.as_bytes().get(start + open.len()).copied();
        let boundary_ok = match after {
            Some(byte) => {
                !byte.is_ascii_alphanumeric() && byte != b'-' && byte != b'_'
            }
            None => false,
        };
        if !boundary_ok {
            out.push_str(&content[cursor..start + open.len()]);
            cursor = start + open.len();
            continue;
        }

        // `<img>` 自闭合：第一个 `>` 就是结尾。
        let end = if open == "<img" {
            lowercase[start..].find('>').map(|idx| start + idx + 1)
        } else {
            lowercase[start..]
                .find(close)
                .map(|idx| start + idx + close.len())
        };
        let Some(end) = end else {
            // 没有闭合：剩下的原样保留，不吞内容。
            break;
        };

        out.push_str(&content[cursor..start]);
        let placeholder = placeholder_for(placeholders.len());
        placeholders.push((placeholder.clone(), content[start..end].to_string()));
        out.push_str(&placeholder);
        cursor = end;
    }

    out.push_str(&content[cursor..]);
    out
}

/// 保护单独成行的 `[newpage]`（手机端只认这种形态，行内出现不算）。
fn protect_newpage(content: &str, placeholders: &mut Vec<(String, String)>) -> String {
    let mut out = String::with_capacity(content.len());
    for (index, line) in content.split('\n').enumerate() {
        if index > 0 {
            out.push('\n');
        }
        // 与手机端 `(?m)^\s*\[newpage]\s*$` 一致：只有整行（允许两侧空白）才算。
        let trimmed = line.trim();
        if trimmed.eq_ignore_ascii_case("[newpage]") {
            let placeholder = placeholder_for(placeholders.len());
            placeholders.push((placeholder.clone(), line.to_string()));
            out.push_str(&placeholder);
        } else {
            out.push_str(line);
        }
    }
    out
}

fn placeholder_for(index: usize) -> String {
    format!("{MARK_LEFT}READER_SPECIAL_{index}{MARK_RIGHT}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn image_tags_survive_a_greedy_rule() {
        let source = "开头<img src=\"a.png\">中间<img src=\"b.png\">结尾";
        let protected = protect(source);
        // 保护后正文里不该再有 `<img`，否则贪婪规则仍会命中标签。
        assert!(!protected.content().contains("<img"));
        // 模拟一条把整行吃掉又吐回来的规则（替换成自身，等价于"规则命中但不该毁掉图片"）。
        let after_rule = protected.content().to_string();
        assert_eq!(protected.restore(&after_rule), source);
    }

    #[test]
    fn newpage_marker_is_preserved() {
        let source = "第一段\n  [newpage]  \n第二段";
        let protected = protect(source);
        assert!(!protected.content().contains("[newpage]"));
        assert_eq!(protected.restore(protected.content()), source);
    }

    #[test]
    fn inline_newpage_is_not_protected() {
        // 行内出现不是分页标记，手机端也不会保护它。
        let source = "这里有 [newpage] 但不在独立行";
        let protected = protect(source);
        assert_eq!(protected.content(), source);
    }

    #[test]
    fn script_and_style_blocks_are_protected() {
        let source = "正文<style>p{color:red}</style>更多<script>var a=1;</script>结束";
        let protected = protect(source);
        assert!(!protected.content().contains("<style"));
        assert!(!protected.content().contains("<script"));
        assert_eq!(protected.restore(protected.content()), source);
    }

    #[test]
    fn unclosed_tag_does_not_swallow_the_rest() {
        // 半截 HTML（书源正文规则切歪时很常见）不能把后面所有正文都吃掉。
        let source = "正文<img src=\"x.png\" 后面还有字";
        let protected = protect(source);
        assert_eq!(protected.content(), source);
    }

    #[test]
    fn similar_tag_names_are_not_treated_as_img() {
        let source = "<images>不是图片标签</images>";
        let protected = protect(source);
        assert!(protected.content().contains("<images>"));
    }

    #[test]
    fn multiple_kinds_of_fragments_keep_their_order() {
        let source = "<img src=\"1.png\">\n[newpage]\n<style>x{}</style>\n尾巴";
        let protected = protect(source);
        assert_eq!(protected.restore(protected.content()), source);
    }

    #[test]
    fn plain_text_is_untouched() {
        let source = "这是一段没有任何特殊标记的正文。";
        let protected = protect(source);
        assert_eq!(protected.content(), source);
        assert_eq!(protected.restore(source), source);
    }

    #[test]
    fn rule_that_mentions_img_keeps_images_exposed() {
        // 「删除评论图」这类规则必须能真的看到 <img>，否则既有规则会全部失效。
        let kinds = kinds_for_patterns(["<img\\b[^>]*>"]);
        assert!(!kinds.img);
        assert!(kinds.newpage);

        let source = "正文<img src=\"a.png\">尾巴";
        let protected = protect_kinds(source, &kinds);
        assert!(protected.content().contains("<img"));
    }

    #[test]
    fn greedy_rule_without_img_mention_still_protects_images() {
        let kinds = kinds_for_patterns([".*"]);
        assert!(kinds.img);
        let source = "广告<img src=\"a.png\">尾巴";
        let protected = protect_kinds(source, &kinds);
        assert!(!protected.content().contains("<img"));
        assert_eq!(protected.restore(protected.content()), source);
    }

    #[test]
    fn kinds_detection_is_case_insensitive_and_covers_all_classes() {
        let kinds = kinds_for_patterns(["<IMG", "<SCRIPT", "<<Style>>", "\\[NEWPAGE\\]"]);
        assert_eq!(
            kinds,
            ContentKinds {
                img: false,
                script: false,
                style: false,
                newpage: false,
            }
        );
    }

    #[test]
    fn empty_rule_set_protects_everything() {
        assert_eq!(kinds_for_patterns(std::iter::empty()), ContentKinds::all());
    }
}
