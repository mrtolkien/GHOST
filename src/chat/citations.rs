use regex::Regex;
use std::sync::LazyLock;

/// A citation extracted from model response text.
#[derive(Debug, Clone)]
pub struct ExtractedCitation {
    pub url: String,
    pub title: Option<String>,
}

/// Match `[N] [Title](url)` or `[N] url` patterns in a Sources/References section.
static CITATION_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\[(\d+)\]\s+\[([^\]]+)\]\(([^)]+)\)|\[(\d+)\]\s+(https?://\S+)")
        .expect("citation regex")
});

/// Extract citations from the trailing Sources/References section of a response.
pub fn extract_citations(text: &str) -> Vec<ExtractedCitation> {
    let section_start = text
        .rfind("## Sources")
        .or_else(|| text.rfind("## References"))
        .or_else(|| text.rfind("**Sources**"))
        .or_else(|| text.rfind("Sources:"));

    let section = match section_start {
        Some(pos) => &text[pos..],
        None => return Vec::new(),
    };

    CITATION_RE
        .captures_iter(section)
        .map(|cap| {
            if let (Some(title), Some(url)) = (cap.get(2), cap.get(3)) {
                ExtractedCitation {
                    url: url.as_str().to_string(),
                    title: Some(title.as_str().to_string()),
                }
            } else if let Some(url) = cap.get(5) {
                ExtractedCitation {
                    url: url
                        .as_str()
                        .trim_end_matches(|c: char| ".,;:)".contains(c))
                        .to_string(),
                    title: None,
                }
            } else {
                ExtractedCitation {
                    url: String::new(),
                    title: None,
                }
            }
        })
        .filter(|c| !c.url.is_empty())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_titled_citations() {
        let text = "Some answer text.\n\n\
            ## Sources\n\
            [1] [Tom's Hardware Review](https://tomshardware.com/reviews/test)\n\
            [2] [All3DP Guide](https://all3dp.com/guide)\n";

        let citations = extract_citations(text);
        assert_eq!(citations.len(), 2);
        assert_eq!(citations[0].url, "https://tomshardware.com/reviews/test");
        assert_eq!(
            citations[0].title.as_deref(),
            Some("Tom's Hardware Review")
        );
        assert_eq!(citations[1].url, "https://all3dp.com/guide");
    }

    #[test]
    fn extract_bare_url_citations() {
        let text = "Answer.\n\nSources:\n[1] https://example.com/page\n";

        let citations = extract_citations(text);
        assert_eq!(citations.len(), 1);
        assert_eq!(citations[0].url, "https://example.com/page");
        assert!(citations[0].title.is_none());
    }

    #[test]
    fn no_sources_section() {
        let text = "Just an answer with no sources.";
        assert!(extract_citations(text).is_empty());
    }
}
