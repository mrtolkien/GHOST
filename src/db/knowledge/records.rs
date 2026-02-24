use serde::{Deserialize, Serialize};
use surrealdb::sql::{Datetime, Thing};

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct NoteRecord {
    pub id: Thing,
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

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ReferenceRecord {
    pub id: Thing,
    pub topic: String,
    pub path: String,
    pub content: String,
    pub source_url: Option<String>,
    pub created_at: Datetime,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct DiaryRecord {
    pub id: Thing,
    pub date: String,
    pub body: String,
    pub updated_at: Datetime,
}

#[derive(Debug, Clone, Serialize)]
pub struct SearchHit {
    pub id: Thing,
    pub title: String,
    pub snippet: String,
    pub score: f64,
    pub kind: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct RecentItem {
    pub id: Thing,
    pub title: String,
    pub kind: String,
    pub updated_at: Datetime,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct EdgeRecord {
    pub id: Thing,
    #[serde(rename = "in")]
    pub in_node: Thing,
    pub out: Thing,
    pub label: String,
    pub created_at: Datetime,
}

pub(super) fn truncate_snippet(text: &str, max_len: usize) -> String {
    let first_line = text.lines().next().unwrap_or("");
    if first_line.len() <= max_len {
        first_line.to_string()
    } else {
        format!("{}...", &first_line[..max_len])
    }
}
