use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
pub struct Book {
    pub name: String,
    pub author: String,
    pub book_url: String,
    pub origin: String,
    pub origin_name: Option<String>,
    pub cover_url: Option<String>,
    pub toc_url: Option<String>,
    pub charset: Option<String>,
    pub custom_cover_url: Option<String>,
    pub can_update: Option<bool>,
    pub dur_chapter_index: Option<i32>,
    pub dur_chapter_pos: Option<i32>,
    pub dur_chapter_time: Option<i64>,
    pub dur_chapter_title: Option<String>,
    pub intro: Option<String>,
    pub latest_chapter_title: Option<String>,
    pub last_check_time: Option<i64>,
    pub total_chapter_num: Option<i32>,
    pub r#type: Option<i32>,
    pub group: Option<i64>,
    pub word_count: Option<String>,
    pub info_html: Option<String>,
    pub toc_html: Option<String>,
    pub kind: Option<String>,
    pub update_time: Option<String>,
    pub can_re_name: Option<String>,
    pub download_urls: Option<String>,
}
