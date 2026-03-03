use sqlx::SqlitePool;

use crate::db::error::DatabaseError;
use crate::db::{new_id, now};

use super::records::TopicRecord;

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct TopicInfo {
    pub id: String,
    pub name: String,
    pub ref_count: i64,
}

#[allow(clippy::too_many_arguments)]
#[tracing::instrument(skip_all, level = "debug", fields(name = %name))]
pub async fn create_topic(
    db: &SqlitePool,
    name: &str,
    note_id: Option<&str>,
    source_url: Option<&str>,
    version_ref: Option<&str>,
    fetched_at: Option<&str>,
) -> Result<String, DatabaseError> {
    let id = new_id();
    let ts = now();

    sqlx::query(
        "INSERT INTO topic \
         (id, name, note_id, source_url, version_ref, fetched_at, created_at, updated_at) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&id)
    .bind(name)
    .bind(note_id)
    .bind(source_url)
    .bind(version_ref)
    .bind(fetched_at)
    .bind(&ts)
    .bind(&ts)
    .execute(db)
    .await
    .map_err(|source| DatabaseError::Query {
        table: "topic",
        operation: "create",
        source,
    })?;

    Ok(id)
}

#[tracing::instrument(skip_all, level = "debug", fields(topic_id = %topic_id))]
pub async fn get_topic(db: &SqlitePool, topic_id: &str) -> Result<TopicRecord, DatabaseError> {
    sqlx::query_as::<_, TopicRecord>("SELECT * FROM topic WHERE id = ?")
        .bind(topic_id)
        .fetch_optional(db)
        .await
        .map_err(|source| DatabaseError::Query {
            table: "topic",
            operation: "get",
            source,
        })?
        .ok_or(DatabaseError::MissingRow {
            table: "topic",
            operation: "get",
        })
}

#[tracing::instrument(skip_all, level = "debug", fields(name = %name))]
pub async fn find_topic_by_name(
    db: &SqlitePool,
    name: &str,
) -> Result<Option<TopicRecord>, DatabaseError> {
    sqlx::query_as::<_, TopicRecord>("SELECT * FROM topic WHERE name = ? LIMIT 1")
        .bind(name)
        .fetch_optional(db)
        .await
        .map_err(|source| DatabaseError::Query {
            table: "topic",
            operation: "find_by_name",
            source,
        })
}

/// Find a topic by exact name match, or create it if it doesn't exist.
/// Returns the topic ID.
#[tracing::instrument(skip_all, level = "debug", fields(name = %name))]
pub async fn find_or_create_topic(db: &SqlitePool, name: &str) -> Result<String, DatabaseError> {
    if let Some(existing) = find_topic_by_name(db, name).await? {
        return Ok(existing.id);
    }
    create_topic(db, name, None, None, None, None).await
}

/// Find all topics matching a prefix: exact name match OR name starts with
/// `prefix/`. For example, prefix="dioxus" matches "dioxus" and "dioxus/docs".
#[tracing::instrument(skip_all, level = "debug", fields(prefix = %prefix))]
pub async fn find_topics_by_prefix(
    db: &SqlitePool,
    prefix: &str,
) -> Result<Vec<TopicRecord>, DatabaseError> {
    sqlx::query_as::<_, TopicRecord>(
        "SELECT * FROM topic WHERE name = ? OR name LIKE ? || '/%' ORDER BY name",
    )
    .bind(prefix)
    .bind(prefix)
    .fetch_all(db)
    .await
    .map_err(|source| DatabaseError::Query {
        table: "topic",
        operation: "find_by_prefix",
        source,
    })
}

#[allow(clippy::too_many_arguments)]
#[tracing::instrument(skip_all, level = "debug", fields(topic_id = %topic_id))]
pub async fn update_topic(
    db: &SqlitePool,
    topic_id: &str,
    note_id: Option<&str>,
    source_url: Option<&str>,
    version_ref: Option<&str>,
    fetched_at: Option<&str>,
) -> Result<(), DatabaseError> {
    sqlx::query(
        "UPDATE topic SET \
         note_id = COALESCE(?, note_id), \
         source_url = COALESCE(?, source_url), \
         version_ref = COALESCE(?, version_ref), \
         fetched_at = COALESCE(?, fetched_at), \
         updated_at = ? \
         WHERE id = ?",
    )
    .bind(note_id)
    .bind(source_url)
    .bind(version_ref)
    .bind(fetched_at)
    .bind(now())
    .bind(topic_id)
    .execute(db)
    .await
    .map_err(|source| DatabaseError::Query {
        table: "topic",
        operation: "update",
        source,
    })?;
    Ok(())
}

/// Delete a topic and cascade: delete all its references, their embeddings,
/// and the topic row itself.
#[tracing::instrument(skip_all, level = "debug", fields(topic_id = %topic_id))]
pub async fn delete_topic(db: &SqlitePool, topic_id: &str) -> Result<(), DatabaseError> {
    delete_references_by_topic(db, topic_id).await?;

    sqlx::query("DELETE FROM topic WHERE id = ?")
        .bind(topic_id)
        .execute(db)
        .await
        .map_err(|source| DatabaseError::Query {
            table: "topic",
            operation: "delete",
            source,
        })?;
    Ok(())
}

/// List all topics with their reference counts.
#[tracing::instrument(skip_all, level = "debug")]
pub async fn list_topics(db: &SqlitePool) -> Result<Vec<TopicInfo>, DatabaseError> {
    sqlx::query_as::<_, TopicInfo>(
        "SELECT t.id, t.name, COUNT(r.id) AS ref_count \
         FROM topic t \
         LEFT JOIN reference r ON r.topic_id = t.id \
         GROUP BY t.id \
         ORDER BY t.name",
    )
    .fetch_all(db)
    .await
    .map_err(|source| DatabaseError::Query {
        table: "topic",
        operation: "list",
        source,
    })
}

#[tracing::instrument(skip_all, level = "debug", fields(topic_id = %topic_id))]
pub async fn count_references_by_topic(
    db: &SqlitePool,
    topic_id: &str,
) -> Result<i64, DatabaseError> {
    let (count,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM reference WHERE topic_id = ?")
        .bind(topic_id)
        .fetch_one(db)
        .await
        .map_err(|source| DatabaseError::Query {
            table: "reference",
            operation: "count_by_topic",
            source,
        })?;
    Ok(count)
}

/// Delete all references under a topic, including their embeddings and
/// cited edges.
#[tracing::instrument(skip_all, level = "debug", fields(topic_id = %topic_id))]
pub async fn delete_references_by_topic(
    db: &SqlitePool,
    topic_id: &str,
) -> Result<(), DatabaseError> {
    // Delete embeddings for all references in this topic
    sqlx::query(
        "DELETE FROM vec_embedding WHERE rowid IN \
         (SELECT e.rowid FROM embedding e \
          JOIN reference r ON r.id = e.source_id \
          WHERE r.topic_id = ? AND e.source_table = 'reference')",
    )
    .bind(topic_id)
    .execute(db)
    .await
    .map_err(|source| DatabaseError::Query {
        table: "vec_embedding",
        operation: "delete_by_topic",
        source,
    })?;

    sqlx::query(
        "DELETE FROM embedding WHERE source_table = 'reference' AND source_id IN \
         (SELECT id FROM reference WHERE topic_id = ?)",
    )
    .bind(topic_id)
    .execute(db)
    .await
    .map_err(|source| DatabaseError::Query {
        table: "embedding",
        operation: "delete_by_topic",
        source,
    })?;

    // Delete cited edges pointing to these references
    sqlx::query(
        "DELETE FROM cited WHERE to_id IN \
         (SELECT id FROM reference WHERE topic_id = ?)",
    )
    .bind(topic_id)
    .execute(db)
    .await
    .map_err(|source| DatabaseError::Query {
        table: "cited",
        operation: "delete_by_topic",
        source,
    })?;

    // Delete the references themselves
    sqlx::query("DELETE FROM reference WHERE topic_id = ?")
        .bind(topic_id)
        .execute(db)
        .await
        .map_err(|source| DatabaseError::Query {
            table: "reference",
            operation: "delete_by_topic",
            source,
        })?;

    Ok(())
}
