use clap::Subcommand;

use crate::error::GhostError;

#[derive(Debug, Subcommand)]
pub enum AuthCommand {
    Codex,
    Status,
    Revoke,
}

#[tracing::instrument(skip_all)]
pub async fn execute(command: AuthCommand) -> Result<(), GhostError> {
    match command {
        AuthCommand::Codex => {
            let path = crate::auth::openai_oauth::run_codex_auth_flow().await?;
            println!(
                "Authenticated successfully. Token stored at {}",
                path.display()
            );
            Ok(())
        }
        AuthCommand::Status => {
            let status = crate::auth::openai_oauth::auth_status().await?;
            match status {
                Some(tokens) => {
                    println!("OpenAI OAuth: authenticated");
                    println!("expires_at: {}", tokens.expires_at.to_rfc3339());
                }
                None => {
                    println!("OpenAI OAuth: unauthenticated");
                }
            }
            Ok(())
        }
        AuthCommand::Revoke => {
            crate::auth::openai_oauth::revoke_openai_tokens().await?;
            println!("OpenAI OAuth token revoked.");
            Ok(())
        }
    }
}
