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
