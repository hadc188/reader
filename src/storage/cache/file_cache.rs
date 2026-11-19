use crate::util::hash::md5_hex;
use std::path::{Path, PathBuf};
use tokio::fs;

#[derive(Clone)]
pub struct FileCache {
    root: PathBuf,
}

impl FileCache {
    pub fn new(root: impl AsRef<Path>) -> Self {
        Self {
            root: root.as_ref().to_path_buf(),
        }
    }

    /// Get cached content for a specific book
    pub async fn get(
        &self,
        user_ns: &str,
        book_key: &str,
        chapter_key: &str,
    ) -> anyhow::Result<Option<String>> {
        let path = self.chapter_path(user_ns, book_key, chapter_key);
        if !path.exists() {
            return Ok(None);
        }
        let data = fs::read_to_string(path).await?;
        Ok(Some(data))
    }

    /// Put cached content for a specific book.
    ///
    /// 同时写入字数 sidecar(`<md5>.count`), 让目录不必为算字数去解析整篇正文。
    /// sidecar 写失败不影响正文可用 —— 读取侧会对缺失的 sidecar 回退现算。
    pub async fn put(
        &self,
        user_ns: &str,
        book_key: &str,
        chapter_key: &str,
        value: &str,
    ) -> anyhow::Result<()> {
        let path = self.chapter_path(user_ns, book_key, chapter_key);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).await?;
        }
        fs::write(&path, value).await?;
        let count =
            crate::util::text::visible_char_count(value).to_string();
        if let Err(err) = fs::write(self.count_path(user_ns, book_key, chapter_key), count).await {
            tracing::warn!("写入章节字数 sidecar 失败(将从正文现算): {err}");
        }
        Ok(())
    }

    /// 读取缓存字数。返回 `None` 表示**缺少** sidecar(旧缓存或写入失败),
    /// 调用方应回退到解析正文; 返回 `Some(0)` 表示确实统计为 0。
    pub async fn get_word_count(
        &self,
        user_ns: &str,
        book_key: &str,
        chapter_key: &str,
    ) -> anyhow::Result<Option<usize>> {
        let path = self.count_path(user_ns, book_key, chapter_key);
        match fs::read_to_string(&path).await {
            Ok(raw) => Ok(raw.trim().parse::<usize>().ok()),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(err) => Err(err.into()),
        }
    }

    /// 补写 sidecar。用于命中旧缓存(无字数)时把现算结果固化下来。
    pub async fn set_word_count(
        &self,
        user_ns: &str,
        book_key: &str,
        chapter_key: &str,
        count: usize,
    ) -> anyhow::Result<()> {
        let path = self.count_path(user_ns, book_key, chapter_key);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).await?;
        }
        fs::write(path, count.to_string()).await?;
        Ok(())
    }

    /// Remove a single chapter cache. sidecar 与正文一并删除, 避免字数残留下来
    /// 让"已删除"的章节仍然报出旧数字。
    pub async fn remove(
        &self,
        user_ns: &str,
        book_key: &str,
        chapter_key: &str,
    ) -> anyhow::Result<()> {
        let path = self.chapter_path(user_ns, book_key, chapter_key);
        if path.exists() {
            fs::remove_file(&path).await?;
        }
        let count_path = self.count_path(user_ns, book_key, chapter_key);
        if count_path.exists() {
            fs::remove_file(&count_path).await?;
        }
        Ok(())
    }

    /// Check if a chapter cache exists
    pub async fn exists(&self, user_ns: &str, book_key: &str, chapter_key: &str) -> bool {
        let path = self.chapter_path(user_ns, book_key, chapter_key);
        path.exists()
    }

    /// List all cached chapter file names (md5 hex stems without extension)
    /// for a book in one directory read. Used by `cached_chapter_count` to
    /// avoid N individual `path.exists()` system calls.
    pub async fn list_chapter_files(
        &self,
        user_ns: &str,
        book_key: &str,
    ) -> anyhow::Result<Vec<String>> {
        let dir = self.book_path(user_ns, book_key);
        let mut names = Vec::new();
        let mut entries = match tokio::fs::read_dir(&dir).await {
            Ok(entries) => entries,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(names),
            Err(e) => return Err(e.into()),
        };
        while let Some(entry) = entries.next_entry().await? {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) == Some("txt") {
                if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                    names.push(stem.to_string());
                }
            }
        }
        Ok(names)
    }

    /// 一本书的章节缓存正文总字节数（只统计 .txt，不含字数 sidecar 与清单）。
    pub async fn book_cache_size(&self, user_ns: &str, book_key: &str) -> anyhow::Result<u64> {
        let dir = self.book_path(user_ns, book_key);
        let mut total = 0u64;
        let mut entries = match tokio::fs::read_dir(&dir).await {
            Ok(entries) => entries,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(0),
            Err(e) => return Err(e.into()),
        };
        while let Some(entry) = entries.next_entry().await? {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("txt") {
                continue;
            }
            if let Ok(meta) = entry.metadata().await {
                total += meta.len();
            }
        }
        Ok(total)
    }

    /// 换源迁移：把一本书的缓存目录整体改名到新的 bookKey 下。
    /// 目标目录已存在时不做覆盖（两边都保留原样），返回 false。
    pub async fn move_book(
        &self,
        user_ns: &str,
        old_book_key: &str,
        new_book_key: &str,
    ) -> anyhow::Result<bool> {
        if old_book_key == new_book_key {
            return Ok(false);
        }
        let old_path = self.book_path(user_ns, old_book_key);
        let new_path = self.book_path(user_ns, new_book_key);
        if !old_path.exists() || new_path.exists() {
            return Ok(false);
        }
        if let Some(parent) = new_path.parent() {
            fs::create_dir_all(parent).await?;
        }
        fs::rename(&old_path, &new_path).await?;
        Ok(true)
    }
    /// 换源重映射：把一章的正文与字数 sidecar 改名到新章节地址对应的文件名。
    /// 旧正文不存在或目标已存在时不做任何事，返回 false。
    pub async fn rename_chapter(
        &self,
        user_ns: &str,
        book_key: &str,
        old_chapter_key: &str,
        new_chapter_key: &str,
    ) -> anyhow::Result<bool> {
        if old_chapter_key == new_chapter_key {
            return Ok(false);
        }
        let old_path = self.chapter_path(user_ns, book_key, old_chapter_key);
        let new_path = self.chapter_path(user_ns, book_key, new_chapter_key);
        if !old_path.exists() || new_path.exists() {
            return Ok(false);
        }
        fs::rename(&old_path, &new_path).await?;
        let old_count = self.count_path(user_ns, book_key, old_chapter_key);
        let new_count = self.count_path(user_ns, book_key, new_chapter_key);
        if old_count.exists() && !new_count.exists() {
            let _ = fs::rename(&old_count, &new_count).await;
        }
        Ok(true)
    }
    /// Remove all cache for a book (delete the book's cache directory)
    pub async fn remove_book(&self, user_ns: &str, book_key: &str) -> anyhow::Result<bool> {
        let path = self.book_path(user_ns, book_key);
        if path.exists() {
            fs::remove_dir_all(&path).await?;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    /// 列出某命名空间下所有书的缓存目录及其**最近一次写入**时间。
    ///
    /// 给「孤儿章节缓存回收」用：只有不在书架上的书才可能是孤儿，而回收还要
    /// 按年龄判断（见 `BookService::clean_orphan_chapter_caches`），所以这里把
    /// 目录的 mtime 一并带出去。目录不存在时返回空表（首次启动的正常情况）。
    pub async fn list_book_dirs(
        &self,
        user_ns: &str,
    ) -> anyhow::Result<Vec<(String, std::time::SystemTime)>> {
        let dir = self.root.join(user_ns);
        let mut out = Vec::new();
        let mut entries = match tokio::fs::read_dir(&dir).await {
            Ok(entries) => entries,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(out),
            Err(e) => return Err(e.into()),
        };
        while let Some(entry) = entries.next_entry().await? {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
                continue;
            };
            // 取不到 mtime 就按「刚刚写过」处理：宁可这次不删，也别误删活跃缓存。
            let modified = entry
                .metadata()
                .await
                .ok()
                .and_then(|meta| meta.modified().ok())
                .unwrap_or_else(std::time::SystemTime::now);
            out.push((name.to_string(), modified));
        }
        Ok(out)
    }

    /// Get the directory path for a book's cache
    /// 书的缓存目录。清单文件与正文文件放在同一目录。
    pub fn book_dir(&self, user_ns: &str, book_key: &str) -> PathBuf {
        self.book_path(user_ns, book_key)
    }
    fn book_path(&self, user_ns: &str, book_key: &str) -> PathBuf {
        self.root.join(user_ns).join(book_key)
    }

    /// Get the file path for a specific chapter
    fn chapter_path(&self, user_ns: &str, book_key: &str, chapter_key: &str) -> PathBuf {
        let name = md5_hex(chapter_key);
        self.book_path(user_ns, book_key)
            .join(name)
            .with_extension("txt")
    }

    /// 章节字数 sidecar 路径。用 `.count` 扩展名而不是 `.txt`, 这样
    /// `list_chapter_files` 不会把它当成一章的正文。
    fn count_path(&self, user_ns: &str, book_key: &str, chapter_key: &str) -> PathBuf {
        let name = md5_hex(chapter_key);
        self.book_path(user_ns, book_key)
            .join(name)
            .with_extension("count")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn temp_cache() -> (FileCache, PathBuf) {
        let root = std::env::temp_dir().join(format!(
            "reader-file-cache-{}",
            uuid::Uuid::new_v4().simple()
        ));
        (FileCache::new(root.join("cache")), root)
    }

    #[tokio::test]
    async fn put_writes_word_count_sidecar() {
        let (cache, root) = temp_cache();
        cache
            .put("default", "book", "chapter-1", "<p>你好世界</p>")
            .await
            .unwrap();

        // 标签不计入: `<p>你好世界</p>` 是 4 个字。
        assert_eq!(
            cache.get_word_count("default", "book", "chapter-1").await.unwrap(),
            Some(4)
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn missing_sidecar_is_none_not_zero() {
        let (cache, root) = temp_cache();
        // 分开判断 None / Some(0), 否则"旧缓存无字数"会被误当成"0 字"。
        assert_eq!(
            cache.get_word_count("default", "book", "absent").await.unwrap(),
            None
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn removing_chapter_also_removes_sidecar() {
        let (cache, root) = temp_cache();
        cache.put("default", "book", "c1", "正文").await.unwrap();
        assert!(cache.get_word_count("default", "book", "c1").await.unwrap().is_some());

        cache.remove("default", "book", "c1").await.unwrap();

        // 正文与 sidecar 都要消失, 否则删掉的章节仍会报出旧字数。
        assert!(!cache.exists("default", "book", "c1").await);
        assert_eq!(cache.get_word_count("default", "book", "c1").await.unwrap(), None);
        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn move_book_renames_directory_and_keeps_content() {
        let (cache, root) = temp_cache();
        cache.put("default", "old", "c1", "正文").await.unwrap();

        let moved = cache.move_book("default", "old", "new").await.unwrap();

        assert!(moved);
        // 旧目录不能再被读到，新目录承接全部正文与字数 sidecar。
        assert!(!cache.exists("default", "old", "c1").await);
        assert_eq!(
            cache.get("default", "new", "c1").await.unwrap().as_deref(),
            Some("正文")
        );
        assert!(cache
            .get_word_count("default", "new", "c1")
            .await
            .unwrap()
            .is_some());
        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn move_book_keeps_both_when_target_exists() {
        let (cache, root) = temp_cache();
        cache.put("default", "old", "c1", "旧正文").await.unwrap();
        cache.put("default", "new", "c2", "新正文").await.unwrap();

        // 目标目录已有缓存时不做覆盖，避免两个书源的缓存被混在一起。
        let moved = cache.move_book("default", "old", "new").await.unwrap();

        assert!(!moved);
        assert_eq!(
            cache.get("default", "old", "c1").await.unwrap().as_deref(),
            Some("旧正文")
        );
        assert_eq!(
            cache.get("default", "new", "c2").await.unwrap().as_deref(),
            Some("新正文")
        );
        let _ = std::fs::remove_dir_all(root);
    }
    #[tokio::test]
    async fn sidecar_is_not_listed_as_a_chapter() {
        let (cache, root) = temp_cache();
        cache.put("default", "book", "c1", "正文").await.unwrap();

        // 目录里同时有 c1.txt 与 c1.count, 列表只能出现 c1 一次。
        let names = cache.list_chapter_files("default", "book").await.unwrap();
        assert_eq!(names.len(), 1, "sidecar 不能被当成章节: {names:?}");
        assert_eq!(names[0], md5_hex("c1"));
        let _ = std::fs::remove_dir_all(root);
    }
}
