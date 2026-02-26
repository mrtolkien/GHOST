use serde::{Deserialize, Serialize};
use surrealdb::Surreal;
use surrealdb::engine::local::Db;
use surrealdb::types::{Datetime, RecordId, SurrealValue};

use crate::db::error::DatabaseError;
use crate::db::query::{CountRow, IdRow, query_exec, take_many, take_one};
use crate::tools::TodoItem;

#[derive(Debug, Clone, Deserialize, Serialize, SurrealValue)]
#[surreal(crate = "surrealdb::types")]
pub struct SessionRecord {
    pub id: RecordId,
    pub created_at: Datetime,
    pub updated_at: Datetime,
    pub last_activity_at: Datetime,
    pub status: String,
    pub compaction_summary: Option<String>,
    pub compaction_cursor_id: Option<String>,
    pub todo_list: Option<Vec<serde_json::Value>>,
}

#[derive(Debug, Clone, Deserialize, Serialize, SurrealValue)]
#[surreal(crate = "surrealdb::types")]
pub struct MessageRecord {
    pub id: RecordId,
    pub session: RecordId,
    pub role: String,
    pub content: String,
    pub tool_calls: Option<Vec<serde_json::Value>>,
    pub tool_results: Option<Vec<serde_json::Value>>,
    pub raw_output: Option<Vec<serde_json::Value>>,
    pub created_at: Datetime,
}

#[derive(Debug, Clone, Deserialize, Serialize, SurrealValue)]
#[surreal(crate = "surrealdb::types")]
pub struct SessionListRecord {
    pub id: RecordId,
    pub last_activity_at: Datetime,
    pub status: String,
}

#[derive(Debug, Deserialize, SurrealValue)]
#[surreal(crate = "surrealdb::types")]
struct InterfaceRow {
    interface: String,
}

#[tracing::instrument(skip_all, level = "debug")]
pub async fn create_session(db: &Surreal<Db>) -> Result<RecordId, DatabaseError> {
    let mut resp = query_exec(
        db.query(
            "CREATE session SET \
                created_at = time::now(), \
                updated_at = time::now(), \
                last_activity_at = time::now(), \
                status = 'active', \
                compaction_summary = NONE, \
                compaction_cursor_id = NONE, \
                todo_list = NONE \
             RETURN id",
        ),
        "session",
        "create",
    )
    .await?;

    let row: IdRow = take_one(&mut resp, 0, "session", "create")?;
    Ok(row.id)
}

#[tracing::instrument(skip_all, level = "debug")]
pub async fn create_agent_session(db: &Surreal<Db>) -> Result<RecordId, DatabaseError> {
    let mut resp = query_exec(
        db.query(
            "CREATE session SET \
                created_at = time::now(), \
                updated_at = time::now(), \
                last_activity_at = time::now(), \
                status = 'agent', \
                compaction_summary = NONE, \
                compaction_cursor_id = NONE, \
                todo_list = NONE \
             RETURN id",
        ),
        "session",
        "create_agent",
    )
    .await?;

    let row: IdRow = take_one(&mut resp, 0, "session", "create_agent")?;
    Ok(row.id)
}

#[tracing::instrument(skip_all, level = "debug", fields(session_id = ?session_id))]
pub async fn get_session(
    db: &Surreal<Db>,
    session_id: &RecordId,
) -> Result<SessionRecord, DatabaseError> {
    let mut resp = query_exec(
        db.query("SELECT * FROM ONLY $session_id")
            .bind(("session_id", session_id.clone())),
        "session",
        "get",
    )
    .await?;

    crate::db::query::take_opt(&mut resp, 0, "session", "get")?.ok_or(DatabaseError::MissingRow {
        table: "session",
        operation: "get",
    })
}

#[tracing::instrument(skip_all, level = "debug", fields(session_id = ?session_id))]
pub async fn mark_rebooted(db: &Surreal<Db>, session_id: &RecordId) -> Result<(), DatabaseError> {
    query_exec(
        db.query("UPDATE $session_id SET status = 'rebooted', updated_at = time::now()")
            .bind(("session_id", session_id.clone())),
        "session",
        "mark_rebooted",
    )
    .await?;
    Ok(())
}

#[tracing::instrument(skip_all, level = "debug", fields(session_id = ?session_id))]
pub async fn update_compaction(
    db: &Surreal<Db>,
    session_id: &RecordId,
    summary: &str,
    cursor_id: &str,
) -> Result<(), DatabaseError> {
    query_exec(
        db.query(
            "UPDATE $session_id SET \
                compaction_summary = $summary, \
                compaction_cursor_id = $cursor_id, \
                updated_at = time::now()",
        )
        .bind(("session_id", session_id.clone()))
        .bind(("summary", summary.to_owned()))
        .bind(("cursor_id", cursor_id.to_owned())),
        "session",
        "update_compaction",
    )
    .await?;
    Ok(())
}

#[tracing::instrument(skip_all, level = "debug", fields(session_id = ?session_id))]
pub async fn update_activity(db: &Surreal<Db>, session_id: &RecordId) -> Result<(), DatabaseError> {
    query_exec(
        db.query("UPDATE $session_id SET updated_at = time::now(), last_activity_at = time::now()")
            .bind(("session_id", session_id.clone())),
        "session",
        "update_activity",
    )
    .await?;
    Ok(())
}

#[tracing::instrument(skip_all, level = "debug", fields(session_id = ?session_id))]
pub async fn get_session_todo_list(
    db: &Surreal<Db>,
    session_id: &RecordId,
) -> Result<Option<Vec<TodoItem>>, DatabaseError> {
    #[derive(Debug, Deserialize, SurrealValue)]
    #[surreal(crate = "surrealdb::types")]
    struct TodoRow {
        todo_list: Option<Vec<serde_json::Value>>,
    }

    let mut resp = query_exec(
        db.query("SELECT todo_list FROM ONLY $session_id")
            .bind(("session_id", session_id.clone())),
        "session",
        "get_todo_list",
    )
    .await?;

    let row: TodoRow = crate::db::query::take_opt(&mut resp, 0, "session", "get_todo_list")?
        .ok_or(DatabaseError::MissingRow {
            table: "session",
            operation: "get_todo_list",
        })?;

    let Some(values) = row.todo_list else {
        return Ok(None);
    };

    let items = values
        .into_iter()
        .map(|v| {
            serde_json::from_value::<TodoItem>(v).map_err(|source| DatabaseError::Query {
                table: "session",
                operation: "get_todo_list/deserialize",
                source: surrealdb::Error::serialization(source.to_string(), None),
            })
        })
        .collect::<Result<Vec<_>, _>>()?;

    Ok(Some(items))
}

#[tracing::instrument(skip_all, level = "debug", fields(session_id = ?session_id))]
pub async fn set_session_todo_list(
    db: &Surreal<Db>,
    session_id: &RecordId,
    todo_list: Option<&[TodoItem]>,
) -> Result<(), DatabaseError> {
    match todo_list {
        Some(items) => {
            let values: Vec<serde_json::Value> = items
                .iter()
                .map(|item| serde_json::to_value(item).unwrap_or_default())
                .collect();
            query_exec(
                db.query(
                    "UPDATE $session_id SET \
                        todo_list = $todo_list, \
                        updated_at = time::now()",
                )
                .bind(("session_id", session_id.clone()))
                .bind(("todo_list", values)),
                "session",
                "set_todo_list",
            )
            .await?;
        }
        None => {
            query_exec(
                db.query(
                    "UPDATE $session_id SET \
                        todo_list = NONE, \
                        updated_at = time::now()",
                )
                .bind(("session_id", session_id.clone())),
                "session",
                "set_todo_list",
            )
            .await?;
        }
    }
    Ok(())
}

#[tracing::instrument(skip_all, level = "debug", fields(session_id = ?session_id, role = %role))]
pub async fn create_message(
    db: &Surreal<Db>,
    session_id: &RecordId,
    role: &str,
    content: &str,
) -> Result<RecordId, DatabaseError> {
    create_message_with_metadata(db, session_id, role, content, None, None, None).await
}

#[allow(clippy::too_many_arguments)]
#[tracing::instrument(skip_all, level = "debug", fields(session_id = ?session_id, role = %role))]
pub async fn create_message_with_metadata(
    db: &Surreal<Db>,
    session_id: &RecordId,
    role: &str,
    content: &str,
    tool_calls: Option<Vec<serde_json::Value>>,
    tool_results: Option<Vec<serde_json::Value>>,
    raw_output: Option<Vec<serde_json::Value>>,
) -> Result<RecordId, DatabaseError> {
    let role = role.to_owned();
    let content = content.to_owned();

    let mut resp = query_exec(
        db.query(
            "CREATE message SET \
                session = $session_id, \
                role = $role, \
                content = $content, \
                tool_calls = $tool_calls, \
                tool_results = $tool_results, \
                raw_output = $raw_output, \
                created_at = time::now() \
             RETURN id",
        )
        .bind(("session_id", session_id.clone()))
        .bind(("role", role))
        .bind(("content", content))
        .bind(("tool_calls", tool_calls))
        .bind(("tool_results", tool_results))
        .bind(("raw_output", raw_output)),
        "message",
        "create",
    )
    .await?;

    let row: IdRow = take_one(&mut resp, 0, "message", "create")?;
    Ok(row.id)
}

#[tracing::instrument(skip_all, level = "debug", fields(session_id = ?session_id))]
pub async fn list_messages_by_session(
    db: &Surreal<Db>,
    session_id: &RecordId,
) -> Result<Vec<MessageRecord>, DatabaseError> {
    let mut resp = query_exec(
        db.query("SELECT * FROM message WHERE session = $session_id ORDER BY created_at ASC")
            .bind(("session_id", session_id.clone())),
        "message",
        "list_by_session",
    )
    .await?;

    take_many(&mut resp, 0, "message", "list_by_session")
}

/// Get the most recent message in a session, or `None` if the session is empty.
#[tracing::instrument(skip_all, level = "debug", fields(session_id = ?session_id))]
pub async fn get_last_message(
    db: &Surreal<Db>,
    session_id: &RecordId,
) -> Result<Option<MessageRecord>, DatabaseError> {
    let mut resp = query_exec(
        db.query(
            "SELECT * FROM message WHERE session = $session_id \
             ORDER BY created_at DESC LIMIT 1",
        )
        .bind(("session_id", session_id.clone())),
        "message",
        "get_last",
    )
    .await?;

    let rows: Vec<MessageRecord> = take_many(&mut resp, 0, "message", "get_last")?;
    Ok(rows.into_iter().next())
}

#[tracing::instrument(skip_all, level = "debug", fields(limit = limit))]
pub async fn list_recent_sessions(
    db: &Surreal<Db>,
    limit: usize,
) -> Result<Vec<SessionListRecord>, DatabaseError> {
    let mut resp = query_exec(
        db.query("SELECT id, last_activity_at, status FROM session ORDER BY last_activity_at DESC LIMIT $limit")
            .bind(("limit", limit)),
        "session",
        "list_recent",
    )
    .await?;

    take_many(&mut resp, 0, "session", "list_recent")
}

#[tracing::instrument(skip_all, level = "debug", fields(session_id = ?session_id))]
pub async fn count_messages_for_session(
    db: &Surreal<Db>,
    session_id: &RecordId,
) -> Result<usize, DatabaseError> {
    let mut resp = query_exec(
        db.query("SELECT count() AS count FROM message WHERE session = $session_id GROUP ALL")
            .bind(("session_id", session_id.clone())),
        "message",
        "count_for_session",
    )
    .await?;

    let rows: Vec<CountRow> = take_many(&mut resp, 0, "message", "count_for_session")?;
    Ok(rows.first().map_or(0, |row| row.count.max(0) as usize))
}

#[tracing::instrument(skip_all, level = "debug", fields(session_id = ?session_id))]
pub async fn count_messages_since(
    db: &Surreal<Db>,
    session_id: &RecordId,
    since: &chrono::DateTime<chrono::Utc>,
) -> Result<usize, DatabaseError> {
    let surreal_since = Datetime::from(since.to_owned());
    let mut resp = query_exec(
        db.query(
            "SELECT count() AS count FROM message \
             WHERE session = $session_id AND created_at > $since \
             GROUP ALL",
        )
        .bind(("session_id", session_id.clone()))
        .bind(("since", surreal_since)),
        "message",
        "count_messages_since",
    )
    .await?;

    let rows: Vec<CountRow> = take_many(&mut resp, 0, "message", "count_messages_since")?;
    Ok(rows.first().map_or(0, |row| row.count.max(0) as usize))
}

#[tracing::instrument(skip_all, level = "debug", fields(session_id = ?session_id))]
pub async fn get_interface_for_session(
    db: &Surreal<Db>,
    session_id: &RecordId,
) -> Result<Option<String>, DatabaseError> {
    let mut resp = query_exec(
        db.query("SELECT interface FROM interface_session WHERE session = $session_id LIMIT 1")
            .bind(("session_id", session_id.clone())),
        "interface_session",
        "get_interface_for_session",
    )
    .await?;

    let rows: Vec<InterfaceRow> = take_many(
        &mut resp,
        0,
        "interface_session",
        "get_interface_for_session",
    )?;
    Ok(rows.first().map(|row| row.interface.clone()))
}
