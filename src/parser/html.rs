use scraper::{ElementRef, Html, Selector};
use std::collections::HashSet;

use crate::parser::rule_analyzer::split_top_level;

#[derive(Clone, Debug, PartialEq)]
enum SelectorBase {
    Css(String),
    Children,
    Text(String),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum IndexMode {
    Select,
    Exclude,
}

#[derive(Clone, Debug, PartialEq)]
enum IndexItem {
    Single(i32),
    Range {
        start: Option<i32>,
        end: Option<i32>,
        step: i32,
    },
}

#[derive(Clone, Debug, PartialEq)]
struct ParsedSelector {
    base: SelectorBase,
    explicit_index: bool,
    index_mode: IndexMode,
    index_items: Vec<IndexItem>,
}

pub fn parse_document(html: &str) -> Html {
    Html::parse_document(html)
}

/// Convert Legado selector format to CSS selector
/// Legado formats:
/// - class.xxx yyy zzz → .xxx.yyy.zzz (multiple classes on one element)
/// - class.xxx or .xxx → .xxx
/// - tag.xxx → xxx (tag name)
/// - id.xxx or #xxx → #xxx
/// - tag.xxx@tag.yyy → nested selectors (split by @)
fn legado_to_css(selector: &str) -> String {
    let selector = selector.trim();

    // Handle special "class." prefix - multiple classes separated by space
    if selector.starts_with("class.") {
        let rest = &selector[6..];
        let classes: Vec<&str> = rest.split_whitespace().collect();
        if classes.len() > 1 {
            return format!(".{}", classes.join("."));
        } else {
            return format!(".{}", rest.trim());
        }
    }

    // Handle id. prefix
    if selector.starts_with("id.") {
        return format!("#{}", &selector[3..]);
    }

    // Handle tag. prefix
    if selector.starts_with("tag.") {
        return selector[4..].to_string();
    }

    // Already CSS selector format (index will be stripped in parse_selector_with_index)
    if selector.starts_with('.') {
        return selector.to_string();
    }

    if selector.starts_with('#') || selector.starts_with('[') {
        return selector.to_string();
    }

    // Default: treat as class if it doesn't look like a tag
    if selector
        .chars()
        .next()
        .map(|c| c.is_alphabetic())
        .unwrap_or(false)
    {
        let parts: Vec<&str> = selector.split_whitespace().collect();
        if parts.len() > 1 {
            return format!(".{}", parts.join("."));
        }
        return selector.to_string();
    }

    selector.to_string()
}

fn parse_selector_with_index(selector: &str) -> ParsedSelector {
    let selector = selector.trim();

    if let Some((base, index_mode, index_items)) = parse_bracket_index_spec(selector) {
        return ParsedSelector {
            base: parse_selector_base(base),
            explicit_index: true,
            index_mode,
            index_items,
        };
    }

    if let Some((base, index_mode, index_items)) = parse_legacy_index_spec(selector) {
        return ParsedSelector {
            base: parse_selector_base(base),
            explicit_index: true,
            index_mode,
            index_items,
        };
    }

    ParsedSelector {
        base: parse_selector_base(selector),
        explicit_index: false,
        index_mode: IndexMode::Select,
        index_items: Vec::new(),
    }
}

fn parse_selector_base(selector: &str) -> SelectorBase {
    let selector = selector.trim();
    if selector.is_empty() || selector == "children" {
        return SelectorBase::Children;
    }
    if let Some(text) = selector.strip_prefix("text.") {
        return SelectorBase::Text(text.trim().to_string());
    }
    SelectorBase::Css(legado_to_css(selector))
}

fn parse_bracket_index_spec(selector: &str) -> Option<(&str, IndexMode, Vec<IndexItem>)> {
    let selector = selector.trim();
    if !selector.ends_with(']') {
        return None;
    }
    let start = selector.rfind('[')?;
    let base = selector[..start].trim();
    let mut inner = selector[start + 1..selector.len() - 1].trim();
    let index_mode = if let Some(rest) = inner.strip_prefix('!') {
        inner = rest.trim();
        IndexMode::Exclude
    } else {
        IndexMode::Select
    };

    let mut items = Vec::new();
    if !inner.is_empty() {
        for part in inner.split(',') {
            let part = part.trim();
            if part.is_empty() {
                continue;
            }
            if let Some(item) = parse_bracket_index_item(part) {
                items.push(item);
            } else {
                return None;
            }
        }
    }

    Some((base, index_mode, items))
}

fn parse_bracket_index_item(part: &str) -> Option<IndexItem> {
    if part.contains(':') {
        let segments: Vec<&str> = part.split(':').collect();
        if segments.len() < 2 || segments.len() > 3 {
            return None;
        }
        let start = parse_optional_i32(segments[0])?;
        let end = parse_optional_i32(segments[1])?;
        let step = match segments.get(2) {
            Some(value) => parse_optional_i32(value)?.unwrap_or(1),
            None => 1,
        };
        return Some(IndexItem::Range { start, end, step });
    }

    Some(IndexItem::Single(part.parse().ok()?))
}

fn parse_optional_i32(part: &str) -> Option<Option<i32>> {
    let part = part.trim();
    if part.is_empty() {
        return Some(None);
    }
    Some(Some(part.parse().ok()?))
}

fn parse_legacy_index_spec(selector: &str) -> Option<(&str, IndexMode, Vec<IndexItem>)> {
    for (delimiter, index_mode) in [('!', IndexMode::Exclude), ('.', IndexMode::Select)] {
        let Some(pos) = selector.rfind(delimiter) else {
            continue;
        };
        let base = selector[..pos].trim();
        let tail = selector[pos + 1..].trim();
        if tail.is_empty() {
            continue;
        }

        let mut items = Vec::new();
        for part in tail.split(':') {
            let part = part.trim();
            if part.is_empty() {
                return None;
            }
            items.push(IndexItem::Single(part.parse().ok()?));
        }
        return Some((base, index_mode, items));
    }
    None
}

fn collect_matches<'a>(doc: &'a Html, selector: &ParsedSelector) -> Vec<ElementRef<'a>> {
    let matches = match &selector.base {
        SelectorBase::Css(css_selector) => select_css(doc, css_selector),
        SelectorBase::Children => Vec::new(),
        SelectorBase::Text(text) => select_by_text_doc(doc, text),
    };
    apply_indices(matches, selector)
}

fn collect_matches_from_element<'a>(
    el: ElementRef<'a>,
    selector: &ParsedSelector,
) -> Vec<ElementRef<'a>> {
    let matches = match &selector.base {
        SelectorBase::Css(css_selector) => select_css_from_element(el, css_selector),
        SelectorBase::Children => child_elements(el),
        SelectorBase::Text(text) => select_by_text_from_element(el, text),
    };
    apply_indices(matches, selector)
}

fn select_css<'a>(doc: &'a Html, css_selector: &str) -> Vec<ElementRef<'a>> {
    let sel = match Selector::parse(css_selector) {
        Ok(s) => s,
        Err(_) => return vec![],
    };
    doc.select(&sel).collect()
}

fn select_css_from_element<'a>(el: ElementRef<'a>, css_selector: &str) -> Vec<ElementRef<'a>> {
    let sel = match Selector::parse(css_selector) {
        Ok(s) => s,
        Err(_) => return vec![],
    };
    el.select(&sel).collect()
}

fn child_elements<'a>(el: ElementRef<'a>) -> Vec<ElementRef<'a>> {
    el.children().filter_map(ElementRef::wrap).collect()
}

/// Descendant elements of `el` whose tag name equals `tag` (case-insensitive).
/// `id.list@tag.li` means "all `li` under `#list`", which may be nested inside
/// a `ul` — so we match descendants, not just direct children.
///
/// An index spec on the tag (e.g. `@tag.tr!0`, `@tag.li[2:]`) is honored:
/// `!0` excludes the first row, `[0]`/`[1:3]` select a range — same semantics
/// as a base selector's index.
fn child_elements_by_tag<'a>(el: ElementRef<'a>, tag: &str) -> Vec<ElementRef<'a>> {
    let (base_tag, index_mode, index_items) =
        if let Some((base, index_mode, index_items)) = parse_bracket_index_spec(tag) {
            (base, index_mode, index_items)
        } else if let Some((base, index_mode, index_items)) = parse_legacy_index_spec(tag) {
            (base, index_mode, index_items)
        } else {
            (tag, IndexMode::Select, Vec::new())
        };

    let base_tag = base_tag.trim();
    let mut matches: Vec<ElementRef<'a>> = if let Ok(sel) = Selector::parse(base_tag) {
        el.select(&sel).collect()
    } else {
        el.children()
            .filter_map(ElementRef::wrap)
            .filter(|child| child.value().name().eq_ignore_ascii_case(base_tag))
            .collect()
    };

    if !index_items.is_empty() {
        matches = apply_indices(matches, &ParsedSelector {
            base: parse_selector_base(base_tag),
            explicit_index: true,
            index_mode,
            index_items,
        });
    }
    matches
}

fn select_by_text_doc<'a>(doc: &'a Html, needle: &str) -> Vec<ElementRef<'a>> {
    let sel = Selector::parse("*").unwrap();
    doc.select(&sel)
        .filter(|el| own_text(el).contains(needle))
        .collect()
}

fn select_by_text_from_element<'a>(el: ElementRef<'a>, needle: &str) -> Vec<ElementRef<'a>> {
    let mut matches = Vec::new();
    if own_text(&el).contains(needle) {
        matches.push(el);
    }
    if let Ok(sel) = Selector::parse("*") {
        matches.extend(
            el.select(&sel)
                .filter(|candidate| own_text(candidate).contains(needle)),
        );
    }
    matches
}

fn own_text(el: &ElementRef) -> String {
    let mut text = String::new();
    for node in el.children() {
        if let Some(text_node) = node.value().as_text() {
            text.push_str(text_node.text.trim());
        }
    }
    text
}

fn apply_indices<'a>(
    matches: Vec<ElementRef<'a>>,
    selector: &ParsedSelector,
) -> Vec<ElementRef<'a>> {
    if !selector.explicit_index {
        return matches;
    }

    let resolved = resolve_indices(matches.len(), &selector.index_items);
    if selector.index_mode == IndexMode::Exclude {
        let exclude_set: HashSet<usize> = resolved.into_iter().collect();
        return matches
            .into_iter()
            .enumerate()
            .filter(|(idx, _)| !exclude_set.contains(idx))
            .map(|(_, el)| el)
            .collect();
    }

    resolved
        .into_iter()
        .filter_map(|idx| matches.get(idx).copied())
        .collect()
}

fn resolve_indices(len: usize, items: &[IndexItem]) -> Vec<usize> {
    if len == 0 {
        return Vec::new();
    }

    let mut seen = HashSet::new();
    let mut resolved = Vec::new();
    for item in items {
        for idx in expand_index_item(len, item) {
            if seen.insert(idx) {
                resolved.push(idx);
            }
        }
    }
    resolved
}

fn expand_index_item(len: usize, item: &IndexItem) -> Vec<usize> {
    match item {
        IndexItem::Single(idx) => normalize_index(*idx, len).into_iter().collect(),
        IndexItem::Range { start, end, step } => expand_range(len, *start, *end, *step),
    }
}

fn normalize_index(index: i32, len: usize) -> Option<usize> {
    let len_i32 = len as i32;
    let resolved = if index < 0 { len_i32 + index } else { index };
    if resolved < 0 || resolved >= len_i32 {
        return None;
    }
    Some(resolved as usize)
}

fn expand_range(len: usize, start: Option<i32>, end: Option<i32>, step: i32) -> Vec<usize> {
    if len == 0 {
        return Vec::new();
    }

    let len_i32 = len as i32;
    let mut start = start.unwrap_or(0);
    let mut end = end.unwrap_or(len_i32 - 1);

    if start < 0 {
        start += len_i32;
    }
    if end < 0 {
        end += len_i32;
    }

    if (start < 0 && end < 0) || (start >= len_i32 && end >= len_i32) {
        return Vec::new();
    }

    start = start.clamp(0, len_i32 - 1);
    end = end.clamp(0, len_i32 - 1);

    let step = if step > 0 {
        step as usize
    } else if -step < len_i32 {
        (step + len_i32).max(1) as usize
    } else {
        1
    };

    let mut indices = Vec::new();
    if start <= end {
        let mut current = start as usize;
        let end = end as usize;
        while current <= end {
            indices.push(current);
            current = match current.checked_add(step) {
                Some(next) => next,
                None => break,
            };
        }
    } else {
        let mut current = start as usize;
        let end = end as usize;
        loop {
            indices.push(current);
            if current <= end || current < step {
                break;
            }
            current -= step;
        }
    }

    indices
}

/// Select elements with Legado rule syntax
pub fn select_list<'a>(doc: &'a Html, selector: &str) -> Vec<ElementRef<'a>> {
    let selector = selector.trim();

    // Split by @ first to get the base selector (handle @@ separately in text extraction)
    let sel_text = selector.split("@@").next().unwrap_or(selector).trim();
    let split = split_top_level(sel_text, &["@"]);
    let parts: Vec<&str> = split.parts.iter().map(|s| s.trim()).collect();
    let sel_text = parts.first().copied().unwrap_or(sel_text).trim();

    // Handle list combination operators at the top level
    if sel_text.contains("&&") || sel_text.contains("||") || sel_text.contains("%%") {
        return select_with_combination(doc, sel_text);
    }

    let base_matches = collect_matches(doc, &parse_selector_with_index(sel_text));

    // `@tag.xxx` suffix: expand each matched element's `xxx` child elements.
    // e.g. `id.list@tag.li` → select `#list`, then all its `li` children.
    if let Some(tag) = parts.get(1).and_then(|p| p.strip_prefix("tag.")) {
        if !tag.is_empty() {
            let mut out = Vec::new();
            for el in base_matches {
                out.extend(child_elements_by_tag(el, tag));
            }
            return out;
        }
    }

    base_matches
}

/// Handle list combination operators
fn select_with_combination<'a>(doc: &'a Html, rule: &str) -> Vec<ElementRef<'a>> {
    let split = split_top_level(rule, &["&&", "||", "%%"]);
    let rules = split.parts;

    if rules.is_empty() {
        return vec![];
    }

    let mut result = select_list_simple(doc, &rules[0]);
    let operator = split.delimiter.as_deref().unwrap_or("");

    for next_rule in rules.iter().skip(1) {
        let next_results = select_list_simple(doc, next_rule);

        match operator {
            "&&" => {
                result.extend(next_results);
            }
            "||" => {
                if result.is_empty() {
                    result = next_results;
                }
            }
            "%%" => {
                let mut zipped = Vec::new();
                let max_len = result.len().max(next_results.len());
                for j in 0..max_len {
                    if j < result.len() {
                        zipped.push(result[j]);
                    }
                    if j < next_results.len() {
                        zipped.push(next_results[j]);
                    }
                }
                result = zipped;
            }
            _ => {}
        }
    }

    result
}

/// Simple select without combination operators
fn select_list_simple<'a>(doc: &'a Html, selector: &str) -> Vec<ElementRef<'a>> {
    collect_matches(doc, &parse_selector_with_index(selector))
}

/// Extract text from element with various Legado extractors
pub fn extract_text(el: &ElementRef, extractor: &str) -> Option<String> {
    let extractor = extractor.trim();

    match extractor {
        "text" | "@text" => {
            let text = el.text().collect::<Vec<_>>().join(" ");
            let text = text.trim().to_string();
            if text.is_empty() {
                None
            } else {
                Some(text)
            }
        }
        "textNodes" | "@textNodes" => {
            let text = get_text_nodes(el);
            if text.is_empty() {
                None
            } else {
                Some(text)
            }
        }
        "ownText" | "@ownText" => {
            let mut own_text = String::new();
            for node in el.children() {
                if let Some(text_node) = node.value().as_text() {
                    own_text.push_str(text_node.text.trim());
                    own_text.push(' ');
                }
            }
            let text = own_text.trim().to_string();
            if text.is_empty() {
                None
            } else {
                Some(text)
            }
        }
        "html" | "@html" => Some(el.html()),
        "all" | "@all" => Some(el.html()),
        _ => {
            if let Some(attr_name) = parse_attr_extractor(extractor) {
                return el.value().attr(attr_name).map(|v| v.to_string());
            }

            if extractor.starts_with('@') {
                el.value().attr(&extractor[1..]).map(|v| v.to_string())
            } else {
                el.value().attr(extractor).map(|v| v.to_string())
            }
        }
    }
}

fn parse_attr_extractor(extractor: &str) -> Option<&str> {
    let extractor = extractor.trim();
    let extractor = extractor.strip_prefix('@').unwrap_or(extractor);
    extractor
        .strip_prefix("attr[")
        .and_then(|s| s.strip_suffix(']'))
        .filter(|s| !s.is_empty())
}

/// Get all text nodes from an element, preserving structure
fn get_text_nodes(el: &ElementRef) -> String {
    let mut texts = Vec::new();
    collect_text_nodes(*el, &mut texts);
    texts.join("\n")
}

fn collect_text_nodes(el: ElementRef, texts: &mut Vec<String>) {
    for node in el.children() {
        if let Some(text_node) = node.value().as_text() {
            let text = text_node.text.trim().to_string();
            if !text.is_empty() {
                texts.push(text);
            }
        }
        if let Some(child_el) = ElementRef::wrap(node) {
            collect_text_nodes(child_el, texts);
        }
    }
}

pub fn select_text_from_element(el: &ElementRef, rule: &str) -> Option<String> {
    let parts = split_top_level(rule, &["@"]).parts;
    let mut current_matches = vec![*el];

    for i in 0..parts.len() {
        let part = parts[i].trim();
        if part.is_empty() {
            continue;
        }

        if i == parts.len() - 1 {
            return current_matches
                .into_iter()
                .find_map(|current| extract_text(&current, part));
        }

        let parsed = parse_selector_with_index(part);
        let current_level = current_matches;
        let mut next_matches = Vec::new();
        for current in current_level {
            next_matches.extend(collect_matches_from_element(current, &parsed));
        }
        if next_matches.is_empty() {
            return None;
        }
        current_matches = next_matches;
    }

    current_matches
        .into_iter()
        .find_map(|current| extract_text(&current, "text"))
}

/// Select all matching elements and collect their text, joined by newlines
pub fn select_all_text(doc: &Html, rule: &str) -> Option<String> {
    let parts = split_top_level(rule, &["@"]).parts;
    if parts.is_empty() {
        return None;
    }

    let first_part = parts[0].trim();
    let roots = collect_matches(doc, &parse_selector_with_index(first_part));
    if roots.is_empty() {
        return None;
    }

    if parts.len() > 1 {
        let mut all_texts = Vec::new();
        let sub_rule = parts[1..].join("@");

        for root in roots {
            let last_part = sub_rule.trim();
            if let Some(text) = extract_text(&root, last_part) {
                if !text.is_empty() {
                    all_texts.push(text);
                }
                continue;
            }

            let parsed = parse_selector_with_index(sub_rule.trim());
            for el in collect_matches_from_element(root, &parsed) {
                if let Some(text) = extract_text(&el, "text") {
                    if !text.is_empty() {
                        all_texts.push(text);
                    }
                }
            }
        }

        if all_texts.is_empty() {
            return None;
        }
        return Some(all_texts.join("\n"));
    }

    let mut texts = Vec::new();
    for root in roots {
        if let Some(text) = extract_text(&root, "textNodes") {
            if !text.is_empty() {
                texts.push(text);
            }
        }
    }
    if texts.is_empty() {
        return None;
    }
    Some(texts.join("\n"))
}

pub fn select_text(doc: &Html, rule: &str) -> Option<String> {
    select_text_list(doc, rule).into_iter().next()
}

pub fn select_text_list(doc: &Html, rule: &str) -> Vec<String> {
    let combo = split_top_level(rule, &["&&", "||", "%%"]);
    if let Some(operator) = combo.delimiter.as_deref() {
        let mut result =
            select_text_list(doc, combo.parts.first().map(String::as_str).unwrap_or(""));
        for part in combo.parts.iter().skip(1) {
            let next = select_text_list(doc, part);
            match operator {
                "&&" => result.extend(next),
                "||" => {
                    if result.is_empty() {
                        result = next;
                    }
                }
                "%%" => {
                    let mut zipped = Vec::new();
                    let max_len = result.len().max(next.len());
                    for idx in 0..max_len {
                        if idx < result.len() {
                            zipped.push(result[idx].clone());
                        }
                        if idx < next.len() {
                            zipped.push(next[idx].clone());
                        }
                    }
                    result = zipped;
                }
                _ => {}
            }
        }
        return result;
    }

    // Handle rule chaining with @@
    if rule.contains("@@") {
        let rules: Vec<&str> = rule.split("@@").collect();
        if rules.is_empty() {
            return vec![];
        }

        // Start with first rule - get all matching texts
        let mut current_texts = select_text_list(doc, rules[0]);

        // Apply subsequent rules
        for r in rules.iter().skip(1) {
            if current_texts.is_empty() {
                break;
            }
            let mut new_texts = Vec::new();
            for text in &current_texts {
                let sub_doc = Html::parse_document(text);
                new_texts.extend(select_text_list(&sub_doc, r));
            }
            current_texts = new_texts;
        }

        return current_texts;
    }

    let parts = split_top_level(rule, &["@"]).parts;
    if parts.is_empty() {
        return vec![];
    }

    let first_part = parts[0].trim();

    let matches = collect_matches(doc, &parse_selector_with_index(first_part));
    if matches.is_empty() {
        return vec![];
    }

    let mut results = Vec::new();
    for el in matches {
        if parts.len() > 1 {
            let rest = parts[1..].join("@");
            if let Some(v) = select_text_from_element(&el, &rest) {
                results.push(v);
            }
        } else {
            results.push(extract_text(&el, "text").unwrap_or_default());
        }
    }
    results
}

/// XPath support using sxd-xpath
/// 把 legado/JsoupXpath 的 `...::text`、`...::ownText`、`...::html` 等轴扩展
/// 拆成 (XPath 路径, 提取器)。返回的路径可供标准 XPath 1.0 解析。
/// `pub` 供 rule_engine 的节点级 XPath 求值复用。
/// legado/JsoupXpath 的轴扩展提取器集合。`split_xpath_axis` 只认这些名字,
/// 避免把标准 XPath 1.0 轴(`child::span` / `parent::node` / `descendant::div`
/// 等)误当成提取器拆掉, 破坏合法 XPath 语义。
const LEGADO_XPATH_EXTRACTORS: &[&str] = &[
    "text",
    "ownText",
    "html",
    "outerHtml",
    "allText",
    "textNodes",
    "node",
];

/// 标准 XPath 1.0 轴名集合。`::` 前是这些名字时, `::word` 是轴限定(node-test),
/// 不是 legado 提取器, 不应拆分(如 `child::span` / `parent::node`)。
const XPATH_AXIS_NAMES: &[&str] = &[
    "ancestor",
    "ancestor-or-self",
    "attribute",
    "child",
    "descendant",
    "descendant-or-self",
    "following",
    "following-sibling",
    "namespace",
    "parent",
    "preceding",
    "preceding-sibling",
    "self",
];

pub fn split_xpath_axis(xpath: &str) -> (String, Option<&str>) {
    // 形如 `//div[@class='x']::text` → 路径 + 提取器 `text`
    // 注意避开 XPath 合法语法中的 `::`(轴限定, 如 `child::span`):
    // 只在末尾 `::word` 且 word 属于 legado 已知提取器集合、且 `::` 前不是
    // 标准 XPath 轴名时, 才拆分。后者用于消歧 `parent::node` 这种 `::` 前
    // 是轴名的情况(node 既是 legado 提取器又是 XPath 节点测试)。
    let trimmed = xpath.trim();
    // 匹配末尾 `::extractor`(extractor 为字母)
    let len = trimmed.len();
    let bytes = trimmed.as_bytes();
    let mut start = len;
    for &b in bytes.iter().rev() {
        if b.is_ascii_alphabetic() {
            start -= 1;
        } else {
            break;
        }
    }
    // 左边必须是 `::`
    if start < 2 {
        return (trimmed.to_string(), None);
    }
    let left = &trimmed[..start];
    if !left.ends_with("::") {
        return (trimmed.to_string(), None);
    }
    let extractor = &trimmed[start..];
    // 只接受 legado 已知提取器; 否则视为标准 XPath 轴, 原样返回。
    if !LEGADO_XPATH_EXTRACTORS.contains(&extractor) {
        return (trimmed.to_string(), None);
    }
    // `::` 前是标准 XPath 轴名 → 这是轴限定(node-test), 不是 legado 提取器。
    // 取 `::` 前最后一段(按 `/` 分割)判断轴名。
    let before = left[..left.len() - 2].trim_end();
    let last_segment = before.rsplit('/').next().unwrap_or(before);
    if XPATH_AXIS_NAMES.contains(&last_segment) {
        return (trimmed.to_string(), None);
    }
    let path = before.trim().to_string();
    (path, Some(extractor))
}

/// 递归把 sxd_xpath 节点序列化为 HTML 字符串(供 `::html` / `::outerHtml`)
/// 递归序列化 sxd 节点为 HTML 字符串(含外层标签)。
fn node_to_html_inner(node: sxd_xpath::nodeset::Node<'_>, text: &mut String) {
    use sxd_xpath::nodeset::Node;
    match node {
        Node::Element(el) => {
            let name = el.name().local_part();
            text.push('<');
            text.push_str(name);
            for attr in el.attributes() {
                let a = attr.name();
                text.push(' ');
                text.push_str(a.local_part());
                text.push_str("=\"");
                text.push_str(&attr.value().replace('"', "&quot;"));
                text.push('"');
            }
            text.push('>');
            for child in el.children() {
                use sxd_document::dom::ChildOfElement;
                match child {
                    // 子元素 → 递归
                    ChildOfElement::Element(e) => {
                        node_to_html_inner(Node::Element(e), text);
                    }
                    ChildOfElement::Text(t) => {
                        text.push_str(
                            &t.text()
                                .replace('&', "&amp;")
                                .replace('<', "&lt;")
                                .replace('>', "&gt;"),
                        );
                    }
                    ChildOfElement::Comment(c) => {
                        text.push_str("<!--");
                        text.push_str(c.text());
                        text.push_str("-->");
                    }
                    ChildOfElement::ProcessingInstruction(pi) => {
                        if let Some(v) = pi.value() {
                            text.push_str(&format!("<?{} {}?>", pi.target(), v));
                        }
                    }
                }
            }
            text.push_str("</");
            text.push_str(name);
            text.push('>');
        }
        Node::Text(t) => {
            text.push_str(
                &t.text()
                    .replace('&', "&amp;")
                    .replace('<', "&lt;")
                    .replace('>', "&gt;"),
            );
        }
        Node::Comment(c) => {
            text.push_str("<!--");
            text.push_str(c.text());
            text.push_str("-->");
        }
        Node::ProcessingInstruction(pi) => {
            if let Some(v) = pi.value() {
                text.push_str(&format!("<?{} {}?>", pi.target(), v));
            }
        }
        _ => {}
    }
}

/// 序列化节点为 outer HTML(含外层标签)。
fn node_to_html(node: &sxd_xpath::nodeset::Node<'_>) -> String {
    let mut out = String::new();
    node_to_html_inner(*node, &mut out);
    out
}

/// 序列化节点的子节点(inner HTML), 不含外层标签本身。
/// Legado 中 `::html` 返回 inner HTML, `::outerHtml` 返回含外层标签的完整序列化。
fn node_inner_html(node: &sxd_xpath::nodeset::Node<'_>) -> String {
    let mut out = String::new();
    for child in node.children() {
        node_to_html_inner(child, &mut out);
    }
    out
}

/// 从 sxd 节点按提取器取出文本/html。提取器为 legado/JsoupXpath 轴扩展名。
pub fn node_extract(node: &sxd_xpath::nodeset::Node<'_>, extractor: &str) -> Option<String> {
    use sxd_xpath::nodeset::Node;
    match extractor {
        // ::text / ::textNodes / ::allText / ::node → 元素树文本
        "text" | "textNodes" | "allText" | "node" => {
            let v = node.string_value();
            if v.trim().is_empty() {
                None
            } else {
                Some(v)
            }
        }
        // ::ownText → 仅直接文本子节点
        "ownText" => {
            let mut own = String::new();
            for child in node.children() {
                if let Node::Text(t) = child {
                    own.push_str(t.text().trim());
                    own.push(' ');
                }
            }
            let v = own.trim().to_string();
            if v.is_empty() {
                None
            } else {
                Some(v)
            }
        }
        // ::html → inner HTML (子节点序列化, 不含外层标签)
        "html" => {
            let v = node_inner_html(node);
            if v.trim().is_empty() {
                None
            } else {
                Some(v)
            }
        }
        // ::outerHtml → outer HTML (含外层标签)
        "outerHtml" => {
            let v = node_to_html(node);
            if v.trim().is_empty() {
                None
            } else {
                Some(v)
            }
        }
        _ => None,
    }
}

pub fn select_xpath(html: &str, xpath: &str) -> Vec<String> {
    let (path, extractor) = split_xpath_axis(xpath);

    let package = match sxd_document::parser::parse(html) {
        Ok(p) => p,
        Err(_) => return vec![],
    };

    let document = package.as_document();
    let context = sxd_xpath::Context::new();

    match sxd_xpath::Factory::new().build(&path) {
        Ok(Some(xpath_expr)) => match xpath_expr.evaluate(&context, document.root()) {
            Ok(value) => match value {
                sxd_xpath::Value::Nodeset(ns) => {
                    let nodes: Vec<_> = ns.document_order();
                    if let Some(ext) = extractor {
                        nodes
                            .iter()
                            .filter_map(|n| node_extract(n, ext))
                            .collect::<Vec<_>>()
                    } else {
                        nodes.iter().map(|n| n.string_value()).collect()
                    }
                }
                sxd_xpath::Value::String(s) => vec![s],
                sxd_xpath::Value::Number(n) => vec![n.to_string()],
                sxd_xpath::Value::Boolean(b) => vec![b.to_string()],
            },
            Err(_) => vec![],
        },
        _ => vec![],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_legado_to_css() {
        assert_eq!(legado_to_css("class.mod block"), ".mod.block");
        assert_eq!(legado_to_css("class.test"), ".test");
        assert_eq!(legado_to_css("id.main"), "#main");
        assert_eq!(legado_to_css("tag.div"), "div");
    }

    #[test]
    fn test_parse_selector_with_index() {
        let parsed = parse_selector_with_index(".test.0");
        assert_eq!(parsed.base, SelectorBase::Css(".test".to_string()));
        assert!(parsed.explicit_index);
        assert_eq!(parsed.index_mode, IndexMode::Select);
        assert_eq!(parsed.index_items, vec![IndexItem::Single(0)]);

        let parsed = parse_selector_with_index(".test.-1");
        assert_eq!(parsed.base, SelectorBase::Css(".test".to_string()));
        assert_eq!(parsed.index_items, vec![IndexItem::Single(-1)]);

        let parsed = parse_selector_with_index(".test!0");
        assert_eq!(parsed.base, SelectorBase::Css(".test".to_string()));
        assert_eq!(parsed.index_mode, IndexMode::Exclude);
        assert_eq!(parsed.index_items, vec![IndexItem::Single(0)]);

        let parsed = parse_selector_with_index("div[-1, 1:3]");
        assert_eq!(parsed.base, SelectorBase::Css("div".to_string()));
        assert_eq!(
            parsed.index_items,
            vec![
                IndexItem::Single(-1),
                IndexItem::Range {
                    start: Some(1),
                    end: Some(3),
                    step: 1,
                },
            ]
        );
    }

    #[test]
    fn test_select_text_list_with_bracket_indices() {
        let doc = parse_document(
            r#"<div><a href="/1">A</a><a href="/2">B</a><a href="/3">C</a><a href="/4">D</a></div>"#,
        );

        assert_eq!(
            select_text_list(&doc, "a[1:2]@text"),
            vec!["B".to_string(), "C".to_string()]
        );
        assert_eq!(
            select_text_list(&doc, "a[!1,2]@text"),
            vec!["A".to_string(), "D".to_string()]
        );
        assert_eq!(
            select_text_list(&doc, "a[-1:0]@text"),
            vec![
                "D".to_string(),
                "C".to_string(),
                "B".to_string(),
                "A".to_string()
            ]
        );
    }

    #[test]
    fn test_extract_attr_bracket_syntax() {
        let doc = parse_document(r#"<div><a href="/book/1" data-id="abc">Book</a></div>"#);

        assert_eq!(
            select_text(&doc, "a@attr[href]"),
            Some("/book/1".to_string())
        );
        assert_eq!(
            select_text(&doc, "a@attr[data-id]"),
            Some("abc".to_string())
        );
        assert_eq!(select_text(&doc, "a@href"), Some("/book/1".to_string()));
    }

    #[test]
    fn test_xpath_axis_extensions() {
        // legado/JsoupXpath 轴扩展: `//div::text`、`::ownText`、`::html`
        let html = r#"<root><div class="x"><span>hello</span> <b>world</b></div><div class="y">seed</div></root>"#;

        // ::text → 元素树文本
        assert_eq!(
            select_xpath(html, "//div[@class='x']::text"),
            vec!["hello world".to_string()]
        );
        // 无提取器 → 路径直接原样
        assert_eq!(
            select_xpath(html, "//div[@class='y']"),
            vec!["seed".to_string()]
        );
        // ::html → 序列化
        let got_html = select_xpath(html, "//div[@class='x']::html");
        assert_eq!(got_html.len(), 1);
        assert!(got_html[0].contains("hello"));
        assert!(got_html[0].contains("<span>"));
        // ::ownText → 该 div 的直接文本只有空白(子元素都被更深层包裹), 允许为空
        let own = select_xpath(html, "//div[@class='y']::ownText");
        assert_eq!(own, vec!["seed".to_string()]);
    }

    #[test]
    fn test_split_xpath_axis_leaves_standard_xpath_axis_intact() {
        // 标准 XPath 1.0 轴(child::span / parent::node / descendant::div)
        // 不能被当成 legado 提取器拆掉, 否则路径被截断、解析失败。
        assert_eq!(
            split_xpath_axis("//div/child::span"),
            ("//div/child::span".to_string(), None)
        );
        // parent::node 中 node 既是 legado 提取器又是 XPath 节点测试,
        // 但 `::` 前是轴名 parent → 不拆
        assert_eq!(
            split_xpath_axis("//a/parent::node"),
            ("//a/parent::node".to_string(), None)
        );
        assert_eq!(
            split_xpath_axis("//div/descendant::div"),
            ("//div/descendant::div".to_string(), None)
        );
        assert_eq!(
            split_xpath_axis("//div/ancestor::div"),
            ("//div/ancestor::div".to_string(), None)
        );
        assert_eq!(
            split_xpath_axis("//div/following-sibling::a"),
            ("//div/following-sibling::a".to_string(), None)
        );
        assert_eq!(
            split_xpath_axis("//div/self::node"),
            ("//div/self::node".to_string(), None)
        );
        // 末尾不是已知提取器名 → 不拆
        assert_eq!(
            split_xpath_axis("//div::notARealExtractor"),
            ("//div::notARealExtractor".to_string(), None)
        );
        // 已知提取器仍正常拆分(`::` 前不是轴名)
        assert_eq!(
            split_xpath_axis("//div[@class='x']::text"),
            ("//div[@class='x']".to_string(), Some("text"))
        );
        assert_eq!(
            split_xpath_axis("//div::ownText"),
            ("//div".to_string(), Some("ownText"))
        );
        assert_eq!(
            split_xpath_axis("//div::html"),
            ("//div".to_string(), Some("html"))
        );
        assert_eq!(
            split_xpath_axis("//div::outerHtml"),
            ("//div".to_string(), Some("outerHtml"))
        );
        assert_eq!(
            split_xpath_axis("//div::allText"),
            ("//div".to_string(), Some("allText"))
        );
        assert_eq!(
            split_xpath_axis("//div::textNodes"),
            ("//div".to_string(), Some("textNodes"))
        );
        // `::` 前是 div(非轴名) → 拆成提取器 node
        assert_eq!(
            split_xpath_axis("//div::node"),
            ("//div".to_string(), Some("node"))
        );
        // 无 :: → 原样
        assert_eq!(split_xpath_axis("//div/span"), ("//div/span".to_string(), None));
        // 无内容
        assert_eq!(split_xpath_axis(""), ("".to_string(), None));
    }

    #[test]
    fn select_xpath_html_extractor_returns_inner_html() {
        // ::html 应返回 inner HTML(子节点序列化), 不含外层 div 标签
        let html = r#"<div><p>hello</p><span>world</span></div>"#;
        let result = select_xpath(html, "//div::html");
        assert_eq!(result.len(), 1);
        assert_eq!(result[0], "<p>hello</p><span>world</span>");
    }

    #[test]
    fn select_xpath_outer_html_extractor_returns_outer_html() {
        // ::outerHtml 应返回含外层标签的完整序列化
        let html = r#"<div><p>hello</p></div>"#;
        let result = select_xpath(html, "//div::outerHtml");
        assert_eq!(result.len(), 1);
        assert_eq!(result[0], "<div><p>hello</p></div>");
    }
}
