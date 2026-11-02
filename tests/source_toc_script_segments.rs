//! 目录列表规则「选择器段 + 后续 `@js:` 段」的串联求值。
//!
//! legado `AnalyzeRule.splitSourceRule` 用 `JS_PATTERN` 把脚本段切出来，剩下的
//! 文本前后各成一段，**上一段的输出喂给下一段**。目录列表规则里这种写法很常见：
//!
//! ```text
//! id.list@dl>a
//! @js:var seen={};var out=[];for(...){...}out;
//! ```
//!
//! 早先 `parse_toc_list` 只看 `detect_mode`：规则开头不是 `@js:` 就判成 CSS，
//! 整条串（含 `@js:` 那几行）丢给 CSS 选择器 → 恒选不到 → **目录 0 章**，
//! 而搜索/详情都正常（yckceo id 8043 就是这样）。
use reader_rust::{
    model::book_source::book_source_from_value,
    parser::rule_engine::RuleEngine,
};
use serde_json::json;

fn source(toc: serde_json::Value) -> reader_rust::model::book_source::BookSource {
    book_source_from_value(json!({
        "bookSourceName": "目录脚本段",
        "bookSourceUrl": "https://toc-script.test",
        "ruleToc": toc,
    }))
    .unwrap()
}

fn body() -> String {
    let mut html = String::from(r#"<div id="list"><dl>"#);
    html.push_str(r#"<dt><a title="书">书</a>最新章节</dt>"#);
    for i in 1..=5 {
        html.push_str(&format!(
            r#"<a href="/b/{i}.html" title="第{i}章" rel="chapter"><dd>第{i}章</dd></a>"#
        ));
    }
    // 一条重复链接：书源自带的去重脚本要把它去掉。
    html.push_str(r#"<a href="/b/5.html" title="第5章" rel="chapter"><dd>第5章</dd></a>"#);
    html.push_str("</dl></div>");
    html
}

/// 选择器段 + `@js:` 段必须串联：先选元素，脚本按 `attr('href')` 去重，再出章节。
#[test]
fn trailing_script_segment_runs_after_the_selector() {
    let engine = RuleEngine::new().unwrap();
    let source = source(json!({
        "chapterList": "id.list@dl>a\n@js:var seen={};var out=[];for(var i=result.length-1;i>=0;i--){var u=String(result[i].attr(\"href\")||\"\");if(u&&!seen[u]){seen[u]=1;out.unshift(result[i])}}out;",
        "chapterName": "text",
        "chapterUrl": "href"
    }));
    let (chapters, _) = engine.chapter_list(&source, &body(), "https://toc-script.test", None);

    assert_eq!(chapters.len(), 5, "脚本段应去掉重复链接后剩 5 章");
    assert_eq!(chapters[0].title, "第1章");
    assert_eq!(chapters[0].url, "https://toc-script.test/b/1.html");
    assert_eq!(chapters[4].title, "第5章");
}

/// 整条规则就是 `@js:` 时保持原来的 JS 分支（返回值可能是 JSONPath 原始节点）。
#[test]
fn whole_rule_script_still_returns_raw_json_items() {
    let engine = RuleEngine::new().unwrap();
    let source = source(json!({
        "chapterList": "@js:result = java.getElements(\"$.list[*]\")",
        "chapterName": "title",
        "chapterUrl": "id"
    }));
    let body = json!({
        "total": 2,
        "list": [
            { "id": "/chapter/1", "title": "第一章" },
            { "id": "/chapter/2", "title": "第二章" }
        ]
    })
    .to_string();
    let (chapters, _) = engine.chapter_list(&source, &body, "https://toc-script.test", None);
    assert_eq!(chapters.len(), 2, "纯脚本目录仍应解析出 2 章");
    assert_eq!(chapters[0].title, "第一章");
}

/// `<js>…</js>` 夹在中间时同样要串联（解析层随后按 URL 去重，故重复链接只剩一条）。
#[test]
fn inline_js_block_between_selector_segments() {
    let engine = RuleEngine::new().unwrap();
    let source = source(json!({
        "chapterList": "id.list@dl>a<js>result</js>",
        "chapterName": "text",
        "chapterUrl": "href"
    }));
    let (chapters, _) = engine.chapter_list(&source, &body(), "https://toc-script.test", None);
    assert_eq!(chapters.len(), 5, "内联 js 段应透传选中的链接，按 URL 去重后 5 章");
    assert_eq!(chapters[0].title, "第1章");
}
