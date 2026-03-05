use clap::Subcommand;

use crate::db;
use crate::error::GhostError;
use crate::reference_import::{ImportConfig, ImportSource};

#[derive(Debug, Subcommand)]
pub enum ReferenceCommand {
    /// Import references from a git repo, web page, crawl, or local file
    Import {
        /// Source type: "git", "page", "crawl", or "file"
        #[arg(long)]
        source: String,

        /// URL to import from (not needed for file imports)
        #[arg(long)]
        url: Option<String>,

        /// Local file path (file import only)
        #[arg(long)]
        path: Option<String>,

        /// Topic name (hierarchical, e.g. "dioxus/docs")
        #[arg(long)]
        topic: String,

        /// Subdirectories to import (git only, comma-separated)
        #[arg(long, value_delimiter = ',')]
        paths: Vec<String>,

        /// File extensions to include (git only, e.g. ".md,.rs")
        #[arg(long, value_delimiter = ',')]
        extensions: Vec<String>,

        /// Max link depth for crawl (default: 3)
        #[arg(long, default_value_t = 3)]
        max_depth: usize,

        /// Max pages to crawl (default: 50)
        #[arg(long, default_value_t = 50)]
        max_pages: usize,
    },

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
            path,
            topic,
            paths,
            extensions,
            max_depth,
            max_pages,
        } => {
            cmd_import(
                &db,
                &config,
                &source,
                url.as_deref(),
                path.as_deref(),
                &topic,
                &paths,
                &extensions,
                max_depth,
                max_pages,
            )
            .await
        }
        ReferenceCommand::Delete { topic } => {
            cmd_delete(&db, std::path::Path::new(&config.workspace), &topic).await
        }
    }
}

#[allow(clippy::too_many_arguments)]
#[tracing::instrument(name = "import references", skip_all, fields(source, topic))]
async fn cmd_import(
    db: &db::GhostDb,
    config: &crate::config::Config,
    source: &str,
    url: Option<&str>,
    path: Option<&str>,
    topic: &str,
    paths: &[String],
    extensions: &[String],
    max_depth: usize,
    max_pages: usize,
) -> Result<(), GhostError> {
    let require_url = || -> Result<&str, GhostError> {
        url.ok_or_else(|| {
            GhostError::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "--url is required for git/page/crawl imports",
            ))
        })
    };

    let import_config = ImportConfig {
        source: match source {
            "git" => ImportSource::Git {
                url: require_url()?.to_string(),
                paths: paths.to_vec(),
                extensions: extensions.to_vec(),
            },
            "page" => ImportSource::Page {
                url: require_url()?.to_string(),
            },
            "crawl" => ImportSource::Crawl {
                url: require_url()?.to_string(),
                max_depth,
                max_pages,
            },
            "file" => {
                let path = path.ok_or_else(|| {
                    GhostError::Io(std::io::Error::new(
                        std::io::ErrorKind::InvalidInput,
                        "--path is required for file imports",
                    ))
                })?;
                ImportSource::File {
                    path: path.to_string(),
                }
            }
            other => {
                return Err(GhostError::Io(std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    format!(
                        "unknown source type: {other} (expected 'git', 'page', 'crawl', or 'file')"
                    ),
                )));
            }
        },
        topic: topic.to_string(),
    };

    let workspace = std::path::Path::new(&config.workspace);

    match &import_config.source {
        ImportSource::File { path } => println!("Importing file: {path}"),
        _ => println!("Importing from {source}: {}", url.unwrap_or("<no url>")),
    }
    println!("Topic: {topic}");

    let result = match &import_config.source {
        ImportSource::Git { .. } => {
            crate::reference_import::import_git(db, workspace, &import_config).await?
        }
        ImportSource::Page { .. } => {
            crate::reference_import::import_page(db, workspace, &config.web, &import_config).await?
        }
        ImportSource::Crawl { .. } => {
            crate::reference_import::import_crawl(db, workspace, &import_config).await?
        }
        ImportSource::File { .. } => {
            crate::reference_import::import_file(db, workspace, &config.web, &import_config).await?
        }
    };

    println!(
        "Done. Created: {}, Skipped: {}",
        result.references_created, result.references_skipped
    );

    if result.references_created > 0 {
        println!(
            "\n  WARNING: A skeleton index note exists at notes/{topic}/index.md\n  \
             It may only contain a placeholder description.\n  \
             Edit it with a real description of what this library/topic is about —\n  \
             semantic search relies on this to discover the topic."
        );
    }

    Ok(())
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
