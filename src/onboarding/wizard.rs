use std::time::Duration;

use crate::cli::init::InitArgs;
use crate::error::GhostError;

use super::{
    ExistingValues, OnboardingState, ServiceChoice, config_writer, container_setup, detect,
    discord, health, provider, service_files, services,
};

/// Run the full onboarding wizard.
///
/// Takes parsed CLI flags and orchestrates all six phases: detection,
/// provider setup, Discord setup, service selection, config writing, and
/// health checks + launch.
pub async fn run(args: InitArgs) -> Result<(), GhostError> {
    // ── Phase 0: Detection ──
    let mut env = detect::detect().await;

    if !env.nix_installed {
        eprintln!("Nix is required but not installed.");
        eprintln!("Install it from: https://install.determinate.systems/nix");
        return Err(GhostError::Other("Nix is not installed".into()));
    }

    cliclack::intro("GHOST -- First-time setup")?;
    display_detection(&env);

    // Offer to install podman if no container runtime found.
    container_setup::prompt_container_setup(&mut env)?;

    let (existing_toml, existing) = read_existing_config(&env)?;
    let ex = existing.as_ref();

    // ── Phase 1: Provider ──
    let _ = cliclack::log::step("LLM Provider");

    let provider_choice =
        provider::prompt_provider(args.provider.as_deref(), ex.and_then(|e| e.provider))?;
    let api_key = provider::prompt_credentials(
        &provider_choice,
        args.api_key.as_deref(),
        ex.and_then(|e| e.api_key.as_deref()),
    )
    .await?;
    let model = provider::prompt_model(
        &provider_choice,
        args.model.as_deref(),
        ex.and_then(|e| e.model.as_deref()),
    )?;
    let context_window =
        provider::prompt_context_window(args.context_window, ex.and_then(|e| e.context_window))?;

    // Ask before making a real API call. Retry on failure.
    let should_test = cliclack::confirm("Test the provider connection? (makes a real API call)")
        .initial_value(true)
        .interact()?;
    if should_test {
        loop {
            let _ = cliclack::log::info("Validating provider connection...");
            match provider::validate_provider(&provider_choice, &model).await {
                Ok(()) => {
                    let _ =
                        cliclack::log::success("Provider verified -- model responded successfully");
                    break;
                }
                Err(e) => {
                    let _ = cliclack::log::warning(format!("Provider validation failed: {e}"));
                    let retry = cliclack::confirm("Try again?")
                        .initial_value(true)
                        .interact()?;
                    if !retry {
                        break;
                    }
                }
            }
        }
    }

    // ── Phase 2: Discord ──
    let _ = cliclack::log::step("Discord");

    let (discord_token, discord_user_id) = discord::prompt_discord(
        args.discord_token.as_deref(),
        args.discord_user.as_deref(),
        ex.and_then(|e| e.discord_token.as_deref()),
        ex.and_then(|e| e.discord_user_id.as_deref()),
    )
    .await?;

    // ── Phase 3: Services ──
    let _ = cliclack::log::step("Services");

    // Embeddings — with inline nix add + remote probe, retry on failure.
    let (embeddings, embedding_model, embedding_hf_repo) = loop {
        let sel = services::prompt_embeddings(&env, args.embeddings.as_deref())?;

        if matches!(sel.choice, ServiceChoice::NixNative) {
            match services::nix_add("llama-cpp", "Adding llama-server via nix...") {
                Ok(()) => {}
                Err(e) => {
                    let _ = cliclack::log::warning(format!("{e}"));
                    if args.embeddings.is_some() {
                        return Err(GhostError::Onboarding(e));
                    }
                    continue;
                }
            }
        }

        // Test remote embeddings endpoint if configured.
        if let ServiceChoice::Remote(ref url) = sel.choice
            && !url.is_empty()
        {
            let should_test =
                cliclack::confirm("Test the embeddings endpoint? (sends a real embedding request)")
                    .initial_value(true)
                    .interact()?;
            if should_test {
                if let Err(msg) = test_embeddings(url, sel.model.as_deref()).await {
                    let _ = cliclack::log::warning(msg);
                    if args.embeddings.is_some() {
                        break (sel.choice, sel.model, sel.hf_repo);
                    }
                    continue;
                }
                let _ = cliclack::log::success(format!("Embeddings verified: {url}"));
            }
        }

        break (sel.choice, sel.model, sel.hf_repo);
    };

    let search = services::prompt_search(&env, args.search.as_deref())?;
    let crawl = services::prompt_crawl(&env, args.crawl.as_deref())?;

    // Docling — with inline nix add, retry on failure.
    let docling = loop {
        let doc = services::prompt_docling(&env, args.docling.as_deref())?;

        if matches!(doc, ServiceChoice::NixNative) {
            match services::nix_add("docling-serve", "Adding docling-serve via nix...") {
                Ok(()) => {}
                Err(e) => {
                    let _ = cliclack::log::warning(format!("{e}"));
                    if args.docling.is_some() {
                        return Err(GhostError::Onboarding(e));
                    }
                    continue;
                }
            }
        }

        break doc;
    };

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
        embedding_hf_repo,
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

    // ── Phase 5: Launch + Health ──
    let should_start = health::prompt_start_daemon(args.start)?;
    if should_start {
        health::start_all_services(
            &env.platform,
            env.container_runtime.as_ref(),
            &config.workspace,
        )?;
        let _ = cliclack::log::success("Services started");

        // Give containers a moment to boot before probing health endpoints.
        let warmup = cliclack::spinner();
        warmup.start("Waiting for services to come up…");
        tokio::time::sleep(Duration::from_secs(5)).await;
        warmup.stop("Ready");

        let _ = cliclack::log::step("Health Checks");
        let results = health::check_all_services(&state).await;
        health::display_health_table(&results);

        health::trigger_first_message().await?;
    }

    // ── Outro ──
    cliclack::outro("Setup complete! Your GHOST is running.")?;
    print_next_steps();

    Ok(())
}

/// Send a test embedding request to verify the remote endpoint works.
async fn test_embeddings(url: &str, model: Option<&str>) -> Result<(), String> {
    let embed_url = format!("{}/v1/embeddings", url.trim_end_matches('/'));
    let model = model.unwrap_or("default");

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(15))
        .build()
        .map_err(|e| format!("failed to build HTTP client: {e}"))?;

    let body = serde_json::json!({
        "model": model,
        "input": ["hello world"],
    });

    let resp = client
        .post(&embed_url)
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("could not reach {embed_url}: {e}"))?;

    if resp.status().is_success() {
        Ok(())
    } else {
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        Err(format!("embeddings test failed: HTTP {status}: {text}"))
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Read existing config.toml (if any), offering update/fresh/cancel when
/// non-empty. Returns the raw TOML (for diff) and parsed existing values
/// (for pre-filling prompts).
fn read_existing_config(
    env: &detect::DetectedEnvironment,
) -> Result<(String, Option<ExistingValues>), GhostError> {
    let Some(ref path) = env.existing_config else {
        return Ok((String::new(), None));
    };

    let content = std::fs::read_to_string(path).unwrap_or_default();
    if content.is_empty() {
        return Ok((String::new(), None));
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
        "fresh" => Ok((String::new(), None)),
        _ => {
            let existing = parse_existing_values(path);
            Ok((content, Some(existing)))
        }
    }
}

/// Parse existing config.toml + .env into pre-fill values for the wizard.
fn parse_existing_values(config_path: &std::path::Path) -> ExistingValues {
    let mut vals = ExistingValues::default();

    // Parse config.toml via the normal config loader.
    if let Ok(config) = crate::config::load() {
        let primary = config.models.aliases.get(&config.models.default);
        if let Some(m) = primary {
            vals.provider = Some(m.provider);
            vals.model = Some(m.model.clone());
            vals.context_window = Some(m.context_window);
        }
        vals.discord_user_id = config.discord.allowed_user_ids.into_iter().next();
    }

    // Parse .env for secrets (sibling to config.toml).
    let env_path = config_path.with_file_name(".env");
    if let Ok(content) = std::fs::read_to_string(env_path) {
        for line in content.lines() {
            if let Some((k, v)) = line.split_once('=') {
                match k {
                    "DISCORD_BOT_TOKEN" => vals.discord_token = Some(v.to_string()),
                    "OPENROUTER_API_KEY" | "KIMI_API_KEY" => {
                        vals.api_key = Some(v.to_string());
                    }
                    _ => {}
                }
            }
        }
    }

    vals
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

    let llama_info = if matches!(state.embeddings, Some(ServiceChoice::NixNative)) {
        let exe_path = format!("{home_str}/.nix-profile/bin/llama-server");
        Some((
            exe_path,
            state.embedding_hf_repo.clone().unwrap_or_default(),
            state.embedding_model.clone().unwrap_or_default(),
        ))
    } else {
        None
    };

    let llama_server =
        llama_info
            .as_ref()
            .map(|(exe, repo, model)| service_files::LlamaServerInfo {
                exe,
                hf_repo: repo,
                alias: model,
            });

    let docling_exe = if matches!(state.docling, Some(ServiceChoice::NixNative)) {
        Some(format!("{home_str}/.nix-profile/bin/docling-serve"))
    } else {
        None
    };

    let installed = service_files::install_all_service_files(
        &env.platform,
        &exe,
        &workspace,
        llama_server.as_ref(),
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
