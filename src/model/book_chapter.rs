use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
pub struct BookChapter {
    pub title: String,
    pub url: String,
    pub index: i32,
    pub tag: Option<String>,
    pub is_vip: bool,
    pub is_pay: bool,
    pub is_volume: bool,
}
