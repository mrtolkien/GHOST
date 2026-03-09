use serde_json::Value;

const DISPLAY_VALUE_MAX: usize = 80;

/// Human-readable summary of a tool call (before execution).
#[must_use]
pub fn display_request(tool_name: &str, args: &Value) -> String {
    match tool_name {
        "knowledge_search" => {
            let query = str_arg(args, "query");
            format!("\u{1F50D}\u{FE0E} \"{}\"", clip(&query, DISPLAY_VALUE_MAX))
        }
        "web_search" => {
            let query = str_arg(args, "query");
            format!("\u{1F310}\u{FE0E} \"{}\"", clip(&query, DISPLAY_VALUE_MAX))
        }
        "web_fetch" => {
            let url = str_arg(args, "url");
            let short = url
                .strip_prefix("https://")
                .or_else(|| url.strip_prefix("http://"))
                .unwrap_or(&url);
            format!("\u{1F4C4}\u{FE0E} {}", clip(short, DISPLAY_VALUE_MAX))
        }
        "run_shell_command" => {
            let cmd = str_arg(args, "command");
            let bg = args
                .get("background")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            let suffix = if bg { " &" } else { "" };
            format!("$ {}{}", clip(&cmd, DISPLAY_VALUE_MAX), suffix)
        }
        "read_file" => {
            let path = str_arg(args, "path");
            format!("\u{1F4D6}\u{FE0E} {}", clip(&path, DISPLAY_VALUE_MAX))
        }
        "write_file" => {
            let path = str_arg(args, "path");
            format!("\u{270F}\u{FE0E} {}", clip(&path, DISPLAY_VALUE_MAX))
        }
        "file_edit" => {
            let path = str_arg(args, "path");
            format!("\u{270F}\u{FE0E} {}", clip(&path, DISPLAY_VALUE_MAX))
        }
        "note_write" => {
            let title = str_arg(args, "title");
            format!(
                "\u{1F4DD}\u{FE0E} \"{}\"",
                clip(&title, DISPLAY_VALUE_MAX)
            )
        }
        "agent_control" => {
            let action = str_arg(args, "action");
            let agent = args
                .get("agent")
                .and_then(Value::as_str)
                .unwrap_or("");
            let agent_id = args
                .get("agent_id")
                .and_then(Value::as_str)
                .unwrap_or("");
            match action.as_str() {
                "start" => format!("\u{1F916}\u{FE0E} {agent}"),
                "status" => format!("\u{1F916}\u{FE0E} status {agent_id}"),
                "stop" => format!("\u{1F916}\u{FE0E} stop {agent_id}"),
                "continue" => format!("\u{1F916}\u{FE0E} continue {agent_id}"),
                _ => format!("\u{1F916}\u{FE0E} {action}"),
            }
        }
        "todo" => {
            // TODO tool display is handled separately by the TodoUpdated event.
            String::new()
        }
        _ => format!("`{tool_name}`"),
    }
}

/// Compact result hint (after execution).
#[must_use]
pub fn display_result(
    tool_name: &str,
    _args: &Value,
    result: &str,
    is_error: bool,
) -> String {
    if is_error {
        return "\u{2717} error".to_string();
    }
    match tool_name {
        "knowledge_search" => {
            if result.contains("No results") {
                "\u{2192} 0 results".to_string()
            } else if let Some(line) =
                result.lines().rev().find(|l| l.contains("results total"))
            {
                let count = line.split_whitespace().next().unwrap_or("?");
                format!("\u{2192} {count} results")
            } else {
                "\u{2192} done".to_string()
            }
        }
        "web_search" => {
            if result.contains("No results") {
                "\u{2192} 0 results".to_string()
            } else {
                let count = result
                    .lines()
                    .filter(|l| l.starts_with(|c: char| c.is_ascii_digit()))
                    .count();
                format!("\u{2192} {count} results")
            }
        }
        "web_fetch" => {
            let chars = result.len();
            format!("\u{2192} {}", format_size(chars))
        }
        "run_shell_command" => {
            if let Some(line) = result.lines().find(|l| l.starts_with("Exit code:")) {
                let code = line
                    .strip_prefix("Exit code: ")
                    .unwrap_or("?")
                    .trim();
                format!("# {code}")
            } else if result.contains("background") {
                "\u{2192} bg".to_string()
            } else if result.contains("timed out") {
                "\u{2717} timeout".to_string()
            } else {
                "\u{2713}".to_string()
            }
        }
        "read_file" => {
            let chars = result.len();
            format!("\u{2192} {}", format_size(chars))
        }
        "write_file" | "file_edit" => "\u{2713}".to_string(),
        "note_write" => "\u{2713}".to_string(),
        "agent_control" => {
            if result.contains("started") {
                "\u{2192} started".to_string()
            } else if result.contains("stopped") {
                "\u{2192} stopped".to_string()
            } else {
                "\u{2713}".to_string()
            }
        }
        _ => "\u{2713}".to_string(),
    }
}

/// Tool emoji for statusline breakdown (with VS15).
#[must_use]
pub fn tool_emoji(tool_name: &str) -> &'static str {
    match tool_name {
        "knowledge_search" => "\u{1F50D}\u{FE0E}",
        "web_search" => "\u{1F310}\u{FE0E}",
        "web_fetch" => "\u{1F4C4}\u{FE0E}",
        "run_shell_command" => "$",
        "read_file" => "\u{1F4D6}\u{FE0E}",
        "write_file" | "file_edit" => "\u{270F}\u{FE0E}",
        "note_write" => "\u{1F4DD}\u{FE0E}",
        "agent_control" => "\u{1F916}\u{FE0E}",
        "todo" => "\u{2713}",
        _ => "\u{2022}",
    }
}

fn str_arg(args: &Value, key: &str) -> String {
    args.get(key)
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string()
}

fn clip(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let truncated: String = s.chars().take(max - 1).collect();
        format!("{truncated}\u{2026}")
    }
}

fn format_size(chars: usize) -> String {
    if chars >= 1_000_000 {
        format!("{:.1}M chars", chars as f64 / 1_000_000.0)
    } else if chars >= 1_000 {
        format!("{:.1}k chars", chars as f64 / 1_000.0)
    } else {
        format!("{chars} chars")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn display_request_knowledge_search() {
        let args = json!({"query": "rust async patterns"});
        let result = display_request("knowledge_search", &args);
        assert!(result.contains("rust async patterns"));
    }

    #[test]
    fn display_request_shell_background() {
        let args = json!({"command": "cargo build", "background": true});
        let result = display_request("run_shell_command", &args);
        assert_eq!(result, "$ cargo build &");
    }

    #[test]
    fn display_request_clipping() {
        let long = "a".repeat(100);
        let args = json!({"query": long});
        let result = display_request("knowledge_search", &args);
        // Clipped query is inside quotes, so result ends with `…"`
        assert!(result.contains('\u{2026}'));
        assert!(result.len() < 110);
    }

    #[test]
    fn display_request_todo_is_empty() {
        let result = display_request("todo", &json!({}));
        assert!(result.is_empty());
    }

    #[test]
    fn display_result_error() {
        let result = display_result("anything", &json!({}), "boom", true);
        assert_eq!(result, "\u{2717} error");
    }

    #[test]
    fn display_result_shell_exit_code() {
        let result =
            display_result("run_shell_command", &json!({}), "Exit code: 0\nok", false);
        assert_eq!(result, "# 0");
    }

    #[test]
    fn format_size_ranges() {
        assert_eq!(format_size(500), "500 chars");
        assert_eq!(format_size(1_500), "1.5k chars");
        assert_eq!(format_size(2_500_000), "2.5M chars");
    }

    #[test]
    fn tool_emoji_known_tools() {
        assert_eq!(tool_emoji("web_search"), "\u{1F310}\u{FE0E}");
        assert_eq!(tool_emoji("run_shell_command"), "$");
        assert_eq!(tool_emoji("unknown_tool"), "\u{2022}");
    }

    #[test]
    fn display_request_web_fetch_strips_scheme() {
        let args = json!({"url": "https://example.com/path"});
        let result = display_request("web_fetch", &args);
        assert!(result.contains("example.com/path"));
        assert!(!result.contains("https://"));
    }

    #[test]
    fn display_request_unknown_tool() {
        let result = display_request("some_new_tool", &json!({}));
        assert_eq!(result, "`some_new_tool`");
    }

    #[test]
    fn display_result_knowledge_no_results() {
        let result = display_result(
            "knowledge_search",
            &json!({}),
            "No results found.",
            false,
        );
        assert_eq!(result, "\u{2192} 0 results");
    }

    #[test]
    fn display_result_knowledge_with_count() {
        let result = display_result(
            "knowledge_search",
            &json!({}),
            "...\n3 results total.",
            false,
        );
        assert_eq!(result, "\u{2192} 3 results");
    }
}
