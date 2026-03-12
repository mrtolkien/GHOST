use serde::Deserialize;
use sqlx::SqlitePool;

use crate::db::error::DatabaseError;
use crate::db::{new_id, now};

#[derive(Debug, Clone, Deserialize, sqlx::FromRow)]
pub struct AgentRunRecord {
    pub id: String,
    pub agent_name: String,
    pub run_kind: String,
    pub started_at: String,
    pub finished_at: Option<String>,
    pub status: String,
    pub transcript: Option<String>,
    pub agent_session_id: Option<String>,
}

#[tracing::instrument(skip_all, level = "debug", fields(
    agent_name = agent_name,
    agent_session_id = %agent_session_id
))]
pub async fn create_agent_run(
    db: &SqlitePool,
    agent_name: &str,
    parent_session_id: Option<&str>,
    agent_session_id: &str,
) -> Result<String, DatabaseError> {
    let id = new_id();

    sqlx::query(
        "INSERT INTO agent_run \
         (id, agent_name, run_kind, session_id, agent_session_id, started_at, status) \
         VALUES (?, ?, 'agent', ?, ?, ?, 'running')",
    )
    .bind(&id)
    .bind(agent_name)
    .bind(parent_session_id)
    .bind(agent_session_id)
    .bind(now())
    .execute(db)
    .await
    .map_err(|source| DatabaseError::Query {
        table: "agent_run",
        operation: "create",
        source,
    })?;

    Ok(id)
}

#[tracing::instrument(skip_all, level = "debug", fields(run_id = %run_id, status = status))]
pub async fn finish_run(
    db: &SqlitePool,
    run_id: &str,
    status: &str,
    transcript: &str,
) -> Result<(), DatabaseError> {
    sqlx::query("UPDATE agent_run SET status = ?, transcript = ?, finished_at = ? WHERE id = ?")
        .bind(status)
        .bind(transcript)
        .bind(now())
        .bind(run_id)
        .execute(db)
        .await
        .map_err(|source| DatabaseError::Query {
            table: "agent_run",
            operation: "finish",
            source,
        })?;
    Ok(())
}

#[tracing::instrument(skip_all, level = "debug", fields(name = name, limit = limit))]
pub async fn list_runs(
    db: &SqlitePool,
    name: Option<&str>,
    limit: usize,
) -> Result<Vec<AgentRunRecord>, DatabaseError> {
    match name {
        Some(n) => {
            sqlx::query_as::<_, AgentRunRecord>(
                "SELECT id, agent_name, run_kind, started_at, finished_at, status, transcript, \
             agent_session_id FROM agent_run \
             WHERE agent_name = ? ORDER BY started_at DESC LIMIT ?",
            )
            .bind(n)
            .bind(limit as i64)
            .fetch_all(db)
            .await
        }
        None => {
            sqlx::query_as::<_, AgentRunRecord>(
                "SELECT id, agent_name, run_kind, started_at, finished_at, status, transcript, \
             agent_session_id FROM agent_run \
             ORDER BY started_at DESC LIMIT ?",
            )
            .bind(limit as i64)
            .fetch_all(db)
            .await
        }
    }
    .map_err(|source| DatabaseError::Query {
        table: "agent_run",
        operation: "list",
        source,
    })
}

/// Fetch a single agent run by ID.
pub async fn get_run(
    db: &SqlitePool,
    run_id: &str,
) -> Result<Option<AgentRunRecord>, DatabaseError> {
    sqlx::query_as::<_, AgentRunRecord>(
        "SELECT id, agent_name, run_kind, started_at, finished_at, status, transcript, \
         agent_session_id FROM agent_run WHERE id = ?",
    )
    .bind(run_id)
    .fetch_optional(db)
    .await
    .map_err(|source| DatabaseError::Query {
        table: "agent_run",
        operation: "get_run",
        source,
    })
}

/// Look up the agent name from an agent_run record by agent session ID.
#[tracing::instrument(skip_all, level = "debug", fields(agent_session_id = %agent_session_id))]
pub async fn get_agent_name_for_session(
    db: &SqlitePool,
    agent_session_id: &str,
) -> Result<Option<String>, DatabaseError> {
    #[derive(sqlx::FromRow)]
    struct AgentNameRow {
        agent_name: String,
    }

    let row = sqlx::query_as::<_, AgentNameRow>(
        "SELECT agent_name FROM agent_run \
         WHERE agent_session_id = ? AND run_kind = 'agent' \
         ORDER BY started_at DESC LIMIT 1",
    )
    .bind(agent_session_id)
    .fetch_optional(db)
    .await
    .map_err(|source| DatabaseError::Query {
        table: "agent_run",
        operation: "get_agent_name_for_session",
        source,
    })?;

    Ok(row.map(|r| r.agent_name))
}

/// Check if an agent run exists for the given agent + parent session
/// that started after the given timestamp.
#[tracing::instrument(skip_all, level = "debug", fields(
    agent_name = agent_name,
    session_id = session_id,
))]
pub async fn has_run_since(
    db: &SqlitePool,
    agent_name: &str,
    session_id: &str,
    since: &str,
) -> Result<bool, DatabaseError> {
    let row: Option<(String,)> = sqlx::query_as(
        "SELECT id FROM agent_run \
         WHERE agent_name = ? AND session_id = ? AND started_at > ? \
         LIMIT 1",
    )
    .bind(agent_name)
    .bind(session_id)
    .bind(since)
    .fetch_optional(db)
    .await
    .map_err(|source| DatabaseError::Query {
        table: "agent_run",
        operation: "has_run_since",
        source,
    })?;
    Ok(row.is_some())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn has_run_since_false_when_no_runs() {
        let dir = tempfile::TempDir::new().unwrap();
        let db = crate::db::connect(dir.path(), 384).await.unwrap();
        assert!(
            !has_run_since(&db, "my-agent", "session-1", "2026-01-01T00:00:00Z")
                .await
                .unwrap()
        );
    }

    #[tokio::test]
    async fn has_run_since_true_when_run_exists_after() {
        let dir = tempfile::TempDir::new().unwrap();
        let db = crate::db::connect(dir.path(), 384).await.unwrap();
        let sid = crate::db::sessions::create_session(&db).await.unwrap();
        let agent_sid = crate::db::sessions::create_agent_session(&db)
            .await
            .unwrap();
        let _run_id = create_agent_run(&db, "my-agent", Some(&sid), &agent_sid)
            .await
            .unwrap();
        assert!(
            has_run_since(&db, "my-agent", &sid, "2026-01-01T00:00:00Z")
                .await
                .unwrap()
        );
    }

    #[tokio::test]
    async fn has_run_since_ignores_runs_with_null_session() {
        let dir = tempfile::TempDir::new().unwrap();
        let db = crate::db::connect(dir.path(), 384).await.unwrap();
        let agent_sid = crate::db::sessions::create_agent_session(&db)
            .await
            .unwrap();
        let _run_id = create_agent_run(&db, "my-agent", None, &agent_sid)
            .await
            .unwrap();
        assert!(
            !has_run_since(&db, "my-agent", "some-session", "2026-01-01T00:00:00Z")
                .await
                .unwrap()
        );
    }

    #[tokio::test]
    async fn has_run_since_false_when_run_before_threshold() {
        let dir = tempfile::TempDir::new().unwrap();
        let db = crate::db::connect(dir.path(), 384).await.unwrap();
        let sid = crate::db::sessions::create_session(&db).await.unwrap();
        let agent_sid = crate::db::sessions::create_agent_session(&db)
            .await
            .unwrap();
        let _run_id = create_agent_run(&db, "my-agent", Some(&sid), &agent_sid)
            .await
            .unwrap();
        assert!(
            !has_run_since(&db, "my-agent", &sid, "2099-01-01T00:00:00Z")
                .await
                .unwrap()
        );
    }
}
