use clap::Subcommand;

use crate::coding;
use crate::config;
use crate::error::GhostError;

#[derive(Debug, Subcommand)]
pub enum HackCommand {
    /// Start a new coding session
    Start {
        /// Working directory (relative to workspace, or absolute)
        dir: String,
        /// Initial prompt for the coding agent
        #[arg(long)]
        prompt: Option<String>,
        /// Discord channel ID for takeover (passed by GHOST)
        #[arg(long)]
        channel_id: Option<String>,
    },
    /// Resume a previous coding session
    Resume {
        /// Coding session ID
        session_id: String,
        /// Resume prompt
        #[arg(long)]
        prompt: Option<String>,
        /// Discord channel ID for takeover (passed by GHOST)
        #[arg(long)]
        channel_id: Option<String>,
    },
    /// List recent coding sessions
    List,
}

pub async fn execute(command: HackCommand) -> Result<(), GhostError> {
    let config = config::load()?;
    let db = crate::db::connect(&config.workspace, config.embeddings.dimension).await?;

    match command {
        HackCommand::Start {
            dir,
            prompt,
            channel_id,
        } => {
            let working_dir = resolve_working_dir(&config, &dir)?;
            let channel_id = channel_id.or_else(|| std::env::var("GHOST_CHANNEL_ID").ok());

            let session =
                coding::session::start(&db, &config, working_dir, channel_id, prompt).await?;

            println!("coding_session_id={}", session.id);
            println!("session_id={}", session.session_id);
            println!("working_dir={}", session.working_dir.display());
        }
        HackCommand::Resume {
            session_id,
            prompt,
            channel_id,
        } => {
            let channel_id = channel_id.or_else(|| std::env::var("GHOST_CHANNEL_ID").ok());
            let (chat_session_id, working_dir, status) =
                crate::db::coding_sessions::get_coding_session(&db, &session_id)
                    .await?
                    .ok_or_else(|| {
                        coding::session::CodingError::WorkingDirNotFound(format!(
                            "Coding session not found: {session_id}"
                        ))
                    })?;

            if status == "active" {
                return Err(
                    coding::session::CodingError::SessionAlreadyActive(session_id.clone()).into(),
                );
            }

            crate::db::coding_sessions::reactivate_coding_session(
                &db,
                &session_id,
                channel_id.as_deref(),
            )
            .await?;

            if let Some(prompt) = prompt {
                crate::db::sessions::create_message(&db, &chat_session_id, "user", &prompt).await?;
            }

            println!("coding_session_id={session_id}");
            println!("session_id={chat_session_id}");
            println!("working_dir={working_dir}");
        }
        HackCommand::List => {
            let sessions = crate::db::coding_sessions::list_recent_coding_sessions(&db, 10).await?;

            if sessions.is_empty() {
                println!("No coding sessions found.");
                return Ok(());
            }

            for (id, _session_id, working_dir, status, started_at) in &sessions {
                let marker = if status == "active" { "*" } else { " " };
                println!("{marker} {id}  {working_dir}  ({status}, {started_at})");
            }
        }
    }

    Ok(())
}

fn resolve_working_dir(
    config: &config::Config,
    dir: &str,
) -> Result<std::path::PathBuf, GhostError> {
    let path = std::path::Path::new(dir);
    let resolved = if path.is_absolute() {
        path.to_path_buf()
    } else {
        config.workspace.join(dir)
    };

    if !resolved.is_dir() {
        return Err(coding::session::CodingError::WorkingDirNotFound(
            resolved.display().to_string(),
        )
        .into());
    }

    Ok(resolved)
}
