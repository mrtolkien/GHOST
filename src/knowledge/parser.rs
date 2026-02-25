use regex::Regex;
use std::sync::LazyLock;

use super::error::KnowledgeError;
use super::types::{NoteFrontMatter, ParsedNote, WikiLink};

const DELIMITER: &str = "---";

static WIKI_LINK_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\[\[(?:(\w+)>)?([^\]]+)\]\]").expect("wiki link regex"));

pub fn parse_note(raw: &str) -> Result<ParsedNote, KnowledgeError> {
    let trimmed = raw.trim_start();

    if !trimmed.starts_with(DELIMITER) {
        return Err(KnowledgeError::InvalidFrontMatter {
            reason: "missing opening --- delimiter".to_string(),
        });
    }

    let after_open = &trimmed[DELIMITER.len()..];
    let Some(end_pos) = after_open.find(DELIMITER) else {
        return Err(KnowledgeError::InvalidFrontMatter {
            reason: "missing closing --- delimiter".to_string(),
        });
    };

    let frontmatter_str = &after_open[..end_pos];
    let body_start = DELIMITER.len() + end_pos + DELIMITER.len();
    let body = trimmed[body_start..]
        .strip_prefix('\n')
        .unwrap_or(&trimmed[body_start..])
        .to_string();

    let front: NoteFrontMatter = serde_yaml::from_str(frontmatter_str)
        .map_err(|source| KnowledgeError::FrontMatterParse { source })?;

    let wiki_links = extract_wiki_links(&body);

    Ok(ParsedNote {
        front,
        body,
        wiki_links,
    })
}

#[must_use]
pub fn extract_wiki_links(body: &str) -> Vec<WikiLink> {
    WIKI_LINK_RE
        .captures_iter(body)
        .map(|cap| WikiLink {
            relationship: cap.get(1).map(|m| m.as_str().to_string()),
            target: cap[2].to_string(),
        })
        .collect()
}

pub fn serialize_note(front: &NoteFrontMatter, body: &str) -> Result<String, KnowledgeError> {
    let yaml_str = serde_yaml::to_string(front)
        .map_err(|source| KnowledgeError::FrontMatterSerialize { source })?;

    Ok(format!("{DELIMITER}\n{yaml_str}{DELIMITER}\n{body}"))
}

#[must_use]
pub fn slug_from_title(title: &str) -> String {
    title
        .to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '_' })
        .collect::<String>()
        .split('_')
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("_")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::knowledge::types::Archetype;

    #[test]
    fn parse_valid_note_roundtrip() {
        let raw = "---\ntitle: Rust\narchetype: concept\ntags:\n  - lang\ntrust: 8\n---\nRust is a systems programming language.\n\nIt has [[Ownership]] and [[concept>Borrowing]].\n";

        let parsed = parse_note(raw).unwrap();
        assert_eq!(parsed.front.title, "Rust");
        assert_eq!(parsed.front.archetype, Some(Archetype::Concept));
        assert_eq!(parsed.front.tags, vec!["lang"]);
        assert_eq!(parsed.front.trust, 8);
        assert!(parsed.body.contains("Rust is a systems"));

        let serialized = serialize_note(&parsed.front, &parsed.body).unwrap();
        let reparsed = parse_note(&serialized).unwrap();
        assert_eq!(reparsed.front, parsed.front);
    }

    #[test]
    fn extract_plain_wiki_link() {
        let links = extract_wiki_links("See [[Target]] for details.");
        assert_eq!(links.len(), 1);
        assert_eq!(links[0].target, "Target");
        assert_eq!(links[0].relationship, None);
    }

    #[test]
    fn extract_typed_wiki_link() {
        let links = extract_wiki_links("Uses [[written_in>Rust]] as language.");
        assert_eq!(links.len(), 1);
        assert_eq!(links[0].target, "Rust");
        assert_eq!(links[0].relationship, Some("written_in".to_string()));
    }

    #[test]
    fn extract_multiple_wiki_links() {
        let links = extract_wiki_links("Has [[Alpha]] and [[rel>Beta]] and [[Gamma]].");
        assert_eq!(links.len(), 3);
        assert_eq!(links[0].target, "Alpha");
        assert_eq!(links[1].target, "Beta");
        assert_eq!(links[2].target, "Gamma");
    }

    #[test]
    fn missing_delimiter_error() {
        let result = parse_note("title: Oops\n\nBody without delimiters.");
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.to_string().contains("missing opening"));
    }

    #[test]
    fn missing_closing_delimiter() {
        let result = parse_note("---\ntitle: Oops\n\nBody without closing.");
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.to_string().contains("missing closing"));
    }

    #[test]
    fn slug_generation() {
        assert_eq!(slug_from_title("Rust Language"), "rust_language");
        assert_eq!(slug_from_title("Hello, World!"), "hello_world");
        assert_eq!(
            slug_from_title("  spaces  and--dashes  "),
            "spaces_and_dashes"
        );
    }

    #[test]
    fn serialize_roundtrip() {
        let front = NoteFrontMatter {
            title: "Test Note".to_string(),
            archetype: Some(Archetype::Project),
            tags: vec!["a".into(), "b".into()],
            sources: vec![],
            trust: 7,
        };
        let body = "Some body content.\n";

        let serialized = serialize_note(&front, body).unwrap();
        let parsed = parse_note(&serialized).unwrap();
        assert_eq!(parsed.front, front);
        assert_eq!(parsed.body, body);
    }

    #[test]
    fn default_trust_value() {
        let raw = "---\ntitle: Minimal\n---\nJust body.\n";
        let parsed = parse_note(raw).unwrap();
        assert_eq!(parsed.front.trust, 5);
    }

    #[test]
    fn sources_roundtrip() {
        let front = NoteFrontMatter {
            title: "With Sources".to_string(),
            archetype: Some(Archetype::Concept),
            tags: vec!["test".into()],
            sources: vec![
                "https://example.com/article".into(),
                "https://other.com/page".into(),
            ],
            trust: 7,
        };
        let body = "Body with sources in frontmatter.\n";

        let serialized = serialize_note(&front, body).unwrap();
        let reparsed = parse_note(&serialized).unwrap();
        assert_eq!(reparsed.front.sources, front.sources);
        assert_eq!(reparsed.front, front);
        assert_eq!(reparsed.body, body);
    }

    #[test]
    fn sources_omitted_when_empty() {
        let front = NoteFrontMatter {
            title: "No Sources".to_string(),
            archetype: None,
            tags: vec![],
            sources: vec![],
            trust: 5,
        };
        let serialized = serialize_note(&front, "body\n").unwrap();
        assert!(
            !serialized.contains("sources"),
            "empty sources should be omitted from serialized output"
        );
    }
}
