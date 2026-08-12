pub mod repo;

use sqlx::{
    migrate::{Migration, Migrator},
    sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous},
    SqlitePool,
};
use std::{borrow::Cow, str::FromStr, time::Duration};

static EMBEDDED_MIGRATOR: Migrator = sqlx::migrate!("src/storage/db/migrations");

fn stable_migrator() -> Migrator {
    let migrations = EMBEDDED_MIGRATOR
        .iter()
        .map(|migration| {
            Migration::new(
                migration.version,
                migration.description.clone(),
                migration.migration_type,
                Cow::Owned(migration.sql.replace("\r\n", "\n")),
            )
        })
        .collect::<Vec<_>>();
    Migrator {
        migrations: Cow::Owned(migrations),
        ..Migrator::DEFAULT
    }
}

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
    let migrator = stable_migrator();
    repair_legacy_line_ending_checksums(&pool, &migrator).await?;
    migrator.run(&pool).await?;
    Ok(pool)
}

/// Older Windows packages could embed CRLF migration scripts while CI packages
/// embedded LF. SQLx hashes raw bytes, so the same SQL then looked modified.
/// Only repair a checksum when it exactly matches the alternate line-ending form.
async fn repair_legacy_line_ending_checksums(
    pool: &SqlitePool,
    migrator: &Migrator,
) -> anyhow::Result<()> {
    let table_exists: i64 = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = '_sqlx_migrations')",
    )
    .fetch_one(pool)
    .await?;
    if table_exists == 0 {
        return Ok(());
    }

    for migration in migrator.iter() {
        let stored: Option<Vec<u8>> =
            sqlx::query_scalar("SELECT checksum FROM _sqlx_migrations WHERE version = ?")
                .bind(migration.version)
                .fetch_optional(pool)
                .await?;
        let Some(stored) = stored else {
            continue;
        };
        if stored.as_slice() == migration.checksum.as_ref() {
            continue;
        }

        let normalized = migration.sql.replace("\r\n", "\n");
        let crlf = normalized.replace('\n', "\r\n");
        let matches_alternate_line_endings = [normalized, crlf].into_iter().any(|sql| {
            if sql.as_bytes() == migration.sql.as_bytes() {
                return false;
            }
            let alternate = Migration::new(
                migration.version,
                migration.description.clone(),
                migration.migration_type,
                Cow::Owned(sql),
            );
            stored.as_slice() == alternate.checksum.as_ref()
        });

        if matches_alternate_line_endings {
            sqlx::query("UPDATE _sqlx_migrations SET checksum = ? WHERE version = ?")
                .bind(migration.checksum.as_ref())
                .bind(migration.version)
                .execute(pool)
                .await?;
            tracing::warn!(
                version = migration.version,
                "repaired legacy migration checksum caused by line endings"
            );
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn migrated_memory_pool() -> SqlitePool {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        stable_migrator().run(&pool).await.unwrap();
        pool
    }

    #[tokio::test]
    async fn repairs_checksum_created_with_windows_line_endings() {
        let pool = migrated_memory_pool().await;
        let migrator = stable_migrator();
        let migration = migrator.iter().find(|item| item.version == 4).unwrap();
        let crlf_sql = migration.sql.replace("\r\n", "\n").replace('\n', "\r\n");
        let crlf_migration = Migration::new(
            migration.version,
            migration.description.clone(),
            migration.migration_type,
            Cow::Owned(crlf_sql),
        );
        sqlx::query("UPDATE _sqlx_migrations SET checksum = ? WHERE version = 4")
            .bind(crlf_migration.checksum.as_ref())
            .execute(&pool)
            .await
            .unwrap();

        repair_legacy_line_ending_checksums(&pool, &migrator)
            .await
            .unwrap();
        migrator.run(&pool).await.unwrap();

        let checksum: Vec<u8> =
            sqlx::query_scalar("SELECT checksum FROM _sqlx_migrations WHERE version = 4")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(checksum.as_slice(), migration.checksum.as_ref());
    }

    #[tokio::test]
    async fn keeps_rejecting_genuinely_modified_migrations() {
        let pool = migrated_memory_pool().await;
        let migrator = stable_migrator();
        sqlx::query("UPDATE _sqlx_migrations SET checksum = x'010203' WHERE version = 4")
            .execute(&pool)
            .await
            .unwrap();

        repair_legacy_line_ending_checksums(&pool, &migrator)
            .await
            .unwrap();
        let error = migrator.run(&pool).await.unwrap_err();

        assert!(error.to_string().contains("migration 4"));
    }
}
