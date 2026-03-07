use sqlx::SqlitePool;

use crate::db::error::DatabaseError;
use crate::db::{new_id, now};

use super::records::{EdgeRecord, NoteRecord};

#[tracing::instrument(skip_all, level = "debug", fields(from = %from, to = %to, label = %label))]
pub async fn create_edge(
    db: &SqlitePool,
    from: &str,
    to: &str,
    label: &str,
) -> Result<String, DatabaseError> {
    let id = new_id();

    sqlx::query(
        "INSERT INTO relates_to (id, from_id, to_id, label, created_at) \
         VALUES (?, ?, ?, ?, ?)",
    )
    .bind(&id)
    .bind(from)
    .bind(to)
    .bind(label)
    .bind(now())
    .execute(db)
    .await
    .map_err(|source| DatabaseError::Query {
        table: "relates_to",
        operation: "create_edge",
        source,
    })?;

    Ok(id)
}

#[tracing::instrument(skip_all, level = "debug", fields(from = %from))]
pub async fn related_note_ids(db: &SqlitePool, from: &str) -> Result<Vec<String>, DatabaseError> {
    #[derive(sqlx::FromRow)]
    struct OutRow {
        to_id: String,
    }

    let rows = sqlx::query_as::<_, OutRow>("SELECT to_id FROM relates_to WHERE from_id = ?")
        .bind(from)
        .fetch_all(db)
        .await
        .map_err(|source| DatabaseError::Query {
            table: "relates_to",
            operation: "related_note_ids",
            source,
        })?;

    Ok(rows.into_iter().map(|r| r.to_id).collect())
}

#[tracing::instrument(skip_all, level = "debug", fields(note_id = %note_id))]
pub async fn outgoing_edges(
    db: &SqlitePool,
    note_id: &str,
) -> Result<Vec<EdgeRecord>, DatabaseError> {
    sqlx::query_as::<_, EdgeRecord>("SELECT * FROM relates_to WHERE from_id = ?")
        .bind(note_id)
        .fetch_all(db)
        .await
        .map_err(|source| DatabaseError::Query {
            table: "relates_to",
            operation: "outgoing_edges",
            source,
        })
}

#[tracing::instrument(skip_all, level = "debug", fields(note_id = %note_id))]
pub async fn incoming_edges(
    db: &SqlitePool,
    note_id: &str,
) -> Result<Vec<EdgeRecord>, DatabaseError> {
    sqlx::query_as::<_, EdgeRecord>("SELECT * FROM relates_to WHERE to_id = ?")
        .bind(note_id)
        .fetch_all(db)
        .await
        .map_err(|source| DatabaseError::Query {
            table: "relates_to",
            operation: "incoming_edges",
            source,
        })
}

#[tracing::instrument(skip_all, level = "debug", fields(note_id = %note_id))]
pub async fn incoming_cited(db: &SqlitePool, note_id: &str) -> Result<Vec<String>, DatabaseError> {
    #[derive(sqlx::FromRow)]
    struct InRow {
        from_id: String,
    }

    let rows = sqlx::query_as::<_, InRow>("SELECT from_id FROM cited WHERE to_id = ?")
        .bind(note_id)
        .fetch_all(db)
        .await
        .map_err(|source| DatabaseError::Query {
            table: "cited",
            operation: "incoming_cited",
            source,
        })?;

    Ok(rows.into_iter().map(|r| r.from_id).collect())
}

#[tracing::instrument(skip_all, level = "debug", fields(from = %from, to = %to))]
pub async fn create_cited_edge(
    db: &SqlitePool,
    from: &str,
    to: &str,
) -> Result<String, DatabaseError> {
    let id = new_id();

    sqlx::query("INSERT INTO cited (id, from_id, to_id, created_at) VALUES (?, ?, ?, ?)")
        .bind(&id)
        .bind(from)
        .bind(to)
        .bind(now())
        .execute(db)
        .await
        .map_err(|source| DatabaseError::Query {
            table: "cited",
            operation: "create_cited_edge",
            source,
        })?;

    Ok(id)
}

#[tracing::instrument(skip_all, level = "debug", fields(note_id = %note_id))]
pub async fn delete_outgoing_edges(db: &SqlitePool, note_id: &str) -> Result<(), DatabaseError> {
    sqlx::query("DELETE FROM relates_to WHERE from_id = ?")
        .bind(note_id)
        .execute(db)
        .await
        .map_err(|source| DatabaseError::Query {
            table: "relates_to",
            operation: "delete_outgoing",
            source,
        })?;
    Ok(())
}

#[tracing::instrument(skip_all, level = "debug")]
#[tracing::instrument(skip_all, level = "debug", fields(message_id = %message_id, url = %url))]
pub async fn create_message_source(
    db: &SqlitePool,
    message_id: &str,
    url: &str,
    title: Option<&str>,
) -> Result<String, DatabaseError> {
    let id = new_id();
    sqlx::query(
        "INSERT INTO message_source (id, message_id, url, title, created_at) \
         VALUES (?, ?, ?, ?, ?)",
    )
    .bind(&id)
    .bind(message_id)
    .bind(url)
    .bind(title)
    .bind(now())
    .execute(db)
    .await
    .map_err(|source| DatabaseError::Query {
        table: "message_source",
        operation: "create_message_source",
        source,
    })?;
    Ok(id)
}

/// Backfill `reference_id` on message_source rows that match the given URL.
/// Called during reflection/curation after reference records are created.
#[tracing::instrument(skip_all, level = "debug", fields(url = %url, reference_id = %reference_id))]
pub async fn backfill_message_source_references(
    db: &SqlitePool,
    url: &str,
    reference_id: &str,
) -> Result<u64, DatabaseError> {
    let result = sqlx::query(
        "UPDATE message_source SET reference_id = ? \
         WHERE url = ? AND reference_id IS NULL",
    )
    .bind(reference_id)
    .bind(url)
    .execute(db)
    .await
    .map_err(|source| DatabaseError::Query {
        table: "message_source",
        operation: "backfill_message_source_references",
        source,
    })?;
    Ok(result.rows_affected())
}

#[tracing::instrument(skip_all, level = "debug")]
pub async fn orphan_notes(db: &SqlitePool) -> Result<Vec<NoteRecord>, DatabaseError> {
    sqlx::query_as::<_, NoteRecord>(
        "SELECT * FROM note \
         WHERE NOT EXISTS (SELECT 1 FROM relates_to WHERE from_id = note.id) \
         AND NOT EXISTS (SELECT 1 FROM relates_to WHERE to_id = note.id)",
    )
    .fetch_all(db)
    .await
    .map_err(|source| DatabaseError::Query {
        table: "note",
        operation: "orphan_notes",
        source,
    })
}
