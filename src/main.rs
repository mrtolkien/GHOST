use clap::{Parser, Subcommand};

use ghost::error::GhostError;

#[derive(Debug, Parser)]
#[command(name = "ghost")]
#[command(version = concat!(env!("CARGO_PKG_VERSION"), " (", env!("GIT_COMMIT_HASH"), ")"))]
#[command(about = "GHOST personal AI agent")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Debug, Subcommand)]
enum Commands {
    Daemon,
    /// Initialize GHOST — interactive setup wizard
    Init(ghost::cli::init::InitArgs),
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
    /// Convert sources to markdown for inspection before import
    Convert {
        #[command(subcommand)]
        command: ghost::cli::convert::ConvertCommand,
    },
    Db {
        #[command(subcommand)]
        command: ghost::cli::db::DbCommand,
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
    /// Manage browser connections
    Browsers {
        #[command(subcommand)]
        command: ghost::cli::browsers::BrowsersCommand,
    },
    /// Manage skills (list, inspect, per-agent views)
    Skills {
        #[command(subcommand)]
        command: ghost::cli::skills::SkillsCommand,
    },
    /// Manage registered services
    Services {
        #[command(subcommand)]
        command: ghost::cli::services::ServicesCommand,
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
    /// Start all services and the daemon
    Start,
    /// Stop the daemon and all services
    Stop,
    /// Show config, daemon, and service health
    Status,
    /// Gracefully restart the running daemon
    Reboot,
    /// Stop all services and delete workspace (inverse of init)
    Reset(ghost::cli::reset::ResetArgs),
    /// Manage the nix shell environment
    Shell {
        #[command(subcommand)]
        command: ghost::cli::shell::ShellCommand,
    },
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
    if let Err(e) = Box::pin(dispatch(cli.command)).await {
        eprintln!("Error: {e}");
        if let Some(hint) = ghost::error::repair_hint(&e.to_string()) {
            eprintln!("{hint}");
        }
        std::process::exit(1);
    }
}

#[tracing::instrument(skip_all)]
async fn dispatch(command: Commands) -> Result<(), GhostError> {
    match command {
        Commands::Daemon => ghost::cli::daemon::execute().await,
        Commands::Init(args) => ghost::cli::init::execute(args).await,
        Commands::Agent { command } => ghost::cli::agent::execute(command).await,
        Commands::Config { command } => ghost::cli::config::execute(command).await,
        Commands::Hack { command } => ghost::cli::hack::execute(command).await,
        Commands::Auth { command } => ghost::cli::auth::execute(command).await,
        Commands::Session { command } => ghost::cli::session::execute(command).await,
        Commands::Knowledge { command } => ghost::cli::knowledge::execute(command).await,
        Commands::Project { command } => ghost::cli::project::execute(command).await,
        Commands::Convert { command } => ghost::cli::convert::execute(command).await,
        Commands::Db { command } => Box::pin(ghost::cli::db::execute(command)).await,
        Commands::Reference { command } => Box::pin(ghost::cli::reference::execute(command)).await,
        Commands::Topics { command } => ghost::cli::topics::execute(command).await,
        Commands::Web { command } => ghost::cli::web::execute(command).await,
        Commands::Browsers { command } => ghost::cli::browsers::execute(command).await,
        Commands::Skills { command } => ghost::cli::skills::execute(command),
        Commands::Services { command } => ghost::cli::services::execute(command),
        Commands::SendImage { path, caption } => {
            ghost::cli::send::execute_send_image(path, caption).await
        }
        Commands::Attach { path, caption } => ghost::cli::send::execute_attach(path, caption).await,
        Commands::Start => ghost::cli::start_stop::execute_start().await,
        Commands::Stop => ghost::cli::start_stop::execute_stop().await,
        Commands::Status => ghost::cli::status::execute().await,
        Commands::Reboot => ghost::cli::reboot::execute(),
        Commands::Reset(args) => ghost::cli::reset::execute(args),
        Commands::Shell { command } => ghost::cli::shell::execute(command).await,
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
