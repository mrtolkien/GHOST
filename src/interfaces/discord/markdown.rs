/// Markdown-to-Components-v2 adapter.
///
/// Converts markdown text into a sequence of v2 component JSON values:
/// - Regular text -> `TextDisplay`
/// - Horizontal rules (`---`, `***`, `___`) -> `Separator`
/// - Tables -> `MediaGallery` with an attached PNG image (code-block fallback)
///
/// Code fences are tracked so that table/HR detection doesn't fire inside
/// fenced code blocks.
use super::components_v2::{self, TEXT_DISPLAY_LIMIT};

/// Output of markdown conversion: v2 components plus file attachments for
/// table images.
pub struct MarkdownComponents {
    pub components: Vec<serde_json::Value>,
    pub attachments: Vec<MarkdownAttachment>,
}

/// A file attachment produced during markdown conversion (table image).
pub struct MarkdownAttachment {
    pub filename: String,
    pub data: Vec<u8>,
}

/// Clean up a trailing Sources/References section for compact display.
///
/// Detects `## Sources`, `## References`, `**Sources**`, or `Sources:` sections
/// at the end of the text. Strips verbose per-source descriptions, keeping only
/// `[N] [Title](url)` lines. Passes through already-clean sections unchanged.
pub fn clean_citation_section(text: &str) -> String {
    let section_markers = ["## Sources", "## References", "**Sources**", "Sources:"];
    let section_start = section_markers
        .iter()
        .filter_map(|marker| text.rfind(marker))
        .max();

    let Some(start) = section_start else {
        return text.to_string();
    };

    let section = &text[start..];

    // If already in clean [N] [Title](url) format, pass through
    let titled_re = regex::Regex::new(r"\[\d+\]\s+\[[^\]]+\]\([^)]+\)")
        .expect("regex is a valid compile-time constant");
    if titled_re.is_match(section) {
        return text.to_string();
    }

    // Otherwise, extract URLs and reformat
    let url_re = regex::Regex::new(r"https?://[^\s\]\)>,]+")
        .expect("regex is a valid compile-time constant");
    let urls: Vec<&str> = url_re
        .find_iter(section)
        .map(|m| m.as_str().trim_end_matches(|c: char| ".,;:)".contains(c)))
        .collect();

    if urls.is_empty() {
        return text.to_string();
    }

    let before = text[..start].trim_end();
    let mut clean = before.to_string();
    clean.push_str("\n\n## Sources\n");
    for (i, url) in urls.iter().enumerate() {
        clean.push_str(&format!("[{}] {}\n", i + 1, url));
    }

    clean
}

/// Convert markdown text into v2 components and optional table-image
/// attachments.
pub fn markdown_to_v2_components(text: &str) -> MarkdownComponents {
    let text = &clean_citation_section(text);
    let mut components = Vec::new();
    let mut attachments = Vec::new();
    let mut text_buf = String::new();
    let mut in_fence = false;
    let mut table_buf: Vec<String> = Vec::new();
    let mut in_table = false;
    let mut table_counter = 0usize;

    for line in text.lines() {
        let trimmed = line.trim();

        // Track code fence open/close
        if trimmed.starts_with("```") {
            if in_table {
                flush_table(
                    &mut table_buf,
                    &mut text_buf,
                    &mut components,
                    &mut attachments,
                    &mut table_counter,
                );
                in_table = false;
            }
            in_fence = !in_fence;
            text_buf.push_str(line);
            text_buf.push('\n');
            continue;
        }

        if in_fence {
            text_buf.push_str(line);
            text_buf.push('\n');
            continue;
        }

        // Horizontal rule detection (outside code fences)
        if is_horizontal_rule(trimmed) {
            if in_table {
                flush_table(
                    &mut table_buf,
                    &mut text_buf,
                    &mut components,
                    &mut attachments,
                    &mut table_counter,
                );
                in_table = false;
            }
            flush_text(&mut text_buf, &mut components);
            components.push(components_v2::separator(true));
            continue;
        }

        // Table detection
        if is_table_line(trimmed) {
            if !in_table && is_table_separator(trimmed) {
                let prev = pop_last_line(&mut text_buf);
                if prev.as_deref().is_some_and(is_table_line) {
                    let prev = prev.expect("checked above");
                    flush_text(&mut text_buf, &mut components);
                    in_table = true;
                    table_buf.push(prev);
                    table_buf.push(line.to_string());
                    continue;
                }
                if let Some(prev) = prev {
                    text_buf.push_str(&prev);
                    text_buf.push('\n');
                }
            } else if in_table {
                table_buf.push(line.to_string());
                continue;
            }
        } else if in_table {
            flush_table(
                &mut table_buf,
                &mut text_buf,
                &mut components,
                &mut attachments,
                &mut table_counter,
            );
            in_table = false;
        }

        text_buf.push_str(line);
        text_buf.push('\n');
    }

    if in_table {
        flush_table(
            &mut table_buf,
            &mut text_buf,
            &mut components,
            &mut attachments,
            &mut table_counter,
        );
    }

    flush_text(&mut text_buf, &mut components);

    MarkdownComponents {
        components,
        attachments,
    }
}

fn is_horizontal_rule(trimmed: &str) -> bool {
    if trimmed.len() < 3 {
        return false;
    }
    let chars: Vec<char> = trimmed.chars().filter(|c| !c.is_whitespace()).collect();
    if chars.len() < 3 {
        return false;
    }
    let first = chars[0];
    if !matches!(first, '-' | '*' | '_') {
        return false;
    }
    chars.iter().all(|&c| c == first)
}

fn is_table_line(trimmed: &str) -> bool {
    trimmed.starts_with('|') && trimmed.ends_with('|') && trimmed.len() > 2
}

fn is_table_separator(trimmed: &str) -> bool {
    if !is_table_line(trimmed) {
        return false;
    }
    let inner = &trimmed[1..trimmed.len() - 1];
    inner
        .split('|')
        .all(|cell| cell.trim().chars().all(|c| matches!(c, '-' | ':' | ' ')))
}

fn pop_last_line(buf: &mut String) -> Option<String> {
    let content = buf.trim_end_matches('\n');
    if content.is_empty() {
        return None;
    }
    let last_nl = content.rfind('\n').map(|i| i + 1).unwrap_or(0);
    let line = content[last_nl..].to_string();
    buf.truncate(last_nl);
    Some(line)
}

fn flush_table(
    table_buf: &mut Vec<String>,
    text_buf: &mut String,
    components: &mut Vec<serde_json::Value>,
    attachments: &mut Vec<MarkdownAttachment>,
    table_counter: &mut usize,
) {
    if table_buf.is_empty() {
        return;
    }

    flush_text(text_buf, components);

    if let Some(png) = super::table_image::render_table_png(table_buf) {
        let filename = format!("table_{table_counter}.png");
        *table_counter += 1;
        components.push(components_v2::media_gallery(&filename));
        attachments.push(MarkdownAttachment {
            filename,
            data: png,
        });
        table_buf.clear();
    } else {
        let mut block = String::from("```\n");
        for line in table_buf.drain(..) {
            block.push_str(&line);
            block.push('\n');
        }
        block.push_str("```");
        components.push(components_v2::text_display(&block));
    }
}

fn flush_text(buf: &mut String, components: &mut Vec<serde_json::Value>) {
    let content = buf.trim();
    if content.is_empty() {
        buf.clear();
        return;
    }
    let content = buf.trim_end_matches('\n');

    if content.len() <= TEXT_DISPLAY_LIMIT {
        components.push(components_v2::text_display(content));
    } else {
        for chunk in split_text_display(content) {
            components.push(components_v2::text_display(&chunk));
        }
    }
    buf.clear();
}

fn split_text_display(content: &str) -> Vec<String> {
    let mut chunks = Vec::new();
    let mut current = String::new();

    for line in content.split_inclusive('\n') {
        if current.len() + line.len() > TEXT_DISPLAY_LIMIT && !current.is_empty() {
            chunks.push(current.trim_end_matches('\n').to_string());
            current = String::new();
        }
        current.push_str(line);
    }

    if !current.is_empty() {
        chunks.push(current.trim_end_matches('\n').to_string());
    }

    // Fallback: if a single line is too long, hard-split by char
    let mut result = Vec::new();
    for chunk in chunks {
        if chunk.len() <= TEXT_DISPLAY_LIMIT {
            result.push(chunk);
        } else {
            let mut piece = String::new();
            for ch in chunk.chars() {
                if piece.len() + ch.len_utf8() > TEXT_DISPLAY_LIMIT {
                    result.push(piece);
                    piece = String::new();
                }
                piece.push(ch);
            }
            if !piece.is_empty() {
                result.push(piece);
            }
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_text_becomes_single_text_display() {
        let out = markdown_to_v2_components("Hello world");
        assert_eq!(out.components.len(), 1);
        assert_eq!(out.components[0]["type"], 10);
        assert_eq!(out.components[0]["content"], "Hello world");
    }

    #[test]
    fn horizontal_rule_becomes_separator() {
        let input = "Before\n---\nAfter";
        let out = markdown_to_v2_components(input);
        assert_eq!(out.components.len(), 3);
        assert_eq!(out.components[0]["type"], 10);
        assert_eq!(out.components[1]["type"], 14);
        assert_eq!(out.components[2]["type"], 10);
    }

    #[test]
    fn triple_asterisk_is_horizontal_rule() {
        let out = markdown_to_v2_components("A\n***\nB");
        assert_eq!(out.components[1]["type"], 14);
    }

    #[test]
    fn triple_underscore_is_horizontal_rule() {
        let out = markdown_to_v2_components("A\n___\nB");
        assert_eq!(out.components[1]["type"], 14);
    }

    #[test]
    fn hr_inside_code_fence_is_preserved() {
        let out = markdown_to_v2_components("```\n---\n```");
        assert_eq!(out.components.len(), 1);
        assert_eq!(out.components[0]["type"], 10);
        let content = out.components[0]["content"].as_str().unwrap();
        assert!(content.contains("---"));
    }

    #[test]
    fn table_becomes_image_or_code_block_fallback() {
        let input = "Before\n| A | B |\n|---|---|\n| 1 | 2 |\nAfter";
        let out = markdown_to_v2_components(input);

        assert_eq!(out.components.first().unwrap()["content"], "Before");
        assert_eq!(out.components.last().unwrap()["content"], "After");

        let table_comp = &out.components[1];
        let is_image = table_comp["type"] == 12;
        let is_code_block = table_comp["type"] == 10
            && table_comp["content"]
                .as_str()
                .is_some_and(|s| s.contains("```"));
        assert!(
            is_image || is_code_block,
            "Table should be MediaGallery or code-block, got type={}",
            table_comp["type"]
        );

        if is_image {
            assert_eq!(out.attachments.len(), 1);
            assert!(out.attachments[0].filename.starts_with("table_"));
            assert_eq!(&out.attachments[0].data[..4], b"\x89PNG");
        }
    }

    #[test]
    fn table_inside_code_fence_not_transformed() {
        let input = "```\n| A | B |\n|---|---|\n| 1 | 2 |\n```";
        let out = markdown_to_v2_components(input);
        assert_eq!(out.components.len(), 1);
        assert!(out.attachments.is_empty());
        let content = out.components[0]["content"].as_str().unwrap();
        assert_eq!(content.matches("```").count(), 2);
    }

    #[test]
    fn empty_text_produces_no_components() {
        let out = markdown_to_v2_components("");
        assert!(out.components.is_empty());
    }

    #[test]
    fn only_whitespace_produces_no_components() {
        let out = markdown_to_v2_components("   \n\n  ");
        assert!(out.components.is_empty());
    }

    #[test]
    fn two_dashes_is_not_horizontal_rule() {
        let out = markdown_to_v2_components("A\n--\nB");
        assert_eq!(out.components.len(), 1);
        assert_eq!(out.components[0]["type"], 10);
    }

    #[test]
    fn hr_with_spaces() {
        assert!(is_horizontal_rule("- - -"));
        assert!(is_horizontal_rule("* * *"));
    }

    #[test]
    fn mixed_chars_not_hr() {
        assert!(!is_horizontal_rule("-*-"));
        assert!(!is_horizontal_rule("---a"));
    }

    #[test]
    fn multiple_separators() {
        let input = "A\n---\nB\n***\nC";
        let out = markdown_to_v2_components(input);
        assert_eq!(out.components.len(), 5);
        assert_eq!(out.components[1]["type"], 14);
        assert_eq!(out.components[3]["type"], 14);
    }

    #[test]
    fn clean_citation_section_passes_through_clean() {
        let text = "Answer.\n\n## Sources\n[1] [Title](https://example.com)\n";
        assert_eq!(clean_citation_section(text), text);
    }

    #[test]
    fn clean_citation_section_reformats_verbose() {
        let text = "Answer.\n\n## Sources\n\
            1. https://example.com/page - This is a really long description\n\
            2. https://other.com/article - Another verbose description\n";
        let cleaned = clean_citation_section(text);
        assert!(cleaned.contains("[1] https://example.com/page"));
        assert!(cleaned.contains("[2] https://other.com/article"));
        assert!(!cleaned.contains("really long description"));
    }

    #[test]
    fn clean_citation_section_no_sources() {
        let text = "Just an answer.";
        assert_eq!(clean_citation_section(text), text);
    }

    #[test]
    fn multiple_tables_get_distinct_filenames() {
        let input = "\
            Intro\n\
            | A | B |\n|---|---|\n| 1 | 2 |\n\
            Middle\n\
            | C | D |\n|---|---|\n| 3 | 4 |\n\
            End";
        let out = markdown_to_v2_components(input);

        // Count MediaGallery (type 12) or code-block fallback components.
        let table_components: Vec<_> = out
            .components
            .iter()
            .filter(|c| {
                c["type"] == 12
                    || (c["type"] == 10
                        && c["content"].as_str().is_some_and(|s| s.starts_with("```")))
            })
            .collect();
        assert_eq!(table_components.len(), 2, "should have 2 table components");

        if out.attachments.len() == 2 {
            // PNG rendering succeeded: filenames must be distinct.
            assert_eq!(out.attachments[0].filename, "table_0.png");
            assert_eq!(out.attachments[1].filename, "table_1.png");
            assert_ne!(
                out.attachments[0].data, out.attachments[1].data,
                "different tables should produce different PNGs"
            );
        }
    }
}
