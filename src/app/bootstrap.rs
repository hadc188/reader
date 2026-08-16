use std::sync::Arc;
use tracing_subscriber::EnvFilter;

use crate::api::AppState;
use crate::app::config::AppConfig;
use crate::crawler::http_client::HttpClient;
use crate::parser::rule_engine::RuleEngine;
use crate::service::{
    book_group_service::BookGroupService, book_service::BookService,
    book_source_service::BookSourceService, json_document_service::JsonDocumentService,
    local_epub_book::LocalEpubBookService, local_pdf_book::LocalPdfBookService,
    local_txt_book::LocalTxtBookService, reading_stats_service::ReadingStatsService,
    update_service::UpdateService, user_service::UserService,
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

    let pool = db::init_pool(&cfg.database_url).await?;
    let repo = db::repo::BookSourceRepo::new(pool.clone());

    let http = HttpClient::new(cfg.request_timeout_secs, None)?;
    let parser = RuleEngine::new()?;
    let cache = FileCache::new(format!("{}/cache", cfg.storage_dir));

    let book_service = Arc::new(BookService::new(http, parser, cache, &cfg.storage_dir));
    let book_source_service = Arc::new(BookSourceService::new(repo, &cfg.storage_dir));
    let local_txt_book_service = Arc::new(LocalTxtBookService::new(&cfg.storage_dir));
    let local_epub_book_service = Arc::new(LocalEpubBookService::new(&cfg.storage_dir));
    let local_pdf_book_service = Arc::new(LocalPdfBookService::new(&cfg.storage_dir));
    let json_document_service = Arc::new(JsonDocumentService::new(pool.clone(), &cfg.storage_dir));
    let user_service = Arc::new(UserService::new(cfg.clone(), pool.clone()));
    user_service.migrate_legacy_users_from_json().await?;
    let book_group_service = Arc::new(BookGroupService::new(json_document_service.clone()));
    let reading_stats_service = Arc::new(ReadingStatsService::new(pool.clone()));
    let update_service = Arc::new(UpdateService::new(
        json_document_service.clone(),
        cfg.request_timeout_secs,
        format!("v{}", env!("CARGO_PKG_VERSION")),
    )?);

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
    })
}