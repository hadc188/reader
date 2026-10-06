use serde::{Deserialize, Serialize};

use super::search::SearchBook;

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
    /// 用户自定义书名/作者/简介之前的原始值, 用于「还原」。
    ///
    /// 只在**首次**保存自定义内容时写入, 之后不再变动 —— 这样无论用户改过多少轮,
    /// 都能一键回到最初始(书源/导入时)的样子。三个字段都是 Option: 从未自定义过
    /// 的书它们是 None, 此时「还原」无内容可回退。
    pub original_name: Option<String>,
    pub original_author: Option<String>,
    pub original_intro: Option<String>,
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
    /// Per-source search hits retained for stable source switching after the
    /// book is added to the shelf.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_candidates: Option<Vec<SearchBook>>,
}
