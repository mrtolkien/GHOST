use std::sync::Arc;

use serenity::http::Http;
use serenity::model::id::{ChannelId, MessageId};
use tokio::sync::mpsc::UnboundedReceiver;

use crate::chat::{RunMetadata, ToolCallInfo, ToolLoopEvent};
use crate::tools::TodoItem;

use super::components_v2::{container, edit_v2_message, send_v2_message, text_display};

/// Catpuccin Mocha Overlay 0 — muted accent for tool call messages.
const TOOL_CALL_COLOR: u32 = 0x6C_70_86;

/// Neon Mauve — accent for TODO progress messages.
const TODO_COLOR: u32 = 0xA9_4D_FF;

/// Renders tool loop events as Discord messages.
///
/// Each provider response (= each `ToolCalls` event) becomes its own
/// persistent message. TODO updates edit in-place.
pub struct DiscordUiRenderer {
    rx: UnboundedReceiver<ToolLoopEvent>,
    http: Arc<Http>,
    channel_id: ChannelId,
    todo_message_id: Option<MessageId>,
}

impl DiscordUiRenderer {
    pub fn new(
        rx: UnboundedReceiver<ToolLoopEvent>,
        http: Arc<Http>,
        channel_id: ChannelId,
    ) -> Self {
        Self {
            rx,
            http,
            channel_id,
            todo_message_id: None,
        }
    }

    pub async fn run(mut self) {
        while let Some(event) = self.rx.recv().await {
            match event {
                ToolLoopEvent::ToolCalls { calls } => {
                    self.handle_tool_calls(&calls).await;
                }
                ToolLoopEvent::TodoUpdated { items } => {
                    self.handle_todo_updated(&items).await;
                }
            }
        }
    }

    async fn handle_tool_calls(&self, calls: &[ToolCallInfo]) {
        let display = format_tool_calls(calls);
        let components = vec![container(
            vec![text_display(&display)],
            Some(TOOL_CALL_COLOR),
        )];

        if let Err(e) = send_v2_message(&self.http, self.channel_id, &components, Vec::new()).await
        {
            logfire::warn!("failed to send tool call message", error = e.to_string(),);
        }
    }

    async fn handle_todo_updated(&mut self, items: &[TodoItem]) {
        let formatted = format_todo_display(items);
        let components = vec![container(vec![text_display(&formatted)], Some(TODO_COLOR))];

        match self.todo_message_id {
            Some(msg_id) => {
                if let Err(e) =
                    edit_v2_message(&self.http, self.channel_id, msg_id, &components).await
                {
                    logfire::warn!("failed to edit TODO message", error = e.to_string(),);
                }
            }
            None => {
                match send_v2_message(&self.http, self.channel_id, &components, Vec::new()).await {
                    Ok(msg) => self.todo_message_id = Some(msg.id),
                    Err(e) => {
                        logfire::warn!("failed to send TODO message", error = e.to_string(),);
                    }
                }
            }
        }
    }
}

/// Format tool calls for display: each call on its own line with args.
fn format_tool_calls(calls: &[ToolCallInfo]) -> String {
    calls
        .iter()
        .map(|c| {
            if c.args_summary.is_empty() {
                format!("`{}`", c.name)
            } else {
                format!("`{}` {}", c.name, c.args_summary)
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Format a TODO list for Discord display.
fn format_todo_display(items: &[TodoItem]) -> String {
    use crate::tools::TodoStatus;

    let done = items
        .iter()
        .filter(|i| matches!(i.status, TodoStatus::Done | TodoStatus::Skipped))
        .count();
    let total = items.len();

    let mut out = format!("TODO [{done}/{total}]\n");
    for (i, item) in items.iter().enumerate() {
        let symbol = match item.status {
            TodoStatus::Pending => "\u{25CB}",    // ○
            TodoStatus::InProgress => "\u{25C9}", // ◉
            TodoStatus::Done => "\u{2713}",       // ✓
            TodoStatus::Skipped => "\u{2013}",    // –
        };
        out.push_str(&format!("{}. {} {}\n", i + 1, symbol, item.title));
    }
    out
}

/// Append a statusline to a response message.
#[must_use]
pub fn format_statusline(text: &str, metadata: &RunMetadata) -> String {
    let mut parts = Vec::new();

    // Model alias
    parts.push(metadata.model_alias.clone());

    // Token counts
    let tokens = format!(
        "{}↑ {}↓{}",
        format_token_count(metadata.input_tokens),
        format_token_count(metadata.output_tokens),
        if metadata.cache_read_tokens > 0 {
            format!(" {}⚡", format_token_count(metadata.cache_read_tokens))
        } else {
            String::new()
        },
    );
    parts.push(tokens);

    // Tool breakdown: "2 web_fetch · 3 knowledge_search"
    if !metadata.tool_counts.is_empty() {
        let mut tools: Vec<_> = metadata.tool_counts.iter().collect();
        tools.sort_by(|a, b| b.1.cmp(a.1).then_with(|| a.0.cmp(b.0)));
        let tool_parts: Vec<String> = tools
            .iter()
            .map(|(name, count)| format!("{count} {name}"))
            .collect();
        parts.push(tool_parts.join(" · "));
    }

    // Duration
    let secs = metadata.duration.as_secs_f64();
    let duration = if secs >= 60.0 {
        let mins = secs as u64 / 60;
        let remaining = secs as u64 % 60;
        format!("{mins}m{remaining:02}s")
    } else {
        format!("{secs:.1}s")
    };
    parts.push(duration);

    format!("{text}\n─\n`{}`", parts.join(" | "))
}

/// Format a compact agent completion summary for a v2 container.
#[must_use]
pub fn format_agent_summary(
    agent_name: &str,
    metadata: &RunMetadata,
    findings: Option<&str>,
) -> String {
    let mut line = format!("**{agent_name}** completed");

    // Tool breakdown
    if !metadata.tool_counts.is_empty() {
        let mut tools: Vec<_> = metadata.tool_counts.iter().collect();
        tools.sort_by(|a, b| b.1.cmp(a.1).then_with(|| a.0.cmp(b.0)));
        let tool_parts: Vec<String> = tools
            .iter()
            .map(|(name, count)| format!("{count} {name}"))
            .collect();
        line.push_str(&format!(" | {}", tool_parts.join(" · ")));
    }

    // Duration
    let secs = metadata.duration.as_secs_f64();
    let duration = if secs >= 60.0 {
        let mins = secs as u64 / 60;
        let remaining = secs as u64 % 60;
        format!("{mins}m{remaining:02}s")
    } else {
        format!("{secs:.1}s")
    };
    line.push_str(&format!(" | {duration}"));

    // Truncated findings snippet
    if let Some(text) = findings {
        let trimmed = text.trim();
        if !trimmed.is_empty() {
            let snippet: String = trimmed.chars().take(120).collect();
            let ellipsis = if trimmed.len() > 120 { "\u{2026}" } else { "" };
            line.push_str(&format!("\n{snippet}{ellipsis}"));
        }
    }

    line
}

fn format_token_count(count: u32) -> String {
    if count >= 1_000_000 {
        format!("{:.1}M", count as f64 / 1_000_000.0)
    } else if count >= 1_000 {
        format!("{:.1}k", count as f64 / 1_000.0)
    } else {
        count.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::time::Duration;

    #[test]
    fn statusline_no_tools() {
        let metadata = RunMetadata {
            model_alias: "primary".to_string(),
            iterations: 0,
            tool_counts: HashMap::new(),
            input_tokens: 500,
            output_tokens: 200,
            cache_read_tokens: 0,
            duration: Duration::from_secs_f64(1.5),
        };
        let result = format_statusline("Hello", &metadata);
        assert_eq!(result, "Hello\n─\n`primary | 500↑ 200↓ | 1.5s`");
    }

    #[test]
    fn statusline_with_tools_and_cache() {
        let mut tool_counts = HashMap::new();
        tool_counts.insert("web_fetch".to_string(), 2);
        tool_counts.insert("knowledge_search".to_string(), 3);
        let metadata = RunMetadata {
            model_alias: "primary".to_string(),
            iterations: 4,
            tool_counts,
            input_tokens: 12_500,
            output_tokens: 856,
            cache_read_tokens: 8_000,
            duration: Duration::from_secs_f64(4.2),
        };
        let result = format_statusline("Done", &metadata);
        assert!(result.starts_with("Done\n─\n`primary | "));
        assert!(result.contains("12.5k↑"));
        assert!(result.contains("856↓"));
        assert!(result.contains("8.0k⚡"));
        assert!(result.contains("3 knowledge_search"));
        assert!(result.contains("2 web_fetch"));
        assert!(result.contains("4.2s"));
    }

    #[test]
    fn statusline_long_duration() {
        let metadata = RunMetadata {
            model_alias: "primary".to_string(),
            iterations: 0,
            tool_counts: HashMap::new(),
            input_tokens: 1_000,
            output_tokens: 500,
            cache_read_tokens: 0,
            duration: Duration::from_secs(154),
        };
        let result = format_statusline("Done", &metadata);
        assert!(result.contains("2m34s"));
    }

    #[test]
    fn format_token_count_values() {
        assert_eq!(format_token_count(500), "500");
        assert_eq!(format_token_count(1_200), "1.2k");
        assert_eq!(format_token_count(12_500), "12.5k");
        assert_eq!(format_token_count(1_500_000), "1.5M");
    }

    #[test]
    fn todo_display_formatting() {
        let items = vec![
            TodoItem {
                title: "Research API".to_string(),
                description: None,
                status: crate::tools::TodoStatus::Done,
                note: None,
            },
            TodoItem {
                title: "Write code".to_string(),
                description: None,
                status: crate::tools::TodoStatus::InProgress,
                note: None,
            },
            TodoItem {
                title: "Add tests".to_string(),
                description: None,
                status: crate::tools::TodoStatus::Pending,
                note: None,
            },
        ];
        let result = format_todo_display(&items);
        assert!(result.starts_with("TODO [1/3]\n"));
        assert!(result.contains("1. ✓ Research API"));
        assert!(result.contains("2. ◉ Write code"));
        assert!(result.contains("3. ○ Add tests"));
    }

    #[test]
    fn tool_calls_display() {
        let calls = vec![
            ToolCallInfo {
                name: "web_search".to_string(),
                args_summary: "query: latest rust news".to_string(),
            },
            ToolCallInfo {
                name: "read_file".to_string(),
                args_summary: "path: /src/main.rs".to_string(),
            },
        ];
        let result = format_tool_calls(&calls);
        assert_eq!(
            result,
            "`web_search` query: latest rust news\n`read_file` path: /src/main.rs"
        );
    }

    #[test]
    fn tool_calls_no_args() {
        let calls = vec![ToolCallInfo {
            name: "list_files".to_string(),
            args_summary: String::new(),
        }];
        let result = format_tool_calls(&calls);
        assert_eq!(result, "`list_files`");
    }

    #[test]
    fn agent_summary_compact() {
        let mut tool_counts = HashMap::new();
        tool_counts.insert("web_fetch".to_string(), 7);
        tool_counts.insert("web_search".to_string(), 5);
        let metadata = RunMetadata {
            model_alias: "primary".to_string(),
            iterations: 10,
            tool_counts,
            input_tokens: 45_000,
            output_tokens: 8_000,
            cache_read_tokens: 32_000,
            duration: Duration::from_secs(154),
        };
        let result = format_agent_summary("deep-research", &metadata, Some("Found 3 papers"));
        assert!(result.starts_with("**deep-research** completed"));
        assert!(result.contains("7 web_fetch"));
        assert!(result.contains("5 web_search"));
        assert!(result.contains("2m34s"));
        assert!(result.contains("Found 3 papers"));
    }

    #[test]
    fn agent_summary_no_findings() {
        let metadata = RunMetadata {
            model_alias: "primary".to_string(),
            iterations: 1,
            tool_counts: HashMap::new(),
            input_tokens: 1_000,
            output_tokens: 500,
            cache_read_tokens: 0,
            duration: Duration::from_secs_f64(3.2),
        };
        let result = format_agent_summary("quick-task", &metadata, None);
        assert_eq!(result, "**quick-task** completed | 3.2s");
    }
}
