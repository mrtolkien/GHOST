use clap::Subcommand;

use crate::db;
use crate::error::GhostError;
use crate::reference_import::{ImportConfig, ImportSource};

#[derive(Debug, Subcommand)]
pub enum ReferenceCommand {
    /// Import references from external sources
    Import {
        #[command(subcommand)]
        command: ReferenceImportCommand,
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

#[derive(Debug, Subcommand)]
pub enum ReferenceImportCommand {
    /// Import from a git repository
    Git {
        #[arg(long)]
        url: String,
        #[arg(long)]
        topic: String,
        #[arg(long, value_delimiter = ',')]
        paths: Vec<String>,
        #[arg(long, value_delimiter = ',')]
        extensions: Vec<String>,
        /// Pin import to a specific branch or tag
        #[arg(long = "ref")]
        git_ref: Option<String>,
    },
    /// Import by crawling a website
    Crawl {
        #[arg(long)]
        url: String,
        #[arg(long)]
        topic: String,
        #[arg(long, default_value_t = 3)]
        max_depth: usize,
        #[arg(long, default_value_t = 50)]
        max_pages: usize,
    },
}

#[tracing::instrument(name = "execute reference_command", skip_all)]
pub async fn execute(command: ReferenceCommand) -> Result<(), GhostError> {
    let _observability = crate::observability::init()?;
    let config = crate::config::load()?;
    crate::config_workspace::bootstrap_workspace(&config)?;
    let db = crate::db::connect(&config.workspace, config.embeddings.dimension).await?;
    let workspace = std::path::Path::new(&config.workspace);

    match command {
        ReferenceCommand::Import { command } => match command {
            ReferenceImportCommand::Git {
                url,
                topic,
                paths,
                extensions,
                git_ref,
            } => {
                let import_config = ImportConfig {
                    source: ImportSource::Git {
                        url: url.clone(),
                        paths,
                        extensions,
                        git_ref,
                    },
                    topic: topic.clone(),
                };
                println!("Importing from git: {url}");
                println!("Topic: {topic}");
                let result =
                    crate::reference_import::import_git(&db, workspace, &import_config).await?;
                print_result(&topic, "git", result);
                Ok(())
            }
            ReferenceImportCommand::Crawl {
                url,
                topic,
                max_depth,
                max_pages,
            } => {
                let import_config = ImportConfig {
                    source: ImportSource::Crawl {
                        url: url.clone(),
                        max_depth,
                        max_pages,
                    },
                    topic: topic.clone(),
                };
                println!("Importing from crawl: {url}");
                println!("Topic: {topic}");
                let result =
                    crate::reference_import::import_crawl(&db, workspace, &import_config).await?;
                print_result(&topic, "crawl", result);
                Ok(())
            }
        },
        ReferenceCommand::Update { topic, git_ref } => {
            println!("Updating references for topic: {topic}");
            let result = crate::reference_import::update_references(
                &db,
                workspace,
                &topic,
                git_ref.as_deref(),
            )
            .await?;
            print_update_result(&topic, &result);
            Ok(())
        }
        ReferenceCommand::Delete { topic } => cmd_delete(&db, workspace, &topic).await,
    }
}

fn print_result(topic: &str, source: &str, result: crate::reference_import::ImportResult) {
    println!(
        "Done. Created: {}, Skipped: {}",
        result.references_created, result.references_skipped
    );
    if result.references_created > 0 {
        let ref_dir = format!("references/{topic}/");
        match source {
            "page" | "file" | "url" => println!("Reference saved to: {ref_dir}"),
            _ => println!("References saved to: {ref_dir}"),
        }
        println!("Embeddings are being computed in the background by the file watcher.");
        println!(
            "\n  NOTE: A skeleton index note exists at notes/{topic}/index.md\n  \
             It may only contain a placeholder description.\n  \
             Edit it with a real description of what this library/topic is about —\n  \
             semantic search relies on this to discover the topic."
        );
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

async fn cmd_delete(
    db: &db::GhostDb,
    workspace: &std::path::Path,
    topic_name: &str,
) -> Result<(), GhostError> {
    let topic = db::knowledge::find_topic_by_name(db, topic_name)
        .await
        .map_err(|e| GhostError::Database(Box::new(e)))?;

    let topic = match topic {
        Some(t) => t,
        None => {
            println!("Topic '{topic_name}' not found.");
            return Ok(());
        }
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
