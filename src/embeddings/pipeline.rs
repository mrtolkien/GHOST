use sha2::{Digest, Sha256};
use surrealdb::Surreal;
use surrealdb::engine::local::Db;
use surrealdb::sql::Thing;

use crate::db;

use super::chunker::{Chunk, chunk_text};
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
#[tracing::instrument(name = "embed", skip_all, fields(
    source_table = %source_table,
    source_id = %source_id,
))]
pub async fn embed_source(
    client: &EmbeddingClient,
    db: &Surreal<Db>,
    source_table: &str,
    source_id: &Thing,
    content: &str,
    tags: &[String],
) -> Result<usize, PipelineError> {
    let hash = content_hash(content);

    if let Some(stored) = db::embeddings::get_content_hash(db, source_id).await?
        && stored == hash
    {
        return Ok(0);
    }

    embed_source_inner(client, db, source_table, source_id, content, tags, &hash).await
}

/// Embed a single source, ignoring any stored hash (for --force reindex).
#[tracing::instrument(skip_all, fields(
    source_table = %source_table,
    source_id = %source_id,
), level = "debug"
)]
pub async fn embed_source_forced(
    client: &EmbeddingClient,
    db: &Surreal<Db>,
    source_table: &str,
    source_id: &Thing,
    content: &str,
    tags: &[String],
) -> Result<usize, PipelineError> {
    let hash = content_hash(content);
    embed_source_inner(client, db, source_table, source_id, content, tags, &hash).await
}

async fn embed_source_inner(
    client: &EmbeddingClient,
    db: &Surreal<Db>,
    source_table: &str,
    source_id: &Thing,
    content: &str,
    tags: &[String],
    hash: &str,
) -> Result<usize, PipelineError> {
    let chunks = chunk_text(content, tags);
    if chunks.is_empty() {
        return Ok(0);
    }

    let vectors = embed_chunks(client, &chunks).await?;

    // Delete old embeddings for this source, then insert new ones
    db::embeddings::delete_embeddings_for_source(db, source_id).await?;

    for (chunk, vector) in chunks.iter().zip(vectors.iter()) {
        db::embeddings::upsert_embedding(
            db,
            source_table,
            source_id,
            chunk.index,
            &chunk.text,
            hash,
            vector,
        )
        .await?;
    }

    Ok(chunks.len())
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

const RECONCILE_PAGE_SIZE: usize = 50;

/// Run boot reconciliation: find sources that need embedding and embed them.
/// Processes records in pages to avoid loading all knowledge into memory at once.
#[tracing::instrument(skip_all, fields(
    embedded = tracing::field::Empty,
    skipped = tracing::field::Empty,
))]
pub async fn reconcile_embeddings(
    client: &EmbeddingClient,
    db: &Surreal<Db>,
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
            let count = embed_source(client, db, "note", &note.id, &note.body, &note.tags).await?;
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
            let count = embed_source(client, db, "diary", &entry.id, &entry.body, &[]).await?;
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
