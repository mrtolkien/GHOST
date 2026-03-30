use std::path::PathBuf;
use std::sync::Arc;

use clap::Subcommand;

use crate::convert::crawl::CrawlConvertResult;
use crate::convert::git::GitConvertResult;
use crate::convert::pdf::{PdfConvertResult, VisionFallback};
use crate::error::GhostError;

/// Default staging root directory name within the workspace.
const STAGING_DIR: &str = ".staging";

/// Default max crawl depth (BFS levels from the seed URL).
const DEFAULT_MAX_DEPTH: usize = 3;

/// Default max number of pages to crawl.
const DEFAULT_MAX_PAGES: usize = 50;

/// Convert sources to markdown for inspection before import.
#[derive(Debug, Subcommand)]
pub enum ConvertCommand {
    /// Convert a PDF file to markdown via docling
    Pdf {
        /// Path to the PDF file
        path: String,
        /// Disable OCR (faster for digital PDFs)
        #[arg(long, default_value_t = false)]
        no_ocr: bool,
        /// Page range, e.g. "1-10" (default: full document)
        #[arg(long)]
        page_range: Option<String>,
        /// Timeout in seconds (default: from config)
        #[arg(long)]
        timeout: Option<u64>,
        /// Output directory for staging (default: <workspace>/.staging)
        #[arg(long)]
        output: Option<PathBuf>,
    },
    /// Clone a git repository and extract matching files
    Git {
        /// Repository URL
        url: String,
        /// Comma-separated paths to include (e.g. "docs/,src/")
        #[arg(long, value_delimiter = ',')]
        paths: Vec<String>,
        /// Comma-separated file extensions to include (e.g. ".md,.rst")
        #[arg(long, value_delimiter = ',')]
        extensions: Vec<String>,
        /// Git ref to check out (branch, tag, or commit)
        #[arg(long, name = "ref")]
        git_ref: Option<String>,
        /// Output directory for staging (default: <workspace>/.staging)
        #[arg(long)]
        output: Option<PathBuf>,
    },
    /// BFS-crawl a website and convert pages to markdown
    Crawl {
        /// Seed URL to start crawling from
        url: String,
        /// Maximum BFS depth from the seed URL
        #[arg(long, default_value_t = DEFAULT_MAX_DEPTH)]
        max_depth: usize,
        /// Maximum number of pages to crawl
        #[arg(long, default_value_t = DEFAULT_MAX_PAGES)]
        max_pages: usize,
        /// Output directory for staging (default: <workspace>/.staging)
        #[arg(long)]
        output: Option<PathBuf>,
    },
}

#[tracing::instrument(name = "execute convert_command", skip_all)]
pub async fn execute(command: ConvertCommand) -> Result<(), GhostError> {
    let _observability = crate::observability::init()?;
    let config = crate::config::load()?;
    let workspace = PathBuf::from(&config.workspace);

    match command {
        ConvertCommand::Pdf {
            path,
            no_ocr,
            page_range,
            timeout,
            output,
        } => {
            let staging_root = staging_root(&workspace, output.as_deref());
            let page_range = parse_page_range(page_range.as_deref())?;
            let mut docling_config = config.docling.clone();
            if let Some(t) = timeout {
                docling_config.timeout = t;
            }

            let vision = resolve_vision_fallback(&config);

            let result = crate::convert::pdf::convert_pdf(
                &staging_root,
                std::path::Path::new(&path),
                &workspace,
                &docling_config,
                no_ocr,
                page_range,
                vision,
            )
            .await
            .map_err(convert_err)?;

            print_pdf_result(&result);
            Ok(())
        }
        ConvertCommand::Git {
            url,
            paths,
            extensions,
            git_ref,
            output,
        } => {
            let staging_root = staging_root(&workspace, output.as_deref());

            let result = crate::convert::git::convert_git(
                &staging_root,
                &url,
                &paths,
                &extensions,
                git_ref.as_deref(),
            )
            .await
            .map_err(convert_err)?;

            print_git_result(&result, git_ref.as_deref());
            Ok(())
        }
        ConvertCommand::Crawl {
            url,
            max_depth,
            max_pages,
            output,
        } => {
            let staging_root = staging_root(&workspace, output.as_deref());

            let result = crate::convert::crawl::convert_crawl(
                &staging_root,
                &url,
                max_depth,
                max_pages,
            )
            .await
            .map_err(convert_err)?;

            print_crawl_result(&result);
            Ok(())
        }
    }
}

/// Resolve the staging root directory, using the explicit output path or
/// falling back to `<workspace>/.staging`.
fn staging_root(
    workspace: &std::path::Path,
    output: Option<&std::path::Path>,
) -> PathBuf {
    output
        .map(PathBuf::from)
        .unwrap_or_else(|| workspace.join(STAGING_DIR))
}

/// Map `ConvertError` into `GhostError`.
fn convert_err(e: crate::convert::error::ConvertError) -> GhostError {
    GhostError::Other(e.to_string())
}

/// Build a `VisionFallback` from config if a vision alias is configured.
fn resolve_vision_fallback(config: &crate::config::Config) -> Option<VisionFallback> {
    let vision_alias = config
        .models
        .vision
        .as_deref()
        .unwrap_or(&config.models.default);

    let provider: Arc<dyn crate::providers::types::Provider> =
        crate::providers::types::provider_for_alias(config, Some(vision_alias)).ok()?;
    let model = config
        .models
        .aliases
        .get(vision_alias)
        .map(|m| m.model.clone())?;

    Some(VisionFallback { provider, model })
}

/// Parse a page range string like "1-10" into `(start, end)`.
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

/// Print stdout metadata for a PDF conversion result.
fn print_pdf_result(result: &PdfConvertResult) {
    println!("{}", result.staging_dir.display());
    println!("source_type=pdf");
    println!("markdown_file={}", result.markdown_file);
}

/// Print stdout metadata for a git conversion result.
fn print_git_result(result: &GitConvertResult, git_ref: Option<&str>) {
    println!("{}", result.staging_dir.display());
    println!("source_type=git");
    println!("source_url={}", result.source_url);
    println!("version_ref={}", result.version_ref);
    if let Some(r) = git_ref {
        println!("git_ref={r}");
    }
}

/// Print stdout metadata for a crawl conversion result.
fn print_crawl_result(result: &CrawlConvertResult) {
    println!("{}", result.staging_dir.display());
    println!("source_type=crawl");
    println!("source_url={}", result.source_url);
    println!("pages={}", result.page_urls.len());
}
