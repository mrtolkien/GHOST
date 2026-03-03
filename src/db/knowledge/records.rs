use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Serialize, sqlx::FromRow)]
pub struct NoteRecord {
    pub id: String,
    pub title: String,
    pub body: String,
    pub archetype: Option<String>,
    pub tags: String,    // JSON array of strings
    pub sources: String, // JSON array of strings
    pub trust: i64,
    pub path: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

impl NoteRecord {
    pub fn tags_parsed(&self) -> Vec<String> {
        serde_json::from_str(&self.tags).unwrap_or_default()
    }

    pub fn sources_parsed(&self) -> Vec<String> {
        serde_json::from_str(&self.sources).unwrap_or_default()
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, sqlx::FromRow)]
pub struct ReferenceRecord {
    pub id: String,
    pub topic_id: String,
    pub path: String,
    pub content: String,
    pub source_url: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, Deserialize, Serialize, sqlx::FromRow)]
pub struct TopicRecord {
    pub id: String,
    pub name: String,
    pub note_id: Option<String>,
    pub source_url: Option<String>,
    pub version_ref: Option<String>,
    pub fetched_at: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Deserialize, Serialize, sqlx::FromRow)]
pub struct DiaryRecord {
    pub id: String,
    pub date: String,
    pub body: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct SearchHit {
    pub id: String,
    pub title: String,
    pub snippet: String,
    pub score: f64,
    pub kind: String,
}

#[derive(Debug, Clone, Deserialize, Serialize, sqlx::FromRow)]
pub struct RecentItem {
    pub id: String,
    pub title: String,
    pub kind: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Deserialize, Serialize, sqlx::FromRow)]
pub struct EdgeRecord {
    pub id: String,
    pub from_id: String,
    pub to_id: String,
    pub label: String,
    pub created_at: String,
}

pub(super) fn truncate_snippet(text: &str, max_len: usize) -> String {
    let first_line = text.lines().next().unwrap_or("");
    if first_line.len() <= max_len {
        first_line.to_string()
    } else {
        let end = first_line.floor_char_boundary(max_len);
        format!("{}...", &first_line[..end])
    }
}
