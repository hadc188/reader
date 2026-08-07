use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
pub struct RssSource {
    pub source_url: String,
    pub source_name: String,
    pub source_icon: Option<String>,
    pub source_group: Option<String>,
    pub source_comment: Option<String>,
    pub enabled: Option<bool>,
    pub concurrent_rate: Option<String>,
    pub header: Option<String>,
    pub login_url: Option<String>,
    pub login_check_js: Option<String>,
    pub sort_url: Option<String>,
    pub single_url: Option<bool>,
    pub article_style: Option<i32>,
    pub rule_articles: Option<String>,
    pub rule_next_page: Option<String>,
    pub rule_title: Option<String>,
    pub rule_pub_date: Option<String>,
    pub rule_description: Option<String>,
    pub rule_image: Option<String>,
    pub rule_link: Option<String>,
    pub rule_content: Option<String>,
    pub style: Option<String>,
    pub enable_js: Option<bool>,
    pub load_with_base_url: Option<bool>,
    pub custom_order: Option<i32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
pub struct RssArticle {
    pub origin: String,
    pub sort: String,
    pub title: String,
    pub order: i64,
    pub link: String,
    pub pub_date: Option<String>,
    pub description: Option<String>,
    pub content: Option<String>,
    pub image: Option<String>,
    pub read: Option<bool>,
    pub variable: Option<String>,
}
