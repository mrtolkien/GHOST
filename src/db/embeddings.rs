use serde::Deserialize;
use surrealdb::Surreal;
use surrealdb::engine::local::Db;
use surrealdb::sql::Thing;

use super::error::DatabaseError;

#[derive(Debug, Clone, Deserialize)]
pub struct EmbeddingHit {
    pub source_id: Thing,
    pub source_table: String,
    pub chunk_text: String,
    pub score: f64,
}

#[derive(Debug, Deserialize)]
struct CountRow {
    count: i64,
}

#[derive(Debug, Deserialize)]
struct HashRow {
    content_hash: String,
}

#[tracing::instrument(skip_all, fields(source_id = %source_id, chunk_index))]
pub async fn upsert_embedding(
    db: &Surreal<Db>,
    source_table: &str,
    source_id: &Thing,
    chunk_index: usize,
    chunk_text: &str,
    content_hash: &str,
    vector: &[f32],
) -> Result<(), DatabaseError> {
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
    .bind(("vector", vector.to_vec()))
    .await
    .map_err(|source| DatabaseError::Query {
        table: "embedding",
        operation: "upsert",
        source,
    })?;

    Ok(())
}

#[tracing::instrument(skip_all, fields(source_id = %source_id))]
pub async fn get_content_hash(
    db: &Surreal<Db>,
    source_id: &Thing,
) -> Result<Option<String>, DatabaseError> {
    let mut response = db
        .query(
            "SELECT content_hash FROM embedding
             WHERE source_id = $source_id
             LIMIT 1",
        )
        .bind(("source_id", source_id.clone()))
        .await
        .map_err(|source| DatabaseError::Query {
            table: "embedding",
            operation: "get_content_hash",
            source,
        })?;

    let rows: Vec<HashRow> = response.take(0).map_err(|source| DatabaseError::Query {
        table: "embedding",
        operation: "get_content_hash_take",
        source,
    })?;

    Ok(rows.first().map(|r| r.content_hash.clone()))
}

#[tracing::instrument(skip_all, fields(source_id = %source_id))]
pub async fn delete_embeddings_for_source(
    db: &Surreal<Db>,
    source_id: &Thing,
) -> Result<(), DatabaseError> {
    db.query("DELETE FROM embedding WHERE source_id = $source_id")
        .bind(("source_id", source_id.clone()))
        .await
        .map_err(|source| DatabaseError::Query {
            table: "embedding",
            operation: "delete_for_source",
            source,
        })?;

    Ok(())
}

#[tracing::instrument(skip_all)]
pub async fn delete_all_embeddings(db: &Surreal<Db>) -> Result<(), DatabaseError> {
    db.query("DELETE FROM embedding")
        .await
        .map_err(|source| DatabaseError::Query {
            table: "embedding",
            operation: "delete_all",
            source,
        })?;

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
         WHERE vector <|{limit}|> $query_vector
         ORDER BY score DESC"
    );
    let mut response = db
        .query(&query)
        .bind(("query_vector", query_vector.to_vec()))
        .await
        .map_err(|source| DatabaseError::Query {
            table: "embedding",
            operation: "vector_search",
            source,
        })?;

    let hits: Vec<EmbeddingHit> = response.take(0).map_err(|source| DatabaseError::Query {
        table: "embedding",
        operation: "vector_search_take",
        source,
    })?;

    Ok(hits)
}

#[tracing::instrument(skip_all)]
pub async fn count_embeddings(db: &Surreal<Db>) -> Result<i64, DatabaseError> {
    let mut response = db
        .query("SELECT count() AS count FROM embedding GROUP ALL")
        .await
        .map_err(|source| DatabaseError::Query {
            table: "embedding",
            operation: "count",
            source,
        })?;

    let rows: Vec<CountRow> = response.take(0).map_err(|source| DatabaseError::Query {
        table: "embedding",
        operation: "count_take",
        source,
    })?;

    Ok(rows.first().map(|r| r.count).unwrap_or(0))
}
