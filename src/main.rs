mod cli;
mod config;
mod daemon;
mod error;

use clap::{Parser, Subcommand};

use crate::error::GhostError;

#[derive(Debug, Parser)]
#[command(name = "ghost")]
#[command(version)]
#[command(about = "GHOST personal AI agent")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Debug, Subcommand)]
enum Commands {
    Daemon,
    Init,
    Config {
        #[command(subcommand)]
        command: cli::config::ConfigCommand,
    },
    Auth {
        #[command(subcommand)]
        command: cli::auth::AuthCommand,
    },
    Job {
        #[command(subcommand)]
        command: cli::job::JobCommand,
    },
    Session {
        #[command(subcommand)]
        command: cli::session::SessionCommand,
    },
    Knowledge {
        #[command(subcommand)]
        command: cli::knowledge::KnowledgeCommand,
    },
    Version,
}

#[tokio::main]
async fn main() -> Result<(), GhostError> {
    init_tracing();

    let cli = Cli::parse();
    dispatch(cli.command).await
}

fn init_tracing() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .try_init();
}

#[tracing::instrument(skip_all)]
async fn dispatch(command: Commands) -> Result<(), GhostError> {
    match command {
        Commands::Daemon => cli::daemon::execute().await,
        Commands::Init => cli::init::execute().await,
        Commands::Config { command } => cli::config::execute(command).await,
        Commands::Auth { command } => cli::auth::execute(command).await,
        Commands::Job { command } => cli::job::execute(command).await,
        Commands::Session { command } => cli::session::execute(command).await,
        Commands::Knowledge { command } => cli::knowledge::execute(command).await,
        Commands::Version => {
            println!("{}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
    }
}
