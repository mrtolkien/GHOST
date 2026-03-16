use clap::Subcommand;
use tracing::debug;

use crate::error::GhostError;

#[derive(Debug, Subcommand)]
pub enum BrowsersCommand {
    /// List known browsers from config
    List,
    /// Add a browser to config.toml
    Add {
        /// Name for the browser (e.g. "headless", "operator")
        name: String,
        /// CDP WebSocket URL (e.g. ws://localhost:9222)
        cdp_url: String,
    },
    /// Remove a browser from config.toml
    Remove {
        /// Name of the browser to remove
        name: String,
    },
    /// Scan for CDP endpoints on localhost and Tailscale peers
    Discover,
    /// Test connectivity to a browser
    Check {
        /// Name of the browser to check, or "all"
        name: String,
    },
}

pub async fn execute(command: BrowsersCommand) -> Result<(), GhostError> {
    match command {
        BrowsersCommand::List => execute_list(),
        BrowsersCommand::Add { name, cdp_url } => execute_add(&name, &cdp_url),
        BrowsersCommand::Remove { name } => execute_remove(&name),
        BrowsersCommand::Discover => execute_discover().await,
        BrowsersCommand::Check { name } => execute_check(&name).await,
    }
}

fn execute_list() -> Result<(), GhostError> {
    let config = crate::config::load()?;
    let browsers = &config.web.browsers;

    if browsers.is_empty() {
        println!("No browsers configured.");
        println!("Run `ghost browsers discover` to find running instances.");
        return Ok(());
    }

    println!("{:<20} {:<45} DISCOVERED", "NAME", "CDP URL");
    println!("{}", "-".repeat(72));
    for b in browsers {
        println!("{:<20} {:<45} {}", b.name, b.cdp_url, b.discovered);
    }

    Ok(())
}

fn execute_add(name: &str, cdp_url: &str) -> Result<(), GhostError> {
    crate::config_cli::add_browser(name, cdp_url, false)?;
    println!("Browser '{name}' added with URL: {cdp_url}");
    println!("Run `ghost reboot` to apply changes.");
    Ok(())
}

fn execute_remove(name: &str) -> Result<(), GhostError> {
    let removed = crate::config_cli::remove_browser(name)?;
    if removed {
        println!("Browser '{name}' removed.");
        println!("Run `ghost reboot` to apply changes.");
    } else {
        println!("Browser '{name}' not found in config.");
    }
    Ok(())
}

async fn execute_discover() -> Result<(), GhostError> {
    println!("Scanning for CDP endpoints...");
    let found = crate::web::browser::discovery::discover().await;

    if found.is_empty() {
        println!("No CDP endpoints found.");
        println!("Start Chrome/Chromium with --remote-debugging-port=9222 to expose one.");
        return Ok(());
    }

    println!("Found {} endpoint(s):\n", found.len());
    for browser in &found {
        let version = browser
            .browser_version
            .as_deref()
            .unwrap_or("unknown version");
        println!("  {}:{} — {version}", browser.host, browser.port);
        println!("  CDP URL: {}", browser.cdp_url);
        println!("  Add: ghost browsers add <name> \"{}\"", browser.cdp_url);
        println!();
    }

    Ok(())
}

async fn execute_check(name: &str) -> Result<(), GhostError> {
    let config = crate::config::load()?;

    let targets: Vec<_> = if name == "all" {
        config.web.browsers.iter().collect()
    } else {
        match config.web.browsers.iter().find(|b| b.name == name) {
            Some(b) => vec![b],
            None => {
                println!("Browser '{name}' not found in config.");
                return Ok(());
            }
        }
    };

    if targets.is_empty() {
        println!("No browsers configured.");
        return Ok(());
    }

    for browser in targets {
        print!("Checking '{}' ({})... ", browser.name, browser.cdp_url);
        match check_browser(&browser.cdp_url).await {
            Ok(()) => println!("OK"),
            Err(e) => {
                debug!(
                    name = browser.name,
                    url = browser.cdp_url,
                    error = %e,
                    "browser check failed"
                );
                println!("FAILED — {e}");
            }
        }
    }

    Ok(())
}

/// Attempt to connect to a browser CDP endpoint with a 5-second timeout.
///
/// Drops the connection immediately after a successful connect — this is
/// purely a reachability check.
async fn check_browser(cdp_url: &str) -> Result<(), crate::web::browser::error::BrowserError> {
    use tokio::time::{Duration, timeout};

    let (browser, handle) = timeout(
        Duration::from_secs(5),
        crate::web::browser::cdp::connect(cdp_url),
    )
    .await
    .map_err(
        |_| crate::web::browser::error::BrowserError::ConnectionFailed {
            url: cdp_url.to_string(),
            source: Box::new(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                "connection timed out after 5s",
            )),
        },
    )??;

    drop(browser);
    handle.abort();

    Ok(())
}
