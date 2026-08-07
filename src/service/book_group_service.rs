use std::sync::Arc;

use crate::error::error::AppError;
use crate::model::book_group::BookGroup;
use crate::service::json_document_service::JsonDocumentService;

pub struct BookGroupService {
    docs: Arc<JsonDocumentService>,
}

impl BookGroupService {
    pub fn new(docs: Arc<JsonDocumentService>) -> Self {
        Self { docs }
    }

    pub async fn get_groups(&self, user_ns: &str) -> Result<Vec<BookGroup>, AppError> {
        self.docs.read_list(user_ns, "book_groups.json").await
    }

    pub async fn save_groups(
        &self,
        user_ns: &str,
        groups: &Vec<BookGroup>,
    ) -> Result<(), AppError> {
        self.docs
            .write_list(user_ns, "book_groups.json", groups)
            .await
    }

    pub async fn save_group(&self, user_ns: &str, mut group: BookGroup) -> Result<(), AppError> {
        let mut groups = self.get_groups(user_ns).await?;
        if group.group_id == 0 {
            let max_id = groups.iter().map(|g| g.group_id).max().unwrap_or(0);
            group.group_id = max_id + 1;
        }
        let mut found = false;
        for g in &mut groups {
            if g.group_id == group.group_id {
                g.group_name = group.group_name.clone();
                g.order_no = group.order_no;
                found = true;
                break;
            }
        }
        if !found {
            groups.push(group);
        }
        self.save_groups(user_ns, &groups).await?;
        Ok(())
    }

    pub async fn delete_group(&self, user_ns: &str, group_id: i64) -> Result<(), AppError> {
        let mut groups = self.get_groups(user_ns).await?;
        groups.retain(|g| g.group_id != group_id);
        self.save_groups(user_ns, &groups).await?;
        Ok(())
    }
}
