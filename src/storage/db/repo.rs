use crate::error::error::AppError;
use crate::model::book_source::BookSource;
use crate::util::time::now_ts;
use sqlx::{Row, SqlitePool};

#[derive(Clone)]
pub struct BookSourceRepo {
    pool: SqlitePool,
}

impl BookSourceRepo {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    pub async fn upsert(
        &self,
        user_ns: &str,
        source: &BookSource,
        json: &str,
    ) -> Result<(), AppError> {
        sqlx::query(
            "INSERT INTO book_sources (user_ns, book_source_url, book_source_name, json, updated_at) VALUES (?1, ?2, ?3, ?4, ?5) \
             ON CONFLICT(user_ns, book_source_url) DO UPDATE SET book_source_name=excluded.book_source_name, json=excluded.json, updated_at=excluded.updated_at"
        )
        .bind(user_ns)
        .bind(&source.book_source_url)
        .bind(&source.book_source_name)
        .bind(json)
        .bind(now_ts())
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn delete(&self, user_ns: &str, book_source_url: &str) -> Result<(), AppError> {
        sqlx::query("DELETE FROM book_sources WHERE user_ns=?1 AND book_source_url=?2")
            .bind(user_ns)
            .bind(book_source_url)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// 批量删除, 一条 SQL 取代 N 次串行 `delete`。
    ///
    /// "删除失效书源"可能一次删几百条: 逐条 await 是 N 次网络往返 + N 次语句
    /// 编译, 用户要等很久。这里用一条 `IN (...)` 完成, 返回实际删除行数。
    ///
    /// 占位符按需拼接(`?2, ?3, ...`), 值仍走 bind —— 不拼字符串, 无注入风险。
    /// SQLite 的变量上限(旧版 999, 新版 32766)对书源规模而言有余量; 仍做分批
    /// 以免极端情况下撞上限。
    ///
    /// **分批必须在同一事务内**: 否则中途某批失败时前面的批次已提交, 而调用方
    /// 拿到的却是 `Err` —— 它会 `?` 提前返回, 于是 `remove_source_candidates` /
    /// `clear_source_cookies` 全都不执行, 留下"DB 已删、书架候选与 Cookie 残留"
    /// 的不一致状态, 且用户以为删除失败了。事务保证全有或全无。
    pub async fn delete_many(
        &self,
        user_ns: &str,
        book_source_urls: &[String],
    ) -> Result<u64, AppError> {
        if book_source_urls.is_empty() {
            return Ok(0);
        }
        // 每批最多 400 条, 加上 user_ns 也远低于 SQLite 的变量上限。
        const BATCH: usize = 400;
        let mut tx = self.pool.begin().await?;
        let mut deleted = 0u64;
        for chunk in book_source_urls.chunks(BATCH) {
            let placeholders = (0..chunk.len())
                .map(|index| format!("?{}", index + 2))
                .collect::<Vec<_>>()
                .join(", ");
            let sql = format!(
                "DELETE FROM book_sources WHERE user_ns=?1 AND book_source_url IN ({placeholders})"
            );
            let mut query = sqlx::query(&sql).bind(user_ns);
            for url in chunk {
                query = query.bind(url);
            }
            let result = query.execute(&mut *tx).await?;
            deleted += result.rows_affected();
        }
        tx.commit().await?;
        Ok(deleted)
    }

    pub async fn delete_all(&self, user_ns: &str) -> Result<(), AppError> {
        sqlx::query("DELETE FROM book_sources WHERE user_ns=?1")
            .bind(user_ns)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn get(
        &self,
        user_ns: &str,
        book_source_url: &str,
    ) -> Result<Option<String>, AppError> {
        let row =
            sqlx::query("SELECT json FROM book_sources WHERE user_ns=?1 AND book_source_url=?2")
                .bind(user_ns)
                .bind(book_source_url)
                .fetch_optional(&self.pool)
                .await?;
        Ok(row.map(|r| r.get::<String, _>("json")))
    }

    pub async fn list(&self, user_ns: &str) -> Result<Vec<String>, AppError> {
        let rows =
            sqlx::query("SELECT json FROM book_sources WHERE user_ns=?1 ORDER BY updated_at DESC")
                .bind(user_ns)
                .fetch_all(&self.pool)
                .await?;
        Ok(rows
            .into_iter()
            .map(|r| r.get::<String, _>("json"))
            .collect())
    }

    pub async fn copy_to(&self, from_ns: &str, to_ns: &str) -> Result<i64, AppError> {
        let rows = sqlx::query("SELECT book_source_url, book_source_name, json, updated_at FROM book_sources WHERE user_ns=?1")
            .bind(from_ns)
            .fetch_all(&self.pool)
            .await?;
        let count = rows.len() as i64;
        for row in rows {
            let url: String = row.get("book_source_url");
            let name: String = row.get("book_source_name");
            let json: String = row.get("json");
            let updated_at: i64 = row.get("updated_at");
            sqlx::query(
                "INSERT INTO book_sources (user_ns, book_source_url, book_source_name, json, updated_at) VALUES (?1, ?2, ?3, ?4, ?5) \
                 ON CONFLICT(user_ns, book_source_url) DO UPDATE SET book_source_name=excluded.book_source_name, json=excluded.json, updated_at=excluded.updated_at"
            )
            .bind(to_ns)
            .bind(&url)
            .bind(&name)
            .bind(&json)
            .bind(updated_at)
            .execute(&self.pool)
            .await?;
        }
        Ok(count)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 内存库(单连接) —— 不需要任何文件清理, 也就不会有临时文件残留。
    async fn test_repo() -> BookSourceRepo {
        BookSourceRepo::new(crate::storage::db::test_pool().await)
    }

    fn source(url: &str) -> BookSource {
        BookSource {
            book_source_name: url.to_string(),
            book_source_url: url.to_string(),
            ..Default::default()
        }
    }

    async fn seed(repo: &BookSourceRepo, urls: &[&str]) {
        for url in urls {
            let source = source(url);
            let json = serde_json::to_string(&source).unwrap();
            repo.upsert("default", &source, &json).await.unwrap();
        }
    }

    async fn seed_owned(repo: &BookSourceRepo, urls: &[String]) {
        for url in urls {
            let source = source(url);
            let json = serde_json::to_string(&source).unwrap();
            repo.upsert("default", &source, &json).await.unwrap();
        }
    }

    async fn remaining(repo: &BookSourceRepo) -> Vec<String> {
        repo.list("default").await.unwrap()
    }

    /// 批量删除要真正删掉指定项、且不误伤其他项 —— 它替换了原先的逐条 delete。
    #[tokio::test]
    async fn delete_many_removes_only_requested_urls() {
        let repo = test_repo().await;
        seed(&repo, &["a", "b", "c", "d"]).await;

        let deleted = repo
            .delete_many("default", &["b".to_string(), "d".to_string()])
            .await
            .unwrap();

        assert_eq!(deleted, 2, "应返回实际删除行数");
        let left = remaining(&repo).await;
        assert_eq!(left.len(), 2);
        assert!(left.iter().any(|json| json.contains("\"a\"")));
        assert!(left.iter().any(|json| json.contains("\"c\"")));
    }

    /// 空输入不能报错(也不该生成非法 SQL, 例如 `IN ()`)。
    #[tokio::test]
    async fn delete_many_with_empty_input_is_noop() {
        let repo = test_repo().await;
        seed(&repo, &["a"]).await;

        let deleted = repo.delete_many("default", &[]).await.unwrap();

        assert_eq!(deleted, 0);
        assert_eq!(remaining(&repo).await.len(), 1);
    }

    /// 超过单批上限(400)时仍要全部删掉 —— 分批逻辑不能漏掉尾部。
    #[tokio::test]
    async fn delete_many_handles_more_than_one_batch() {
        let repo = test_repo().await;
        let urls = (0..950).map(|i| format!("s{i}")).collect::<Vec<_>>();
        seed_owned(&repo, &urls).await;

        let deleted = repo.delete_many("default", &urls).await.unwrap();

        assert_eq!(deleted, 950, "跨批次时应删除全部(950 > 单批 400)");
        assert!(remaining(&repo).await.is_empty());
    }

    /// 删除不存在的 URL 返回 0, 不应报错。
    #[tokio::test]
    async fn delete_many_ignores_unknown_urls() {
        let repo = test_repo().await;
        seed(&repo, &["a"]).await;

        let deleted = repo
            .delete_many("default", &["a".to_string(), "missing".to_string()])
            .await
            .unwrap();

        assert_eq!(deleted, 1);
        assert!(remaining(&repo).await.is_empty());
    }
}
