use serde::de::DeserializeOwned;
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::{json, Map, Value};

use crate::model::rule::{BookInfoRule, ContentRule, ExploreRule, ReviewRule, SearchRule, TocRule};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
pub struct BookSource {
    pub book_source_name: String,
    pub book_source_group: Option<String>,
    pub book_source_url: String,
    pub book_source_type: Option<i32>,
    pub book_url_pattern: Option<String>,
    pub custom_order: Option<i32>,
    pub enabled: Option<bool>,
    pub enabled_explore: Option<bool>,
    pub enabled_cookie_jar: Option<bool>,
    pub js_lib: Option<String>,
    pub concurrent_rate: Option<String>,
    pub header: Option<String>,
    pub login_url: Option<String>,
    #[serde(deserialize_with = "deserialize_login_ui")]
    pub login_ui: Option<String>,
    pub login_check_js: Option<String>,
    pub cover_decode_js: Option<String>,
    #[serde(deserialize_with = "deserialize_i64_option")]
    pub last_update_time: Option<i64>,
    pub weight: Option<i32>,
    pub explore_url: Option<String>,
    pub explore_screen: Option<String>,
    #[serde(deserialize_with = "deserialize_rule_option")]
    pub rule_explore: Option<ExploreRule>,
    pub search_url: Option<String>,
    #[serde(deserialize_with = "deserialize_rule_option")]
    pub rule_search: Option<SearchRule>,
    #[serde(deserialize_with = "deserialize_rule_option")]
    pub rule_book_info: Option<BookInfoRule>,
    #[serde(deserialize_with = "deserialize_rule_option")]
    pub rule_toc: Option<TocRule>,
    #[serde(deserialize_with = "deserialize_rule_option")]
    pub rule_content: Option<ContentRule>,
    #[serde(deserialize_with = "deserialize_rule_option")]
    pub rule_review: Option<ReviewRule>,
    pub book_source_comment: Option<String>,
    pub variable_comment: Option<String>,
    #[serde(deserialize_with = "deserialize_i64_option")]
    pub respond_time: Option<i64>,
    pub load_with_base_url: Option<bool>,
    pub single_url: Option<bool>,
}

impl BookSource {
    pub fn is_enabled(&self) -> bool {
        self.enabled != Some(false)
    }

    /// 检测书源可用性时使用的搜索关键词。
    ///
    /// 默认值取 `"我的"` 而不是 `"测试"`: 书源站点普遍是小说站, "我的" 这类高频
    /// 泛词几乎必然能搜到结果, 而 "测试"/"测试书籍" 在真实站点上常常一本书都搜不到,
    /// 于是"搜索解析为空"会把**完全正常的书源误判成失效**。Legado 官方默认值同样是
    /// "我的" (CheckSource.kt: `var keyword = "我的"`)。
    ///
    /// `checkKeyWord` 不可用时回退到默认值, 与 Legado 的 `getCheckKeyword` 对齐:
    /// 它要求关键词不是空串, 且**不含 `http` / `::` / `++` / `--`** —— 这些子串说明
    /// 该字段里填的是 URL 或规则语法片段(如 `a++b--c`), 拿去搜索必然解析为空,
    /// 从而把好源误判失效。
    pub fn check_keyword(&self) -> String {
        self.rule_search
            .as_ref()
            .and_then(|rule| rule.check_key_word.as_deref())
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .filter(|value| !["http", "::", "++", "--"].iter().any(|token| value.contains(token)))
            .unwrap_or(DEFAULT_CHECK_KEYWORD)
            .to_string()
    }
}

/// 检测书源可用性时的默认搜索关键词(与 Legado 官方一致)。
pub const DEFAULT_CHECK_KEYWORD: &str = "我的";

/// 发现页分类项，对应 legado `ExploreKind`。
///
/// 它不只是"一个分类链接"：`type` 决定渲染成什么控件，`action` 是该控件的
/// 点击脚本，`chars`/`default` 是 select 与 toggle 的选项与初值。聚合类书源的
/// 发现页筛选栏、搜索框、刷新按钮全靠这几个字段 ——
/// 早先只保留 `title/url/style` 时它们会在反序列化阶段被静默丢弃，前端因而
/// 只能把每个控件都渲染成一个普通分类按钮。
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
pub struct ExploreKind {
    pub title: String,
    pub url: Option<String>,
    /// `url` | `text` | `button` | `toggle` | `select`，见 `ExploreKindType`。
    /// 字段名 `kind_type` 是为了避开 Rust 关键字，线上 JSON 键仍是 `type`
    /// （书源里就写作 `type`，前端也按 `type` 读）。
    #[serde(rename = "type")]
    pub kind_type: Option<String>,
    pub action: Option<String>,
    pub chars: Option<Vec<Option<String>>>,
    pub default: Option<String>,
    /// 控件显示名：`'字面量'` 直接取用，否则是段 JS（legado 语义）。
    pub view_name: Option<String>,
    pub style: Option<Value>,
}

impl ExploreKind {
    /// 归一化后的控件类型；legado 缺省值是 `url`。
    pub fn kind_type(&self) -> ExploreKindType {
        match self.kind_type.as_deref().map(str::trim) {
            Some("text") => ExploreKindType::Text,
            Some("button") => ExploreKindType::Button,
            Some("toggle") => ExploreKindType::Toggle,
            Some("select") => ExploreKindType::Select,
            _ => ExploreKindType::Url,
        }
    }

    /// 非空选项列表（legado 的 `chars` 允许含 null 元素，需过滤）。
    pub fn chars_list(&self) -> Vec<String> {
        self.chars
            .as_deref()
            .unwrap_or_default()
            .iter()
            .flatten()
            .map(|value| value.trim().to_string())
            .collect()
    }
}

/// legado `ExploreKind.Type`。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExploreKindType {
    Url,
    Text,
    Button,
    Toggle,
    Select,
}

fn deserialize_rule_option<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: Deserializer<'de>,
    T: DeserializeOwned,
{
    let value = Option::<Value>::deserialize(deserializer)?;
    let Some(value) = value else {
        return Ok(None);
    };
    match value {
        Value::Null => Ok(None),
        Value::String(raw) => {
            let raw = raw.trim();
            if raw.is_empty() || raw.eq_ignore_ascii_case("null") {
                Ok(None)
            } else {
                serde_json::from_str(raw)
                    .map(Some)
                    .map_err(serde::de::Error::custom)
            }
        }
        other => serde_json::from_value(other)
            .map(Some)
            .map_err(serde::de::Error::custom),
    }
}

fn deserialize_i64_option<'de, D>(deserializer: D) -> Result<Option<i64>, D::Error>
where
    D: Deserializer<'de>,
{
    let value = Option::<Value>::deserialize(deserializer)?;
    let Some(value) = value else {
        return Ok(None);
    };
    match value {
        Value::Null => Ok(None),
        Value::Number(num) => num
            .as_i64()
            .map(Some)
            .ok_or_else(|| serde::de::Error::custom("expected i64-compatible number")),
        Value::String(raw) => {
            let raw = raw.trim();
            if raw.is_empty() || raw.eq_ignore_ascii_case("null") {
                Ok(None)
            } else {
                raw.parse::<i64>()
                    .map(Some)
                    .map_err(serde::de::Error::custom)
            }
        }
        other => Err(serde::de::Error::custom(format!(
            "expected i64-compatible value, got {other}"
        ))),
    }
}

fn deserialize_login_ui<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<Option<String>, D::Error> {
    match Option::<Value>::deserialize(deserializer)? {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(text)) => Ok(Some(text)),
        Some(value @ Value::Array(_)) => Ok(Some(value.to_string())),
        Some(_) => Err(serde::de::Error::custom("loginUi must be a string or an array")),
    }
}

/// 把「本该是字符串、却写成了 JSON 对象/数组」的字段还原成字符串。
///
/// ## 为什么需要
///
/// legado 用 **Gson** 反序列化书源。Gson 的 `TypeAdapters.STRING` 对非字符串
/// 输入走 `nextString()`：对象被序列化成紧凑 JSON、数组取第一个元素，**从不
/// 报类型错误**。所以手机端对这类「格式不严谨」的源照单全收。
///
/// serde 是严格类型检查，同一个源会直接失败：
///
/// ```text
/// bad request: invalid type: map, expected a string
/// ```
///
/// 用户看到的就是「这个源无法导入」，而同一份 JSON 在手机上正常
/// （yckceo 8048 / twkan.cc 的 `header` 就写成了对象）。
///
/// ## 处理范围
///
/// 只覆盖 `BookSource` 里声明为 `Option<String>` 的字段。对象/数组原样
/// `to_string()` 回字符串 —— 与 Gson 的 `nextString()` 一致，下游解析
/// （`parse_source_headers` 按 JSON 解析 `header`、规则引擎按规则串求值）
/// 拿到的仍是它原本期望的形态。
///
/// 规则对象（`ruleSearch` / `ruleToc` …）的子字段不在这里处理：它们经
/// `deserialize_rule_option` 走 `Value` 中转，塞对象同样会失败，但真实书源里
/// 从未出现过（836 个样本零命中），贸然放宽反而会把写错的规则悄悄变成
/// 「合法但无用」的字符串。
fn normalize_string_fields(obj: &mut serde_json::Map<String, Value>) {
    const STRING_FIELDS: &[&str] = &[
        "bookSourceName",
        "bookSourceGroup",
        "bookSourceUrl",
        "bookSourceComment",
        "bookUrlPattern",
        "jsLib",
        "concurrentRate",
        "header",
        "loginUrl",
        "loginUi",
        "loginCheckJs",
        "coverDecodeJs",
        "exploreUrl",
        "exploreScreen",
        "searchUrl",
        "variableComment",
        "ruleSearchUrl",
        "ruleFindUrl",
        "httpUserAgent",
    ];
    for field in STRING_FIELDS {
        let Some(value) = obj.get(*field) else { continue };
        let normalized = match value {
            // 与 legado `StringJsonDeserializer` 一致：非标量一律 `toString()`
            // （紧凑 JSON）。
            Value::Object(_) | Value::Array(_) => Some(Value::String(value.to_string())),
            // 标量：`isJsonPrimitive → asString`（`true` → `"true"`、`3` → `"3"`）。
            Value::Bool(flag) => Some(Value::String(flag.to_string())),
            Value::Number(number) => Some(Value::String(number.to_string())),
            _ => None,
        };
        if let Some(value) = normalized {
            obj.insert((*field).to_string(), value);
        }
    }
}

pub fn book_source_from_value(value: Value) -> serde_json::Result<BookSource> {
    serde_json::from_value(migrate_legacy_book_source_value(value))
}

pub fn migrate_legacy_book_source_value(mut value: Value) -> Value {
    let Some(obj) = value.as_object_mut() else {
        return value;
    };

    move_if_absent(obj, "ruleBookUrlPattern", "bookUrlPattern");
    move_if_absent(obj, "serialNumber", "customOrder");
    // Only URLs originating in the old schema need syntax migration. Modern
    // fields can contain arbitrary JS/JSON; rewriting braces corrupts them.
    for (old, new) in [("ruleFindUrl", "exploreUrl"), ("ruleSearchUrl", "searchUrl")] {
        if !obj.contains_key(new) {
            if let Some(raw) = obj.remove(old) {
                let converted = match raw {
                    Value::String(raw) => Value::String(convert_legacy_url_rule(&raw)),
                    other => other,
                };
                obj.insert(new.to_string(), converted);
            }
        }
    }
    move_if_absent(obj, "enable", "enabled");

    if let Some(Value::String(kind)) = obj.get("bookSourceType").cloned() {
        let mapped = if kind.eq_ignore_ascii_case("AUDIO") {
            1
        } else {
            0
        };
        obj.insert("bookSourceType".to_string(), json!(mapped));
    }

    if !obj.contains_key("header") {
        if let Some(ua) = obj.get("httpUserAgent").and_then(Value::as_str) {
            if !ua.trim().is_empty() {
                obj.insert(
                    "header".to_string(),
                    Value::String(json!({ "User-Agent": ua }).to_string()),
                );
            }
        }
    }
    // 字符串字段被写成 JSON 对象/数组时按 Gson 语义还原成字符串。
    normalize_string_fields(obj);

    migrate_rule_object(
        obj,
        "ruleSearch",
        &[
            ("ruleSearchList", "bookList"),
            ("ruleSearchName", "name"),
            ("ruleSearchAuthor", "author"),
            ("ruleSearchIntro", "intro"),
            ("ruleSearchKind", "kind"),
            ("ruleSearchLastChapter", "lastChapter"),
            ("ruleSearchUpdateTime", "updateTime"),
            ("ruleSearchNoteUrl", "bookUrl"),
            ("ruleSearchBookUrl", "bookUrl"),
            ("ruleSearchCoverUrl", "coverUrl"),
            ("ruleSearchWordCount", "wordCount"),
        ],
    );
    migrate_rule_object(
        obj,
        "ruleExplore",
        &[
            ("ruleFindList", "bookList"),
            ("ruleFindName", "name"),
            ("ruleFindAuthor", "author"),
            ("ruleFindIntro", "intro"),
            ("ruleFindKind", "kind"),
            ("ruleFindLastChapter", "lastChapter"),
            ("ruleFindUpdateTime", "updateTime"),
            ("ruleFindNoteUrl", "bookUrl"),
            ("ruleFindBookUrl", "bookUrl"),
            ("ruleFindCoverUrl", "coverUrl"),
            ("ruleFindWordCount", "wordCount"),
        ],
    );
    migrate_rule_object(
        obj,
        "ruleBookInfo",
        &[
            ("ruleBookInfoInit", "init"),
            ("ruleBookName", "name"),
            ("ruleBookAuthor", "author"),
            ("ruleIntroduce", "intro"),
            ("ruleBookIntro", "intro"),
            ("ruleBookKind", "kind"),
            ("ruleBookLastChapter", "lastChapter"),
            ("ruleBookUpdateTime", "updateTime"),
            ("ruleCoverUrl", "coverUrl"),
            ("ruleBookCoverUrl", "coverUrl"),
            ("ruleBookWordCount", "wordCount"),
            ("ruleChapterUrl", "tocUrl"),
            ("ruleBookTocUrl", "tocUrl"),
        ],
    );
    migrate_rule_object(
        obj,
        "ruleToc",
        &[
            ("ruleChapterList", "chapterList"),
            ("ruleChapterName", "chapterName"),
            ("ruleContentUrl", "chapterUrl"),
            ("ruleChapterUrl", "chapterUrl"),
            ("ruleChapterUpdateTime", "updateTime"),
            ("ruleChapterUrlNext", "nextTocUrl"),
            ("ruleTocUrlNext", "nextTocUrl"),
        ],
    );
    migrate_rule_object(
        obj,
        "ruleContent",
        &[
            ("ruleBookContent", "content"),
            ("ruleBookContentReplace", "replaceRegex"),
            ("ruleContentUrlNext", "nextContentUrl"),
            ("ruleContentTitle", "title"),
        ],
    );

    value
}

fn move_if_absent(obj: &mut Map<String, Value>, old: &str, new: &str) {
    if obj.contains_key(new) {
        return;
    }
    if let Some(value) = obj.remove(old) {
        obj.insert(new.to_string(), value);
    }
}

fn migrate_rule_object(obj: &mut Map<String, Value>, target: &str, fields: &[(&str, &str)]) {
    let mut target_obj = obj
        .get(target)
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    for (old, new) in fields {
        if target_obj.contains_key(*new) {
            continue;
        }
        if let Some(value) = obj.remove(*old) {
            if !value.as_str().map(str::trim).unwrap_or("x").is_empty() {
                target_obj.insert((*new).to_string(), value);
            }
        }
    }
    if !target_obj.is_empty() {
        // 规则子字段在书源格式里一律是**字符串**，但站点上下的源常把布尔/数字
        // 直接写进去（`"canReName": true`）。legado 用 Gson，`nextString()` 对
        // 标量照单全收（`true` → `"true"`、`3` → `"3"`），serde 则报
        // `invalid type: boolean `true', expected a string`。
        // 这里按 Gson 语义把标量转成字符串。
        normalize_rule_scalars(&mut target_obj);
        obj.insert(target.to_string(), Value::Object(target_obj));
    }
}

/// 把规则对象里「写成布尔/数字/对象/数组」的字段转成字符串。
///
/// 语义完全照搬 legado 的 `StringJsonDeserializer`（`GsonExtensions.kt`）：
///
/// ```kotlin
/// json.isJsonPrimitive -> json.asString   // true → "true"、3 → "3"
/// json.isJsonNull      -> null
/// else                 -> json.toString() // 对象/数组 → 紧凑 JSON
/// ```
///
/// 这个自定义反序列化器对 `INITIAL_GSON` 里的**所有** `String` 字段生效，
/// 所以手机上任何字段写成什么类型都能导入；serde 严格类型检查则会直接
/// 报 `invalid type: boolean `true', expected a string`（yckceo 8050 的
/// `ruleBookInfo.canReName: true` 就是这样）。
fn normalize_rule_scalars(fields: &mut Map<String, Value>) {
    for (_, value) in fields.iter_mut() {
        let converted = match &*value {
            Value::Bool(flag) => Some(Value::String(flag.to_string())),
            Value::Number(number) => Some(Value::String(number.to_string())),
            Value::Object(_) | Value::Array(_) => Some(Value::String(value.to_string())),
            _ => None,
        };
        if let Some(replacement) = converted {
            *value = replacement;
        }
    }
}

fn convert_legacy_url_rule(raw: &str) -> String {
    if raw.contains("@js:") || raw.contains("<js>") || raw.contains(",{\"") {
        return raw.to_string();
    }
    let mut url = raw.to_string();
    let mut option = Map::new();

    if let Some((start, end, header)) = extract_legacy_header(&url) {
        url.replace_range(start..end, "");
        if let Ok(value) = serde_json::from_str::<Value>(&header) {
            option.insert("headers".to_string(), value);
        } else {
            option.insert("headers".to_string(), Value::String(header));
        }
    }

    if let Some(idx) = url.find("|charset=") {
        let charset = url[idx + "|charset=".len()..].trim().to_string();
        url.truncate(idx);
        if !charset.is_empty() {
            option.insert("charset".to_string(), Value::String(charset));
        }
    }

    if let Some(idx) = url.find("@body") {
        let body = url[idx + "@body".len()..]
            .trim_start_matches([':', '='])
            .trim()
            .to_string();
        url.truncate(idx);
        option.insert("method".to_string(), Value::String("POST".to_string()));
        option.insert("body".to_string(), Value::String(body));
    }

    url = url
        .replace("searchKey", "{{key}}")
        .replace("searchPage", "{{page}}");
    url = convert_legacy_page_braces(&url);

    if option.is_empty() {
        url
    } else {
        format!("{},{}", url, Value::Object(option))
    }
}

fn extract_legacy_header(input: &str) -> Option<(usize, usize, String)> {
    let start = input.find("@Header:")?;
    let object_start = start + "@Header:".len();
    let rest = input.get(object_start..)?.trim_start();
    let skipped = input.get(object_start..)?.len() - rest.len();
    let open = object_start + skipped;
    if !input.get(open..)?.starts_with('{') {
        return None;
    }
    let mut depth = 0i32;
    for (offset, ch) in input[open..].char_indices() {
        match ch {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    let end = open + offset + ch.len_utf8();
                    return Some((start, end, input[open..end].to_string()));
                }
            }
            _ => {}
        }
    }
    None
}

fn convert_legacy_page_braces(input: &str) -> String {
    let re = regex::Regex::new(r"\{(\s*\d+(?:\s*,\s*\d+)+\s*)\}").unwrap();
    re.replace_all(input, "<$1>").into_owned()
}

#[cfg(test)]
mod check_keyword_tests {
    use super::*;

    fn source_with_check_key_word(value: Option<&str>) -> BookSource {
        BookSource {
            book_source_name: "kw test".to_string(),
            book_source_url: "https://kw.test".to_string(),
            rule_search: Some(crate::model::rule::SearchRule {
                check_key_word: value.map(str::to_string),
                ..Default::default()
            }),
            ..Default::default()
        }
    }

    /// 默认关键词必须是"我的"而不是"测试": 后者在真实站点上常搜不到书,
    /// 会把正常书源误判成失效。
    #[test]
    fn falls_back_to_a_generic_keyword_that_actually_returns_books() {
        let source = source_with_check_key_word(None);
        assert_eq!(source.check_keyword(), "我的");

        // 空串/纯空白同样回退, 不能被当成"用户指定了空关键词"。
        for value in [Some(""), Some("   ")] {
            assert_eq!(
                source_with_check_key_word(value).check_keyword(),
                "我的",
                "空 checkKeyWord 应回退默认值"
            );
        }
    }

    /// 源自己配置了关键词时必须优先使用(只取首尾空白后的原值)。
    #[test]
    fn prefers_source_configured_keyword() {
        let source = source_with_check_key_word(Some("  剑来  "));
        assert_eq!(source.check_keyword(), "剑来");
    }

    /// 含 URL / 规则语法片段的关键词拿去搜索必然解析为空, 会误判失效 ——
    /// 与 Legado `getCheckKeyword` 一致地回退默认值。
    #[test]
    fn rejects_url_or_rule_syntax_keywords() {
        for value in [
            "https://example.com/search?q=1",
            "http://a.test",
            "{{key}}++{{page}}",
            "a--b",
            "class::name",
        ] {
            assert_eq!(
                source_with_check_key_word(Some(value)).check_keyword(),
                "我的",
                "无效关键词 {value:?} 应回退默认值"
            );
        }
    }

    /// 没有 ruleSearch 的源也要能拿到默认关键词(不能 panic)。
    #[test]
    fn works_without_search_rule() {
        let source = BookSource {
            book_source_name: "no rule".to_string(),
            book_source_url: "https://none.test".to_string(),
            rule_search: None,
            ..Default::default()
        };
        assert_eq!(source.check_keyword(), "我的");
    }
}
