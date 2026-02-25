use std::path::Path;

use surrealdb::Surreal;
use surrealdb::engine::local::{Db, SurrealKv};
use tracing::info;

use super::error::DatabaseError;
use super::schema;

const NAMESPACE: &str = "ghost";
const DATABASE: &str = "main";

pub type GhostDb = Surreal<Db>;

#[tracing::instrument(skip_all, fields(db_path = %workspace.join("ghost.db").display()))]
pub async fn connect(workspace: &Path) -> Result<GhostDb, DatabaseError> {
    // Safety net: cap SurrealDB memory to 4 GiB unless the operator overrides.
    // SAFETY: called before any SurrealDB threads are spawned.
    if std::env::var_os("SURREAL_MEMORY_THRESHOLD").is_none() {
        unsafe { std::env::set_var("SURREAL_MEMORY_THRESHOLD", "4GiB") };
    }

    let db_path = workspace.join("ghost.db");

    let db = Surreal::new::<SurrealKv>(db_path.clone())
        .await
        .map_err(|source| DatabaseError::Connect {
            path: db_path.clone(),
            source,
        })?;

    db.use_ns(NAMESPACE)
        .use_db(DATABASE)
        .await
        .map_err(|source| DatabaseError::SelectNamespace {
            namespace: NAMESPACE,
            database: DATABASE,
            source,
        })?;

    let start = std::time::Instant::now();
    schema::apply_schema(&db).await?;
    info!(
        elapsed_ms = start.elapsed().as_millis() as u64,
        "schema applied"
    );

    Ok(db)
}
