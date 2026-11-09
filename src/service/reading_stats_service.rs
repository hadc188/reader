use chrono::Local;
use sqlx::{Row, SqlitePool};

use crate::error::error::AppError;

/// 单次写入的上限。阅读时长来自前端会话结算, 正常一次最多几分钟;
/// 时钟跳变、重复 flush 或脏请求不该把统计表推到离谱数值。
const MAX_SECONDS_PER_WRITE: i64 = 24 * 60 * 60;
const MAX_CHARACTERS_PER_WRITE: i64 = 10_000_000;

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
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BookReadingStats {
    pub book_url: String,
    pub book_name: String,
    pub seconds: i64,
    pub last_read_date: String,
}

/// 统计页概览：今日 / 本月 / 累计时长与活跃天数。
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReadingStatsOverview {
    pub today_seconds: i64,
    pub month_seconds: i64,
    pub total_seconds: i64,
    pub active_days: i64,
    pub read_books: i64,
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
        let seconds = seconds.clamp(0, MAX_SECONDS_PER_WRITE);
        let characters = characters.clamp(0, MAX_CHARACTERS_PER_WRITE);
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
            })
            .collect())
    }

    /// Totals across all time.
    pub async fn get_summary(&self, user_ns: &str) -> Result<ReadingStatsOverview, AppError> {
        let row = sqlx::query(
        "SELECT
           COALESCE(SUM(seconds), 0) AS seconds,
           COALESCE(SUM(CASE WHEN date = ?2 THEN seconds ELSE 0 END), 0) AS today_seconds,
           COALESCE(SUM(CASE WHEN date >= ?3 AND date <= ?2 THEN seconds ELSE 0 END), 0) AS month_seconds,
           COUNT(CASE WHEN seconds > 0 THEN 1 END) AS days,
           (SELECT COUNT(DISTINCT COALESCE(NULLIF(TRIM(book_name), ''), book_url))
            FROM reading_book_stats WHERE user_ns = ?1) AS books
         FROM reading_stats WHERE user_ns = ?1",
        )
        .bind(user_ns)
    .bind(Self::today())
    .bind(Local::now().format("%Y-%m-01").to_string())
        .fetch_one(&self.pool)
        .await?;
        Ok(ReadingStatsOverview {
            total_seconds: row.get("seconds"),
            today_seconds: row.get("today_seconds"),
            month_seconds: row.get("month_seconds"),
            active_days: row.get("days"),
            read_books: row.get("books"),
        })
    }

    pub async fn get_by_book(
        &self,
        user_ns: &str,
        start: &str,
        end: &str,
    ) -> Result<Vec<BookReadingStats>, AppError> {
        // 排行按规范化书名分组。桌面排行不展示作者, 若把作者纳入分组键,
        // 历史记录里"有作者/作者为空/作者改名"会产生多个视觉上完全相同的条目。
        // 同一书名换源时也应收敛为一行; 作者仅作为辅助信息取最新非空值。
        let rows = sqlx::query(
            "WITH agg AS (
               SELECT
                 COALESCE(NULLIF(TRIM(book_name), ''), book_url) AS norm_name,
                 COALESCE(SUM(seconds), 0) AS seconds,
                 MAX(date) AS last_read_date
               FROM reading_book_stats
               WHERE user_ns = ?1 AND date >= ?2 AND date <= ?3
               GROUP BY COALESCE(NULLIF(TRIM(book_name), ''), book_url)
               HAVING SUM(seconds) > 0
             )
             SELECT
               agg.norm_name AS book_name,
               COALESCE((
                 SELECT r.book_author FROM reading_book_stats r
                 WHERE r.user_ns = ?1 AND r.date >= ?2 AND r.date <= ?3
                   AND COALESCE(NULLIF(TRIM(r.book_name), ''), r.book_url) = agg.norm_name
                   AND TRIM(r.book_author) <> ''
                 ORDER BY r.date DESC, r.seconds DESC
                 LIMIT 1
               ), '') AS book_author,
               COALESCE((
                 SELECT r.book_url FROM reading_book_stats r
                 WHERE r.user_ns = ?1 AND r.date >= ?2 AND r.date <= ?3
                   AND COALESCE(NULLIF(TRIM(r.book_name), ''), r.book_url) = agg.norm_name
                 ORDER BY r.date DESC, r.seconds DESC
                 LIMIT 1
               ), agg.norm_name) AS book_url,
               agg.seconds,
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

    /// Delete all per-book reading stats rows for a given book.
    ///
    /// `book_url` is the representative URL returned by `get_by_book`. Resolve it
    /// to the normalized book name, then delete every row sharing that name so
    /// historical author changes/source switches cannot leave a duplicate row.
    pub async fn delete_book_stats(
        &self,
        user_ns: &str,
        book_url: &str,
    ) -> Result<u64, AppError> {
        let row: Option<String> = sqlx::query_scalar(
            "SELECT
               COALESCE(NULLIF(TRIM(book_name), ''), book_url) AS name
             FROM reading_book_stats
             WHERE user_ns = ?1 AND book_url = ?2 LIMIT 1",
        )
        .bind(user_ns)
        .bind(book_url)
        .fetch_optional(&self.pool)
        .await?;
        let name = match row {
            Some(name) => name,
            None => return Ok(0),
        };
        let mut tx = self.pool.begin().await?;
        // 先保存要扣减的按日汇总，再删除分书行；反过来会在删除后查不到数据。
        let deductions = sqlx::query(
            "SELECT date, SUM(seconds) AS seconds, SUM(characters) AS characters
             FROM reading_book_stats
             WHERE user_ns = ?1
               AND COALESCE(NULLIF(TRIM(book_name), ''), book_url) = ?2
             GROUP BY date",
        )
        .bind(user_ns)
        .bind(&name)
        .fetch_all(&mut *tx)
        .await?;
        let result = sqlx::query(
            "DELETE FROM reading_book_stats
             WHERE user_ns = ?1
               AND COALESCE(NULLIF(TRIM(book_name), ''), book_url) = ?2",
        )
        .bind(user_ns)
        .bind(&name)
        .execute(&mut *tx)
        .await?;
        // 分书统计删除后, 按日总量必须同步扣减; 否则统计页的累计时长
        // 仍包含这本书, 与“删除该书阅读记录”的语义矛盾。
        for row in deductions {
            let date: String = row.get("date");
            let seconds: i64 = row.get("seconds");
            let characters: i64 = row.get("characters");
            sqlx::query(
                "UPDATE reading_stats
                 SET seconds = MAX(0, seconds - ?3),
                     characters = MAX(0, characters - ?4)
                 WHERE user_ns = ?1 AND date = ?2",
            )
            .bind(user_ns)
            .bind(date)
            .bind(seconds)
            .bind(characters)
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await?;
        Ok(result.rows_affected())
    }

    /// 书架详情页编辑书名/作者后迁移按书统计。
    ///
    /// 本地书的 `book_url` 是 `md5("{user_ns}:{文件名}:{内容md5}")`(TXT) / 
    /// `md5_bytes("{user_ns}:{文件名}:", 内容)`(EPUB)。**文件名参与了哈希**，所以
    /// 在书架里改名不会改变 URL —— 改的只是展示名。这正是本方法存在的理由:
    /// 统计列表按 `(book_name, book_author)` 分组，不改历史行的名字，改名后的
    /// 时长就会被算到另一个(旧名)条目下，看起来像"丢了几十小时"。
    ///
    /// 也正因如此，删掉本地文件后改文件名重新导入会生成新的 `book_url`，
    /// 那条路径不在本方法的覆盖范围内。
    ///
    /// 改名后若与书架里另一本同名同作者的书撞车，统计展示会把两者并成一条 ——
    /// 这是 `get_by_book` 既有的分组口径，此处不另行处理。
    pub async fn rename_book_stats(
        &self,
        user_ns: &str,
        book_url: &str,
        book_name: &str,
        book_author: &str,
    ) -> Result<u64, AppError> {
        let result = sqlx::query(
            "UPDATE reading_book_stats
             SET book_name = ?3, book_author = ?4
             WHERE user_ns = ?1 AND book_url = ?2",
        )
        .bind(user_ns)
        .bind(book_url)
        .bind(book_name.trim())
        .bind(book_author.trim())
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
    async fn overview_reports_today_month_total_and_active_days() {
        let service = setup_service().await;
        let today = ReadingStatsService::today();
        let month_start = format!("{}-01", &today[..7]);
        service
            .add_reading("default", 120, 0, Some(&today), Some("book-a"), Some("A书"), Some("甲"))
            .await
            .unwrap();
        service
            .add_reading("default", 60, 0, Some(&month_start), Some("book-b"), Some("B书"), Some("乙"))
            .await
            .unwrap();

        let overview = service.get_summary("default").await.unwrap();
        assert!(overview.today_seconds >= 120);
        assert!(overview.month_seconds >= 180);
        assert!(overview.total_seconds >= 180);
        assert!(overview.active_days >= 1);
        assert_eq!(overview.read_books, 2);
    }

    #[tokio::test]
    async fn same_book_different_source_urls_merges_into_one_row() {
        // 同一本小说换源后 book_url 不同, 但 book_name 相同。
        // 按书名分组后应合并成一条, 时长累加。
        let service = setup_service().await;
        // 源 A 的 URL
        service
            .add_reading("default", 120, 300, Some("2026-08-10"), Some("url-a"), Some("测试书籍"), Some("测试作者"))
            .await
            .unwrap();
        // 源 B 的 URL (换源后)
        service
            .add_reading("default", 180, 500, Some("2026-08-11"), Some("url-b"), Some("测试书籍"), Some("测试作者"))
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
        let doupo = books.iter().find(|b| b.book_name == "测试书籍").unwrap();
        assert_eq!(doupo.seconds, 300);
        assert_eq!(doupo.last_read_date, "2026-08-11");
        // book_url 取最近阅读日期对应的 (2026-08-11 → url-b)
        assert_eq!(doupo.book_url, "url-b");
    }

    #[tokio::test]
    async fn same_book_name_with_legacy_blank_author_merges_into_one_row() {
        let service = setup_service().await;
        service
            .add_reading("default", 120, 0, Some("2026-08-10"), Some("url-old"), Some("同一本书"), Some(""))
            .await
            .unwrap();
        service
            .add_reading("default", 180, 0, Some("2026-08-11"), Some("url-new"), Some("同一本书"), Some("作者甲"))
            .await
            .unwrap();

        let books = service.get_by_book("default", "2026-08-10", "2026-08-11").await.unwrap();
        assert_eq!(books.len(), 1, "作者缺失的旧记录应和新记录合并, 实际: {:?}", books);
        assert_eq!(books[0].seconds, 300);
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

        let daily = service.get_daily("default", "2026-08-08", "2026-08-08").await.unwrap();
        assert_eq!(daily[0].seconds, 90);
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
    async fn rename_book_stats_updates_all_rows_for_local_book_url() {
        let service = setup_service().await;
        service
            .add_reading(
                "default",
                120,
                300,
                Some("2026-08-08"),
                Some("local-txt:abc"),
                Some("旧书名"),
                Some("旧作者"),
            )
            .await
            .unwrap();
        service
            .add_reading(
                "default",
                180,
                500,
                Some("2026-08-09"),
                Some("local-txt:abc"),
                Some("旧书名"),
                Some("旧作者"),
            )
            .await
            .unwrap();
        service
            .add_reading(
                "default",
                60,
                100,
                Some("2026-08-08"),
                Some("local-txt:other"),
                Some("另一本"),
                Some("其他作者"),
            )
            .await
            .unwrap();

        let updated = service
            .rename_book_stats(
                "default",
                "local-txt:abc",
                "新书名",
                "新作者",
            )
            .await
            .unwrap();
        assert_eq!(updated, 2);

        let books = service
            .get_by_book("default", "2026-08-08", "2026-08-09")
            .await
            .unwrap();
        let renamed = books.iter().find(|book| book.book_url == "local-txt:abc").unwrap();
        assert_eq!(renamed.book_name, "新书名");
        assert_eq!(renamed.seconds, 300);
        assert!(books.iter().any(|book| book.book_name == "另一本"));
    }

    #[tokio::test]
    async fn same_name_is_merged_even_when_author_differs() {
        // 排行榜只显示书名; 同名记录若按作者拆开会直接出现视觉重复。
        // 这里确保同名一律收敛为一条, 删除时也一次清理全部来源。
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
        assert_eq!(books.len(), 1, "同名记录应合并, 实际: {:?}", books);
        assert_eq!(books[0].seconds, 300);

        let deleted = service.delete_book_stats("default", "url-甲").await.unwrap();
        assert_eq!(deleted, 2);
        let books = service.get_by_book("default", "2026-08-08", "2026-08-08").await.unwrap();
        assert!(books.is_empty());
    }
}
