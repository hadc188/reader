use std::sync::Arc;
use tracing_subscriber::EnvFilter;

use crate::api::AppState;
use crate::app::config::AppConfig;
use crate::crawler::http_client::HttpClient;
use crate::parser::rule_engine::RuleEngine;
use crate::service::{
    book_group_service::BookGroupService, book_service::BookService,
    book_source_service::BookSourceService, http_tts_service::HttpTtsService,
    json_document_service::JsonDocumentService, local_epub_book::LocalEpubBookService,
    local_pdf_book::LocalPdfBookService, local_txt_book::LocalTxtBookService,
    reading_stats_service::ReadingStatsService, update_service::UpdateService,
    user_service::UserService,
};
use crate::storage::{cache::file_cache::FileCache, db, fs::storage_fs::StorageFs};

/// Install the global tracing subscriber.
///
/// Safe to call more than once — a second call is ignored instead of panicking,
/// so an embedding process can install its own subscriber first.
pub fn init_tracing(log_level: &str) {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::new(log_level))
        .try_init();
}

/// Prepare storage, run migrations and wire up every service.
pub async fn build_state(cfg: &AppConfig) -> anyhow::Result<AppState> {
    let storage_fs = StorageFs::new(&cfg.storage_dir, &cfg.assets_dir);
    storage_fs.ensure().await?;
    crate::parser::js::set_js_storage_dir(&cfg.storage_dir);

    let pool = db::init_pool(&cfg.database_url).await?;
    let repo = db::repo::BookSourceRepo::new(pool.clone());

    let http = HttpClient::new(cfg.request_timeout_secs, None)?;
    crate::parser::js::set_js_http_client(http.clone());
    let parser = RuleEngine::new()?;
    let cache = FileCache::new(format!("{}/cache", cfg.storage_dir));

    let book_service = Arc::new(BookService::new(http.clone(), parser, cache, &cfg.storage_dir));
    // Search results can be abandoned by a crash or forced quit, bypassing the
    // frontend's session cleanup. Only unreferenced files older than the
    // threshold are removed, so an active cover cache is never evicted.
    match book_service
        .clean_stale_search_covers(std::time::Duration::from_secs(30 * 60))
        .await
    {
        Ok(removed) if removed > 0 => {
            tracing::info!(removed, "cleaned stale search cover cache")
        }
        Ok(_) => {}
        Err(err) => tracing::warn!("failed to clean stale search cover cache: {err}"),
    }
    // 章节正文缓存没有任何自动淘汰：从搜索结果/发现页读过、但没加入书架的书，
    // 其 `cache/default/<md5(bookUrl)>/` 在书删除路径里匹配不到，会永久残留。
    // 这里回收闲置超过 24 小时的孤儿目录；书架上的书一律豁免。
    // 24 小时而不是 30 分钟：用户可能今天读了几章、明天才决定加书架。
    match book_service
        .clean_orphan_chapter_caches(std::time::Duration::from_secs(24 * 60 * 60))
        .await
    {
        Ok(removed) if removed > 0 => {
            tracing::info!(removed, "cleaned orphan chapter caches")
        }
        Ok(_) => {}
        Err(err) => tracing::warn!("failed to clean orphan chapter caches: {err}"),
    }
    // 历史版本删书源时只清 cookie 内容、不删 `source-state/<md5>` 目录，攒下大量
    // 空壳（实测 907 个里 870 个是空的）。这里回收闲置超过 24 小时的空目录；
    // 有 cookie/state 的目录是用户登录态，一律保留。
    let removed_state_dirs = book_service
        .clean_empty_source_state_dirs(std::time::Duration::from_secs(24 * 60 * 60))
        .await;
    if removed_state_dirs > 0 {
        tracing::info!(removed_state_dirs, "cleaned empty source-state dirs");
    }
    let book_source_service = Arc::new(BookSourceService::new(repo, &cfg.storage_dir));
    let local_txt_book_service = Arc::new(LocalTxtBookService::new(&cfg.storage_dir));
    let local_epub_book_service = Arc::new(LocalEpubBookService::new(&cfg.storage_dir));
    let local_pdf_book_service = Arc::new(LocalPdfBookService::new(&cfg.storage_dir));
    let json_document_service = Arc::new(JsonDocumentService::new(pool.clone(), &cfg.storage_dir));
    let http_tts_service = Arc::new(HttpTtsService::new(json_document_service.clone()));
    let user_service = Arc::new(UserService::new(cfg.clone(), pool.clone()));
    user_service.migrate_legacy_users_from_json().await?;
    let book_group_service = Arc::new(BookGroupService::new(json_document_service.clone()));
    // 一次性修复 issue #9 的存量数据: 旧版给新分组分配 max+1 产生非 2 幂 id,
    // 书加入后同时命中多个分组。幂等, 干净数据不写任何文件。
    // 失败只记日志、不阻断启动 —— 迁移本身是修复性操作, 数据越脏越不该让
    // 应用起不来; 下次启动会重试(先书架后分组的落盘顺序保证可重入)。
    let book_service_for_read = book_service.clone();
    let book_service_for_write = book_service.clone();
    match book_group_service
        .migrate_non_power_of_two_group_ids(
            "default",
            // 闭包持有自己的 Arc 克隆, future 才不借用 bootstrap 的局部变量。
            move || {
                let service = book_service_for_read.clone();
                async move { service.get_bookshelf("default").await }
            },
            move |books| {
                let service = book_service_for_write.clone();
                async move { service.save_books("default", books).await.map(|_| ()) }
            },
        )
        .await
    {
        Ok(true) => tracing::info!("已迁移非 2 幂分组 id(修复书籍同时出现在多个分组的问题)"),
        Ok(false) => {}
        Err(err) => tracing::error!("分组 id 迁移失败(不阻断启动, 下次启动重试): {err}"),
    }
    let reading_stats_service = Arc::new(ReadingStatsService::new(pool.clone()));
    let update_service = Arc::new(UpdateService::new(
        json_document_service.clone(),
        http.clone(),
        format!("v{}", env!("CARGO_PKG_VERSION")),
    ));

    Ok(AppState {
        config: cfg.clone(),
        book_service,
        book_source_service,
        user_service,
        book_group_service,
        local_txt_book_service,
        local_epub_book_service,
        local_pdf_book_service,
        json_document_service,
        reading_stats_service,
        update_service,
        http_tts_service,
    })
}
