/// Claude Code canonical tool name translation.
///
/// Maps Ghost tool names to Claude Code canonical names before sending
/// requests to the Anthropic API via OAuth (stealth mode), and back again
/// when processing responses.
const CANONICAL_NAMES: &[&str] = &[
    "Read",
    "Write",
    "Edit",
    "Bash",
    "Grep",
    "Glob",
    "AskUserQuestion",
    "EnterPlanMode",
    "ExitPlanMode",
    "KillShell",
    "NotebookEdit",
    "Skill",
    "Task",
    "TaskOutput",
    "TodoWrite",
    "WebFetch",
    "WebSearch",
];

/// Map a Ghost tool name to its Claude Code canonical form.
/// Case-insensitive match. Unknown names pass through unchanged.
pub(crate) fn to_claude_code_name(name: &str) -> String {
    let lower = name.to_lowercase();
    for &canonical in CANONICAL_NAMES {
        if canonical.to_lowercase() == lower {
            return canonical.to_string();
        }
    }
    name.to_string()
}

/// Map a Claude Code canonical name back to the original Ghost tool name.
/// Searches `ghost_tool_names` for a case-insensitive match against the
/// canonical name. Falls back to returning `canonical` as-is.
pub(crate) fn from_claude_code_name(canonical: &str, ghost_tool_names: &[&str]) -> String {
    let lower = canonical.to_lowercase();
    for &ghost_name in ghost_tool_names {
        if ghost_name.to_lowercase() == lower {
            return ghost_name.to_string();
        }
    }
    canonical.to_string()
}

/// Normalize a tool call ID for Anthropic compatibility.
/// Strips non-`[a-zA-Z0-9_-]` characters (replaces with `_`) and
/// truncates to 64 chars. Per pi-mono.
pub(crate) fn normalize_tool_call_id(id: &str) -> String {
    let normalized: String = id
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect();
    if normalized.len() > 64 {
        normalized[..64].to_string()
    } else {
        normalized
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn to_claude_code_name_case_insensitive() {
        assert_eq!(to_claude_code_name("read"), "Read");
        assert_eq!(to_claude_code_name("BASH"), "Bash");
        assert_eq!(to_claude_code_name("webfetch"), "WebFetch");
        assert_eq!(to_claude_code_name("WebSearch"), "WebSearch");
    }

    #[test]
    fn to_claude_code_name_passthrough_unknown() {
        assert_eq!(to_claude_code_name("my_custom_tool"), "my_custom_tool");
    }

    #[test]
    fn from_claude_code_name_reverses() {
        let ghost_tools = &["file_read", "shell", "search"];
        // No match — returns as-is
        assert_eq!(from_claude_code_name("Read", ghost_tools), "Read");
    }

    #[test]
    fn from_claude_code_name_finds_original() {
        let ghost_tools = &["read", "bash", "grep"];
        assert_eq!(from_claude_code_name("Read", ghost_tools), "read");
        assert_eq!(from_claude_code_name("Bash", ghost_tools), "bash");
    }

    #[test]
    fn normalize_tool_call_id_strips_and_truncates() {
        assert_eq!(normalize_tool_call_id("abc-123_def"), "abc-123_def");
        assert_eq!(normalize_tool_call_id("a|b|c"), "a_b_c");
        let long = "a".repeat(100);
        assert_eq!(normalize_tool_call_id(&long).len(), 64);
    }
}
