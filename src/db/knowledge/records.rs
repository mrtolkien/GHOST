use serde::{Deserialize, Serialize};
use surrealdb::types::{Datetime, RecordId, SurrealValue};

#[derive(Debug, Clone, Deserialize, Serialize, SurrealValue)]
#[surreal(crate = "surrealdb::types")]
pub struct NoteRecord {
    pub id: RecordId,
    pub title: String,
    pub body: String,
    pub archetype: Option<String>,
    pub tags: Vec<String>,
    #[serde(default)]
    pub sources: Vec<String>,
    pub trust: i64,
    pub path: Option<String>,
    pub created_at: Datetime,
    pub updated_at: Datetime,
}

#[derive(Debug, Clone, Deserialize, Serialize, SurrealValue)]
#[surreal(crate = "surrealdb::types")]
pub struct ReferenceRecord {
    pub id: RecordId,
    pub topic: String,
    pub path: String,
    pub content: String,
    pub source_url: Option<String>,
    pub created_at: Datetime,
}

#[derive(Debug, Clone, Deserialize, Serialize, SurrealValue)]
#[surreal(crate = "surrealdb::types")]
pub struct DiaryRecord {
    pub id: RecordId,
    pub date: String,
    pub body: String,
    pub updated_at: Datetime,
}

#[derive(Debug, Clone, Serialize, SurrealValue)]
#[surreal(crate = "surrealdb::types")]
pub struct SearchHit {
    pub id: RecordId,
    pub title: String,
    pub snippet: String,
    pub score: f64,
    pub kind: String,
}

#[derive(Debug, Clone, Deserialize, Serialize, SurrealValue)]
#[surreal(crate = "surrealdb::types")]
pub struct RecentItem {
    pub id: RecordId,
    pub title: String,
    pub kind: String,
    pub updated_at: Datetime,
}

#[derive(Debug, Clone, Deserialize, Serialize, SurrealValue)]
#[surreal(crate = "surrealdb::types")]
pub struct EdgeRecord {
    pub id: RecordId,
    #[serde(rename = "in")]
    #[surreal(rename = "in")]
    pub in_node: RecordId,
    pub out: RecordId,
    pub label: String,
    pub created_at: Datetime,
}

pub(super) fn truncate_snippet(text: &str, max_len: usize) -> String {
    let first_line = text.lines().next().unwrap_or("");
    if first_line.len() <= max_len {
        first_line.to_string()
    } else {
        // Find the nearest char boundary at or before max_len
        let end = first_line.floor_char_boundary(max_len);
        format!("{}...", &first_line[..end])
    }
}
