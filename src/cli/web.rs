use clap::Subcommand;

use crate::config::SearchProviderConfig;
use crate::error::GhostError;
use crate::web::{self, BraveSearchProvider, SearxngSearchProvider, format_search_metadata};

#[derive(Debug, Subcommand)]
pub enum WebCommand {
    /// Search the web (Brave or SearXNG, based on config)
    Search {
        query: String,
        /// Maximum number of results
        #[arg(short = 'n', long)]
        max_results: Option<usize>,
    },
    /// Fetch and extract content from a URL
    Fetch {
        url: String,
        /// Use Mozilla Readability to extract article content only
        /// (strips navigation, sidebars, etc. — best for single articles)
        #[arg(long, conflicts_with = "raw")]
        readability: bool,
        /// Output raw HTML without conversion
        #[arg(long, conflicts_with = "readability")]
        raw: bool,
    },
}

#[tracing::instrument(skip_all)]
pub async fn execute(command: WebCommand) -> Result<(), GhostError> {
    let config = crate::config::load()?;

    match command {
        WebCommand::Search { query, max_results } => {
            let max = max_results.unwrap_or(config.web.search_max_results);

            let results = match &config.web.search_provider {
                SearchProviderConfig::Brave => {
                    let api_key = std::env::var("BRAVE_API_KEY").map_err(|_| {
                        crate::web::WebError::MissingApiKey {
                            name: "BRAVE_API_KEY",
                        }
                    })?;
                    let provider = BraveSearchProvider::new(&api_key, max)?;
                    provider.search(&query).await?
                }
                SearchProviderConfig::Searxng { url } => {
                    let provider = SearxngSearchProvider::new(url, max)?;
                    provider.search(&query).await?
                }
            };

            if let Err(e) = web::save_search_cache(&config.workspace, &query, &results) {
                logfire::warn!("failed to cache search results", error = e.to_string(),);
            }

            for (i, result) in results.iter().enumerate() {
                println!("{}. {}", i + 1, result.title);
                println!("   {}", result.url);
                if let Some(snippet) = &result.snippet {
                    println!("   {snippet}");
                }
                if let Some(meta) = format_search_metadata(result) {
                    println!("   {meta}");
                }
                println!();
            }
        }
        WebCommand::Fetch {
            url,
            readability,
            raw,
        } => {
            let options = web::FetchOptions { readability, raw };
            let content = web::fetch(&url, &options, config.web.crawl4ai_url.as_deref()).await?;

            match web::save_fetch_cache(&config.workspace, &url, &content) {
                Ok(path) => {
                    eprintln!("Cached to: {}", path.display());
                }
                Err(e) => {
                    logfire::warn!("failed to cache fetch result", error = e.to_string(),);
                }
            }

            if let Some(title) = &content.title {
                println!("# {title}\n");
            }
            print!("{}", content.text);
        }
    }

    Ok(())
}
