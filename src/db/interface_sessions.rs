//! TEMPORARY SCAFFOLDING
//! This is the minimal interface-session mapping layer needed by spec 06 reboot flow.
//! It is expected to be revisited/reworked in full Discord interface work (spec 09).

use serde::Deserialize;
use surrealdb::Surreal;
use surrealdb::engine::local::Db;
use surrealdb::sql::Thing;

use crate::db::error::DatabaseError;

#[derive(Debug, Deserialize)]
struct SessionRow {
    session: Thing,
}

#[tracing::instrument(skip_all, level = "debug", fields(interface = interface))]
pub async fn get_active_session_for_interface(
    db: &Surreal<Db>,
    interface: &str,
) -> Result<Option<Thing>, DatabaseError> {
    let mut response = db
        .query("SELECT session FROM interface_session WHERE interface = $interface LIMIT 1")
        .bind(("interface", interface.to_string()))
        .await
        .map_err(|source| DatabaseError::Query {
            table: "interface_session",
            operation: "get_active",
            source,
        })?;

    let rows: Vec<SessionRow> = response.take(0).map_err(|source| DatabaseError::Query {
        table: "interface_session",
        operation: "get_active/take",
        source,
    })?;

    Ok(rows.first().map(|row| row.session.clone()))
}

#[tracing::instrument(skip_all, level = "debug", fields(interface = interface, session_id = %session_id))]
pub async fn set_active_session_for_interface(
    db: &Surreal<Db>,
    interface: &str,
    session_id: &Thing,
) -> Result<(), DatabaseError> {
    db.query(
        "UPSERT interface_session SET \
            interface = $interface, \
            session = $session_id, \
            created_at = time::now() \
         WHERE interface = $interface",
    )
    .bind(("interface", interface.to_string()))
    .bind(("session_id", session_id.clone()))
    .await
    .map_err(|source| DatabaseError::Query {
        table: "interface_session",
        operation: "set_active",
        source,
    })?;

    Ok(())
}

#[derive(Debug, Deserialize)]
pub struct InterfaceSessionRecord {
    pub interface: String,
    pub session: Thing,
}

#[tracing::instrument(skip_all, level = "debug")]
pub async fn list_all_interface_sessions(
    db: &Surreal<Db>,
) -> Result<Vec<InterfaceSessionRecord>, DatabaseError> {
    let mut response = db
        .query("SELECT interface, session FROM interface_session")
        .await
        .map_err(|source| DatabaseError::Query {
            table: "interface_session",
            operation: "list_all",
            source,
        })?;

    response
        .take::<Vec<InterfaceSessionRecord>>(0)
        .map_err(|source| DatabaseError::Query {
            table: "interface_session",
            operation: "list_all/take",
            source,
        })
}

#[tracing::instrument(skip_all, level = "debug", fields(old_session_id = %old_session_id, new_session_id = %new_session_id))]
pub async fn replace_session_everywhere(
    db: &Surreal<Db>,
    old_session_id: &Thing,
    new_session_id: &Thing,
) -> Result<(), DatabaseError> {
    db.query(
        "UPDATE interface_session SET session = $new_session_id WHERE session = $old_session_id",
    )
    .bind(("old_session_id", old_session_id.clone()))
    .bind(("new_session_id", new_session_id.clone()))
    .await
    .map_err(|source| DatabaseError::Query {
        table: "interface_session",
        operation: "replace_session_everywhere",
        source,
    })?;

    Ok(())
}
