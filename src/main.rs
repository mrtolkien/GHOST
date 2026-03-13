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
    Hack {
        #[command(subcommand)]
        command: ghost::cli::hack::HackCommand,
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
    Project {
        #[command(subcommand)]
        command: ghost::cli::project::ProjectCommand,
    },
    Document {
        #[command(subcommand)]
        command: ghost::cli::document::DocumentCommand,
    },
    Reference {
        #[command(subcommand)]
        command: ghost::cli::reference::ReferenceCommand,
    },
    Topics {
        #[command(subcommand)]
        command: ghost::cli::topics::TopicsCommand,
    },
    Web {
        #[command(subcommand)]
        command: ghost::cli::web::WebCommand,
    },
    /// Send an image to the OPERATOR
    SendImage {
        /// Path to the image file
        path: std::path::PathBuf,
        /// Optional caption
        #[arg(long)]
        caption: Option<String>,
    },
    /// Send a file attachment to the OPERATOR
    Attach {
        /// Path to the file
        path: std::path::PathBuf,
        /// Optional caption
        #[arg(long)]
        caption: Option<String>,
    },
    /// Gracefully restart the running daemon
    Reboot,
    /// Update ghost binary via nix profile
    Update {
        /// Build from main branch instead of latest release
        #[arg(long, conflicts_with = "version")]
        from_source: bool,
        /// Install a specific version tag (e.g. v0.3.0)
        #[arg(long)]
        version: Option<String>,
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
        Commands::Hack { command } => ghost::cli::hack::execute(command).await,
        Commands::Auth { command } => ghost::cli::auth::execute(command).await,
        Commands::Session { command } => ghost::cli::session::execute(command).await,
        Commands::Knowledge { command } => ghost::cli::knowledge::execute(command).await,
        Commands::Project { command } => ghost::cli::project::execute(command).await,
        Commands::Document { command } => ghost::cli::document::execute(command).await,
        Commands::Reference { command } => ghost::cli::reference::execute(command).await,
        Commands::Topics { command } => ghost::cli::topics::execute(command).await,
        Commands::Web { command } => ghost::cli::web::execute(command).await,
        Commands::SendImage { path, caption } => {
            ghost::cli::send::execute_send_image(path, caption).await
        }
        Commands::Attach { path, caption } => ghost::cli::send::execute_attach(path, caption).await,
        Commands::Reboot => ghost::cli::reboot::execute(),
        Commands::Update {
            from_source,
            version,
        } => ghost::cli::update::execute(from_source, version).await,
        Commands::Version => {
            println!(
                "ghost {} ({})",
                env!("CARGO_PKG_VERSION"),
                env!("GIT_COMMIT_HASH")
            );
            Ok(())
        }
    }
}
