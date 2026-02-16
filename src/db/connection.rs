use std::path::Path;

use surrealdb::Surreal;
use surrealdb::engine::local::{Db, SurrealKv};

use super::error::DatabaseError;
use super::schema;

const NAMESPACE: &str = "ghost";
const DATABASE: &str = "main";

pub type GhostDb = Surreal<Db>;

#[tracing::instrument(skip_all, fields(db_path = %workspace.join("ghost.db").display()))]
pub async fn connect(workspace: &Path) -> Result<GhostDb, DatabaseError> {
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

    schema::apply_schema(&db).await?;
    Ok(db)
}
