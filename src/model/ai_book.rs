use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct AiBookMemory {
    pub book_url: String,
    pub book_name: Option<String>,
    pub author: Option<String>,
    pub enabled: bool,
    pub processed_chapter_index: Option<i32>,
    pub processed_chapter_title: Option<String>,
    pub updated_at: i64,
    pub summary: String,
    pub worldview: Vec<AiBookNote>,
    pub characters: Vec<AiBookCharacter>,
    pub relationships: Vec<AiBookRelationship>,
    pub locations: Vec<AiBookLocation>,
    pub map: Option<AiBookMap>,
    pub map_dirty: bool,
    pub last_error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct AiBookNote {
    pub title: String,
    pub content: String,
    pub category: Option<String>,
    pub confidence: Option<String>,
    pub importance: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct AiBookCharacter {
    pub name: String,
    pub aliases: Vec<String>,
    pub status: String,
    pub faction: Option<String>,
    pub location: Option<String>,
    pub description: Option<String>,
    pub last_seen_chapter: Option<String>,
    pub importance: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct AiBookRelationship {
    pub source: String,
    pub target: String,
    pub relation: String,
    pub status: Option<String>,
    pub description: Option<String>,
    pub importance: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct AiBookLocation {
    pub name: String,
    pub kind: Option<String>,
    pub parent_name: Option<String>,
    pub description: String,
    pub status: Option<String>,
    pub related_characters: Vec<String>,
    pub first_seen_chapter: Option<String>,
    pub importance: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct AiBookMap {
    pub image_url: Option<String>,
    pub prompt: Option<String>,
    pub updated_at: Option<i64>,
    pub source_chapter_index: Option<i32>,
    pub fallback: Option<String>,
    pub fallback_reason: Option<String>,
}
