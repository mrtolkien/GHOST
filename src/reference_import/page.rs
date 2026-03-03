use std::path::Path;

use crate::config::EmbeddingsConfig;
use crate::db;
use crate::db::GhostDb;
use crate::embeddings::EmbeddingClient;
use crate::embeddings::pipeline::{EmbedRequest, embed_sources};
use crate::web;

use super::topic::ensure_topic_hierarchy;
use super::types::{ImportConfig, ImportError, ImportResult, ImportSource};

/// Import a single web page as a reference under a topic.
pub async fn import_page(
    db: &GhostDb,
    workspace: &Path,
    embeddings_config: &EmbeddingsConfig,
    config: &ImportConfig,
) -> Result<ImportResult, ImportError> {
    let ImportSource::Page { url } = &config.source else {
        return Err(ImportError::Fetch("expected page source".into()));
    };

    // Ensure topic hierarchy
    let topic_id = ensure_topic_hierarchy(db, &config.topic).await?;

    // Use url as the reference path
    let ref_path = url.clone();

    // Idempotency: skip if already imported
    if db::knowledge::find_reference_by_path(db, &ref_path)
        .await?
        .is_some()
    {
        // Upsert batch even if skipping (topic may not have one yet)
        let batch_id =
            db::knowledge::upsert_import_batch(db, &topic_id, "page", url, None, 1).await?;
        return Ok(ImportResult {
            topic_id,
            batch_id,
            references_created: 0,
            references_skipped: 1,
            embeddings_generated: 0,
        });
    }

    // Upsert import batch
    let batch_id = db::knowledge::upsert_import_batch(db, &topic_id, "page", url, None, 0).await?;

    // Fetch page content
    let extracted = web::fetch(url, &web::FetchOptions::default(), None)
        .await
        .map_err(|e| ImportError::Fetch(e.to_string()))?;

    // Store as reference
    let ref_id = db::knowledge::create_reference(
        db,
        &topic_id,
        &ref_path,
        &extracted.text,
        Some(url),
        Some(&batch_id),
    )
    .await?;

    // Embed
    let client = EmbeddingClient::new(embeddings_config);
    let embed_requests = vec![EmbedRequest {
        source_table: "reference".into(),
        source_id: ref_id,
        content: extracted.text,
        tags: vec![config.topic.clone()],
        topic_id: Some(topic_id.clone()),
    }];
    let embeddings_generated = embed_sources(&client, db, embed_requests).await?;

    // Update import batch with final ref count
    let total_refs = db::knowledge::count_references_by_topic(db, &topic_id).await? as usize;
    let batch_id =
        db::knowledge::upsert_import_batch(db, &topic_id, "page", url, None, total_refs as i64)
            .await?;

    // Write _import.toml and ensure index notes
    super::topic::write_import_toml(workspace, &config.topic, "page", url, None, total_refs)?;

    Ok(ImportResult {
        topic_id,
        batch_id,
        references_created: 1,
        references_skipped: 0,
        embeddings_generated,
    })
}
