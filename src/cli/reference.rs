use clap::Subcommand;

use crate::db;
use crate::error::GhostError;
use crate::reference_import::{ImportConfig, ImportSource};

#[derive(Debug, Subcommand)]
pub enum ReferenceCommand {
    /// Import references from a git repo or web page
    Import {
        /// Source type: "git" or "page"
        #[arg(long)]
        source: String,

        /// URL to import from
        #[arg(long)]
        url: String,

        /// Topic name (hierarchical, e.g. "dioxus/docs")
        #[arg(long)]
        topic: String,

        /// Subdirectories to import (git only, comma-separated)
        #[arg(long, value_delimiter = ',')]
        paths: Vec<String>,

        /// File extensions to include (git only, e.g. ".md,.rs")
        #[arg(long, value_delimiter = ',')]
        extensions: Vec<String>,
    },

    /// List all topics with reference counts
    Topics,

    /// Delete a topic and all its references
    Delete {
        /// Topic name to delete
        #[arg(long)]
        topic: String,
    },
}

#[tracing::instrument(skip_all)]
pub async fn execute(command: ReferenceCommand) -> Result<(), GhostError> {
    let config = crate::config::load()?;
    crate::config_workspace::bootstrap_workspace(&config)?;
    let db = crate::db::connect(&config.workspace, config.embeddings.dimension).await?;

    match command {
        ReferenceCommand::Import {
            source,
            url,
            topic,
            paths,
            extensions,
        } => cmd_import(&db, &config, &source, &url, &topic, &paths, &extensions).await,
        ReferenceCommand::Topics => cmd_topics(&db).await,
        ReferenceCommand::Delete { topic } => cmd_delete(&db, &topic).await,
    }
}

async fn cmd_import(
    db: &db::GhostDb,
    config: &crate::config::Config,
    source: &str,
    url: &str,
    topic: &str,
    paths: &[String],
    extensions: &[String],
) -> Result<(), GhostError> {
    let import_config = ImportConfig {
        source: match source {
            "git" => ImportSource::Git {
                url: url.to_string(),
                paths: paths.to_vec(),
                extensions: extensions.to_vec(),
            },
            "page" => ImportSource::Page {
                url: url.to_string(),
            },
            other => {
                return Err(GhostError::Io(std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    format!("unknown source type: {other} (expected 'git' or 'page')"),
                )));
            }
        },
        topic: topic.to_string(),
    };

    let workspace = std::path::Path::new(&config.workspace);

    println!("Importing from {source}: {url}");
    println!("Topic: {topic}");

    let result = match &import_config.source {
        ImportSource::Git { .. } => {
            crate::reference_import::import_git(db, workspace, &config.embeddings, &import_config)
                .await?
        }
        ImportSource::Page { .. } => {
            crate::reference_import::import_page(db, workspace, &config.embeddings, &import_config)
                .await?
        }
    };

    println!(
        "Done. Created: {}, Skipped: {}, Embeddings: {}",
        result.references_created, result.references_skipped, result.embeddings_generated
    );

    Ok(())
}

async fn cmd_topics(db: &db::GhostDb) -> Result<(), GhostError> {
    let topics = db::knowledge::list_topics(db)
        .await
        .map_err(|e| GhostError::Database(Box::new(e)))?;

    if topics.is_empty() {
        println!("No topics found.");
        return Ok(());
    }

    println!(
        "{:<40} {:>6}  {:<8} {}",
        "Topic", "Refs", "Source", "Version"
    );
    println!("{}", "-".repeat(80));
    for t in &topics {
        let batch = db::knowledge::get_import_batch_by_topic(db, &t.id)
            .await
            .map_err(|e| GhostError::Database(Box::new(e)))?;
        let (source_type, version) = match &batch {
            Some(b) => (
                b.source_type.as_str(),
                b.version_ref
                    .as_deref()
                    .map(|v| &v[..v.len().min(8)])
                    .unwrap_or("-"),
            ),
            None => ("-", "-"),
        };
        println!(
            "{:<40} {:>6}  {:<8} {}",
            t.name, t.ref_count, source_type, version
        );
    }
    println!("\n{} topics total.", topics.len());

    Ok(())
}

async fn cmd_delete(db: &db::GhostDb, topic_name: &str) -> Result<(), GhostError> {
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

    db::knowledge::delete_references_by_topic(db, &topic.id)
        .await
        .map_err(|e| GhostError::Database(Box::new(e)))?;

    db::knowledge::delete_topic(db, &topic.id)
        .await
        .map_err(|e| GhostError::Database(Box::new(e)))?;

    println!("Done.");

    Ok(())
}
