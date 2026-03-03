use sqlx::SqlitePool;

use crate::db::error::DatabaseError;
use crate::db::{new_id, now};

use super::records::ImportBatchRecord;

/// Upsert an import batch for a topic. One batch per topic — re-import
/// replaces the existing batch metadata.
#[tracing::instrument(skip_all, level = "debug", fields(topic_id = %topic_id))]
pub async fn upsert_import_batch(
    db: &SqlitePool,
    topic_id: &str,
    source_type: &str,
    source_url: &str,
    version_ref: Option<&str>,
    ref_count: i64,
) -> Result<String, DatabaseError> {
    let id = new_id();
    let ts = now();

    // Use INSERT ... ON CONFLICT to upsert
    let result = sqlx::query_as::<_, (String,)>(
        "INSERT INTO import_batch \
         (id, topic_id, source_type, source_url, version_ref, ref_count, created_at, updated_at) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?) \
         ON CONFLICT(topic_id) DO UPDATE SET \
         source_type = excluded.source_type, \
         source_url = excluded.source_url, \
         version_ref = excluded.version_ref, \
         ref_count = excluded.ref_count, \
         updated_at = excluded.updated_at \
         RETURNING id",
    )
    .bind(&id)
    .bind(topic_id)
    .bind(source_type)
    .bind(source_url)
    .bind(version_ref)
    .bind(ref_count)
    .bind(&ts)
    .bind(&ts)
    .fetch_one(db)
    .await
    .map_err(|source| DatabaseError::Query {
        table: "import_batch",
        operation: "upsert",
        source,
    })?;

    Ok(result.0)
}

#[tracing::instrument(skip_all, level = "debug", fields(topic_id = %topic_id))]
pub async fn get_import_batch_by_topic(
    db: &SqlitePool,
    topic_id: &str,
) -> Result<Option<ImportBatchRecord>, DatabaseError> {
    sqlx::query_as::<_, ImportBatchRecord>("SELECT * FROM import_batch WHERE topic_id = ? LIMIT 1")
        .bind(topic_id)
        .fetch_optional(db)
        .await
        .map_err(|source| DatabaseError::Query {
            table: "import_batch",
            operation: "get_by_topic",
            source,
        })
}

#[tracing::instrument(skip_all, level = "debug")]
pub async fn list_import_batches(db: &SqlitePool) -> Result<Vec<ImportBatchRecord>, DatabaseError> {
    sqlx::query_as::<_, ImportBatchRecord>("SELECT * FROM import_batch ORDER BY updated_at DESC")
        .fetch_all(db)
        .await
        .map_err(|source| DatabaseError::Query {
            table: "import_batch",
            operation: "list",
            source,
        })
}

#[tracing::instrument(skip_all, level = "debug", fields(topic_id = %topic_id))]
pub async fn delete_import_batch(db: &SqlitePool, topic_id: &str) -> Result<(), DatabaseError> {
    sqlx::query("DELETE FROM import_batch WHERE topic_id = ?")
        .bind(topic_id)
        .execute(db)
        .await
        .map_err(|source| DatabaseError::Query {
            table: "import_batch",
            operation: "delete",
            source,
        })?;
    Ok(())
}
