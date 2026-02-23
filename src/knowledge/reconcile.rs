use std::collections::HashSet;

use surrealdb::Surreal;
use surrealdb::engine::local::Db;
use surrealdb::sql::Thing;

use crate::db;

use super::error::KnowledgeError;
use super::types::WikiLink;

#[derive(Debug)]
pub struct ReconcileResult {
    pub created: usize,
    pub deleted: usize,
    pub stubs_created: usize,
}

#[tracing::instrument(skip_all, fields(note_id = %note_id, link_count = new_links.len()))]
pub async fn reconcile_edges(
    db_conn: &Surreal<Db>,
    note_id: &Thing,
    _note_title: &str,
    new_links: &[WikiLink],
) -> Result<ReconcileResult, KnowledgeError> {
    let mut stubs_created = 0usize;

    // Resolve each wiki link target to a note ID, creating stubs as needed.
    let mut desired: Vec<(Thing, String)> = Vec::new();
    for link in new_links {
        let target_id = match db::knowledge::find_note_by_title(db_conn, &link.target)
            .await
            .map_err(Box::new)?
        {
            Some(note) => note.id,
            None => {
                let id =
                    db::knowledge::create_note_full(db_conn, &link.target, "", None, &[], 1, None)
                        .await
                        .map_err(Box::new)?;
                stubs_created += 1;
                id
            }
        };
        let label = link
            .relationship
            .as_deref()
            .unwrap_or("relates_to")
            .to_string();
        desired.push((target_id, label));
    }

    // Fetch existing outgoing edges.
    let existing = db::knowledge::outgoing_edges(db_conn, note_id)
        .await
        .map_err(Box::new)?;

    // Build sets for diffing: (out_id, label)
    let desired_set: HashSet<(String, String)> = desired
        .iter()
        .map(|(id, label)| (id.to_string(), label.clone()))
        .collect();

    let existing_set: HashSet<(String, String)> = existing
        .iter()
        .map(|e| (e.out.to_string(), e.label.clone()))
        .collect();

    // Create new edges (in desired but not existing)
    let mut created = 0usize;
    for (target_id, label) in &desired {
        let key = (target_id.to_string(), label.clone());
        if !existing_set.contains(&key) {
            db::knowledge::create_edge(db_conn, note_id, target_id, label)
                .await
                .map_err(Box::new)?;
            created += 1;
        }
    }

    // Delete removed edges (in existing but not desired)
    let mut deleted = 0usize;
    for edge in &existing {
        let key = (edge.out.to_string(), edge.label.clone());
        if !desired_set.contains(&key) {
            db_conn
                .query("DELETE $edge_id")
                .bind(("edge_id", edge.id.clone()))
                .await
                .map_err(|source| {
                    Box::new(db::DatabaseError::Query {
                        table: "relates_to",
                        operation: "delete_edge",
                        source,
                    })
                })?;
            deleted += 1;
        }
    }

    Ok(ReconcileResult {
        created,
        deleted,
        stubs_created,
    })
}
