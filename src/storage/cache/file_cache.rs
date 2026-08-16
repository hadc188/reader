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

    /// Put cached content for a specific book
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
        fs::write(path, value).await?;
        Ok(())
    }

    /// Remove a single chapter cache
    pub async fn remove(
        &self,
        user_ns: &str,
        book_key: &str,
        chapter_key: &str,
    ) -> anyhow::Result<()> {
        let path = self.chapter_path(user_ns, book_key, chapter_key);
        if path.exists() {
            fs::remove_file(path).await?;
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

    /// Get the directory path for a book's cache
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
}
