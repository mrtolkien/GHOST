use crate::db;
use crate::db::GhostDb;
use crate::knowledge;

use super::types::ImportError;

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

/// Create or update the note linked to a topic. Uses the existing
/// `ensure_index_notes` pattern — archetype=Topic notes.
pub async fn ensure_topic_note(
    db: &GhostDb,
    workspace: &std::path::Path,
    topic_id: &str,
    topic_name: &str,
    source_url: Option<&str>,
    version_ref: Option<&str>,
    ref_count: usize,
) -> Result<(), ImportError> {
    let title = format!("Topic: {topic_name}");
    let mut body = format!("# {topic_name}\n\n");

    if let Some(url) = source_url {
        body.push_str(&format!("Source: {url}\n"));
    }
    if let Some(ver) = version_ref {
        body.push_str(&format!("Version: {ver}\n"));
    }
    body.push_str(&format!("\nReferences: {ref_count}\n"));

    let note_id = match db::knowledge::find_note_by_title(db, &title).await? {
        Some(existing) => {
            db::knowledge::update_note(
                db,
                &existing.id,
                &body,
                Some("Topic"),
                &[topic_name.to_string()],
                &[],
                5,
                None,
            )
            .await?;
            existing.id
        }
        None => {
            db::knowledge::create_note_full(
                db,
                &title,
                &body,
                Some("Topic"),
                &[topic_name.to_string()],
                &[],
                5,
                None,
            )
            .await?
        }
    };

    // Write the topic note as index.md in the topic's directory
    let front = knowledge::NoteFrontMatter {
        title: title.clone(),
        archetype: Some(knowledge::Archetype::Topic),
        tags: vec![topic_name.to_string()],
        sources: vec![],
        trust: 5,
    };
    let content = knowledge::serialize_note(&front, &body)
        .map_err(|e| ImportError::Io(std::io::Error::other(e.to_string())))?;
    let note_dir = workspace.join("notes").join(topic_name);
    std::fs::create_dir_all(&note_dir)?;
    let note_path = note_dir.join("index.md");
    std::fs::write(&note_path, content)?;

    // Link topic to its note (preserve existing source_url/version_ref)
    db::knowledge::update_topic(db, topic_id, Some(&note_id), source_url, version_ref, None)
        .await?;

    Ok(())
}
