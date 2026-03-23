use std::sync::Arc;

use serenity::http::Http;
use serenity::model::id::{ChannelId, MessageId};
use tokio::sync::mpsc::UnboundedReceiver;

use crate::chat::{RunMetadata, ToolCallInfo, ToolLoopEvent, ToolResultInfo};
use crate::tools::TodoItem;

use super::components_v2::{container, edit_v2_message, send_v2_message, text_display};

/// Catpuccin Mocha Overlay 0 — muted accent for tool call messages.
const TOOL_CALL_COLOR: u32 = 0x6C_70_86;

/// Neon Mauve — accent for TODO progress messages.
const TODO_COLOR: u32 = 0xA9_4D_FF;

/// Muted grey for compaction notices.
const COMPACTION_COLOR: u32 = 0x58_5B_70;

/// Renders tool loop events as Discord messages.
///
/// Each provider response (= each `ToolCalls` event) becomes its own
/// persistent message. TODO updates edit in-place.
pub struct DiscordUiRenderer {
    rx: UnboundedReceiver<ToolLoopEvent>,
    http: Arc<Http>,
    channel_id: ChannelId,
    todo_message_id: Option<MessageId>,
    tool_call_message_id: Option<MessageId>,
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
            tool_call_message_id: None,
        }
    }

    pub async fn run(mut self) {
        while let Some(event) = self.rx.recv().await {
            match event {
                ToolLoopEvent::ToolCalls { calls } => {
                    self.handle_tool_calls(&calls).await;
                }
                ToolLoopEvent::ToolResults { results } => {
                    self.handle_tool_results(&results).await;
                }
                ToolLoopEvent::TodoUpdated { items } => {
                    self.handle_todo_updated(&items).await;
                }
                ToolLoopEvent::Compacted => {
                    self.handle_compacted().await;
                }
            }
        }
    }

    async fn handle_tool_calls(&mut self, calls: &[ToolCallInfo]) {
        let lines: Vec<&str> = calls
            .iter()
            .filter(|c| !c.display.is_empty())
            .map(|c| c.display.as_str())
            .collect();
        if lines.is_empty() {
            return;
        }
        let display = lines.join("\n");

        let components = vec![container(
            vec![text_display(&display)],
            Some(TOOL_CALL_COLOR),
        )];

        match send_v2_message(&self.http, self.channel_id, &components, Vec::new()).await {
            Ok(msg) => self.tool_call_message_id = Some(msg.id),
            Err(e) => {
                tracing::warn!(error = e.to_string(), "failed to send tool call message")
            }
        }
    }

    async fn handle_tool_results(&mut self, results: &[ToolResultInfo]) {
        let Some(msg_id) = self.tool_call_message_id.take() else {
            return;
        };

        let lines: Vec<String> = results
            .iter()
            .filter(|r| !r.display_request.is_empty())
            .map(|r| format!("{}  {}", r.display_request, r.display_result))
            .collect();
        if lines.is_empty() {
            return;
        }
        let display = lines.join("\n");

        let components = vec![container(
            vec![text_display(&display)],
            Some(TOOL_CALL_COLOR),
        )];

        if let Err(e) = edit_v2_message(&self.http, self.channel_id, msg_id, &components).await {
            tracing::warn!(error = e.to_string(), "failed to edit tool result message");
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
                    tracing::warn!(error = e.to_string(), "failed to edit TODO message");
                }
            }
            None => {
                match send_v2_message(&self.http, self.channel_id, &components, Vec::new()).await {
                    Ok(msg) => self.todo_message_id = Some(msg.id),
                    Err(e) => {
                        tracing::warn!(error = e.to_string(), "failed to send TODO message");
                    }
                }
            }
        }
    }

    async fn handle_compacted(&self) {
        let components = vec![container(
            vec![text_display(
                "context compacted — older conversation was summarized \
                 to fit the model's context window",
            )],
            Some(COMPACTION_COLOR),
        )];
        if let Err(e) = send_v2_message(&self.http, self.channel_id, &components, Vec::new()).await
        {
            tracing::warn!(error = e.to_string(), "failed to send compaction message");
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

/// Build statusline v2 components: a separator + `-#` subtext line.
#[must_use]
pub fn format_statusline(metadata: &RunMetadata) -> Vec<serde_json::Value> {
    use super::components_v2::{separator, text_display};
    use crate::tools::display::tool_emoji;

    let mut parts = Vec::new();

    // Model alias in inline code
    parts.push(format!("`{}`", metadata.model_alias));

    // Token counts
    let mut tokens = format!(
        "{}↑ {}↓",
        format_token_count(metadata.input_tokens),
        format_token_count(metadata.output_tokens),
    );
    if metadata.cache_read_tokens > 0 {
        tokens.push_str(&format!(
            " {}⚡\u{FE0E}",
            format_token_count(metadata.cache_read_tokens)
        ));
    }
    parts.push(tokens);

    // Tool breakdown with emojis: 🔍︎×3 📄︎×2
    if !metadata.tool_counts.is_empty() {
        let mut tools: Vec<_> = metadata.tool_counts.iter().collect();
        tools.sort_by(|a, b| b.1.cmp(a.1).then_with(|| a.0.cmp(b.0)));
        let tool_parts: Vec<String> = tools
            .iter()
            .map(|(name, count)| format!("{}×{}", tool_emoji(name), count))
            .collect();
        parts.push(tool_parts.join(" "));
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

    vec![
        separator(true),
        text_display(&format!("-# {}", parts.join(" · "))),
    ]
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

    /// Extract text content from a TextDisplay component.
    fn text_content(component: &serde_json::Value) -> &str {
        component["content"].as_str().unwrap_or("")
    }

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
        let components = format_statusline(&metadata);
        assert_eq!(components.len(), 2);
        // First component is a separator
        assert_eq!(components[0]["type"], 14);
        // Second is a TextDisplay with -# subtext
        let text = text_content(&components[1]);
        assert!(text.starts_with("-# "), "expected subtext: {text}");
        assert!(text.contains("`primary`"));
        assert!(text.contains("500↑"));
        assert!(text.contains("200↓"));
        assert!(text.contains("1.5s"));
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
        let components = format_statusline(&metadata);
        let text = text_content(&components[1]);
        assert!(text.contains("12.5k↑"));
        assert!(text.contains("856↓"));
        assert!(text.contains("8.0k⚡"));
        assert!(text.contains("×3")); // knowledge_search count
        assert!(text.contains("×2")); // web_fetch count
        assert!(text.contains("4.2s"));
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
        let components = format_statusline(&metadata);
        let text = text_content(&components[1]);
        assert!(text.contains("2m34s"));
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
