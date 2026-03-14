use sqlx::SqlitePool;

use super::error::DatabaseError;
use crate::db::{new_id, now};

#[derive(Debug, Clone)]
pub struct EmbeddingHit {
    pub source_id: String,
    pub source_table: String,
    pub chunk_text: String,
    pub topic_id: Option<String>,
    pub score: f64,
}

#[tracing::instrument(skip_all, fields(source_id = %source_id, chunk_index))]
pub async fn upsert_embedding(
    db: &SqlitePool,
    source_table: &str,
    source_id: &str,
    chunk_index: usize,
    chunk_text: &str,
    vector: &[f32],
    topic_id: Option<&str>,
) -> Result<(), DatabaseError> {
    let mut tx = db.begin().await.map_err(|source| DatabaseError::Query {
        table: "embedding",
        operation: "upsert/begin",
        source,
    })?;

    // Delete existing rows from both tables if present
    #[derive(sqlx::FromRow)]
    struct RowidRow {
        rowid: i64,
    }

    let old = sqlx::query_as::<_, RowidRow>(
        "SELECT rowid FROM embedding WHERE source_id = ? AND chunk_index = ?",
    )
    .bind(source_id)
    .bind(chunk_index as i64)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|source| DatabaseError::Query {
        table: "embedding",
        operation: "upsert/find_old",
        source,
    })?;

    if let Some(old) = old {
        sqlx::query("DELETE FROM vec_embedding WHERE rowid = ?")
            .bind(old.rowid)
            .execute(&mut *tx)
            .await
            .map_err(|source| DatabaseError::Query {
                table: "vec_embedding",
                operation: "upsert/delete_vec",
                source,
            })?;

        sqlx::query("DELETE FROM embedding WHERE rowid = ?")
            .bind(old.rowid)
            .execute(&mut *tx)
            .await
            .map_err(|source| DatabaseError::Query {
                table: "embedding",
                operation: "upsert/delete",
                source,
            })?;
    }

    // Insert new metadata row
    let id = new_id();
    sqlx::query(
        "INSERT INTO embedding \
         (id, source_table, source_id, chunk_index, chunk_text, topic_id, created_at) \
         VALUES (?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&id)
    .bind(source_table)
    .bind(source_id)
    .bind(chunk_index as i64)
    .bind(chunk_text)
    .bind(topic_id)
    .bind(now())
    .execute(&mut *tx)
    .await
    .map_err(|source| DatabaseError::Query {
        table: "embedding",
        operation: "upsert/insert",
        source,
    })?;

    // Get rowid of just-inserted row and insert vector
    let (rowid,): (i64,) = sqlx::query_as("SELECT last_insert_rowid()")
        .fetch_one(&mut *tx)
        .await
        .map_err(|source| DatabaseError::Query {
            table: "embedding",
            operation: "upsert/rowid",
            source,
        })?;

    let vec_json = serde_json::to_string(vector).unwrap_or_default();
    sqlx::query("INSERT INTO vec_embedding(rowid, embedding) VALUES (?, ?)")
        .bind(rowid)
        .bind(&vec_json)
        .execute(&mut *tx)
        .await
        .map_err(|source| DatabaseError::Query {
            table: "vec_embedding",
            operation: "upsert/insert_vec",
            source,
        })?;

    tx.commit().await.map_err(|source| DatabaseError::Query {
        table: "embedding",
        operation: "upsert/commit",
        source,
    })?;

    Ok(())
}

#[tracing::instrument(skip_all, fields(source_id = %source_id))]
pub async fn delete_embeddings_for_source(
    db: &SqlitePool,
    source_id: &str,
) -> Result<(), DatabaseError> {
    let mut tx = db.begin().await.map_err(|source| DatabaseError::Query {
        table: "embedding",
        operation: "delete_for_source/begin",
        source,
    })?;

    // Delete vec rows that match embedding rows for this source
    sqlx::query(
        "DELETE FROM vec_embedding WHERE rowid IN \
         (SELECT rowid FROM embedding WHERE source_id = ?)",
    )
    .bind(source_id)
    .execute(&mut *tx)
    .await
    .map_err(|source| DatabaseError::Query {
        table: "vec_embedding",
        operation: "delete_for_source",
        source,
    })?;

    sqlx::query("DELETE FROM embedding WHERE source_id = ?")
        .bind(source_id)
        .execute(&mut *tx)
        .await
        .map_err(|source| DatabaseError::Query {
            table: "embedding",
            operation: "delete_for_source",
            source,
        })?;

    tx.commit().await.map_err(|source| DatabaseError::Query {
        table: "embedding",
        operation: "delete_for_source/commit",
        source,
    })?;

    Ok(())
}

#[tracing::instrument(skip_all)]
pub async fn delete_all_embeddings(db: &SqlitePool) -> Result<(), DatabaseError> {
    let mut tx = db.begin().await.map_err(|source| DatabaseError::Query {
        table: "embedding",
        operation: "delete_all/begin",
        source,
    })?;

    sqlx::query("DELETE FROM vec_embedding")
        .execute(&mut *tx)
        .await
        .map_err(|source| DatabaseError::Query {
            table: "vec_embedding",
            operation: "delete_all",
            source,
        })?;

    sqlx::query("DELETE FROM embedding")
        .execute(&mut *tx)
        .await
        .map_err(|source| DatabaseError::Query {
            table: "embedding",
            operation: "delete_all",
            source,
        })?;

    tx.commit().await.map_err(|source| DatabaseError::Query {
        table: "embedding",
        operation: "delete_all/commit",
        source,
    })?;

    Ok(())
}

#[tracing::instrument(skip_all, fields(limit))]
pub async fn vector_search(
    db: &SqlitePool,
    query_vector: &[f32],
    limit: usize,
    topic_ids: &[String],
) -> Result<Vec<EmbeddingHit>, DatabaseError> {
    #[derive(sqlx::FromRow)]
    struct VecSearchRow {
        source_id: String,
        source_table: String,
        chunk_text: String,
        topic_id: Option<String>,
        distance: f64,
    }

    let vec_json = serde_json::to_string(query_vector).unwrap_or_default();

    // Over-fetch when filtering by topic to compensate for post-filtering
    let fetch_limit = if topic_ids.is_empty() {
        limit
    } else {
        limit * 3
    };

    let rows = sqlx::query_as::<_, VecSearchRow>(
        "SELECT e.source_id, e.source_table, e.chunk_text, e.topic_id, v.distance \
         FROM vec_embedding v \
         JOIN embedding e ON e.rowid = v.rowid \
         WHERE v.embedding MATCH ? AND k = ?",
    )
    .bind(&vec_json)
    .bind(fetch_limit as i64)
    .fetch_all(db)
    .await
    .map_err(|source| DatabaseError::Query {
        table: "embedding",
        operation: "vector_search",
        source,
    })?;

    let mut hits: Vec<EmbeddingHit> = rows
        .into_iter()
        .filter(|r| {
            topic_ids.is_empty()
                || topic_ids
                    .iter()
                    .any(|tid| r.topic_id.as_deref() == Some(tid.as_str()))
        })
        .map(|r| EmbeddingHit {
            source_id: r.source_id,
            source_table: r.source_table,
            chunk_text: r.chunk_text,
            topic_id: r.topic_id,
            score: 1.0 / (1.0 + r.distance),
        })
        .collect();

    hits.truncate(limit);
    Ok(hits)
}

/// Atomically replace all embeddings for a source in a single transaction.
///
/// Deletes all existing chunks for `source_id`, then inserts all new chunks.
/// Either all chunks are persisted (with the new hash) or none are.
#[tracing::instrument(skip_all, fields(source_id = %source_id, chunks = chunks.len()))]
pub async fn replace_embeddings_for_source(
    db: &SqlitePool,
    source_table: &str,
    source_id: &str,
    chunks: &[(usize, String, Vec<f32>)],
    topic_id: Option<&str>,
) -> Result<(), DatabaseError> {
    let mut tx = db.begin().await.map_err(|source| DatabaseError::Query {
        table: "embedding",
        operation: "replace/begin",
        source,
    })?;

    // Delete old vec_embedding rows
    sqlx::query(
        "DELETE FROM vec_embedding WHERE rowid IN \
         (SELECT rowid FROM embedding WHERE source_id = ?)",
    )
    .bind(source_id)
    .execute(&mut *tx)
    .await
    .map_err(|source| DatabaseError::Query {
        table: "vec_embedding",
        operation: "replace/delete_vec",
        source,
    })?;

    // Delete old embedding rows
    sqlx::query("DELETE FROM embedding WHERE source_id = ?")
        .bind(source_id)
        .execute(&mut *tx)
        .await
        .map_err(|source| DatabaseError::Query {
            table: "embedding",
            operation: "replace/delete",
            source,
        })?;

    // Insert all new chunks
    for (chunk_index, chunk_text, vector) in chunks {
        let id = new_id();
        sqlx::query(
            "INSERT INTO embedding \
             (id, source_table, source_id, chunk_index, chunk_text, topic_id, created_at) \
             VALUES (?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&id)
        .bind(source_table)
        .bind(source_id)
        .bind(*chunk_index as i64)
        .bind(chunk_text)
        .bind(topic_id)
        .bind(now())
        .execute(&mut *tx)
        .await
        .map_err(|source| DatabaseError::Query {
            table: "embedding",
            operation: "replace/insert",
            source,
        })?;

        let (rowid,): (i64,) = sqlx::query_as("SELECT last_insert_rowid()")
            .fetch_one(&mut *tx)
            .await
            .map_err(|source| DatabaseError::Query {
                table: "embedding",
                operation: "replace/rowid",
                source,
            })?;

        let vec_json = serde_json::to_string(vector).unwrap_or_default();
        sqlx::query("INSERT INTO vec_embedding(rowid, embedding) VALUES (?, ?)")
            .bind(rowid)
            .bind(&vec_json)
            .execute(&mut *tx)
            .await
            .map_err(|source| DatabaseError::Query {
                table: "vec_embedding",
                operation: "replace/insert_vec",
                source,
            })?;
    }

    tx.commit().await.map_err(|source| DatabaseError::Query {
        table: "embedding",
        operation: "replace/commit",
        source,
    })?;

    Ok(())
}

#[tracing::instrument(skip_all)]
pub async fn count_embeddings(db: &SqlitePool) -> Result<i64, DatabaseError> {
    let (count,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM embedding")
        .fetch_one(db)
        .await
        .map_err(|source| DatabaseError::Query {
            table: "embedding",
            operation: "count",
            source,
        })?;
    Ok(count)
}
