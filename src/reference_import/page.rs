use std::path::Path;

use crate::config::DoclingConfig;
use crate::db;
use crate::db::GhostDb;
use crate::web;

use super::topic::ensure_topic_hierarchy;
use super::types::{ImportConfig, ImportConfigJson, ImportError, ImportResult, ImportSource};

/// Import a single web page or document URL as a reference under a topic.
/// HTML pages are fetched and converted to markdown directly. Non-text URLs
/// (PDF, DOCX, etc.) are routed through docling-serve for conversion.
///
/// Embeddings are handled by the file watcher when it detects the new file
/// under `references/`.
#[tracing::instrument(name = "import page", skip_all, fields(topic = %config.topic))]
pub async fn import_page(
    db: &GhostDb,
    workspace: &Path,
    docling_config: &DoclingConfig,
    config: &ImportConfig,
) -> Result<ImportResult, ImportError> {
    let ImportSource::Page {
        url,
        no_ocr,
        page_range,
    } = &config.source
    else {
        return Err(ImportError::Fetch("expected page source".into()));
    };

    // Ensure topic hierarchy
    let topic_id = ensure_topic_hierarchy(db, &config.topic).await?;

    // Build serializable config snapshot for DB and TOML
    let config_json = ImportConfigJson::from(config);
    let config_json_str = serde_json::to_string(&config_json).ok();

    // Build file-based path: {topic}/{slug}.md
    let slug = crate::web::slug_from_url(url);
    let filename = format!("{slug}.md");
    let ref_path = format!("{}/{filename}", config.topic);

    // Idempotency: skip if already imported
    if db::knowledge::find_reference_by_path(db, &ref_path)
        .await?
        .is_some()
    {
        // Upsert batch even if skipping (topic may not have one yet)
        let batch_id = db::knowledge::upsert_import_batch(
            db,
            &topic_id,
            "page",
            url,
            None,
            1,
            config_json_str.as_deref(),
        )
        .await?;
        return Ok(ImportResult {
            topic_id,
            batch_id,
            references_created: 0,
            references_skipped: 1,
        });
    }

    // Upsert import batch
    let batch_id = db::knowledge::upsert_import_batch(
        db,
        &topic_id,
        "page",
        url,
        None,
        0,
        config_json_str.as_deref(),
    )
    .await?;

    // Fetch page content: try HTML fetch first, fall back to docling for non-text
    let text = match web::fetch(url, &web::FetchOptions::default(), None, None).await {
        Ok(extracted) => extracted.text,
        Err(web::WebError::UnsupportedContentType { .. }) => {
            let convert_opts = web::docling::ConvertOptions {
                ocr: !no_ocr,
                page_range: *page_range,
            };
            web::docling::convert(
                docling_config,
                web::docling::DoclingSource::Url { url },
                &convert_opts,
            )
            .await
            .map_err(|e| ImportError::Fetch(e.to_string()))?
        }
        Err(e) => return Err(ImportError::Fetch(e.to_string())),
    };

    // Write to disk: references/{topic}/{slug}.md
    let disk_path = workspace
        .join("references")
        .join(&config.topic)
        .join(&filename);
    if let Some(parent) = disk_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&disk_path, &text)?;

    // Store as reference
    let hash = crate::embeddings::pipeline::content_hash(&text);
    db::knowledge::create_reference(
        db,
        &topic_id,
        &ref_path,
        &text,
        Some(url),
        Some(&batch_id),
        Some(&hash),
    )
    .await?;

    // Update import batch with final ref count
    let total_refs = db::knowledge::count_references_by_topic(db, &topic_id).await? as usize;
    let batch_id = db::knowledge::upsert_import_batch(
        db,
        &topic_id,
        "page",
        url,
        None,
        total_refs as i64,
        config_json_str.as_deref(),
    )
    .await?;

    // Write _import.toml and ensure index notes
    super::topic::write_import_toml(workspace, &config.topic, &config_json, None, total_refs)?;

    Ok(ImportResult {
        topic_id,
        batch_id,
        references_created: 1,
        references_skipped: 0,
    })
}
