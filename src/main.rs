use clap::{Parser, Subcommand};

use ghost::error::GhostError;

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
    Agent {
        #[command(subcommand)]
        command: ghost::cli::agent::AgentCommand,
    },
    Config {
        #[command(subcommand)]
        command: ghost::cli::config::ConfigCommand,
    },
    Auth {
        #[command(subcommand)]
        command: ghost::cli::auth::AuthCommand,
    },
    Session {
        #[command(subcommand)]
        command: ghost::cli::session::SessionCommand,
    },
    Knowledge {
        #[command(subcommand)]
        command: ghost::cli::knowledge::KnowledgeCommand,
    },
    Web {
        #[command(subcommand)]
        command: ghost::cli::web::WebCommand,
    },
    Version,
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();
    if let Err(e) = dispatch(cli.command).await {
        eprintln!("Error: {e}");
        std::process::exit(1);
    }
}

#[tracing::instrument(skip_all)]
async fn dispatch(command: Commands) -> Result<(), GhostError> {
    match command {
        Commands::Daemon => ghost::cli::daemon::execute().await,
        Commands::Init => ghost::cli::init::execute().await,
        Commands::Agent { command } => ghost::cli::agent::execute(command).await,
        Commands::Config { command } => ghost::cli::config::execute(command).await,
        Commands::Auth { command } => ghost::cli::auth::execute(command).await,
        Commands::Session { command } => ghost::cli::session::execute(command).await,
        Commands::Knowledge { command } => ghost::cli::knowledge::execute(command).await,
        Commands::Web { command } => ghost::cli::web::execute(command).await,
        Commands::Version => {
            println!("{}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
    }
}
