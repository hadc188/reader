use serde::{Deserialize, Serialize};

/// 一本书的缓存清单。按 legado 手机端的 `cache_manifest.json` 结构落盘，
/// 目的是让「哪些章已缓存、哪些章缺失」不再靠猜文件是否存在。
///
/// 与 legado 的差异：增加 `failed` 列表，任务结束后仍能看到失败原因并单独重试。
/// 清单只属于 `book_url` 对应的这一条书架记录，不同书源之间互不共享。
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
pub struct CacheManifest {
    pub version: i32,
    pub book_url: String,
    pub name: String,
    pub author: String,
    pub origin: String,
    pub origin_name: Option<String>,
    pub total_chapter_num: i32,
    pub updated_at: i64,
    pub chapters: Vec<CacheChapterEntry>,
    pub failed: Vec<CacheFailedEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
pub struct CacheChapterEntry {
    pub index: i32,
    pub title: String,
    pub url: String,
    #[serde(default)]
    pub is_volume: bool,
    #[serde(default)]
    pub cached: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
pub struct CacheFailedEntry {
    pub index: i32,
    pub title: String,
    pub url: String,
    pub reason: String,
    pub attempts: i32,
    pub last_attempt_at: i64,
    /// 这次失败发生在覆盖已缓存章节时。旧正文仍在，章节看起来是已缓存，
    /// 但失败记录必须保留，否则界面会把覆盖失败误报成成功。
    #[serde(default)]
    pub overwrite_failed: bool,
}

impl CacheManifest {
    /// 已缓存章节数（卷标题章天然没有正文，不计入）。
    pub fn cached_count(&self) -> usize {
        self.chapters
            .iter()
            .filter(|chapter| !chapter.is_volume && chapter.cached)
            .count()
    }

    /// 有效章节总数（同样剔除卷标题章）。
    pub fn real_chapter_count(&self) -> usize {
        self.chapters.iter().filter(|chapter| !chapter.is_volume).count()
    }
}
