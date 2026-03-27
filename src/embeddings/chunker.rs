use tree_sitter::{Language, Node, Parser};

use crate::constants::EMBEDDING_CHUNK_TARGET;

#[derive(Debug, Clone)]
pub struct Chunk {
    pub index: usize,
    pub text: String,
}

/// Chunk content using tree-sitter AST analysis.
///
/// For code files, detects the language from `file_path` and chunks by AST
/// nodes. For everything else (markdown, plain text), uses the tree-sitter
/// markdown grammar to chunk by sections/headings. Returns AST-aware chunks
/// with contextual headers (file/language for code, section path for markdown).
pub fn chunk_content(content: &str, tags: &[String], file_path: Option<&str>) -> Vec<Chunk> {
    let trimmed = content.trim();
    if trimmed.is_empty() {
        return Vec::new();
    }

    let tag_prefix = if tags.is_empty() {
        String::new()
    } else {
        format!("[tags: {}] ", tags.join(", "))
    };

    // Try code chunking first if we have a file path with a known extension
    if let Some(path) = file_path
        && let Some((language, lang_name)) = detect_code_language(path)
        && let Some(chunks) = chunk_code(trimmed, &tag_prefix, path, language, lang_name)
    {
        return chunks;
    }

    // Default: markdown-aware chunking (works for plain text too)
    chunk_markdown(trimmed, &tag_prefix)
}

// ---------------------------------------------------------------------------
// Markdown chunking
// ---------------------------------------------------------------------------

/// Chunk markdown content by walking the tree-sitter AST. Sections that fit
/// within `EMBEDDING_CHUNK_TARGET` are emitted whole. Oversized sections are split into
/// their child nodes. Each chunk is prefixed with its heading path for context.
fn chunk_markdown(content: &str, tag_prefix: &str) -> Vec<Chunk> {
    let mut parser = Parser::new();
    if parser
        .set_language(&tree_sitter_md::LANGUAGE.into())
        .is_err()
    {
        return fallback_single_chunk(content, tag_prefix);
    }
    let Some(tree) = parser.parse(content, None) else {
        return fallback_single_chunk(content, tag_prefix);
    };

    let mut raw_chunks: Vec<String> = Vec::new();
    let root = tree.root_node();

    // Walk top-level children of document (which are sections)
    for i in 0..root.child_count() as u32 {
        if let Some(child) = root.child(i) {
            collect_markdown_sections(child, content, &[], &mut raw_chunks);
        }
    }

    if raw_chunks.is_empty() {
        return fallback_single_chunk(content, tag_prefix);
    }

    // Greedy merge small consecutive chunks
    let merged = greedy_merge(&raw_chunks);

    merged
        .into_iter()
        .enumerate()
        .map(|(i, text)| {
            let prefix = if i == 0 { tag_prefix } else { "" };
            Chunk {
                index: i,
                text: format!("{prefix}{text}"),
            }
        })
        .collect()
}

/// Recursively collect markdown sections. Each section's heading text is
/// tracked to build a `[section: A > B > C]` prefix for context.
fn collect_markdown_sections(
    node: Node,
    source: &str,
    heading_path: &[String],
    out: &mut Vec<String>,
) {
    if node.kind() != "section" {
        // Non-section node (e.g. thematic_break at document level)
        let text = source[node.byte_range()].trim();
        if !text.is_empty() {
            out.push(text.to_string());
        }
        return;
    }

    // Extract this section's heading text (if any)
    let mut current_path = heading_path.to_vec();
    if let Some(heading_text) = extract_heading_text(node, source) {
        current_path.push(heading_text);
    }

    let section_text = source[node.byte_range()].trim();

    // If the whole section fits, emit it as one chunk with section path
    if section_text.len() <= EMBEDDING_CHUNK_TARGET {
        let prefix = section_prefix(&current_path);
        out.push(format!("{prefix}{section_text}"));
        return;
    }

    // Section is too large — split into its direct children.
    // Non-section children (paragraphs, lists, etc.) are emitted individually.
    // Child sections recurse with the updated heading path.
    for i in 0..node.child_count() as u32 {
        let Some(child) = node.child(i) else {
            continue;
        };
        match child.kind() {
            "section" => {
                collect_markdown_sections(child, source, &current_path, out);
            }
            "atx_heading" | "setext_heading" => {
                // The heading itself — emit it so it appears in the chunk
                let text = source[child.byte_range()].trim();
                if !text.is_empty() {
                    let prefix = section_prefix(&current_path);
                    out.push(format!("{prefix}{text}"));
                }
            }
            _ => {
                // Content node (paragraph, list, code block, etc.)
                let text = source[child.byte_range()].trim();
                if !text.is_empty() {
                    if text.len() <= EMBEDDING_CHUNK_TARGET {
                        let prefix = section_prefix(&current_path);
                        out.push(format!("{prefix}{text}"));
                    } else {
                        // Extremely long node — split by lines
                        let prefix = section_prefix(&current_path);
                        for part in split_oversized(text) {
                            out.push(format!("{prefix}{part}"));
                        }
                    }
                }
            }
        }
    }
}

/// Build a `[section: A > B > C]\n` prefix from the heading path.
fn section_prefix(path: &[String]) -> String {
    if path.is_empty() {
        return String::new();
    }
    format!("[section: {}]\n", path.join(" > "))
}

/// Extract the inline text from an atx_heading or setext_heading node.
fn extract_heading_text(section: Node, source: &str) -> Option<String> {
    for i in 0..section.child_count() as u32 {
        let child = section.child(i)?;
        if child.kind() == "atx_heading" || child.kind() == "setext_heading" {
            // The heading text is in the `inline` child
            for j in 0..child.child_count() as u32 {
                if let Some(inline) = child.child(j)
                    && (inline.kind() == "inline" || inline.kind() == "paragraph")
                {
                    let text = source[inline.byte_range()].trim();
                    if !text.is_empty() {
                        return Some(text.to_string());
                    }
                }
            }
            // Heading with no inline text — use the raw heading text minus markers
            let raw = source[child.byte_range()].trim();
            let stripped = raw.trim_start_matches('#').trim();
            if !stripped.is_empty() {
                return Some(stripped.to_string());
            }
            return None;
        }
    }
    None
}

/// Split an oversized text block into parts of roughly `EMBEDDING_CHUNK_TARGET` chars,
/// preferring paragraph (`\n\n`) or line (`\n`) boundaries.
fn split_oversized(text: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut start = 0;

    while start < text.len() {
        let remaining = text.len() - start;
        if remaining <= EMBEDDING_CHUNK_TARGET {
            let chunk = text[start..].trim();
            if !chunk.is_empty() {
                parts.push(chunk.to_string());
            }
            break;
        }

        let end = text.floor_char_boundary((start + EMBEDDING_CHUNK_TARGET).min(text.len()));
        let search_region = &text[start..end];

        let split_at = search_region
            .rfind("\n\n")
            .or_else(|| search_region.rfind('\n'))
            .unwrap_or(search_region.len())
            .max(1);

        let abs_split = start + split_at;
        let chunk = text[start..abs_split].trim();
        if !chunk.is_empty() {
            parts.push(chunk.to_string());
        }

        start = text.ceil_char_boundary(abs_split.max(start + 1));
    }

    parts
}

fn fallback_single_chunk(content: &str, tag_prefix: &str) -> Vec<Chunk> {
    vec![Chunk {
        index: 0,
        text: format!("{tag_prefix}{content}"),
    }]
}

// ---------------------------------------------------------------------------
// Code chunking
// ---------------------------------------------------------------------------

/// Chunk source code by walking the tree-sitter AST. Nodes that fit within
/// `EMBEDDING_CHUNK_TARGET` are emitted whole; oversized nodes recurse into children.
fn chunk_code(
    content: &str,
    tag_prefix: &str,
    file_path: &str,
    language: Language,
    lang_name: &str,
) -> Option<Vec<Chunk>> {
    let mut parser = Parser::new();
    parser.set_language(&language).ok()?;
    let tree = parser.parse(content, None)?;
    let root = tree.root_node();

    let mut raw_chunks: Vec<String> = Vec::new();
    collect_code_chunks(root, content, &mut raw_chunks);

    if raw_chunks.is_empty() {
        return None;
    }

    let merged = greedy_merge(&raw_chunks);

    let chunks = merged
        .into_iter()
        .enumerate()
        .map(|(i, text)| {
            let header = if i == 0 {
                format!("{tag_prefix}[file: {file_path}] [language: {lang_name}]\n")
            } else {
                format!("[file: {file_path}] [language: {lang_name}]\n")
            };
            Chunk {
                index: i,
                text: format!("{header}{text}"),
            }
        })
        .collect();

    Some(chunks)
}

/// Recursively collect chunks from a code AST.
fn collect_code_chunks(node: Node, source: &str, out: &mut Vec<String>) {
    let text = &source[node.byte_range()];
    if text.len() <= EMBEDDING_CHUNK_TARGET {
        let trimmed = text.trim();
        if !trimmed.is_empty() {
            out.push(trimmed.to_string());
        }
        return;
    }

    let child_count = node.child_count() as u32;
    if child_count == 0 {
        let trimmed = text.trim();
        if !trimmed.is_empty() {
            out.push(trimmed.to_string());
        }
        return;
    }

    for i in 0..child_count {
        if let Some(child) = node.child(i) {
            collect_code_chunks(child, source, out);
        }
    }
}

// ---------------------------------------------------------------------------
// Shared helpers
// ---------------------------------------------------------------------------

/// Greedily merge consecutive small chunks up to `EMBEDDING_CHUNK_TARGET`.
fn greedy_merge(chunks: &[String]) -> Vec<String> {
    let mut merged = Vec::new();
    let mut current = String::new();

    for chunk in chunks {
        if current.is_empty() {
            current = chunk.clone();
        } else if current.len() + chunk.len() < EMBEDDING_CHUNK_TARGET {
            current.push('\n');
            current.push_str(chunk);
        } else {
            merged.push(std::mem::take(&mut current));
            current = chunk.clone();
        }
    }

    if !current.is_empty() {
        merged.push(current);
    }

    merged
}

/// Detect programming language from file extension.
fn detect_code_language(file_path: &str) -> Option<(Language, &'static str)> {
    let ext = file_path.rsplit('.').next()?;
    match ext {
        "rs" => Some((tree_sitter_rust::LANGUAGE.into(), "rust")),
        "py" | "pyi" => Some((tree_sitter_python::LANGUAGE.into(), "python")),
        "js" | "mjs" | "cjs" | "jsx" => {
            Some((tree_sitter_javascript::LANGUAGE.into(), "javascript"))
        }
        "ts" => Some((
            tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
            "typescript",
        )),
        "tsx" => Some((tree_sitter_typescript::LANGUAGE_TSX.into(), "tsx")),
        "go" => Some((tree_sitter_go::LANGUAGE.into(), "go")),
        "sh" | "bash" => Some((tree_sitter_bash::LANGUAGE.into(), "bash")),
        "toml" => Some((tree_sitter_toml_ng::LANGUAGE.into(), "toml")),
        "json" => Some((tree_sitter_json::LANGUAGE.into(), "json")),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // -- Markdown tests --

    #[test]
    fn empty_text_returns_no_chunks() {
        let chunks = chunk_content("", &[], None);
        assert!(chunks.is_empty());
    }

    #[test]
    fn whitespace_only_returns_no_chunks() {
        let chunks = chunk_content("   \n  \n  ", &[], None);
        assert!(chunks.is_empty());
    }

    #[test]
    fn short_markdown_single_chunk() {
        let chunks = chunk_content("# Hello\n\nworld", &[], None);
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].index, 0);
        assert!(chunks[0].text.contains("# Hello"));
        assert!(chunks[0].text.contains("world"));
    }

    #[test]
    fn tags_prepended_to_first_chunk() {
        let chunks = chunk_content("# Hello\n\nworld", &["rust".into(), "ai".into()], None);
        assert_eq!(chunks.len(), 1);
        assert!(chunks[0].text.starts_with("[tags: rust, ai]"));
    }

    #[test]
    fn markdown_sections_become_chunks() {
        // Build a document with sections larger than EMBEDDING_CHUNK_TARGET total
        let body = "X".repeat(800);
        let text = format!(
            "# Title\n\n{body}\n\n## Section A\n\n{body}\n\n## Section B\n\n{body}\n\n## Section C\n\n{body}"
        );

        let chunks = chunk_content(&text, &[], None);
        assert!(
            chunks.len() >= 2,
            "expected multiple chunks, got {}",
            chunks.len()
        );

        // Check sequential indices
        for (i, chunk) in chunks.iter().enumerate() {
            assert_eq!(chunk.index, i);
            assert!(!chunk.text.is_empty());
        }
    }

    #[test]
    fn section_path_in_chunks() {
        // Create a document with nested sections large enough to split
        let body = "Y".repeat(1500);
        let text = format!("# Top\n\n## Inner\n\n{body}\n\n## Other\n\n{body}");

        let chunks = chunk_content(&text, &[], None);
        // At least one chunk should have a section prefix
        let has_section_prefix = chunks.iter().any(|c| c.text.contains("[section:"));
        assert!(
            has_section_prefix,
            "expected section prefix in at least one chunk"
        );
    }

    #[test]
    fn plain_text_without_headers() {
        // Plain text (no markdown headers) should still produce chunks
        let paragraph = "Z".repeat(800);
        let text = format!("{paragraph}\n\n{paragraph}\n\n{paragraph}");

        let chunks = chunk_content(&text, &[], None);
        assert!(!chunks.is_empty(), "should produce chunks for plain text");
    }

    #[test]
    fn tags_only_on_first_chunk() {
        let body = "B".repeat(800);
        let text = format!(
            "# Title\n\n{body}\n\n## Section A\n\n{body}\n\n## Section B\n\n{body}\n\n## Section C\n\n{body}"
        );
        let chunks = chunk_content(&text, &["tag1".into()], None);
        assert!(chunks.len() >= 2);
        assert!(chunks[0].text.contains("[tags: tag1]"));
        for chunk in &chunks[1..] {
            assert!(
                !chunk.text.contains("[tags:"),
                "only first chunk should have tags"
            );
        }
    }

    #[test]
    fn no_infinite_loop_on_real_markdown() {
        let text = include_str!("../../tests/fixtures/next_steps.md");
        let chunks = chunk_content(text, &["dioxus".into()], None);
        assert!(!chunks.is_empty(), "should produce chunks");
        for (i, chunk) in chunks.iter().enumerate() {
            assert_eq!(chunk.index, i);
            assert!(!chunk.text.is_empty());
        }
    }

    #[test]
    fn multibyte_text_does_not_panic() {
        let segment = "日本語のテスト文章です。これは長いテキストの分割をテストします。";
        let text = std::iter::repeat_n(segment, 80)
            .collect::<Vec<_>>()
            .join("\n\n");

        let chunks = chunk_content(&text, &["テスト".into()], None);
        assert!(!chunks.is_empty());
        for (i, chunk) in chunks.iter().enumerate() {
            assert_eq!(chunk.index, i);
            assert!(!chunk.text.is_empty());
        }
    }

    // -- Code tests --

    #[test]
    fn chunks_rust_code() {
        let code = r#"
fn hello() {
    println!("hello");
}

fn world() {
    println!("world");
}
"#;
        let chunks = chunk_content(code, &[], Some("src/main.rs"));
        assert!(!chunks.is_empty());
        assert!(chunks[0].text.contains("[file: src/main.rs]"));
        assert!(chunks[0].text.contains("[language: rust]"));
    }

    #[test]
    fn unknown_extension_uses_markdown() {
        let result = chunk_content("# Hello\n\nworld", &[], Some("data.csv"));
        assert!(!result.is_empty());
        // Should NOT have a code header
        assert!(!result[0].text.contains("[language:"));
    }

    #[test]
    fn markdown_extension_uses_markdown_chunker() {
        let result = chunk_content("# Hello\n\nworld", &[], Some("docs/readme.md"));
        assert!(!result.is_empty());
        // Should NOT have a code header
        assert!(!result[0].text.contains("[language:"));
    }

    #[test]
    fn code_tags_on_first_chunk() {
        let code = "fn main() {}";
        let chunks = chunk_content(code, &["dioxus".into()], Some("src/main.rs"));
        assert!(chunks[0].text.contains("[tags: dioxus]"));
    }

    #[test]
    fn splits_large_code_file() {
        let functions: Vec<String> = (0..50)
            .map(|i| {
                format!(
                    "fn func_{i}() {{\n    let x = {i};\n    println!(\"{{x}}\");\n    \
                     let y = x * 2;\n    println!(\"{{y}}\");\n}}\n"
                )
            })
            .collect();
        let code = functions.join("\n");
        let chunks = chunk_content(&code, &[], Some("src/big.rs"));
        assert!(
            chunks.len() > 1,
            "large file should produce multiple chunks, got {}",
            chunks.len()
        );
        for (i, chunk) in chunks.iter().enumerate() {
            assert_eq!(chunk.index, i);
            assert!(!chunk.text.is_empty());
        }
    }

    #[test]
    fn chunks_python() {
        let code = "def hello():\n    print('hello')\n";
        let chunks = chunk_content(code, &[], Some("app.py"));
        assert!(!chunks.is_empty());
        assert!(chunks[0].text.contains("[language: python]"));
    }

    #[test]
    fn chunks_typescript() {
        let code = "function greet(name: string): void {\n  console.log(name);\n}\n";
        let chunks = chunk_content(code, &[], Some("app.ts"));
        assert!(!chunks.is_empty());
        assert!(chunks[0].text.contains("[language: typescript]"));
    }
}
