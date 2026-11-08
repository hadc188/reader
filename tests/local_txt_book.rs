use reader_rust::service::local_txt_book::{build_chapter_url, parse_txt_chapters};

#[test]
fn txt_parser_splits_common_chapter_headings() {
    let text = "序言\n这里是序言。\n第1章 初见\n第一章正文。\n第二章 重逢\n第二章正文。\n";

    let chapters = parse_txt_chapters("local-txt:abc123", text);

    assert_eq!(chapters.len(), 3);
    assert_eq!(chapters[0].title, "序章");
    assert_eq!(chapters[0].url, "local-txt:abc123#0");
    assert_eq!(chapters[0].content.trim(), "序言\n这里是序言。");
    assert_eq!(chapters[1].title, "第1章 初见");
    assert_eq!(chapters[1].content.trim(), "第一章正文。");
    assert_eq!(chapters[2].title, "第二章 重逢");
    assert_eq!(chapters[2].content.trim(), "第二章正文。");
}

#[test]
fn txt_parser_splits_no_space_chapter_headings() {
    let text = "第一章初见\n第一章正文。\n第2章重逢\n第二章正文。\n";

    let chapters = parse_txt_chapters("local-txt:nospace", text);

    assert_eq!(chapters.len(), 2);
    assert_eq!(chapters[0].title, "第一章初见");
    assert_eq!(chapters[0].content.trim(), "第一章正文。");
    assert_eq!(chapters[1].title, "第2章重逢");
    assert_eq!(chapters[1].content.trim(), "第二章正文。");
}

#[test]
fn txt_parser_falls_back_to_single_body_chapter() {
    let text = "没有章节标题\n只有正文\n";

    let chapters = parse_txt_chapters("local-txt:book", text);

    assert_eq!(chapters.len(), 1);
    assert_eq!(chapters[0].title, "正文");
    assert_eq!(chapters[0].url, "local-txt:book#0");
    assert_eq!(chapters[0].content, text);
}

#[test]
fn txt_parser_allows_empty_chapters_between_adjacent_headings() {
    let text = "第1章 空章\n第二章 正文\n第二章内容。\n";

    let chapters = parse_txt_chapters("local-txt:empty", text);

    assert_eq!(chapters.len(), 2);
    assert_eq!(chapters[0].title, "第1章 空章");
    assert_eq!(chapters[0].content, "");
    assert_eq!(chapters[1].title, "第二章 正文");
    assert_eq!(chapters[1].content.trim(), "第二章内容。");
}

#[test]
fn txt_chapter_urls_are_stable_by_book_url_and_index() {
    assert_eq!(
        build_chapter_url("local-txt:abc123", 7),
        "local-txt:abc123#7"
    );
}

#[tokio::test]
async fn txt_import_saves_book_index_and_content_for_server_bookshelf() {
    let storage_dir =
        std::env::temp_dir().join(format!("reader-rust-local-txt-test-{}", std::process::id()));
    if storage_dir.exists() {
        std::fs::remove_dir_all(&storage_dir).unwrap();
    }
    let service = reader_rust::service::local_txt_book::LocalTxtBookService::new(&storage_dir);
    let bytes = "第1章 开始\n第一章内容。\n第二章 后来\n第二章内容。".as_bytes();

    let book = service
        .import_txt_book("alice", "测试小说.txt", bytes)
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

    assert_eq!(book.name, "测试小说");
    assert_eq!(book.origin, "local-txt");
    assert_eq!(book.origin_name.as_deref(), Some("本地 TXT"));
    assert_eq!(book.can_update, Some(false));
    assert_eq!(chapters.len(), 2);
    assert_eq!(chapters[0].title, "第1章 开始");
    assert_eq!(chapters[1].title, "第二章 后来");
    assert_eq!(content.trim(), "第二章内容。");

    let _ = std::fs::remove_dir_all(storage_dir);
}

#[tokio::test]
async fn txt_import_rejects_non_txt_files() {
    let storage_dir = std::env::temp_dir().join(format!(
        "reader-rust-local-txt-reject-test-{}",
        std::process::id()
    ));
    let service = reader_rust::service::local_txt_book::LocalTxtBookService::new(&storage_dir);

    let err = service
        .import_txt_book("alice", "测试小说.md", "正文".as_bytes())
        .await
        .expect_err("non txt file should be rejected");

    assert!(err.to_string().contains(".txt"));
    let _ = std::fs::remove_dir_all(storage_dir);
}

#[tokio::test]
async fn txt_service_rejects_non_hash_local_txt_urls() {
    let storage_dir = std::env::temp_dir().join(format!(
        "reader-rust-local-txt-path-test-{}",
        std::process::id()
    ));
    let service = reader_rust::service::local_txt_book::LocalTxtBookService::new(&storage_dir);

    let err = service
        .get_chapter_list("alice", "local-txt:/tmp/escape")
        .await
        .expect_err("absolute local txt paths should be rejected");

    assert!(err.to_string().contains("本地 TXT 地址无效"));
    let _ = std::fs::remove_dir_all(storage_dir);
}

#[tokio::test]
async fn txt_delete_removes_imported_book_files() {
    let storage_dir = std::env::temp_dir().join(format!(
        "reader-rust-local-txt-delete-test-{}",
        std::process::id()
    ));
    if storage_dir.exists() {
        std::fs::remove_dir_all(&storage_dir).unwrap();
    }
    let service = reader_rust::service::local_txt_book::LocalTxtBookService::new(&storage_dir);
    let book = service
        .import_txt_book("alice", "待删除.txt", "第一章 开始\n正文".as_bytes())
        .await
        .unwrap();
    let chapters = service
        .get_chapter_list("alice", &book.book_url)
        .await
        .unwrap();

    assert!(service
        .delete_book_files("alice", &book.book_url)
        .await
        .unwrap());
    let err = service
        .get_content("alice", &chapters[0].url)
        .await
        .expect_err("deleted local txt content should not remain readable");

    assert!(err.to_string().contains("不存在") || err.to_string().contains("No such file"));
    let _ = std::fs::remove_dir_all(storage_dir);
}

#[tokio::test]
async fn txt_reimport_reuses_url_and_still_reads_the_new_file() {
    // 同一份文件名 + 同样内容会得到同一个 book_url（哈希只取名字与内容）。
    // 目录缓存必须跟着失效, 否则会拿旧分章的字节区间去切新文件。
    let storage_dir = std::env::temp_dir().join(format!(
        "reader-rust-local-txt-reimport-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&storage_dir);
    let service = reader_rust::service::local_txt_book::LocalTxtBookService::new(&storage_dir);
    let text = "第一章 开始\n第一版正文。\n第二章 后来\n第二版正文。\n";

    let first = service
        .import_txt_book("alice", "同一本.txt", text.as_bytes())
        .await
        .unwrap();
    let before = service
        .get_chapter_list("alice", &first.book_url)
        .await
        .unwrap();
    // 先读一次把目录灌进缓存
    assert_eq!(
        service
            .get_content("alice", &before[1].url)
            .await
            .unwrap()
            .trim(),
        "第二版正文。"
    );

    let second = service
        .import_txt_book("alice", "同一本.txt", text.as_bytes())
        .await
        .unwrap();
    assert_eq!(first.book_url, second.book_url);

    let chapters = service
        .get_chapter_list("alice", &second.book_url)
        .await
        .unwrap();
    assert_eq!(chapters.len(), 2);
    assert_eq!(
        service
            .get_content("alice", &chapters[1].url)
            .await
            .unwrap()
            .trim(),
        "第二版正文。"
    );

    let _ = std::fs::remove_dir_all(storage_dir);
}

/// 正文按字节区间读取(不整本读盘): 章节序号跨越整本书时也要取到正确的一段。
#[tokio::test]
async fn txt_content_reads_only_the_chapter_byte_range() {
    let storage_dir = std::env::temp_dir().join(format!(
        "reader-rust-local-txt-range-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&storage_dir);
    let service = reader_rust::service::local_txt_book::LocalTxtBookService::new(&storage_dir);

    let mut text = String::new();
    for index in 1..=200 {
        text.push_str(&format!("第{index}章 标题\n第{index}章正文。\n"));
    }
    let book = service
        .import_txt_book("alice", "多章.txt", text.as_bytes())
        .await
        .unwrap();
    let chapters = service
        .get_chapter_list("alice", &book.book_url)
        .await
        .unwrap();
    assert_eq!(chapters.len(), 200);

    let last = service
        .get_content("alice", &chapters[199].url)
        .await
        .unwrap();
    assert_eq!(last.trim(), "第200章正文。");

    let _ = std::fs::remove_dir_all(storage_dir);
}

#[tokio::test]
async fn saving_same_named_local_txt_books_keeps_distinct_book_urls() {
    use reader_rust::crawler::http_client::HttpClient;
    use reader_rust::parser::rule_engine::RuleEngine;
    use reader_rust::service::book_service::BookService;
    use reader_rust::storage::cache::file_cache::FileCache;

    let storage_dir = std::env::temp_dir().join(format!(
        "reader-rust-local-txt-save-test-{}",
        std::process::id()
    ));
    if storage_dir.exists() {
        std::fs::remove_dir_all(&storage_dir).unwrap();
    }

    let local_service =
        reader_rust::service::local_txt_book::LocalTxtBookService::new(&storage_dir);
    let book_service = BookService::new(
        HttpClient::new(5, None).unwrap(),
        RuleEngine::new().unwrap(),
        FileCache::new(storage_dir.join("cache")),
        storage_dir.to_str().unwrap(),
    );

    let first = local_service
        .import_txt_book("alice", "同名小说.txt", "第一版正文".as_bytes())
        .await
        .unwrap();
    let second = local_service
        .import_txt_book("alice", "同名小说.txt", "第二版正文".as_bytes())
        .await
        .unwrap();

    book_service
        .save_book("alice", first.clone())
        .await
        .unwrap();
    book_service
        .save_book("alice", second.clone())
        .await
        .unwrap();
    let shelf = book_service.get_bookshelf("alice").await.unwrap();

    assert_ne!(first.book_url, second.book_url);
    assert_eq!(shelf.len(), 2);
    assert!(shelf.iter().any(|book| book.book_url == first.book_url));
    assert!(shelf.iter().any(|book| book.book_url == second.book_url));

    let _ = std::fs::remove_dir_all(storage_dir);
}

/// issue #33：`第一卷` 单独成章，但正文为空 —— 必须标 isVolume，
/// 否则前端会把它当成"书源正文解析为空"报错。
#[test]
fn txt_parser_marks_volume_headings_as_volume_chapters() {
    let text = "序章正文。\n\n第一卷\n\n第一章 初见\n第一章正文。\n\n第二章 重逢\n第二章正文。\n";
    let chapters = parse_txt_chapters("local-txt:volume", text);

    assert_eq!(chapters.len(), 4);
    assert_eq!(chapters[0].title, "序章");
    assert!(!chapters[0].is_volume);

    assert_eq!(chapters[1].title, "第一卷");
    assert!(chapters[1].is_volume, "卷标题必须标 isVolume");
    assert_eq!(chapters[1].content, "", "卷标题章正文为空");

    assert_eq!(chapters[2].title, "第一章 初见");
    assert!(!chapters[2].is_volume, "普通章节不能被误标成卷");
}

/// 卷名带副标题（`第一卷 风起云涌`）同样要认出来。
#[test]
fn txt_parser_marks_volume_with_subtitle() {
    let text = "第一卷 风起云涌\n\n第一章 初见\n正文。\n";
    let chapters = parse_txt_chapters("local-txt:volume-sub", text);

    assert_eq!(chapters.len(), 2);
    assert_eq!(chapters[0].title, "第一卷 风起云涌");
    assert!(chapters[0].is_volume);
}

/// 部/篇/集 也属于卷级标题。
#[test]
fn txt_parser_marks_other_volume_units() {
    for title in ["第二部", "第三篇", "第四集", "卷五"] {
        let text = format!("{title}\n\n第一章 正文\n内容。\n");
        let chapters = parse_txt_chapters("local-txt:unit", &text);
        assert!(chapters[0].is_volume, "{title} 应识别为卷级标题");
    }
}

/// 空正文但标题不是卷级（`第一章` 后紧跟 `第二章`）：不能标卷，
/// 否则真正的缺内容会被伪装成正常的卷标题页。
#[test]
fn txt_parser_does_not_mark_empty_chapter_as_volume() {
    let text = "第一章 空章\n第二章 正文\n第二章内容。\n";
    let chapters = parse_txt_chapters("local-txt:empty-not-volume", text);

    assert_eq!(chapters.len(), 2);
    assert!(!chapters[0].is_volume, "章级空章节不是卷标签");
    assert_eq!(chapters[0].content, "");
}

/// 带副标题的卷名后面若跟着引子，正文不空 —— 此时照常显示文字，不算卷页。
#[test]
fn txt_parser_volume_with_intro_is_not_empty() {
    let text = "第一卷 风起\n本卷引子。\n\n第一章 初见\n正文。\n";
    let chapters = parse_txt_chapters("local-txt:volume-intro", text);

    assert_eq!(chapters[0].title, "第一卷 风起");
    assert!(!chapters[0].is_volume, "有正文时不该被当成空白卷页");
    assert_eq!(chapters[0].content.trim(), "本卷引子。");
}

/// `【第一卷】` 这类成对括号写法要能识别成标题（issue #33 的次要问题）。
/// 修前它会被当成正文吞进上一章。
#[test]
fn txt_parser_recognises_bracketed_headings() {
    let text = "序章正文。\n\n【第一卷】\n\n第一章 初见\n第一章正文。\n";
    let chapters = parse_txt_chapters("local-txt:bracket", text);

    assert_eq!(chapters.len(), 3, "【第一卷】必须单独成章，不能被吞进序章");
    assert_eq!(chapters[0].title, "序章");
    assert_eq!(chapters[0].content.trim(), "序章正文。");
    assert_eq!(chapters[1].title, "【第一卷】");
    assert!(chapters[1].is_volume, "括号包裹的卷名同样是卷");
    assert_eq!(chapters[1].content, "");
    assert_eq!(chapters[2].title, "第一章 初见");
}

/// 括号包裹的**普通**章节也要认（`（第二章 初见）`），但不标卷。
#[test]
fn txt_parser_recognises_bracketed_normal_chapters() {
    let text = "（第一章 开始）\n正文一。\n（第二章 继续）\n正文二。\n";
    let chapters = parse_txt_chapters("local-txt:bracket-ch", text);

    assert_eq!(chapters.len(), 2);
    assert_eq!(chapters[0].title, "（第一章 开始）");
    assert!(!chapters[0].is_volume);
    assert_eq!(chapters[0].content.trim(), "正文一。");
}

/// 正文里夹着的括号提示语不能误判成分章。
#[test]
fn txt_parser_ignores_bracketed_prose() {
    let text = "第一章 开始\n他看了一眼【此处删除】然后继续。\n第二章 结束\n正文二。\n";
    let chapters = parse_txt_chapters("local-txt:bracket-prose", text);

    assert_eq!(chapters.len(), 2, "正文中的【此处删除】不是标题");
    assert!(chapters[0].content.contains("【此处删除】"));
}
/// 旧版导入的书（chapters.json 里没有 isVolume）读出来也要能识别卷章，
/// 否则用户必须删书重导才能修好 issue #33。
#[tokio::test]
async fn legacy_index_without_is_volume_is_upgraded_on_read() {
    let storage_dir = std::env::temp_dir().join(format!(
        "reader-rust-local-txt-legacy-volume-{}",
        std::process::id()
    ));
    if storage_dir.exists() {
        std::fs::remove_dir_all(&storage_dir).unwrap();
    }
    let service =
        reader_rust::service::local_txt_book::LocalTxtBookService::new(&storage_dir);

    let text = "序章正文。\n\n第一卷\n\n第一章 初见\n第一章正文。\n";
    let book = service
        .import_txt_book("alice", "旧书.txt", text.as_bytes())
        .await
        .unwrap();

    // 把 chapters.json 改写成旧格式：抹掉 isVolume 字段。
    let book_dir = storage_dir
        .join("data")
        .join("alice")
        .join("local_books")
        .join(book.book_url.strip_prefix("local-txt:").unwrap());
    let index_path = book_dir.join("chapters.json");
    let raw = std::fs::read_to_string(&index_path).unwrap();
    let mut value: serde_json::Value = serde_json::from_str(&raw).unwrap();
    for chapter in value["chapters"].as_array_mut().unwrap() {
        chapter.as_object_mut().unwrap().remove("isVolume");
    }
    std::fs::write(&index_path, serde_json::to_string_pretty(&value).unwrap()).unwrap();

    let chapters = service
        .get_chapter_list("alice", &book.book_url)
        .await
        .unwrap();
    let volume = chapters.iter().find(|c| c.title == "第一卷").unwrap();
    assert!(volume.is_volume, "旧索引里的卷章必须在读取时补标");
    let normal = chapters.iter().find(|c| c.title == "第一章 初见").unwrap();
    assert!(!normal.is_volume);

    let _ = std::fs::remove_dir_all(storage_dir);
}