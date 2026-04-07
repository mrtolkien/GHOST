use std::collections::HashMap;
use std::path::Path;

use crate::convert::crawl::convert_crawl;
use crate::convert::git::convert_git;
use crate::db;
use crate::db::GhostDb;

use super::topic::{ensure_update_metadata, write_import_toml};
use super::types::{ImportConfig, ImportError, ImportSource, UpdateResult};

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

    // 2. Read repair-critical import config from disk, backfilling older topics
    // from DB import metadata when possible.
    let metadata = ensure_update_metadata(db, workspace, topic_id, topic_name).await?;
    let disk_version_ref = metadata.version_ref;
    let mut config_json = metadata.config;

    // 3. Apply --ref override if provided
    if let Some(r) = ref_override {
        config_json.git_ref = Some(r.to_string());
    }

    // 4. Build ImportConfig
    let import_config = config_json.to_import_config(topic_name)?;

    // 5. Load the existing import batch for batch-id reuse on created refs.
    let old_batch = db::knowledge::get_import_batch_by_topic(db, topic_id).await?;
    let old_version_ref = disk_version_ref;

    // 6. Convert source to staging directory, then read as manifest.
    //    The staging tempdir is auto-cleaned when fetch_to_staging returns
    //    (all file contents are read into memory before that).
    let (new_version_ref, manifest) = fetch_to_staging(workspace, &import_config).await?;

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
            if old_hash.as_deref() == Some(new_hash.as_str()) {
                unchanged += 1;
            } else {
                write_reference_to_disk(workspace, ref_path, content)?;
                db::knowledge::update_reference(db, &ref_id, content, Some(&new_hash)).await?;
                updated += 1;
            }
        } else {
            write_reference_to_disk(workspace, ref_path, content)?;
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

/// Convert the import source to a staging directory and read it as a manifest.
///
/// Returns `(optional_version_ref, manifest_entries)`. Manifest paths are
/// prefixed with the topic name (matching DB `references.path` format). The
/// staging tempdir is automatically cleaned up when this function returns.
async fn fetch_to_staging(
    workspace: &Path,
    config: &ImportConfig,
) -> Result<(Option<String>, Vec<(String, String, Option<String>)>), ImportError> {
    let staging_root = tempfile::tempdir()?;

    match &config.source {
        ImportSource::Git {
            url,
            paths,
            extensions,
            git_ref,
        } => {
            let result = convert_git(
                workspace,
                staging_root.path(),
                url,
                paths,
                extensions,
                git_ref.as_deref(),
            )
            .await?;
            let manifest = read_staging_as_manifest(&result.staging_dir, &config.topic, None)?;
            Ok((Some(result.version_ref), manifest))
        }
        ImportSource::Crawl {
            url,
            max_depth,
            max_pages,
        } => {
            let result = convert_crawl(staging_root.path(), url, *max_depth, *max_pages).await?;
            let url_map: HashMap<String, String> = result.page_urls.into_iter().collect();
            let manifest =
                read_staging_as_manifest(&result.staging_dir, &config.topic, Some(&url_map))?;
            Ok((None, manifest))
        }
        ImportSource::File { .. } | ImportSource::Book { .. } => Err(ImportError::Config(
            "only git and crawl sources support update".into(),
        )),
    }
}

/// Write a reference file to disk under `workspace/references/`.
fn write_reference_to_disk(
    workspace: &Path,
    ref_path: &str,
    content: &str,
) -> Result<(), ImportError> {
    let disk_path = workspace.join("references").join(ref_path);
    if let Some(parent) = disk_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&disk_path, content)?;
    Ok(())
}

/// Read all files from a staging directory into a manifest.
///
/// Each entry is `(topic_prefixed_path, file_content, optional_source_url)`.
/// Paths are prefixed with `topic` to match the DB `references.path` format
/// (e.g. `"dioxus/docs/getting-started.md"`). Directories starting with `_`
/// or `.` are skipped. The optional `url_map` maps relative filenames to their
/// source URLs (used for crawl imports).
fn read_staging_as_manifest(
    staging_dir: &Path,
    topic: &str,
    url_map: Option<&HashMap<String, String>>,
) -> Result<Vec<(String, String, Option<String>)>, ImportError> {
    let mut entries = Vec::new();
    collect_staging_files(staging_dir, staging_dir, topic, url_map, &mut entries)?;
    Ok(entries)
}

/// Recursively collect files from `dir`, building manifest entries.
fn collect_staging_files(
    root: &Path,
    dir: &Path,
    topic: &str,
    url_map: Option<&HashMap<String, String>>,
    out: &mut Vec<(String, String, Option<String>)>,
) -> Result<(), ImportError> {
    let read_dir = std::fs::read_dir(dir)?;

    for entry in read_dir {
        let entry = entry?;
        let path = entry.path();
        let name = entry.file_name();
        let name_str = name.to_string_lossy();

        if path.is_dir() {
            if name_str.starts_with('_') || name_str.starts_with('.') {
                continue;
            }
            collect_staging_files(root, &path, topic, url_map, out)?;
        } else if path.is_file() {
            let rel = path
                .strip_prefix(root)
                .map_err(|e| ImportError::Config(format!("path prefix error: {e}")))?
                .to_string_lossy();
            let ref_path = format!("{topic}/{rel}");

            let Ok(content) = std::fs::read_to_string(&path) else {
                continue; // skip binary/unreadable files
            };

            let source_url = url_map.and_then(|m| {
                // Try matching by filename (crawl uses flat filenames)
                let filename = path
                    .file_name()
                    .map(|f| f.to_string_lossy().to_string())
                    .unwrap_or_default();
                m.get(&filename).cloned()
            });

            out.push((ref_path, content, source_url));
        }
    }

    Ok(())
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
