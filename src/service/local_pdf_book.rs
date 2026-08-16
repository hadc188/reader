use crate::error::error::AppError;
use crate::model::{book::Book, book_chapter::BookChapter};
use crate::util::hash::md5_hex;
use lopdf::Document as PdfDocument;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use tokio::fs;

pub const LOCAL_PDF_ORIGIN: &str = "local-pdf";
pub const LOCAL_PDF_ORIGIN_NAME: &str = "本地 PDF";
pub const MAX_PDF_UPLOAD_BYTES: usize = 100 * 1024 * 1024;
const LOCAL_BOOK_DIR: &str = "local_books";
const LOCAL_PDF_HASH_LEN: usize = 32;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct StoredPdfChapter {
    title: String,
    url: String,
    index: i32,
    page_start: usize,
    page_end: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct StoredPdfIndex {
    book_url: String,
    name: String,
    file_name: String,
    byte_len: usize,
    char_len: usize,
    chapters: Vec<StoredPdfChapter>,
}

pub fn is_local_pdf_origin(value: &str) -> bool {
    value.trim() == LOCAL_PDF_ORIGIN
}

pub fn is_local_pdf_url(value: &str) -> bool {
    value.trim().starts_with("local-pdf:")
}

pub fn build_chapter_url(book_url: &str, index: usize) -> String {
    format!("{}#{}", book_url.trim_end_matches('#'), index)
}

pub fn sanitize_pdf_file_name(file_name: &str) -> String {
    let name = Path::new(file_name)
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("book.pdf")
        .trim()
        .to_string();
    if name.is_empty() {
        "book.pdf".to_string()
    } else {
        name
    }
}

pub fn book_name_from_file_name(file_name: &str) -> String {
    let safe = sanitize_pdf_file_name(file_name);
    Path::new(&safe)
        .file_stem()
        .and_then(|value| value.to_str())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("本地小说")
        .to_string()
}

pub fn validate_pdf_upload(file_name: &str, byte_len: usize) -> Result<(), AppError> {
    let safe = sanitize_pdf_file_name(file_name);
    if !safe.to_lowercase().ends_with(".pdf") {
        return Err(AppError::BadRequest("仅支持上传 .pdf 文件".to_string()));
    }
    if byte_len == 0 {
        return Err(AppError::BadRequest("PDF 文件不能为空".to_string()));
    }
    if byte_len > MAX_PDF_UPLOAD_BYTES {
        return Err(AppError::BadRequest("PDF 文件不能超过 100MB".to_string()));
    }
    Ok(())
}

/// 从 PDF 字节提取每页文本。lopdf 的 extract_text 遍历页面内容流中的
/// 文本算子 (Tj/TJ) 并拼装。无法解析的页面返回空串。
fn extract_pdf_pages(bytes: &[u8]) -> Result<Vec<String>, AppError> {
    let doc = PdfDocument::load_mem(bytes)
        .map_err(|e| AppError::BadRequest(format!("无法解析 PDF 文件: {e}")))?;

    // get_pages() 返回 BTreeMap<u32, ObjectId>，key 是页码(1-based)，
    // BTreeMap 已按 key 排序，逐页提取文本。
    let page_numbers: Vec<u32> = doc.get_pages().keys().copied().collect();

    let mut result = Vec::with_capacity(page_numbers.len());
    for page_num in page_numbers {
        let text = doc.extract_text(&[page_num]).unwrap_or_default();
        result.push(text);
    }
    Ok(result)
}

/// 按页拆分章节: 每页一章, 标题「第 N 页」。不跳过空文本页(封面/空白页/扫描页),
/// 保证目录页数与 PDF 实际页数一致, 阅读器翻页和目录能对上。
fn build_chapters_from_pages(book_url: &str, pages: &[String]) -> Vec<StoredPdfChapter> {
    let mut chapters = Vec::with_capacity(pages.len());
    for (page_idx, _text) in pages.iter().enumerate() {
        let chapter_index = page_idx as i32;
        chapters.push(StoredPdfChapter {
            title: format!("第 {} 页", page_idx + 1),
            url: build_chapter_url(book_url, page_idx),
            index: chapter_index,
            page_start: page_idx,
            page_end: page_idx,
        });
    }
    chapters
}

#[derive(Clone)]
pub struct LocalPdfBookService {
    storage_dir: PathBuf,
}

impl LocalPdfBookService {
    pub fn new(storage_dir: impl AsRef<Path>) -> Self {
        Self {
            storage_dir: storage_dir.as_ref().to_path_buf(),
        }
    }

    pub async fn import_pdf_book(
        &self,
        user_ns: &str,
        file_name: &str,
        bytes: &[u8],
    ) -> Result<Book, AppError> {
        validate_pdf_upload(file_name, bytes.len())?;
        let safe_file_name = sanitize_pdf_file_name(file_name);

        let pages = extract_pdf_pages(bytes)?;
        if pages.iter().all(|p| p.trim().is_empty()) {
            return Err(AppError::BadRequest(
                "PDF 文件未提取到任何文本内容（可能是扫描件/图片 PDF）".to_string(),
            ));
        }

        let all_text = pages.join("\n\n");
        let hash = md5_hex(&format!(
            "{}:{}:{}",
            user_ns,
            safe_file_name,
            md5_hex(&all_text)
        ));
        let book_url = format!("{}:{}", LOCAL_PDF_ORIGIN, hash);
        let chapters = build_chapters_from_pages(&book_url, &pages);
        if chapters.is_empty() {
            return Err(AppError::BadRequest("PDF 文件未提取到任何可读页面".to_string()));
        }

        let book_dir = self.book_dir(user_ns, &book_url)?;
        fs::create_dir_all(&book_dir)
            .await
            .map_err(|e| AppError::Internal(e.into()))?;

        // 存储原始 PDF 文件, 供后续按页读取
        fs::write(book_dir.join("book.pdf"), bytes)
            .await
            .map_err(|e| AppError::Internal(e.into()))?;

        let char_len = all_text.chars().count();
        let index = StoredPdfIndex {
            book_url: book_url.clone(),
            name: book_name_from_file_name(&safe_file_name),
            file_name: safe_file_name,
            byte_len: bytes.len(),
            char_len,
            chapters,
        };
        let data =
            serde_json::to_string_pretty(&index).map_err(|e| AppError::Internal(e.into()))?;
        fs::write(book_dir.join("chapters.json"), data)
            .await
            .map_err(|e| AppError::Internal(e.into()))?;

        Ok(Book {
            name: index.name,
            author: "本地导入".to_string(),
            book_url: book_url.clone(),
            origin: LOCAL_PDF_ORIGIN.to_string(),
            origin_name: Some(LOCAL_PDF_ORIGIN_NAME.to_string()),
            toc_url: Some(book_url),
            can_update: Some(false),
            dur_chapter_index: Some(0),
            dur_chapter_pos: Some(0),
            total_chapter_num: Some(index.chapters.len() as i32),
            latest_chapter_title: index.chapters.last().map(|c| c.title.clone()),
            kind: Some("本地PDF".to_string()),
            word_count: Some(format!("{}字", char_len)),
            ..Book::default()
        })
    }

    pub async fn get_book_info(&self, user_ns: &str, book_url: &str) -> Result<Book, AppError> {
        let index = self.read_index(user_ns, book_url).await?;
        Ok(Book {
            name: index.name,
            author: "本地导入".to_string(),
            book_url: index.book_url.clone(),
            origin: LOCAL_PDF_ORIGIN.to_string(),
            origin_name: Some(LOCAL_PDF_ORIGIN_NAME.to_string()),
            toc_url: Some(index.book_url.clone()),
            can_update: Some(false),
            total_chapter_num: Some(index.chapters.len() as i32),
            latest_chapter_title: index.chapters.last().map(|c| c.title.clone()),
            kind: Some("本地PDF".to_string()),
            word_count: Some(format!("{}字", index.char_len)),
            ..Book::default()
        })
    }

    pub async fn get_chapter_list(
        &self,
        user_ns: &str,
        book_url: &str,
    ) -> Result<Vec<BookChapter>, AppError> {
        let index = self.read_index(user_ns, book_url).await?;
        Ok(index
            .chapters
            .into_iter()
            .map(|chapter| BookChapter {
                title: chapter.title,
                url: chapter.url,
                index: chapter.index,
                ..BookChapter::default()
            })
            .collect())
    }

    pub async fn get_content(&self, user_ns: &str, chapter_url: &str) -> Result<String, AppError> {
        let (book_url, requested_index) = parse_chapter_url(chapter_url)?;
        let index = self.read_index(user_ns, &book_url).await?;
        let chapter = index
            .chapters
            .iter()
            .find(|c| c.index == requested_index)
            .ok_or_else(|| AppError::BadRequest("章节不存在".to_string()))?;

        // 从 PDF 文件重新提取指定页的文本
        let pdf_path = self.book_dir(user_ns, &book_url)?.join("book.pdf");
        let bytes = fs::read(&pdf_path)
            .await
            .map_err(map_local_pdf_read_error)?;
        let doc = PdfDocument::load_mem(&bytes)
            .map_err(|e| AppError::Internal(e.into()))?;
        let page_numbers: Vec<u32> = doc.get_pages().keys().copied().collect();
        let page_num = page_numbers
            .get(chapter.page_start)
            .copied()
            .ok_or_else(|| AppError::BadRequest("PDF 页码无效".to_string()))?;
        let text = doc.extract_text(&[page_num]).unwrap_or_default();
        Ok(text)
    }

    pub async fn delete_book_files(&self, user_ns: &str, book_url: &str) -> Result<bool, AppError> {
        let book_dir = self.book_dir(user_ns, book_url)?;
        match fs::remove_dir_all(book_dir).await {
            Ok(()) => Ok(true),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(false),
            Err(err) => Err(AppError::Internal(err.into())),
        }
    }

    fn local_root(&self, user_ns: &str) -> PathBuf {
        self.storage_dir
            .join("data")
            .join(user_ns)
            .join(LOCAL_BOOK_DIR)
    }

    fn book_dir(&self, user_ns: &str, book_url: &str) -> Result<PathBuf, AppError> {
        let hash = local_pdf_hash_from_url(book_url)?;
        Ok(self.local_root(user_ns).join(hash))
    }

    async fn read_index(&self, user_ns: &str, book_url: &str) -> Result<StoredPdfIndex, AppError> {
        let path = self.book_dir(user_ns, book_url)?.join("chapters.json");
        let data = fs::read_to_string(path)
            .await
            .map_err(map_local_pdf_read_error)?;
        serde_json::from_str(&data).map_err(|e| AppError::BadRequest(e.to_string()))
    }
}

fn parse_chapter_url(chapter_url: &str) -> Result<(String, i32), AppError> {
    let (book_url, raw_index) = chapter_url
        .rsplit_once('#')
        .ok_or_else(|| AppError::BadRequest("章节地址无效".to_string()))?;
    if !is_local_pdf_url(book_url) {
        return Err(AppError::BadRequest("章节地址无效".to_string()));
    }
    let index = raw_index
        .parse::<i32>()
        .map_err(|_| AppError::BadRequest("章节序号无效".to_string()))?;
    Ok((book_url.to_string(), index))
}

fn local_pdf_hash_from_url(book_url: &str) -> Result<&str, AppError> {
    let hash = book_url
        .strip_prefix("local-pdf:")
        .filter(|value| {
            value.len() == LOCAL_PDF_HASH_LEN && value.chars().all(|ch| ch.is_ascii_hexdigit())
        })
        .ok_or_else(|| AppError::BadRequest("本地 PDF 地址无效".to_string()))?;
    Ok(hash)
}

fn map_local_pdf_read_error(err: std::io::Error) -> AppError {
    if err.kind() == std::io::ErrorKind::NotFound {
        AppError::BadRequest("本地 PDF 不存在".to_string())
    } else {
        AppError::Internal(err.into())
    }
}
