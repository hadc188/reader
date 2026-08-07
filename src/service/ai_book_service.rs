use std::path::PathBuf;

use sqlx::{Row, SqlitePool};
use tokio::fs;

use crate::error::error::AppError;
use crate::model::ai_book::AiBookMemory;
use crate::util::hash::md5_hex;
use crate::util::time::now_ts;

#[derive(Clone)]
pub struct AiBookService {
    pool: SqlitePool,
    storage_dir: PathBuf,
}

impl AiBookService {
    pub fn new(pool: SqlitePool, storage_dir: &str) -> Self {
        Self {
            pool,
            storage_dir: PathBuf::from(storage_dir),
        }
    }

    pub async fn get(
        &self,
        user_ns: &str,
        book_url: &str,
    ) -> Result<Option<AiBookMemory>, AppError> {
        let key = md5_hex(book_url);
        if let Some(row) =
            sqlx::query("SELECT json FROM ai_book_memories WHERE user_ns=?1 AND book_key=?2")
                .bind(user_ns)
                .bind(&key)
                .fetch_optional(&self.pool)
                .await?
        {
            let json: String = row.get("json");
            let memory = serde_json::from_str::<AiBookMemory>(&json)
                .map_err(|e| AppError::BadRequest(e.to_string()))?;
            return Ok(Some(memory));
        }

        let path = self.memory_path(user_ns, book_url);
        if !path.exists() {
            return Ok(None);
        }
        let data = fs::read_to_string(&path)
            .await
            .map_err(|e| AppError::Internal(e.into()))?;
        let memory = serde_json::from_str::<AiBookMemory>(&data)
            .map_err(|e| AppError::BadRequest(e.to_string()))?;
        self.save_memory_row(user_ns, book_url, &memory).await?;
        let _ = fs::remove_file(path).await;
        Ok(Some(memory))
    }

    pub async fn save_for_book(
        &self,
        user_ns: &str,
        book_url: &str,
        mut memory: AiBookMemory,
    ) -> Result<AiBookMemory, AppError> {
        if book_url.trim().is_empty() {
            return Err(AppError::BadRequest("bookUrl required".to_string()));
        }
        if memory.book_url.trim().is_empty() {
            memory.book_url = book_url.to_string();
        }
        if memory.book_url != book_url {
            return Err(AppError::BadRequest("bookUrl mismatch".to_string()));
        }
        if memory.updated_at <= 0 {
            memory.updated_at = now_ts() * 1000;
        }

        self.save_memory_row(user_ns, book_url, &memory).await?;
        Ok(memory)
    }

    pub async fn delete(&self, user_ns: &str, book_url: &str) -> Result<bool, AppError> {
        let key = md5_hex(book_url);
        let result = sqlx::query("DELETE FROM ai_book_memories WHERE user_ns=?1 AND book_key=?2")
            .bind(user_ns)
            .bind(&key)
            .execute(&self.pool)
            .await?;

        let path = self.memory_path(user_ns, book_url);
        let removed_file = if path.exists() {
            fs::remove_file(path)
                .await
                .map_err(|e| AppError::Internal(e.into()))?;
            true
        } else {
            false
        };

        Ok(result.rows_affected() > 0 || removed_file)
    }

    async fn save_memory_row(
        &self,
        user_ns: &str,
        book_url: &str,
        memory: &AiBookMemory,
    ) -> Result<(), AppError> {
        let key = md5_hex(book_url);
        let data =
            serde_json::to_string(memory).map_err(|e| AppError::BadRequest(e.to_string()))?;
        sqlx::query(
            "INSERT INTO ai_book_memories (user_ns, book_key, book_url, json, updated_at) VALUES (?1, ?2, ?3, ?4, ?5) \
             ON CONFLICT(user_ns, book_key) DO UPDATE SET book_url=excluded.book_url, json=excluded.json, updated_at=excluded.updated_at",
        )
        .bind(user_ns)
        .bind(&key)
        .bind(book_url)
        .bind(data)
        .bind(memory.updated_at)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    fn memory_path(&self, user_ns: &str, book_url: &str) -> PathBuf {
        self.storage_dir
            .join("data")
            .join(user_ns)
            .join("ai-books")
            .join(format!("{}.json", md5_hex(book_url)))
    }
}
