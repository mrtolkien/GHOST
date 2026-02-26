use std::path::{Path, PathBuf};

use chrono::Utc;

use super::{ExtractedContent, SearchResult, WebError};

const WEB_CACHE_DIR: &str = ".web-cache";

/// Format SearXNG-specific metadata into a "Sources: ... · score: N" line.
/// Returns `None` when no metadata is present (e.g. Brave results).
pub fn format_search_metadata(result: &SearchResult) -> Option<String> {
    let mut parts = Vec::new();

    if let (Some(engines), Some(positions)) = (&result.engines, &result.positions) {
        let sources: Vec<String> = engines
            .iter()
            .zip(positions.iter())
            .map(|(e, p)| format!("{e} #{p}"))
            .collect();
        if !sources.is_empty() {
            parts.push(format!("Sources: {}", sources.join(", ")));
        }
    } else if let Some(engines) = &result.engines
        && !engines.is_empty()
    {
        parts.push(format!("Sources: {}", engines.join(", ")));
    }

    if let Some(score) = result.score {
        parts.push(format!("score: {score:.1}"));
    }

    if let Some(date) = &result.published_date
        && !date.is_empty()
    {
        parts.push(date.clone());
    }

    if parts.is_empty() {
        None
    } else {
        Some(parts.join(" · "))
    }
}

pub fn save_fetch_cache(
    workspace: &Path,
    url: &str,
    content: &ExtractedContent,
) -> Result<PathBuf, WebError> {
    let now = Utc::now();
    let timestamp = now.format("%Y-%m-%dT%H-%M-%S");
    let slug = slug_from_url(url);
    let filename = format!("{timestamp}_{slug}.md");
    let path = workspace.join(WEB_CACHE_DIR).join(&filename);

    let title_header = content
        .title
        .as_ref()
        .map(|t| format!("\n# {t}\n"))
        .unwrap_or_default();

    let body = format!(
        "---\nurl: {url}\nfetched_at: {iso}\n---\n{title_header}\n{text}\n",
        iso = now.to_rfc3339(),
        text = content.text,
    );

    std::fs::write(&path, body).map_err(|source| WebError::CacheWrite {
        path: path.clone(),
        source,
    })?;

    Ok(path)
}

pub fn save_search_cache(
    workspace: &Path,
    query: &str,
    results: &[SearchResult],
) -> Result<PathBuf, WebError> {
    let now = Utc::now();
    let timestamp = now.format("%Y-%m-%dT%H-%M-%S");
    let slug = slug_from_query(query);
    let filename = format!("{timestamp}_{slug}.md");
    let path = workspace.join(WEB_CACHE_DIR).join(&filename);

    let mut body = format!(
        "---\nquery: {query}\nsearched_at: {iso}\n---\n\n",
        iso = now.to_rfc3339(),
    );

    for (i, result) in results.iter().enumerate() {
        body.push_str(&format!("{}. {}\n", i + 1, result.title));
        body.push_str(&format!("   {}\n", result.url));
        if let Some(snippet) = &result.snippet {
            body.push_str(&format!("   {snippet}\n"));
        }
        if let Some(meta) = format_search_metadata(result) {
            body.push_str(&format!("   {meta}\n"));
        }
        body.push('\n');
    }

    std::fs::write(&path, body).map_err(|source| WebError::CacheWrite {
        path: path.clone(),
        source,
    })?;

    Ok(path)
}

pub fn scan_web_cache(workspace: &Path) -> Result<Option<String>, WebError> {
    let cache_dir = workspace.join(WEB_CACHE_DIR);

    if !cache_dir.exists() {
        return Ok(None);
    }

    let mut entries: Vec<_> = std::fs::read_dir(&cache_dir)
        .map_err(|source| WebError::CacheRead {
            path: cache_dir.clone(),
            source,
        })?
        .filter_map(|entry| {
            let entry = entry.ok()?;
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) == Some("md") {
                Some(path)
            } else {
                None
            }
        })
        .collect();

    if entries.is_empty() {
        return Ok(None);
    }

    entries.sort();

    let mut lines = Vec::with_capacity(entries.len());
    for path in &entries {
        let filename = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown");

        let label = extract_frontmatter_label(path);
        lines.push(format!("- `.web-cache/{filename}` — {label}"));
    }

    Ok(Some(lines.join("\n")))
}

fn extract_frontmatter_label(path: &Path) -> String {
    let content = match std::fs::read_to_string(path) {
        Ok(c) => c,
        Err(_) => return "unknown".to_string(),
    };

    // Look for url: or query: in YAML frontmatter (between --- delimiters)
    let mut in_frontmatter = false;
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed == "---" {
            if in_frontmatter {
                break;
            }
            in_frontmatter = true;
            continue;
        }
        if in_frontmatter {
            if let Some(url) = trimmed.strip_prefix("url: ") {
                return url.to_string();
            }
            if let Some(query) = trimmed.strip_prefix("query: ") {
                return format!("search: {query}");
            }
        }
    }

    "unknown".to_string()
}

pub fn slug_from_url(url: &str) -> String {
    let stripped = url
        .trim_start_matches("https://")
        .trim_start_matches("http://")
        .trim_start_matches("www.");
    sanitize_slug(stripped)
}

fn slug_from_query(query: &str) -> String {
    sanitize_slug(query)
}

fn sanitize_slug(input: &str) -> String {
    let slug: String = input
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '-' })
        .collect::<String>()
        .to_lowercase();

    // Collapse consecutive dashes and trim
    let mut result = String::with_capacity(slug.len());
    let mut prev_dash = false;
    for c in slug.chars() {
        if c == '-' {
            if !prev_dash && !result.is_empty() {
                result.push('-');
            }
            prev_dash = true;
        } else {
            result.push(c);
            prev_dash = false;
        }
    }

    let trimmed = result.trim_end_matches('-');
    if trimmed.len() > 60 {
        trimmed[..60].trim_end_matches('-').to_string()
    } else {
        trimmed.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn test_workspace() -> (PathBuf, TempDir) {
        let dir = TempDir::new().unwrap();
        let cache_dir = dir.path().join(WEB_CACHE_DIR);
        std::fs::create_dir_all(&cache_dir).unwrap();
        (dir.path().to_path_buf(), dir)
    }

    #[test]
    fn save_fetch_creates_file_with_frontmatter() {
        let (workspace, _dir) = test_workspace();
        let content = ExtractedContent {
            title: Some("Test Page".to_string()),
            text: "Hello world".to_string(),
            word_count: 2,
            truncated: false,
        };

        let path = save_fetch_cache(&workspace, "https://example.com/page", &content).unwrap();

        assert!(path.exists());
        let body = std::fs::read_to_string(&path).unwrap();
        assert!(body.contains("url: https://example.com/page"));
        assert!(body.contains("fetched_at:"));
        assert!(body.contains("# Test Page"));
        assert!(body.contains("Hello world"));
    }

    #[test]
    fn save_search_creates_file_with_results() {
        let (workspace, _dir) = test_workspace();
        let results = vec![
            SearchResult {
                title: "First Result".to_string(),
                url: "https://example.com/1".to_string(),
                snippet: Some("A snippet".to_string()),
                engines: None,
                positions: None,
                score: None,
                published_date: None,
            },
            SearchResult {
                title: "Second Result".to_string(),
                url: "https://example.com/2".to_string(),
                snippet: None,
                engines: None,
                positions: None,
                score: None,
                published_date: None,
            },
        ];

        let path = save_search_cache(&workspace, "test query", &results).unwrap();

        assert!(path.exists());
        let body = std::fs::read_to_string(&path).unwrap();
        assert!(body.contains("query: test query"));
        assert!(body.contains("searched_at:"));
        assert!(body.contains("1. First Result"));
        assert!(body.contains("https://example.com/1"));
        assert!(body.contains("A snippet"));
        assert!(body.contains("2. Second Result"));
    }

    #[test]
    fn slug_from_url_strips_scheme_and_www() {
        assert_eq!(
            slug_from_url("https://www.example.com/page"),
            "example-com-page"
        );
        assert_eq!(
            slug_from_url("http://docs.rs/surrealdb"),
            "docs-rs-surrealdb"
        );
    }

    #[test]
    fn slug_truncates_at_60_chars() {
        let long_url = format!("https://example.com/{}", "a".repeat(100));
        let slug = slug_from_url(&long_url);
        assert!(slug.len() <= 60);
    }

    #[test]
    fn slug_handles_special_characters() {
        let slug = slug_from_url("https://example.com/path?q=hello&x=1");
        assert!(!slug.contains('?'));
        assert!(!slug.contains('&'));
        assert!(!slug.contains('='));
    }

    #[test]
    fn scan_web_cache_lists_files() {
        let (workspace, _dir) = test_workspace();

        let fetch_content = ExtractedContent {
            title: Some("Rust Docs".to_string()),
            text: "Content here".to_string(),
            word_count: 2,
            truncated: false,
        };
        save_fetch_cache(&workspace, "https://doc.rust-lang.org", &fetch_content).unwrap();

        let results = vec![SearchResult {
            title: "Result".to_string(),
            url: "https://example.com".to_string(),
            snippet: None,
            engines: None,
            positions: None,
            score: None,
            published_date: None,
        }];
        save_search_cache(&workspace, "rust tutorials", &results).unwrap();

        let listing = scan_web_cache(&workspace).unwrap().unwrap();

        assert!(listing.contains("https://doc.rust-lang.org"));
        assert!(listing.contains("search: rust tutorials"));
        assert!(listing.contains(".web-cache/"));
        assert_eq!(listing.lines().count(), 2);
    }

    #[test]
    fn scan_web_cache_empty_returns_none() {
        let (workspace, _dir) = test_workspace();
        assert!(scan_web_cache(&workspace).unwrap().is_none());
    }

    #[test]
    fn scan_web_cache_missing_dir_returns_none() {
        let dir = TempDir::new().unwrap();
        assert!(scan_web_cache(dir.path()).unwrap().is_none());
    }
}
