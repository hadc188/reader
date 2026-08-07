use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
pub struct AiModelEndpointConfig {
    pub enabled: bool,
    pub base_url: String,
    pub api_key: String,
    pub model: String,
    pub use_full_url: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct AiImageModelConfig {
    pub enabled: bool,
    pub base_url: String,
    pub api_key: String,
    pub model: String,
    pub use_full_url: bool,
    pub image_size: String,
}

impl Default for AiImageModelConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            base_url: String::new(),
            api_key: String::new(),
            model: "gpt-image-1".to_string(),
            use_full_url: false,
            image_size: "1024x1024".to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct AiSpeechModelConfig {
    pub enabled: bool,
    pub base_url: String,
    pub api_key: String,
    pub model: String,
    pub use_full_url: bool,
    pub voice: String,
    pub response_format: String,
}

impl Default for AiSpeechModelConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            base_url: String::new(),
            api_key: String::new(),
            model: "gpt-4o-mini-tts".to_string(),
            use_full_url: false,
            voice: "alloy".to_string(),
            response_format: "mp3".to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
pub struct AiModelConfig {
    pub text: AiModelEndpointConfig,
    pub image: AiImageModelConfig,
    pub speech: AiSpeechModelConfig,
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum AiModelKind {
    Text,
    Image,
    Speech,
}

#[derive(Debug, Clone)]
pub struct ResolvedAiModelEndpoint {
    pub enabled: bool,
    pub base_url: String,
    pub api_key: String,
    pub model: String,
    pub use_full_url: bool,
    pub image_size: Option<String>,
    pub voice: Option<String>,
    pub response_format: Option<String>,
}

impl AiModelConfig {
    pub fn sanitized(mut self) -> Self {
        self.text.base_url = normalize_url(self.text.base_url);
        self.text.api_key = self.text.api_key.trim().to_string();
        self.text.model = self.text.model.trim().to_string();

        self.image.base_url = normalize_url(self.image.base_url);
        self.image.api_key = self.image.api_key.trim().to_string();
        self.image.model = self.image.model.trim().to_string();
        self.image.image_size = default_if_empty(self.image.image_size, "1024x1024");

        self.speech.base_url = normalize_url(self.speech.base_url);
        self.speech.api_key = self.speech.api_key.trim().to_string();
        self.speech.model = default_if_empty(self.speech.model, "gpt-4o-mini-tts");
        self.speech.voice = default_if_empty(self.speech.voice, "alloy");
        self.speech.response_format = default_if_empty(self.speech.response_format, "mp3");
        self
    }

    pub fn without_secrets(mut self) -> Self {
        self.text.api_key.clear();
        self.image.api_key.clear();
        self.speech.api_key.clear();
        self
    }

    pub fn resolve(&self, kind: AiModelKind) -> ResolvedAiModelEndpoint {
        match kind {
            AiModelKind::Text => ResolvedAiModelEndpoint {
                enabled: self.text.enabled,
                base_url: self.text.base_url.clone(),
                api_key: self.text.api_key.clone(),
                model: self.text.model.clone(),
                use_full_url: self.text.use_full_url,
                image_size: None,
                voice: None,
                response_format: None,
            },
            AiModelKind::Image => ResolvedAiModelEndpoint {
                enabled: self.image.enabled,
                base_url: self.image.base_url.clone(),
                api_key: self.image.api_key.clone(),
                model: self.image.model.clone(),
                use_full_url: self.image.use_full_url,
                image_size: Some(self.image.image_size.clone()),
                voice: None,
                response_format: None,
            },
            AiModelKind::Speech => ResolvedAiModelEndpoint {
                enabled: self.speech.enabled,
                base_url: self.speech.base_url.clone(),
                api_key: self.speech.api_key.clone(),
                model: self.speech.model.clone(),
                use_full_url: self.speech.use_full_url,
                image_size: None,
                voice: Some(self.speech.voice.clone()),
                response_format: Some(self.speech.response_format.clone()),
            },
        }
    }
}

fn normalize_url(value: String) -> String {
    value.trim().trim_end_matches('/').to_string()
}

fn default_if_empty(value: String, default_value: &str) -> String {
    let value = value.trim();
    if value.is_empty() {
        default_value.to_string()
    } else {
        value.to_string()
    }
}
