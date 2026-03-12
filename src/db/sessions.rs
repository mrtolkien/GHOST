use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;

use crate::db::error::DatabaseError;
use crate::db::{new_id, now};
use crate::tools::TodoItem;

#[derive(Debug, Clone, Deserialize, Serialize, sqlx::FromRow)]
pub struct SessionRecord {
    pub id: String,
    pub created_at: String,
    pub updated_at: String,
    pub last_activity_at: String,
    pub status: String,
    pub compaction_summary: Option<String>,
    pub compaction_cursor_id: Option<String>,
    pub todo_list: Option<String>, // JSON
}

#[derive(Debug, Clone, Deserialize, Serialize, sqlx::FromRow)]
pub struct MessageRecord {
    pub id: String,
    pub session_id: String,
    pub role: String,
    pub content: String,
    pub tool_calls: Option<String>,   // JSON
    pub tool_results: Option<String>, // JSON
    pub raw_output: Option<String>,   // JSON
    pub images: Option<String>,       // JSON
    pub created_at: String,
}

impl MessageRecord {
    pub fn tool_calls_parsed(&self) -> Option<Vec<serde_json::Value>> {
        self.tool_calls
            .as_deref()
            .and_then(|s| serde_json::from_str(s).ok())
    }

    pub fn tool_results_parsed(&self) -> Option<Vec<serde_json::Value>> {
        self.tool_results
            .as_deref()
            .and_then(|s| serde_json::from_str(s).ok())
    }

    pub fn raw_output_parsed(&self) -> Option<Vec<serde_json::Value>> {
        self.raw_output
            .as_deref()
            .and_then(|s| serde_json::from_str(s).ok())
    }

    pub fn images_parsed(&self) -> Option<Vec<serde_json::Value>> {
        self.images
            .as_deref()
            .and_then(|s| serde_json::from_str(s).ok())
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, sqlx::FromRow)]
pub struct SessionListRecord {
    pub id: String,
    pub last_activity_at: String,
    pub status: String,
}

#[tracing::instrument(skip_all, level = "debug")]
pub async fn create_session(db: &SqlitePool) -> Result<String, DatabaseError> {
    let id = new_id();
    let ts = now();

    sqlx::query(
        "INSERT INTO session (id, created_at, updated_at, last_activity_at, status) \
         VALUES (?, ?, ?, ?, 'active')",
    )
    .bind(&id)
    .bind(&ts)
    .bind(&ts)
    .bind(&ts)
    .execute(db)
    .await
    .map_err(|source| DatabaseError::Query {
        table: "session",
        operation: "create",
        source,
    })?;

    Ok(id)
}

#[tracing::instrument(skip_all, level = "debug")]
pub async fn create_agent_session(db: &SqlitePool) -> Result<String, DatabaseError> {
    let id = new_id();
    let ts = now();

    sqlx::query(
        "INSERT INTO session (id, created_at, updated_at, last_activity_at, status) \
         VALUES (?, ?, ?, ?, 'agent')",
    )
    .bind(&id)
    .bind(&ts)
    .bind(&ts)
    .bind(&ts)
    .execute(db)
    .await
    .map_err(|source| DatabaseError::Query {
        table: "session",
        operation: "create_agent",
        source,
    })?;

    Ok(id)
}

#[tracing::instrument(skip_all, level = "debug", fields(session_id = %session_id))]
pub async fn get_session(
    db: &SqlitePool,
    session_id: &str,
) -> Result<SessionRecord, DatabaseError> {
    sqlx::query_as::<_, SessionRecord>("SELECT * FROM session WHERE id = ?")
        .bind(session_id)
        .fetch_optional(db)
        .await
        .map_err(|source| DatabaseError::Query {
            table: "session",
            operation: "get",
            source,
        })?
        .ok_or(DatabaseError::MissingRow {
            table: "session",
            operation: "get",
        })
}

#[tracing::instrument(skip_all, level = "debug", fields(session_id = %session_id))]
pub async fn mark_rebooted(db: &SqlitePool, session_id: &str) -> Result<(), DatabaseError> {
    sqlx::query("UPDATE session SET status = 'rebooted', updated_at = ? WHERE id = ?")
        .bind(now())
        .bind(session_id)
        .execute(db)
        .await
        .map_err(|source| DatabaseError::Query {
            table: "session",
            operation: "mark_rebooted",
            source,
        })?;
    Ok(())
}

#[tracing::instrument(skip_all, level = "debug", fields(session_id = %session_id))]
pub async fn update_compaction(
    db: &SqlitePool,
    session_id: &str,
    summary: &str,
    cursor_id: &str,
) -> Result<(), DatabaseError> {
    sqlx::query(
        "UPDATE session SET compaction_summary = ?, compaction_cursor_id = ?, updated_at = ? \
         WHERE id = ?",
    )
    .bind(summary)
    .bind(cursor_id)
    .bind(now())
    .bind(session_id)
    .execute(db)
    .await
    .map_err(|source| DatabaseError::Query {
        table: "session",
        operation: "update_compaction",
        source,
    })?;
    Ok(())
}

#[tracing::instrument(skip_all, level = "debug", fields(session_id = %session_id))]
pub async fn update_activity(db: &SqlitePool, session_id: &str) -> Result<(), DatabaseError> {
    let ts = now();
    sqlx::query("UPDATE session SET updated_at = ?, last_activity_at = ? WHERE id = ?")
        .bind(&ts)
        .bind(&ts)
        .bind(session_id)
        .execute(db)
        .await
        .map_err(|source| DatabaseError::Query {
            table: "session",
            operation: "update_activity",
            source,
        })?;
    Ok(())
}

#[tracing::instrument(skip_all, level = "debug", fields(session_id = %session_id))]
pub async fn get_session_todo_list(
    db: &SqlitePool,
    session_id: &str,
) -> Result<Option<Vec<TodoItem>>, DatabaseError> {
    #[derive(sqlx::FromRow)]
    struct TodoRow {
        todo_list: Option<String>,
    }

    let row = sqlx::query_as::<_, TodoRow>("SELECT todo_list FROM session WHERE id = ?")
        .bind(session_id)
        .fetch_optional(db)
        .await
        .map_err(|source| DatabaseError::Query {
            table: "session",
            operation: "get_todo_list",
            source,
        })?
        .ok_or(DatabaseError::MissingRow {
            table: "session",
            operation: "get_todo_list",
        })?;

    match row.todo_list {
        Some(json) => {
            let items: Vec<TodoItem> =
                serde_json::from_str(&json).map_err(|e| DatabaseError::Query {
                    table: "session",
                    operation: "get_todo_list/deserialize",
                    source: sqlx::Error::Protocol(e.to_string()),
                })?;
            Ok(Some(items))
        }
        None => Ok(None),
    }
}

#[tracing::instrument(skip_all, level = "debug", fields(session_id = %session_id))]
pub async fn set_session_todo_list(
    db: &SqlitePool,
    session_id: &str,
    todo_list: Option<&[TodoItem]>,
) -> Result<(), DatabaseError> {
    let json = todo_list.map(|items| serde_json::to_string(items).unwrap_or_default());

    sqlx::query("UPDATE session SET todo_list = ?, updated_at = ? WHERE id = ?")
        .bind(json)
        .bind(now())
        .bind(session_id)
        .execute(db)
        .await
        .map_err(|source| DatabaseError::Query {
            table: "session",
            operation: "set_todo_list",
            source,
        })?;
    Ok(())
}

#[tracing::instrument(skip_all, level = "debug", fields(session_id = %session_id, role = %role))]
pub async fn create_message(
    db: &SqlitePool,
    session_id: &str,
    role: &str,
    content: &str,
) -> Result<String, DatabaseError> {
    create_message_with_metadata(db, session_id, role, content, None, None, None, None).await
}

#[allow(clippy::too_many_arguments)]
#[tracing::instrument(skip_all, level = "debug", fields(session_id = %session_id, role = %role))]
pub async fn create_message_with_metadata(
    db: &SqlitePool,
    session_id: &str,
    role: &str,
    content: &str,
    tool_calls: Option<Vec<serde_json::Value>>,
    tool_results: Option<Vec<serde_json::Value>>,
    raw_output: Option<Vec<serde_json::Value>>,
    images: Option<Vec<serde_json::Value>>,
) -> Result<String, DatabaseError> {
    let id = new_id();

    sqlx::query(
        "INSERT INTO message (id, session_id, role, content, tool_calls, tool_results, raw_output, images, created_at) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&id)
    .bind(session_id)
    .bind(role)
    .bind(content)
    .bind(tool_calls.as_ref().map(|v| serde_json::to_string(v).unwrap_or_default()))
    .bind(tool_results.as_ref().map(|v| serde_json::to_string(v).unwrap_or_default()))
    .bind(raw_output.as_ref().map(|v| serde_json::to_string(v).unwrap_or_default()))
    .bind(images.as_ref().map(|v| serde_json::to_string(v).unwrap_or_default()))
    .bind(now())
    .execute(db)
    .await
    .map_err(|source| DatabaseError::Query {
        table: "message",
        operation: "create",
        source,
    })?;

    Ok(id)
}

#[tracing::instrument(skip_all, level = "debug", fields(session_id = %session_id))]
pub async fn list_messages_by_session(
    db: &SqlitePool,
    session_id: &str,
) -> Result<Vec<MessageRecord>, DatabaseError> {
    sqlx::query_as::<_, MessageRecord>(
        "SELECT * FROM message WHERE session_id = ? ORDER BY created_at ASC",
    )
    .bind(session_id)
    .fetch_all(db)
    .await
    .map_err(|source| DatabaseError::Query {
        table: "message",
        operation: "list_by_session",
        source,
    })
}

/// Get the most recent message in a session, or `None` if the session is empty.
#[tracing::instrument(skip_all, level = "debug", fields(session_id = %session_id))]
pub async fn get_last_message(
    db: &SqlitePool,
    session_id: &str,
) -> Result<Option<MessageRecord>, DatabaseError> {
    sqlx::query_as::<_, MessageRecord>(
        "SELECT * FROM message WHERE session_id = ? ORDER BY created_at DESC LIMIT 1",
    )
    .bind(session_id)
    .fetch_optional(db)
    .await
    .map_err(|source| DatabaseError::Query {
        table: "message",
        operation: "get_last",
        source,
    })
}

#[tracing::instrument(skip_all, level = "debug", fields(session_id = %session_id, n = n))]
pub async fn get_last_n_messages(
    db: &SqlitePool,
    session_id: &str,
    n: usize,
) -> Result<Vec<MessageRecord>, DatabaseError> {
    sqlx::query_as::<_, MessageRecord>(
        "SELECT * FROM (
             SELECT * FROM message WHERE session_id = ? ORDER BY created_at DESC LIMIT ?
         ) ORDER BY created_at ASC",
    )
    .bind(session_id)
    .bind(n as i64)
    .fetch_all(db)
    .await
    .map_err(|source| DatabaseError::Query {
        table: "message",
        operation: "get_last_n",
        source,
    })
}

#[tracing::instrument(skip_all, level = "debug", fields(limit = limit))]
pub async fn list_recent_sessions(
    db: &SqlitePool,
    limit: usize,
) -> Result<Vec<SessionListRecord>, DatabaseError> {
    sqlx::query_as::<_, SessionListRecord>(
        "SELECT id, last_activity_at, status FROM session \
         ORDER BY last_activity_at DESC LIMIT ?",
    )
    .bind(limit as i64)
    .fetch_all(db)
    .await
    .map_err(|source| DatabaseError::Query {
        table: "session",
        operation: "list_recent",
        source,
    })
}

#[tracing::instrument(skip_all, level = "debug", fields(session_id = %session_id))]
pub async fn count_messages_for_session(
    db: &SqlitePool,
    session_id: &str,
) -> Result<usize, DatabaseError> {
    let count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM message WHERE session_id = ?")
        .bind(session_id)
        .fetch_one(db)
        .await
        .map_err(|source| DatabaseError::Query {
            table: "message",
            operation: "count_for_session",
            source,
        })?;
    Ok(count.0.max(0) as usize)
}

#[tracing::instrument(skip_all, level = "debug", fields(session_id = %session_id))]
pub async fn count_messages_since(
    db: &SqlitePool,
    session_id: &str,
    since: &chrono::DateTime<chrono::Utc>,
) -> Result<usize, DatabaseError> {
    let since_str = since.to_rfc3339();
    let count: (i64,) =
        sqlx::query_as("SELECT COUNT(*) FROM message WHERE session_id = ? AND created_at > ?")
            .bind(session_id)
            .bind(&since_str)
            .fetch_one(db)
            .await
            .map_err(|source| DatabaseError::Query {
                table: "message",
                operation: "count_messages_since",
                source,
            })?;
    Ok(count.0.max(0) as usize)
}

/// Fetch all message IDs for a session, ordered by creation time.
///
/// Used by `compact_in_tool_loop` to obtain the `stored_message_ids` vector
/// needed for Phase 2 summarization without loading full message content.
#[tracing::instrument(skip_all, level = "debug", fields(session_id = %session_id))]
pub async fn get_session_message_ids(
    db: &SqlitePool,
    session_id: &str,
) -> Result<Vec<String>, DatabaseError> {
    sqlx::query_scalar("SELECT id FROM message WHERE session_id = ? ORDER BY created_at ASC")
        .bind(session_id)
        .fetch_all(db)
        .await
        .map_err(|source| DatabaseError::Query {
            table: "message",
            operation: "get_session_message_ids",
            source,
        })
}

#[tracing::instrument(skip_all, level = "debug", fields(session_id = %session_id))]
pub async fn get_interface_for_session(
    db: &SqlitePool,
    session_id: &str,
) -> Result<Option<String>, DatabaseError> {
    #[derive(sqlx::FromRow)]
    struct InterfaceRow {
        interface: String,
    }

    let row = sqlx::query_as::<_, InterfaceRow>(
        "SELECT interface FROM interface_session WHERE session_id = ? LIMIT 1",
    )
    .bind(session_id)
    .fetch_optional(db)
    .await
    .map_err(|source| DatabaseError::Query {
        table: "interface_session",
        operation: "get_interface_for_session",
        source,
    })?;

    Ok(row.map(|r| r.interface))
}

/// Returns the `created_at` of the most recent message in a session, or None.
#[tracing::instrument(skip_all, level = "debug", fields(session_id = %session_id))]
pub async fn last_message_at(
    db: &SqlitePool,
    session_id: &str,
) -> Result<Option<String>, DatabaseError> {
    let row: Option<(String,)> = sqlx::query_as(
        "SELECT created_at FROM message \
         WHERE session_id = ? \
         ORDER BY created_at DESC LIMIT 1",
    )
    .bind(session_id)
    .fetch_optional(db)
    .await
    .map_err(|source| DatabaseError::Query {
        table: "message",
        operation: "last_message_at",
        source,
    })?;
    Ok(row.map(|r| r.0))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn last_message_at_returns_none_for_empty_session() {
        let dir = tempfile::TempDir::new().unwrap();
        let db = crate::db::connect(dir.path(), 384).await.unwrap();
        let sid = create_session(&db).await.unwrap();
        assert!(last_message_at(&db, &sid).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn last_message_at_returns_latest() {
        let dir = tempfile::TempDir::new().unwrap();
        let db = crate::db::connect(dir.path(), 384).await.unwrap();
        let sid = create_session(&db).await.unwrap();
        create_message(&db, &sid, "user", "first").await.unwrap();
        create_message(&db, &sid, "assistant", "second").await.unwrap();
        let ts = last_message_at(&db, &sid).await.unwrap().unwrap();
        let msgs = list_messages_by_session(&db, &sid).await.unwrap();
        assert_eq!(ts, msgs.last().unwrap().created_at);
    }
}
