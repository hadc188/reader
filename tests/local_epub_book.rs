use reader_rust::service::local_epub_book::LocalEpubBookService;
use std::io::{Cursor, Write};
use zip::write::FileOptions;

fn make_epub(files: &[(&str, &str)], binary_files: &[(&str, &[u8])]) -> Vec<u8> {
    let mut cursor = Cursor::new(Vec::new());
    {
        let mut zip = zip::ZipWriter::new(&mut cursor);
        let options = FileOptions::default().compression_method(zip::CompressionMethod::Stored);
        for (path, content) in files {
            zip.start_file(*path, options).unwrap();
            zip.write_all(content.as_bytes()).unwrap();
        }
        for (path, content) in binary_files {
            zip.start_file(*path, options).unwrap();
            zip.write_all(content).unwrap();
        }
        zip.finish().unwrap();
    }
    cursor.into_inner()
}

fn epub3_fixture() -> Vec<u8> {
    make_epub(
        &[
            ("mimetype", "application/epub+zip"),
            (
                "META-INF/container.xml",
                r#"<?xml version="1.0"?>
<container xmlns="urn:oasis:names:tc:opendocument:xmlns:container">
  <rootfiles><rootfile full-path="OEBPS/content.opf"/></rootfiles>
</container>"#,
            ),
            (
                "OEBPS/content.opf",
                r#"<?xml version="1.0"?>
<package version="3.0" xmlns="http://www.idpf.org/2007/opf">
  <metadata xmlns:dc="http://purl.org/dc/elements/1.1/">
    <dc:title>测试 EPUB</dc:title>
    <dc:creator>作者甲</dc:creator>
  </metadata>
  <manifest>
    <item id="nav" href="nav.xhtml" media-type="application/xhtml+xml" properties="nav"/>
    <item id="cover" href="Images/cover.png" media-type="image/png" properties="cover-image"/>
    <item id="pic" href="Images/pic.png" media-type="image/png"/>
    <item id="ch1" href="Text/ch1.xhtml" media-type="application/xhtml+xml"/>
    <item id="ch2" href="Text/ch2.xhtml" media-type="application/xhtml+xml"/>
  </manifest>
  <spine>
    <itemref idref="ch1"/>
    <itemref idref="ch2"/>
  </spine>
</package>"#,
            ),
            (
                "OEBPS/nav.xhtml",
                r#"<html xmlns="http://www.w3.org/1999/xhtml"><body>
<nav epub:type="toc"><ol>
<li><a href="Text/ch1.xhtml">第一章 导入</a></li>
<li><a href="Text/ch2.xhtml">第二章 图片</a></li>
</ol></nav>
</body></html>"#,
            ),
            (
                "OEBPS/Text/ch1.xhtml",
                r#"<html xmlns="http://www.w3.org/1999/xhtml"><head><title>旧标题</title></head><body>
<script>alert(1)</script>
<h1 onclick="bad()">第一章</h1>
<p>第一段正文。</p>
<p><img src="../Images/pic.png" onerror="bad()" alt="插图"/></p>
<p><img src="../Images/unlisted.png" alt="未声明图片"/></p>
</body></html>"#,
            ),
            (
                "OEBPS/Text/ch2.xhtml",
                r#"<html xmlns="http://www.w3.org/1999/xhtml"><body><ul><li>列表项</li></ul></body></html>"#,
            ),
        ],
        &[
            ("OEBPS/Images/pic.png", b"\x89PNG\r\n\x1a\n"),
            ("OEBPS/Images/cover.png", b"\x89PNG\r\n\x1a\ncover"),
            ("OEBPS/Images/unlisted.png", b"\x89PNG\r\n\x1a\nunlisted"),
        ],
    )
}

fn epub2_fixture() -> Vec<u8> {
    make_epub(
        &[
            ("mimetype", "application/epub+zip"),
            (
                "META-INF/container.xml",
                r#"<container><rootfiles><rootfile full-path="OPS/book.opf"/></rootfiles></container>"#,
            ),
            (
                "OPS/book.opf",
                r#"<package version="2.0">
  <metadata xmlns:dc="http://purl.org/dc/elements/1.1/"><dc:title>NCX 书</dc:title></metadata>
  <manifest>
    <item id="ncx" href="toc.ncx" media-type="application/x-dtbncx+xml"></item>
    <item id="c1" href="chapters/one.xhtml" media-type="application/xhtml+xml"></item>
  </manifest>
  <spine toc="ncx"><itemref idref="c1"></itemref></spine>
</package>"#,
            ),
            (
                "OPS/toc.ncx",
                r#"<ncx><navMap><navPoint><navLabel><text>NCX 第一章</text></navLabel><content src="chapters/one.xhtml"/></navPoint></navMap></ncx>"#,
            ),
            (
                "OPS/chapters/one.xhtml",
                r#"<html><body><p>NCX 正文</p></body></html>"#,
            ),
        ],
        &[],
    )
}

#[tokio::test]
async fn epub_import_saves_manifest_chapters_and_assets() {
    let storage_dir = std::env::temp_dir().join(format!(
        "reader-rust-local-epub-test-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&storage_dir);
    let service = LocalEpubBookService::new(&storage_dir);

    let book = service
        .import_epub_book("alice", "测试.epub", &epub3_fixture())
        .await
        .unwrap();
    let chapters = service
        .get_chapter_list("alice", &book.book_url)
        .await
        .unwrap();
    let content = service
        .get_content("alice", &chapters[0].url)
        .await
        .unwrap();
    let asset = service
        .get_asset("alice", &book.book_url, "OEBPS/Images/pic.png")
        .await
        .unwrap();

    assert_eq!(book.name, "测试 EPUB");
    assert_eq!(book.author, "作者甲");
    assert_eq!(book.origin, "local-epub");
    assert_eq!(book.origin_name.as_deref(), Some("本地 EPUB"));
    assert_eq!(book.can_update, Some(false));
    assert!(book
        .cover_url
        .as_deref()
        .unwrap_or_default()
        .contains("localEpubAsset"));
    assert_eq!(chapters.len(), 2);
    assert_eq!(chapters[0].title, "第一章 导入");
    assert!(content.contains("<h1>第一章</h1>"));
    assert!(content.contains("localEpubAsset"));
    assert!(content.contains("<img"));
    assert!(!content.contains("unlisted.png"));
    assert!(!content.contains("<script"));
    assert!(!content.contains("onclick"));
    assert!(!content.contains("onerror"));
    assert_eq!(asset.content_type, "image/png");
    assert_eq!(asset.bytes, b"\x89PNG\r\n\x1a\n");
    assert!(service
        .get_asset("alice", &book.book_url, "OEBPS/Images/unlisted.png")
        .await
        .is_err());

    let _ = std::fs::remove_dir_all(storage_dir);
}

#[tokio::test]
async fn epub2_ncx_titles_are_used_for_chapter_list() {
    let storage_dir = std::env::temp_dir().join(format!(
        "reader-rust-local-epub-ncx-test-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&storage_dir);
    let service = LocalEpubBookService::new(&storage_dir);

    let book = service
        .import_epub_book("alice", "ncx.epub", &epub2_fixture())
        .await
        .unwrap();
    let chapters = service
        .get_chapter_list("alice", &book.book_url)
        .await
        .unwrap();

    assert_eq!(book.name, "NCX 书");
    assert_eq!(chapters[0].title, "NCX 第一章");

    let _ = std::fs::remove_dir_all(storage_dir);
}

#[tokio::test]
async fn epub_import_rejects_non_epub_and_path_traversal() {
    let storage_dir = std::env::temp_dir().join(format!(
        "reader-rust-local-epub-reject-test-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&storage_dir);
    let service = LocalEpubBookService::new(&storage_dir);

    let err = service
        .import_epub_book("alice", "bad.txt", b"not epub")
        .await
        .expect_err("non epub file should be rejected");
    assert!(err.to_string().contains(".epub"));

    let traversal = make_epub(&[("../evil.txt", "x")], &[]);
    let err = service
        .import_epub_book("alice", "bad.epub", &traversal)
        .await
        .expect_err("traversal path should be rejected");
    assert!(err.to_string().contains("非法路径") || err.to_string().contains("缺少文件"));

    let _ = std::fs::remove_dir_all(storage_dir);
}

#[tokio::test]
async fn real_sample_epub_imports_from_repo_epub_folder_when_present() {
    let sample = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("epub")
        .join("盘龙 (我吃西红柿) (z-library.sk, 1lib.sk, z-lib.sk).epub");
    if !sample.exists() {
        return;
    }
    let storage_dir = std::env::temp_dir().join(format!(
        "reader-rust-local-epub-real-test-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&storage_dir);
    let service = LocalEpubBookService::new(&storage_dir);
    let bytes = std::fs::read(&sample).unwrap();

    let book = service
        .import_epub_book(
            "alice",
            sample.file_name().unwrap().to_str().unwrap(),
            &bytes,
        )
        .await
        .unwrap();
    let chapters = service
        .get_chapter_list("alice", &book.book_url)
        .await
        .unwrap();
    let content = service
        .get_content("alice", &chapters[1].url)
        .await
        .unwrap();

    assert_eq!(book.name, "盘龙");
    assert!(chapters.len() > 100);
    assert!(content.contains("localEpubAsset"));

    let _ = std::fs::remove_dir_all(storage_dir);
}
