use std::collections::{HashSet, VecDeque};
use std::path::Path;

use scraper::{Html, Selector};
use url::Url;

use crate::config::EmbeddingsConfig;
use crate::db;
use crate::db::GhostDb;
use crate::embeddings::EmbeddingClient;
use crate::embeddings::pipeline::{EmbedRequest, embed_sources};
use crate::web;

use super::topic::ensure_topic_hierarchy;
use super::types::{ImportConfig, ImportError, ImportResult, ImportSource};

/// Import references by BFS-crawling a website, following same-host links.
pub async fn import_crawl(
    db: &GhostDb,
    workspace: &Path,
    embeddings_config: &EmbeddingsConfig,
    config: &ImportConfig,
) -> Result<ImportResult, ImportError> {
    let ImportSource::Crawl {
        url: seed_url,
        max_depth,
        max_pages,
    } = &config.source
    else {
        return Err(ImportError::Fetch("expected crawl source".into()));
    };

    let seed =
        Url::parse(seed_url).map_err(|e| ImportError::Fetch(format!("invalid seed URL: {e}")))?;
    let seed_host = seed
        .host_str()
        .ok_or_else(|| ImportError::Fetch("seed URL has no host".into()))?
        .to_string();

    // Ensure topic hierarchy
    let topic_id = ensure_topic_hierarchy(db, &config.topic).await?;

    // Upsert import batch with placeholder count
    let batch_id =
        db::knowledge::upsert_import_batch(db, &topic_id, "crawl", seed_url, None, 0).await?;

    let mut queue: VecDeque<(Url, usize)> = VecDeque::new();
    let mut visited: HashSet<String> = HashSet::new();
    let mut embed_requests: Vec<EmbedRequest> = Vec::new();
    let mut created = 0usize;
    let mut skipped = 0usize;

    queue.push_back((seed, 0));

    let link_selector = Selector::parse("a[href]").expect("static selector should parse");

    while let Some((url, depth)) = queue.pop_front() {
        let normalized = normalize_url(&url);
        if visited.contains(&normalized) || depth > *max_depth || created + skipped >= *max_pages {
            continue;
        }
        visited.insert(normalized.clone());

        // Idempotency: skip if already imported
        if db::knowledge::find_reference_by_path(db, &normalized)
            .await?
            .is_some()
        {
            skipped += 1;
            continue;
        }

        // Fetch raw HTML for link extraction
        let (html, final_url) = match web::fetch_raw(url.as_str()).await {
            Ok(r) => r,
            Err(e) => {
                logfire::warn!(
                    "crawl: failed to fetch page",
                    url = url.as_str().to_string(),
                    error = e.to_string(),
                );
                continue;
            }
        };

        // Extract same-host links and enqueue
        if depth < *max_depth {
            let base = Url::parse(&final_url).unwrap_or(url.clone());
            let doc = Html::parse_document(&html);
            for element in doc.select(&link_selector) {
                let Some(href) = element.value().attr("href") else {
                    continue;
                };
                let Ok(resolved) = base.join(href) else {
                    continue;
                };
                if resolved.host_str() != Some(&seed_host) {
                    continue;
                }
                let norm = normalize_url(&resolved);
                if !visited.contains(&norm) {
                    queue.push_back((resolved, depth + 1));
                }
            }
        }

        // Convert HTML to markdown via the standard fetch pipeline
        let extracted = match web::fetch(url.as_str(), &web::FetchOptions::default(), None).await {
            Ok(e) => e,
            Err(e) => {
                logfire::warn!(
                    "crawl: markdown extraction failed",
                    url = url.as_str().to_string(),
                    error = e.to_string(),
                );
                continue;
            }
        };

        // Store as reference
        let ref_id = db::knowledge::create_reference(
            db,
            &topic_id,
            &normalized,
            &extracted.text,
            Some(url.as_str()),
            Some(&batch_id),
        )
        .await?;

        embed_requests.push(EmbedRequest {
            source_table: "reference".into(),
            source_id: ref_id,
            content: extracted.text,
            tags: vec![config.topic.clone()],
            topic_id: Some(topic_id.clone()),
            path: None,
        });

        created += 1;

        // Small delay between fetches to be polite
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    }

    // Batch embed
    let client = EmbeddingClient::new(embeddings_config);
    let embeddings_generated = embed_sources(&client, db, embed_requests).await?;

    // Update import batch with final count
    let total_refs = db::knowledge::count_references_by_topic(db, &topic_id).await? as usize;
    let batch_id = db::knowledge::upsert_import_batch(
        db,
        &topic_id,
        "crawl",
        seed_url,
        None,
        total_refs as i64,
    )
    .await?;

    // Write _import.toml and ensure index notes
    super::topic::write_import_toml(
        workspace,
        &config.topic,
        "crawl",
        seed_url,
        None,
        total_refs,
    )?;

    Ok(ImportResult {
        topic_id,
        batch_id,
        references_created: created,
        references_skipped: skipped,
        embeddings_generated,
    })
}

/// Normalize a URL by stripping fragment and query params for dedup.
fn normalize_url(url: &Url) -> String {
    let mut normalized = url.clone();
    normalized.set_fragment(None);
    normalized.set_query(None);
    // Remove trailing slash for consistency
    let s = normalized.to_string();
    s.strip_suffix('/').unwrap_or(&s).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_strips_fragment_and_query() {
        let url = Url::parse("https://example.com/page?foo=bar#section").unwrap();
        assert_eq!(normalize_url(&url), "https://example.com/page");
    }

    #[test]
    fn normalize_strips_trailing_slash() {
        let url = Url::parse("https://example.com/page/").unwrap();
        assert_eq!(normalize_url(&url), "https://example.com/page");
    }

    #[test]
    fn normalize_preserves_path() {
        let url = Url::parse("https://example.com/docs/getting-started").unwrap();
        assert_eq!(
            normalize_url(&url),
            "https://example.com/docs/getting-started"
        );
    }
}
