use crate::db::sessions::MessageRecord;

/// Filter a transcript for agents: preserve user/assistant text,
/// preserve tool call names+inputs, strip tool results.
///
/// If `since` is provided (RFC 3339 timestamp), messages with
/// `created_at <= since` are excluded.
pub fn filter_transcript(messages: &[MessageRecord], since: Option<&str>) -> String {
    let mut lines = Vec::new();

    for msg in messages {
        if let Some(cutoff) = since
            && msg.created_at.as_str() <= cutoff
        {
            continue;
        }
        match msg.role.as_str() {
            "user" => {
                // Tool-result messages have tool_results set — skip them
                if msg.tool_results.is_some() {
                    continue;
                }
                if !msg.content.trim().is_empty() {
                    lines.push(format!("[user] {}", msg.content));
                }
            }
            "assistant" => {
                // Include reasoning summaries from raw_output
                if let Some(raw_items) = msg.raw_output_parsed() {
                    for item in &raw_items {
                        if item.get("original_type").and_then(|v| v.as_str()) == Some("reasoning")
                            && let Some(value) = item.get("value")
                        {
                            let summary = crate::providers::extract_reasoning_summary(value);
                            if !summary.is_empty() {
                                lines.push(format!("[reasoning] {summary}"));
                            }
                        }
                    }
                }
                if !msg.content.trim().is_empty() {
                    lines.push(format!("[assistant] {}", msg.content));
                }
                // Include tool call names + brief summary
                if let Some(calls) = msg.tool_calls_parsed() {
                    for call in &calls {
                        let name = call
                            .get("name")
                            .and_then(|v| v.as_str())
                            .unwrap_or("unknown");
                        let input = call
                            .get("input")
                            .map(|v| {
                                let s = v.to_string();
                                if s.len() > 200 {
                                    let end = s.floor_char_boundary(200);
                                    format!("{}...", &s[..end])
                                } else {
                                    s
                                }
                            })
                            .unwrap_or_default();
                        lines.push(format!("[tool_call] {name}({input})"));
                    }
                }
            }
            _ => {}
        }
    }

    lines.join("\n")
}

/// Extract the last substantial assistant message as "agent findings".
///
/// In agent sessions, the final assistant message typically contains the
/// synthesized research report. Returns `None` if no assistant message
/// has at least 500 chars of content.
#[must_use]
pub fn extract_agent_findings(messages: &[MessageRecord]) -> Option<String> {
    messages
        .iter()
        .rev()
        .find(|m| m.role == "assistant" && m.content.len() >= 500)
        .map(|m| m.content.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_message(
        role: &str,
        content: &str,
        tool_calls: Option<Vec<serde_json::Value>>,
        tool_results: Option<Vec<serde_json::Value>>,
    ) -> MessageRecord {
        MessageRecord {
            id: "test_msg".to_string(),
            session_id: "test_session".to_string(),
            role: role.to_string(),
            content: content.to_string(),
            tool_calls: tool_calls.map(|v| serde_json::to_string(&v).unwrap()),
            tool_results: tool_results.map(|v| serde_json::to_string(&v).unwrap()),
            raw_output: None,
            images: None,
            created_at: crate::db::now(),
        }
    }

    #[test]
    fn transcript_preserves_user_and_assistant_text() {
        let messages = vec![
            make_message("user", "Hello", None, None),
            make_message("assistant", "Hi there!", None, None),
        ];

        let result = filter_transcript(&messages, None);
        assert!(result.contains("[user] Hello"));
        assert!(result.contains("[assistant] Hi there!"));
    }

    #[test]
    fn transcript_preserves_tool_calls() {
        let tool_call = serde_json::json!({
            "name": "read_file",
            "input": {"path": "/tmp/test.txt"}
        });
        let messages = vec![make_message("assistant", "", Some(vec![tool_call]), None)];

        let result = filter_transcript(&messages, None);
        assert!(result.contains("[tool_call] read_file("));
        assert!(result.contains("/tmp/test.txt"));
    }

    #[test]
    fn transcript_strips_tool_results() {
        let tool_result = serde_json::json!({
            "tool_use_id": "123",
            "content": "file contents here very long..."
        });
        let messages = vec![
            make_message("user", "Do something", None, None),
            make_message("user", "", None, Some(vec![tool_result])),
        ];

        let result = filter_transcript(&messages, None);
        assert!(result.contains("[user] Do something"));
        assert!(!result.contains("file contents here"));
    }

    #[test]
    fn transcript_truncates_long_tool_inputs() {
        let long_input = "x".repeat(300);
        let tool_call = serde_json::json!({
            "name": "write_file",
            "input": {"content": long_input}
        });
        let messages = vec![make_message("assistant", "", Some(vec![tool_call]), None)];

        let result = filter_transcript(&messages, None);
        assert!(result.contains("..."));
    }

    #[test]
    fn extract_agent_findings_picks_last_long_message() {
        let messages = vec![
            make_message("user", "Research something", None, None),
            make_message("assistant", "Short reply", None, None),
            make_message("assistant", &"x".repeat(600), None, None),
        ];
        let findings = extract_agent_findings(&messages);
        assert!(findings.is_some());
        assert_eq!(findings.unwrap().len(), 600);
    }

    #[test]
    fn extract_agent_findings_returns_none_for_short_messages() {
        let messages = vec![
            make_message("user", "Hello", None, None),
            make_message("assistant", "Short", None, None),
        ];
        assert!(extract_agent_findings(&messages).is_none());
    }
}
