use serde::{Deserialize, Serialize};
use surrealdb::Surreal;
use surrealdb::engine::local::Db;
use surrealdb::sql::{Datetime, Thing};

use crate::db::error::DatabaseError;

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SessionRecord {
    pub id: Thing,
    pub created_at: Datetime,
    pub updated_at: Datetime,
    pub last_activity_at: Datetime,
    pub compaction_summary: Option<String>,
    pub compaction_cursor_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct MessageRecord {
    pub id: Thing,
    pub session: Thing,
    pub role: String,
    pub content: String,
    pub tool_calls: Option<Vec<serde_json::Value>>,
    pub tool_results: Option<Vec<serde_json::Value>>,
    pub created_at: Datetime,
}

#[derive(Debug, Deserialize)]
struct IdRow {
    id: Thing,
}

#[tracing::instrument(skip_all)]
pub async fn create_session(db: &Surreal<Db>) -> Result<Thing, DatabaseError> {
    let mut response = db
        .query(
            "CREATE session SET \
                created_at = time::now(), \
                updated_at = time::now(), \
                last_activity_at = time::now(), \
                compaction_summary = NONE, \
                compaction_cursor_id = NONE \
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

#[tracing::instrument(skip_all, fields(session_id = %session_id))]
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

#[tracing::instrument(skip_all, fields(session_id = %session_id))]
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

#[tracing::instrument(skip_all, fields(session_id = %session_id, role = %role))]
pub async fn create_message(
    db: &Surreal<Db>,
    session_id: &Thing,
    role: &str,
    content: &str,
) -> Result<Thing, DatabaseError> {
    let role = role.to_owned();
    let content = content.to_owned();

    let mut response = db
        .query(
            "CREATE message SET \
                session = $session_id, \
                role = $role, \
                content = $content, \
                tool_calls = NONE, \
                tool_results = NONE, \
                created_at = time::now() \
             RETURN id",
        )
        .bind(("session_id", session_id.clone()))
        .bind(("role", role))
        .bind(("content", content))
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

#[tracing::instrument(skip_all, fields(session_id = %session_id))]
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
