use crate::cli::init::InitArgs;
use crate::error::GhostError;

use super::{
    OnboardingState, ServiceChoice, config_writer, detect, discord, health, provider,
    service_files, services,
};

/// Run the full onboarding wizard.
///
/// Takes parsed CLI flags and orchestrates all six phases: detection,
/// provider setup, Discord setup, service selection, config writing, and
/// health checks + launch.
pub async fn run(args: InitArgs) -> Result<(), GhostError> {
    // ── Phase 0: Detection ──
    let env = detect::detect().await;

    if !env.nix_installed {
        eprintln!("Nix is required but not installed.");
        eprintln!("Install it from: https://install.determinate.systems/nix");
        return Err(GhostError::Other("Nix is not installed".into()));
    }

    cliclack::intro("GHOST -- First-time setup")?;
    display_detection(&env);

    let existing_toml = read_existing_config(&env)?;

    // ── Phase 1: Provider ──
    let _ = cliclack::log::step("LLM Provider");

    let provider_choice = provider::prompt_provider(args.provider.as_deref())?;
    let api_key = provider::prompt_credentials(&provider_choice, args.api_key.as_deref()).await?;
    let model = provider::prompt_model(&provider_choice, args.model.as_deref())?;
    let context_window = provider::prompt_context_window(args.context_window)?;

    let _ = cliclack::log::info("Validating provider connection...");
    provider::validate_provider(&provider_choice, api_key.as_deref(), &model).await?;
    let _ = cliclack::log::success("Provider verified -- model responded successfully");

    // ── Phase 2: Discord ──
    let _ = cliclack::log::step("Discord");

    let (discord_token, discord_user_id) =
        discord::prompt_discord(args.discord_token.as_deref(), args.discord_user.as_deref())
            .await?;

    // ── Phase 3: Services ──
    let _ = cliclack::log::step("Services");

    let (embeddings, embedding_model) =
        services::prompt_embeddings(&env, args.embeddings.as_deref())?;
    let search = services::prompt_search(&env, args.search.as_deref())?;
    let crawl = services::prompt_crawl(&env, args.crawl.as_deref())?;
    let docling = services::prompt_docling(&env, args.docling.as_deref())?;

    services::install_nix_packages(&embeddings, &docling)?;

    // Build cumulative state
    let state = OnboardingState {
        provider: Some(provider_choice),
        api_key,
        model: Some(model),
        context_window: Some(context_window),
        discord_token: Some(discord_token),
        discord_user_id: Some(discord_user_id),
        embeddings: Some(embeddings),
        embedding_model,
        search: Some(search),
        crawl: Some(crawl),
        docling: Some(docling),
    };

    // ── Phase 4: Write config + install services ──
    let _ = cliclack::log::step("Configuration");

    let config_toml = config_writer::generate_config_toml(&state);
    let env_content = config_writer::generate_env(&state);

    let confirmed = config_writer::display_diff_and_confirm(&existing_toml, &config_toml)?;
    if !confirmed {
        cliclack::outro("Setup cancelled.")?;
        return Ok(());
    }

    let config_dir = crate::config::config_dir()?;
    config_writer::write_config_files(&config_dir, &config_toml, &env_content)?;
    let _ = cliclack::log::success("config.toml and .env written");

    let config = crate::config::load()?;
    crate::config_workspace::bootstrap_workspace(&config)?;
    let _ = cliclack::log::success(format!(
        "Workspace bootstrapped at {}",
        config.workspace.display()
    ));

    let selections = services::build_selections(&state);
    services::write_compose_and_configs(&config.workspace, &selections, env.platform.is_linux())?;
    let _ = cliclack::log::success("docker-compose.yml written");

    install_service_files(&env, &state, &config)?;

    // ── Phase 5: Health + Launch ──
    let _ = cliclack::log::step("Health Checks");

    let results = health::check_all_services(&state);
    health::display_health_table(&results);

    let should_start = health::prompt_start_daemon(args.start)?;
    if should_start {
        health::start_all_services(
            &env.platform,
            env.container_runtime.as_ref(),
            &config.workspace,
        )?;
        let _ = cliclack::log::success("Services started");
        health::trigger_first_message()?;
    }

    // ── Outro ──
    cliclack::outro("Setup complete! Your GHOST is running.")?;
    print_next_steps();

    Ok(())
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Read existing config.toml (if any), offering update/fresh/cancel when
/// non-empty.
fn read_existing_config(env: &detect::DetectedEnvironment) -> Result<String, GhostError> {
    let Some(ref path) = env.existing_config else {
        return Ok(String::new());
    };

    let content = std::fs::read_to_string(path).unwrap_or_default();
    if content.is_empty() {
        return Ok(String::new());
    }

    let action: &str = cliclack::select("Existing config.toml found")
        .item(
            "update",
            "Update existing config",
            "pre-fill with current values",
        )
        .item("fresh", "Fresh install", "start from scratch")
        .item("cancel", "Cancel", "exit without changes")
        .interact()?;

    match action {
        "cancel" => {
            cliclack::outro("Setup cancelled.")?;
            std::process::exit(0);
        }
        "fresh" => Ok(String::new()),
        _ => Ok(content),
    }
}

/// Display environment detection results.
fn display_detection(env: &detect::DetectedEnvironment) {
    let _ = cliclack::log::success("Nix installed");
    let _ = cliclack::log::success(format!("Platform: {:?}", env.platform));

    match &env.container_runtime {
        Some(detect::ContainerRuntime::Podman) => {
            let _ = cliclack::log::success("Container runtime: Podman");
        }
        Some(detect::ContainerRuntime::Docker) => {
            let _ = cliclack::log::success("Container runtime: Docker");
        }
        None => {
            let _ = cliclack::log::warning("No container runtime found (podman or docker)");
        }
    }

    if env.llama_server_in_path {
        let _ = cliclack::log::success("llama-server found in PATH");
    } else {
        let _ = cliclack::log::info("llama-server not found in PATH");
    }

    if env.docling_serve_in_path {
        let _ = cliclack::log::success("docling-serve found in PATH");
    } else {
        let _ = cliclack::log::info("docling-serve not found in PATH");
    }

    if env.existing_config.is_some() {
        let _ = cliclack::log::info("Existing config.toml detected");
    }
}

/// Install systemd/launchd service files for the daemon and native
/// services.
fn install_service_files(
    env: &detect::DetectedEnvironment,
    state: &OnboardingState,
    config: &crate::config::Config,
) -> Result<(), GhostError> {
    let exe = service_files::stable_exe_path()?;
    let workspace = config.workspace.display().to_string();

    let home_str = dirs::home_dir()
        .map(|h| h.display().to_string())
        .unwrap_or_default();

    let llama_exe = if matches!(state.embeddings, Some(ServiceChoice::NixNative)) {
        Some(format!("{home_str}/.nix-profile/bin/llama-server"))
    } else {
        None
    };

    let docling_exe = if matches!(state.docling, Some(ServiceChoice::NixNative)) {
        Some(format!("{home_str}/.nix-profile/bin/docling-serve"))
    } else {
        None
    };

    let installed = service_files::install_all_service_files(
        &env.platform,
        &exe,
        &workspace,
        llama_exe.as_deref(),
        docling_exe.as_deref(),
    )?;

    for path in &installed {
        let _ = cliclack::log::success(format!("Installed {path}"));
    }

    Ok(())
}

/// Print post-setup guidance.
fn print_next_steps() {
    let _ = cliclack::log::info("Open Discord -- your GHOST just sent you a message");
    let _ = cliclack::log::info("  Manage services: read the 'services' skill");
    let _ = cliclack::log::info("  Add observability: services skill -> observability extra");
    let _ = cliclack::log::info("  Add Tailscale: services skill -> tailscale extra");
    let _ = cliclack::log::info("  Reconfigure: ghost init");
}
