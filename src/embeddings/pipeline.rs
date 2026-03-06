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

/// Embed a single knowledge source. Skips if content hash is unchanged.
/// Returns the number of chunks embedded (0 if skipped).
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
) -> Result<usize, PipelineError> {
    let hash = content_hash(content);

    if let Some(stored) = db::embeddings::get_content_hash(db, source_id).await?
        && stored == hash
    {
        return Ok(0);
    }

    embed_source_inner(
        client,
        db,
        source_table,
        source_id,
        content,
        tags,
        &hash,
        topic_id,
    )
    .await
}

/// Embed a single source, ignoring any stored hash (for --force reindex).
#[tracing::instrument(skip_all, fields(
    source_table = %source_table,
    source_id = ?source_id,
), level = "debug"
)]
pub async fn embed_source_forced(
    client: &EmbeddingClient,
    db: &GhostDb,
    source_table: &str,
    source_id: &str,
    content: &str,
    tags: &[String],
    topic_id: Option<&str>,
) -> Result<usize, PipelineError> {
    let hash = content_hash(content);
    embed_source_inner(
        client,
        db,
        source_table,
        source_id,
        content,
        tags,
        &hash,
        topic_id,
    )
    .await
}

async fn embed_source_inner(
    client: &EmbeddingClient,
    db: &GhostDb,
    source_table: &str,
    source_id: &str,
    content: &str,
    tags: &[String],
    hash: &str,
    topic_id: Option<&str>,
) -> Result<usize, PipelineError> {
    let chunks = chunk_content(content, tags, None);
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
        hash,
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

    // Phase 1: filter unchanged sources and chunk the rest
    struct PreparedSource {
        table: String,
        id: String,
        hash: String,
        chunks: Vec<Chunk>,
        topic_id: Option<String>,
    }

    let mut prepared: Vec<PreparedSource> = Vec::new();
    let mut skipped = 0usize;

    for req in &requests {
        let hash = content_hash(&req.content);
        if let Some(stored) = db::embeddings::get_content_hash(db, &req.source_id).await?
            && stored == hash
        {
            skipped += 1;
            continue;
        }
        let chunks = chunk_content(&req.content, &req.tags, req.path.as_deref());
        if chunks.is_empty() {
            skipped += 1;
            continue;
        }
        prepared.push(PreparedSource {
            table: req.source_table.clone(),
            id: req.source_id.clone(),
            hash,
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
            &src.hash,
            src.topic_id.as_deref(),
        )
        .await?;

        total_embedded += chunk_data.len();
    }

    tracing::Span::current().record("embedded", total_embedded as u64);

    Ok(total_embedded)
}

const RECONCILE_PAGE_SIZE: usize = 50;

/// Run boot reconciliation: find sources that need embedding and embed them.
/// Processes records in pages to avoid loading all knowledge into memory at once.
#[tracing::instrument(name = "reconcile embeddings", skip_all, fields(
    embedded = tracing::field::Empty,
    skipped = tracing::field::Empty,
))]
pub async fn reconcile_embeddings(
    client: &EmbeddingClient,
    db: &GhostDb,
) -> Result<(usize, usize), PipelineError> {
    let mut embedded = 0usize;
    let mut skipped = 0usize;

    // Reconcile notes (paginated)
    let t = std::time::Instant::now();
    let mut offset = 0;
    loop {
        let notes = db::knowledge::list_notes_page(db, offset, RECONCILE_PAGE_SIZE).await?;
        let batch_len = notes.len();
        for note in &notes {
            let tags = note.tags_parsed();
            let count = embed_source(
                client,
                db,
                "note",
                &note.id,
                &note.body,
                &tags,
                note.topic_id.as_deref(),
            )
            .await?;
            if count > 0 {
                embedded += count;
            } else {
                skipped += 1;
            }
        }
        if batch_len < RECONCILE_PAGE_SIZE {
            break;
        }
        offset += batch_len;
    }
    tracing::info!(ms = t.elapsed().as_millis() as u64, "reconciled notes");

    // Reconcile references (paginated)
    let t = std::time::Instant::now();
    let mut offset = 0;
    loop {
        let refs = db::knowledge::list_references_page(db, offset, RECONCILE_PAGE_SIZE).await?;
        let batch_len = refs.len();
        for reference in &refs {
            let count = embed_source(
                client,
                db,
                "reference",
                &reference.id,
                &reference.content,
                &[],
                Some(&reference.topic_id),
            )
            .await?;
            if count > 0 {
                embedded += count;
            } else {
                skipped += 1;
            }
        }
        if batch_len < RECONCILE_PAGE_SIZE {
            break;
        }
        offset += batch_len;
    }
    tracing::info!(ms = t.elapsed().as_millis() as u64, "reconciled references");

    // Reconcile diary entries (paginated)
    let t = std::time::Instant::now();
    let mut offset = 0;
    loop {
        let entries = db::knowledge::list_diary_page(db, offset, RECONCILE_PAGE_SIZE).await?;
        let batch_len = entries.len();
        for entry in &entries {
            let count =
                embed_source(client, db, "diary", &entry.id, &entry.body, &[], None).await?;
            if count > 0 {
                embedded += count;
            } else {
                skipped += 1;
            }
        }
        if batch_len < RECONCILE_PAGE_SIZE {
            break;
        }
        offset += batch_len;
    }
    tracing::info!(ms = t.elapsed().as_millis() as u64, "reconciled diary");

    tracing::Span::current().record("embedded", embedded as u64);
    tracing::Span::current().record("skipped", skipped as u64);

    Ok((embedded, skipped))
}
