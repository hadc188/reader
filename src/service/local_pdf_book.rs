use crate::error::error::AppError;
use crate::model::{book::Book, book_chapter::BookChapter};
use crate::util::hash::{md5_hex, md5_hex_bytes};
// PDF 解析统一走 pdf-extract 重导出的 lopdf: 与文本提取共用同一版本, 页序
// 只有一个来源, 章节页号和提取出的页文本不会错位。
use pdf_extract::{Dictionary, Document, Object, ObjectId};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use tokio::fs;

pub const LOCAL_PDF_ORIGIN: &str = "local-pdf";
pub const LOCAL_PDF_ORIGIN_NAME: &str = "本地 PDF";
pub const MAX_PDF_UPLOAD_BYTES: usize = 100 * 1024 * 1024;
const LOCAL_BOOK_DIR: &str = "local_books";
const LOCAL_PDF_HASH_LEN: usize = 32;
const MIN_DOUBLED_RENDER_PAIRS: usize = 3;
/// 无书签且正文里也认不出章节标题时, 按固定页数分段 (Legado 用 10,
/// 大书取 20 平衡目录长度)
pub const PAGES_PER_CHAPTER: usize = 20;
/// chapters.json 结构/缓存语义版本。4 = 在 3 (导入期全量提取 + 章节级文本
/// 缓存 chapters/{i}.txt) 基础上把提取行重组为段落 (merge_wrapped_lines)。
/// 更早版本: <3 按页缓存在 pages/ 且章节切分规则不同, 3 行未合并段落,
/// 读取时按本版本重新提取。
const EXTRACTOR_VERSION: u32 = 4;
/// 书签总条目/层级深度上限, 防损坏或恶意 PDF 的书签环拖垮导入。
/// 同层兄弟 (Next 链) 走循环不占栈, 深度只计 First 嵌套层级, 因此可以
/// 收得很紧; 环由 visited 去重截断。
const MAX_OUTLINE_ITEMS: usize = 10_000;
const MAX_OUTLINE_DEPTH: usize = 64;

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
    #[serde(default)]
    page_count: usize,
    #[serde(default)]
    extractor_version: u32,
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

/// 从已解析的文档提取指定页范围的文本。
///
/// 走 vendor 副本新增的 `extract_text_for_page_ids`: 整段共享一个解析器,
/// 字体与 CMap 只解析一次 —— 上游的逐页接口每页重建, 1300 页中文书要 70s+,
/// 共享后同样内容 1.4s。pdf-extract 内部仍有 panic!/todo! 分支 (罕见编码),
/// 用 catch_unwind 兜底, 失败时退回按页逐个提取, 单页失败只丢那一页。
fn extract_pages_from_doc(doc: &Document, page_ids: &[ObjectId]) -> Vec<String> {
    let extracted = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        pdf_extract::extract_text_for_page_ids(doc, page_ids)
    }));
    match extracted {
        Ok(Ok(pages)) => return pages,
        Ok(Err(e)) => tracing::warn!("PDF 批量提取失败, 退回逐页: {e:?}"),
        Err(_) => tracing::warn!("PDF 批量提取时 panic, 退回逐页"),
    }
    page_ids
        .iter()
        .map(|id| {
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                pdf_extract::extract_text_for_page_ids(doc, std::slice::from_ref(id))
            }))
            .ok()
            .and_then(|r| r.ok())
            .and_then(|mut v| v.pop())
            .unwrap_or_default()
        })
        .collect()
}

/// 载入 PDF 并提取指定页区间 (闭区间, 越界自动收敛)。CPU 密集, 交给
/// blocking 线程池。
async fn extract_page_range_blocking(
    bytes: Vec<u8>,
    start: usize,
    end: usize,
) -> Result<Vec<String>, AppError> {
    tokio::task::spawn_blocking(move || {
        let doc = load_pdf(&bytes)?;
        let ids = collect_page_ids(&doc);
        let lo = start.min(ids.len());
        let hi = (end + 1).min(ids.len());
        if lo >= hi {
            return Ok(Vec::new());
        }
        Ok(extract_pages_from_doc(&doc, &ids[lo..hi])
            .iter()
            .map(|p| normalize_page_text(p))
            .collect())
    })
    .await
    .map_err(|e| AppError::Internal(anyhow::anyhow!("PDF 提取任务失败: {e}")))?
}

fn load_pdf(bytes: &[u8]) -> Result<Document, AppError> {
    Document::load_mem(bytes)
        .map_err(|e| AppError::BadRequest(format!("无法解析 PDF 文件: {e}")))
}

/// CJK 统一表意文字 (含扩展A/兼容表意), 双重渲染折叠只对汉字生效,
/// 避免误伤标点连用 (如中文省略号的多个句号)。
fn is_cjk_ideograph(c: char) -> bool {
    matches!(c,
        '\u{4E00}'..='\u{9FFF}'
        | '\u{3400}'..='\u{4DBF}'
        | '\u{F900}'..='\u{FAFF}'
        | '\u{20000}'..='\u{2A6DF}')
}

/// 折叠「双重渲染」文本: 部分 PDF 的标题用 fill+stroke 两遍绘制, 每个汉字
/// 在内容流里画两次, 提取出「第第一一回回」式交错叠字。双重渲染的标题总是
/// 独占一行, 因此只在「整行仅由汉字叠字对与空白构成、对数达到
/// MIN_DOUBLED_RENDER_PAIRS」时折叠该行; 正文里混在其他文字中的正常叠词
/// (「谢谢」「高高兴兴平平安安」) 不受影响。
fn fold_doubled_render_text(text: &str) -> String {
    let folded: Vec<String> = text.lines().map(fold_if_pure_doubled_line).collect();
    folded.join("\n")
}

fn fold_if_pure_doubled_line(line: &str) -> String {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return line.to_string();
    }
    let chars: Vec<char> = trimmed.chars().collect();
    let mut pairs = 0usize;
    let mut i = 0;
    let mut pure = true;
    while i < chars.len() {
        let c = chars[i];
        if c.is_whitespace() {
            i += 1;
            continue;
        }
        if i + 1 < chars.len() && chars[i + 1] == c && is_cjk_ideograph(c) {
            pairs += 1;
            i += 2;
            continue;
        }
        pure = false;
        break;
    }
    if !pure || pairs < MIN_DOUBLED_RENDER_PAIRS {
        return line.to_string();
    }
    let mut out = String::with_capacity(trimmed.len());
    let mut j = 0;
    while j < chars.len() {
        let c = chars[j];
        if c.is_whitespace() {
            out.push(c);
            j += 1;
            continue;
        }
        out.push(c);
        j += 2;
    }
    out
}

/// 单页文本后处理: 折叠双重渲染叠字, 规整首尾空白。
fn normalize_page_text(text: &str) -> String {
    fold_doubled_render_text(text.trim())
}

/// PDF 书签项 (扁平深度优先序列, 阅读器目录是一维的)
#[derive(Debug, Clone)]
struct OutlineItem {
    title: String,
    page_index: usize,
}

/// 页序索引: 有序页 id 列表 + 反查表, 书签解析全程复用, 避免每条书签
/// 重新遍历页树。
struct PageIndex {
    ids: Vec<ObjectId>,
    by_id: HashMap<ObjectId, usize>,
}

impl PageIndex {
    fn new(ids: Vec<ObjectId>) -> Self {
        let by_id = ids.iter().enumerate().map(|(idx, id)| (*id, idx)).collect();
        Self { ids, by_id }
    }
}

/// 解析 PDF 书签树 (/Root/Outlines)。深度优先扁平化, 无书签/损坏返回空。
fn extract_outline(doc: &Document, page_ids: &[ObjectId]) -> Vec<OutlineItem> {
    let mut items = Vec::new();
    let pages = PageIndex::new(page_ids.to_vec());
    let Ok(catalog) = doc.catalog() else {
        return items;
    };
    let Ok(outlines_ref) = catalog.get(b"Outlines") else {
        return items;
    };
    let Some(outlines) = deref_dict(doc, outlines_ref) else {
        return items;
    };
    let Ok(first) = outlines.get(b"First") else {
        return items;
    };
    let mut visited = HashSet::new();
    walk_outline_siblings(doc, first, &pages, &mut visited, &mut items, 0);
    items
}

/// 遍历一层书签: 沿 Next 链循环 (兄弟数不限, 不占栈), 只对 First 子树
/// 递归, 递归深度即书签层级深度。
fn walk_outline_siblings(
    doc: &Document,
    first: &Object,
    pages: &PageIndex,
    visited: &mut HashSet<ObjectId>,
    out: &mut Vec<OutlineItem>,
    depth: usize,
) {
    if depth > MAX_OUTLINE_DEPTH {
        return;
    }
    let mut cursor: Option<Object> = Some(first.clone());
    while let Some(node) = cursor.take() {
        if out.len() >= MAX_OUTLINE_ITEMS {
            return;
        }
        let Ok((Some(node_id), Object::Dictionary(dict))) = doc.dereference(&node) else {
            return;
        };
        if !visited.insert(node_id) {
            return;
        }
        let title = dict
            .get(b"Title")
            .ok()
            .and_then(|o| match o {
                Object::String(bytes, _) => Some(decode_pdf_text_string(bytes)),
                _ => None,
            })
            .unwrap_or_default();
        let page_index = outline_item_page(doc, dict, pages)
            .and_then(|page_id| pages.by_id.get(&page_id).copied());
        if let Some(page_index) = page_index {
            let trimmed = title.trim();
            if !trimmed.is_empty() {
                out.push(OutlineItem {
                    title: trimmed.to_string(),
                    page_index,
                });
            }
        }
        if let Ok(child) = dict.get(b"First") {
            walk_outline_siblings(doc, child, pages, visited, out, depth + 1);
        }
        cursor = dict.get(b"Next").ok().cloned();
    }
}

/// 书签项目标页: /Dest 或 /A (GoTo action 的 /D)
fn outline_item_page(doc: &Document, dict: &Dictionary, pages: &PageIndex) -> Option<ObjectId> {
    if let Ok(dest) = dict.get(b"Dest") {
        return dest_page_from(doc, dest, pages);
    }
    let action = dict.get(b"A").ok()?;
    let action_dict = deref_dict(doc, action)?;
    let dest = action_dict.get(b"D").ok()?;
    dest_page_from(doc, dest, pages)
}

fn dest_page_from(doc: &Document, dest: &Object, pages: &PageIndex) -> Option<ObjectId> {
    match doc.dereference(dest) {
        Ok((_, Object::Array(arr))) => arr.first().and_then(|o| match o {
            Object::Reference(id) => Some(*id),
            // 遗留内联页号 (PDF 1.1 风格, 0-based)
            Object::Integer(n) => usize::try_from(*n).ok().and_then(|n| pages.ids.get(n).copied()),
            _ => None,
        }),
        Ok((_, Object::String(name, _))) => resolve_named_dest(doc, name, pages),
        Ok((_, Object::Dictionary(d))) => {
            d.get(b"D").ok().and_then(|d| dest_page_from(doc, d, pages))
        }
        _ => None,
    }
}

/// 命名目标: /Root/Dests (PDF 1.1) 或 /Root/Names/Dests name tree (PDF 1.2+)。
/// name tree 只查本节点平铺的 Names 数组, 不递归 Kids。
fn resolve_named_dest(doc: &Document, name: &[u8], pages: &PageIndex) -> Option<ObjectId> {
    let catalog = doc.catalog().ok()?;
    if let Ok(dests_ref) = catalog.get(b"Dests") {
        if let Some(dests) = deref_dict(doc, dests_ref) {
            if let Ok(value) = dests.get(name) {
                if let Some(page) = dest_page_from(doc, value, pages) {
                    return Some(page);
                }
            }
        }
    }
    let names = deref_dict(doc, catalog.get(b"Names").ok()?)?;
    let dests = deref_dict(doc, names.get(b"Dests").ok()?)?;
    let Ok(Object::Array(pairs)) = dests.get(b"Names") else {
        return None;
    };
    let mut i = 0;
    while i + 1 < pairs.len() {
        if matches!(&pairs[i], Object::String(b, _) if b == name) {
            return dest_page_from(doc, &pairs[i + 1], pages);
        }
        i += 2;
    }
    None
}

fn deref_dict<'a>(doc: &'a Document, obj: &'a Object) -> Option<&'a Dictionary> {
    match doc.dereference(obj) {
        Ok((_, Object::Dictionary(d))) => Some(d),
        _ => None,
    }
}

/// PDF 文本字符串解码: UTF-16BE (BOM FE FF) 或 PDFDocEncoding (按 latin1 近似)
fn decode_pdf_text_string(bytes: &[u8]) -> String {
    if bytes.len() >= 2 && bytes[0] == 0xFE && bytes[1] == 0xFF {
        let units: Vec<u16> = bytes[2..]
            .chunks_exact(2)
            .map(|c| ((c[0] as u16) << 8) | c[1] as u16)
            .collect();
        String::from_utf16(&units).unwrap_or_default()
    } else {
        bytes.iter().map(|&b| b as char).collect()
    }
}

/// 正文里识别章节标题行。很多 PDF 不带书签 (Outlines), 只能从排版后的
/// 文字里认标题: 独占一行、以「第…回/章/节/卷/篇」开头且不长。
///
/// 目录页会把所有回目列成密集的一片, 因此单页命中过多时整页判为目录跳过;
/// 同名标题只认第一次出现 (目录漏网 + 页眉重复)。
fn detect_headings(pages: &[String]) -> Vec<OutlineItem> {
    /// 一页里出现这么多标题行, 判定为目录页而非正文
    const MAX_HEADINGS_PER_PAGE: usize = 3;
    const MAX_HEADING_CHARS: usize = 40;

    let mut items: Vec<OutlineItem> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    for (page_index, text) in pages.iter().enumerate() {
        let hits: Vec<String> = text
            .lines()
            .map(str::trim)
            .filter(|line| is_heading_line(line, MAX_HEADING_CHARS))
            .map(str::to_string)
            .collect();
        if hits.len() > MAX_HEADINGS_PER_PAGE {
            continue;
        }
        for title in hits {
            if seen.insert(title.clone()) {
                items.push(OutlineItem { title, page_index });
            }
        }
    }
    items
}

fn is_heading_line(line: &str, max_chars: usize) -> bool {
    let mut chars = line.chars().peekable();
    if chars.next() != Some('第') {
        return false;
    }
    // PDF 排版常在字间插空格 (「第 12 章」), 数字前后都要容忍空白
    while chars.peek().is_some_and(|c| is_layout_space(*c)) {
        chars.next();
    }
    let mut digits = 0usize;
    while chars.peek().is_some_and(|c| is_chapter_digit(*c)) {
        chars.next();
        digits += 1;
        if digits > 12 {
            return false;
        }
    }
    if digits == 0 {
        return false;
    }
    while chars.peek().is_some_and(|c| is_layout_space(*c)) {
        chars.next();
    }
    if !matches!(
        chars.next(),
        Some('回' | '章' | '节' | '卷' | '篇' | '集' | '部' | '话')
    ) {
        return false;
    }
    // 量词后要么结束, 要么跟空白分隔的标题; 「第一回合他就赢了」这类
    // 正文句子会因为紧跟非空白而被排除
    match chars.next() {
        None => true,
        Some(c) if is_layout_space(c) => line.chars().count() <= max_chars,
        _ => false,
    }
}

fn is_layout_space(c: char) -> bool {
    c.is_whitespace() || c == '\u{3000}'
}

fn is_chapter_digit(c: char) -> bool {
    c.is_ascii_digit()
        || matches!(
            c,
            '〇' | '零' | '一' | '二' | '三' | '四' | '五' | '六' | '七' | '八' | '九'
                | '十' | '百' | '千' | '万' | '两'
        )
}

/// 丢弃没有任何文字的章节并重新编号。封面图页、插页会被切成空章,
/// 打开就是一片空白 —— 与其让读者点进去报错, 不如目录里就不出现。
fn drop_blank_chapters(
    book_url: &str,
    chapters: Vec<StoredPdfChapter>,
    pages: &[String],
) -> Vec<StoredPdfChapter> {
    chapters
        .into_iter()
        .filter(|c| !join_pages(pages, c.page_start, c.page_end).trim().is_empty())
        .enumerate()
        .map(|(index, chapter)| StoredPdfChapter {
            url: build_chapter_url(book_url, index),
            index: index as i32,
            ..chapter
        })
        .collect()
}

/// 章节合成: 有目录项 (书签或正文识别) 就按它切分, 未覆盖的前导页
/// 合成引导章; 都没有则按 PAGES_PER_CHAPTER 页一章。
fn build_chapters(
    book_url: &str,
    page_count: usize,
    outline: &[OutlineItem],
) -> Vec<StoredPdfChapter> {
    let mut chapters: Vec<StoredPdfChapter> = Vec::new();
    if page_count == 0 {
        return chapters;
    }
    // 书签顺序不保证单调 (附录/前言常指回前面的页), 按页号稳定排序后
    // 再切分, 否则回跳项会把前一章压成单页、中间页无人认领。
    let mut items: Vec<(String, usize)> = outline
        .iter()
        .filter(|item| item.page_index < page_count)
        .map(|item| (item.title.clone(), item.page_index))
        .collect();
    items.sort_by_key(|(_, page)| *page);

    let push_chapter = |chapters: &mut Vec<StoredPdfChapter>, title: String, start: usize, end: usize| {
        let end = end.min(page_count - 1).max(start);
        chapters.push(StoredPdfChapter {
            title,
            url: build_chapter_url(book_url, chapters.len()),
            index: chapters.len() as i32,
            page_start: start,
            page_end: end,
        });
    };

    if !items.is_empty() {
        // 书签前的封面/版权/目录页合成引导章
        let first_start = items[0].1;
        if first_start > 0 {
            push_chapter(&mut chapters, format!("开头 (P1-P{first_start})"), 0, first_start - 1);
        }
        for (i, (title, start)) in items.iter().enumerate() {
            let end = items
                .get(i + 1)
                .map(|(_, next_start)| next_start.saturating_sub(1))
                .unwrap_or(page_count - 1);
            push_chapter(&mut chapters, title.clone(), *start, end);
        }
    } else {
        let mut start = 0;
        while start < page_count {
            let end = (start + PAGES_PER_CHAPTER - 1).min(page_count - 1);
            let seq = chapters.len() + 1;
            push_chapter(
                &mut chapters,
                format!("第 {seq} 部分 (P{}-P{})", start + 1, end + 1),
                start,
                end,
            );
            start = end + 1;
        }
    }
    chapters
}

/// 页序的唯一来源: 章节页号、正文提取、书签页映射都走这里, 三者不会错位。
///
/// 主路径是 lopdf 的 `get_pages()`; 它拿不到页时 (页对象缺 /Type 等不规范
/// 写法) 再手动走 /Root/Pages, 对无类型节点按「有 Kids 就是中间节点」推断。
fn collect_page_ids(doc: &Document) -> Vec<ObjectId> {
    let via_page_tree: Vec<ObjectId> = doc.get_pages().values().copied().collect();
    if !via_page_tree.is_empty() {
        return via_page_tree;
    }
    let Ok(catalog) = doc.catalog() else {
        return via_page_tree;
    };
    let Ok(pages_ref) = catalog.get(b"Pages") else {
        return via_page_tree;
    };
    let mut pages = Vec::new();
    let mut visited = HashSet::new();
    collect_page_ids_inner(doc, pages_ref, &mut visited, &mut pages, 0);
    pages
}

fn collect_page_ids_inner(
    doc: &Document,
    node: &Object,
    visited: &mut HashSet<ObjectId>,
    out: &mut Vec<ObjectId>,
    depth: usize,
) {
    const MAX_TREE_DEPTH: usize = 128;
    const MAX_TREE_NODES: usize = 100_000;
    if depth > MAX_TREE_DEPTH || out.len() >= MAX_TREE_NODES {
        return;
    }
    let Ok((node_id, obj)) = doc.dereference(node) else {
        return;
    };
    // 一个数组本身不是节点: 元素应逐个处理
    if let Object::Array(arr) = obj {
        for item in arr {
            collect_page_ids_inner(doc, item, visited, out, depth);
        }
        return;
    }
    let Object::Dictionary(dict) = obj else {
        return;
    };
    if let Some(id) = node_id {
        if !visited.insert(id) {
            return;
        }
    }
    let type_name = dict
        .get(b"Type")
        .ok()
        .and_then(|o| o.as_name().ok())
        .map(|n| n.to_vec());
    match type_name.as_deref() {
        Some(b"Page") => {
            if let Some(id) = node_id {
                out.push(id);
            }
        }
        Some(b"Pages") => {
            if let Ok(kids) = dict.get(b"Kids") {
                collect_page_ids_inner(doc, kids, visited, out, depth + 1);
            }
        }
        _ => {
            // 无 /Type 的节点: 带 Kids 视为中间节点, 否则视为页
            if dict.get(b"Kids").is_ok() {
                if let Ok(kids) = dict.get(b"Kids") {
                    collect_page_ids_inner(doc, kids, visited, out, depth + 1);
                }
            } else if let Some(id) = node_id {
                out.push(id);
            }
        }
    }
}

/// 提取第一页上面积最大的 DCTDecode (JPEG) 图像作封面。JPEG 流的原始字节
/// 就是完整 JPEG 文件, 可直接落盘; 其他编码 (Flate 原始像素等) 需要重新
/// 编码, 不在范围内 —— 返回 None 让前端走默认封面。
fn extract_cover(doc: &Document) -> Option<(Vec<u8>, &'static str)> {
    let first_page_id = *collect_page_ids(doc).first()?;
    let page_obj = doc.get_object(first_page_id).ok()?;
    let page_dict = match page_obj {
        Object::Stream(s) => &s.dict,
        Object::Dictionary(d) => d,
        _ => return None,
    };
    let resources = deref_dict(doc, page_dict.get(b"Resources").ok()?)?;
    let xobjects = deref_dict(doc, resources.get(b"XObject").ok()?)?;
    let mut best: Option<(i64, Vec<u8>)> = None;
    for (_name, obj) in xobjects.iter() {
        let Ok((_, Object::Stream(stream))) = doc.dereference(obj) else {
            continue;
        };
        let subtype = stream.dict.get(b"Subtype").ok().and_then(|o| o.as_name().ok());
        if subtype != Some(b"Image".as_slice()) {
            continue;
        }
        let is_jpeg = match stream.dict.get(b"Filter") {
            Ok(Object::Name(n)) => n == b"DCTDecode",
            Ok(Object::Array(filters)) => filters
                .iter()
                .any(|f| matches!(f, Object::Name(n) if n == b"DCTDecode")),
            _ => false,
        };
        if !is_jpeg {
            continue;
        }
        let width = stream.dict.get(b"Width").ok().and_then(|o| o.as_i64().ok()).unwrap_or(0);
        let height = stream.dict.get(b"Height").ok().and_then(|o| o.as_i64().ok()).unwrap_or(0);
        let area = width * height;
        if area <= 0 || best.as_ref().is_some_and(|(a, _)| *a >= area) {
            continue;
        }
        best = Some((area, stream.content.clone()));
    }
    best.map(|(_, bytes)| (bytes, "jpg"))
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

    /// 导入: 解析页数/书签/封面, 并一次性提取全部页文本 (共享解析器,
    /// 千页中文书约 1.5 秒), 按章节落盘。之后翻页全是缓存直读。
    /// 顺带用全文补一件书签做不到的事: PDF 不带书签时从正文认出回目标题。
    pub async fn import_pdf_book(
        &self,
        user_ns: &str,
        file_name: &str,
        bytes: &[u8],
    ) -> Result<Book, AppError> {
        validate_pdf_upload(file_name, bytes.len())?;
        let safe_file_name = sanitize_pdf_file_name(file_name);

        let owned = bytes.to_vec();
        let (pages, outline, cover) = tokio::task::spawn_blocking(move || {
            let doc = load_pdf(&owned)?;
            let page_ids = collect_page_ids(&doc);
            let cover = extract_cover(&doc);
            let pages: Vec<String> = extract_pages_from_doc(&doc, &page_ids)
                .iter()
                .map(|p| normalize_page_text(p))
                .collect();
            // 书签是权威目录; 没有书签才退回从正文认标题
            let mut outline = extract_outline(&doc, &page_ids);
            if outline.is_empty() {
                outline = detect_headings(&pages);
            }
            Ok::<_, AppError>((pages, outline, cover))
        })
        .await
        .map_err(|e| AppError::Internal(anyhow::anyhow!("PDF 解析任务失败: {e}")))??;

        let page_count = pages.len();
        if page_count == 0 {
            return Err(AppError::BadRequest("PDF 文件没有可读页面".to_string()));
        }
        if pages.iter().all(|p| p.trim().is_empty()) {
            return Err(AppError::BadRequest(
                "PDF 文件未提取到任何文本内容（可能是扫描件/图片 PDF）".to_string(),
            ));
        }

        // hash 基于文件字节: 同一文件重复导入幂等覆盖
        let hash = md5_hex(&format!(
            "{}:{}:{}",
            user_ns,
            safe_file_name,
            md5_hex_bytes(bytes)
        ));
        let book_url = format!("{}:{}", LOCAL_PDF_ORIGIN, hash);
        let chapters = drop_blank_chapters(
            &book_url,
            build_chapters(&book_url, page_count, &outline),
            &pages,
        );
        if chapters.is_empty() {
            return Err(AppError::BadRequest(
                "PDF 文件未提取到任何可读章节".to_string(),
            ));
        }

        let book_dir = self.book_dir(user_ns, &book_url)?;
        fs::create_dir_all(book_dir.join("chapters"))
            .await
            .map_err(|e| AppError::Internal(e.into()))?;
        fs::write(book_dir.join("book.pdf"), bytes)
            .await
            .map_err(|e| AppError::Internal(e.into()))?;
        if let Some((cover_bytes, ext)) = &cover {
            let _ = fs::write(book_dir.join(format!("cover.{ext}")), cover_bytes).await;
        }
        for chapter in &chapters {
            let text = join_pages(&pages, chapter.page_start, chapter.page_end);
            fs::write(chapter_text_path(&book_dir, chapter.index), text)
                .await
                .map_err(|e| AppError::Internal(e.into()))?;
        }

        let index = StoredPdfIndex {
            book_url: book_url.clone(),
            name: book_name_from_file_name(&safe_file_name),
            file_name: safe_file_name,
            byte_len: bytes.len(),
            char_len: pages.iter().map(|p| p.chars().count()).sum(),
            page_count,
            extractor_version: EXTRACTOR_VERSION,
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
            cover_url: cover.as_ref().map(|(_, ext)| {
                format!("/local-pdf/{hash}/cover.{ext}")
            }),
            toc_url: Some(book_url),
            can_update: Some(false),
            dur_chapter_index: Some(0),
            dur_chapter_pos: Some(0),
            total_chapter_num: Some(index.chapters.len() as i32),
            latest_chapter_title: index.chapters.last().map(|c| c.title.clone()),
            kind: Some("本地PDF".to_string()),
            word_count: Some(format!("{}字", index.char_len)),
            ..Book::default()
        })
    }

    pub async fn get_book_info(&self, user_ns: &str, book_url: &str) -> Result<Book, AppError> {
        let index = self.read_index(user_ns, book_url).await?;
        let hash = book_url
            .strip_prefix(&format!("{LOCAL_PDF_ORIGIN}:"))
            .unwrap_or_default()
            .to_string();
        let pages = display_page_count(&index);
        Ok(Book {
            name: index.name,
            author: "本地导入".to_string(),
            book_url: index.book_url.clone(),
            origin: LOCAL_PDF_ORIGIN.to_string(),
            origin_name: Some(LOCAL_PDF_ORIGIN_NAME.to_string()),
            cover_url: self.cover_url_for(user_ns, &hash).await,
            toc_url: Some(index.book_url.clone()),
            can_update: Some(false),
            total_chapter_num: Some(index.chapters.len() as i32),
            latest_chapter_title: index.chapters.last().map(|c| c.title.clone()),
            kind: Some("本地PDF".to_string()),
            word_count: Some(if index.char_len > 0 {
                format!("{}字", index.char_len)
            } else {
                format!("{pages} 页")
            }),
            ..Book::default()
        })
    }

    /// 目录里已存的封面文件名 (cover.jpg 等), 供 get_book_info 回填 cover_url
    async fn cover_url_for(&self, user_ns: &str, hash: &str) -> Option<String> {
        if hash.is_empty() {
            return None;
        }
        let dir = self.local_root(user_ns).join(hash);
        for ext in ["jpg", "png", "jp2"] {
            if dir.join(format!("cover.{ext}")).exists() {
                return Some(format!("/local-pdf/{hash}/cover.{ext}"));
            }
        }
        None
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

    /// 章节正文: 命中章节缓存时零解析直读。缓存缺失 (导入被打断、文件被
    /// 清理) 时只重提取本章覆盖的页并回填, 不牵动整本。
    ///
    /// 旧版本索引 (extractor_version < 4, 行未合并段落或切分规则不同) 不能
    /// 借用其缓存, 也不就地升级版本号 (那需要重切章节、改动目录); 每次读
    /// 都重提取本章。用户重新导入同一文件即可升级 (hash 幂等覆盖)。
    pub async fn get_content(&self, user_ns: &str, chapter_url: &str) -> Result<String, AppError> {
        let (book_url, requested_index) = parse_chapter_url(chapter_url)?;
        let index = self.read_index(user_ns, &book_url).await?;
        let chapter = index
            .chapters
            .iter()
            .find(|c| c.index == requested_index)
            .ok_or_else(|| AppError::BadRequest("章节不存在".to_string()))?;
        let book_dir = self.book_dir(user_ns, &book_url)?;

        if index.extractor_version >= EXTRACTOR_VERSION {
            if let Ok(text) = fs::read_to_string(chapter_text_path(&book_dir, chapter.index)).await
            {
                return ensure_non_blank(text);
            }
        }

        let bytes = fs::read(book_dir.join("book.pdf"))
            .await
            .map_err(map_local_pdf_read_error)?;
        let pages = extract_page_range_blocking(bytes, chapter.page_start, chapter.page_end).await?;
        let text = join_pages(&pages, 0, pages.len().saturating_sub(1));
        let _ = fs::create_dir_all(book_dir.join("chapters")).await;
        let _ = fs::write(chapter_text_path(&book_dir, chapter.index), &text).await;
        ensure_non_blank(text)
    }

    /// 读取已落盘的封面文件 (protocol.rs 的 /local-pdf/ 路由调用)
    pub async fn read_cover_file(
        &self,
        user_ns: &str,
        hash: &str,
        file_name: &str,
    ) -> Result<(Vec<u8>, String), AppError> {
        let book_url = format!("{LOCAL_PDF_ORIGIN}:{hash}");
        let hash = local_pdf_hash_from_url(&book_url)?;
        let ext = file_name.rsplit('.').next().unwrap_or_default();
        let content_type = match ext {
            "jpg" | "jpeg" => "image/jpeg",
            "png" => "image/png",
            "jp2" => "image/jp2",
            _ => return Err(AppError::BadRequest("不支持的封面格式".to_string())),
        };
        let bytes = fs::read(self.local_root(user_ns).join(hash).join(file_name))
            .await
            .map_err(map_local_pdf_read_error)?;
        Ok((bytes, content_type.to_string()))
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

fn chapter_text_path(book_dir: &Path, chapter_index: i32) -> PathBuf {
    book_dir
        .join("chapters")
        .join(format!("{chapter_index:05}.txt"))
}

/// 索引里的页数: 新数据存 page_count, 旧数据从最后一章推
fn display_page_count(index: &StoredPdfIndex) -> usize {
    if index.page_count > 0 {
        index.page_count
    } else {
        index.chapters.last().map(|c| c.page_end + 1).unwrap_or(0)
    }
}

/// 段落重组 —— 把 PDF 提取出的「每视觉行一段」拼回真正的段落。
///
/// PDF 排版只有行的概念没有段的概念, 提取出的文本每一视觉行一个换行,
/// 直接展示就是一句被拆成几段。判据有三层:
/// 1. 行宽信号: 写满整行 (≥ 85% 栏宽) 且行尾无句读 → 排版换行, 与下一行
///    拼回同段; 没写满的短行是段落末行 (或诗行/对话), 断段。
/// 2. 句读信号: 行尾是句末标点 (。!?… 及其后的收尾引号) → 断段。宁可断在
///    句界, 也绝不把句子从中间截断。
/// 3. 结构信号: 章节标题行 (第X回/章…)、页码行、空行永远独立成段;
///    行首缩进 (全角空格开头) 之前断段。
///
/// 栏宽按本章非短行行宽的 P85 估算 (正文行大多写满整行)。栏宽过窄
/// (< MIN_MERGE_COLUMN_WIDTH, 如整章诗行) 时判定不可信, 放弃合并维持
/// 逐行分段; 混排在正文章节里的诗句/对话行宽必然远小于栏宽, 不受影响。
fn merge_wrapped_lines(text: &str) -> String {
    /// 整行判定阈值: 行宽达到栏宽的这个比例视为「写满」
    const FULL_LINE_RATIO: f64 = 0.85;
    const MIN_COLUMN_SAMPLE_WIDTH: f64 = 4.0;
    /// 启用合并的最小栏宽: 常见书页正文栏约 25-40 全角字宽, 诗行/竖排等
    /// 窄栏说明栏宽估计是在一串等宽短行上做的, 不可信
    const MIN_MERGE_COLUMN_WIDTH: f64 = 16.0;

    let lines: Vec<&str> = text.lines().collect();
    let column = estimate_column_width(&lines, MIN_COLUMN_SAMPLE_WIDTH);
    let full_line_width = if column < MIN_MERGE_COLUMN_WIDTH {
        f64::INFINITY
    } else {
        column * FULL_LINE_RATIO
    };

    let mut paragraphs: Vec<String> = Vec::new();
    let mut pending: Option<String> = None;
    for line in &lines {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            if let Some(p) = pending.take() {
                paragraphs.push(p);
            }
            continue;
        }
        if is_standalone_line(trimmed) {
            if let Some(p) = pending.take() {
                paragraphs.push(p);
            }
            paragraphs.push(trimmed.to_string());
            continue;
        }
        // 行首缩进检测必须看原始行: trim() 会把全角空格一并剥掉
        let indented = line.starts_with('\u{3000}') || line.starts_with("  ");
        let merge_with_prev = pending.as_ref().is_some_and(|prev| {
            !indented
                && !ends_with_sentence_terminal(prev)
                && line_width_units(prev) >= full_line_width
        });
        if merge_with_prev {
            let prev = pending.as_mut().expect("is_some_and 已确认");
            if needs_ascii_join(prev, trimmed) {
                prev.push(' ');
            }
            prev.push_str(trimmed);
        } else {
            if let Some(p) = pending.take() {
                paragraphs.push(p);
            }
            pending = Some(trimmed.to_string());
        }
    }
    if let Some(p) = pending.take() {
        paragraphs.push(p);
    }
    paragraphs.join("\n")
}

/// 估算版心栏宽 (全角字宽单位): 正文行大多写满整行, 取高分位 (P85) 抗
/// 标题/段落末行等短行干扰。可用行不足时返回 0, 由调用方决定不启用合并。
fn estimate_column_width(lines: &[&str], min_sample_width: f64) -> f64 {
    let mut widths: Vec<f64> = lines
        .iter()
        .map(|l| line_width_units(l.trim()))
        .filter(|w| *w >= min_sample_width)
        .collect();
    if widths.is_empty() {
        return 0.0;
    }
    widths.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let idx = (widths.len() * 85 / 100).min(widths.len() - 1);
    widths[idx]
}

/// 字宽单位: CJK 与全角字符计 1, 半角字符与空格计 0.5 (近似排版宽度)
fn char_width_units(c: char) -> f64 {
    if c >= '\u{2E80}' || c == '\u{3000}' {
        1.0
    } else {
        0.5
    }
}

fn line_width_units(line: &str) -> f64 {
    line.chars().map(char_width_units).sum()
}

/// 行尾是否句末标点: 先剥掉收尾引号/括号, 看剩下的最后一个字。
/// 分号/冒号是句内停顿, 不算段末。
fn ends_with_sentence_terminal(line: &str) -> bool {
    let mut chars: Vec<char> = line.trim_end().chars().collect();
    while matches!(chars.last(), Some(c) if is_closing_delimiter(*c)) {
        chars.pop();
    }
    matches!(
        chars.last(),
        Some('。' | '！' | '？' | '…' | '.' | '!' | '?')
    )
}

fn is_closing_delimiter(c: char) -> bool {
    matches!(
        c,
        '」' | '』' | '”' | '’' | '"' | '）' | ')' | ']' | '】' | '〉' | '》'
    )
}

/// 独立成段的行: 章节标题 (第X回/章…) 与页码行
fn is_standalone_line(line: &str) -> bool {
    is_heading_line(line, 40) || is_page_number_like(line)
}

/// 页码行: 短行, 由数字 (阿拉伯/汉字/罗马) 加装饰符构成,
/// 如「12」「- 3 -」「· 一百二十 ·」
fn is_page_number_like(line: &str) -> bool {
    const MAX_PAGE_NUMBER_WIDTH: f64 = 6.0;
    let mut width = 0.0;
    let mut has_numeral = false;
    for c in line.trim().chars() {
        if c.is_ascii_digit() || is_chapter_digit(c) {
            has_numeral = true;
            width += 1.0;
        } else if matches!(
            c,
            '-' | '—' | '－' | '·' | '.' | '。' | ' ' | '\u{3000}' | 'V' | 'v'
                | 'X' | 'x' | 'I' | 'i' | 'L' | 'l' | 'C' | 'c' | 'D' | 'M' | 'm'
        ) {
            width += 0.5;
        } else {
            return false;
        }
    }
    has_numeral && width <= MAX_PAGE_NUMBER_WIDTH
}

/// 拼接两行时, 两侧都是半角字母数字则补一个空格 (西文按空格分词)
fn needs_ascii_join(prev: &str, next: &str) -> bool {
    prev.ends_with(|c: char| c.is_ascii_alphanumeric())
        && next.starts_with(|c: char| c.is_ascii_alphanumeric())
}

/// 拼接 [start, end] 闭区间的页文本并重组段落 (页与页之间保持分段, 页
/// 眉页脚不会被并进正文), 空白页不留多余分隔
fn join_pages(pages: &[String], start: usize, end: usize) -> String {
    if start >= pages.len() {
        return String::new();
    }
    let joined = pages[start..=end.min(pages.len() - 1)]
        .iter()
        .map(|p| p.trim())
        .filter(|p| !p.is_empty())
        .collect::<Vec<_>>()
        .join("\n\n");
    merge_wrapped_lines(&joined)
}

/// 全空白视为不可读 (扫描件/图片型 PDF), 让上层给出可理解的报错
fn ensure_non_blank(text: String) -> Result<String, AppError> {
    if text.trim().is_empty() {
        return Err(AppError::BadRequest(
            "本章未提取到文本（PDF 可能是扫描件/图片型）".to_string(),
        ));
    }
    Ok(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fold_doubled_render_text_folds_pure_doubled_lines() {
        // fill+stroke 双重渲染的标题行: 每字两遍
        assert_eq!(fold_doubled_render_text("第第一一回回"), "第一回");
        assert_eq!(fold_doubled_render_text("西西游游记记"), "西游记");
        assert_eq!(
            fold_doubled_render_text("灵灵根根育育孕孕源源流流出出"),
            "灵根育孕源流出"
        );
        // 标题行内可夹空白
        assert_eq!(
            fold_doubled_render_text("第第一一回回   灵灵根根育育孕孕源源流流出出"),
            "第一回   灵根育孕源流出"
        );
        // 多行文本只折叠命中行
        assert_eq!(
            fold_doubled_render_text("正文第一行\n第第一一回回\n正文第三行"),
            "正文第一行\n第一回\n正文第三行"
        );
    }

    #[test]
    fn fold_doubled_render_text_keeps_normal_text() {
        // 混在其他文字中的正常叠词/成语不受影响
        assert_eq!(fold_doubled_render_text("谢谢老板"), "谢谢老板");
        assert_eq!(
            fold_doubled_render_text("他高高兴兴平平安安地走了"),
            "他高高兴兴平平安安地走了"
        );
        // 标点连用 (省略号) 不折叠
        assert_eq!(fold_doubled_render_text("他走了。。。。。。"), "他走了。。。。。。");
        // ASCII / 数字不参与折叠
        assert_eq!(fold_doubled_render_text("第 12 回"), "第 12 回");
        assert_eq!(fold_doubled_render_text("1122 3344"), "1122 3344");
    }

    #[test]
    fn join_pages_skips_blank_pages() {
        let pages = vec![
            "第一页".to_string(),
            "   ".to_string(),
            "第三页".to_string(),
        ];
        // 段落重组后页与页之间单换行分段 (空行被折叠)
        assert_eq!(join_pages(&pages, 0, 2), "第一页\n第三页");
        assert_eq!(join_pages(&pages, 2, 99), "第三页");
        assert_eq!(join_pages(&pages, 99, 120), "");
    }

    /// 21 全角字宽、行尾无句读的「写满行」
    fn full_line(suffix: &str) -> String {
        let mut line = "一二三四五六七八九十一二三四五六七八九十".to_string();
        line.push_str(suffix);
        line
    }

    #[test]
    fn merge_wrapped_lines_rejoins_split_sentences() {
        // 首行写满且以逗号结尾 → 是排版换行, 与下一行拼回同段;
        // 拼回后的段以句号收尾, 下一段从「新的段落」开始
        let text = format!("{}\n周了。\n新的段落。", full_line("，"));
        assert_eq!(
            merge_wrapped_lines(&text),
            format!("{}\n新的段落。", full_line("，周了。"))
        );
    }

    #[test]
    fn merge_wrapped_lines_breaks_after_sentence_terminal() {
        // 写满行以句号收尾 → 句界断段 (即使下一行其实是同段的延续也不会
        // 从句子中间截断)
        let text = format!("{}\n他接着说。", full_line("。"));
        assert_eq!(
            merge_wrapped_lines(&text),
            format!("{}\n他接着说。", full_line("。"))
        );
    }

    #[test]
    fn merge_wrapped_lines_keeps_short_lines_separate() {
        // 整章短行 (栏宽 < 16 字宽, 如诗行): 放弃合并, 逐行分段
        let verse = "床前明月光，\n疑是地上霜。\n举头望明月，\n低头思故乡。";
        assert_eq!(merge_wrapped_lines(verse), verse);
        // 对话短行同理, 即使章节里有正文行拉高栏宽估计: 短行永远断段
        let mixed = format!("{}\n好的。\n他也点头。", full_line("，"));
        let merged = merge_wrapped_lines(&mixed);
        assert!(merged.starts_with(&full_line("，好的。")));
        assert!(merged.ends_with("\n他也点头。"));
    }

    #[test]
    fn merge_wrapped_lines_keeps_headings_standalone() {
        // 标题行前后都断段, 不被并进正文
        let text = format!("{}\n第一回　灵根育孕源流出\n正文从这里开始", full_line("，"));
        assert_eq!(
            merge_wrapped_lines(&text),
            format!("{}\n第一回　灵根育孕源流出\n正文从这里开始", full_line("，"))
        );
    }

    #[test]
    fn merge_wrapped_lines_keeps_page_numbers_standalone() {
        // 页码行独立成段, 不被并进页尾正文, 也不吞掉下一页开头
        let text = format!("{}\n12\n他继续说了下去。", full_line("，"));
        assert_eq!(
            merge_wrapped_lines(&text),
            format!("{}\n12\n他继续说了下去。", full_line("，"))
        );
        assert!(is_page_number_like("- 3 -"));
        assert!(is_page_number_like("· 一百二十 ·"));
        assert!(!is_page_number_like("第12章 标题"));
        assert!(!is_page_number_like("abc123"));
    }

    #[test]
    fn merge_wrapped_lines_breaks_before_indented_line() {
        // 行首全角空格缩进 = 新段落, 即便上一行写满且无句读
        let text = format!("{}\n\u{3000}\u{3000}新段从这里开始。", full_line("，"));
        assert_eq!(
            merge_wrapped_lines(&text),
            format!("{}\n新段从这里开始。", full_line("，"))
        );
    }

    #[test]
    fn merge_wrapped_lines_adds_ascii_space_when_merging_ascii() {
        // 西文合并补空格; 中西混排直接拼接
        let long_ascii = "the quick brown fox jumps over the lazy dog now";
        let text = format!("{long_ascii}\nand then it ran away");
        let merged = merge_wrapped_lines(&text);
        assert!(merged.contains("lazy dog now and then it ran away"));

        let mixed = format!("{}\nOK 继续说", full_line("，"));
        assert!(merge_wrapped_lines(&mixed).contains("，OK 继续说"));
    }

    #[test]
    fn ends_with_sentence_terminal_handles_quotes_and_ellipses() {
        // 句末标点后跟收尾引号/括号
        assert!(ends_with_sentence_terminal("他说：「住手！」"));
        assert!(ends_with_sentence_terminal("「我走了。」"));
        assert!(ends_with_sentence_terminal("也许吧……"));
        assert!(ends_with_sentence_terminal("It was over."));
        // 句内停顿/未收尾
        assert!(!ends_with_sentence_terminal("他说："));
        assert!(!ends_with_sentence_terminal("其一、"));
        assert!(!ends_with_sentence_terminal("然而"));
    }

    #[test]
    fn join_pages_merges_paragraphs_within_chapter() {
        let pages = vec![
            format!("{}\n周了。", full_line("，")),
            "第二页开头。".to_string(),
        ];
        // 页内重组, 页与页之间保持分段
        assert_eq!(
            join_pages(&pages, 0, 1),
            format!("{}\n第二页开头。", full_line("，周了。"))
        );
    }

    #[test]
    fn ensure_non_blank_rejects_empty() {
        assert!(ensure_non_blank("  \n ".to_string()).is_err());
        assert_eq!(ensure_non_blank("正文".to_string()).unwrap(), "正文");
    }

    #[test]
    fn detect_headings_finds_chapter_titles() {
        let pages = vec![
            "西游记\n吴承恩撰\n第一回   灵根育孕源流出   心性修持大道生\n诗曰：混沌未分天地乱".to_string(),
            "正文继续，他说第一回合就赢了。".to_string(),
            "第二回　悟彻菩提真妙理\n正文".to_string(),
        ];
        let items = detect_headings(&pages);
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].page_index, 0);
        assert!(items[0].title.starts_with("第一回"));
        assert_eq!(items[1].page_index, 2);
        assert!(items[1].title.starts_with("第二回"));
    }

    #[test]
    fn detect_headings_skips_toc_pages_and_duplicates() {
        // 目录页: 一页里挤了 4 个回目 → 整页跳过
        let toc = "第一回　甲\n第二回　乙\n第三回　丙\n第四回　丁".to_string();
        let body = "第一回　甲\n正文".to_string();
        let repeat = "第一回　甲\n又是页眉".to_string();
        let items = detect_headings(&[toc, body, repeat]);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].page_index, 1);
    }

    #[test]
    fn is_heading_line_rejects_prose() {
        assert!(is_heading_line("第一回　甄士隐梦幻识通灵", 40));
        assert!(is_heading_line("第 12 章 标题", 40));
        assert!(is_heading_line("第三节", 40));
        // 紧跟正文的不是标题
        assert!(!is_heading_line("第一回合他就赢了", 40));
        // 缺数字/缺量词
        assert!(!is_heading_line("第回", 40));
        assert!(!is_heading_line("第一个人", 40));
        assert!(!is_heading_line("这是第一回", 40));
        // 超长行不是标题
        assert!(!is_heading_line(&format!("第一回　{}", "字".repeat(60)), 40));
    }

    #[test]
    fn drop_blank_chapters_removes_and_reindexes() {
        let url = "local-pdf:abc";
        let pages = vec![
            "   ".to_string(),   // 封面图页, 无文字
            "正文一".to_string(),
            "正文二".to_string(),
        ];
        let chapters = build_chapters(url, 3, &[
            chapter_outline("封面", 0),
            chapter_outline("第一章", 1),
            chapter_outline("第二章", 2),
        ]);
        assert_eq!(chapters.len(), 3);
        let kept = drop_blank_chapters(url, chapters, &pages);
        assert_eq!(kept.len(), 2);
        assert_eq!(kept[0].title, "第一章");
        // 重新编号: index 与 url 连续
        assert_eq!(kept[0].index, 0);
        assert_eq!(kept[0].url, format!("{url}#0"));
        assert_eq!(kept[1].index, 1);
        assert_eq!(kept[1].url, format!("{url}#1"));
        // 页范围不受影响
        assert_eq!(kept[0].page_start, 1);
        assert_eq!(kept[1].page_start, 2);
    }

    #[test]
    fn chapter_text_path_is_stable() {
        let dir = Path::new("data");
        assert_eq!(
            chapter_text_path(dir, 0),
            dir.join("chapters").join("00000.txt")
        );
        assert_eq!(
            chapter_text_path(dir, 121),
            dir.join("chapters").join("00121.txt")
        );
    }

    fn chapter_outline(title: &str, page_index: usize) -> OutlineItem {
        OutlineItem {
            title: title.to_string(),
            page_index,
        }
    }

    #[test]
    fn build_chapters_without_outline_groups_fixed_pages() {
        let url = "local-pdf:abc";
        // 100 页 → 5 章各 20 页
        let chapters = build_chapters(url, 100, &[]);
        assert_eq!(chapters.len(), 5);
        assert_eq!(chapters[0].page_start, 0);
        assert_eq!(chapters[0].page_end, 19);
        assert_eq!(chapters[4].page_end, 99);
        assert!(chapters[0].title.contains("第 1 部分"));
        // 1318 页 → 66 章, 末章 18 页
        let chapters = build_chapters(url, 1318, &[]);
        assert_eq!(chapters.len(), 66);
        assert_eq!(chapters[65].page_start, 1300);
        assert_eq!(chapters[65].page_end, 1317);
        // 章节 url/index 连续
        assert_eq!(chapters[3].index, 3);
        assert_eq!(chapters[3].url, format!("{url}#3"));
    }

    #[test]
    fn build_chapters_with_outline_uses_bookmarks() {
        let url = "local-pdf:abc";
        let outline = vec![
            chapter_outline("第一回", 5),
            chapter_outline("第二回", 30),
            chapter_outline("第三回", 88),
        ];
        let chapters = build_chapters(url, 100, &outline);
        // 引导章 [0,4] + 3 个书签章
        assert_eq!(chapters.len(), 4);
        assert_eq!(chapters[0].page_start, 0);
        assert_eq!(chapters[0].page_end, 4);
        assert_eq!(chapters[0].title, "开头 (P1-P5)");
        assert_eq!(chapters[1].title, "第一回");
        assert_eq!(chapters[1].page_start, 5);
        assert_eq!(chapters[1].page_end, 29);
        assert_eq!(chapters[2].page_start, 30);
        assert_eq!(chapters[2].page_end, 87);
        // 末章延伸到最后一页
        assert_eq!(chapters[3].page_start, 88);
        assert_eq!(chapters[3].page_end, 99);
    }

    #[test]
    fn build_chapters_drops_out_of_range_and_same_page_outline() {
        let url = "local-pdf:abc";
        let outline = vec![
            chapter_outline("超范围", 500),
            chapter_outline("第一章", 0),
            chapter_outline("同页书签", 0),
        ];
        let chapters = build_chapters(url, 10, &outline);
        // 超范围被过滤; 同页书签两章都至少含起始页
        assert_eq!(chapters.len(), 2);
        assert_eq!(chapters[0].title, "第一章");
        assert_eq!(chapters[0].page_end, 0);
        assert_eq!(chapters[1].title, "同页书签");
        assert_eq!(chapters[1].page_start, 0);
        assert_eq!(chapters[1].page_end, 9);
    }

    #[test]
    fn build_chapters_sorts_non_monotonic_outline() {
        let url = "local-pdf:abc";
        // 附录书签指回前面的页: 排序后按页号切分
        let outline = vec![
            chapter_outline("第一章", 10),
            chapter_outline("第二章", 50),
            chapter_outline("前言", 2),
        ];
        let chapters = build_chapters(url, 100, &outline);
        let titles: Vec<&str> = chapters.iter().map(|c| c.title.as_str()).collect();
        assert_eq!(titles, ["开头 (P1-P2)", "前言", "第一章", "第二章"]);
        assert_eq!((chapters[1].page_start, chapters[1].page_end), (2, 9));
        assert_eq!((chapters[2].page_start, chapters[2].page_end), (10, 49));
        assert_eq!((chapters[3].page_start, chapters[3].page_end), (50, 99));
    }

    #[test]
    fn build_chapters_empty_book() {
        assert!(build_chapters("local-pdf:abc", 0, &[]).is_empty());
    }

    #[test]
    fn decode_pdf_text_string_handles_utf16_and_latin1() {
        // UTF-16BE 带 BOM
        let utf16 = [0xFE, 0xFF, 0x7B, 0x2C]; // 第
        assert_eq!(decode_pdf_text_string(&utf16), "第");
        // PDFDocEncoding 近似 latin1
        assert_eq!(decode_pdf_text_string(&[0x41, 0x42]), "AB");
    }
}
