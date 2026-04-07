use std::path::Path;

use crate::db;
use crate::db::GhostDb;
use crate::knowledge;

use super::topic::ensure_topic_hierarchy;
use super::types::{ImportConfigJson, ImportError, ImportProvenance, ImportResult};

/// Directories to skip when recursively collecting markdown files.
const SKIP_DIR_PREFIXES: &[char] = &['_', '.'];
const BOOK_METADATA_FILENAME: &str = "_metadata.json";

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

    let config_json = build_import_config_json(path, provenance)?;

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

    for (relative, abs_path) in &md_files {
        let ref_path = format!("{topic}/{relative}");

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
            source_url,
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
        provenance,
        &config_json,
        created + skipped,
    )
    .await?;

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
    config_json: &ImportConfigJson,
    total_refs_fallback: usize,
) -> Result<Option<String>, ImportError> {
    let config_json_str = serde_json::to_string(&config_json)
        .map_err(|e| ImportError::Config(format!("invalid import metadata JSON: {e}")))?;

    // Total references for this topic (existing + newly created)
    let total_refs = usize::try_from(
        db::knowledge::count_references_by_topic(db, topic_id)
            .await?
            .max(0),
    )
    .unwrap_or(total_refs_fallback);

    let batch_id = db::knowledge::upsert_import_batch(
        db,
        topic_id,
        &config_json.source_type,
        &config_json.source_url,
        provenance.version_ref.as_deref(),
        total_refs as i64,
        Some(&config_json_str),
    )
    .await?;

    super::topic::write_import_toml(
        workspace,
        topic,
        config_json,
        provenance.version_ref.as_deref(),
        total_refs,
    )?;

    Ok(Some(batch_id))
}

fn build_import_config_json(
    path: &Path,
    provenance: &ImportProvenance,
) -> Result<ImportConfigJson, ImportError> {
    let (source_type, source_url) =
        require_repair_critical_provenance(&provenance.source_type, &provenance.source_url)?;
    let is_book_import = source_type == "book";

    let mut config_json = ImportConfigJson {
        source_type,
        source_url,
        git_ref: provenance.git_ref.clone(),
        paths: provenance.paths.clone(),
        extensions: provenance.extensions.clone(),
        max_depth: provenance.max_depth,
        max_pages: provenance.max_pages,
        no_ocr: provenance.no_ocr,
        page_range: provenance.page_range,
        title: None,
        authors: None,
        language: None,
        publisher: None,
        publication_date: None,
    };

    match config_json.source_type.as_str() {
        "git" => validate_git_import_provenance(&config_json, provenance)?,
        "crawl" => validate_crawl_import_provenance(&config_json)?,
        "file" => enrich_file_import_metadata(path, &mut config_json)?,
        "book" if is_book_import => enrich_book_import_metadata(path, &mut config_json)?,
        _ => {}
    }

    Ok(config_json)
}

fn validate_git_import_provenance(
    _config_json: &ImportConfigJson,
    provenance: &ImportProvenance,
) -> Result<(), ImportError> {
    if provenance.version_ref.is_none() {
        return Err(ImportError::Config(
            "supported imports require repair-critical import provenance for source_type 'git': missing version_ref"
                .to_string(),
        ));
    }

    Ok(())
}

fn validate_crawl_import_provenance(config_json: &ImportConfigJson) -> Result<(), ImportError> {
    if config_json.max_depth.is_none() || config_json.max_pages.is_none() {
        return Err(ImportError::Config(
            "supported imports require repair-critical import provenance for source_type 'crawl': missing max_depth or max_pages"
                .to_string(),
        ));
    }

    Ok(())
}

fn enrich_book_import_metadata(
    path: &Path,
    config_json: &mut ImportConfigJson,
) -> Result<(), ImportError> {
    let metadata_path = path.join(BOOK_METADATA_FILENAME);
    let metadata_content = std::fs::read_to_string(&metadata_path).map_err(|e| {
        ImportError::Config(format!(
            "book imports require staging metadata at {}: {e}",
            metadata_path.display()
        ))
    })?;
    let metadata: crate::convert::epub::EpubMetadata = serde_json::from_str(&metadata_content)
        .map_err(|e| {
            ImportError::Config(format!(
                "invalid book staging metadata at {}: {e}",
                metadata_path.display()
            ))
        })?;

    config_json.title = metadata.title;
    config_json.authors = (!metadata.authors.is_empty()).then_some(metadata.authors);
    config_json.language = metadata.language;
    config_json.publisher = metadata.publisher;
    config_json.publication_date = metadata.publication_date;

    Ok(())
}

fn enrich_file_import_metadata(
    path: &Path,
    config_json: &mut ImportConfigJson,
) -> Result<(), ImportError> {
    let metadata_path = path.join(BOOK_METADATA_FILENAME);
    let Ok(metadata_content) = std::fs::read_to_string(&metadata_path) else {
        return Ok(());
    };
    let metadata: FileImportMetadata = serde_json::from_str(&metadata_content).map_err(|e| {
        ImportError::Config(format!(
            "invalid file staging metadata at {}: {e}",
            metadata_path.display()
        ))
    })?;
    config_json.no_ocr = metadata.no_ocr;
    config_json.page_range = metadata.page_range;
    Ok(())
}

fn require_repair_critical_provenance(
    source_type: &Option<String>,
    source_url: &Option<String>,
) -> Result<(String, String), ImportError> {
    let Some(source_type) = source_type.as_ref().map(|value| value.trim()) else {
        return Err(ImportError::Config(
            "supported imports require repair-critical import provenance: missing source_type"
                .to_string(),
        ));
    };
    if source_type.is_empty() {
        return Err(ImportError::Config(
            "supported imports require repair-critical import provenance: missing source_type"
                .to_string(),
        ));
    }

    let Some(source_url) = source_url.as_ref().map(|value| value.trim()) else {
        return Err(ImportError::Config(format!(
            "supported imports require repair-critical import provenance for source_type '{source_type}': missing source_url"
        )));
    };
    if source_url.is_empty() {
        return Err(ImportError::Config(format!(
            "supported imports require repair-critical import provenance for source_type '{source_type}': missing source_url"
        )));
    }

    Ok((source_type.to_string(), source_url.to_string()))
}

#[derive(serde::Deserialize)]
struct FileImportMetadata {
    no_ocr: Option<bool>,
    page_range: Option<(u32, u32)>,
}
