use serde::Deserialize;
use sqlx::SqlitePool;

use crate::db::error::DatabaseError;
use crate::db::{new_id, now};

#[derive(Debug, Clone, Deserialize, sqlx::FromRow)]
pub struct JobLogRecord {
    pub id: String,
    pub job_name: String,
    pub job_kind: String,
    pub started_at: String,
    pub finished_at: Option<String>,
    pub status: String,
    pub transcript: Option<String>,
    pub agent_session_id: Option<String>,
}

#[tracing::instrument(skip_all, level = "debug", fields(job_name = job_name, job_kind = job_kind))]
pub async fn create_running_job_log(
    db: &SqlitePool,
    job_name: &str,
    job_kind: &str,
    session_id: Option<&str>,
) -> Result<String, DatabaseError> {
    let id = new_id();

    sqlx::query(
        "INSERT INTO job_log (id, job_name, job_kind, session_id, started_at, status) \
         VALUES (?, ?, ?, ?, ?, 'running')",
    )
    .bind(&id)
    .bind(job_name)
    .bind(job_kind)
    .bind(session_id)
    .bind(now())
    .execute(db)
    .await
    .map_err(|source| DatabaseError::Query {
        table: "job_log",
        operation: "create_running",
        source,
    })?;

    Ok(id)
}

#[tracing::instrument(skip_all, level = "debug", fields(
    job_name = job_name,
    agent_session_id = %agent_session_id
))]
pub async fn create_agent_job_log(
    db: &SqlitePool,
    job_name: &str,
    parent_session_id: Option<&str>,
    agent_session_id: &str,
) -> Result<String, DatabaseError> {
    let id = new_id();

    sqlx::query(
        "INSERT INTO job_log \
         (id, job_name, job_kind, session_id, agent_session_id, started_at, status) \
         VALUES (?, ?, 'agent', ?, ?, ?, 'running')",
    )
    .bind(&id)
    .bind(job_name)
    .bind(parent_session_id)
    .bind(agent_session_id)
    .bind(now())
    .execute(db)
    .await
    .map_err(|source| DatabaseError::Query {
        table: "job_log",
        operation: "create_agent",
        source,
    })?;

    Ok(id)
}

#[tracing::instrument(skip_all, level = "debug", fields(job_log_id = %job_log_id, status = status))]
pub async fn finish_job_log(
    db: &SqlitePool,
    job_log_id: &str,
    status: &str,
    transcript: &str,
) -> Result<(), DatabaseError> {
    sqlx::query("UPDATE job_log SET status = ?, transcript = ?, finished_at = ? WHERE id = ?")
        .bind(status)
        .bind(transcript)
        .bind(now())
        .bind(job_log_id)
        .execute(db)
        .await
        .map_err(|source| DatabaseError::Query {
            table: "job_log",
            operation: "finish",
            source,
        })?;
    Ok(())
}

#[tracing::instrument(skip_all, level = "debug", fields(name = name, limit = limit))]
pub async fn list_job_logs(
    db: &SqlitePool,
    name: Option<&str>,
    limit: usize,
) -> Result<Vec<JobLogRecord>, DatabaseError> {
    match name {
        Some(n) => {
            sqlx::query_as::<_, JobLogRecord>(
                "SELECT id, job_name, job_kind, started_at, finished_at, status, transcript, \
             agent_session_id FROM job_log \
             WHERE job_name = ? ORDER BY started_at DESC LIMIT ?",
            )
            .bind(n)
            .bind(limit as i64)
            .fetch_all(db)
            .await
        }
        None => {
            sqlx::query_as::<_, JobLogRecord>(
                "SELECT id, job_name, job_kind, started_at, finished_at, status, transcript, \
             agent_session_id FROM job_log \
             ORDER BY started_at DESC LIMIT ?",
            )
            .bind(limit as i64)
            .fetch_all(db)
            .await
        }
    }
    .map_err(|source| DatabaseError::Query {
        table: "job_log",
        operation: "list",
        source,
    })
}

/// Look up the agent name from a job_log record by agent session ID.
#[tracing::instrument(skip_all, level = "debug", fields(agent_session_id = %agent_session_id))]
pub async fn get_agent_name_for_session(
    db: &SqlitePool,
    agent_session_id: &str,
) -> Result<Option<String>, DatabaseError> {
    #[derive(sqlx::FromRow)]
    struct JobNameRow {
        job_name: String,
    }

    let row = sqlx::query_as::<_, JobNameRow>(
        "SELECT job_name FROM job_log \
         WHERE agent_session_id = ? AND job_kind = 'agent' \
         ORDER BY started_at DESC LIMIT 1",
    )
    .bind(agent_session_id)
    .fetch_optional(db)
    .await
    .map_err(|source| DatabaseError::Query {
        table: "job_log",
        operation: "get_agent_name_for_session",
        source,
    })?;

    Ok(row.map(|r| r.job_name))
}
