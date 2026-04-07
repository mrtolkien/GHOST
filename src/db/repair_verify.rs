use std::path::Path;

use crate::db::DatabaseError;

use super::repair_copy::DB_ONLY_TABLES;
use super::repair_types::TableVerification;

pub async fn verify_db_only_tables(
    candidate_db_path: &Path,
    live_db_path: &Path,
) -> Result<Vec<TableVerification>, DatabaseError> {
    let mut tables = Vec::with_capacity(DB_ONLY_TABLES.len());
    for table in DB_ONLY_TABLES {
        let (source_rows, copied_rows) =
            super::repair_copy::count_rows(candidate_db_path, live_db_path, table).await?;
        tables.push(TableVerification {
            table,
            copied_rows,
            source_rows,
            verified: source_rows == copied_rows,
        });
    }
    Ok(tables)
}
