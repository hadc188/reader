//! 批量导出书架书籍为 txt / epub。进度通过 IPC Channel 逐章推送。

use std::path::{Path, PathBuf};

use serde::Deserialize;
use serde_json::{json, Value};
use tauri::ipc::Channel;
use tauri_plugin_dialog::DialogExt;
use tokio::task::JoinSet;

use crate::api::AppState;
use crate::error::error::AppError;
use crate::export::epub;
use crate::model::book::Book;
use crate::model::book_chapter::BookChapter;
use crate::service::local_epub_book::is_local_epub_origin;
use crate::service::local_pdf_book::is_local_pdf_origin;
use crate::service::local_txt_book::is_local_txt_origin;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportBooksRequest {
    pub books: Vec<Book>,
    pub format: String,
    #[serde(default)]
    pub chapter_ranges: std::collections::HashMap<String, String>,
}

/// 章节内容的获取方式: 本地书直接读文件, 在线书带 BookSource 走抓取+缓存。
#[derive(Clone)]
enum Fetcher {
    LocalTxt,
    LocalEpub,
    Online(crate::model::book_source::BookSource),
}

#[tauri::command]
pub async fn export_books_sse(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    req: ExportBooksRequest,
    on_event: Channel<Value>,
) -> Result<(), AppError> {
    let user_ns = "default";
    let format = match req.format.as_str() {
        "txt" | "epub" => req.format.clone(),
        _ => {
            return Err(AppError::BadRequest(
                "导出格式仅支持 txt / epub".to_string(),
            ))
        }
    };
    if req.books.is_empty() {
        return Err(AppError::BadRequest("未选择书籍".to_string()));
    }

    // 选导出目录; 取消则直接结束 (非错误)。
    let Some(folder_file) = app
        .dialog()
        .file()
        .set_title("选择导出目录")
        .blocking_pick_folder()
    else {
        let _ = on_event.send(json!({"event":"end","cancelled":true}));
        return Ok(());
    };
    let folder: PathBuf = folder_file
        .into_path()
        .map_err(|e| AppError::BadRequest(format!("无法访问目录：{e}")))?;

    let state_clone = state.inner().clone();
    let books = req.books;
    let fmt = format;
    let chapter_ranges = req.chapter_ranges;
    let on_event_clone = on_event.clone();
    tokio::spawn(async move {
        let total = books.len();
        let mut succeeded = 0usize;
        let mut failed = 0usize;
        for (book_index, book) in books.iter().enumerate() {
            let book_name = book.name.clone();
            let chapter_range = chapter_ranges
                .get(&book.book_url)
                .map(String::as_str)
                .unwrap_or("");
            match export_one(
                &state_clone,
                user_ns,
                book,
                chapter_range,
                &fmt,
                &folder,
                &on_event_clone,
                book_index,
            )
            .await
            {
                Ok((path, chapter_count)) => {
                    succeeded += 1;
                    if on_event_clone
                        .send(json!({
                            "event":"book_done",
                            "bookIndex": book_index,
                            "bookName": book_name,
                            "path": path,
                            "chapters": chapter_count,
                        }))
                        .is_err()
                    {
                        return;
                    }
                }
                Err(err) => {
                    failed += 1;
                    let msg = err.to_string();
                    // 用户取消静默退出, 其它失败逐书可见。
                    let is_cancel = msg.contains("导出已取消");
                    if on_event_clone
                        .send(json!({
                            "event":"book_failed",
                            "bookIndex": book_index,
                            "bookName": book_name,
                            "error": msg,
                            "cancelled": is_cancel,
                        }))
                        .is_err()
                        || is_cancel
                    {
                        return;
                    }
                }
            }
        }
        let _ = on_event_clone.send(json!({
            "event":"end",
            "total": total,
            "succeeded": succeeded,
            "failed": failed,
            "folder": folder.to_string_lossy(),
        }));
    });
    Ok(())
}

async fn export_one(
    state: &AppState,
    user_ns: &str,
    book: &Book,
    chapter_range: &str,
    format: &str,
    folder: &Path,
    on_event: &Channel<Value>,
    book_index: usize,
) -> Result<(String, usize), AppError> {
    // 1. 解析章节目录与内容获取方式。
    let (chapters, fetcher) = resolve_chapters(state, user_ns, book).await?;
    let chapters = if chapter_range.trim().is_empty() {
        chapters
    } else {
        let indices = parse_chapter_ranges(chapter_range, chapters.len())
            .map_err(AppError::BadRequest)?;
        if indices.is_empty() {
            return Err(AppError::BadRequest("自定义章节为空".to_string()));
        }
        indices
            .into_iter()
            .filter_map(|index| chapters.get(index).cloned())
            .collect()
    };
    if chapters.is_empty() {
        return Err(AppError::BadRequest("该书无章节, 无法导出".to_string()));
    }

    // 2. 并发拉取正文 (spawn-join 交错, JoinSet 限并发度); get_content 自带缓存,
    //    命中即返回。进度在 join 步发单调递增的 done 计数, 避免乱序完成导致进度条回跳。
    const EXPORT_CONCURRENCY: usize = 16;
    let total = chapters.len();
    let mut results: Vec<Option<(String, String)>> = vec![None; total];
    let mut tasks: JoinSet<(usize, String, Result<String, AppError>)> = JoinSet::new();
    let mut next_ci = 0usize;
    let mut done = 0usize;

    while next_ci < chapters.len() || !tasks.is_empty() {
        // 派发新任务, 直到达到并发上限或章节用完。
        while tasks.len() < EXPORT_CONCURRENCY && next_ci < chapters.len() {
            let ch = &chapters[next_ci];
            let ci = next_ci;
            let svc = state.clone();
            let user_ns_owned = user_ns.to_string();
            let book_url_owned = book.book_url.clone();
            let ch_url = ch.url.clone();
            let ch_title = ch.title.clone();
            let fetcher_clone = fetcher.clone();
            tasks.spawn(async move {
                let content = match &fetcher_clone {
                    Fetcher::LocalTxt => {
                        svc.local_txt_book_service.get_content(&user_ns_owned, &ch_url).await
                    }
                    Fetcher::LocalEpub => {
                        svc.local_epub_book_service.get_content(&user_ns_owned, &ch_url).await
                    }
                    Fetcher::Online(source) => {
                        svc.book_service
                            .get_content(&user_ns_owned, &book_url_owned, source, &ch_url)
                            .await
                    }
                };
                (ci, ch_title, content)
            });
            next_ci += 1;
        }
        if tasks.is_empty() {
            break;
        }
        // 等一个完成, 收集 (按章节序号回填, 保证最终顺序), 推单调进度。
        let res = match tasks.join_next().await {
            Some(Ok(r)) => r,
            _ => continue,
        };
        done += 1;
        let (ci, title, content_res) = res;
        let content = match content_res {
            Ok(c) => crate::export::html_to_plain_text(&c),
            // 单章失败不致命: 留空正文, 保留章节占位。
            Err(_) => String::new(),
        };
        results[ci] = Some((title.clone(), content));
        if on_event
            .send(json!({
                "event":"progress",
                "bookIndex": book_index,
                "bookName": book.name,
                "done": done,
                "total": total,
                "chapterTitle": title,
            }))
            .is_err()
        {
            // 前端关通道 = 取消; 中止剩余任务, 不留半文件 (尚未写出)。
            tasks.abort_all();
            return Err(AppError::BadRequest("导出已取消".to_string()));
        }
    }

    let collected: Vec<(String, String)> = results.into_iter().flatten().collect();

    // 3. 组装并写出 (全在内存, 写文件是原子的, 中途取消不会留半文件)。
    let ext = if format == "epub" { "epub" } else { "txt" };
    let out_path = unique_path(folder, &sanitize_file_name(&book.name), ext);
    let chapter_count = collected.len();
    match format {
        "epub" => {
            let author = book.author.trim();
            // epub 写入是同步 CPU 活, 丢到 blocking 线程避免卡 async 运行时。
            let title = book.name.clone();
            let author = author.to_string();
            let path = out_path.clone();
            tokio::task::spawn_blocking(move || epub::write_epub(&title, &author, &collected, &path))
                .await
                .map_err(|e| AppError::Internal(e.into()))??;
        }
        _ => {
            let mut text = String::new();
            for (title, body) in &collected {
                text.push_str(title);
                text.push_str("\n\n");
                text.push_str(body);
                text.push_str("\n\n\n");
            }
            tokio::fs::write(&out_path, text)
                .await
                .map_err(|e| AppError::Internal(e.into()))?;
        }
    }

    Ok((out_path.to_string_lossy().to_string(), chapter_count))
}

async fn resolve_chapters(
    state: &AppState,
    user_ns: &str,
    book: &Book,
) -> Result<(Vec<BookChapter>, Fetcher), AppError> {
    if is_local_pdf_origin(&book.origin) {
        return Err(AppError::BadRequest("PDF 暂不支持导出".to_string()));
    }
    if is_local_txt_origin(&book.origin) {
        let chapters = state
            .local_txt_book_service
            .get_chapter_list(user_ns, &book.book_url)
            .await?;
        return Ok((chapters, Fetcher::LocalTxt));
    }
    if is_local_epub_origin(&book.origin) {
        let chapters = state
            .local_epub_book_service
            .get_chapter_list(user_ns, &book.book_url)
            .await?;
        return Ok((chapters, Fetcher::LocalEpub));
    }
    let source = state
        .book_source_service
        .get(user_ns, &book.origin)
        .await?
        .ok_or_else(|| AppError::BadRequest(format!("书源不存在: {}", book.origin)))?;
    let toc_url = book
        .toc_url
        .clone()
        .filter(|u| !u.trim().is_empty())
        .unwrap_or_else(|| book.book_url.clone());
    let chapters = state
        .book_service
        .get_chapter_list(user_ns, &source, &toc_url)
        .await?;
    Ok((chapters, Fetcher::Online(source)))
}

/// Parse 1-based chapter expressions such as `1-20,25,30-35`.
fn parse_chapter_ranges(input: &str, total: usize) -> Result<Vec<usize>, String> {
    let mut indices = Vec::new();
    for part in input.split(&[',', '，', ' '][..]) {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        let parse_number = |value: &str| -> Result<usize, String> {
            let number = value
                .parse::<usize>()
                .map_err(|_| format!("章节序号无效: {value}"))?;
            if number == 0 || number > total {
                Err(format!("章节序号超出范围: {number}"))
            } else {
                Ok(number - 1)
            }
        };
        if let Some((start, end)) = part.split_once(&['-', '~'][..]) {
            let start = parse_number(start.trim())?;
            let end = if end.trim().is_empty() {
                total.saturating_sub(1)
            } else {
                parse_number(end.trim())?
            };
            if start > end {
                return Err(format!("章节区间无效: {part}"));
            }
            indices.extend(start..=end);
        } else {
            let index = parse_number(part)?;
            indices.push(index);
        }
    }
    indices.sort_unstable();
    indices.dedup();
    Ok(indices)
}

/// 清洗文件名非法字符 (Windows / 通用)。
fn sanitize_file_name(name: &str) -> String {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return "未命名".to_string();
    }
    let mut out = String::with_capacity(trimmed.len());
    for ch in trimmed.chars() {
        if matches!(ch, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*') {
            out.push('_');
        } else {
            out.push(ch);
        }
    }
    out
}

/// 目标路径已存在时追加 ` (1)` ` (2)` 避免覆盖。
fn unique_path(folder: &Path, name: &str, ext: &str) -> PathBuf {
    let base = folder.join(format!("{name}.{ext}"));
    if !base.exists() {
        return base;
    }
    for i in 1..9999 {
        let candidate = folder.join(format!("{name} ({i}).{ext}"));
        if !candidate.exists() {
            return candidate;
        }
    }
    base
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_chapter_ranges_with_dedup_and_bounds() {
        assert_eq!(
            parse_chapter_ranges("1-3, 2，5~6, 6-", 6).unwrap(),
            vec![0, 1, 2, 4, 5]
        );
        assert!(parse_chapter_ranges("0", 6).is_err());
        assert!(parse_chapter_ranges("7", 6).is_err());
        assert!(parse_chapter_ranges("3-1", 6).is_err());
        assert!(parse_chapter_ranges("abc", 6).is_err());
    }
}
