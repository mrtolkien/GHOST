use serde::Deserialize;
use surrealdb::IndexedResults;
use surrealdb::types::{RecordId, SurrealValue};

use super::error::DatabaseError;

#[derive(Debug, Deserialize, SurrealValue)]
#[surreal(crate = "surrealdb::types")]
pub struct IdRow {
    pub id: RecordId,
}

#[derive(Debug, Deserialize, SurrealValue)]
#[surreal(crate = "surrealdb::types")]
pub struct CountRow {
    pub count: i64,
}

#[allow(clippy::result_large_err)]
pub fn take_many<T: SurrealValue>(
    response: &mut IndexedResults,
    idx: usize,
    table: &'static str,
    operation: &'static str,
) -> Result<Vec<T>, DatabaseError> {
    response.take(idx).map_err(|source| DatabaseError::Query {
        table,
        operation,
        source,
    })
}

#[allow(clippy::result_large_err)]
pub fn take_one<T: SurrealValue>(
    response: &mut IndexedResults,
    idx: usize,
    table: &'static str,
    operation: &'static str,
) -> Result<T, DatabaseError> {
    let rows: Vec<T> = take_many(response, idx, table, operation)?;
    rows.into_iter()
        .next()
        .ok_or(DatabaseError::MissingRow { table, operation })
}

#[allow(clippy::result_large_err)]
pub fn take_opt<T: SurrealValue>(
    response: &mut IndexedResults,
    idx: usize,
    table: &'static str,
    operation: &'static str,
) -> Result<Option<T>, DatabaseError> {
    response
        .take::<Option<T>>(idx)
        .map_err(|source| DatabaseError::Query {
            table,
            operation,
            source,
        })
}

#[allow(clippy::result_large_err)]
pub async fn query_exec(
    query: impl std::future::IntoFuture<Output = Result<IndexedResults, surrealdb::Error>>,
    table: &'static str,
    operation: &'static str,
) -> Result<IndexedResults, DatabaseError> {
    query.await.map_err(|source| DatabaseError::Query {
        table,
        operation,
        source,
    })
}
