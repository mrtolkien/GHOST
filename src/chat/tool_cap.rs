use std::path::Path;

use crate::providers::ContentBlock;

/// Result of capping a tool result that exceeded the limit.
pub(super) struct CappedToolResult {
    /// Head+tail preview with a `{path}` placeholder for the overflow file path.
    pub preview: String,
    /// The original full content to write to the overflow file.
    pub full_content: String,
}

/// Check if a tool result exceeds the byte limit. Returns `None` if the
/// content fits within `max_bytes`, or a `CappedToolResult` with a head+tail
/// preview and the full content for overflow storage.
///
/// The preview contains a `{path}` placeholder that the caller must replace
/// with the actual overflow file path before storing.
pub(super) fn cap_tool_result(content: &str, max_bytes: usize) -> Option<CappedToolResult> {
    if content.len() <= max_bytes {
        return None;
    }

    let head_budget = max_bytes * 7 / 10; // 70%
    let tail_budget = max_bytes - head_budget; // 30%

    let head_end = safe_truncate(content, head_budget);
    let tail_start = safe_truncate_back(content, tail_budget);

    let preview = format!(
        "{}\n\n... [full output saved to {{path}}] ...\n\n{}",
        &content[..head_end],
        &content[tail_start..],
    );

    Some(CappedToolResult {
        preview,
        full_content: content.to_string(),
    })
}

/// Write the full content to an overflow file and return the preview with the
/// path filled in. The file is named `{tool_use_id}.txt` under the
/// `.tool-overflow/` workspace directory.
pub(super) async fn write_overflow_file(
    workspace: &Path,
    message_id: &str,
    capped: CappedToolResult,
) -> String {
    let overflow_dir = workspace.join(".tool-overflow");
    let filename = format!("{message_id}.txt");
    let file_path = overflow_dir.join(&filename);

    match tokio::fs::write(&file_path, &capped.full_content).await {
        Ok(()) => capped
            .preview
            .replace("{path}", &format!(".tool-overflow/{filename}")),
        Err(e) => {
            tracing::warn!(
                error = %e,
                path = %file_path.display(),
                "Failed to write tool overflow file — storing uncapped",
            );
            capped.full_content
        }
    }
}

/// Cap any oversized tool result text in the content blocks. Writes overflow
/// files for results that exceed the limit. Returns the (possibly modified)
/// blocks.
pub(super) async fn cap_content_blocks(
    blocks: Vec<ContentBlock>,
    workspace: &Path,
    max_bytes: usize,
) -> Vec<ContentBlock> {
    let mut result = Vec::with_capacity(blocks.len());
    for block in blocks {
        match block {
            ContentBlock::ToolResult {
                tool_use_id,
                content,
                is_error,
            } => {
                let content = match cap_tool_result(&content, max_bytes) {
                    Some(capped) => {
                        write_overflow_file(workspace, &tool_use_id, capped).await
                    }
                    None => content,
                };
                result.push(ContentBlock::ToolResult {
                    tool_use_id,
                    content,
                    is_error,
                });
            }
            other => result.push(other),
        }
    }
    result
}

/// Find the largest byte index <= `max_bytes` that is a valid char boundary.
fn safe_truncate(s: &str, max_bytes: usize) -> usize {
    if max_bytes >= s.len() {
        return s.len();
    }
    let mut end = max_bytes;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    end
}

/// Find the smallest byte index such that `s[index..]` is at most `max_bytes`
/// and starts on a char boundary.
fn safe_truncate_back(s: &str, max_bytes: usize) -> usize {
    if max_bytes >= s.len() {
        return 0;
    }
    let mut start = s.len() - max_bytes;
    while start < s.len() && !s.is_char_boundary(start) {
        start += 1;
    }
    start
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_content_passes_through() {
        let result = cap_tool_result("hello world", 100);
        assert!(result.is_none(), "should return None when under limit");
    }

    #[test]
    fn long_content_produces_head_tail_preview() {
        let content = "A".repeat(70) + &"B".repeat(30) + &"C".repeat(100);
        let result = cap_tool_result(&content, 100).unwrap();

        // Head is 70% of cap = 70 chars
        assert!(result.preview.starts_with(&"A".repeat(70)));
        // Tail is 30% of cap = 30 chars
        assert!(result.preview.ends_with(&"C".repeat(30)));
        // Contains marker
        assert!(result.preview.contains("full output saved to"));
        // Full content preserved
        assert_eq!(result.full_content, content);
    }

    #[test]
    fn preview_format_includes_placeholder_path() {
        let content = "X".repeat(200);
        let result = cap_tool_result(&content, 100).unwrap();

        // Placeholder uses {path} which the caller replaces
        assert!(result.preview.contains("{path}"));
    }

    #[test]
    fn head_tail_split_at_char_boundaries() {
        // Multi-byte UTF-8: each char is 4 bytes
        let content = "🎉".repeat(100);
        let result = cap_tool_result(&content, 40).unwrap();

        // Should not panic on char boundary issues
        assert!(result.preview.starts_with("🎉"));
        assert!(result.preview.ends_with("🎉"));
    }
}
