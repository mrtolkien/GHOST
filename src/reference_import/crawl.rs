use std::collections::{HashSet, VecDeque};
use std::path::Path;

use scraper::{Html, Selector};
use url::Url;

use crate::db;
use crate::db::GhostDb;
use crate::web;

/// Polite delay between sequential HTTP fetches during crawls.
const CRAWL_FETCH_DELAY: std::time::Duration = std::time::Duration::from_millis(200);

use super::topic::ensure_topic_hierarchy;
use super::types::{ImportConfig, ImportConfigJson, ImportError, ImportResult, ImportSource};

/// BFS-crawl a website and return a manifest of (topic-relative path, content,
/// source_url) tuples without touching the database.
pub(crate) async fn fetch_crawl_manifest(
    config: &ImportConfig,
) -> Result<Vec<(String, String, String)>, ImportError> {
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

    let mut queue: VecDeque<(Url, usize)> = VecDeque::new();
    let mut visited: HashSet<String> = HashSet::new();
    let mut manifest = Vec::new();

    queue.push_back((seed, 0));

    let link_selector = Selector::parse("a[href]").expect("static selector should parse");

    while let Some((url, depth)) = queue.pop_front() {
        let normalized = normalize_url(&url);
        if visited.contains(&normalized) || depth > *max_depth || manifest.len() >= *max_pages {
            continue;
        }
        visited.insert(normalized.clone());

        let slug = crate::web::slug_from_url(url.as_str());
        let filename = format!("{slug}.md");
        let ref_path = format!("{}/{filename}", config.topic);

        let page_num = manifest.len() + 1;
        println!("  [{page_num}/{max_pages}] {}", url.as_str());

        // Fetch raw HTML for link extraction
        let (html, final_url) = match web::fetch_raw(url.as_str()).await {
            Ok(r) => r,
            Err(e) => {
                tracing::warn!(
                    url = url.as_str().to_string(),
                    error = e.to_string(),
                    "crawl: failed to fetch page",
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

        // Convert to markdown
        let extracted = web::extract_content(&html, url.as_str(), &web::FetchOptions::default());

        manifest.push((ref_path, extracted.text, url.to_string()));

        tokio::time::sleep(CRAWL_FETCH_DELAY).await;
    }

    Ok(manifest)
}

/// Import references by BFS-crawling a website, following same-host links.
#[tracing::instrument(name = "import crawl", skip_all, fields(topic = %config.topic))]
pub async fn import_crawl(
    db: &GhostDb,
    workspace: &Path,
    config: &ImportConfig,
) -> Result<ImportResult, ImportError> {
    let ImportSource::Crawl { url: seed_url, .. } = &config.source else {
        return Err(ImportError::Fetch("expected crawl source".into()));
    };
    let seed_url = seed_url.clone();

    let manifest = fetch_crawl_manifest(config).await?;

    // Ensure topic hierarchy
    let topic_id = ensure_topic_hierarchy(db, &config.topic).await?;

    // Build serializable config snapshot for DB and TOML
    let config_json = ImportConfigJson::from(config);
    let config_json_str = serde_json::to_string(&config_json).ok();

    // Upsert import batch with placeholder count
    let batch_id = db::knowledge::upsert_import_batch(
        db,
        &topic_id,
        "crawl",
        &seed_url,
        None,
        0,
        config_json_str.as_deref(),
    )
    .await?;

    let mut created = 0usize;
    let mut skipped = 0usize;

    for (ref_path, content, source_url) in &manifest {
        // Idempotency: skip if already imported
        if db::knowledge::find_reference_by_path(db, ref_path)
            .await?
            .is_some()
        {
            skipped += 1;
            continue;
        }

        // Write to disk: references/{ref_path}
        let disk_path = workspace.join("references").join(ref_path);
        if let Some(parent) = disk_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&disk_path, content)?;

        // Store as reference
        let hash = crate::embeddings::pipeline::content_hash(content);
        db::knowledge::create_reference(
            db,
            &topic_id,
            ref_path,
            content,
            Some(source_url.as_str()),
            Some(&batch_id),
            Some(&hash),
        )
        .await?;

        created += 1;
    }

    // Update import batch with final count
    let total_refs = usize::try_from(
        db::knowledge::count_references_by_topic(db, &topic_id)
            .await?
            .max(0),
    )
    .unwrap_or(0);
    let batch_id = db::knowledge::upsert_import_batch(
        db,
        &topic_id,
        "crawl",
        &seed_url,
        None,
        total_refs as i64,
        config_json_str.as_deref(),
    )
    .await?;

    // Write _import.toml and ensure index notes
    super::topic::write_import_toml(workspace, &config.topic, &config_json, None, total_refs)?;

    Ok(ImportResult {
        topic_id,
        batch_id,
        references_created: created,
        references_skipped: skipped,
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
