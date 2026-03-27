use clap::Subcommand;

use crate::error::GhostError;

#[derive(Debug, Subcommand)]
pub enum AgentCommand {
    /// List all discovered agents.
    List,
    /// Validate agent Lua configs. Validates all agents if no name given.
    Validate {
        /// Agent name (folder name under agents/). Omit to validate all.
        name: Option<String>,
    },
}

pub fn execute(command: AgentCommand) -> Result<(), GhostError> {
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
        }
        AgentCommand::Validate { name } => {
            let mut has_errors = false;

            if let Some(name) = name {
                let errors = crate::agents::loader::validate_agent(&config.workspace, &name);
                if errors.is_empty() {
                    println!("  {name} ok");
                } else {
                    has_errors = true;
                    eprintln!("  {name} ERRORS:");
                    for e in &errors {
                        eprintln!("    - {e}");
                    }
                }
            } else {
                let agents = crate::agents::discover_agents(&config.workspace);
                if agents.is_empty() {
                    println!("No agents found to validate.");
                    return Ok(());
                }
                for agent in &agents {
                    let errors =
                        crate::agents::loader::validate_agent(&config.workspace, &agent.name);
                    if errors.is_empty() {
                        println!("  {} ok", agent.name);
                    } else {
                        has_errors = true;
                        eprintln!("  {} ERRORS:", agent.name);
                        for e in &errors {
                            eprintln!("    - {e}");
                        }
                    }
                }
            }

            if has_errors {
                std::process::exit(1);
            }
        }
    }

    Ok(())
}
