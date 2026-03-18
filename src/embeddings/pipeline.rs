use std::collections::HashMap;

use sha2::{Digest, Sha256};

use crate::db;
use crate::db::GhostDb;

use super::chunker::{Chunk, chunk_content};
use super::client::EmbeddingClient;
use super::error::EmbeddingError;

#[derive(Debug, thiserror::Error)]
pub enum PipelineError {
    #[error(transparent)]
    Embedding(#[from] EmbeddingError),

    #[error(transparent)]
    Database(#[from] db::DatabaseError),
}

/// Compute a hex-encoded SHA-256 hash of the given content.
pub fn content_hash(content: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(content.as_bytes());
    hex::encode(hasher.finalize())
}

/// Embed a single knowledge source. Returns the number of chunks embedded.
#[tracing::instrument(name = "embed source", skip_all, fields(
    source_table = %source_table,
    source_id = ?source_id,
))]
pub async fn embed_source(
    client: &EmbeddingClient,
    db: &GhostDb,
    source_table: &str,
    source_id: &str,
    content: &str,
    tags: &[String],
    topic_id: Option<&str>,
    path: Option<&str>,
) -> Result<usize, PipelineError> {
    let chunks = chunk_content(content, tags, path);
    if chunks.is_empty() {
        return Ok(0);
    }

    let vectors = embed_chunks(client, &chunks).await?;

    let chunk_data: Vec<(usize, String, Vec<f32>)> = chunks
        .iter()
        .zip(vectors)
        .map(|(chunk, vec)| (chunk.index, chunk.text.clone(), vec))
        .collect();

    db::embeddings::replace_embeddings_for_source(
        db,
        source_table,
        source_id,
        &chunk_data,
        topic_id,
    )
    .await?;

    Ok(chunk_data.len())
}

/// Embed chunk texts in batches according to client batch_size.
async fn embed_chunks(
    client: &EmbeddingClient,
    chunks: &[Chunk],
) -> Result<Vec<Vec<f32>>, EmbeddingError> {
    let batch_size = client.batch_size();
    let texts: Vec<String> = chunks.iter().map(|c| c.text.clone()).collect();

    let mut all_vectors = Vec::with_capacity(texts.len());
    for batch in texts.chunks(batch_size) {
        let vectors = client.embed_batch(batch).await?;
        all_vectors.extend(vectors);
    }
    Ok(all_vectors)
}

/// A request to embed a single knowledge source, used for cross-file batching.
#[derive(Debug)]
pub struct EmbedRequest {
    pub source_table: String,
    pub source_id: String,
    pub content: String,
    pub tags: Vec<String>,
    pub topic_id: Option<String>,
    /// File path for code chunking (e.g. "src/main.rs"). When set, the
    /// pipeline tries tree-sitter AST chunking before falling back to text.
    pub path: Option<String>,
}

/// Embed multiple knowledge sources in a single batched Ollama call.
///
/// Filters unchanged sources (hash check), chunks all changed sources,
/// sends all chunks to Ollama in one (or few) batch call(s), then
/// distributes the resulting vectors back to their respective sources.
/// Returns the total number of chunks embedded.
#[tracing::instrument(name = "embed sources", skip_all, fields(
    sources = requests.len(),
    embedded = tracing::field::Empty,
    skipped = tracing::field::Empty,
))]
pub async fn embed_sources(
    client: &EmbeddingClient,
    db: &GhostDb,
    requests: Vec<EmbedRequest>,
) -> Result<usize, PipelineError> {
    if requests.is_empty() {
        return Ok(0);
    }

    // Phase 1: chunk all sources
    struct PreparedSource {
        table: String,
        id: String,
        chunks: Vec<Chunk>,
        topic_id: Option<String>,
    }

    let mut prepared: Vec<PreparedSource> = Vec::new();
    let mut skipped = 0usize;

    for req in &requests {
        let chunks = chunk_content(&req.content, &req.tags, req.path.as_deref());
        if chunks.is_empty() {
            skipped += 1;
            continue;
        }
        prepared.push(PreparedSource {
            table: req.source_table.clone(),
            id: req.source_id.clone(),
            chunks,
            topic_id: req.topic_id.clone(),
        });
    }

    tracing::Span::current().record("skipped", skipped as u64);

    if prepared.is_empty() {
        tracing::Span::current().record("embedded", 0u64);
        return Ok(0);
    }

    // Phase 2: collect all chunk texts into one big batch
    let mut all_texts: Vec<String> = Vec::new();
    // Track which range of the flat vector belongs to each source
    let mut ranges: Vec<std::ops::Range<usize>> = Vec::new();

    for src in &prepared {
        let start = all_texts.len();
        all_texts.extend(src.chunks.iter().map(|c| c.text.clone()));
        ranges.push(start..all_texts.len());
    }

    // Phase 3: embed all chunks in batch_size batches
    let batch_size = client.batch_size();
    let mut all_vectors: Vec<Vec<f32>> = Vec::with_capacity(all_texts.len());
    for batch in all_texts.chunks(batch_size) {
        let batch_strings: Vec<String> = batch.to_vec();
        let vectors = client.embed_batch(&batch_strings).await?;
        all_vectors.extend(vectors);
    }

    // Phase 4: distribute vectors back and persist atomically per source
    let mut total_embedded = 0usize;
    for (src, range) in prepared.iter().zip(ranges.iter()) {
        let src_vectors = &all_vectors[range.clone()];

        let chunk_data: Vec<(usize, String, Vec<f32>)> = src
            .chunks
            .iter()
            .zip(src_vectors.iter())
            .map(|(chunk, vec)| (chunk.index, chunk.text.clone(), vec.clone()))
            .collect();

        db::embeddings::replace_embeddings_for_source(
            db,
            &src.table,
            &src.id,
            &chunk_data,
            src.topic_id.as_deref(),
        )
        .await?;

        total_embedded += chunk_data.len();
    }

    tracing::Span::current().record("embedded", total_embedded as u64);

    Ok(total_embedded)
}

/// Scan the filesystem and reconcile with DB using batch hash checking.
///
/// Walks `notes/`, `references/`, `diary/`, `scripts/` under the workspace.
/// For each file, computes SHA-256 and compares against stored file_hash:
/// - Hash matches + has embeddings: skip entirely
/// - Hash matches + no embeddings: queue embed request only (no DB upsert)
/// - Hash differs or path missing: full `process_change` + store hash
///
/// Returns (files_discovered, embed_requests).
#[tracing::instrument(name = "reconcile filesystem", skip_all, fields(
    discovered = tracing::field::Empty,
))]
pub async fn reconcile_filesystem(
    db: &GhostDb,
    workspace: &std::path::Path,
) -> Result<(usize, Vec<EmbedRequest>), PipelineError> {
    // Phase 1: Load existing hashes from DB
    let note_hashes = db::knowledge::load_note_file_hashes(db).await?;
    let ref_hashes = db::knowledge::load_reference_file_hashes(db).await?;
    let diary_hashes = db::knowledge::load_diary_file_hashes(db).await?;
    let script_hashes = db::knowledge::load_script_file_hashes(db).await?;

    let mut known: HashMap<String, (Option<String>, bool)> = HashMap::new();
    for r in note_hashes {
        known.insert(r.path, (r.file_hash, r.has_embeddings));
    }
    for r in ref_hashes {
        known.insert(
            format!("references/{}", r.path),
            (r.file_hash, r.has_embeddings),
        );
    }
    for r in diary_hashes {
        known.insert(
            format!("diary/{}.md", r.path),
            (r.file_hash, r.has_embeddings),
        );
    }
    for r in script_hashes {
        known.insert(
            format!("scripts/{}", r.path),
            (r.file_hash, r.has_embeddings),
        );
    }

    // Phase 2: Walk filesystem, check hashes, process changed files
    let mut discovered = 0usize;
    let mut embed_requests = Vec::new();

    for subdir in ["notes", "references", "diary", "scripts"] {
        let dir = workspace.join(subdir);
        if !dir.exists() {
            continue;
        }
        let files = walk_directory(&dir);
        for file_path in files {
            let rel = file_path.strip_prefix(workspace).unwrap_or(&file_path);
            let rel_str = rel.to_string_lossy().to_string();

            // Read file and compute hash
            let raw = match tokio::fs::read_to_string(&file_path).await {
                Ok(r) => r,
                Err(_) => continue,
            };
            let hash = content_hash(&raw);

            match known.get(&rel_str) {
                // Hash matches and has embeddings -> skip entirely
                Some((Some(stored_hash), true)) if *stored_hash == hash => continue,
                // Hash matches but missing embeddings -> re-embed only (no DB upsert)
                Some((Some(stored_hash), false)) if *stored_hash == hash => {
                    if let Some(req) =
                        build_embed_request_from_db(db, workspace, &file_path, &raw).await
                    {
                        embed_requests.push(req);
                    }
                }
                // Hash differs or missing -> full process
                _ => {
                    match crate::daemon::watcher::process_change(
                        db,
                        workspace,
                        &file_path,
                        Some(&raw),
                        Some(&hash),
                    )
                    .await
                    {
                        Ok(Some(req)) => {
                            embed_requests.push(req);
                            discovered += 1;
                        }
                        Ok(None) => {}
                        Err(e) => {
                            tracing::warn!(
                                path = file_path.display().to_string(),
                                error = e.to_string(),
                                "reconcile: failed to process file"
                            );
                        }
                    }
                }
            }
        }
    }

    tracing::Span::current().record("discovered", discovered as u64);
    Ok((discovered, embed_requests))
}

/// Build an EmbedRequest for a file that exists in DB but lacks embeddings.
async fn build_embed_request_from_db(
    db: &GhostDb,
    workspace: &std::path::Path,
    file_path: &std::path::Path,
    content: &str,
) -> Option<EmbedRequest> {
    let rel = file_path.strip_prefix(workspace).ok()?;
    let rel_str = rel.to_string_lossy();

    if rel_str.starts_with("notes/") {
        let note = db::knowledge::find_note_by_path(db, &rel_str)
            .await
            .ok()??;
        let tags = note.tags_parsed();
        Some(EmbedRequest {
            source_table: "note".into(),
            source_id: note.id,
            content: content.to_string(),
            tags,
            topic_id: note.topic_id,
            path: None,
        })
    } else if rel_str.starts_with("references/") {
        let ref_path = rel_str.strip_prefix("references/").unwrap_or(&rel_str);
        let reference = db::knowledge::find_reference_by_path(db, ref_path)
            .await
            .ok()??;
        Some(EmbedRequest {
            source_table: "reference".into(),
            source_id: reference.id,
            content: content.to_string(),
            tags: vec![],
            topic_id: Some(reference.topic_id),
            path: Some(rel_str.to_string()),
        })
    } else if rel_str.starts_with("diary/") {
        let date = file_path.file_stem()?.to_str()?;
        let diary = db::knowledge::get_diary_by_date(db, date).await.ok()??;
        Some(EmbedRequest {
            source_table: "diary".into(),
            source_id: diary.id,
            content: content.to_string(),
            tags: vec![],
            topic_id: None,
            path: None,
        })
    } else if rel_str.starts_with("scripts/") {
        let script_path = rel_str.strip_prefix("scripts/").unwrap_or(&rel_str);
        let script = db::knowledge::find_script_by_path(db, script_path)
            .await
            .ok()??;
        Some(EmbedRequest {
            source_table: "script".into(),
            source_id: script.id,
            content: content.to_string(),
            tags: vec![],
            topic_id: None,
            path: Some(rel_str.to_string()),
        })
    } else {
        None
    }
}

/// File extensions eligible for code embedding.
pub(crate) const CODE_EXTENSIONS: &[&str] = &[
    // Tree-sitter supported (AST-aware chunking)
    "rs", "py", "js", "jsx", "ts", "tsx", "go", "sh", "bash", "toml", "json",
    // Text fallback (line-based chunking)
    "c", "h", "cpp", "hpp", "java", "kt", "rb", "sql", "lua", "zig", "ex", "exs", "yaml", "yml",
    "md",
];

/// Maximum file size for code embedding (100KB).
pub(crate) const MAX_CODE_FILE_SIZE: u64 = 100 * 1024;

/// Walk a repo directory respecting .gitignore, extension allowlist, and size limit.
pub(crate) fn walk_code_repo(repo_dir: &std::path::Path) -> Vec<std::path::PathBuf> {
    let mut files = Vec::new();
    let walker = ignore::WalkBuilder::new(repo_dir)
        .hidden(true) // skip hidden files (but .gitignore still read)
        .git_ignore(true) // respect .gitignore
        .git_exclude(true) // respect .git/info/exclude
        .build();

    for entry in walker.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        // Extension allowlist
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
        if !CODE_EXTENSIONS.contains(&ext) {
            continue;
        }
        // Size guard
        if let Ok(meta) = path.metadata() {
            if meta.len() > MAX_CODE_FILE_SIZE {
                tracing::debug!(
                    path = path.display().to_string(),
                    size = meta.len(),
                    "skipping large code file"
                );
                continue;
            }
        }
        files.push(path.to_path_buf());
    }
    files
}

/// Extract repo slug from a path under `code/`. Returns `None` if not a code path.
pub(crate) fn extract_repo_slug(
    workspace: &std::path::Path,
    path: &std::path::Path,
) -> Option<String> {
    let rel = path.strip_prefix(workspace).ok()?;
    let mut components = rel.components();
    let first = components.next()?;
    if first.as_os_str() != "code" {
        return None;
    }
    let slug = components.next()?;
    Some(slug.as_os_str().to_string_lossy().to_string())
}

fn walk_directory(dir: &std::path::Path) -> Vec<std::path::PathBuf> {
    let mut files = Vec::new();
    walk_directory_inner(dir, &mut files);
    files
}

fn walk_directory_inner(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if path.file_name().is_some_and(|n| n == ".archive") {
                continue;
            }
            walk_directory_inner(&path, out);
        } else if path.is_file() {
            out.push(path);
        }
    }
}
