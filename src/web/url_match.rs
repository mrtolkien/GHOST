/// Normalize a URL for comparison: strip protocol, www prefix, trailing slashes,
/// and utm_* query params (preserving other query params).
#[must_use]
pub fn normalize_url(url: &str) -> String {
    let stripped = url
        .trim_end_matches('/')
        .trim_start_matches("https://")
        .trim_start_matches("http://")
        .trim_start_matches("www.");

    // Parse and filter query params — strip only utm_* keys
    if let Some(pos) = stripped.find('?') {
        let (path, query_with_q) = stripped.split_at(pos);
        let query = &query_with_q[1..]; // skip '?'
        let kept: Vec<&str> = query
            .split('&')
            .filter(|p| !p.starts_with("utm_"))
            .collect();
        if kept.is_empty() {
            path.to_string()
        } else {
            format!("{path}?{}", kept.join("&"))
        }
    } else {
        stripped.to_string()
    }
}

/// Check if two URLs refer to the same page using normalized exact comparison.
#[must_use]
pub fn urls_match(a: &str, b: &str) -> bool {
    normalize_url(a) == normalize_url(b)
}

/// Extract URL and whether it's a search result from frontmatter.
/// Returns (url_or_empty, is_search).
#[must_use]
pub fn extract_frontmatter_info(content: &str) -> (String, bool) {
    let mut in_frontmatter = false;
    let mut url = String::new();
    let mut is_search = false;
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
            if let Some(u) = trimmed.strip_prefix("url: ") {
                url = u.to_string();
            } else if trimmed.starts_with("query: ") {
                is_search = true;
            }
        }
    }
    (url, is_search)
}

/// Extract a topic directory name from a URL's domain.
///
/// E.g. `https://www.tomshardware.com/reviews/...` -> `tomshardware-com`
#[must_use]
pub fn topic_from_url(url: &str) -> String {
    let stripped = url
        .trim_start_matches("https://")
        .trim_start_matches("http://")
        .trim_start_matches("www.");

    // Take just the domain part (up to first '/')
    let domain = stripped.split('/').next().unwrap_or(stripped);

    // Replace dots with hyphens for directory name
    domain.replace('.', "-")
}

/// Generate a filesystem-safe slug from a URL, stripping scheme and `www.` prefix.
#[must_use]
pub fn slug_from_url(url: &str) -> String {
    let stripped = url
        .trim_start_matches("https://")
        .trim_start_matches("http://")
        .trim_start_matches("www.");
    sanitize_slug(stripped)
}

/// Sanitize an arbitrary string into a filesystem-safe slug.
///
/// Replaces non-alphanumeric characters with hyphens, collapses consecutive
/// hyphens, and truncates to 60 characters on a char boundary.
#[must_use]
pub(crate) fn sanitize_slug(input: &str) -> String {
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
        // Find a char boundary at or before byte 60
        let mut end = 60;
        while !trimmed.is_char_boundary(end) {
            end -= 1;
        }
        trimmed[..end].trim_end_matches('-').to_string()
    } else {
        trimmed.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn urls_match_exact() {
        assert!(urls_match(
            "https://www.example.com/page/",
            "https://example.com/page"
        ));
    }

    #[test]
    fn urls_match_no_prefix_false_positive() {
        assert!(!urls_match(
            "https://example.com/page",
            "https://example.com/page-extra"
        ));
    }

    #[test]
    fn urls_match_utm_stripped() {
        assert!(urls_match(
            "https://example.com/article?utm_source=twitter",
            "https://example.com/article"
        ));
    }

    #[test]
    fn non_utm_query_params_preserved() {
        assert!(!urls_match(
            "https://example.com/search?q=rust",
            "https://example.com/search?q=python"
        ));
        assert!(urls_match(
            "https://example.com/search?q=rust&utm_source=x",
            "https://example.com/search?q=rust"
        ));
    }

    #[test]
    fn topic_from_url_extracts_domain() {
        assert_eq!(
            topic_from_url("https://www.tomshardware.com/reviews/test"),
            "tomshardware-com"
        );
        assert_eq!(
            topic_from_url("https://all3dp.com/best-printers"),
            "all3dp-com"
        );
        assert_eq!(topic_from_url("http://example.org/page"), "example-org");
    }

    #[test]
    fn slug_from_url_strips_scheme_and_www() {
        assert_eq!(
            slug_from_url("https://www.example.com/page"),
            "example-com-page"
        );
        assert_eq!(slug_from_url("http://docs.rs/tokio"), "docs-rs-tokio");
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
    fn extract_frontmatter_info_parses_url() {
        let content = "---\nurl: https://example.com/page\nfetched_at: 2026-01-01\n---\n# Title\n";
        let (url, is_search) = extract_frontmatter_info(content);
        assert_eq!(url, "https://example.com/page");
        assert!(!is_search);
    }

    #[test]
    fn extract_frontmatter_info_parses_search() {
        let content = "---\nquery: best printers\nsearched_at: 2026-01-01\n---\n1. Result\n";
        let (url, is_search) = extract_frontmatter_info(content);
        assert!(url.is_empty());
        assert!(is_search);
    }
}
