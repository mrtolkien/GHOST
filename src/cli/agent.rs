use clap::Subcommand;
use serde::Serialize;

use crate::db::agent_runs::AgentRunRecord;
use crate::error::GhostError;

/// Default number of recent runs shown by `ghost agent status`.
const DEFAULT_STATUS_LIMIT: usize = 20;

/// Number of characters shown for run IDs in the table view.
const ID_PREFIX_LEN: usize = 8;

#[derive(Debug, Subcommand)]
pub enum AgentCommand {
    /// List all discovered agents.
    List,
    /// Validate agent Lua configs. Validates all agents if no name given.
    Validate {
        /// Agent name (folder name under agents/). Omit to validate all.
        name: Option<String>,
    },
    /// Show running agents and recent runs.
    Status {
        /// Filter to a specific agent name.
        #[arg(long)]
        agent: Option<String>,
        /// Maximum number of runs to show.
        #[arg(long, default_value_t = DEFAULT_STATUS_LIMIT)]
        limit: usize,
    },
    /// Show details of a specific agent run.
    Show {
        /// Run ID (full ULID or unique prefix, minimum 4 chars).
        run_id: String,
        /// Show full session message history.
        #[arg(long)]
        full: bool,
        /// Output as JSON.
        #[arg(long)]
        json: bool,
    },
}

pub async fn execute(command: AgentCommand) -> Result<(), GhostError> {
    let config = crate::config::load()?;

    match command {
        AgentCommand::List => {
            let agents = crate::agents::discover_agents(&config.workspace);
            if agents.is_empty() {
                println!("No agents found in {}/agents/", config.workspace.display());
                return Ok(());
            }
            for agent in &agents {
                println!("  {} — {}", agent.name, agent.description);
            }
            Ok(())
        }
        AgentCommand::Validate { name } => {
            execute_validate(&config.workspace, name)?;
            Ok(())
        }
        AgentCommand::Status { agent, limit } => {
            crate::config_workspace::bootstrap_workspace(&config)?;
            let db = crate::db::connect(&config.workspace, config.embeddings.dimension).await?;
            execute_status(&db, agent.as_deref(), limit).await
        }
        AgentCommand::Show { run_id, full, json } => {
            crate::config_workspace::bootstrap_workspace(&config)?;
            let db = crate::db::connect(&config.workspace, config.embeddings.dimension).await?;
            execute_show(&db, &run_id, full, json).await
        }
    }
}

fn execute_validate(workspace: &std::path::Path, name: Option<String>) -> Result<(), GhostError> {
    let mut has_errors = false;

    if let Some(name) = name {
        let errors = crate::agents::loader::validate_agent(workspace, &name);
        if errors.is_empty() {
            println!("  {name} ok");
        } else {
            has_errors = true;
            print_validation_errors(&name, &errors);
        }
    } else {
        let agents = crate::agents::discover_agents(workspace);
        if agents.is_empty() {
            println!("No agents found to validate.");
            return Ok(());
        }
        for agent in &agents {
            let errors = crate::agents::loader::validate_agent(workspace, &agent.name);
            if errors.is_empty() {
                println!("  {} ok", agent.name);
            } else {
                has_errors = true;
                print_validation_errors(&agent.name, &errors);
            }
        }
    }

    if has_errors {
        std::process::exit(1);
    }
    Ok(())
}

async fn execute_status(
    db: &sqlx::SqlitePool,
    agent: Option<&str>,
    limit: usize,
) -> Result<(), GhostError> {
    let runs = crate::db::agent_runs::list_runs(db, agent, limit).await?;

    if runs.is_empty() {
        println!("No agent runs found.");
        return Ok(());
    }

    println!(
        "{:<ID_PREFIX_LEN$}  {:<20}  {:<8}  {:<20}  DURATION",
        "ID", "AGENT", "STATUS", "STARTED"
    );

    for run in &runs {
        let id_short = &run.id[..run.id.len().min(ID_PREFIX_LEN)];
        let duration = format_duration(run);
        let started = format_timestamp(&run.started_at);
        println!(
            "{:<ID_PREFIX_LEN$}  {:<20}  {:<8}  {:<20}  {}",
            id_short, run.agent_name, run.status, started, duration,
        );
    }

    Ok(())
}

async fn execute_show(
    db: &sqlx::SqlitePool,
    run_id: &str,
    full: bool,
    json: bool,
) -> Result<(), GhostError> {
    let run = crate::db::agent_runs::get_run_by_prefix(db, run_id)
        .await?
        .ok_or_else(|| GhostError::Other(format!("no agent run found matching '{run_id}'")))?;

    let messages = if full || run.status == "failed" {
        if let Some(ref session_id) = run.agent_session_id {
            Some(crate::db::sessions::list_messages_by_session(db, session_id).await?)
        } else {
            None
        }
    } else {
        None
    };

    if json {
        print_show_json(&run, messages.as_deref(), full)?;
    } else {
        print_show_text(&run, messages.as_deref(), full);
    }

    Ok(())
}

fn print_show_text(
    run: &AgentRunRecord,
    messages: Option<&[crate::db::sessions::MessageRecord]>,
    full: bool,
) {
    println!("Agent:    {}", run.agent_name);
    println!("Status:   {}", run.status);
    println!("Started:  {}", format_timestamp(&run.started_at));
    if let Some(ref finished) = run.finished_at {
        println!("Finished: {}", format_timestamp(finished));
    }
    println!("Duration: {}", format_duration(run));
    println!("Run ID:   {}", run.id);
    println!();

    if run.status == "failed"
        && let Some(msgs) = messages
        && let Some(last) = msgs.iter().rev().find(|m| m.role != "tool")
    {
        println!("Error:\n{}\n", last.content);
    }

    if let Some(ref transcript) = run.transcript
        && !transcript.is_empty()
    {
        println!("{transcript}");
    }

    if full && let Some(msgs) = messages {
        println!("\n--- Session Messages ---\n");
        for msg in msgs {
            println!("[{}] {}", msg.role.to_uppercase(), msg.content);
            print_tool_call_names(msg);
            println!();
        }
    }
}

#[derive(Serialize)]
struct ShowJson {
    id: String,
    agent_name: String,
    status: String,
    started_at: String,
    finished_at: Option<String>,
    transcript: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    messages: Option<Vec<MessageJson>>,
}

#[derive(Serialize)]
struct MessageJson {
    role: String,
    content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_calls: Option<Vec<serde_json::Value>>,
}

fn print_show_json(
    run: &AgentRunRecord,
    messages: Option<&[crate::db::sessions::MessageRecord]>,
    full: bool,
) -> Result<(), GhostError> {
    let messages_json = if full {
        messages.map(|msgs| {
            msgs.iter()
                .map(|m| MessageJson {
                    role: m.role.clone(),
                    content: m.content.clone(),
                    tool_calls: m.tool_calls_parsed(),
                })
                .collect()
        })
    } else {
        None
    };

    let output = ShowJson {
        id: run.id.clone(),
        agent_name: run.agent_name.clone(),
        status: run.status.clone(),
        started_at: run.started_at.clone(),
        finished_at: run.finished_at.clone(),
        transcript: run.transcript.clone(),
        messages: messages_json,
    };

    let json = serde_json::to_string_pretty(&output)
        .map_err(|e| GhostError::Other(format!("failed to serialize JSON: {e}")))?;
    println!("{json}");
    Ok(())
}

fn format_duration(run: &AgentRunRecord) -> String {
    let started = chrono::DateTime::parse_from_rfc3339(&run.started_at).ok();
    let ended = run
        .finished_at
        .as_deref()
        .and_then(|f| chrono::DateTime::parse_from_rfc3339(f).ok());

    let duration = match (started, ended) {
        (Some(s), Some(e)) => e.signed_duration_since(s),
        (Some(s), None) => chrono::Utc::now().signed_duration_since(s),
        _ => return "-".to_string(),
    };

    let secs = duration.num_seconds();
    if secs < 60 {
        format!("{secs}s")
    } else if secs < 3600 {
        format!("{}m {:02}s", secs / 60, secs % 60)
    } else {
        format!("{}h {:02}m", secs / 3600, (secs % 3600) / 60)
    }
}

fn format_timestamp(ts: &str) -> String {
    // Strip the timezone suffix for display, keep date + time.
    ts.replace('T', " ").chars().take(19).collect()
}

fn print_tool_call_names(msg: &crate::db::sessions::MessageRecord) {
    if let Some(tool_calls) = msg.tool_calls_parsed()
        && !tool_calls.is_empty()
    {
        for tc in &tool_calls {
            let name = tc.get("name").and_then(|v| v.as_str()).unwrap_or("unknown");
            println!("  -> tool: {name}");
        }
    }
}

fn print_validation_errors(name: &str, errors: &[String]) {
    eprintln!("  {name} ERRORS:");
    for e in errors {
        eprintln!("    - {e}");
    }
}
