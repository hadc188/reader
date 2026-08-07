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
    ) -> Result<(), AppError> {
        let date = date.map(String::from).unwrap_or_else(Self::today);
        sqlx::query(
            "INSERT INTO reading_stats (user_ns, date, seconds, characters)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(user_ns, date) DO UPDATE SET
               seconds = seconds + excluded.seconds,
               characters = characters + excluded.characters",
        )
        .bind(user_ns)
        .bind(&date)
        .bind(seconds.max(0))
        .bind(characters.max(0))
        .execute(&self.pool)
        .await?;
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
}
