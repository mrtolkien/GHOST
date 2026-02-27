/// Max characters before a text is split into multiple chunks.
const SHORT_THRESHOLD: usize = 1500;

/// Target size for each chunk when splitting long text.
const CHUNK_TARGET: usize = 1000;

/// Overlap between consecutive chunks to preserve context.
const OVERLAP_CHARS: usize = 200;

#[derive(Debug, Clone)]
pub struct Chunk {
    pub index: usize,
    pub text: String,
}

/// Split content into embedding-friendly chunks.
///
/// Short text (<1500 chars) stays as a single chunk. Longer text is
/// split at ~1000 chars with ~200 char overlap at paragraph boundaries.
/// Tags are prepended to the first chunk only.
pub fn chunk_text(text: &str, tags: &[String]) -> Vec<Chunk> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Vec::new();
    }

    let tag_prefix = if tags.is_empty() {
        String::new()
    } else {
        format!("[tags: {}]\n", tags.join(", "))
    };

    if trimmed.len() < SHORT_THRESHOLD {
        return vec![Chunk {
            index: 0,
            text: format!("{tag_prefix}{trimmed}"),
        }];
    }

    let parts = split_with_overlap(trimmed, CHUNK_TARGET, OVERLAP_CHARS);
    parts
        .into_iter()
        .enumerate()
        .map(|(i, part)| {
            let text = if i == 0 {
                format!("{tag_prefix}{part}")
            } else {
                part
            };
            Chunk { index: i, text }
        })
        .collect()
}

/// Split text into parts of roughly `target` chars with `overlap` char
/// overlap, preferring paragraph (`\n\n`) or line (`\n`) boundaries.
fn split_with_overlap(text: &str, target: usize, overlap: usize) -> Vec<String> {
    let mut parts = Vec::new();
    let mut start = 0;

    while start < text.len() {
        let remaining = text.len() - start;
        if remaining <= target + overlap {
            let chunk = text[start..].trim();
            if !chunk.is_empty() {
                parts.push(chunk.to_string());
            }
            break;
        }

        let end = text.floor_char_boundary((start + target).min(text.len()));
        let search_region = &text[start..end];

        let split_at = search_region
            .rfind("\n\n")
            .or_else(|| search_region.rfind('\n'))
            .unwrap_or(search_region.len());

        let split_at = split_at.max(1);
        let abs_split = start + split_at;

        let chunk = text[start..abs_split].trim();
        if !chunk.is_empty() {
            parts.push(chunk.to_string());
        }

        // Step back by overlap for the next chunk
        let overlap_start = text.floor_char_boundary(abs_split.saturating_sub(overlap));
        // Snap overlap to a paragraph/line boundary if possible
        let next_start = text[overlap_start..abs_split]
            .find("\n\n")
            .or_else(|| text[overlap_start..abs_split].find('\n'))
            .map(|pos| overlap_start + pos)
            .unwrap_or(abs_split);

        start = next_start;
    }

    parts
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_text_returns_no_chunks() {
        let chunks = chunk_text("", &[]);
        assert!(chunks.is_empty());
    }

    #[test]
    fn whitespace_only_returns_no_chunks() {
        let chunks = chunk_text("   \n  \n  ", &[]);
        assert!(chunks.is_empty());
    }

    #[test]
    fn short_text_single_chunk() {
        let chunks = chunk_text("Hello world", &[]);
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].index, 0);
        assert_eq!(chunks[0].text, "Hello world");
    }

    #[test]
    fn short_text_with_tags() {
        let chunks = chunk_text("Hello world", &["rust".into(), "ai".into()]);
        assert_eq!(chunks.len(), 1);
        assert!(chunks[0].text.starts_with("[tags: rust, ai]\n"));
        assert!(chunks[0].text.contains("Hello world"));
    }

    #[test]
    fn long_text_splits() {
        let paragraph = "A".repeat(300);
        let text = format!(
            "{paragraph}\n\n{paragraph}\n\n{paragraph}\n\n\
             {paragraph}\n\n{paragraph}\n\n{paragraph}"
        );
        assert!(text.len() > SHORT_THRESHOLD);

        let chunks = chunk_text(&text, &[]);
        assert!(
            chunks.len() >= 2,
            "expected >=2 chunks, got {}",
            chunks.len()
        );

        for (i, chunk) in chunks.iter().enumerate() {
            assert_eq!(chunk.index, i);
            assert!(!chunk.text.is_empty(), "chunk {i} should not be empty");
        }
    }

    #[test]
    fn tags_only_on_first_chunk() {
        let paragraph = "B".repeat(300);
        let text = format!(
            "{paragraph}\n\n{paragraph}\n\n{paragraph}\n\n\
             {paragraph}\n\n{paragraph}\n\n{paragraph}"
        );
        let chunks = chunk_text(&text, &["tag1".into()]);
        assert!(chunks.len() >= 2);
        assert!(chunks[0].text.starts_with("[tags: tag1]\n"));
        for chunk in &chunks[1..] {
            assert!(
                !chunk.text.starts_with("[tags:"),
                "only first chunk should have tags"
            );
        }
    }

    #[test]
    fn chunks_have_sequential_indices() {
        let paragraph = "C".repeat(300);
        let text = format!(
            "{paragraph}\n\n{paragraph}\n\n{paragraph}\n\n\
             {paragraph}\n\n{paragraph}\n\n{paragraph}"
        );
        let chunks = chunk_text(&text, &[]);
        for (i, chunk) in chunks.iter().enumerate() {
            assert_eq!(chunk.index, i);
        }
    }

    #[test]
    fn multibyte_text_does_not_panic() {
        // Japanese text that would break byte-based slicing at char boundaries
        let segment = "日本語のテスト文章です。これは長いテキストの分割をテストします。";
        let text = std::iter::repeat_n(segment, 40)
            .collect::<Vec<_>>()
            .join("\n\n");
        assert!(text.len() > SHORT_THRESHOLD);

        let chunks = chunk_text(&text, &["テスト".into()]);
        assert!(!chunks.is_empty());
        for (i, chunk) in chunks.iter().enumerate() {
            assert_eq!(chunk.index, i);
            assert!(!chunk.text.is_empty());
        }
    }
}
