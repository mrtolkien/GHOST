use serde::Deserialize;
use surrealdb::Surreal;
use surrealdb::engine::local::Db;
use surrealdb::types::{RecordId, SurrealValue};

use super::error::DatabaseError;
use super::query::{CountRow, query_exec, take_many};

#[derive(Debug, Clone, Deserialize, SurrealValue)]
#[surreal(crate = "surrealdb::types")]
pub struct EmbeddingHit {
    pub source_id: RecordId,
    pub source_table: String,
    pub chunk_text: String,
    pub score: f64,
}

#[derive(Debug, Deserialize, SurrealValue)]
#[surreal(crate = "surrealdb::types")]
struct HashRow {
    content_hash: String,
}

#[tracing::instrument(skip_all, fields(source_id = ?source_id, chunk_index))]
pub async fn upsert_embedding(
    db: &Surreal<Db>,
    source_table: &str,
    source_id: &RecordId,
    chunk_index: usize,
    chunk_text: &str,
    content_hash: &str,
    vector: &[f32],
) -> Result<(), DatabaseError> {
    query_exec(
        db.query(
            "INSERT INTO embedding {
            source_table: $source_table,
            source_id: $source_id,
            chunk_index: $chunk_index,
            chunk_text: $chunk_text,
            content_hash: $content_hash,
            vector: $vector,
            created_at: time::now()
        } ON DUPLICATE KEY UPDATE
            chunk_text = $input.chunk_text,
            content_hash = $input.content_hash,
            vector = $input.vector,
            created_at = time::now()",
        )
        .bind(("source_table", source_table.to_string()))
        .bind(("source_id", source_id.clone()))
        .bind(("chunk_index", chunk_index as i64))
        .bind(("chunk_text", chunk_text.to_string()))
        .bind(("content_hash", content_hash.to_string()))
        .bind(("vector", vector.to_vec())),
        "embedding",
        "upsert",
    )
    .await?;
    Ok(())
}

#[tracing::instrument(skip_all, fields(source_id = ?source_id))]
pub async fn get_content_hash(
    db: &Surreal<Db>,
    source_id: &RecordId,
) -> Result<Option<String>, DatabaseError> {
    let mut resp = query_exec(
        db.query(
            "SELECT content_hash FROM embedding
             WHERE source_id = $source_id
             LIMIT 1",
        )
        .bind(("source_id", source_id.clone())),
        "embedding",
        "get_content_hash",
    )
    .await?;

    let rows: Vec<HashRow> = take_many(&mut resp, 0, "embedding", "get_content_hash")?;
    Ok(rows.first().map(|r| r.content_hash.clone()))
}

#[tracing::instrument(skip_all, fields(source_id = ?source_id))]
pub async fn delete_embeddings_for_source(
    db: &Surreal<Db>,
    source_id: &RecordId,
) -> Result<(), DatabaseError> {
    query_exec(
        db.query("DELETE FROM embedding WHERE source_id = $source_id")
            .bind(("source_id", source_id.clone())),
        "embedding",
        "delete_for_source",
    )
    .await?;
    Ok(())
}

#[tracing::instrument(skip_all)]
pub async fn delete_all_embeddings(db: &Surreal<Db>) -> Result<(), DatabaseError> {
    query_exec(db.query("DELETE FROM embedding"), "embedding", "delete_all").await?;
    Ok(())
}

#[tracing::instrument(skip_all, fields(limit))]
pub async fn vector_search(
    db: &Surreal<Db>,
    query_vector: &[f32],
    limit: usize,
) -> Result<Vec<EmbeddingHit>, DatabaseError> {
    let query = format!(
        "SELECT source_id, source_table, chunk_text,
                vector::similarity::cosine(vector, $query_vector) AS score
         FROM embedding
         ORDER BY score DESC
         LIMIT {limit}"
    );
    let mut resp = query_exec(
        db.query(&query)
            .bind(("query_vector", query_vector.to_vec())),
        "embedding",
        "vector_search",
    )
    .await?;

    take_many(&mut resp, 0, "embedding", "vector_search")
}

#[tracing::instrument(skip_all)]
pub async fn count_embeddings(db: &Surreal<Db>) -> Result<i64, DatabaseError> {
    let mut resp = query_exec(
        db.query("SELECT count() AS count FROM embedding GROUP ALL"),
        "embedding",
        "count",
    )
    .await?;

    let rows: Vec<CountRow> = take_many(&mut resp, 0, "embedding", "count")?;
    Ok(rows.first().map(|r| r.count).unwrap_or(0))
}
