pub mod repo;

use sqlx::{
    sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous},
    SqlitePool,
};
use std::{str::FromStr, time::Duration};

/// Build connect options from either a `sqlite:` URL or a bare filesystem path.
///
/// URL parsing rejects Windows paths that contain a drive letter, backslashes or
/// spaces (`D:\Reader Data\reader.db`), so callers that already hold an absolute
/// path can pass it through directly instead of encoding it into a URL.
fn connect_options(database_url: &str) -> anyhow::Result<SqliteConnectOptions> {
    let options = if database_url.starts_with("sqlite:") {
        SqliteConnectOptions::from_str(database_url)?
    } else {
        SqliteConnectOptions::new().filename(database_url)
    };
    Ok(options
        .create_if_missing(true)
        .foreign_keys(true)
        // Concurrent readers should not block a short user/session write.
        .journal_mode(SqliteJournalMode::Wal)
        .synchronous(SqliteSynchronous::Normal)
        // Windows can retain file locks briefly after a writer completes. Wait
        // instead of immediately returning SQLITE_BUSY to the application.
        .busy_timeout(Duration::from_secs(15)))
}

pub async fn init_pool(database_url: &str) -> anyhow::Result<SqlitePool> {
    let options = connect_options(database_url)?;
    let pool = SqlitePoolOptions::new()
        .max_connections(5)
        .connect_with(options)
        .await?;
    sqlx::migrate!("src/storage/db/migrations")
        .run(&pool)
        .await?;
    Ok(pool)
}
