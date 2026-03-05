use std::path::Path;

use crate::config::WebConfig;
use crate::db;
use crate::db::GhostDb;

use super::topic::ensure_topic_hierarchy;
use super::types::{ImportConfig, ImportError, ImportResult, ImportSource};

#[tracing::instrument(name = "import file", skip_all, fields(topic = %config.topic))]
pub async fn import_file(
    db: &GhostDb,
    workspace: &Path,
    web_config: &WebConfig,
    config: &ImportConfig,
) -> Result<ImportResult, ImportError> {
    let ImportSource::File { path: file_path } = &config.source else {
        return Err(ImportError::Fetch("expected file source".into()));
    };

    let docling_url = web_config
        .docling_url
        .as_deref()
        .ok_or_else(|| ImportError::Fetch("docling_url not configured".into()))?;

    let source_path = if Path::new(file_path).is_absolute() {
        std::path::PathBuf::from(file_path)
    } else {
        workspace.join(file_path)
    };

    if !source_path.exists() {
        return Err(ImportError::Io(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!("file not found: {}", source_path.display()),
        )));
    }

    let original_filename = source_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("file")
        .to_string();
    let stem = source_path
        .file_stem()
        .and_then(|n| n.to_str())
        .unwrap_or("file")
        .to_string();

    let topic_id = ensure_topic_hierarchy(db, &config.topic).await?;

    let filename = format!("{stem}.md");
    let ref_path = format!("{}/{filename}", config.topic);

    // Idempotency
    if db::knowledge::find_reference_by_path(db, &ref_path)
        .await?
        .is_some()
    {
        let batch_id =
            db::knowledge::upsert_import_batch(db, &topic_id, "file", file_path, None, 1).await?;
        return Ok(ImportResult {
            topic_id,
            batch_id,
            references_created: 0,
            references_skipped: 1,
        });
    }

    let batch_id =
        db::knowledge::upsert_import_batch(db, &topic_id, "file", file_path, None, 0).await?;

    // Convert via docling
    let markdown = crate::web::docling::convert_file(docling_url, &source_path)
        .await
        .map_err(|e| ImportError::Fetch(e.to_string()))?;

    // Preserve original
    let originals_dir = workspace
        .join("references")
        .join(&config.topic)
        .join("_originals");
    std::fs::create_dir_all(&originals_dir)?;
    std::fs::copy(&source_path, originals_dir.join(&original_filename))?;

    // Write markdown
    let disk_path = workspace
        .join("references")
        .join(&config.topic)
        .join(&filename);
    if let Some(parent) = disk_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&disk_path, &markdown)?;

    // DB record
    db::knowledge::create_reference(db, &topic_id, &ref_path, &markdown, None, Some(&batch_id))
        .await?;

    // Update batch
    let total_refs = db::knowledge::count_references_by_topic(db, &topic_id).await? as usize;
    let batch_id = db::knowledge::upsert_import_batch(
        db,
        &topic_id,
        "file",
        file_path,
        None,
        total_refs as i64,
    )
    .await?;

    super::topic::write_import_toml(
        workspace,
        &config.topic,
        "file",
        file_path,
        None,
        total_refs,
    )?;

    Ok(ImportResult {
        topic_id,
        batch_id,
        references_created: 1,
        references_skipped: 0,
    })
}
