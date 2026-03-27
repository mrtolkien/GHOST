use std::collections::HashMap;
use std::path::Path;

use crate::db;
use crate::db::GhostDb;

use super::crawl::fetch_crawl_manifest;
use super::git::fetch_git_manifest;
use super::topic::{load_import_config_from_db, read_import_toml, write_import_toml};
use super::types::{ImportError, ImportSource, UpdateResult};

/// Re-fetch references from their original source and apply changes.
///
/// Compares new content against existing references by file hash, creating,
/// updating, or deleting as needed. References cited by notes are moved to
/// `_orphaned/` instead of deleted.
#[tracing::instrument(
    name = "update references",
    skip_all,
    fields(topic = %topic_name)
)]
pub async fn update_references(
    db: &GhostDb,
    workspace: &Path,
    topic_name: &str,
    ref_override: Option<&str>,
) -> Result<UpdateResult, ImportError> {
    // 1. Look up topic
    let topic = db::knowledge::find_topic_by_name(db, topic_name)
        .await?
        .ok_or_else(|| ImportError::Config(format!("topic '{topic_name}' not found")))?;
    let topic_id = &topic.id;

    // 2. Read import config (TOML on disk first, DB fallback)
    let mut config_json = match read_import_toml(workspace, topic_name) {
        Ok(cfg) => cfg,
        Err(_) => load_import_config_from_db(db, topic_id)
            .await?
            .ok_or_else(|| {
                ImportError::Config(format!("no import config found for topic '{topic_name}'"))
            })?,
    };

    // 3. Apply --ref override if provided
    if let Some(r) = ref_override {
        config_json.git_ref = Some(r.to_string());
    }

    // 4. Build ImportConfig
    let import_config = config_json.to_import_config(topic_name)?;

    // 5. Get old version_ref from import batch
    let old_batch = db::knowledge::get_import_batch_by_topic(db, topic_id).await?;
    let old_version_ref = old_batch.as_ref().and_then(|b| b.version_ref.clone());

    // 6. Re-fetch from source
    let (new_version_ref, manifest) = fetch_manifest(&import_config).await?;

    // 7. Git short-circuit: same commit hash and no --ref override
    if let (Some(old_ref), Some(new_ref)) = (&old_version_ref, &new_version_ref)
        && old_ref == new_ref
        && ref_override.is_none()
    {
        println!("Already up to date at {new_ref}");
        return Ok(UpdateResult {
            created: 0,
            updated: 0,
            deleted: 0,
            orphaned: 0,
            unchanged: manifest.len(),
            old_version_ref: old_version_ref.clone(),
            new_version_ref: new_version_ref.clone(),
        });
    }

    // 8. Load existing references -> HashMap<path, (ref_id, file_hash)>
    let existing_refs = db::knowledge::list_references_by_topic(db, Some(topic_id), 10_000).await?;
    let mut existing_map: HashMap<String, (String, Option<String>)> = existing_refs
        .into_iter()
        .map(|r| (r.path, (r.id, r.file_hash)))
        .collect();

    // 9. Diff and apply
    let mut created = 0usize;
    let mut updated = 0usize;
    let mut unchanged = 0usize;

    let batch_id_opt = old_batch.as_ref().map(|b| b.id.clone());

    for (ref_path, content, source_url) in &manifest {
        let new_hash = crate::embeddings::pipeline::content_hash(content);

        if let Some((ref_id, old_hash)) = existing_map.remove(ref_path.as_str()) {
            // Exists -- check if changed
            if old_hash.as_deref() == Some(new_hash.as_str()) {
                unchanged += 1;
            } else {
                // Update: overwrite disk + DB
                let disk_path = workspace.join("references").join(ref_path);
                if let Some(parent) = disk_path.parent() {
                    std::fs::create_dir_all(parent)?;
                }
                std::fs::write(&disk_path, content)?;

                db::knowledge::update_reference(db, &ref_id, content, Some(&new_hash)).await?;
                updated += 1;
            }
        } else {
            // New file -- create on disk + DB
            let disk_path = workspace.join("references").join(ref_path);
            if let Some(parent) = disk_path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(&disk_path, content)?;

            db::knowledge::create_reference(
                db,
                topic_id,
                ref_path,
                content,
                source_url.as_deref(),
                batch_id_opt.as_deref(),
                Some(&new_hash),
            )
            .await?;
            created += 1;
        }
    }

    // 10. Handle deletions (remaining in existing_map = deleted upstream)
    let (deleted, orphaned) =
        handle_deletions(db, workspace, topic_name, topic_id, &existing_map).await?;

    // 11. Update batch metadata
    let total_refs = usize::try_from(
        db::knowledge::count_references_by_topic(db, topic_id)
            .await?
            .max(0),
    )
    .unwrap_or(0);
    let config_json_str = serde_json::to_string(&config_json).ok();
    db::knowledge::upsert_import_batch(
        db,
        topic_id,
        &config_json.source_type,
        &config_json.source_url,
        new_version_ref.as_deref(),
        total_refs as i64,
        config_json_str.as_deref(),
    )
    .await?;

    // 12. Rewrite _import.toml
    write_import_toml(
        workspace,
        topic_name,
        &config_json,
        new_version_ref.as_deref(),
        total_refs,
    )?;

    Ok(UpdateResult {
        created,
        updated,
        deleted,
        orphaned,
        unchanged,
        old_version_ref,
        new_version_ref,
    })
}

/// Fetch a manifest from the appropriate source.
/// Returns (optional version_ref, vec of (path, content, optional source_url)).
async fn fetch_manifest(
    config: &super::types::ImportConfig,
) -> Result<(Option<String>, Vec<(String, String, Option<String>)>), ImportError> {
    match &config.source {
        ImportSource::Git { .. } => {
            let (hash, files) = fetch_git_manifest(config).await?;
            let manifest = files
                .into_iter()
                .map(|(path, content)| (path, content, None))
                .collect();
            Ok((Some(hash), manifest))
        }
        ImportSource::Crawl { .. } => {
            let pages = fetch_crawl_manifest(config).await?;
            let manifest = pages
                .into_iter()
                .map(|(path, content, src)| (path, content, Some(src)))
                .collect();
            Ok((None, manifest))
        }
        ImportSource::File { .. } => Err(ImportError::Config(
            "only git and crawl sources support update".into(),
        )),
    }
}

/// Process references that were deleted upstream. Cited references are moved
/// to `_orphaned/` instead of being deleted.
async fn handle_deletions(
    db: &GhostDb,
    workspace: &Path,
    topic_name: &str,
    topic_id: &str,
    existing_map: &HashMap<String, (String, Option<String>)>,
) -> Result<(usize, usize), ImportError> {
    if existing_map.is_empty() {
        return Ok((0, 0));
    }

    let deleted_ids: Vec<String> = existing_map.values().map(|(id, _)| id.clone()).collect();
    let cited = db::knowledge::cited_reference_ids(db, &deleted_ids).await?;

    let mut deleted = 0usize;
    let mut orphaned = 0usize;

    for (ref_path, (ref_id, _)) in existing_map {
        let disk_path = workspace.join("references").join(ref_path);

        if cited.contains(ref_id) {
            // Orphan: move to _orphaned/
            let filename = Path::new(ref_path)
                .file_name()
                .unwrap_or_default()
                .to_string_lossy();
            let orphan_dir = workspace
                .join("references")
                .join(topic_name)
                .join("_orphaned");
            std::fs::create_dir_all(&orphan_dir)?;

            let orphan_disk = orphan_dir.join(&*filename);
            if disk_path.exists() {
                std::fs::rename(&disk_path, &orphan_disk)?;
            }

            let orphan_path = format!("{topic_name}/_orphaned/{filename}");
            db::knowledge::update_reference_path(db, ref_id, &orphan_path, topic_id).await?;

            println!(
                "Warning: {ref_path} deleted upstream but cited by notes. \
                 Moved to _orphaned/"
            );
            orphaned += 1;
        } else {
            // Safe to delete
            if disk_path.exists() {
                std::fs::remove_file(&disk_path)?;
            }
            db::knowledge::delete_reference(db, ref_id).await?;
            deleted += 1;
        }
    }

    Ok((deleted, orphaned))
}
