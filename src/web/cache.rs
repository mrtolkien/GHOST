use std::path::{Path, PathBuf};

use chrono::Utc;

use super::{ExtractedContent, SearchResult, WebError};

const WEB_CACHE_DIR: &str = ".web-cache";

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
        body.push('\n');
    }

    std::fs::write(&path, body).map_err(|source| WebError::CacheWrite {
        path: path.clone(),
        source,
    })?;

    Ok(path)
}

fn slug_from_url(url: &str) -> String {
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
            },
            SearchResult {
                title: "Second Result".to_string(),
                url: "https://example.com/2".to_string(),
                snippet: None,
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
}
