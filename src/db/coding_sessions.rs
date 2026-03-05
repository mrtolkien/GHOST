use sqlx::SqlitePool;

use crate::db::DatabaseError;

pub async fn create_coding_session(
    db: &SqlitePool,
    id: &str,
    session_id: &str,
    channel_id: Option<&str>,
    working_dir: &str,
) -> Result<(), DatabaseError> {
    let now = chrono::Utc::now().to_rfc3339();
    sqlx::query(
        "INSERT INTO coding_sessions \
         (id, session_id, channel_id, working_dir, status, started_at)
         VALUES (?, ?, ?, ?, 'active', ?)",
    )
    .bind(id)
    .bind(session_id)
    .bind(channel_id)
    .bind(working_dir)
    .bind(&now)
    .execute(db)
    .await
    .map_err(|source| DatabaseError::Query {
        table: "coding_sessions",
        operation: "create",
        source,
    })?;
    Ok(())
}

/// Returns `(coding_session_id, session_id, working_dir)` for the active
/// takeover on a channel, if any.
pub async fn get_active_takeover(
    db: &SqlitePool,
    channel_id: &str,
) -> Result<Option<(String, String, String)>, DatabaseError> {
    sqlx::query_as::<_, (String, String, String)>(
        "SELECT id, session_id, working_dir FROM coding_sessions
         WHERE channel_id = ? AND status = 'active'
         LIMIT 1",
    )
    .bind(channel_id)
    .fetch_optional(db)
    .await
    .map_err(|source| DatabaseError::Query {
        table: "coding_sessions",
        operation: "get_active_takeover",
        source,
    })
}

pub async fn end_coding_session(db: &SqlitePool, id: &str) -> Result<(), DatabaseError> {
    let now = chrono::Utc::now().to_rfc3339();
    sqlx::query("UPDATE coding_sessions SET status = 'ended', ended_at = ? WHERE id = ?")
        .bind(&now)
        .bind(id)
        .execute(db)
        .await
        .map_err(|source| DatabaseError::Query {
            table: "coding_sessions",
            operation: "end",
            source,
        })?;
    Ok(())
}

/// Returns `(session_id, working_dir, status)`.
pub async fn get_coding_session(
    db: &SqlitePool,
    id: &str,
) -> Result<Option<(String, String, String)>, DatabaseError> {
    sqlx::query_as::<_, (String, String, String)>(
        "SELECT session_id, working_dir, status FROM coding_sessions WHERE id = ?",
    )
    .bind(id)
    .fetch_optional(db)
    .await
    .map_err(|source| DatabaseError::Query {
        table: "coding_sessions",
        operation: "get",
        source,
    })
}

pub async fn reactivate_coding_session(
    db: &SqlitePool,
    id: &str,
    channel_id: Option<&str>,
) -> Result<(), DatabaseError> {
    sqlx::query(
        "UPDATE coding_sessions \
         SET status = 'active', channel_id = ?, ended_at = NULL \
         WHERE id = ?",
    )
    .bind(channel_id)
    .bind(id)
    .execute(db)
    .await
    .map_err(|source| DatabaseError::Query {
        table: "coding_sessions",
        operation: "reactivate",
        source,
    })?;
    Ok(())
}

/// Returns `(id, session_id, working_dir, status, started_at)`.
pub async fn list_recent_coding_sessions(
    db: &SqlitePool,
    limit: u32,
) -> Result<Vec<(String, String, String, String, String)>, DatabaseError> {
    sqlx::query_as::<_, (String, String, String, String, String)>(
        "SELECT id, session_id, working_dir, status, started_at
         FROM coding_sessions
         ORDER BY started_at DESC
         LIMIT ?",
    )
    .bind(limit)
    .fetch_all(db)
    .await
    .map_err(|source| DatabaseError::Query {
        table: "coding_sessions",
        operation: "list_recent",
        source,
    })
}
