use chrono::Local;
use sqlx::{Row, SqlitePool};

use crate::error::error::AppError;

/// Per-day reading statistics: seconds read and characters read, keyed by user
/// namespace and calendar date (YYYY-MM-DD).
#[derive(Clone)]
pub struct ReadingStatsService {
    pool: SqlitePool,
}

/// A single day's aggregate.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DailyReadingStats {
    pub date: String,
    pub seconds: i64,
    pub characters: i64,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BookReadingStats {
    pub book_url: String,
    pub book_name: String,
    pub seconds: i64,
    pub characters: i64,
    pub last_read_date: String,
}

impl ReadingStatsService {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    /// Today's local date as YYYY-MM-DD.
    fn today() -> String {
        Local::now().format("%Y-%m-%d").to_string()
    }

    /// Add reading time / characters for a given date (defaults to today).
    /// Uses upsert so repeated flushes accumulate rather than overwrite.
    pub async fn add_reading(
        &self,
        user_ns: &str,
        seconds: i64,
        characters: i64,
        date: Option<&str>,
        book_url: Option<&str>,
        book_name: Option<&str>,
    ) -> Result<(), AppError> {
        let date = date.map(String::from).unwrap_or_else(Self::today);
        let seconds = seconds.max(0);
        let characters = characters.max(0);
        let mut tx = self.pool.begin().await?;
        sqlx::query(
            "INSERT INTO reading_stats (user_ns, date, seconds, characters)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(user_ns, date) DO UPDATE SET
               seconds = seconds + excluded.seconds,
               characters = characters + excluded.characters",
        )
        .bind(user_ns)
        .bind(&date)
        .bind(seconds)
        .bind(characters)
        .execute(&mut *tx)
        .await?;

        if let Some(book_url) = book_url.map(str::trim).filter(|value| !value.is_empty()) {
            let book_name = book_name.map(str::trim).unwrap_or_default();
            sqlx::query(
                "INSERT INTO reading_book_stats
                   (user_ns, date, book_url, book_name, seconds, characters)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                 ON CONFLICT(user_ns, date, book_url) DO UPDATE SET
                   book_name = CASE
                     WHEN excluded.book_name <> '' THEN excluded.book_name
                     ELSE reading_book_stats.book_name
                   END,
                   seconds = seconds + excluded.seconds,
                   characters = characters + excluded.characters",
            )
            .bind(user_ns)
            .bind(&date)
            .bind(book_url)
            .bind(book_name)
            .bind(seconds)
            .bind(characters)
            .execute(&mut *tx)
            .await?;
        }

        tx.commit().await?;
        Ok(())
    }

    /// Per-day stats between `start` and `end` (inclusive, YYYY-MM-DD), oldest first.
    pub async fn get_daily(
        &self,
        user_ns: &str,
        start: &str,
        end: &str,
    ) -> Result<Vec<DailyReadingStats>, AppError> {
        let rows = sqlx::query(
            "SELECT date, seconds, characters FROM reading_stats
             WHERE user_ns = ?1 AND date >= ?2 AND date <= ?3
             ORDER BY date ASC",
        )
        .bind(user_ns)
        .bind(start)
        .bind(end)
        .fetch_all(&self.pool)
        .await?;

        Ok(rows
            .iter()
            .map(|row| DailyReadingStats {
                date: row.get("date"),
                seconds: row.get("seconds"),
                characters: row.get("characters"),
            })
            .collect())
    }

    /// Totals across all time.
    pub async fn get_summary(&self, user_ns: &str) -> Result<serde_json::Value, AppError> {
        let row = sqlx::query(
            "SELECT COALESCE(SUM(seconds),0) AS seconds,
                    COALESCE(SUM(characters),0) AS characters,
                    COUNT(*) AS days
             FROM reading_stats WHERE user_ns = ?1",
        )
        .bind(user_ns)
        .fetch_one(&self.pool)
        .await?;
        Ok(serde_json::json!({
            "totalSeconds": row.get::<i64, _>("seconds"),
            "totalCharacters": row.get::<i64, _>("characters"),
            "activeDays": row.get::<i64, _>("days"),
        }))
    }

    pub async fn get_by_book(
        &self,
        user_ns: &str,
        start: &str,
        end: &str,
    ) -> Result<Vec<BookReadingStats>, AppError> {
        let rows = sqlx::query(
            "SELECT book_url,
                    COALESCE(NULLIF(MAX(book_name), ''), book_url) AS book_name,
                    COALESCE(SUM(seconds), 0) AS seconds,
                    COALESCE(SUM(characters), 0) AS characters,
                    MAX(date) AS last_read_date
             FROM reading_book_stats
             WHERE user_ns = ?1 AND date >= ?2 AND date <= ?3
             GROUP BY book_url
             HAVING SUM(seconds) > 0 OR SUM(characters) > 0
             ORDER BY seconds DESC, book_name ASC",
        )
        .bind(user_ns)
        .bind(start)
        .bind(end)
        .fetch_all(&self.pool)
        .await?;

        Ok(rows
            .iter()
            .map(|row| BookReadingStats {
                book_url: row.get("book_url"),
                book_name: row.get("book_name"),
                seconds: row.get("seconds"),
                characters: row.get("characters"),
                last_read_date: row.get("last_read_date"),
            })
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::sqlite::SqlitePoolOptions;

    async fn setup_service() -> ReadingStatsService {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        sqlx::query(
            "CREATE TABLE reading_stats (
                user_ns TEXT NOT NULL,
                date TEXT NOT NULL,
                seconds INTEGER NOT NULL DEFAULT 0,
                characters INTEGER NOT NULL DEFAULT 0,
                PRIMARY KEY (user_ns, date)
            )",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "CREATE TABLE reading_book_stats (
                user_ns TEXT NOT NULL,
                date TEXT NOT NULL,
                book_url TEXT NOT NULL,
                book_name TEXT NOT NULL DEFAULT '',
                seconds INTEGER NOT NULL DEFAULT 0,
                characters INTEGER NOT NULL DEFAULT 0,
                PRIMARY KEY (user_ns, date, book_url)
            )",
        )
        .execute(&pool)
        .await
        .unwrap();
        ReadingStatsService::new(pool)
    }

    #[tokio::test]
    async fn aggregates_reading_time_by_book_and_range() {
        let service = setup_service().await;
        service
            .add_reading(
                "default",
                120,
                300,
                Some("2026-08-08"),
                Some("book-a"),
                Some("第一本书"),
            )
            .await
            .unwrap();
        service
            .add_reading(
                "default",
                180,
                500,
                Some("2026-08-09"),
                Some("book-a"),
                Some("第一本书"),
            )
            .await
            .unwrap();
        service
            .add_reading(
                "default",
                90,
                200,
                Some("2026-08-09"),
                Some("book-b"),
                Some("第二本书"),
            )
            .await
            .unwrap();
        service
            .add_reading("default", 30, 0, Some("2026-08-09"), None, None)
            .await
            .unwrap();

        let books = service
            .get_by_book("default", "2026-08-08", "2026-08-09")
            .await
            .unwrap();
        assert_eq!(books.len(), 2);
        assert_eq!(books[0].book_url, "book-a");
        assert_eq!(books[0].book_name, "第一本书");
        assert_eq!(books[0].seconds, 300);
        assert_eq!(books[0].characters, 800);
        assert_eq!(books[0].last_read_date, "2026-08-09");
        assert_eq!(books[1].book_url, "book-b");
        assert_eq!(books[1].seconds, 90);

        let daily = service
            .get_daily("default", "2026-08-09", "2026-08-09")
            .await
            .unwrap();
        assert_eq!(daily[0].seconds, 300);
    }
}
