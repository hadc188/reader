use crate::error::error::AppError;
use crate::model::{book::Book, book_chapter::BookChapter};
use crate::util::hash::md5_hex;
use encoding_rs::GB18030;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use tokio::fs;

pub const LOCAL_TXT_ORIGIN: &str = "local-txt";
pub const LOCAL_TXT_ORIGIN_NAME: &str = "本地 TXT";
pub const MAX_TXT_UPLOAD_BYTES: usize = 50 * 1024 * 1024;
const LOCAL_BOOK_DIR: &str = "local_books";
const LOCAL_TXT_HASH_LEN: usize = 32;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ParsedTxtChapter {
    pub title: String,
    pub url: String,
    pub index: i32,
    pub start: usize,
    pub end: usize,
    pub content: String,
    pub is_volume: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct StoredTxtChapter {
    title: String,
    url: String,
    index: i32,
    start: usize,
    end: usize,
    /// 旧版索引里没有这个字段（issue #33 之前），缺省按 false 读，
    /// 不要让它把整份 `chapters.json` 变成无法反序列化。
    #[serde(default)]
    is_volume: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct StoredTxtIndex {
    book_url: String,
    name: String,
    file_name: String,
    byte_len: usize,
    char_len: usize,
    chapters: Vec<StoredTxtChapter>,
}

pub fn is_local_txt_origin(value: &str) -> bool {
    value.trim() == LOCAL_TXT_ORIGIN
}

pub fn is_local_txt_url(value: &str) -> bool {
    value.trim().starts_with("local-txt:")
}

pub fn build_chapter_url(book_url: &str, index: usize) -> String {
    format!("{}#{}", book_url.trim_end_matches('#'), index)
}

/// 章节区间(不含正文)。导入只需要它, 不必把整本正文按章复制一遍。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TxtChapterRange {
    pub title: String,
    pub url: String,
    pub index: i32,
    pub start: usize,
    pub end: usize,
    /// 卷标题（`第一卷` / `第二部` 之类），正文天然为空。
    ///
    /// 这类行要单独成章用于分卷导航，但**不能**被当成"正文解析失败"。legado 用
    /// 同一个概念（`BookChapter.isVolume`）把它渲染成居中的大标题空白页；
    /// 缺了这个标记，空正文会一路冒到前端，变成「书源正文解析为空」的假报错
    /// （issue #33）。
    pub is_volume: bool,
}

pub fn parse_txt_chapters(book_url: &str, text: &str) -> Vec<ParsedTxtChapter> {
    parse_txt_chapter_ranges(book_url, text)
        .into_iter()
        .map(|range| ParsedTxtChapter {
            content: text[range.start..range.end].to_string(),
            title: range.title,
            url: range.url,
            index: range.index,
            start: range.start,
            end: range.end,
            is_volume: range.is_volume,
        })
        .collect()
}

/// 只解析章节标题与字符区间, 不复制正文。
///
/// 导入路径用它: 旧实现顺手为每章 `to_string()` 一份正文却从不用(索引只存区间),
/// 一本几十 MB 的 TXT 等于白白多复制一整本书。
pub fn parse_txt_chapter_ranges(book_url: &str, text: &str) -> Vec<TxtChapterRange> {
    let Some(heading_re) = chapter_heading_regex() else {
        return fallback_range(book_url, text);
    };

    let mut headings: Vec<(usize, usize, String)> = Vec::new();
    let mut offset = 0usize;
    for line in text.split_inclusive('\n') {
        let raw_line = line.trim_end_matches(['\r', '\n']);
        let title = raw_line.trim();
        let content_len = raw_line.len();
        if is_chapter_heading(title, &heading_re) || is_bracketed_heading(title, &heading_re) {
            let leading_ws = raw_line.len().saturating_sub(raw_line.trim_start().len());
            headings.push((offset + leading_ws, offset + content_len, title.to_string()));
        }
        offset += line.len();
    }

    if offset < text.len() {
        let raw_line = &text[offset..];
        let title = raw_line.trim();
        if is_chapter_heading(title, &heading_re) || is_bracketed_heading(title, &heading_re) {
            let leading_ws = raw_line.len().saturating_sub(raw_line.trim_start().len());
            headings.push((offset + leading_ws, text.len(), title.to_string()));
        }
    }

    if headings.is_empty() {
        return fallback_range(book_url, text);
    }

    let mut chapters = Vec::new();
    let first_start = headings[0].0;
    if text[..first_start].trim().len() > 0 {
        chapters.push(TxtChapterRange {
            title: "序章".to_string(),
            url: build_chapter_url(book_url, 0),
            index: 0,
            start: 0,
            end: trim_chapter_end(text, first_start),
            is_volume: false,
        });
    }

    for (heading_index, (title_start, title_end, title)) in headings.iter().enumerate() {
        let content_start = skip_line_break(text, *title_end);
        let next_start = headings
            .get(heading_index + 1)
            .map(|(start, _, _)| *start)
            .unwrap_or(text.len());
        let end = trim_chapter_end(text, next_start).max(content_start);
        let index = chapters.len() as i32;
        // 只有"标题看起来是卷 + 区间里确实没有正文"才算卷章。单看空正文不够：
        // 「第一章」后面紧跟「第二章」时前一章也是空的，那多半是源文件本身缺内容，
        // 不能悄悄当成卷标题放过去（用户会以为是排版问题而不是书的问题）。
        let is_volume = end == content_start && is_volume_title(title);
        chapters.push(TxtChapterRange {
            title: title.clone(),
            url: build_chapter_url(book_url, index as usize),
            index,
            start: content_start,
            end,
            is_volume,
        });
        if *title_start >= text.len() {
            break;
        }
    }

    if chapters.is_empty() {
        fallback_range(book_url, text)
    } else {
        chapters
    }
}

pub fn decode_txt_bytes(bytes: &[u8]) -> String {
    if bytes.starts_with(&[0xEF, 0xBB, 0xBF]) {
        return String::from_utf8_lossy(&bytes[3..]).into_owned();
    }
    match std::str::from_utf8(bytes) {
        Ok(value) => value.to_string(),
        Err(_) => {
            let (decoded, _, _) = GB18030.decode(bytes);
            decoded.into_owned()
        }
    }
}

pub fn sanitize_txt_file_name(file_name: &str) -> String {
    let name = Path::new(file_name)
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("book.txt")
        .trim()
        .to_string();
    if name.is_empty() {
        "book.txt".to_string()
    } else {
        name
    }
}

pub fn book_name_from_file_name(file_name: &str) -> String {
    let safe = sanitize_txt_file_name(file_name);
    Path::new(&safe)
        .file_stem()
        .and_then(|value| value.to_str())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("本地小说")
        .to_string()
}

pub fn validate_txt_upload(file_name: &str, byte_len: usize) -> Result<(), AppError> {
    let safe = sanitize_txt_file_name(file_name);
    if !safe.to_lowercase().ends_with(".txt") {
        return Err(AppError::BadRequest("仅支持上传 .txt 文件".to_string()));
    }
    if byte_len == 0 {
        return Err(AppError::BadRequest("TXT 文件不能为空".to_string()));
    }
    if byte_len > MAX_TXT_UPLOAD_BYTES {
        return Err(AppError::BadRequest("TXT 文件不能超过 50MB".to_string()));
    }
    Ok(())
}

#[derive(Clone)]
pub struct LocalTxtBookService {
    storage_dir: PathBuf,
}

impl LocalTxtBookService {
    pub fn new(storage_dir: impl AsRef<Path>) -> Self {
        Self {
            storage_dir: storage_dir.as_ref().to_path_buf(),
        }
    }

    pub async fn import_txt_book(
        &self,
        user_ns: &str,
        file_name: &str,
        bytes: &[u8],
    ) -> Result<Book, AppError> {
        validate_txt_upload(file_name, bytes.len())?;
        let safe_file_name = sanitize_txt_file_name(file_name);
        let text = decode_txt_bytes(bytes);
        if text.trim().is_empty() {
            return Err(AppError::BadRequest("TXT 文件内容不能为空".to_string()));
        }

        let hash = md5_hex(&format!(
            "{}:{}:{}",
            user_ns,
            safe_file_name,
            md5_hex(&text)
        ));
        let book_url = format!("{}:{}", LOCAL_TXT_ORIGIN, hash);
        // 只要区间: 旧实现为每章复制一份正文却从不使用(索引只存 start/end)。
        let chapters = parse_txt_chapter_ranges(&book_url, &text);
        let book_dir = self.book_dir(user_ns, &book_url)?;
        fs::create_dir_all(&book_dir)
            .await
            .map_err(|e| AppError::Internal(e.into()))?;
        fs::write(book_dir.join("book.txt"), text.as_bytes())
            .await
            .map_err(|e| AppError::Internal(e.into()))?;

        let index = StoredTxtIndex {
            book_url: book_url.clone(),
            name: book_name_from_file_name(&safe_file_name),
            file_name: safe_file_name,
            byte_len: bytes.len(),
            char_len: text.chars().count(),
            chapters: chapters
                .into_iter()
                .map(|chapter| StoredTxtChapter {
                    title: chapter.title,
                    url: chapter.url,
                    index: chapter.index,
                    start: chapter.start,
                    end: chapter.end,
                    is_volume: chapter.is_volume,
                })
                .collect(),
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
            origin: LOCAL_TXT_ORIGIN.to_string(),
            origin_name: Some(LOCAL_TXT_ORIGIN_NAME.to_string()),
            toc_url: Some(book_url),
            can_update: Some(false),
            dur_chapter_index: Some(0),
            dur_chapter_pos: Some(0),
            total_chapter_num: Some(index.chapters.len() as i32),
            latest_chapter_title: index.chapters.last().map(|chapter| chapter.title.clone()),
            kind: Some("本地TXT".to_string()),
            word_count: Some(format!("{}字", index.char_len)),
            ..Book::default()
        })
    }

    pub async fn get_book_info(&self, user_ns: &str, book_url: &str) -> Result<Book, AppError> {
        let index = self.read_index(user_ns, book_url).await?;
        Ok(Book {
            name: index.name,
            author: "本地导入".to_string(),
            book_url: index.book_url.clone(),
            origin: LOCAL_TXT_ORIGIN.to_string(),
            origin_name: Some(LOCAL_TXT_ORIGIN_NAME.to_string()),
            toc_url: Some(index.book_url.clone()),
            can_update: Some(false),
            total_chapter_num: Some(index.chapters.len() as i32),
            latest_chapter_title: index.chapters.last().map(|chapter| chapter.title.clone()),
            kind: Some("本地TXT".to_string()),
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
                is_volume: chapter.is_volume,
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
            .find(|chapter| chapter.index == requested_index)
            .ok_or_else(|| AppError::BadRequest("章节不存在".to_string()))?;
        let text = fs::read_to_string(self.book_dir(user_ns, &book_url)?.join("book.txt"))
            .await
            .map_err(map_local_txt_read_error)?;
        if chapter.start > chapter.end || chapter.end > text.len() {
            return Err(AppError::BadRequest("章节索引无效".to_string()));
        }
        Ok(text[chapter.start..chapter.end].to_string())
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
        let hash = local_txt_hash_from_url(book_url)?;
        Ok(self.local_root(user_ns).join(hash))
    }

    async fn read_index(&self, user_ns: &str, book_url: &str) -> Result<StoredTxtIndex, AppError> {
        let path = self.book_dir(user_ns, book_url)?.join("chapters.json");
        let data = fs::read_to_string(path)
            .await
            .map_err(map_local_txt_read_error)?;
        let mut index: StoredTxtIndex =
            serde_json::from_str(&data).map_err(|e| AppError::BadRequest(e.to_string()))?;
        // 旧版索引没有 isVolume（issue #33 之前导入的书）。这里按与分章时相同的
        // 判据就地补标：空区间 + 卷级标题。不回写文件 —— 索引是只读缓存，回写会
        // 引入并发写入与版本兼容问题，而每次读取重算的成本可以忽略。
        for chapter in index.chapters.iter_mut() {
            if !chapter.is_volume && chapter.start >= chapter.end {
                chapter.is_volume = is_volume_title(&chapter.title);
            }
        }
        Ok(index)
    }
}

fn fallback_range(book_url: &str, text: &str) -> Vec<TxtChapterRange> {
    vec![TxtChapterRange {
        title: "正文".to_string(),
        url: build_chapter_url(book_url, 0),
        index: 0,
        start: 0,
        end: text.len(),
        is_volume: false,
    }]
}

fn chapter_heading_regex() -> Option<Regex> {
    Regex::new(r"^(?:第[\p{N}零〇一二两三四五六七八九十百千万壹贰叁肆伍陆柒捌玖拾佰仟]+[章节回卷部集篇]|卷[\p{N}零〇一二两三四五六七八九十百千万壹贰叁肆伍陆柒捌玖拾佰仟]+)(?:$|[\s:：、.．\-—].*|[^\s:：、.．\-—](?:.*[^。！？!?；;，,、])?)$").ok()
}

/// 整行被成对括号包住的标题，如 `【第一卷】`、`（第二章 初见）`、`「楔子」`。
///
/// 这类写法在中医/出版排版里很常见，裸正则因为开头是 `【` 匹配不上，整行会被
/// 当成正文吞进上一章。legado 有对应规则（`特殊符号 标题(成对)`，默认关闭）；
/// 这里只在**去掉括号后整行仍是一个标题**时才认，因此不会把 `【此处删除】`
/// 这种夹在正文里的提示语误判成分章。
fn is_bracketed_heading(line: &str, heading_re: &Regex) -> bool {
    const PAIRS: &[(char, char)] = &[
        ('【', '】'),
        ('〔', '〕'),
        ('〖', '〗'),
        ('「', '」'),
        ('『', '』'),
        ('《', '》'),
        ('〈', '〉'),
        ('（', '）'),
        ('(', ')'),
        ('[', ']'),
        ('［', '］'),
    ];
    let mut chars = line.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    let Some(last) = line.chars().next_back() else {
        return false;
    };
    if !PAIRS.iter().any(|(open, close)| *open == first && *close == last) {
        return false;
    }
    // `【】` 里除了标题不该再有换行/多余空白，直接取内层再走一遍标题判定。
    let inner = line[first.len_utf8()..line.len() - last.len_utf8()].trim();
    !inner.is_empty() && is_chapter_heading(inner, heading_re)
}

/// 标题本身是不是"卷/部/篇/集"这一层，而不是具体的"章/回/节"。
///
/// 用来决定空正文的章节是**卷标题页**还是**真正的解析异常**：只有卷级标题
/// 后面紧跟下一章（正文天然为空）才属于正常排版。
fn is_volume_title(line: &str) -> bool {
    // 先剥掉成对括号，`【第一卷】` 与 `第一卷` 判定一致。
    let trimmed = line.trim();
    let inner = trimmed
        .strip_prefix(['【', '〔', '〖', '「', '『', '《', '〈', '（', '(', '[', '［'])
        .and_then(|rest| {
            rest.strip_suffix(['】', '〕', '〗', '」', '』', '》', '〉', '）', ')', ']', '］'])
        })
        .unwrap_or(trimmed);
    let rest = inner.trim();
    // 卷名可能带副标题（`第一卷 风起云涌`），只看开头那一段。
    let head: String = rest
        .chars()
        .take_while(|ch| !ch.is_whitespace() && !matches!(ch, ':' | '：' | '、' | '.' | '．'))
        .collect();
    let is_digit = |ch: char| {
        ch.is_ascii_digit()
            || "零〇一二两三四五六七八九十百千万壹贰叁肆伍陆柒捌玖拾佰仟".contains(ch)
    };
    // 两种卷级写法都要认：
    //   `第一卷` / `第二部` —— 序号在单位前面（末位是单位字）
    //   `卷五`              —— 单位在序号前面（首位是单位字）
    let chars: Vec<char> = head.chars().collect();
    let ends_with_unit = chars
        .last()
        .is_some_and(|ch| matches!(ch, '卷' | '部' | '篇' | '集'));
    let starts_with_unit = chars.first() == Some(&'卷');
    if !ends_with_unit && !starts_with_unit {
        return false;
    }
    // 必须有数字部分，否则「卷」单独一个字不认。
    chars.iter().any(|ch| is_digit(*ch))
}

fn is_chapter_heading(line: &str, re: &Regex) -> bool {
    if line.is_empty() || line.chars().count() > 80 {
        return false;
    }
    re.is_match(line)
}

fn skip_line_break(text: &str, offset: usize) -> usize {
    let rest = &text[offset..];
    if rest.starts_with("\r\n") {
        offset + 2
    } else if rest.starts_with('\n') || rest.starts_with('\r') {
        offset + 1
    } else {
        offset
    }
}

fn trim_chapter_end(text: &str, mut end: usize) -> usize {
    while end > 0 {
        let prev = text[..end].chars().next_back();
        match prev {
            Some('\n') | Some('\r') => end -= prev.unwrap().len_utf8(),
            _ => break,
        }
    }
    end
}

fn parse_chapter_url(chapter_url: &str) -> Result<(String, i32), AppError> {
    let (book_url, raw_index) = chapter_url
        .rsplit_once('#')
        .ok_or_else(|| AppError::BadRequest("章节地址无效".to_string()))?;
    if !is_local_txt_url(book_url) {
        return Err(AppError::BadRequest("章节地址无效".to_string()));
    }
    let index = raw_index
        .parse::<i32>()
        .map_err(|_| AppError::BadRequest("章节序号无效".to_string()))?;
    Ok((book_url.to_string(), index))
}

fn local_txt_hash_from_url(book_url: &str) -> Result<&str, AppError> {
    let hash = book_url
        .strip_prefix("local-txt:")
        .filter(|value| {
            value.len() == LOCAL_TXT_HASH_LEN && value.chars().all(|ch| ch.is_ascii_hexdigit())
        })
        .ok_or_else(|| AppError::BadRequest("本地 TXT 地址无效".to_string()))?;
    Ok(hash)
}

fn map_local_txt_read_error(err: std::io::Error) -> AppError {
    if err.kind() == std::io::ErrorKind::NotFound {
        AppError::BadRequest("本地 TXT 不存在".to_string())
    } else {
        AppError::Internal(err.into())
    }
}
