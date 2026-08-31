//! 最小合法 EPUB3 写入器 (text-only, 不处理图片)。
//!
//! 用 `zip` crate 打包: mimetype (stored) + META-INF/container.xml +
//! OEBPS/content.opf + OEBPS/nav.xhtml + OEBPS/toc.ncx + OEBPS/chap-N.xhtml。
//! 正文是 `book_service.get_content` 已提取的纯文本, 按 `\n` 切分包成 `<p>`。

use std::io::{Cursor, Write};
use std::path::Path;

use crate::error::error::AppError;

use zip::write::FileOptions;
use zip::CompressionMethod;

/// 写入一本 EPUB。
///
/// `chapters` 为 `(章节标题, 正文文本)` 列表。正文按换行切分为段落,
/// 每段转义 HTML 后包成 `<p>`。`out` 为输出文件路径 (覆盖写)。
pub fn write_epub(
    title: &str,
    author: &str,
    chapters: &[(String, String)],
    out: &Path,
) -> Result<(), AppError> {
    let mut buf = Cursor::new(Vec::new());
    {
        let mut zip = zip::ZipWriter::new(&mut buf);
        let stored = FileOptions::default()
            .compression_method(CompressionMethod::Stored)
            .unix_permissions(0o644);
        let deflate = FileOptions::default()
            .compression_method(CompressionMethod::Deflated)
            .unix_permissions(0o644);

        // mimetype 必须是第一个且不压缩。
        zip.start_file("mimetype", stored)
            .map_err(to_err)?;
        zip.write_all(b"application/epub+zip")
            .map_err(to_err)?;

        zip.start_file("META-INF/container.xml", deflate)
            .map_err(to_err)?;
        zip.write_all(CONTAINER_XML.as_bytes())
            .map_err(to_err)?;

        let opf = build_opf(title, author, chapters);
        zip.start_file("OEBPS/content.opf", deflate)
            .map_err(to_err)?;
        zip.write_all(opf.as_bytes())
            .map_err(to_err)?;

        let nav = build_nav(chapters);
        zip.start_file("OEBPS/nav.xhtml", deflate)
            .map_err(to_err)?;
        zip.write_all(nav.as_bytes())
            .map_err(to_err)?;

        let ncx = build_ncx(title, chapters);
        zip.start_file("OEBPS/toc.ncx", deflate)
            .map_err(to_err)?;
        zip.write_all(ncx.as_bytes())
            .map_err(to_err)?;

        for (index, (chap_title, body)) in chapters.iter().enumerate() {
            let name = format!("OEBPS/chap-{}.xhtml", index + 1);
            zip.start_file(&name, deflate).map_err(to_err)?;
            zip.write_all(build_chapter_xhtml(chap_title, body).as_bytes())
                .map_err(to_err)?;
        }

        zip.finish().map_err(to_err)?;
    }
    let bytes = buf.into_inner();
    std::fs::write(out, &bytes).map_err(|e| AppError::Internal(e.into()))?;
    Ok(())
}

fn to_err<E: std::error::Error + Send + Sync + 'static>(e: E) -> AppError {
    AppError::Internal(e.into())
}

fn escape_xml(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '&' => out.push_str("&amp;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            _ => out.push(ch),
        }
    }
    out
}

/// 正文按行切分, 空行跳过, 每行一段 `<p>`。
fn build_chapter_xhtml(title: &str, body: &str) -> String {
    let mut html = String::new();
    html.push_str("<!DOCTYPE html>\n<html xmlns=\"http://www.w3.org/1999/xhtml\">");
    html.push_str("<head><meta charset=\"utf-8\"/><title>");
    html.push_str(&escape_xml(title));
    html.push_str("</title></head><body>");
    html.push_str("<h1>");
    html.push_str(&escape_xml(title));
    html.push_str("</h1>");
    for line in body.split('\n') {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        html.push_str("<p>");
        html.push_str(&escape_xml(trimmed));
        html.push_str("</p>");
    }
    html.push_str("</body></html>");
    html
}

fn build_opf(title: &str, author: &str, chapters: &[(String, String)]) -> String {
    let mut s = String::new();
    s.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    s.push_str(
        "<package xmlns=\"http://www.idpf.org/2007/opf\" version=\"3.0\" unique-identifier=\"bookid\">\n",
    );
    s.push_str("<metadata xmlns:dc=\"http://purl.org/dc/elements/1.1/\">\n");
    s.push_str("<dc:identifier id=\"bookid\">reader-export-");
    s.push_str(&uuid_like_id(title, chapters));
    s.push_str("</dc:identifier>\n");
    s.push_str("<dc:title>");
    s.push_str(&escape_xml(title));
    s.push_str("</dc:title>\n");
    s.push_str("<dc:creator>");
    s.push_str(&escape_xml(author));
    s.push_str("</dc:creator>\n");
    s.push_str("<dc:language>zh-CN</dc:language>\n");
    s.push_str("</metadata>\n<manifest>\n");
    s.push_str("<item id=\"nav\" href=\"nav.xhtml\" media-type=\"application/xhtml+xml\" properties=\"nav\"/>\n");
    s.push_str("<item id=\"ncx\" href=\"toc.ncx\" media-type=\"application/x-dtbncx+xml\"/>\n");
    for i in 1..=chapters.len() {
        s.push_str(&format!(
            "<item id=\"chap{i}\" href=\"chap-{i}.xhtml\" media-type=\"application/xhtml+xml\"/>\n"
        ));
    }
    s.push_str("</manifest>\n<spine toc=\"ncx\">\n");
    s.push_str("<itemref idref=\"nav\" linear=\"yes\"/>\n");
    for i in 1..=chapters.len() {
        s.push_str(&format!("<itemref idref=\"chap{i}\"/>\n"));
    }
    s.push_str("</spine>\n</package>");
    s
}

fn build_nav(chapters: &[(String, String)]) -> String {
    let mut s = String::new();
    s.push_str(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE html>\n<html xmlns=\"http://www.w3.org/1999/xhtml\" xmlns:epub=\"http://www.idpf.org/2007/ops\">\n",
    );
    s.push_str("<head><meta charset=\"utf-8\"/><title>目录</title></head><body>\n");
    s.push_str("<nav epub:type=\"toc\" id=\"toc\">\n<ol>\n");
    for (index, (title, _)) in chapters.iter().enumerate() {
        // href 用章节序号, 锚文本用章节标题。
        s.push_str(&format!(
            "<li><a href=\"chap-{idx}.xhtml\">{title}</a></li>\n",
            idx = index + 1,
            title = escape_xml(title)
        ));
    }
    s.push_str("</ol>\n</nav>\n</body></html>");
    s
}

fn build_ncx(title: &str, chapters: &[(String, String)]) -> String {
    let mut s = String::new();
    s.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    s.push_str(
        "<ncx xmlns=\"http://www.daisy.org/z3986/2005/ncx/\" version=\"2005-1\">\n",
    );
    s.push_str("<head><meta name=\"dtb:title\" content=\"");
    s.push_str(&escape_xml(title));
    s.push_str("\"/></head>\n<docTitle><text>");
    s.push_str(&escape_xml(title));
    s.push_str("</text></docTitle>\n<navMap>\n");
    for (index, (chap_title, _)) in chapters.iter().enumerate() {
        s.push_str(&format!(
            "<navPoint id=\"np{idx}\" playOrder=\"{idx}\"><navLabel><text>{t}</text></navLabel><content src=\"chap-{idx}.xhtml\"/></navPoint>\n",
            idx = index + 1,
            t = escape_xml(chap_title)
        ));
    }
    s.push_str("</navMap>\n</ncx>");
    s
}

/// 不引入 uuid 依赖, 用标题 + 章节数拼一个稳定标识。
fn uuid_like_id(title: &str, chapters: &[(String, String)]) -> String {
    let mut h: u64 = 14695981039346656037;
    for b in title.as_bytes() {
        h ^= *b as u64;
        h = h.wrapping_mul(1099511628211);
    }
    h ^= chapters.len() as u64;
    h = h.wrapping_mul(1099511628211);
    format!("{h:016x}")
}

const CONTAINER_XML: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container">
  <rootfiles>
    <rootfile full-path="OEBPS/content.opf" media-type="application/oebps-package+xml"/>
  </rootfiles>
</container>"#;

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;

    #[test]
    fn write_epub_roundtrips_valid_package_with_chapters() {
        let dir = std::env::temp_dir().join(format!("reader-epub-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let out = dir.join("test.epub");
        let chapters = vec![
            ("第一章".to_string(), "第一段\n\n第二段".to_string()),
            ("第二章".to_string(), "你好 & <再见>".to_string()),
        ];
        write_epub("书名", "作者", &chapters, &out).unwrap();

        let bytes = std::fs::read(&out).unwrap();
        let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();

        // mimetype 必须是第一项且未压缩; 单独作用域让 ZipFile 借用先释放。
        {
            let mut f = archive.by_index(0).unwrap();
            assert_eq!(f.name(), "mimetype");
            assert_eq!(f.compression(), CompressionMethod::Stored);
            let mut mt = String::new();
            f.read_to_string(&mut mt).unwrap();
            assert_eq!(mt, "application/epub+zip");
        }

        let mut names = Vec::new();
        for i in 0..archive.len() {
            names.push(archive.by_index(i).unwrap().name().to_string());
        }
        assert!(names.iter().any(|n| n == "META-INF/container.xml"));
        assert!(names.iter().any(|n| n == "OEBPS/content.opf"));
        assert!(names.iter().any(|n| n == "OEBPS/nav.xhtml"));
        assert!(names.iter().any(|n| n == "OEBPS/toc.ncx"));
        assert!(names.iter().any(|n| n == "OEBPS/chap-1.xhtml"));
        assert!(names.iter().any(|n| n == "OEBPS/chap-2.xhtml"));

        // 转义生效: & < > 被转成实体。
        let mut chap2 = String::new();
        archive
            .by_name("OEBPS/chap-2.xhtml")
            .unwrap()
            .read_to_string(&mut chap2)
            .unwrap();
        assert!(chap2.contains("你好 &amp; &lt;再见&gt;"));

        let _ = std::fs::remove_dir_all(&dir);
    }
}
