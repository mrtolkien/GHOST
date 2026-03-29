use clap::Subcommand;

use crate::db;
use crate::error::GhostError;

#[derive(Debug, Subcommand)]
pub enum TopicsCommand {
    /// List all topics with note and reference counts
    List,

    /// Search topics by name
    Search {
        /// Search query
        query: String,
    },
}

#[tracing::instrument(skip_all)]
pub async fn execute(command: TopicsCommand) -> Result<(), GhostError> {
    let config = crate::config::load()?;
    let db = crate::db::connect(&config.workspace, config.embeddings.dimension).await?;

    match command {
        TopicsCommand::List => cmd_list(&db).await,
        TopicsCommand::Search { query } => cmd_search(&db, &query).await,
    }
}

async fn cmd_list(db: &db::GhostDb) -> Result<(), GhostError> {
    let topics = db::knowledge::list_topics(db)
        .await
        .map_err(|e| GhostError::Database(Box::new(e)))?;

    if topics.is_empty() {
        println!("No topics found.");
        return Ok(());
    }

    for t in &topics {
        let mut counts = Vec::new();
        if t.note_count > 0 {
            counts.push(format!("{} notes", t.note_count));
        }
        if t.ref_count > 0 {
            counts.push(format!("{} refs", t.ref_count));
        }
        let counts_str = if counts.is_empty() {
            "empty".into()
        } else {
            counts.join(", ")
        };

        println!("{:<30} {counts_str}", t.name);
    }

    Ok(())
}

async fn cmd_search(db: &db::GhostDb, query: &str) -> Result<(), GhostError> {
    let hits = db::knowledge::search_topics(db, query, 10)
        .await
        .map_err(|e| GhostError::Database(Box::new(e)))?;

    if hits.is_empty() {
        println!("No matching topics.");
        return Ok(());
    }

    for hit in &hits {
        println!("{:<30} (score: {:.2})", hit.title, hit.score);
    }

    Ok(())
}
