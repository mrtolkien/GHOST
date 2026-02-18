use serde::Deserialize;
use surrealdb::Surreal;
use surrealdb::engine::local::Db;
use surrealdb::sql::{Datetime, Thing};

use crate::db::error::DatabaseError;
use crate::db::query::{IdRow, query_exec, take_many, take_one};

#[derive(Debug, Clone, Deserialize)]
pub struct JobLogRecord {
    pub id: Thing,
    pub job_name: String,
    pub job_kind: String,
    pub started_at: Datetime,
    pub finished_at: Option<Datetime>,
    pub status: String,
    pub transcript: Option<String>,
}

#[tracing::instrument(skip_all, level = "debug", fields(job_name = job_name, job_kind = job_kind))]
pub async fn create_running_job_log(
    db: &Surreal<Db>,
    job_name: &str,
    job_kind: &str,
    session_id: Option<&Thing>,
) -> Result<Thing, DatabaseError> {
    let mut resp = query_exec(
        db.query(
            "CREATE job_log SET \
                job_name = $job_name, \
                job_kind = $job_kind, \
                session = $session_id, \
                started_at = time::now(), \
                finished_at = NONE, \
                status = 'running', \
                transcript = NONE, \
                handoff_note = NONE, \
                todo_list = NONE \
             RETURN id",
        )
        .bind(("job_name", job_name.to_string()))
        .bind(("job_kind", job_kind.to_string()))
        .bind(("session_id", session_id.cloned())),
        "job_log",
        "create_running",
    )
    .await?;

    let row: IdRow = take_one(&mut resp, 0, "job_log", "create_running")?;
    Ok(row.id)
}

#[tracing::instrument(skip_all, level = "debug", fields(
    job_name = job_name,
    agent_session_id = %agent_session_id
))]
pub async fn create_agent_job_log(
    db: &Surreal<Db>,
    job_name: &str,
    parent_session_id: Option<&Thing>,
    agent_session_id: &Thing,
) -> Result<Thing, DatabaseError> {
    let mut resp = query_exec(
        db.query(
            "CREATE job_log SET \
                job_name = $job_name, \
                job_kind = 'agent', \
                session = $parent_session_id, \
                agent_session = $agent_session_id, \
                started_at = time::now(), \
                finished_at = NONE, \
                status = 'running', \
                transcript = NONE, \
                handoff_note = NONE, \
                todo_list = NONE \
             RETURN id",
        )
        .bind(("job_name", job_name.to_string()))
        .bind(("parent_session_id", parent_session_id.cloned()))
        .bind(("agent_session_id", agent_session_id.clone())),
        "job_log",
        "create_agent",
    )
    .await?;

    let row: IdRow = take_one(&mut resp, 0, "job_log", "create_agent")?;
    Ok(row.id)
}

#[tracing::instrument(skip_all, level = "debug", fields(job_log_id = %job_log_id, status = status))]
pub async fn finish_job_log(
    db: &Surreal<Db>,
    job_log_id: &Thing,
    status: &str,
    transcript: &str,
) -> Result<(), DatabaseError> {
    query_exec(
        db.query(
            "UPDATE $job_log_id SET \
                status = $status, \
                transcript = $transcript, \
                finished_at = time::now()",
        )
        .bind(("job_log_id", job_log_id.clone()))
        .bind(("status", status.to_string()))
        .bind(("transcript", transcript.to_string())),
        "job_log",
        "finish",
    )
    .await?;
    Ok(())
}

#[tracing::instrument(skip_all, level = "debug", fields(name = name, limit = limit))]
pub async fn list_job_logs(
    db: &Surreal<Db>,
    name: Option<&str>,
    limit: usize,
) -> Result<Vec<JobLogRecord>, DatabaseError> {
    let query = match name {
        Some(_) => {
            "SELECT * FROM job_log WHERE job_name = $name \
             ORDER BY started_at DESC LIMIT $limit"
        }
        None => {
            "SELECT * FROM job_log \
             ORDER BY started_at DESC LIMIT $limit"
        }
    };

    let mut resp = query_exec(
        db.query(query)
            .bind(("name", name.map(|n| n.to_string())))
            .bind(("limit", limit)),
        "job_log",
        "list",
    )
    .await?;

    take_many(&mut resp, 0, "job_log", "list")
}
