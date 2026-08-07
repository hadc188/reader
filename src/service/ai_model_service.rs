use std::path::PathBuf;
use std::sync::Arc;

use tokio::fs;

use crate::error::error::AppError;
use crate::model::ai_model::AiModelConfig;
use crate::service::json_document_service::JsonDocumentService;

const APP_NAMESPACE: &str = "__app__";
const AI_MODEL_CONFIG_NAME: &str = "ai-model-config.json";

#[derive(Clone)]
pub struct AiModelService {
    docs: Arc<JsonDocumentService>,
    legacy_config_path: PathBuf,
}

impl AiModelService {
    pub fn new(docs: Arc<JsonDocumentService>, storage_dir: &str) -> Self {
        Self {
            docs,
            legacy_config_path: PathBuf::from(storage_dir)
                .join("data")
                .join(AI_MODEL_CONFIG_NAME),
        }
    }

    pub async fn get(&self) -> Result<AiModelConfig, AppError> {
        if let Some(value) = self
            .docs
            .get_value(APP_NAMESPACE, AI_MODEL_CONFIG_NAME)
            .await?
        {
            let config = serde_json::from_value::<AiModelConfig>(value)
                .map_err(|e| AppError::BadRequest(e.to_string()))?
                .sanitized();
            return Ok(config);
        }

        if !self.legacy_config_path.exists() {
            return Ok(AiModelConfig::default());
        }
        let data = fs::read_to_string(&self.legacy_config_path)
            .await
            .map_err(|e| AppError::Internal(e.into()))?;
        let config = serde_json::from_str::<AiModelConfig>(&data)
            .map_err(|e| AppError::BadRequest(e.to_string()))?
            .sanitized();
        self.docs
            .set_value(APP_NAMESPACE, AI_MODEL_CONFIG_NAME, &config)
            .await?;
        Ok(config)
    }

    pub async fn save(&self, config: AiModelConfig) -> Result<AiModelConfig, AppError> {
        let config = config.sanitized();
        self.docs
            .set_value(APP_NAMESPACE, AI_MODEL_CONFIG_NAME, &config)
            .await?;
        Ok(config)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::db;
    use crate::util::time::now_ts;

    async fn create_service() -> (AiModelService, PathBuf) {
        let dir = std::env::temp_dir().join(format!("reader-ai-model-test-{}", now_ts()));
        std::fs::create_dir_all(&dir).unwrap();
        let database_url = format!("sqlite:{}?mode=rwc", dir.join("reader.db").display());
        let pool = db::init_pool(&database_url).await.unwrap();
        let docs = Arc::new(JsonDocumentService::new(pool, dir.to_str().unwrap()));
        (AiModelService::new(docs, dir.to_str().unwrap()), dir)
    }

    #[tokio::test]
    async fn ai_model_config_round_trips_and_sanitizes() {
        let (service, dir) = create_service().await;
        let mut config = AiModelConfig::default();
        config.text.enabled = true;
        config.text.base_url = "https://example.test/v1/".to_string();
        config.text.api_key = " text-key ".to_string();
        config.text.model = " gpt-4o-mini ".to_string();

        let saved = service.save(config).await.unwrap();
        let loaded = service.get().await.unwrap();

        assert_eq!(saved.text.base_url, "https://example.test/v1");
        assert_eq!(saved.text.api_key, "text-key");
        assert_eq!(loaded.text.model, "gpt-4o-mini");
        let _ = fs::remove_dir_all(dir).await;
    }
}
