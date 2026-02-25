use std::sync::Arc;

use serenity::http::Http;
use serenity::model::id::{ChannelId, MessageId};
use tokio::sync::mpsc::UnboundedReceiver;

use crate::chat::{RunMetadata, ToolLoopEvent};
use crate::tools::TodoItem;

use super::components_v2::{container, edit_v2_message, send_v2_message, text_display};

/// Muted accent color for tool call messages.
const TOOL_CALL_COLOR: u32 = 0x99_99_99;

/// Blurple accent color for TODO progress messages.
const TODO_COLOR: u32 = 0x58_65_F2;

/// State returned by the renderer after it finishes.
#[derive(Debug, Default)]
pub struct UiState {
    pub tool_message_id: Option<MessageId>,
    /// TODO message persists after the response (not deleted).
    #[allow(dead_code)]
    pub todo_message_id: Option<MessageId>,
}

/// Renders tool loop events as live-updating Discord messages.
///
/// Spawned as a background task by `bot.rs`. Receives events from
/// the tool loop and sends/edits Discord messages accordingly.
pub struct DiscordUiRenderer {
    rx: UnboundedReceiver<ToolLoopEvent>,
    http: Arc<Http>,
    channel_id: ChannelId,
    tool_names: Vec<String>,
    tool_message_id: Option<MessageId>,
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
            tool_names: Vec::new(),
            tool_message_id: None,
            todo_message_id: None,
        }
    }

    pub async fn run(mut self) -> UiState {
        while let Some(event) = self.rx.recv().await {
            match event {
                ToolLoopEvent::ToolCalls { names } => {
                    self.handle_tool_calls(names).await;
                }
                ToolLoopEvent::TodoUpdated { items } => {
                    self.handle_todo_updated(&items).await;
                }
            }
        }

        UiState {
            tool_message_id: self.tool_message_id,
            todo_message_id: self.todo_message_id,
        }
    }

    async fn handle_tool_calls(&mut self, names: Vec<String>) {
        self.tool_names.extend(names);

        let display = self
            .tool_names
            .iter()
            .map(|n| format!("`{n}`"))
            .collect::<Vec<_>>()
            .join(" ");

        let components = vec![container(
            vec![text_display(&display)],
            Some(TOOL_CALL_COLOR),
        )];

        match self.tool_message_id {
            Some(msg_id) => {
                if let Err(e) =
                    edit_v2_message(&self.http, self.channel_id, msg_id, &components).await
                {
                    logfire::warn!("failed to edit tool call message", error = e.to_string(),);
                }
            }
            None => {
                match send_v2_message(&self.http, self.channel_id, &components, Vec::new()).await {
                    Ok(msg) => self.tool_message_id = Some(msg.id),
                    Err(e) => {
                        logfire::warn!("failed to send tool call message", error = e.to_string(),);
                    }
                }
            }
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
}
