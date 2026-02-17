use serde::{Deserialize, Serialize};
use surrealdb::Surreal;
use surrealdb::engine::local::Db;
use surrealdb::sql::{Datetime, Thing};

use crate::db::error::DatabaseError;
use crate::tools::TodoItem;

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SessionRecord {
    pub id: Thing,
    pub created_at: Datetime,
    pub updated_at: Datetime,
    pub last_activity_at: Datetime,
    pub status: String,
    pub compaction_summary: Option<String>,
    pub compaction_cursor_id: Option<String>,
    pub todo_list: Option<Vec<serde_json::Value>>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct MessageRecord {
    pub id: Thing,
    pub session: Thing,
    pub role: String,
    pub content: String,
    pub tool_calls: Option<Vec<serde_json::Value>>,
    pub tool_results: Option<Vec<serde_json::Value>>,
    pub citations: Option<Vec<serde_json::Value>>,
    pub created_at: Datetime,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SessionListRecord {
    pub id: Thing,
    pub last_activity_at: Datetime,
    pub status: String,
}

#[derive(Debug, Deserialize)]
struct IdRow {
    id: Thing,
}

#[derive(Debug, Deserialize)]
struct CountRow {
    count: i64,
}

#[derive(Debug, Deserialize)]
struct InterfaceRow {
    interface: String,
}

#[tracing::instrument(skip_all, level = "debug")]
pub async fn create_session(db: &Surreal<Db>) -> Result<Thing, DatabaseError> {
    let mut response = db
        .query(
            "CREATE session SET \
                created_at = time::now(), \
                updated_at = time::now(), \
                last_activity_at = time::now(), \
                status = 'active', \
                compaction_summary = NONE, \
                compaction_cursor_id = NONE, \
                todo_list = NONE \
             RETURN id",
        )
        .await
        .map_err(|source| DatabaseError::Query {
            table: "session",
            operation: "create",
            source,
        })?;

    let rows: Vec<IdRow> = response.take(0).map_err(|source| DatabaseError::Query {
        table: "session",
        operation: "create/take",
        source,
    })?;

    rows.into_iter()
        .next()
        .map(|row| row.id)
        .ok_or(DatabaseError::MissingRow {
            table: "session",
            operation: "create",
        })
}

#[tracing::instrument(skip_all, level = "debug", fields(session_id = %session_id))]
pub async fn get_session(
    db: &Surreal<Db>,
    session_id: &Thing,
) -> Result<SessionRecord, DatabaseError> {
    let mut response = db
        .query("SELECT * FROM ONLY $session_id")
        .bind(("session_id", session_id.clone()))
        .await
        .map_err(|source| DatabaseError::Query {
            table: "session",
            operation: "get",
            source,
        })?;

    response
        .take::<Option<SessionRecord>>(0)
        .map_err(|source| DatabaseError::Query {
            table: "session",
            operation: "get/take",
            source,
        })?
        .ok_or(DatabaseError::MissingRow {
            table: "session",
            operation: "get",
        })
}

#[tracing::instrument(skip_all, level = "debug", fields(session_id = %session_id))]
pub async fn mark_rebooted(db: &Surreal<Db>, session_id: &Thing) -> Result<(), DatabaseError> {
    db.query("UPDATE $session_id SET status = 'rebooted', updated_at = time::now()")
        .bind(("session_id", session_id.clone()))
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
    db: &Surreal<Db>,
    session_id: &Thing,
    summary: &str,
    cursor_id: &str,
) -> Result<(), DatabaseError> {
    db.query(
        "UPDATE $session_id SET \
            compaction_summary = $summary, \
            compaction_cursor_id = $cursor_id, \
            updated_at = time::now()",
    )
    .bind(("session_id", session_id.clone()))
    .bind(("summary", summary.to_owned()))
    .bind(("cursor_id", cursor_id.to_owned()))
    .await
    .map_err(|source| DatabaseError::Query {
        table: "session",
        operation: "update_compaction",
        source,
    })?;

    Ok(())
}

#[tracing::instrument(skip_all, level = "debug", fields(session_id = %session_id))]
pub async fn update_activity(db: &Surreal<Db>, session_id: &Thing) -> Result<(), DatabaseError> {
    db.query("UPDATE $session_id SET updated_at = time::now(), last_activity_at = time::now()")
        .bind(("session_id", session_id.clone()))
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
    db: &Surreal<Db>,
    session_id: &Thing,
) -> Result<Option<Vec<TodoItem>>, DatabaseError> {
    #[derive(Debug, Deserialize)]
    struct TodoRow {
        todo_list: Option<Vec<serde_json::Value>>,
    }

    let mut response = db
        .query("SELECT todo_list FROM ONLY $session_id")
        .bind(("session_id", session_id.clone()))
        .await
        .map_err(|source| DatabaseError::Query {
            table: "session",
            operation: "get_todo_list",
            source,
        })?;

    let row = response
        .take::<Option<TodoRow>>(0)
        .map_err(|source| DatabaseError::Query {
            table: "session",
            operation: "get_todo_list/take",
            source,
        })?
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
                source: surrealdb::Error::Db(surrealdb::error::Db::Serialization(
                    source.to_string(),
                )),
            })
        })
        .collect::<Result<Vec<_>, _>>()?;

    Ok(Some(items))
}

#[tracing::instrument(skip_all, level = "debug", fields(session_id = %session_id))]
pub async fn set_session_todo_list(
    db: &Surreal<Db>,
    session_id: &Thing,
    todo_list: Option<&[TodoItem]>,
) -> Result<(), DatabaseError> {
    match todo_list {
        Some(items) => {
            let values: Vec<serde_json::Value> = items
                .iter()
                .map(|item| serde_json::to_value(item).unwrap_or_default())
                .collect();
            db.query(
                "UPDATE $session_id SET \
                    todo_list = $todo_list, \
                    updated_at = time::now()",
            )
            .bind(("session_id", session_id.clone()))
            .bind(("todo_list", values))
            .await
            .map_err(|source| DatabaseError::Query {
                table: "session",
                operation: "set_todo_list",
                source,
            })?;
        }
        None => {
            db.query(
                "UPDATE $session_id SET \
                    todo_list = NONE, \
                    updated_at = time::now()",
            )
            .bind(("session_id", session_id.clone()))
            .await
            .map_err(|source| DatabaseError::Query {
                table: "session",
                operation: "set_todo_list",
                source,
            })?;
        }
    }

    Ok(())
}

#[tracing::instrument(skip_all, level = "debug", fields(session_id = %session_id, role = %role))]
pub async fn create_message(
    db: &Surreal<Db>,
    session_id: &Thing,
    role: &str,
    content: &str,
) -> Result<Thing, DatabaseError> {
    create_message_with_metadata(db, session_id, role, content, None, None, None).await
}

#[tracing::instrument(skip_all, level = "debug", fields(session_id = %session_id, role = %role))]
pub async fn create_message_with_metadata(
    db: &Surreal<Db>,
    session_id: &Thing,
    role: &str,
    content: &str,
    tool_calls: Option<Vec<serde_json::Value>>,
    tool_results: Option<Vec<serde_json::Value>>,
    citations: Option<Vec<serde_json::Value>>,
) -> Result<Thing, DatabaseError> {
    let role = role.to_owned();
    let content = content.to_owned();

    let mut response = db
        .query(
            "CREATE message SET \
                session = $session_id, \
                role = $role, \
                content = $content, \
                tool_calls = $tool_calls, \
                tool_results = $tool_results, \
                citations = $citations, \
                created_at = time::now() \
             RETURN id",
        )
        .bind(("session_id", session_id.clone()))
        .bind(("role", role))
        .bind(("content", content))
        .bind(("tool_calls", tool_calls))
        .bind(("tool_results", tool_results))
        .bind(("citations", citations))
        .await
        .map_err(|source| DatabaseError::Query {
            table: "message",
            operation: "create",
            source,
        })?;

    let rows: Vec<IdRow> = response.take(0).map_err(|source| DatabaseError::Query {
        table: "message",
        operation: "create/take",
        source,
    })?;

    rows.into_iter()
        .next()
        .map(|row| row.id)
        .ok_or(DatabaseError::MissingRow {
            table: "message",
            operation: "create",
        })
}

#[tracing::instrument(skip_all, level = "debug", fields(session_id = %session_id))]
pub async fn list_messages_by_session(
    db: &Surreal<Db>,
    session_id: &Thing,
) -> Result<Vec<MessageRecord>, DatabaseError> {
    let mut response = db
        .query("SELECT * FROM message WHERE session = $session_id ORDER BY created_at ASC")
        .bind(("session_id", session_id.clone()))
        .await
        .map_err(|source| DatabaseError::Query {
            table: "message",
            operation: "list_by_session",
            source,
        })?;

    response
        .take::<Vec<MessageRecord>>(0)
        .map_err(|source| DatabaseError::Query {
            table: "message",
            operation: "list_by_session/take",
            source,
        })
}

#[tracing::instrument(skip_all, level = "debug", fields(limit = limit))]
pub async fn list_recent_sessions(
    db: &Surreal<Db>,
    limit: usize,
) -> Result<Vec<SessionListRecord>, DatabaseError> {
    let mut response = db
        .query("SELECT id, last_activity_at, status FROM session ORDER BY last_activity_at DESC LIMIT $limit")
        .bind(("limit", limit))
        .await
        .map_err(|source| DatabaseError::Query {
            table: "session",
            operation: "list_recent",
            source,
        })?;

    response
        .take::<Vec<SessionListRecord>>(0)
        .map_err(|source| DatabaseError::Query {
            table: "session",
            operation: "list_recent/take",
            source,
        })
}

#[tracing::instrument(skip_all, level = "debug", fields(session_id = %session_id))]
pub async fn count_messages_for_session(
    db: &Surreal<Db>,
    session_id: &Thing,
) -> Result<usize, DatabaseError> {
    let mut response = db
        .query("SELECT count() AS count FROM message WHERE session = $session_id GROUP ALL")
        .bind(("session_id", session_id.clone()))
        .await
        .map_err(|source| DatabaseError::Query {
            table: "message",
            operation: "count_for_session",
            source,
        })?;

    let rows: Vec<CountRow> = response.take(0).map_err(|source| DatabaseError::Query {
        table: "message",
        operation: "count_for_session/take",
        source,
    })?;
    Ok(rows.first().map_or(0, |row| row.count.max(0) as usize))
}

#[tracing::instrument(skip_all, level = "debug", fields(session_id = %session_id))]
pub async fn count_messages_since(
    db: &Surreal<Db>,
    session_id: &Thing,
    since: &chrono::DateTime<chrono::Utc>,
) -> Result<usize, DatabaseError> {
    let surreal_since = Datetime::from(since.to_owned());
    let mut response = db
        .query(
            "SELECT count() AS count FROM message \
             WHERE session = $session_id AND created_at > $since \
             GROUP ALL",
        )
        .bind(("session_id", session_id.clone()))
        .bind(("since", surreal_since))
        .await
        .map_err(|source| DatabaseError::Query {
            table: "message",
            operation: "count_messages_since",
            source,
        })?;

    let rows: Vec<CountRow> = response.take(0).map_err(|source| DatabaseError::Query {
        table: "message",
        operation: "count_messages_since/take",
        source,
    })?;
    Ok(rows.first().map_or(0, |row| row.count.max(0) as usize))
}

#[tracing::instrument(skip_all, level = "debug", fields(session_id = %session_id))]
pub async fn get_interface_for_session(
    db: &Surreal<Db>,
    session_id: &Thing,
) -> Result<Option<String>, DatabaseError> {
    let mut response = db
        .query("SELECT interface FROM interface_session WHERE session = $session_id LIMIT 1")
        .bind(("session_id", session_id.clone()))
        .await
        .map_err(|source| DatabaseError::Query {
            table: "interface_session",
            operation: "get_interface_for_session",
            source,
        })?;

    let rows: Vec<InterfaceRow> = response.take(0).map_err(|source| DatabaseError::Query {
        table: "interface_session",
        operation: "get_interface_for_session/take",
        source,
    })?;
    Ok(rows.first().map(|row| row.interface.clone()))
}
