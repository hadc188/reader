//! 书源导入时的**类型宽容**（对齐 legado 的 `StringJsonDeserializer`）。
//!
//! legado 在 `GsonExtensions.kt` 里给 `INITIAL_GSON` 注册了自定义的
//! `StringJsonDeserializer`，对**所有** `String` 字段生效：
//!
//! ```kotlin
//! json.isJsonPrimitive -> json.asString   // true → "true"、3 → "3"
//! json.isJsonNull      -> null
//! else                 -> json.toString() // 对象/数组 → 紧凑 JSON
//! ```
//!
//! 也就是说手机上任何字段写成什么类型都能导入。serde 是严格类型检查，
//! 同一份书源会直接失败，用户看到的是「这个源无法导入」：
//!
//! ```text
//! bad request: invalid type: map, expected a string            (yckceo 8048)
//! bad request: invalid type: boolean `true', expected a string (yckceo 8050)
//! ```
//!
//! 两处都要按上面的语义还原，否则「网站上能下、手机能导、桌面导不进」。
use reader_rust::model::book_source::book_source_from_value;
use serde_json::json;

fn base() -> serde_json::Value {
    json!({
        "bookSourceName": "测试源",
        "bookSourceUrl": "https://t.test",
    })
}

/// `header` 写成 JSON 对象时必须能导入，且解析出的请求头正确。
///
/// 这是 yckceo 8048 的直接回归。
#[test]
fn header_as_object_is_accepted() {
    let mut v = base();
    v["header"] = json!({
        "User-Agent": "MyUA/1.0",
        "Accept-Language": "zh-CN"
    });
    let source = book_source_from_value(v).expect("header 是对象也应能导入");

    let raw = source.header.expect("header 应被保留");
    // Gson 语义：对象 → 紧凑 JSON 字符串
    assert!(raw.contains("User-Agent"), "实际: {raw}");
    assert!(raw.contains("MyUA/1.0"), "实际: {raw}");

    // 下游必须能照常解析出来
    let headers = reader_rust::crawler::url_analyzer::parse_source_headers(&raw);
    assert!(
        headers.iter().any(|(k, v)| k.eq_ignore_ascii_case("User-Agent") && v == "MyUA/1.0"),
        "请求头应能解析出来，实际: {headers:?}"
    );
    assert!(headers.iter().any(|(k, _)| k.eq_ignore_ascii_case("Accept-Language")));
}

/// 其它字符串字段写成对象时同样宽容（legado 的自定义反序列化器对所有
/// `String` 字段生效，不只 `header`）。
#[test]
fn other_string_fields_accept_objects() {
    for field in [
        "bookSourceGroup",
        "bookSourceComment",
        "loginUrl",
        "searchUrl",
        "exploreUrl",
        "jsLib",
        "concurrentRate",
    ] {
        let mut v = base();
        v[field] = json!({ "k": "v" });
        assert!(
            book_source_from_value(v).is_ok(),
            "{field} 写成对象时应能导入"
        );
    }
}

/// 每个规则对象的子字段都不能因为类型而拒绝导入。
///
/// 这条是「宽口径」守卫：书源作者写规则时把 `true` / `3` / 对象塞进任意字段
/// 都不该让整个源导入失败（手机上就是如此）。
#[test]
fn any_rule_field_accepts_any_json_type() {
    let rule_fields = [
        "bookList", "name", "author", "intro", "kind", "lastChapter", "updateTime",
        "bookUrl", "coverUrl", "wordCount", "checkKeyWord", "tocUrl", "canReName",
        "downloadUrls", "init", "chapterList", "chapterName", "chapterUrl",
        "nextTocUrl", "isVolume", "isVip", "isPay", "formatJs", "preUpdateJs",
        "content", "title", "nextContentUrl", "webJs", "sourceRegex", "replaceRegex",
    ];
    let values = [
        json!(true),
        json!(false),
        json!(3),
        json!({"a": "b"}),
        json!(["x"]),
    ];
    for rule in ["ruleSearch", "ruleBookInfo", "ruleToc", "ruleContent", "ruleExplore"] {
        for field in rule_fields {
            for value in &values {
                let mut v = base();
                v[rule] = json!({ field: value.clone() });
                assert!(
                    book_source_from_value(v).is_ok(),
                    "{rule}.{field} 写成 {value} 时不应拒绝导入"
                );
            }
        }
    }
}

/// 数组按 legado `StringJsonDeserializer` 语义 `toString()` 成紧凑 JSON。
///
/// 注意**不是**「取第一个元素」—— 那是 Gson 默认 `nextString()` 的行为，
/// 而 legado 装的是自定义反序列化器：`else -> json.toString()`，整个数组被
/// 序列化成 `["…"]`。
#[test]
fn array_becomes_compact_json() {
    let mut v = base();
    v["header"] = json!([r#"{"User-Agent":"UA"}"#]);
    let source = book_source_from_value(v).expect("数组应能导入");
    let got = source.header.expect("header 应被保留");
    assert_eq!(got, r#"["{\"User-Agent\":\"UA\"}"]"#, "实际: {got}");
}

/// 标量按 `isJsonPrimitive -> asString` 转成字符串（yckceo 8050 的回归）。
///
/// `"ruleBookInfo": {"canReName": true}` 在手机上能导入，桌面端曾报
/// `bad request: invalid type: boolean `true', expected a string`。
#[test]
fn rule_scalars_become_strings() {
    let mut v = base();
    v["ruleBookInfo"] = json!({
        "name": "h1@text",
        "canReName": true,
        "wordCount": 3,
    });
    let source = book_source_from_value(v).expect("规则里的布尔/数字应能导入");
    let info = source.rule_book_info.expect("ruleBookInfo 应被保留");
    assert_eq!(info.can_re_name.as_deref(), Some("true"));
    assert_eq!(info.word_count.as_deref(), Some("3"));
    assert_eq!(info.name.as_deref(), Some("h1@text"));
}

/// 顶层字符串字段的布尔值/数字同样按标量语义转换。
#[test]
fn top_level_scalars_become_strings() {
    let mut v = base();
    v["concurrentRate"] = json!(1000);
    v["bookSourceComment"] = json!(true);
    let source = book_source_from_value(v).expect("顶层标量应能导入");
    assert_eq!(source.concurrent_rate.as_deref(), Some("1000"));
    assert_eq!(source.book_source_comment.as_deref(), Some("true"));
}

/// 正常的字符串形态必须**原样保留**，不能被宽容处理改写。
#[test]
fn string_headers_are_left_untouched() {
    let raw = r#"{"User-Agent":"UA","Accept":"*/*"}"#;
    let mut v = base();
    v["header"] = json!(raw);
    let source = book_source_from_value(v).unwrap();
    assert_eq!(source.header.as_deref(), Some(raw), "字符串形态应逐字保留");

    // 空串、null 也要保持原样
    let mut v = base();
    v["header"] = json!("");
    assert_eq!(
        book_source_from_value(v).unwrap().header.as_deref(),
        Some("")
    );
    let mut v = base();
    v["header"] = serde_json::Value::Null;
    assert!(book_source_from_value(v).unwrap().header.is_none());
}

/// 真实书源样本批量导入不得回归。
///
/// 用仓库里的 836 条样本做一次「全量能导入」检查，确保宽容处理没有把
/// 原本正常的源改坏。
#[test]
fn real_source_corpus_still_imports() {
    let path = std::path::Path::new("tmp_probe/sources-836.json");
    if !path.exists() {
        // 样本是本地调试产物，CI 上可能没有；不因此判失败。
        return;
    }
    let raw = std::fs::read_to_string(path).unwrap();
    let list: Vec<serde_json::Value> = serde_json::from_str(&raw).unwrap();
    let mut failed = Vec::new();
    for item in &list {
        if let Err(err) = book_source_from_value(item.clone()) {
            let name = item
                .get("bookSourceName")
                .and_then(|v| v.as_str())
                .unwrap_or("?");
            failed.push(format!("{name}: {err}"));
        }
    }
    assert!(failed.is_empty(), "真实样本导入失败 {} 条: {failed:#?}", failed.len());
}
