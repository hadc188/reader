use serde::{Deserialize, Serialize};

/// 一条净化规则。
///
/// 字段与手机端 legado 的 `ReplaceRule` 对齐。**新增字段全部带默认值**：老存档
/// （`replaceRule.json`）里没有这些键时反序列化仍然成功，行为与改动前一致；
/// 前端只发老字段时也不会被新字段的默认值改变语义（标题默认关、正文默认开）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct ReplaceRule {
    pub id: i64,
    pub name: String,
    pub group: Option<String>,
    pub pattern: String,
    pub replacement: String,
    /// 生效范围。保留桌面端两种前缀写法（`book:<bookUrl>` / `source:<origin>`）；
    /// `*` / 空表示全部书籍；其余情况按**子串**匹配书名与书源地址，
    /// 与手机端 `scope LIKE '%name%'` 的语义一致。
    pub scope: Option<String>,
    /// 排除范围：命中即跳过本规则（子串匹配，语义同 `scope`）。
    pub exclude_scope: Option<String>,
    /// 作用于章节标题。默认关：老规则升级后不会突然开始改写标题。
    #[serde(rename = "scopeTitle")]
    pub scope_title: bool,
    /// 作用于正文。默认开，与改动前的行为一致。
    #[serde(rename = "scopeContent")]
    pub scope_content: bool,
    #[serde(rename = "isEnabled")]
    pub is_enabled: bool,
    #[serde(rename = "isRegex")]
    pub is_regex: bool,
    /// 单条规则的超时（毫秒）。手机端默认 3000；非正值按默认处理。
    pub timeout_millisecond: i64,
    pub order: i32,
}

/// 手机端 `getValidTimeoutMillisecond()` 的默认值。
pub const DEFAULT_RULE_TIMEOUT_MS: i64 = 3000;

impl Default for ReplaceRule {
    fn default() -> Self {
        Self {
            id: 0,
            name: String::new(),
            group: None,
            pattern: String::new(),
            replacement: String::new(),
            scope: None,
            exclude_scope: None,
            scope_title: false,
            // 默认只作用于正文：老规则升级后不会突然开始改标题。
            scope_content: true,
            is_enabled: true,
            is_regex: false,
            timeout_millisecond: DEFAULT_RULE_TIMEOUT_MS,
            order: 0,
        }
    }
}

impl ReplaceRule {
    /// 手机端 `getValidTimeoutMillisecond()`：非正值回落到默认 3000ms。
    pub fn valid_timeout_ms(&self) -> i64 {
        if self.timeout_millisecond <= 0 {
            DEFAULT_RULE_TIMEOUT_MS
        } else {
            self.timeout_millisecond
        }
    }
}
