use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BookGroup {
    #[serde(default)]
    pub group_id: i64,
    pub group_name: String,
    #[serde(default)]
    pub order_no: i32,
}
