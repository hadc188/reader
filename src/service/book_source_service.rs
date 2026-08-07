use crate::error::error::AppError;
use crate::model::book_source::{book_source_from_value, BookSource};
use crate::storage::db::repo::BookSourceRepo;
use std::path::PathBuf;
use tokio::fs;

pub const INVALID_BOOK_SOURCE_GROUP: &str = "失效";

#[derive(Clone)]
pub struct BookSourceService {
    repo: BookSourceRepo,
    default_owner_path: PathBuf,
}

impl BookSourceService {
    pub fn new(repo: BookSourceRepo, storage_dir: &str) -> Self {
        let default_owner_path = PathBuf::from(storage_dir)
            .join("data")
            .join("__default__")
            .join("defaultBookSourceOwner.txt");
        Self {
            repo,
            default_owner_path,
        }
    }

    pub async fn save(&self, user_ns: &str, source: BookSource) -> Result<(), AppError> {
        let json =
            serde_json::to_string(&source).map_err(|e| AppError::BadRequest(e.to_string()))?;
        self.repo.upsert(user_ns, &source, &json).await
    }

    pub async fn save_many(&self, user_ns: &str, sources: Vec<BookSource>) -> Result<(), AppError> {
        for s in sources {
            self.save(user_ns, s).await?;
        }
        Ok(())
    }

    /// Pin a source to the top of the search/换源 order by giving it the
    /// smallest `custom_order` (Legado `topSource` semantics).
    pub async fn pin(&self, user_ns: &str, book_source_url: &str) -> Result<(), AppError> {
        let mut source = self
            .get(user_ns, book_source_url)
            .await?
            .ok_or_else(|| AppError::NotFound("bookSource not found".to_string()))?;
        let min_order = self
            .list(user_ns)
            .await?
            .into_iter()
            .filter_map(|s| s.custom_order)
            .min()
            .unwrap_or(0);
        source.custom_order = Some(min_order - 1);
        self.save(user_ns, source).await
    }

    /// Unpin a source by resetting its `custom_order` to 0.
    pub async fn unpin(&self, user_ns: &str, book_source_url: &str) -> Result<(), AppError> {
        let mut source = self
            .get(user_ns, book_source_url)
            .await?
            .ok_or_else(|| AppError::NotFound("bookSource not found".to_string()))?;
        source.custom_order = Some(0);
        self.save(user_ns, source).await
    }

    pub async fn get(
        &self,
        user_ns: &str,
        book_source_url: &str,
    ) -> Result<Option<BookSource>, AppError> {
        let json = self.repo.get(user_ns, book_source_url).await?;
        if let Some(j) = json {
            let value: serde_json::Value =
                serde_json::from_str(&j).map_err(|e| AppError::BadRequest(e.to_string()))?;
            let source =
                book_source_from_value(value).map_err(|e| AppError::BadRequest(e.to_string()))?;
            Ok(Some(source))
        } else {
            Ok(None)
        }
    }

    pub async fn list(&self, user_ns: &str) -> Result<Vec<BookSource>, AppError> {
        let rows = self.repo.list(user_ns).await?;
        let mut out = Vec::with_capacity(rows.len());
        for j in rows {
            if let Ok(value) = serde_json::from_str::<serde_json::Value>(&j) {
                if let Ok(s) = book_source_from_value(value) {
                    out.push(s);
                }
            } else if let Ok(s) = serde_json::from_str::<BookSource>(&j) {
                out.push(s);
            }
        }
        // Pinned sources (lower custom_order) come first; unpinned (None) sort
        // last, then by name. This ordering drives multi-source search, 换源,
        // and the source manager list.
        out.sort_by(|a, b| {
            let oa = a.custom_order.unwrap_or(i32::MAX);
            let ob = b.custom_order.unwrap_or(i32::MAX);
            oa.cmp(&ob).then_with(|| a.book_source_name.cmp(&b.book_source_name))
        });
        Ok(out)
    }

    pub async fn delete(&self, user_ns: &str, book_source_url: &str) -> Result<(), AppError> {
        self.repo.delete(user_ns, book_source_url).await
    }

    pub async fn delete_all(&self, user_ns: &str) -> Result<(), AppError> {
        self.repo.delete_all(user_ns).await
    }

    /// Copy sources from one user to another (used for setting default sources)
    pub async fn copy_to(&self, from_ns: &str, to_ns: &str) -> Result<i64, AppError> {
        self.repo.copy_to(from_ns, to_ns).await
    }

    /// Set a user's sources as the default sources (for new users)
    pub async fn set_as_default(&self, from_ns: &str) -> Result<i64, AppError> {
        let count = self.copy_to(from_ns, "__default__").await?;
        if let Some(dir) = self.default_owner_path.parent() {
            fs::create_dir_all(dir)
                .await
                .map_err(|e| AppError::Internal(e.into()))?;
        }
        fs::write(&self.default_owner_path, from_ns)
            .await
            .map_err(|e| AppError::Internal(e.into()))?;
        Ok(count)
    }

    /// Copy default sources to a new user
    pub async fn copy_default_to_user(&self, to_ns: &str) -> Result<i64, AppError> {
        let defaults = self.list("__default__").await?;
        if defaults.is_empty() {
            return Ok(0);
        }
        let count = defaults.len() as i64;
        self.save_many(to_ns, defaults).await?;
        Ok(count)
    }

    pub async fn get_default_owner(&self) -> Result<Option<String>, AppError> {
        match fs::read_to_string(&self.default_owner_path).await {
            Ok(value) => {
                let trimmed = value.trim();
                if trimmed.is_empty() {
                    Ok(None)
                } else {
                    Ok(Some(trimmed.to_string()))
                }
            }
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(err) => Err(AppError::Internal(err.into())),
        }
    }
}

pub fn book_source_has_group(source: &BookSource, target: &str) -> bool {
    source
        .book_source_group
        .as_deref()
        .map(split_source_groups)
        .unwrap_or_default()
        .into_iter()
        .any(|group| group == target)
}

pub fn set_invalid_book_source_group(source: &mut BookSource, invalid: bool) -> bool {
    let mut groups = source
        .book_source_group
        .as_deref()
        .map(split_source_groups)
        .unwrap_or_default();
    let had_invalid = groups
        .iter()
        .any(|group| group == INVALID_BOOK_SOURCE_GROUP);

    if invalid {
        if had_invalid {
            return false;
        }
        groups.push(INVALID_BOOK_SOURCE_GROUP.to_string());
    } else {
        if !had_invalid {
            return false;
        }
        groups.retain(|group| group != INVALID_BOOK_SOURCE_GROUP);
    }

    source.book_source_group = if groups.is_empty() {
        None
    } else {
        Some(groups.join(","))
    };
    true
}

fn split_source_groups(raw: &str) -> Vec<String> {
    raw.split(|ch| matches!(ch, ',' | ';' | '；' | '、'))
        .map(str::trim)
        .filter(|group| !group.is_empty())
        .map(str::to_string)
        .fold(Vec::new(), |mut groups, group| {
            if !groups.contains(&group) {
                groups.push(group);
            }
            groups
        })
}
