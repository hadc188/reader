use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
pub struct User {
    pub username: String,
    pub password: String,
    pub salt: String,
    pub token: String,
    #[serde(alias = "last_login_at")]
    pub last_login_at: i64,
    #[serde(alias = "created_at")]
    pub created_at: i64,
    #[serde(alias = "enable_webdav")]
    pub enable_webdav: bool,
    #[serde(alias = "token_map")]
    pub token_map: Option<HashMap<String, i64>>,
    #[serde(alias = "enable_local_store")]
    pub enable_local_store: bool,
    #[serde(alias = "enable_ai_model")]
    pub enable_ai_model: bool,
    #[serde(alias = "is_admin")]
    pub is_admin: bool,
}
