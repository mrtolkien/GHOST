use std::path::{Path, PathBuf};

use clap::Subcommand;

use crate::db;
use crate::error::GhostError;
use crate::reference_import::{ImportError, ImportProvenance};

#[derive(Debug, Subcommand)]
pub enum ReferenceCommand {
    /// Import markdown files or directories as references
    Import {
        /// Path to a markdown file or directory of markdown files
        path: PathBuf,
        /// Topic name (hierarchical, e.g., "dioxus/docs")
        #[arg(long)]
        topic: String,
        /// Source type (git, crawl, file)
        #[arg(long)]
        source_type: Option<String>,
        /// Source URL
        #[arg(long)]
        source_url: Option<String>,
        /// Version reference (e.g., git commit hash)
        #[arg(long)]
        version_ref: Option<String>,
        /// Git ref (branch or tag)
        #[arg(long)]
        git_ref: Option<String>,
        /// Comma-separated paths to include for git imports
        #[arg(long, value_delimiter = ',')]
        paths: Vec<String>,
        /// Comma-separated file extensions to include for git imports
        #[arg(long, value_delimiter = ',')]
        extensions: Vec<String>,
        /// Maximum BFS depth for crawl imports
        #[arg(long)]
        max_depth: Option<usize>,
        /// Maximum number of pages for crawl imports
        #[arg(long)]
        max_pages: Option<usize>,
    },
    /// Update references for a topic from its original source
    Update {
        /// Topic name (e.g. "dioxus/docs")
        #[arg(long)]
        topic: String,
        /// Override git ref (branch or tag) for this update
        #[arg(long = "ref")]
        git_ref: Option<String>,
    },
    /// Delete a topic and all its references
    Delete {
        #[arg(long)]
        topic: String,
    },
}

#[tracing::instrument(name = "execute reference_command", skip_all)]
pub async fn execute(command: ReferenceCommand) -> Result<(), GhostError> {
    let _observability = crate::observability::init()?;
    let config = crate::config::load()?;
    let db = crate::db::connect(&config.workspace, config.embeddings.dimension).await?;
    let workspace = Path::new(&config.workspace);

    match command {
        ReferenceCommand::Import {
            path,
            topic,
            source_type,
            source_url,
            version_ref,
            git_ref,
            paths,
            extensions,
            max_depth,
            max_pages,
        } => {
            let provenance = ImportProvenance {
                source_type,
                source_url,
                version_ref,
                git_ref,
                paths,
                extensions,
                max_depth,
                max_pages,
                no_ocr: None,
                page_range: None,
            };

            println!("Importing from: {}", path.display());
            println!("Topic: {topic}");

            let result = crate::reference_import::import_from_path(
                &db,
                workspace,
                &path,
                &topic,
                &provenance,
                None,
            )
            .await
            .map_err(|error| map_reference_import_error("import", &topic, error))?;

            print_import_result(&topic, &result);
            cleanup_staging(&path, workspace);
            Ok(())
        }
        ReferenceCommand::Update { topic, git_ref } => {
            println!("Updating references for topic: {topic}");
            let result = crate::reference_import::update_references(
                &db,
                workspace,
                &topic,
                git_ref.as_deref(),
            )
            .await
            .map_err(|error| map_reference_import_error("update", &topic, error))?;
            print_update_result(&topic, &result);
            Ok(())
        }
        ReferenceCommand::Delete { topic } => cmd_delete(&db, workspace, &topic).await,
    }
}

fn print_import_result(topic: &str, result: &crate::reference_import::ImportResult) {
    println!(
        "Done. Created: {}, Skipped: {}",
        result.references_created, result.references_skipped
    );
    if result.references_created > 0 {
        let ref_dir = format!("references/{topic}/");
        println!("References saved to: {ref_dir}");
        println!("Embeddings are being computed in the background by the file watcher.");
        println!(
            "\n  NOTE: A skeleton index note exists at notes/{topic}/index.md\n  \
             It may only contain a placeholder description.\n  \
             Edit it with a real description of what this library/topic is about —\n  \
             semantic search relies on this to discover the topic."
        );
    }
}

/// Remove the staging directory after a successful import.
///
/// If the imported path is inside `workspace/.staging/`, clean it up so
/// converted files don't linger after they have been ingested.
fn cleanup_staging(path: &Path, workspace: &Path) {
    let staging_root = workspace.join(".staging");
    if !path.starts_with(&staging_root) {
        return;
    }
    // For a directory, remove the directory itself.
    // For a file, remove its parent if it's a child of .staging/ (not .staging/ itself).
    let dir_to_remove = if path.is_dir() {
        path.to_path_buf()
    } else {
        path.parent()
            .filter(|p| p.starts_with(&staging_root) && *p != staging_root)
            .map(Path::to_path_buf)
            .unwrap_or_default()
    };
    if !dir_to_remove.as_os_str().is_empty() {
        let _ = std::fs::remove_dir_all(&dir_to_remove);
    }
}

fn print_update_result(_topic: &str, result: &crate::reference_import::UpdateResult) {
    if let (Some(old), Some(new)) = (&result.old_version_ref, &result.new_version_ref)
        && old != new
    {
        println!("Version: {old} → {new}");
    }
    println!(
        "Done. Created: {}, Updated: {}, Deleted: {}, Orphaned: {}, Unchanged: {}",
        result.created, result.updated, result.deleted, result.orphaned, result.unchanged
    );
    if result.created + result.updated > 0 {
        println!("Embeddings are being re-computed in the background by the file watcher.");
    }
}

fn map_reference_import_error(action: &str, topic: &str, error: ImportError) -> GhostError {
    match &error {
        ImportError::Config(message)
            if message.contains("_import.toml") || message.contains("repair-critical") =>
        {
            GhostError::Other(format!(
                "reference {action} failed for topic '{topic}': {message}"
            ))
        }
        _ => GhostError::Import(error),
    }
}

async fn cmd_delete(
    db: &db::GhostDb,
    workspace: &std::path::Path,
    topic_name: &str,
) -> Result<(), GhostError> {
    let topic = db::knowledge::find_topic_by_name(db, topic_name)
        .await
        .map_err(|e| GhostError::Database(Box::new(e)))?;

    let Some(topic) = topic else {
        println!("Topic '{topic_name}' not found.");
        return Ok(());
    };

    let ref_count = db::knowledge::count_references_by_topic(db, &topic.id)
        .await
        .map_err(|e| GhostError::Database(Box::new(e)))?;

    println!("Deleting topic '{topic_name}' ({ref_count} references)...");

    // Delete DB records: references + embeddings (cascaded), import batch, topic
    db::knowledge::delete_references_by_topic(db, &topic.id)
        .await
        .map_err(|e| GhostError::Database(Box::new(e)))?;

    db::knowledge::delete_import_batch(db, &topic.id)
        .await
        .map_err(|e| GhostError::Database(Box::new(e)))?;

    db::knowledge::delete_topic(db, &topic.id)
        .await
        .map_err(|e| GhostError::Database(Box::new(e)))?;

    // Clean up workspace files
    let ref_dir = workspace.join("references").join(topic_name);
    if ref_dir.exists() {
        std::fs::remove_dir_all(&ref_dir).ok();
        println!("  Removed {}", ref_dir.display());
    }

    let note_dir = workspace.join("notes").join(topic_name);
    if note_dir.exists() {
        std::fs::remove_dir_all(&note_dir).ok();
        println!("  Removed {}", note_dir.display());
    }

    println!("Done.");

    Ok(())
}
