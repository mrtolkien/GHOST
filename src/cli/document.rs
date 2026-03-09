use clap::Subcommand;

use crate::db;
use crate::error::GhostError;
use crate::reference_import::{ImportConfig, ImportSource};

#[derive(Debug, Subcommand)]
pub enum DocumentCommand {
    /// Import a document (PDF, DOCX, etc.)
    Import {
        #[command(subcommand)]
        command: DocumentImportCommand,
    },
}

#[derive(Debug, Subcommand)]
pub enum DocumentImportCommand {
    /// Import a document from a URL
    Url {
        #[arg(long)]
        url: String,
        #[arg(long)]
        topic: String,
        /// Disable OCR (faster for digital PDFs)
        #[arg(long, default_value_t = false)]
        no_ocr: bool,
        /// Page range, e.g. "1-10" (default: full document)
        #[arg(long)]
        page_range: Option<String>,
        /// Timeout in seconds (default: from config, usually 600)
        #[arg(long)]
        timeout: Option<u64>,
    },
    /// Import a document from a local file
    File {
        #[arg(long)]
        path: String,
        #[arg(long)]
        topic: String,
        /// Disable OCR (faster for digital PDFs)
        #[arg(long, default_value_t = false)]
        no_ocr: bool,
        /// Page range, e.g. "1-10" (default: full document)
        #[arg(long)]
        page_range: Option<String>,
        /// Timeout in seconds (default: from config, usually 600)
        #[arg(long)]
        timeout: Option<u64>,
    },
}

#[tracing::instrument(name = "execute document_command", skip_all)]
pub async fn execute(command: DocumentCommand) -> Result<(), GhostError> {
    let _observability = crate::observability::init()?;
    let config = crate::config::load()?;
    crate::config_workspace::bootstrap_workspace(&config)?;
    let db = crate::db::connect(&config.workspace, config.embeddings.dimension).await?;
    let workspace = std::path::Path::new(&config.workspace);

    match command {
        DocumentCommand::Import { command } => match command {
            DocumentImportCommand::Url {
                url,
                topic,
                no_ocr,
                page_range,
                timeout,
            } => {
                let page_range = parse_page_range(page_range.as_deref())?;
                let mut docling_config = config.docling.clone();
                if let Some(t) = timeout {
                    docling_config.timeout = t;
                }
                let import_config = ImportConfig {
                    source: ImportSource::Page {
                        url: url.clone(),
                        no_ocr,
                        page_range,
                    },
                    topic: topic.clone(),
                };
                println!("Importing document from URL: {url}");
                println!("Topic: {topic}");
                let result = crate::reference_import::import_page(
                    &db,
                    workspace,
                    &docling_config,
                    &import_config,
                )
                .await?;
                print_result(&topic, result);
                Ok(())
            }
            DocumentImportCommand::File {
                path,
                topic,
                no_ocr,
                page_range,
                timeout,
            } => {
                let page_range = parse_page_range(page_range.as_deref())?;
                let mut docling_config = config.docling.clone();
                if let Some(t) = timeout {
                    docling_config.timeout = t;
                }
                let import_config = ImportConfig {
                    source: ImportSource::File {
                        path: path.clone(),
                        no_ocr,
                        page_range,
                    },
                    topic: topic.clone(),
                };
                println!("Importing document from file: {path}");
                println!("Topic: {topic}");
                let result = crate::reference_import::import_file(
                    &db,
                    workspace,
                    &docling_config,
                    &import_config,
                )
                .await?;
                print_result(&topic, result);
                Ok(())
            }
        },
    }
}

fn parse_page_range(s: Option<&str>) -> Result<Option<(u32, u32)>, GhostError> {
    let Some(s) = s else { return Ok(None) };
    let parts: Vec<&str> = s.split('-').collect();
    if parts.len() != 2 {
        return Err(GhostError::Io(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("invalid page range '{s}', expected format: '1-10'"),
        )));
    }
    let start: u32 = parts[0].trim().parse().map_err(|_| {
        GhostError::Io(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("invalid page range start: '{}'", parts[0]),
        ))
    })?;
    let end: u32 = parts[1].trim().parse().map_err(|_| {
        GhostError::Io(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("invalid page range end: '{}'", parts[1]),
        ))
    })?;
    Ok(Some((start, end)))
}

fn print_result(topic: &str, result: crate::reference_import::ImportResult) {
    println!(
        "Done. Created: {}, Skipped: {}",
        result.references_created, result.references_skipped
    );
    if result.references_created > 0 {
        let ref_dir = format!("references/{topic}/");
        println!("Reference saved to: {ref_dir}");
        println!("Embeddings are being computed in the background by the file watcher.");
        println!(
            "\n  NOTE: A skeleton index note exists at notes/{topic}/index.md\n  \
             It may only contain a placeholder description.\n  \
             Edit it with a real description of what this library/topic is about —\n  \
             semantic search relies on this to discover the topic."
        );
    }
}
