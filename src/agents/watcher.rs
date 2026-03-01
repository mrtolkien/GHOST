use std::sync::Arc;
use std::time::Duration;

use tokio::sync::watch;
use tokio::task::JoinHandle;

use crate::chat::SessionChat;
use crate::db;
use crate::db::GhostDb;
use crate::interfaces::discord::DiscordSender;
use crate::scripting::AgentContext;
use crate::scripting::types::AgentTrigger;

use super::loader::{discover_agents, load_agent, load_agent_with_host};
use super::runner::AgentRunner;

const POLL_INTERVAL_SECS: u64 = 3;

/// Poll for completed agents and inject their findings into parent sessions.
pub fn spawn_agent_watcher(
    agent_runner: Arc<AgentRunner>,
    session_chat: Arc<SessionChat>,
    discord_sender: Arc<DiscordSender>,
    db: GhostDb,
    workspace: std::path::PathBuf,
    mut shutdown: watch::Receiver<bool>,
) -> JoinHandle<()> {
    logfire::info!("agent watcher started");

    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(POLL_INTERVAL_SECS));

        loop {
            tokio::select! {
                _ = interval.tick() => {
                    check_completed_agents(
                        &agent_runner,
                        &session_chat,
                        &discord_sender,
                        &db,
                        &workspace,
                    ).await;
                }
                _ = shutdown.changed() => {
                    logfire::info!("agent watcher shutting down");
                    break;
                }
            }
        }
    })
}

/// Poll for completed agent tasks and handle their results: inject findings
/// into the parent session, notify Discord, trigger a continuation chat turn,
/// and spawn post-agent hooks for after_agent agents.
async fn check_completed_agents(
    agent_runner: &AgentRunner,
    session_chat: &SessionChat,
    discord_sender: &DiscordSender,
    db: &GhostDb,
    workspace: &std::path::Path,
) {
    let agent_ids = agent_runner.list_agent_ids().await;

    for agent_id in agent_ids {
        let Some((status, parent_session)) = agent_runner.take_completed(&agent_id).await else {
            continue;
        };

        logfire::info!(
            "agent completed, injecting findings",
            agent_name = status.agent_name.clone(),
            agent_id = status.agent_id.clone(),
        );

        let findings = status
            .findings
            .as_deref()
            .unwrap_or("Agent completed without producing findings.");

        let Some(parent_id) = parent_session else {
            logfire::warn!(
                "completed agent has no parent session, skipping injection",
                agent_id = status.agent_id.clone(),
            );
            continue;
        };

        // Inject findings as a system message in the parent session
        let system_msg = format!("[agent:{} completed]\n\n{findings}", status.agent_name);
        if let Err(e) = db::sessions::create_message(db, &parent_id, "system", &system_msg).await {
            logfire::error!(
                "failed to inject agent findings into parent session",
                error = e.to_string(),
            );
            continue;
        }

        // Resolve Discord channel for this parent session
        let channel = db::sessions::get_interface_for_session(db, &parent_id)
            .await
            .ok()
            .flatten();
        let discord_channel_id = channel.as_deref().and_then(parse_discord_channel_id);

        // Send compact agent summary to Discord
        if let Some(channel_id) = discord_channel_id
            && let Some(ref metadata) = status.metadata
        {
            let findings_snippet = status.findings.as_deref();
            let summary = crate::interfaces::discord::ui_events::format_agent_summary(
                &status.agent_name,
                metadata,
                findings_snippet,
            );
            let _ = discord_sender
                .send_compact_container(channel_id, &summary, None)
                .await;
        }

        // Trigger a new chat turn with a synthetic user message
        let trigger = "[system] Research agent completed.";
        match session_chat.chat(&parent_id, trigger, None).await {
            Ok((result, _metadata)) => {
                if let Some(channel_id) = discord_channel_id
                    && let Err(e) = discord_sender
                        .send_to_channel(channel_id, &result.message)
                        .await
                {
                    logfire::error!(
                        "failed to send agent findings to Discord",
                        error = e.to_string(),
                    );
                }
            }
            Err(e) => {
                logfire::error!(
                    "failed to trigger chat turn after agent completion",
                    error = e.to_string(),
                );
            }
        }

        // Discover and run after_agent Lua agents
        let completed_agent_name = status.agent_name.clone();
        let agent_session_thing = parse_agent_session_thing(&agent_id);
        let after_agents = find_after_agent_agents(workspace);

        for after_agent_name in after_agents {
            // Skip self-triggering
            if after_agent_name == completed_agent_name {
                continue;
            }

            let Some(ref session_thing) = agent_session_thing else {
                continue;
            };

            // Check if the after_agent should continue the trigger session
            let after_config = match load_agent(workspace, &after_agent_name) {
                Ok(c) => c,
                Err(_) => continue,
            };
            let continue_session = after_config.continue_trigger_session;

            // Check should_trigger hook
            if after_config.has_should_trigger {
                match load_agent_with_host(workspace, &after_agent_name) {
                    Ok((_config, host)) => {
                        let ctx = AgentContext {
                            db: db.clone(),
                            workspace: workspace.to_path_buf(),
                            agent_slug: after_agent_name.clone(),
                            session_id: String::new(),
                            trigger_session_id: Some(session_thing.clone()),
                            trigger_agent_name: Some(completed_agent_name.clone()),
                        };
                        match host.call_should_trigger(ctx) {
                            Ok(false) => {
                                logfire::debug!(
                                    "after_agent skipped by should_trigger",
                                    agent_name = after_agent_name.clone(),
                                    trigger_agent = completed_agent_name.clone(),
                                );
                                continue;
                            }
                            Err(e) => {
                                logfire::warn!(
                                    "should_trigger hook error, proceeding anyway",
                                    agent_name = after_agent_name.clone(),
                                    error = e.to_string(),
                                );
                            }
                            Ok(true) => {}
                        }
                    }
                    Err(e) => {
                        logfire::warn!(
                            "failed to load agent for should_trigger check",
                            agent_name = after_agent_name.clone(),
                            error = e.to_string(),
                        );
                    }
                }
            }

            let agent_runner = agent_runner.clone();
            let after_name = after_agent_name.clone();
            let thing = session_thing.clone();

            tokio::spawn(async move {
                if continue_session {
                    // Continue the completed agent's session with the after_agent's config
                    match agent_runner
                        .continue_to_completion(
                            &thing,
                            "Continue with post-processing.",
                            Some(&after_name),
                        )
                        .await
                    {
                        Ok((_findings, _meta)) => {
                            logfire::info!(
                                "after_agent completed (continued session)",
                                agent_name = after_name,
                            );
                        }
                        Err(e) => {
                            logfire::error!(
                                "after_agent failed (continued session)",
                                agent_name = after_name,
                                error = e.to_string(),
                            );
                        }
                    }
                } else {
                    // Start a fresh session
                    match agent_runner
                        .run_to_completion(&after_name, "Run post-agent processing.", Some(&thing))
                        .await
                    {
                        Ok((_findings, _meta)) => {
                            logfire::info!("after_agent completed", agent_name = after_name,);
                        }
                        Err(e) => {
                            logfire::error!(
                                "after_agent failed",
                                agent_name = after_name,
                                error = e.to_string(),
                            );
                        }
                    }
                }
            });
        }
    }
}

/// Find all Lua agents with trigger=after_agent.
fn find_after_agent_agents(workspace: &std::path::Path) -> Vec<String> {
    let agents = discover_agents(workspace);
    agents
        .into_iter()
        .filter_map(|info| {
            let config = load_agent(workspace, &info.name).ok()?;
            if matches!(config.trigger, AgentTrigger::AfterAgent) {
                Some(config.name)
            } else {
                None
            }
        })
        .collect()
}

/// Parse "session:abc123" into a bare ID string.
fn parse_agent_session_thing(agent_id: &str) -> Option<String> {
    if let Some((_table, id)) = agent_id.split_once(':') {
        if id.is_empty() {
            return None;
        }
        Some(id.to_string())
    } else if !agent_id.is_empty() {
        Some(agent_id.to_string())
    } else {
        None
    }
}

/// Extract channel ID from an interface key like "discord:channel:123456".
fn parse_discord_channel_id(interface_key: &str) -> Option<u64> {
    interface_key
        .strip_prefix("discord:channel:")
        .and_then(|id| id.parse().ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_discord_channel_id_valid() {
        assert_eq!(
            parse_discord_channel_id("discord:channel:123456789"),
            Some(123456789)
        );
    }

    #[test]
    fn parse_discord_channel_id_invalid() {
        assert_eq!(parse_discord_channel_id("slack:channel:123"), None);
        assert_eq!(parse_discord_channel_id("discord:channel:"), None);
        assert_eq!(parse_discord_channel_id("garbage"), None);
    }
}
