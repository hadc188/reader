//! 书籍导出 (txt / epub)。

pub mod epub;

use scraper::{ElementRef, Html};

/// 把可能含 HTML 标签的正文转为纯文本, 供 txt/epub 导出使用。
///
/// CSS 规则已返回纯文本 (无 '<'), 走快速路径; XPath `@html` / JS 规则返回的
/// HTML 片段在此剥离标签, 块级元素 (p/div/br/li/标题等) 转为换行, 保留行内空格。
pub fn html_to_plain_text(html: &str) -> String {
    if !html.contains('<') {
        return html.to_string();
    }
    let frag = Html::parse_fragment(html);
    let mut out = String::with_capacity(html.len());
    walk_element(&frag.root_element(), &mut out);
    // 折叠多余空行, 去每行首尾空白。
    let lines: Vec<&str> = out.lines().map(|l| l.trim()).filter(|l| !l.is_empty()).collect();
    lines.join("\n")
}

fn walk_element(el: &ElementRef, out: &mut String) {
    for node in el.children() {
        if let Some(text_node) = node.value().as_text() {
            out.push_str(&text_node.text);
        } else if let Some(child_el) = ElementRef::wrap(node) {
            if is_block_tag(child_el.value().name()) {
                if !out.is_empty() && !out.ends_with('\n') {
                    out.push('\n');
                }
                walk_element(&child_el, out);
                if !out.ends_with('\n') {
                    out.push('\n');
                }
            } else {
                walk_element(&child_el, out);
            }
        }
    }
}

fn is_block_tag(name: &str) -> bool {
    matches!(
        name,
        "p" | "div" | "br" | "li" | "ul" | "ol" | "tr" | "td" | "th" | "section" | "article"
            | "blockquote" | "pre" | "table" | "header" | "footer" | "hr"
            | "h1" | "h2" | "h3" | "h4" | "h5" | "h6"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_tags_and_breaks_blocks_into_lines() {
        let html = "<div><p>第一段</p><p>第二段</p></div>";
        assert_eq!(html_to_plain_text(html), "第一段\n第二段");
    }

    #[test]
    fn keeps_inline_text_spaced() {
        let html = "<p>Hello <b>world</b>!</p>";
        assert_eq!(html_to_plain_text(html), "Hello world!");
    }

    #[test]
    fn passes_through_plain_text_without_tags() {
        assert_eq!(html_to_plain_text("纯文本内容"), "纯文本内容");
    }

    #[test]
    fn handles_br_and_nested_blocks() {
        let html = "<p>第一行<br>第二行</p>";
        assert_eq!(html_to_plain_text(html), "第一行\n第二行");
    }
}
