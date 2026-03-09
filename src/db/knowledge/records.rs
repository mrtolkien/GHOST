use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Serialize, sqlx::FromRow)]
pub struct NoteRecord {
    pub id: String,
    pub title: String,
    pub body: String,
    pub tags: String,    // JSON array of strings
    pub sources: String, // JSON array of strings
    pub trust: i64,
    pub topic_id: Option<String>,
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
    pub import_batch_id: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, Deserialize, Serialize, sqlx::FromRow)]
pub struct TopicRecord {
    pub id: String,
    pub name: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Deserialize, Serialize, sqlx::FromRow)]
pub struct ImportBatchRecord {
    pub id: String,
    pub topic_id: String,
    pub source_type: String,
    pub source_url: String,
    pub version_ref: Option<String>,
    pub ref_count: i64,
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
    /// Workspace-relative file path (e.g. `references/dioxus/docs/hooks.md`).
    pub path: Option<String>,
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
    let trimmed = text.trim();
    if trimmed.len() <= max_len {
        return trimmed.to_string();
    }
    let end = trimmed.floor_char_boundary(max_len);
    format!("{}...", &trimmed[..end])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncate_snippet_takes_chars_across_lines() {
        let text = "first line\nsecond line\nthird line";
        let result = truncate_snippet(text, 25);
        assert!(result.len() <= 28, "should respect max_len (plus ...)");
        assert!(
            result.contains("second"),
            "should include second line content, got: {result}"
        );
    }

    #[test]
    fn truncate_snippet_short_text_unchanged() {
        let text = "short";
        assert_eq!(truncate_snippet(text, 150), "short");
    }

    #[test]
    fn truncate_snippet_single_long_line_truncates() {
        let text = "a".repeat(200);
        let result = truncate_snippet(&text, 50);
        assert!(result.ends_with("..."));
        assert!(result.len() <= 53 + 3);
    }

    #[test]
    fn truncate_snippet_strips_section_prefix() {
        let text = "[section: Break Rules]\n## Break Rules\n\nDuring a break, players discard.";
        let result = truncate_snippet(text, 150);
        assert!(
            result.contains("break") || result.contains("Break"),
            "should contain actual content, got: {result}"
        );
    }
}
