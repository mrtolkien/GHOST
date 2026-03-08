use std::path::Path;

use sqlx::SqlitePool;
use tracing::warn;

use crate::db;
use crate::db::sessions::MessageRecord;

/// Max characters for tool call arguments / tool result output in transcript.
const TRUNCATE_LEN: usize = 2000;

/// Slugify the first few words of a feedback message for folder naming.
pub fn make_slug(message: &str) -> String {
    message
        .split_whitespace()
        .take(5)
        .collect::<Vec<_>>()
        .join("-")
        .to_lowercase()
        .chars()
        .filter(|c| c.is_alphanumeric() || *c == '-')
        .collect::<String>()
        .trim_matches('-')
        .to_string()
}

/// Save a feedback snapshot: feedback.md, transcript.md, ghost.db copy.
pub async fn save_feedback(
    workspace: &Path,
    feedback_dir: &Path,
    db: &SqlitePool,
    session_id: &str,
    message: &str,
) -> Result<String, std::io::Error> {
    tokio::fs::create_dir_all(feedback_dir).await?;

    // feedback.md
    let timestamp = chrono::Utc::now().to_rfc3339();
    let feedback_md = format!(
        "# Feedback\n\n\
         **Timestamp:** {timestamp}\n\
         **Session ID:** {session_id}\n\n\
         ## Issue\n\n\
         {message}\n"
    );
    tokio::fs::write(feedback_dir.join("feedback.md"), &feedback_md).await?;

    // transcript.md
    let transcript = match render_transcript(db, session_id).await {
        Ok(t) => t,
        Err(e) => {
            warn!("Failed to render transcript: {e}");
            format!("(failed to render transcript: {e})")
        }
    };
    tokio::fs::write(feedback_dir.join("transcript.md"), &transcript).await?;

    // ghost.db copy
    let db_src = workspace.join("ghost.db");
    if db_src.exists() {
        tokio::fs::copy(&db_src, feedback_dir.join("ghost.db")).await?;
    }

    // Return the folder name for the ephemeral reply
    let folder_name = feedback_dir
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    Ok(folder_name)
}

async fn render_transcript(
    db: &SqlitePool,
    session_id: &str,
) -> Result<String, crate::db::DatabaseError> {
    let messages = db::sessions::get_last_n_messages(db, session_id, 10).await?;
    let mut out = String::from("# Session Transcript\n\n");
    out.push_str(&format!("Session: {session_id}\n"));
    out.push_str(&format!("Messages: {} (last 10)\n\n", messages.len()));

    for msg in &messages {
        render_message(&mut out, msg);
    }

    Ok(out)
}

fn render_message(out: &mut String, msg: &MessageRecord) {
    out.push_str(&format!("---\n\n### {} — {}\n\n", msg.role, msg.created_at));

    // Content
    if !msg.content.is_empty() {
        out.push_str(&msg.content);
        out.push_str("\n\n");
    }

    // Tool calls
    if let Some(calls) = msg.tool_calls_parsed() {
        for call in calls {
            let name = call
                .get("function")
                .and_then(|f| f.get("name"))
                .or_else(|| call.get("name"))
                .and_then(|v| v.as_str())
                .unwrap_or("?");
            let args = call
                .get("function")
                .and_then(|f| f.get("arguments"))
                .or_else(|| call.get("input"))
                .map(|v| truncate(&format!("{v}"), TRUNCATE_LEN))
                .unwrap_or_default();
            out.push_str(&format!("**Tool call:** `{name}`\n```\n{args}\n```\n\n"));
        }
    }

    // Tool results
    if let Some(results) = msg.tool_results_parsed() {
        for res in results {
            let output = res
                .get("output")
                .or_else(|| res.get("content"))
                .map(|v| truncate(&format!("{v}"), TRUNCATE_LEN))
                .unwrap_or_default();
            out.push_str(&format!("**Tool result:**\n```\n{output}\n```\n\n"));
        }
    }
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        format!("{}…(truncated)", &s[..max])
    }
}
