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

/// 复刻一类多用（杜坎）排版：章节标题被 `<br/>` 拆成两段、标题前有 logo、
/// 封面用 SVG `<image>`、样式表里有 `@import`/`@font-face` 与相对 `url()`。
fn styled_fixture() -> Vec<u8> {
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
<package version="2.0" xmlns="http://www.idpf.org/2007/opf">
  <metadata xmlns:dc="http://purl.org/dc/elements/1.1/">
    <dc:title>示例书名</dc:title>
    <dc:creator>示例作者</dc:creator>
    <meta name="cover" content="cover.jpg"/>
  </metadata>
  <manifest>
    <item id="ncx" href="toc.ncx" media-type="application/x-dtbncx+xml"/>
    <item id="css" href="Styles/main.css" media-type="text/css"/>
    <item id="cover.jpg" href="Images/cover.jpg" media-type="image/jpeg"/>
    <item id="pic" href="Images/pic.png" media-type="image/png"/>
    <item id="text0001" href="Text/text0001.html" media-type="application/xhtml+xml"/>
    <item id="text0002" href="Text/text0002.html" media-type="application/xhtml+xml"/>
    <item id="coverpage" href="Text/coverpage.html" media-type="application/xhtml+xml"/>
  </manifest>
  <spine>
    <itemref idref="text0001"/>
    <itemref idref="coverpage"/>
    <itemref idref="text0002"/>
  </spine>
</package>"#,
            ),
            (
                "OEBPS/toc.ncx",
                r#"<ncx><navMap>
<navPoint><navLabel><text>第001章 星空中的青铜巨棺</text></navLabel><content src="Text/text0001.html"/></navPoint>
<navPoint><navLabel><text>Cover</text></navLabel><content src="Text/coverpage.html"/></navPoint>
<navPoint><navLabel><text>第002章 素 问</text></navLabel><content src="Text/text0002.html"/></navPoint>
</navMap></ncx>"#,
            ),
            (
                "OEBPS/Styles/main.css",
                // `@charset`/`@import` 之后**直接**跟普通规则: 曾经会把紧随其后的
                // 第一条规则(body)当成 at-rule 内容一起丢掉。`@font-face` 放在后面,
                // 免得它替 body 挡了这一刀。
                r#"@charset "utf-8";
@import url("../Styles/fonts.css");
body { text-align: justify; }
@font-face { font-family: "zw"; src: url(../Fonts/zw.ttf); }
p { text-indent: 2em; font-family: "DK-SONGTI", "宋体", serif; }
div { margin: 0; }
p.foot { text-align: center; duokan-text-indent: 0em; }
.speci { background: url(../Images/pic.png) no-repeat bottom right; }
"#,
            ),
            (
                "OEBPS/Text/text0001.html",
                r#"<html xmlns="http://www.w3.org/1999/xhtml"><head>
<link href="../Styles/main.css" rel="stylesheet" type="text/css"/>
<title></title></head>
<body>
  <div>
    <div class="logo"><img alt="logo" class="logo" src="../Images/pic.png"/></div>
    <h1 class="head">第001章<br/>
    <b>星空中的青铜巨棺</b></h1>
    <p>生命是世间最伟大的奇迹。</p>
  </div>
</body></html>"#,
            ),
            (
                "OEBPS/Text/coverpage.html",
                r#"<html xmlns="http://www.w3.org/1999/xhtml"><body>
<div style="text-align: center;">
<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 960 1280" width="100%" height="100%"><image width="960" height="1280" xlink:href="../Images/cover.jpg"/></svg>
</div>
</body></html>"#,
            ),
            (
                // 标题写在 `<p class="title">` 里(整个 class 值就是 title)。
                "OEBPS/Text/text0002.html",
                r#"<html xmlns="http://www.w3.org/1999/xhtml"><head>
<link href="../Styles/main.css" rel="stylesheet" type="text/css"/>
<title></title></head>
<body>
  <p class="title">第002章 素 问</p>
  <p>第二段正文。</p>
</body></html>"#,
            ),
        ],
        &[
            ("OEBPS/Images/cover.jpg", b"\xff\xd8\xffcover"),
            ("OEBPS/Images/pic.png", b"\x89PNG\r\n\x1a\npic"),
        ],
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
    // 正文 <h1> 与目录标题相同: 阅读页已统一渲染目录标题, 正文这份要删掉。
    assert!(!content.contains("<h1>第一章</h1>"));
    assert!(content.contains("epub-chapter-content"));
    assert!(content.contains("第一段正文。"));
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
async fn epub_import_keeps_book_styles_scoped_to_the_chapter() {
    let storage_dir = std::env::temp_dir().join(format!(
        "reader-rust-local-epub-style-test-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&storage_dir);
    let service = LocalEpubBookService::new(&storage_dir);
    let book = service
        .import_epub_book("alice", "styled.epub", &styled_fixture())
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

    // 原书 CSS 被保留并加了作用域前缀, 不会波及阅读器界面。
    assert!(content.contains("<style>"));
    assert!(content.contains(".epub-chapter-content p{"));
    // `@charset`/`@import` 之后的 body 规则必须还在(且已作用域化) —— 它曾经被
    // 误当成 at-rule 的内容整块丢掉。
    assert!(content.contains(".epub-chapter-content{"));
    // 选择器必须带作用域前缀: 裸 `p{...}`/`body{...}` 会波及阅读器界面。
    // `body{}` 在这里映射成容器自身, 否则 `.scope body` 选不到任何元素。
    assert!(!content.contains(" body{"));
    assert!(!content.contains(".epub-chapter-content body"));
    assert!(content.contains(".epub-chapter-content div{"));
    // `@import`/`@font-face` 被丢弃: 字体文件不在资源白名单里, 内联只会产生 404。
    assert!(!content.contains("@import"));
    assert!(!content.contains("@font-face"));
    // 字体交给阅读器的「字体」设置: 原书的 font-family 若留着会以更高优先级命中
    // 正文元素, 用户换字体就没有任何反应。
    assert!(!content.contains("font-family"), "font-family must be dropped: {content}");
    // 但同一规则里的其它排版声明要保留(只接管字体, 不动字号/缩进)。
    assert!(content.contains("text-indent:2em"));
    // CSS 里的相对 url() 改写成资源接口地址。
    assert!(content.contains("localEpubAsset"));
    assert!(!content.contains("url(../Images/pic.png)"));

    let _ = std::fs::remove_dir_all(storage_dir);
}

#[tokio::test]
async fn epub_import_strips_chapter_heading_split_by_br() {
    let storage_dir = std::env::temp_dir().join(format!(
        "reader-rust-local-epub-heading-test-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&storage_dir);
    let service = LocalEpubBookService::new(&storage_dir);
    let book = service
        .import_epub_book("alice", "styled.epub", &styled_fixture())
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

    // 目录标题是「第001章 星空中的青铜巨棺」, 正文里的 h1 被 <br/> 拆成两段,
    // 且前面还有一个 logo 容器 —— 两者都不该妨碍去重。
    assert!(chapters[0].title.contains("第001章"));
    assert!(!content.contains("<h1"));
    assert!(!content.contains("星空中的青铜巨棺</b>"));
    assert!(content.contains("生命是世间最伟大的奇迹"));

    let _ = std::fs::remove_dir_all(storage_dir);
}

#[tokio::test]
async fn epub_import_keeps_svg_cover_page() {
    let storage_dir = std::env::temp_dir().join(format!(
        "reader-rust-local-epub-svg-test-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&storage_dir);
    let service = LocalEpubBookService::new(&storage_dir);
    let book = service
        .import_epub_book("alice", "styled.epub", &styled_fixture())
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

    // 封面页没有 `<link rel=stylesheet>`, 走的是兜底样式: 它同样必须带作用域前缀,
    // 否则裸 `svg{display:block}` 会命中阅读器界面里的所有内联图标。
    assert!(content.contains("<svg"));
    assert!(content.contains("<image"));
    assert!(content.contains("xlink:href"));
    assert!(content.contains("localEpubAsset"));
    assert!(content.contains(".epub-chapter-content img"));
    assert!(content.contains(".epub-chapter-content svg"));
    assert!(!content.contains("<style>img"));
    assert!(book
        .cover_url
        .as_deref()
        .unwrap_or_default()
        .contains("cover.jpg"));

    let _ = std::fs::remove_dir_all(storage_dir);
}

#[tokio::test]
async fn legacy_manifest_without_stylesheets_is_rebuilt_on_read() {
    let storage_dir = std::env::temp_dir().join(format!(
        "reader-rust-local-epub-rebuild-test-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&storage_dir);
    let service = LocalEpubBookService::new(&storage_dir);
    let book = service
        .import_epub_book("alice", "styled.epub", &styled_fixture())
        .await
        .unwrap();

    // 模拟 v2 版本的索引: 章节只有落盘文件名(没有 path), 也没有版本号。
    let hash = book.book_url.trim_start_matches("local-epub:");
    let dir = storage_dir
        .join("data")
        .join("alice")
        .join("local_books")
        .join(hash);
    std::fs::create_dir_all(dir.join("chapters")).unwrap();
    std::fs::write(dir.join("chapters").join("0.html"), "<h1>第001章<br/>星空中的青铜巨棺</h1><p>旧正文</p>").unwrap();

    let mut manifest: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(dir.join("manifest.json")).unwrap()).unwrap();
    manifest.as_object_mut().unwrap().remove("engineVersion");
    let chapters = manifest.get_mut("chapters").unwrap().as_array_mut().unwrap();
    for (index, chapter) in chapters.iter_mut().enumerate() {
        let object = chapter.as_object_mut().unwrap();
        object.remove("path");
        object.insert("fileName".to_string(), serde_json::json!(format!("{index}.html")));
    }
    std::fs::write(
        dir.join("manifest.json"),
        serde_json::to_string_pretty(&manifest).unwrap(),
    )
    .unwrap();

    // 读目录时触发重建: 正文改为按需从 original.epub 解压, 样式回来了。
    let chapters = service
        .get_chapter_list("alice", &book.book_url)
        .await
        .unwrap();
    let content = service
        .get_content("alice", &chapters[0].url)
        .await
        .unwrap();
    assert!(content.contains("<style>"));
    assert!(content.contains(".epub-chapter-content p{"));
    assert!(!content.contains("旧正文"));
    assert!(content.contains("生命是世间最伟大的奇迹"));

    // 重建结果要写回 manifest, 否则每次读取都会重解包一遍。
    let manifest: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(dir.join("manifest.json")).unwrap()).unwrap();
    assert_eq!(
        manifest.get("engineVersion").and_then(|value| value.as_u64()),
        Some(4),
    );
    // 正文按需解压: 索引里记的是 zip 路径, 逐章 HTML 不再生成, 旧的也要清掉。
    assert!(!manifest.get("chapters").unwrap()[0].get("fileName").is_some());
    assert_eq!(
        manifest.get("chapters").unwrap()[0].get("path").and_then(|value| value.as_str()),
        Some("OEBPS/Text/text0001.html"),
    );
    assert!(!dir.join("chapters").exists(), "legacy chapter files should be cleaned up");

    // 导入阶段不再统计字数(0 表示未知)。
    let chars = manifest.get("charLen").and_then(|value| value.as_u64()).unwrap_or(0);
    assert_eq!(chars, 0);

    let _ = std::fs::remove_dir_all(storage_dir);
}

#[tokio::test]
async fn epub_rebuild_failure_is_not_retried_for_the_same_engine_version() {
    let storage_dir = std::env::temp_dir().join(format!(
        "reader-rust-local-epub-marker-test-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&storage_dir);
    let service = LocalEpubBookService::new(&storage_dir);
    let book = service
        .import_epub_book("alice", "styled.epub", &styled_fixture())
        .await
        .unwrap();

    let hash = book.book_url.trim_start_matches("local-epub:");
    let dir = storage_dir
        .join("data")
        .join("alice")
        .join("local_books")
        .join(hash);
    // v2 旧索引(章节只有 fileName) + 损坏的 original.epub: 重建必然失败。
    let mut manifest: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(dir.join("manifest.json")).unwrap()).unwrap();
    manifest.as_object_mut().unwrap().remove("engineVersion");
    let chapter_array = manifest.get_mut("chapters").unwrap().as_array_mut().unwrap();
    for (index, chapter) in chapter_array.iter_mut().enumerate() {
        let object = chapter.as_object_mut().unwrap();
        object.remove("path");
        object.insert("fileName".to_string(), serde_json::json!(format!("{index}.html")));
    }
    std::fs::write(
        dir.join("manifest.json"),
        serde_json::to_string_pretty(&manifest).unwrap(),
    )
    .unwrap();
    std::fs::write(dir.join("original.epub"), b"not a zip at all").unwrap();
    // v2 遗留的落盘章节: 迁移失败时仍要能读出来, 不能让书打不开。
    // 里面的 `<img src>` 是 v2 写入时已改写好的绝对地址 —— 不能再清洗一遍, 否则
    // 会被当成非法相对路径丢掉, 插图全部消失。
    std::fs::create_dir_all(dir.join("chapters")).unwrap();
    std::fs::write(
        dir.join("chapters").join("0.html"),
        "<div class=\"epub-chapter-content\"><p>v2 遗留正文</p>\
<img src=\"/reader3/localEpubAsset?bookUrl=local-epub%3Aabc&amp;path=OEBPS%2FImages%2Fpic.png\"/></div>",
    )
    .unwrap();

    // 重建失败要退回旧索引(不能因为修不好就让书打不开)。
    let chapters = service
        .get_chapter_list("alice", &book.book_url)
        .await
        .unwrap();
    assert_eq!(chapters.len(), 3);
    let legacy_content = service
        .get_content("alice", &chapters[0].url)
        .await
        .unwrap();
    assert!(legacy_content.contains("v2 遗留正文"));
    assert!(
        legacy_content.contains("localEpubAsset"),
        "v2 fallback must keep already-rewritten image urls: {legacy_content}"
    );

    // 失败哨兵记下当前引擎版本: 同一版本不再重试。
    let marker = dir.join(".rebuild-failed");
    assert_eq!(std::fs::read_to_string(&marker).unwrap().trim(), "4");

    // 版本升级后允许重试(哨兵里的旧版本号不再匹配)。
    //
    // 必须换一个 service 实例: 目录缓存(manifest 的解析结果)只活在进程内, 而
    // "版本升级"在真实场景里就是换了新二进制、缓存自然清空。用同一个实例改写
    // 哨兵文件会命中缓存, 测不到重试判定。
    std::fs::write(&marker, "1").unwrap();
    let service = LocalEpubBookService::new(&storage_dir);
    let chapters = service
        .get_chapter_list("alice", &book.book_url)
        .await
        .unwrap();
    assert_eq!(chapters.len(), 3);
    assert_eq!(std::fs::read_to_string(&marker).unwrap().trim(), "4");

    let _ = std::fs::remove_dir_all(storage_dir);
}

#[tokio::test]
async fn epub_import_strips_paragraph_whose_class_is_exactly_title() {
    let storage_dir = std::env::temp_dir().join(format!(
        "reader-rust-local-epub-ptitle-test-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&storage_dir);
    let service = LocalEpubBookService::new(&storage_dir);
    let book = service
        .import_epub_book("alice", "styled.epub", &styled_fixture())
        .await
        .unwrap();
    let chapters = service
        .get_chapter_list("alice", &book.book_url)
        .await
        .unwrap();
    let content = service
        .get_content("alice", &chapters[2].url)
        .await
        .unwrap();

    assert_eq!(chapters[2].title, "第002章 素 问");
    assert!(!content.contains("第002章"));
    assert!(content.contains("第二段正文。"));

    let _ = std::fs::remove_dir_all(storage_dir);
}

/// 没有 nav/ncx 的书: 章节名只能从正文开头提取(只读前 8KB, 不整章解压)。
fn no_toc_fixture() -> Vec<u8> {
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
  <metadata xmlns:dc="http://purl.org/dc/elements/1.1/"><dc:title>无目录</dc:title></metadata>
  <manifest>
    <item id="c1" href="Text/a.xhtml" media-type="application/xhtml+xml"/>
    <item id="c2" href="Text/b.xhtml" media-type="application/xhtml+xml"/>
  </manifest>
  <spine><itemref idref="c1"/><itemref idref="c2"/></spine>
</package>"#,
            ),
            (
                "OEBPS/Text/a.xhtml",
                r#"<html xmlns="http://www.w3.org/1999/xhtml"><head><title></title></head>
<body><h1>卷首 无目录之章</h1><p>甲章正文。</p></body></html>"#,
            ),
            (
                "OEBPS/Text/b.xhtml",
                r#"<html xmlns="http://www.w3.org/1999/xhtml"><head><title>乙章标题</title></head>
<body><p>乙章正文。</p></body></html>"#,
            ),
        ],
        &[],
    )
}

#[tokio::test]
async fn epub_without_toc_reads_titles_from_chapter_heads() {
    let storage_dir = std::env::temp_dir().join(format!(
        "reader-rust-local-epub-notoc-test-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&storage_dir);
    let service = LocalEpubBookService::new(&storage_dir);
    let book = service
        .import_epub_book("alice", "notoc.epub", &no_toc_fixture())
        .await
        .unwrap();
    let chapters = service
        .get_chapter_list("alice", &book.book_url)
        .await
        .unwrap();

    assert_eq!(chapters.len(), 2);
    assert_eq!(chapters[0].title, "卷首 无目录之章");
    assert_eq!(chapters[1].title, "乙章标题");

    let content = service
        .get_content("alice", &chapters[0].url)
        .await
        .unwrap();
    assert!(content.contains("甲章正文。"));
    // 标题与目录一致, 正文里那份要去掉。
    assert!(!content.contains("卷首 无目录之章"));

    let _ = std::fs::remove_dir_all(storage_dir);
}

#[tokio::test]
async fn epub_import_defers_chapter_extraction() {
    let storage_dir = std::env::temp_dir().join(format!(
        "reader-rust-local-epub-defer-test-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&storage_dir);
    let service = LocalEpubBookService::new(&storage_dir);
    let book = service
        .import_epub_book("alice", "styled.epub", &styled_fixture())
        .await
        .unwrap();

    let hash = book.book_url.trim_start_matches("local-epub:");
    let dir = storage_dir
        .join("data")
        .join("alice")
        .join("local_books")
        .join(hash);
    // 导入只落原文件 + 资源 + 样式, 不再逐章生成 HTML。
    assert!(dir.join("original.epub").exists());
    assert!(!dir.join("chapters").exists());
    assert!(dir.join("assets").exists());
    assert!(dir.join("styles").exists());

    // 索引里记的是 zip 路径, 不再是落盘文件名。
    let manifest: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(dir.join("manifest.json")).unwrap()).unwrap();
    let first = &manifest.get("chapters").unwrap()[0];
    assert!(first.get("fileName").is_none());
    assert_eq!(
        first.get("path").and_then(|value| value.as_str()),
        Some("OEBPS/Text/text0001.html"),
    );
    // 正文字数改为未知(不再在导入时统计)。
    assert_eq!(manifest.get("charLen").and_then(|v| v.as_u64()), Some(0));
    assert!(book.word_count.is_none());

    // 翻到这一章时才解压出正文。
    let chapters = service
        .get_chapter_list("alice", &book.book_url)
        .await
        .unwrap();
    let content = service
        .get_content("alice", &chapters[0].url)
        .await
        .unwrap();
    assert!(content.contains("生命是世间最伟大的奇迹"));

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
