use std::io::Read;

use clap::Subcommand;

use crate::db;
use crate::db::GhostDb;
use crate::error::GhostError;
use crate::knowledge::reconcile::reconcile_edges;
use crate::knowledge::sanitize::sanitize_reference_links;
use crate::knowledge::{self, Archetype, NoteFrontMatter, extract_wiki_links};

#[derive(Debug, Subcommand)]
pub enum NoteCommand {
    Create {
        #[arg(long)]
        title: String,
        #[arg(long = "tag")]
        tags: Vec<String>,
        #[arg(long = "source")]
        sources: Vec<String>,
        #[arg(long, default_value_t = 5)]
        trust: i64,
    },
    Update {
        #[arg(long)]
        title: String,
        #[arg(long = "tag")]
        tags: Vec<String>,
        #[arg(long = "source")]
        sources: Vec<String>,
        #[arg(long, default_value_t = 5)]
        trust: i64,
    },
}

#[tracing::instrument(skip_all)]
pub async fn execute(command: NoteCommand) -> Result<(), GhostError> {
    let config = crate::config::load()?;
    crate::config_workspace::bootstrap_workspace(&config)?;
    let db = crate::db::connect(&config.workspace, config.embeddings.dimension).await?;

    let mut body = String::new();
    std::io::stdin()
        .read_to_string(&mut body)
        .map_err(std::io::Error::other)?;

    match command {
        NoteCommand::Create {
            title,
            tags,
            sources,
            trust,
        } => {
            let msg = create_note(&db, &config.workspace, &title, &body, &tags, &sources, trust)
                .await?;
            println!("{msg}");
        }
        NoteCommand::Update {
            title,
            tags,
            sources,
            trust,
        } => {
            let msg = update_note(&db, &config.workspace, &title, &body, &tags, &sources, trust)
                .await?;
            println!("{msg}");
        }
    }

    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn create_note(
    db: &GhostDb,
    workspace: &std::path::Path,
    title: &str,
    body: &str,
    tags: &[String],
    sources: &[String],
    trust: i64,
) -> Result<String, GhostError> {
    let (sanitized_body, ref_warning) = sanitize_reference_links(workspace, body);

    let front = NoteFrontMatter {
        title: title.to_string(),
        archetype: Archetype::Entity,
        tags: tags.to_vec(),
        parent: None,
        sources: sources.to_vec(),
        trust,
        written_at: chrono::Utc::now().to_rfc3339(),
        updated_at: None,
    };

    let subfolder = knowledge::subfolder_from_tags(tags);
    let slug = knowledge::slug_from_title(title);
    let rel_path = knowledge::note_relative_path(subfolder, &slug);

    let path = knowledge::write_note(workspace, &front, &sanitized_body)
        .map_err(|e| std::io::Error::other(e.to_string()))?;

    // Ensure index notes exist for each level of the subfolder
    let mut index_info = String::new();
    if let Some(sub) = subfolder {
        let created = knowledge::ensure_index_notes(workspace, sub)
            .map_err(|e| std::io::Error::other(e.to_string()))?;
        if !created.is_empty() {
            let paths: Vec<String> = created
                .iter()
                .map(|p| p.display().to_string())
                .collect();
            index_info = format!(
                "\n  Skeleton index notes created:\n    {}\n  \
                 Edit them with a meaningful topic description for semantic search.",
                paths.join("\n    "),
            );
        }
    }

    let note_id = db::knowledge::create_note_full(
        db,
        title,
        &sanitized_body,
        tags,
        sources,
        trust,
        None,
        None,
        Some(&rel_path),
        None,
    )
    .await
    .map_err(|e| GhostError::Database(Box::new(e)))?;

    let wiki_links = extract_wiki_links(&sanitized_body);
    let result = reconcile_edges(db, &note_id, title, &wiki_links, None)
        .await
        .map_err(|e| std::io::Error::other(e.to_string()))?;

    let mut msg = format!(
        "Created note '{}' at {}\n\
         DB record: {}\n\
         Edges: {} created, {} stubs created{index_info}",
        title,
        path.display(),
        note_id,
        result.created,
        result.stubs_created,
    );
    if let Some(warning) = ref_warning {
        msg.push_str(&format!("\n\n{warning}"));
    }
    if wiki_links.is_empty() {
        msg.push_str(
            "\n\nHINT: This note has no [[wiki links]]. Consider adding links \
             to related entities to build the knowledge graph.",
        );
    }
    if !result.stub_titles.is_empty() {
        let stubs = result
            .stub_titles
            .iter()
            .map(|t| format!("  - [[{t}]]"))
            .collect::<Vec<_>>()
            .join("\n");
        msg.push_str(&format!(
            "\n\nNew stub notes created from wiki links:\n{stubs}\n\
             If any of these deserve a full note, create them before your handoff."
        ));
    }
    Ok(msg)
}

#[allow(clippy::too_many_arguments)]
async fn update_note(
    db: &GhostDb,
    workspace: &std::path::Path,
    title: &str,
    body: &str,
    tags: &[String],
    sources: &[String],
    trust: i64,
) -> Result<String, GhostError> {
    let (sanitized_body, ref_warning) = sanitize_reference_links(workspace, body);

    let existing = db::knowledge::find_note_by_title(db, title)
        .await
        .map_err(|e| GhostError::Database(Box::new(e)))?
        .ok_or_else(|| std::io::Error::other(format!("note '{title}' not found")))?;

    let subfolder = knowledge::subfolder_from_tags(tags);
    let slug = knowledge::slug_from_title(title);
    let rel_path = knowledge::note_relative_path(subfolder, &slug);

    // If the note moved to a different path, remove the old file
    if let Some(old_path) = &existing.path
        && *old_path != rel_path
    {
        let old_abs = workspace.join(old_path);
        if old_abs.exists() {
            let _ = std::fs::remove_file(&old_abs);
        }
    }

    db::knowledge::update_note(
        db,
        &existing.id,
        &sanitized_body,
        tags,
        sources,
        trust,
        None,
        None,
        Some(&rel_path),
        None,
    )
    .await
    .map_err(|e| GhostError::Database(Box::new(e)))?;

    let front = NoteFrontMatter {
        title: title.to_string(),
        archetype: Archetype::Entity,
        tags: tags.to_vec(),
        parent: None,
        sources: sources.to_vec(),
        trust,
        written_at: chrono::Utc::now().to_rfc3339(),
        updated_at: None,
    };
    let path = knowledge::write_note(workspace, &front, &sanitized_body)
        .map_err(|e| std::io::Error::other(e.to_string()))?;

    // Ensure index notes exist for each level of the subfolder
    if let Some(sub) = subfolder {
        if let Ok(created) = knowledge::ensure_index_notes(workspace, sub) {
            for p in &created {
                println!(
                    "  Skeleton index note created: {}\n  \
                     Edit it with a meaningful topic description for semantic search.",
                    p.display()
                );
            }
        }
    }

    let wiki_links = extract_wiki_links(&sanitized_body);
    let result = reconcile_edges(db, &existing.id, title, &wiki_links, None)
        .await
        .map_err(|e| std::io::Error::other(e.to_string()))?;

    let mut msg = format!(
        "Updated note '{}' at {}\n\
         Edges: {} created, {} deleted, {} stubs created",
        title,
        path.display(),
        result.created,
        result.deleted,
        result.stubs_created,
    );
    if let Some(warning) = ref_warning {
        msg.push_str(&format!("\n\n{warning}"));
    }
    if wiki_links.is_empty() {
        msg.push_str(
            "\n\nHINT: This note has no [[wiki links]]. Consider adding links \
             to related entities to build the knowledge graph.",
        );
    }
    if !result.stub_titles.is_empty() {
        let stubs = result
            .stub_titles
            .iter()
            .map(|t| format!("  - [[{t}]]"))
            .collect::<Vec<_>>()
            .join("\n");
        msg.push_str(&format!(
            "\n\nNew stub notes created from wiki links:\n{stubs}\n\
             If any of these deserve a full note, create them before your handoff."
        ));
    }
    Ok(msg)
}
