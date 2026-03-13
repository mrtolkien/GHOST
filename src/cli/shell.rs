use clap::Subcommand;

use crate::error::GhostError;

#[derive(Debug, Subcommand)]
pub enum ShellCommand {
    /// Rebuild the nix shell environment (picks up flake.nix changes)
    Rebuild,
}

#[tracing::instrument(skip_all)]
pub async fn execute(command: ShellCommand) -> Result<(), GhostError> {
    match command {
        ShellCommand::Rebuild => {
            let config = crate::config::load()?;
            crate::tools::shell::rebuild_shell_env(&config.workspace)
                .await
                .map_err(std::io::Error::other)?;
            println!("shell environment rebuilt — new tools available immediately");
            Ok(())
        }
    }
}
