use clap::Subcommand;

use crate::error::GhostError;
use crate::services::{ServiceEntry, ServiceField, ServiceRegistry};

#[derive(Debug, Subcommand)]
pub enum ServicesCommand {
    /// List all registered services
    List,
    /// Add a service entry
    Add {
        #[arg(long)]
        name: String,
        #[arg(long)]
        start: Option<String>,
        #[arg(long)]
        stop: Option<String>,
        #[arg(long)]
        update: Option<String>,
        #[arg(long)]
        status: Option<String>,
    },
    /// Remove a service entry
    Remove { name: String },
    /// Run update commands for all services
    Update,
    /// Check status of all services
    Status,
}

pub async fn execute(command: ServicesCommand) -> Result<(), GhostError> {
    match command {
        ServicesCommand::List => execute_list(),
        ServicesCommand::Add {
            name,
            start,
            stop,
            update,
            status,
        } => execute_add(&name, start, stop, update, status),
        ServicesCommand::Remove { name } => execute_remove(&name),
        ServicesCommand::Update => execute_update(),
        ServicesCommand::Status => execute_status(),
    }
}

pub(crate) fn services_toml_path() -> Result<std::path::PathBuf, GhostError> {
    let config = crate::config::load()?;
    Ok(config.workspace.join("services/services.toml"))
}

fn execute_list() -> Result<(), GhostError> {
    let path = services_toml_path()?;
    let registry = ServiceRegistry::load(&path)?;

    if registry.entries.is_empty() {
        println!("No services registered.");
        println!("Run `ghost services add --name <name> --start <cmd>` to add one.");
        return Ok(());
    }

    println!(
        "{:<25} {:<8} {:<8} {:<8} {:<8}",
        "NAME", "START", "STOP", "UPDATE", "STATUS"
    );
    println!("{}", "-".repeat(61));
    for (name, entry) in &registry.entries {
        let present = |opt: &Option<String>| if opt.is_some() { "✓" } else { "-" };
        println!(
            "{:<25} {:<8} {:<8} {:<8} {:<8}",
            name,
            present(&entry.start),
            present(&entry.stop),
            present(&entry.update),
            present(&entry.status),
        );
    }

    Ok(())
}

fn execute_add(
    name: &str,
    start: Option<String>,
    stop: Option<String>,
    update: Option<String>,
    status: Option<String>,
) -> Result<(), GhostError> {
    let path = services_toml_path()?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut registry = ServiceRegistry::load_or_empty(&path)?;
    let entry = ServiceEntry {
        start,
        stop,
        update,
        status,
    };
    registry.add(name.to_string(), entry)?;
    registry.save(&path)?;
    println!("Service '{name}' added.");
    Ok(())
}

fn execute_remove(name: &str) -> Result<(), GhostError> {
    let path = services_toml_path()?;
    let mut registry = ServiceRegistry::load(&path)?;
    registry.remove(name)?;
    registry.save(&path)?;
    println!("Service '{name}' removed.");
    Ok(())
}

fn execute_update() -> Result<(), GhostError> {
    let path = services_toml_path()?;
    let registry = ServiceRegistry::load(&path)?;
    let results = registry.run_field(ServiceField::Update, true, false);

    if results.is_empty() {
        println!("No services have an update command.");
        return Ok(());
    }

    for result in &results {
        let mark = if result.success { "✓" } else { "✗" };
        println!("{mark} {}", result.service);
        if !result.output.is_empty() {
            for line in result.output.lines() {
                println!("  {line}");
            }
        }
    }

    if results.iter().any(|r| !r.success) {
        return Err(GhostError::Other(
            "one or more services failed to update".into(),
        ));
    }

    Ok(())
}

fn execute_status() -> Result<(), GhostError> {
    let path = services_toml_path()?;
    let registry = ServiceRegistry::load(&path)?;
    let results = registry.run_field(ServiceField::Status, false, false);

    if results.is_empty() {
        println!("No services have a status command.");
        return Ok(());
    }

    for result in &results {
        let mark = if result.success { "✓" } else { "✗" };
        println!("{mark} {}", result.service);
        if !result.output.is_empty() {
            for line in result.output.lines() {
                println!("  {line}");
            }
        }
    }

    Ok(())
}
