use clap::Subcommand;

use crate::chat::ChatError;
use crate::error::GhostError;

#[derive(Debug, Subcommand)]
pub enum SessionCommand {
    List,
    Logs {
        session_id: String,
        #[arg(long, default_value_t = 40)]
        count: usize,
        #[arg(long)]
        around: Option<String>,
    },
}

#[tracing::instrument(skip_all)]
pub async fn execute(command: SessionCommand) -> Result<(), GhostError> {
    let config = crate::config::load()?;
    crate::config::bootstrap_workspace(&config)?;
    let db = crate::db::connect(&config.workspace).await?;

    match command {
        SessionCommand::List => {
            let sessions = crate::db::sessions::list_recent_sessions(&db, 50).await?;
            for session in sessions {
                let message_count =
                    crate::db::sessions::count_messages_for_session(&db, &session.id).await?;
                let interface =
                    crate::db::sessions::get_interface_for_session(&db, &session.id).await?;
                println!(
                    "{}  {}  {} messages  {}  {}",
                    session.id,
                    render_date(&session.last_activity_at.to_string()),
                    message_count,
                    session.status,
                    interface.unwrap_or_else(|| "-".to_string())
                );
            }
            Ok(())
        }
        SessionCommand::Logs {
            session_id,
            count,
            around,
        } => {
            let session_thing = parse_session_thing(&session_id)?;
            let messages =
                crate::db::sessions::list_messages_by_session(&db, &session_thing).await?;

            let selected = if let Some(around_id) = around {
                let middle = messages
                    .iter()
                    .position(|message| message.id.to_string() == around_id);
                if let Some(index) = middle {
                    let half = count / 2;
                    let start = index.saturating_sub(half);
                    let end = (start + count).min(messages.len());
                    messages[start..end].to_vec()
                } else {
                    messages
                        .iter()
                        .rev()
                        .take(count)
                        .cloned()
                        .collect::<Vec<_>>()
                        .into_iter()
                        .rev()
                        .collect()
                }
            } else {
                messages
                    .iter()
                    .rev()
                    .take(count)
                    .cloned()
                    .collect::<Vec<_>>()
                    .into_iter()
                    .rev()
                    .collect()
            };

            for message in selected {
                println!(
                    "[{}] {}  {}",
                    message.id,
                    message.role.to_uppercase(),
                    message.content
                );
                if let Some(tool_calls) = message.tool_calls
                    && !tool_calls.is_empty()
                {
                    println!(
                        "         tools: {}",
                        serde_json::to_string(&tool_calls)
                            .unwrap_or_else(|_| "<invalid tool payload>".to_string())
                    );
                }
                if let Some(citations) = message.citations
                    && !citations.is_empty()
                {
                    println!(
                        "         cited: {}",
                        serde_json::to_string(&citations)
                            .unwrap_or_else(|_| "<invalid citation payload>".to_string())
                    );
                }
            }
            Ok(())
        }
    }
}

fn render_date(value: &str) -> String {
    value.chars().take(10).collect()
}

fn parse_session_thing(session_id: &str) -> Result<surrealdb::sql::Thing, GhostError> {
    if session_id.contains(':') {
        let mut parts = session_id.splitn(2, ':');
        let table = parts.next().unwrap_or_default();
        let id = parts.next().unwrap_or_default();
        if table.is_empty() || id.is_empty() {
            return Err(ChatError::InvalidSessionId {
                session_id: session_id.to_string(),
            }
            .into());
        }
        return Ok(surrealdb::sql::Thing::from((table, id)));
    }
    if session_id.trim().is_empty() {
        return Err(ChatError::InvalidSessionId {
            session_id: session_id.to_string(),
        }
        .into());
    }
    Ok(surrealdb::sql::Thing::from(("session", session_id)))
}
