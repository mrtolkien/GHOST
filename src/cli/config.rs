use clap::Subcommand;

use crate::error::GhostError;

#[derive(Debug, Subcommand)]
pub enum ConfigCommand {
    Get {
        key: String,
    },
    Set {
        key: String,
        value: String,
    },
    /// Reload config without restarting the daemon
    Reload,
}

#[tracing::instrument(skip_all)]
pub async fn execute(command: ConfigCommand) -> Result<(), GhostError> {
    match command {
        ConfigCommand::Get { key } => {
            let value = crate::config_cli::get_resolved_value(&key)?;
            println!("{value}");
            Ok(())
        }
        ConfigCommand::Set { key, value } => {
            crate::config_cli::set_value(&key, &value)?;
            Ok(())
        }
        ConfigCommand::Reload => crate::cli::reload::execute(),
    }
}
