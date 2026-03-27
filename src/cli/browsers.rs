use clap::Subcommand;
use tracing::debug;

use crate::error::GhostError;

/// Timeout for individual CDP readiness probes.
const CDP_PROBE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(2);

/// Delay between CDP readiness polls.
const CDP_POLL_INTERVAL: std::time::Duration = std::time::Duration::from_millis(500);

/// Maximum number of CDP readiness polls.
const CDP_POLL_MAX: usize = 15;

/// Timeout for browser reachability check.
const BROWSER_CHECK_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

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
    /// Start a browser with CDP and relay it over Tailscale
    Serve {
        /// CDP port (default: 9222)
        #[arg(long, default_value = "9222")]
        port: u16,
        /// Tailscale IP to bind on (auto-detected if omitted)
        #[arg(long)]
        bind: Option<String>,
        /// Browser command (auto-detected if omitted)
        #[arg(long)]
        browser: Option<String>,
        /// Profile directory (default: <config-dir>/browser-profile)
        #[arg(long)]
        profile: Option<String>,
    },
}

pub async fn execute(command: BrowsersCommand) -> Result<(), GhostError> {
    match command {
        BrowsersCommand::List => execute_list(),
        BrowsersCommand::Add { name, cdp_url } => execute_add(&name, &cdp_url),
        BrowsersCommand::Remove { name } => execute_remove(&name),
        BrowsersCommand::Discover => execute_discover().await,
        BrowsersCommand::Check { name } => execute_check(&name).await,
        BrowsersCommand::Serve {
            port,
            bind,
            browser,
            profile,
        } => execute_serve(port, bind, browser, profile).await,
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
    println!("Run `ghost config reload` to apply changes.");
    Ok(())
}

fn execute_remove(name: &str) -> Result<(), GhostError> {
    let removed = crate::config_cli::remove_browser(name)?;
    if removed {
        println!("Browser '{name}' removed.");
        println!("Run `ghost config reload` to apply changes.");
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

async fn execute_serve(
    port: u16,
    bind: Option<String>,
    browser_cmd: Option<String>,
    profile: Option<String>,
) -> Result<(), GhostError> {
    use std::net::SocketAddr;

    use tokio::io::AsyncWriteExt;
    use tokio::net::TcpListener;

    // 1. Resolve Tailscale IP.
    let tailscale_ip = match bind {
        Some(ip) => ip,
        None => crate::web::browser::discovery::tailscale_self_ip()
            .await
            .map_err(|e| {
                GhostError::Other(format!(
                    "Tailscale is required for `ghost browsers serve`. \
                     Use --bind <ip> to override.\n{e}"
                ))
            })?,
    };

    // 2. Resolve profile path.
    let profile_path = match profile {
        Some(p) => p,
        None => {
            let config = crate::config::config_dir()
                .map_err(|e| GhostError::Other(format!("cannot resolve config dir: {e}")))?;
            config
                .join("browser-profile")
                .to_string_lossy()
                .into_owned()
        }
    };
    std::fs::create_dir_all(&profile_path)
        .map_err(|e| GhostError::Other(format!("failed to create profile dir: {e}")))?;

    // 3. Find browser binary.
    let browser_bin = match browser_cmd {
        Some(cmd) => cmd,
        None => detect_browser()?,
    };

    // 4. Pick a free internal port for Chromium.
    //
    // We can't use the external port (default 9222) because
    // something else may already be on 127.0.0.1:9222 (e.g. a
    // headless-shell Docker container). Chromium would silently
    // fall back to [::1] and the relay would hit the wrong Chrome.
    let internal_port = {
        let sock = std::net::TcpListener::bind("127.0.0.1:0")
            .map_err(|e| GhostError::Other(format!("no free port: {e}")))?;
        sock.local_addr()
            .map_err(|e| GhostError::Other(e.to_string()))?
            .port()
    };

    // 5. Start browser on the internal port.
    eprintln!("Starting {browser_bin}...");
    let mut child = tokio::process::Command::new(&browser_bin)
        .args([
            &format!("--remote-debugging-port={internal_port}"),
            &format!("--user-data-dir={profile_path}"),
        ])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| GhostError::Other(format!("failed to start {browser_bin}: {e}")))?;

    // 6. Wait for CDP to be ready.
    let local_url = format!("http://127.0.0.1:{internal_port}/json/version");
    let client = reqwest::Client::builder()
        .timeout(CDP_PROBE_TIMEOUT)
        .build()
        .map_err(|e| GhostError::Other(e.to_string()))?;

    eprintln!("Waiting for CDP...");
    let mut ready = false;
    for _ in 0..CDP_POLL_MAX {
        if client.get(&local_url).send().await.is_ok() {
            ready = true;
            break;
        }
        tokio::time::sleep(CDP_POLL_INTERVAL).await;
    }
    if !ready {
        let _ = child.kill().await;
        return Err(GhostError::Other(
            "Browser did not start CDP within 7.5s".into(),
        ));
    }

    // 7. Start TCP relay: tailscale_ip:port → 127.0.0.1:internal_port
    let relay_addr: SocketAddr = format!("{tailscale_ip}:{port}")
        .parse()
        .map_err(|e| GhostError::Other(format!("invalid bind address: {e}")))?;
    let relay_target: SocketAddr = format!("127.0.0.1:{internal_port}")
        .parse()
        .map_err(|e| GhostError::Other(format!("invalid target: {e}")))?;

    let listener = TcpListener::bind(relay_addr)
        .await
        .map_err(|e| GhostError::Other(format!("failed to bind {relay_addr}: {e}")))?;

    eprintln!();
    eprintln!("Browser ready!");
    eprintln!("  Local:  ws://127.0.0.1:{port}");
    eprintln!("  Remote: ws://{tailscale_ip}:{port}");
    eprintln!();
    eprintln!("From the GHOST machine, run:");
    eprintln!("  ghost browsers add operator ws://{tailscale_ip}:{port}");
    eprintln!("  ghost config reload");
    eprintln!();
    eprintln!("Press Ctrl+C to stop.");

    // Relay loop + Ctrl+C handling.
    let relay_handle = tokio::spawn(async move {
        loop {
            let (inbound, peer) = match listener.accept().await {
                Ok(conn) => conn,
                Err(e) => {
                    debug!("relay accept error: {e}");
                    continue;
                }
            };
            debug!(%peer, "CDP relay connection");
            let target = relay_target;
            tokio::spawn(async move {
                let outbound = match tokio::net::TcpStream::connect(target).await {
                    Ok(s) => s,
                    Err(e) => {
                        debug!("relay connect to Chrome failed: {e}");
                        return;
                    }
                };
                let (mut ri, mut wi) = inbound.into_split();
                let (mut ro, mut wo) = outbound.into_split();
                tokio::select! {
                    r = tokio::io::copy(&mut ri, &mut wo) => {
                        let _ = wo.shutdown().await;
                        if let Err(e) = r { debug!("relay inbound→outbound: {e}"); }
                    }
                    r = tokio::io::copy(&mut ro, &mut wi) => {
                        let _ = wi.shutdown().await;
                        if let Err(e) = r { debug!("relay outbound→inbound: {e}"); }
                    }
                }
            });
        }
    });

    // Wait for Ctrl+C.
    tokio::signal::ctrl_c()
        .await
        .map_err(|e| GhostError::Other(format!("signal handler failed: {e}")))?;
    eprintln!("\nShutting down...");
    relay_handle.abort();
    let _ = child.kill().await;
    eprintln!("Done.");

    Ok(())
}

/// Find a Chromium-based browser in PATH.
fn detect_browser() -> Result<String, GhostError> {
    let candidates = [
        "chromium",
        "chromium-browser",
        "google-chrome",
        "google-chrome-stable",
        "chrome",
    ];
    for name in candidates {
        let ok = std::process::Command::new("which")
            .arg(name)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .is_ok_and(|s| s.success());
        if ok {
            return Ok(name.to_string());
        }
    }
    Err(GhostError::Other(format!(
        "no browser found in PATH. Tried: {}. \
         Use --browser <path> to specify one.",
        candidates.join(", ")
    )))
}

/// Attempt to connect to a browser CDP endpoint with a 5-second timeout.
///
/// Drops the connection immediately after a successful connect — this is
/// purely a reachability check.
async fn check_browser(cdp_url: &str) -> Result<(), crate::web::browser::error::BrowserError> {
    use tokio::time::timeout;

    let (browser, handle) = timeout(
        BROWSER_CHECK_TIMEOUT,
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
