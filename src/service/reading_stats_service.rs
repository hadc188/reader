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

/// 备份导出用: reading_stats 全量按日行。
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DailyStatsRow {
    pub date: String,
    pub seconds: i64,
    pub characters: i64,
}

/// 备份导出用: reading_book_stats 全量原始行(按日按书)。
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BookStatsRow {
    pub date: String,
    pub book_url: String,
    pub book_name: String,
    pub book_author: String,
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
        book_url: Option<&str>,
        book_name: Option<&str>,
        book_author: Option<&str>,
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
            let book_author = book_author.map(str::trim).unwrap_or_default();
            sqlx::query(
                "INSERT INTO reading_book_stats
                   (user_ns, date, book_url, book_name, book_author, seconds, characters)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
                 ON CONFLICT(user_ns, date, book_url) DO UPDATE SET
                   book_name = CASE
                     WHEN excluded.book_name <> '' THEN excluded.book_name
                     ELSE reading_book_stats.book_name
                   END,
                   book_author = CASE
                     WHEN excluded.book_author <> '' THEN excluded.book_author
                     ELSE reading_book_stats.book_author
                   END,
                   seconds = seconds + excluded.seconds,
                   characters = characters + excluded.characters",
            )
            .bind(user_ns)
            .bind(&date)
            .bind(book_url)
            .bind(book_name)
            .bind(book_author)
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
        // 按 (书名, 作者) 联合分组, 而非仅按 book_url 或仅按 book_name。
        // 原因 1: 同一本小说换源后 book_url 会变成新源的 URL, 按 book_url 分组
        //         会让同一本书出现多条记录(每个源一条)。
        // 原因 2: 仅按 book_name 分组会把不同作者的同名书合并成一条,
        //         删除其中一本会误删另一本。加入作者区分可避免此问题。
        // 作者缺失时回退为空串参与分组(同名无作者的书仍会合并, 这是可接受的退化)。
        let rows = sqlx::query(
            "WITH agg AS (
               SELECT
                 COALESCE(NULLIF(TRIM(book_name), ''), book_url) AS norm_name,
                 COALESCE(NULLIF(TRIM(book_author), ''), '') AS norm_author,
                 COALESCE(SUM(seconds), 0) AS seconds,
                 COALESCE(SUM(characters), 0) AS characters,
                 MAX(date) AS last_read_date
               FROM reading_book_stats
               WHERE user_ns = ?1 AND date >= ?2 AND date <= ?3
               GROUP BY COALESCE(NULLIF(TRIM(book_name), ''), book_url),
                        COALESCE(NULLIF(TRIM(book_author), ''), '')
               HAVING SUM(seconds) > 0 OR SUM(characters) > 0
             )
             SELECT
               agg.norm_name AS book_name,
               agg.norm_author AS book_author,
               COALESCE((
                 SELECT r.book_url FROM reading_book_stats r
                 WHERE r.user_ns = ?1 AND r.date >= ?2 AND r.date <= ?3
                   AND COALESCE(NULLIF(TRIM(r.book_name), ''), r.book_url) = agg.norm_name
                   AND COALESCE(NULLIF(TRIM(r.book_author), ''), '') = agg.norm_author
                 ORDER BY r.date DESC, r.seconds DESC
                 LIMIT 1
               ), agg.norm_name) AS book_url,
               agg.seconds,
               agg.characters,
               agg.last_read_date
             FROM agg
             ORDER BY agg.seconds DESC, agg.norm_name ASC",
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

    /// 导出全量按日统计(无日期范围限制), 用于备份。
    pub async fn get_all_daily(&self, user_ns: &str) -> Result<Vec<DailyStatsRow>, AppError> {
        let rows = sqlx::query(
            "SELECT date, seconds, characters FROM reading_stats
             WHERE user_ns = ?1 ORDER BY date ASC",
        )
        .bind(user_ns)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .iter()
            .map(|row| DailyStatsRow {
                date: row.get("date"),
                seconds: row.get("seconds"),
                characters: row.get("characters"),
            })
            .collect())
    }

    /// 导出全量按书统计原始行(按日按书), 用于备份。
    pub async fn get_all_book_rows(&self, user_ns: &str) -> Result<Vec<BookStatsRow>, AppError> {
        let rows = sqlx::query(
            "SELECT date, book_url, book_name, book_author, seconds, characters
             FROM reading_book_stats WHERE user_ns = ?1 ORDER BY date ASC",
        )
        .bind(user_ns)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .iter()
            .map(|row| BookStatsRow {
                date: row.get("date"),
                book_url: row.get("book_url"),
                book_name: row.get("book_name"),
                book_author: row.get("book_author"),
                seconds: row.get("seconds"),
                characters: row.get("characters"),
            })
            .collect())
    }

    /// 用备份数据整体替换该用户的统计(覆盖式恢复)。
    pub async fn replace_all(
        &self,
        user_ns: &str,
        daily: &[DailyStatsRow],
        book_rows: &[BookStatsRow],
    ) -> Result<(), AppError> {
        let mut tx = self.pool.begin().await?;
        sqlx::query("DELETE FROM reading_stats WHERE user_ns = ?1")
            .bind(user_ns)
            .execute(&mut *tx)
            .await?;
        sqlx::query("DELETE FROM reading_book_stats WHERE user_ns = ?1")
            .bind(user_ns)
            .execute(&mut *tx)
            .await?;

        for row in daily {
            sqlx::query(
                "INSERT INTO reading_stats (user_ns, date, seconds, characters)
                 VALUES (?1, ?2, ?3, ?4)",
            )
            .bind(user_ns)
            .bind(row.date.trim())
            .bind(row.seconds.max(0))
            .bind(row.characters.max(0))
            .execute(&mut *tx)
            .await?;
        }
        for row in book_rows {
            sqlx::query(
                "INSERT INTO reading_book_stats
                   (user_ns, date, book_url, book_name, book_author, seconds, characters)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            )
            .bind(user_ns)
            .bind(row.date.trim())
            .bind(row.book_url.trim())
            .bind(row.book_name.trim())
            .bind(row.book_author.trim())
            .bind(row.seconds.max(0))
            .bind(row.characters.max(0))
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await?;
        Ok(())
    }

    /// Delete all per-book reading stats rows for a given book. Since get_by_book
    /// now groups by book_name (not book_url), deletion must also match by book_name
    /// to remove all rows for that book across different source URLs. The book_url
    /// passed in is the "latest" URL from the stats display; we resolve it to the
    /// (book_name, book_author) pair first, then delete all rows matching that pair
    /// to remove all rows for that book across different source URLs without
    /// accidentally deleting a different book with the same name.
    pub async fn delete_book_stats(
        &self,
        user_ns: &str,
        book_url: &str,
    ) -> Result<u64, AppError> {
        // 先查出该 book_url 对应的 (书名, 作者), 再按此二元组删除所有记录。
        // 仅按书名删除会误伤同名不同作者的书; 加入作者可精确区分。
        let row: Option<(String, String)> = sqlx::query_as(
            "SELECT
               COALESCE(NULLIF(TRIM(book_name), ''), book_url) AS name,
               COALESCE(NULLIF(TRIM(book_author), ''), '') AS author
             FROM reading_book_stats
             WHERE user_ns = ?1 AND book_url = ?2 LIMIT 1",
        )
        .bind(user_ns)
        .bind(book_url)
        .fetch_optional(&self.pool)
        .await?;
        let (name, author) = match row {
            Some(pair) => pair,
            None => return Ok(0),
        };
        let result = sqlx::query(
            "DELETE FROM reading_book_stats
             WHERE user_ns = ?1
               AND COALESCE(NULLIF(TRIM(book_name), ''), book_url) = ?2
               AND COALESCE(NULLIF(TRIM(book_author), ''), '') = ?3",
        )
        .bind(user_ns)
        .bind(&name)
        .bind(&author)
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected())
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
                book_author TEXT NOT NULL DEFAULT '',
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
    async fn replace_all_round_trips_full_history() {
        let service = setup_service().await;
        service
            .add_reading(
                "default",
                100,
                10,
                Some("2026-08-01"),
                Some("old-book"),
                Some("旧数据"),
                Some(""),
            )
            .await
            .unwrap();

        let daily = vec![DailyStatsRow {
            date: "2026-08-11".to_string(),
            seconds: 90,
            characters: 12,
        }];
        let book_rows = vec![BookStatsRow {
            date: "2026-08-11".to_string(),
            book_url: "book-a".to_string(),
            book_name: "第一本书".to_string(),
            book_author: "作者甲".to_string(),
            seconds: 90,
            characters: 12,
        }];
        service.replace_all("default", &daily, &book_rows).await.unwrap();

        let exported_daily = service.get_all_daily("default").await.unwrap();
        let exported_books = service.get_all_book_rows("default").await.unwrap();
        assert_eq!(exported_daily, daily);
        assert_eq!(exported_books, book_rows);

        // 其他用户命名空间不受影响(单用户应用, 仅验证隔离性)。
        let other = service.get_all_daily("other").await.unwrap();
        assert!(other.is_empty());
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
                Some("作者甲"),
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
                Some("作者甲"),
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
                Some("作者乙"),
            )
            .await
            .unwrap();
        service
            .add_reading("default", 30, 0, Some("2026-08-09"), None, None, None)
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

    #[tokio::test]
    async fn same_book_different_source_urls_merges_into_one_row() {
        // 同一本小说换源后 book_url 不同, 但 book_name 相同。
        // 按书名分组后应合并成一条, 时长累加。
        let service = setup_service().await;
        // 源 A 的 URL
        service
            .add_reading("default", 120, 300, Some("2026-08-10"), Some("url-a"), Some("斗破苍穹"), Some("天蚕土豆"))
            .await
            .unwrap();
        // 源 B 的 URL (换源后)
        service
            .add_reading("default", 180, 500, Some("2026-08-11"), Some("url-b"), Some("斗破苍穹"), Some("天蚕土豆"))
            .await
            .unwrap();
        // 另一本书
        service
            .add_reading("default", 60, 100, Some("2026-08-10"), Some("url-c"), Some("凡人修仙"), Some("忘语"))
            .await
            .unwrap();

        let books = service.get_by_book("default", "2026-08-10", "2026-08-11").await.unwrap();
        assert_eq!(books.len(), 2, "同一本书换源后应合并成一条, 实际: {:?}", books);
        // 时长累加: 120 + 180 = 300
        let doupo = books.iter().find(|b| b.book_name == "斗破苍穹").unwrap();
        assert_eq!(doupo.seconds, 300);
        assert_eq!(doupo.characters, 800);
        assert_eq!(doupo.last_read_date, "2026-08-11");
        // book_url 取最近阅读日期对应的 (2026-08-11 → url-b)
        assert_eq!(doupo.book_url, "url-b");
    }

    #[tokio::test]
    async fn delete_book_stats_removes_only_target_book() {
        let service = setup_service().await;
        service
            .add_reading("default", 120, 300, Some("2026-08-08"), Some("book-a"), Some("A书"), Some("甲"))
            .await
            .unwrap();
        service
            .add_reading("default", 90, 200, Some("2026-08-08"), Some("book-b"), Some("B书"), Some("乙"))
            .await
            .unwrap();

        let deleted = service.delete_book_stats("default", "book-a").await.unwrap();
        assert_eq!(deleted, 1);

        let books = service.get_by_book("default", "2026-08-08", "2026-08-08").await.unwrap();
        assert_eq!(books.len(), 1);
        assert_eq!(books[0].book_url, "book-b");
    }

    #[tokio::test]
    async fn delete_book_stats_removes_all_rows_across_source_urls() {
        // 换源后同一本书有多个 book_url, 删除时应全部删除
        let service = setup_service().await;
        service
            .add_reading("default", 120, 0, Some("2026-08-08"), Some("url-a"), Some("同一本"), Some("同作者"))
            .await
            .unwrap();
        service
            .add_reading("default", 180, 0, Some("2026-08-09"), Some("url-b"), Some("同一本"), Some("同作者"))
            .await
            .unwrap();
        service
            .add_reading("default", 60, 0, Some("2026-08-08"), Some("url-c"), Some("另一本"), Some("其他作者"))
            .await
            .unwrap();

        // 用 url-b 删除应删掉 url-a 和 url-b 两条
        let deleted = service.delete_book_stats("default", "url-b").await.unwrap();
        assert_eq!(deleted, 2);

        let books = service.get_by_book("default", "2026-08-08", "2026-08-09").await.unwrap();
        assert_eq!(books.len(), 1);
        assert_eq!(books[0].book_name, "另一本");
    }

    #[tokio::test]
    async fn same_name_different_authors_not_merged() {
        // 不同作者的同名书不应合并, 删除其中一本不影响另一本
        let service = setup_service().await;
        service
            .add_reading("default", 100, 0, Some("2026-08-08"), Some("url-甲"), Some("同名书"), Some("作者甲"))
            .await
            .unwrap();
        service
            .add_reading("default", 200, 0, Some("2026-08-08"), Some("url-乙"), Some("同名书"), Some("作者乙"))
            .await
            .unwrap();

        let books = service.get_by_book("default", "2026-08-08", "2026-08-08").await.unwrap();
        assert_eq!(books.len(), 2, "同名不同作者的书应分开, 实际: {:?}", books);

        // 删除作者甲的书, 作者乙的书不受影响
        let deleted = service.delete_book_stats("default", "url-甲").await.unwrap();
        assert_eq!(deleted, 1);
        let books = service.get_by_book("default", "2026-08-08", "2026-08-08").await.unwrap();
        assert_eq!(books.len(), 1);
        assert_eq!(books[0].book_url, "url-乙");
    }
}
