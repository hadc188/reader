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

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
pub struct ExploreKind {
    pub title: String,
    pub url: Option<String>,
    pub style: Option<Value>,
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

pub fn book_source_from_value(value: Value) -> serde_json::Result<BookSource> {
    serde_json::from_value(migrate_legacy_book_source_value(value))
}

pub fn migrate_legacy_book_source_value(mut value: Value) -> Value {
    let Some(obj) = value.as_object_mut() else {
        return value;
    };

    move_if_absent(obj, "ruleBookUrlPattern", "bookUrlPattern");
    move_if_absent(obj, "serialNumber", "customOrder");
    move_if_absent(obj, "ruleFindUrl", "exploreUrl");
    move_if_absent(obj, "ruleSearchUrl", "searchUrl");
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

    for key in ["searchUrl", "exploreUrl", "loginUrl"] {
        if let Some(Value::String(raw)) = obj.get(key).cloned() {
            obj.insert(
                key.to_string(),
                Value::String(convert_legacy_url_rule(&raw)),
            );
        }
    }

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
        obj.insert(target.to_string(), Value::Object(target_obj));
    }
}

fn convert_legacy_url_rule(raw: &str) -> String {
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
    let re = regex::Regex::new(r"\{([^{}]*,[^{}]*)\}").unwrap();
    re.replace_all(input, "<$1>").into_owned()
}
