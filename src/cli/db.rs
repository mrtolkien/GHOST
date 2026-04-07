use clap::{Args, Subcommand};

use crate::error::GhostError;

#[derive(Debug, Subcommand)]
pub enum DbCommand {
    Repair(RepairArgs),
}

#[derive(Debug, Args)]
pub struct RepairArgs {
    #[arg(long, default_value_t = false)]
    pub dry_run: bool,
}

pub async fn execute(command: DbCommand) -> Result<(), GhostError> {
    match command {
        DbCommand::Repair(args) => Box::pin(crate::db::repair::execute(args.dry_run)).await,
    }
}
