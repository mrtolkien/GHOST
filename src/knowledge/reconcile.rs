use std::collections::HashSet;

use crate::db;
use crate::db::GhostDb;
use crate::db::knowledge::NoteInput;

use super::error::KnowledgeError;
use super::types::WikiLink;

#[derive(Debug)]
pub struct ReconcileResult {
    pub created: usize,
    pub deleted: usize,
    pub stubs_created: usize,
    /// Titles of notes that were created as empty stubs from wiki links.
    pub stub_titles: Vec<String>,
}

#[tracing::instrument(skip_all, fields(note_id = ?note_id, link_count = new_links.len()))]
pub async fn reconcile_edges(
    db_conn: &GhostDb,
    note_id: &str,
    _note_title: &str,
    new_links: &[WikiLink],
    parent: Option<&str>,
) -> Result<ReconcileResult, KnowledgeError> {
    let mut stubs_created = 0usize;
    let mut stub_titles = Vec::new();

    // Build desired links, injecting parent edge if set
    let mut all_links = new_links.to_vec();
    if let Some(parent_title) = parent {
        let already_linked = all_links
            .iter()
            .any(|l| l.target == parent_title && l.relationship.as_deref() == Some("parent"));
        if !already_linked {
            all_links.push(WikiLink {
                target: parent_title.to_string(),
                relationship: Some("parent".to_string()),
            });
        }
    }

    // Resolve each wiki link target to a note ID, creating stubs as needed.
    let mut desired: Vec<(String, String)> = Vec::new();
    for link in &all_links {
        let target_id = match db::knowledge::find_note_by_title(db_conn, &link.target)
            .await
            .map_err(Box::new)?
        {
            Some(note) => note.id,
            None => {
                let id = db::knowledge::create_note_full(
                    db_conn,
                    &NoteInput {
                        title: &link.target,
                        trust: 1,
                        archetype: Some("entity"),
                        ..Default::default()
                    },
                )
                .await
                .map_err(Box::new)?;
                stubs_created += 1;
                stub_titles.push(link.target.clone());
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

    // Build sets for diffing: (to_id, label)
    let desired_set: HashSet<(String, String)> = desired
        .iter()
        .map(|(id, label)| (id.clone(), label.clone()))
        .collect();

    let existing_set: HashSet<(String, String)> = existing
        .iter()
        .map(|e| (e.to_id.clone(), e.label.clone()))
        .collect();

    // Create new edges (in desired but not existing)
    let mut created = 0usize;
    for (target_id, label) in &desired {
        let key = (target_id.clone(), label.clone());
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
        let key = (edge.to_id.clone(), edge.label.clone());
        if !desired_set.contains(&key) {
            sqlx::query("DELETE FROM relates_to WHERE id = ?")
                .bind(&edge.id)
                .execute(db_conn)
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
        stub_titles,
    })
}
