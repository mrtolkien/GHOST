use std::path::Path;

use crate::db;
use crate::db::GhostDb;
use crate::knowledge;

use super::topic::ensure_topic_hierarchy;
use super::types::{ImportError, ImportProvenance, ImportResult, YoutubeImportProvenance};

/// Directories to skip when recursively collecting markdown files.
const SKIP_DIR_PREFIXES: &[char] = &['_', '.'];

/// Converter metadata file used to carry source-specific staging metadata.
const METADATA_FILE: &str = "_metadata.json";

/// Generic entry point for writing converted references into the workspace and DB.
///
/// Handles single-file and directory imports, optional provenance (batch +
/// `_import.toml`), and idempotent reference creation (skip-if-path-exists).
/// All reference writes — CLI import, update diffs, curation — should go
/// through this function.
#[tracing::instrument(
    name = "import_from_path",
    skip_all,
    fields(topic = %topic, path = %path.display())
)]
pub async fn import_from_path(
    db: &GhostDb,
    workspace: &Path,
    path: &Path,
    topic: &str,
    provenance: &ImportProvenance,
    source_url: Option<&str>,
) -> Result<ImportResult, ImportError> {
    if !path.exists() {
        return Err(ImportError::Io(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!("import path not found: {}", path.display()),
        )));
    }

    let provenance = enrich_youtube_provenance_from_staging(path, provenance)?;

    // Ensure topic hierarchy in DB
    let topic_id = ensure_topic_hierarchy(db, topic).await?;

    // Collect markdown files as (relative_path, absolute_path) pairs
    let md_files = collect_markdown_files(path)?;
    if md_files.is_empty() {
        return Err(ImportError::Config(format!(
            "no markdown files found in {}",
            path.display()
        )));
    }

    let ref_dir = workspace.join("references").join(topic);
    std::fs::create_dir_all(&ref_dir)?;

    // Copy _originals/ directory if present in source (e.g. PDF imports)
    copy_originals_dir(path, &ref_dir)?;

    let total_files = md_files.len();
    let mut created = 0usize;
    let mut skipped = 0usize;
    let reference_source_url = source_url.or(provenance.source_url.as_deref());
    let mut imported_ref_paths = Vec::with_capacity(md_files.len());

    for (relative, abs_path) in &md_files {
        let ref_path = format!("{topic}/{relative}");
        imported_ref_paths.push(ref_path.clone());

        // Idempotency: skip if reference with this path already exists
        if db::knowledge::find_reference_by_path(db, &ref_path)
            .await?
            .is_some()
        {
            skipped += 1;
            continue;
        }

        let processed = created + skipped + 1;
        println!("  [{processed}/{total_files}] {ref_path}");

        let content = std::fs::read_to_string(abs_path)?;

        // Write to disk: references/{topic}/{relative}
        let disk_path = workspace.join("references").join(&ref_path);
        if let Some(parent) = disk_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&disk_path, &content)?;

        let hash = crate::embeddings::pipeline::content_hash(&content);
        db::knowledge::create_reference(
            db,
            &topic_id,
            &ref_path,
            &content,
            reference_source_url,
            None, // batch_id filled below if provenance present
            Some(&hash),
        )
        .await?;

        created += 1;
    }

    // Provenance: create batch + write _import.toml if we have source metadata
    let batch_id = upsert_provenance(
        db,
        workspace,
        &topic_id,
        topic,
        &provenance,
        created,
        skipped,
    )
    .await?;

    if let Some(batch_id) = batch_id.as_deref() {
        for ref_path in &imported_ref_paths {
            db::knowledge::update_reference_import_metadata_by_path(
                db,
                &topic_id,
                ref_path,
                batch_id,
                reference_source_url,
            )
            .await?;
        }
    }

    // Ensure skeleton index notes exist for the topic hierarchy
    knowledge::ensure_index_notes(workspace, topic)
        .map_err(|e| ImportError::Io(std::io::Error::other(e.to_string())))?;

    Ok(ImportResult {
        topic_id,
        batch_id,
        references_created: created,
        references_skipped: skipped,
    })
}

fn enrich_youtube_provenance_from_staging(
    path: &Path,
    provenance: &ImportProvenance,
) -> Result<ImportProvenance, ImportError> {
    if provenance.source_type.as_deref() != Some("youtube") || provenance.youtube.is_some() {
        return Ok(provenance.clone());
    }
    if !path.is_dir() {
        return Ok(provenance.clone());
    }

    let metadata_path = path.join(METADATA_FILE);
    if !metadata_path.is_file() {
        return Ok(provenance.clone());
    }

    let metadata = read_youtube_metadata(&metadata_path)?;
    let mut enriched = provenance.clone();
    enriched.youtube = Some(YoutubeImportProvenance {
        video_id: Some(metadata.metadata.video_id),
        title: metadata.metadata.title,
        channel: metadata.metadata.channel,
        published_at: metadata.metadata.published_at,
        duration_seconds: metadata.metadata.duration_seconds,
        transcript_source: Some(
            match metadata.metadata.transcript_source {
                crate::convert::youtube::TranscriptSource::Manual => "manual",
                crate::convert::youtube::TranscriptSource::Auto => "auto",
                crate::convert::youtube::TranscriptSource::Whisper => "whisper",
            }
            .to_string(),
        ),
        section_count: Some(metadata.section_count),
        chapter_count: Some(metadata.chapter_count),
        language: metadata.metadata.language,
    });
    Ok(enriched)
}

fn read_youtube_metadata(
    metadata_path: &Path,
) -> Result<crate::convert::youtube::YoutubeStagingMetadata, ImportError> {
    let raw = std::fs::read_to_string(metadata_path)?;
    serde_json::from_str(&raw).map_err(|error| {
        ImportError::Config(format!(
            "failed to parse YouTube staging metadata {}: {error}",
            metadata_path.display()
        ))
    })
}

/// Collect markdown files from `path`. Returns (relative_path, absolute_path) pairs.
///
/// If `path` is a file, returns it directly with just the filename as
/// relative path. If `path` is a directory, recursively collects all `.md`
/// files, skipping directories starting with `_` or `.`.
fn collect_markdown_files(path: &Path) -> Result<Vec<(String, std::path::PathBuf)>, ImportError> {
    if path.is_file() {
        let filename = path
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or_else(|| ImportError::Config(format!("invalid filename: {}", path.display())))?;
        return Ok(vec![(filename.to_string(), path.to_path_buf())]);
    }

    let mut results = Vec::new();
    collect_md_recursive(path, path, &mut results)?;
    results.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(results)
}

/// Recursively walk `dir`, collecting `.md` files relative to `base`.
fn collect_md_recursive(
    base: &Path,
    dir: &Path,
    results: &mut Vec<(String, std::path::PathBuf)>,
) -> Result<(), ImportError> {
    let entries = std::fs::read_dir(dir)?;

    for entry in entries {
        let entry = entry?;
        let path = entry.path();
        let file_name = entry.file_name();
        let name = file_name.to_string_lossy();

        if path.is_dir() {
            // Skip directories starting with _ or .
            if name.starts_with(SKIP_DIR_PREFIXES) {
                continue;
            }
            collect_md_recursive(base, &path, results)?;
        } else if path.extension().and_then(|e| e.to_str()) == Some("md") {
            let relative = path
                .strip_prefix(base)
                .map_err(|e| ImportError::Config(e.to_string()))?;
            let rel_str = relative.to_string_lossy().to_string();
            results.push((rel_str, path));
        }
    }

    Ok(())
}

/// Copy the `_originals/` directory from source into the reference dir, if present.
fn copy_originals_dir(source: &Path, ref_dir: &Path) -> Result<(), ImportError> {
    if !source.is_dir() {
        return Ok(());
    }
    let originals_src = source.join("_originals");
    if !originals_src.is_dir() {
        return Ok(());
    }

    let originals_dst = ref_dir.join("_originals");
    std::fs::create_dir_all(&originals_dst)?;

    for entry in std::fs::read_dir(&originals_src)? {
        let entry = entry?;
        let src_path = entry.path();
        if src_path.is_file() {
            let filename = entry.file_name();
            std::fs::copy(&src_path, originals_dst.join(filename))?;
        }
    }

    Ok(())
}

/// If provenance has source metadata, create/update the import batch and write
/// `_import.toml`. Returns the batch_id if one was created.
async fn upsert_provenance(
    db: &GhostDb,
    workspace: &Path,
    topic_id: &str,
    topic: &str,
    provenance: &ImportProvenance,
    created: usize,
    skipped: usize,
) -> Result<Option<String>, ImportError> {
    let (Some(source_type), Some(source_url)) = (&provenance.source_type, &provenance.source_url)
    else {
        return Ok(None);
    };

    let config_json = provenance.to_import_config_json(source_type, source_url);
    let config_json_str = serde_json::to_string(&config_json).ok();

    // Total references for this topic (existing + newly created)
    let total_refs = usize::try_from(
        db::knowledge::count_references_by_topic(db, topic_id)
            .await?
            .max(0),
    )
    .unwrap_or(created + skipped);

    let batch_id = db::knowledge::upsert_import_batch(
        db,
        topic_id,
        source_type,
        source_url,
        provenance.version_ref.as_deref(),
        total_refs as i64,
        config_json_str.as_deref(),
    )
    .await?;

    super::topic::write_import_toml(
        workspace,
        topic,
        &config_json,
        provenance.version_ref.as_deref(),
        total_refs,
    )?;

    Ok(Some(batch_id))
}
