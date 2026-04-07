use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::db;
use crate::db::GhostDb;
use crate::knowledge;

use super::types::{ImportConfigJson, ImportError};

/// Read an `ImportConfigJson` from the `_import.toml` file on disk for the
/// given topic. Extra fields (version_ref, ref_count) are silently ignored.
pub fn read_import_toml(
    workspace: &Path,
    topic_name: &str,
) -> Result<ImportConfigJson, ImportError> {
    let path = workspace
        .join("references")
        .join(topic_name)
        .join("_import.toml");
    let content = std::fs::read_to_string(&path).map_err(|e| {
        ImportError::Config(format!("no _import.toml for topic '{topic_name}': {e}"))
    })?;
    let import_toml: ImportToml = toml::from_str(&content).map_err(|e| {
        ImportError::Config(format!(
            "invalid _import.toml for topic '{topic_name}': {e}"
        ))
    })?;
    Ok(import_toml.into_config())
}

/// Load an `ImportConfigJson` from the DB's `import_batch.import_config`
/// column. Returns `None` if no batch exists or if `import_config` is null.
pub async fn load_import_config_from_db(
    db: &GhostDb,
    topic_id: &str,
) -> Result<Option<ImportConfigJson>, ImportError> {
    let batch = db::knowledge::get_import_batch_by_topic(db, topic_id).await?;
    match batch.and_then(|b| b.import_config) {
        Some(json) => {
            let config: ImportConfigJson = serde_json::from_str(&json)
                .map_err(|e| ImportError::Config(format!("invalid import_config JSON: {e}")))?;
            Ok(Some(config))
        }
        None => Ok(None),
    }
}

/// Ensure that a topic hierarchy exists in the DB. For "dioxus/docs",
/// ensures both "dioxus" and "dioxus/docs" exist as topic rows.
/// Returns the leaf topic ID.
pub async fn ensure_topic_hierarchy(db: &GhostDb, topic_name: &str) -> Result<String, ImportError> {
    let parts: Vec<&str> = topic_name.split('/').collect();
    let mut last_id = String::new();

    for i in 0..parts.len() {
        let name = parts[..=i].join("/");
        last_id = db::knowledge::find_or_create_topic(db, &name).await?;
    }

    Ok(last_id)
}

/// Flat structure serialized into `_import.toml` — merges `ImportConfigJson`
/// fields with runtime values (version_ref, ref_count).
#[derive(Debug, Serialize, Deserialize)]
struct ImportToml {
    source_type: String,
    source_url: String,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    git_ref: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    paths: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    extensions: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    max_depth: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    max_pages: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    authors: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    language: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    publisher: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    publication_date: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    video_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    channel: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    published_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    duration_seconds: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    transcript_source: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    section_count: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    chapter_count: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    version_ref: Option<String>,
    ref_count: usize,
}

impl ImportToml {
    fn from_config(config: &ImportConfigJson, version_ref: Option<&str>, ref_count: usize) -> Self {
        Self {
            source_type: config.source_type.clone(),
            source_url: config.source_url.clone(),
            git_ref: config.git_ref.clone(),
            paths: config.paths.clone(),
            extensions: config.extensions.clone(),
            max_depth: config.max_depth,
            max_pages: config.max_pages,
            title: config.title.clone(),
            authors: config.authors.clone(),
            language: config.language.clone(),
            publisher: config.publisher.clone(),
            publication_date: config.publication_date.clone(),
            video_id: config.video_id.clone(),
            channel: config.channel.clone(),
            published_at: config.published_at.clone(),
            duration_seconds: config.duration_seconds,
            transcript_source: config.transcript_source.clone(),
            section_count: config.section_count,
            chapter_count: config.chapter_count,
            version_ref: version_ref.map(String::from),
            ref_count,
        }
    }

    fn into_config(self) -> ImportConfigJson {
        ImportConfigJson {
            source_type: self.source_type,
            source_url: self.source_url,
            git_ref: self.git_ref,
            paths: self.paths,
            extensions: self.extensions,
            max_depth: self.max_depth,
            max_pages: self.max_pages,
            title: self.title,
            authors: self.authors,
            language: self.language,
            publisher: self.publisher,
            publication_date: self.publication_date,
            video_id: self.video_id,
            channel: self.channel,
            published_at: self.published_at,
            duration_seconds: self.duration_seconds,
            transcript_source: self.transcript_source,
            section_count: self.section_count,
            chapter_count: self.chapter_count,
        }
    }
}

/// Write `_import.toml` alongside imported references to record import metadata.
pub fn write_import_toml(
    workspace: &Path,
    topic_name: &str,
    config: &ImportConfigJson,
    version_ref: Option<&str>,
    ref_count: usize,
) -> Result<(), ImportError> {
    let ref_dir = workspace.join("references").join(topic_name);
    std::fs::create_dir_all(&ref_dir)?;

    let import_toml = ImportToml::from_config(config, version_ref, ref_count);

    let content = format!(
        "# Auto-generated by ghost reference import\n{}",
        toml::to_string_pretty(&import_toml)
            .map_err(|e| ImportError::Io(std::io::Error::other(e.to_string())))?
    );

    let import_path = ref_dir.join("_import.toml");
    std::fs::write(&import_path, content)?;

    // Also ensure index notes exist for the topic hierarchy
    knowledge::ensure_index_notes(workspace, topic_name)
        .map_err(|e| ImportError::Io(std::io::Error::other(e.to_string())))?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn write_import_toml_includes_youtube_fields() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let config = ImportConfigJson {
            source_type: "youtube".into(),
            source_url: "https://www.youtube.com/watch?v=test123".into(),
            git_ref: None,
            paths: vec![],
            extensions: vec![],
            max_depth: None,
            max_pages: None,
            title: Some("Test Video".into()),
            authors: Some(vec!["Author One".into(), "Author Two".into()]),
            language: Some("en".into()),
            publisher: Some("Test Publisher".into()),
            publication_date: Some("2024-01-02".into()),
            video_id: Some("test123".into()),
            channel: Some("Example Channel".into()),
            published_at: Some("2024-01-02".into()),
            duration_seconds: Some(1_234),
            transcript_source: Some("auto".into()),
            section_count: Some(3),
            chapter_count: Some(1),
        };

        write_import_toml(tmp.path(), "videos/test", &config, Some("version-123"), 3)
            .expect("write import toml");

        let content =
            std::fs::read_to_string(tmp.path().join("references/videos/test/_import.toml"))
                .expect("read toml");
        assert!(content.contains("source_type = \"youtube\""));
        assert!(content.contains("title = \"Test Video\""));
        assert!(content.contains("authors ="));
        assert!(content.contains("Author One"));
        assert!(content.contains("Author Two"));
        assert!(content.contains("publisher = \"Test Publisher\""));
        assert!(content.contains("publication_date = \"2024-01-02\""));
        assert!(content.contains("video_id = \"test123\""));
        assert!(content.contains("transcript_source = \"auto\""));

        let parsed = read_import_toml(tmp.path(), "videos/test").expect("round-trip toml");
        assert_eq!(parsed.source_type, config.source_type);
        assert_eq!(parsed.source_url, config.source_url);
        assert_eq!(parsed.git_ref, config.git_ref);
        assert_eq!(parsed.paths, config.paths);
        assert_eq!(parsed.extensions, config.extensions);
        assert_eq!(parsed.max_depth, config.max_depth);
        assert_eq!(parsed.max_pages, config.max_pages);
        assert_eq!(parsed.title, config.title);
        assert_eq!(parsed.authors, config.authors);
        assert_eq!(parsed.language, config.language);
        assert_eq!(parsed.publisher, config.publisher);
        assert_eq!(parsed.publication_date, config.publication_date);
        assert_eq!(parsed.video_id, config.video_id);
        assert_eq!(parsed.channel, config.channel);
        assert_eq!(parsed.published_at, config.published_at);
        assert_eq!(parsed.duration_seconds, config.duration_seconds);
        assert_eq!(parsed.transcript_source, config.transcript_source);
        assert_eq!(parsed.section_count, config.section_count);
        assert_eq!(parsed.chapter_count, config.chapter_count);
    }
}
