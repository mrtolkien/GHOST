use std::path::Path;
use std::process::Command;

use super::detect::{ContainerRuntime, DetectedEnvironment, Platform};
use super::{OnboardingError, OnboardingState, SearchChoice, ServiceChoice};
use crate::services::{ServiceEntry, ServiceRegistry};

const SEARXNG_FRAGMENT: &str = include_str!("../../assets/services/docker-compose.searxng.yml");
const CRAWL4AI_FRAGMENT: &str = include_str!("../../assets/services/docker-compose.crawl4ai.yml");
const DOCLING_FRAGMENT: &str = include_str!("../../assets/services/docker-compose.docling.yml");
const SEARXNG_SETTINGS: &str = include_str!("../../assets/services/searxng-settings.yml");

/// Which container services to include in the compose file.
#[derive(Debug, Default)]
pub struct ServiceSelections {
    pub searxng: bool,
    pub crawl4ai: bool,
    pub docling_container: bool,
}

// ---------------------------------------------------------------------------
// Section header helper
// ---------------------------------------------------------------------------

fn show_section(
    title: &str,
    description: &str,
    link: &str,
    env: &DetectedEnvironment,
    service_detected: bool,
    detected_label: &str,
) {
    let _ = cliclack::log::step(format!("── {title} ──"));
    let _ = cliclack::log::info(description);
    let _ = cliclack::log::info(link);
    if service_detected {
        let _ = cliclack::log::success(format!("Detected: {detected_label} already running"));
    }
    let _ = env; // used implicitly through service_detected
}

// ---------------------------------------------------------------------------
// Embeddings
// ---------------------------------------------------------------------------

/// Result of the embeddings prompt: choice, model name, and optional HF repo.
pub struct EmbeddingSelection {
    pub choice: ServiceChoice,
    pub model: Option<String>,
    pub hf_repo: Option<String>,
}

/// Prompt the user for how to provide embeddings (llama-server).
pub fn prompt_embeddings(
    env: &DetectedEnvironment,
    flag: Option<&str>,
) -> Result<EmbeddingSelection, OnboardingError> {
    show_section(
        "Embeddings (llama-server)",
        "Your GHOST converts text into vectors for semantic search. \
         This lets it find relevant notes even when exact words don't match. \
         Powered by llama.cpp.",
        "https://github.com/ggml-org/llama.cpp",
        env,
        env.services_running.llama_server,
        "llama-server on :11434",
    );

    let choice = match flag {
        Some(f) => ServiceChoice::from_flag(f)?,
        None => prompt_embeddings_interactive(env)?,
    };

    if matches!(choice, ServiceChoice::Skip) {
        return Ok(EmbeddingSelection {
            choice,
            model: None,
            hf_repo: None,
        });
    }

    let from_flag = flag.is_some();

    // For NixNative (llama-server), ask for the HuggingFace repo first.
    let hf_repo = if matches!(choice, ServiceChoice::NixNative) {
        Some(prompt_hf_repo(from_flag)?)
    } else {
        None
    };

    let model = prompt_embedding_model(from_flag)?;
    Ok(EmbeddingSelection {
        choice,
        model: Some(model),
        hf_repo,
    })
}

fn prompt_embeddings_interactive(
    env: &DetectedEnvironment,
) -> Result<ServiceChoice, OnboardingError> {
    let default = if env.low_memory {
        ServiceChoice::Skip
    } else {
        ServiceChoice::NixNative
    };

    let mut select = cliclack::select("How should GHOST run llama-server for embeddings?");

    select = select.item(
        ServiceChoice::NixNative,
        "Install llama-server via nix (recommended)",
        "",
    );

    if env.services_running.llama_server {
        select = select.item(
            ServiceChoice::Remote("http://127.0.0.1:11434".to_string()),
            "Use existing (detected on :11434)",
            "",
        );
    }

    select = select.item(
        ServiceChoice::Remote(String::new()),
        "Remote — enter URL",
        "",
    );
    select = select.item(ServiceChoice::Skip, "Skip (embeddings unavailable)", "");

    let mut choice = select.initial_value(default).interact()?;

    // If "Remote — enter URL" was chosen (empty URL), prompt for actual URL.
    if choice == ServiceChoice::Remote(String::new()) {
        let url: String = cliclack::input("Enter llama-server URL:").interact()?;
        choice = ServiceChoice::Remote(url);
    }

    Ok(choice)
}

const DEFAULT_EMBEDDING_HF_REPO: &str = "Qwen/Qwen3-Embedding-8B-GGUF:Q8_0";
const DEFAULT_EMBEDDING_MODEL: &str = "qwen3-embedding:8b";

fn prompt_hf_repo(from_flag: bool) -> Result<String, OnboardingError> {
    if from_flag {
        return Ok(DEFAULT_EMBEDDING_HF_REPO.to_string());
    }

    let repo: String = cliclack::input("HuggingFace model repo (user/model[:quant]):")
        .default_input(DEFAULT_EMBEDDING_HF_REPO)
        .interact()?;
    Ok(repo)
}

fn prompt_embedding_model(from_flag: bool) -> Result<String, OnboardingError> {
    if from_flag {
        return Ok(DEFAULT_EMBEDDING_MODEL.to_string());
    }

    let model: String = cliclack::input("Embedding model name (used in API requests):")
        .default_input(DEFAULT_EMBEDDING_MODEL)
        .interact()?;
    Ok(model)
}

// ---------------------------------------------------------------------------
// Search
// ---------------------------------------------------------------------------

/// Prompt the user for web search configuration.
pub fn prompt_search(
    env: &DetectedEnvironment,
    flag: Option<&str>,
) -> Result<SearchChoice, OnboardingError> {
    show_section(
        "Web Search",
        "Your GHOST searches the web for information. \
         SearXNG is a self-hosted meta search engine — no API keys needed.",
        "https://docs.searxng.org",
        env,
        env.services_running.searxng,
        "SearXNG on :8080",
    );

    match flag {
        Some(f) => SearchChoice::from_flag(f),
        None => prompt_search_interactive(env),
    }
}

fn prompt_search_interactive(_env: &DetectedEnvironment) -> Result<SearchChoice, OnboardingError> {
    let default = SearchChoice::SearxngLocal;

    let mut choice = cliclack::select("How should GHOST search the web?")
        .item(
            SearchChoice::SearxngLocal,
            "SearXNG (local container)",
            "recommended — no API key needed",
        )
        .item(
            SearchChoice::BraveApi(String::new()),
            "Brave Search API",
            "requires API key",
        )
        .item(
            SearchChoice::SearxngRemote(String::new()),
            "SearXNG (remote URL)",
            "",
        )
        .item(SearchChoice::Skip, "Skip", "")
        .initial_value(default)
        .interact()?;

    // Prompt for extra data if needed.
    match &choice {
        SearchChoice::BraveApi(k) if k.is_empty() => {
            let key: String = cliclack::input("Brave Search API key:").interact()?;
            choice = SearchChoice::BraveApi(key);
        }
        SearchChoice::SearxngRemote(u) if u.is_empty() => {
            let url: String = cliclack::input("SearXNG URL:").interact()?;
            choice = SearchChoice::SearxngRemote(url);
        }
        _ => {}
    }

    Ok(choice)
}

// ---------------------------------------------------------------------------
// Crawl
// ---------------------------------------------------------------------------

/// Prompt the user for web crawling configuration.
pub fn prompt_crawl(
    env: &DetectedEnvironment,
    flag: Option<&str>,
) -> Result<ServiceChoice, OnboardingError> {
    show_section(
        "Web Crawling (Crawl4AI)",
        "Your GHOST reads web pages to extract content. \
         Crawl4AI renders JavaScript pages using headless Chrome.",
        "https://github.com/unclecode/crawl4ai",
        env,
        env.services_running.crawl4ai,
        "Crawl4AI on :11235",
    );

    match flag {
        Some(f) => ServiceChoice::from_flag(f),
        None => prompt_crawl_interactive(env),
    }
}

fn prompt_crawl_interactive(env: &DetectedEnvironment) -> Result<ServiceChoice, OnboardingError> {
    let mut choice = cliclack::select("How should GHOST crawl web pages?")
        .item(
            ServiceChoice::Container,
            "Container (Crawl4AI + headless Chrome)",
            "recommended",
        )
        .item(
            ServiceChoice::Remote(String::new()),
            "Remote — enter URL",
            "",
        )
        .item(ServiceChoice::Skip, "Skip", "")
        .initial_value(ServiceChoice::Container)
        .interact()?;

    if choice == ServiceChoice::Remote(String::new()) {
        let url: String = cliclack::input("Enter Crawl4AI URL:").interact()?;
        choice = ServiceChoice::Remote(url);
    }

    let _ = env;
    Ok(choice)
}

// ---------------------------------------------------------------------------
// Docling
// ---------------------------------------------------------------------------

/// Prompt the user for document processing configuration.
pub fn prompt_docling(
    env: &DetectedEnvironment,
    flag: Option<&str>,
) -> Result<ServiceChoice, OnboardingError> {
    show_section(
        "Document Processing (Docling)",
        "Your GHOST processes PDFs, Word docs, and presentations. \
         Docling handles OCR, tables, and complex layouts.",
        "https://github.com/docling-project/docling",
        env,
        env.services_running.docling,
        "Docling on :5001",
    );

    match flag {
        Some(f) => {
            let choice = ServiceChoice::from_flag(f)?;
            // NixNative docling is deprecated — map to Native (uv script).
            if choice == ServiceChoice::NixNative {
                Ok(ServiceChoice::Native)
            } else {
                Ok(choice)
            }
        }
        None => prompt_docling_interactive(env),
    }
}

fn prompt_docling_interactive(env: &DetectedEnvironment) -> Result<ServiceChoice, OnboardingError> {
    let default = if env.low_memory {
        ServiceChoice::Skip
    } else {
        ServiceChoice::Native
    };

    let mut choice = cliclack::select("How should GHOST process documents?")
        .item(
            ServiceChoice::Native,
            "Native (uv script, recommended)",
            "runs docling via Python — no container needed",
        )
        .item(ServiceChoice::Container, "Container", "")
        .item(
            ServiceChoice::Remote(String::new()),
            "Remote — enter URL",
            "",
        )
        .item(ServiceChoice::Skip, "Skip", "")
        .initial_value(default)
        .interact()?;

    if choice == ServiceChoice::Remote(String::new()) {
        let url: String = cliclack::input("Enter docling-serve URL:").interact()?;
        choice = ServiceChoice::Remote(url);
    }

    Ok(choice)
}

// ---------------------------------------------------------------------------
// Nix installation
// ---------------------------------------------------------------------------

/// Add a nix package via `nix profile add`.
pub fn nix_add(package: &str, message: &str) -> Result<(), OnboardingError> {
    let spinner = cliclack::spinner();
    spinner.start(message);

    let output = Command::new("nix")
        .args(["profile", "add", &format!("nixpkgs#{package}")])
        .output()
        .map_err(|e| OnboardingError::NixAdd(format!("failed to run nix: {e}")))?;

    if output.status.success() {
        spinner.stop(format!("{package} added"));
        Ok(())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        spinner.stop(format!("{package} add failed"));
        Err(OnboardingError::NixAdd(format!(
            "nix profile add nixpkgs#{package} failed: {stderr}"
        )))
    }
}

// ---------------------------------------------------------------------------
// Compose generation
// ---------------------------------------------------------------------------

/// Build a complete docker-compose.yml from selected service fragments.
pub fn generate_compose(selections: &ServiceSelections, is_linux: bool) -> String {
    let mut out = String::from("services:\n");

    if selections.searxng {
        append_fragment(&mut out, SEARXNG_FRAGMENT, is_linux, false);
    }
    if selections.crawl4ai {
        append_fragment(&mut out, CRAWL4AI_FRAGMENT, is_linux, true);
    }
    if selections.docling_container {
        append_fragment(&mut out, DOCLING_FRAGMENT, is_linux, false);
    }

    if !is_linux && has_any_service(selections) {
        out.push_str("\nnetworks:\n  ghost:\n    driver: bridge\n");
    }

    out
}

fn has_any_service(sel: &ServiceSelections) -> bool {
    sel.searxng || sel.crawl4ai || sel.docling_container
}

/// Append a YAML fragment, adjusting networking for the target platform.
///
/// Each fragment line is indented by 2 spaces so that service definitions
/// sit properly under the top-level `services:` key.
///
/// `add_host_network` — on Linux, inject `network_mode: host` for this
/// service (used for crawl4ai which needs to reach host-bound services).
fn append_fragment(out: &mut String, fragment: &str, is_linux: bool, add_host_network: bool) {
    for line in fragment.lines() {
        // Indent fragment lines by 2 spaces to nest under `services:`.
        // Keep blank lines blank.
        let indented = if line.is_empty() {
            String::new()
        } else {
            format!("  {line}")
        };

        if is_linux && is_ports_line(&indented) {
            // On Linux with host networking, skip port bindings.
            continue;
        }
        out.push_str(&indented);
        out.push('\n');

        // On Linux, inject network_mode after the service name line (top-level
        // service key, indented 2 spaces with a colon).
        if is_linux && add_host_network && is_service_name_line(&indented) {
            out.push_str("    network_mode: host\n");
        }

        // On macOS, inject extra_hosts + network after the service name line.
        if !is_linux && is_service_name_line(&indented) {
            out.push_str(
                "    extra_hosts:\n\
                 \x20     - \"host.docker.internal:host-gateway\"\n\
                 \x20   networks:\n\
                 \x20     - ghost\n",
            );
        }
    }
}

/// Check if a YAML line is a port binding (e.g. `    - "127.0.0.1:8080:8080"`).
fn is_ports_line(line: &str) -> bool {
    let trimmed = line.trim();
    // Match both the `ports:` key and individual port entries.
    trimmed == "ports:" || (trimmed.starts_with("- \"") && trimmed.contains(':'))
}

/// Check if a line is a top-level service name (2-space indent, ends with `:`).
fn is_service_name_line(line: &str) -> bool {
    let stripped = line.strip_prefix("  ");
    match stripped {
        Some(rest) => !rest.starts_with(' ') && rest.ends_with(':') && !rest.is_empty(),
        None => false,
    }
}

// ---------------------------------------------------------------------------
// State mapping
// ---------------------------------------------------------------------------

/// Convert wizard state into service selections for compose generation.
pub fn build_selections(state: &OnboardingState) -> ServiceSelections {
    ServiceSelections {
        searxng: matches!(state.search, Some(SearchChoice::SearxngLocal)),
        crawl4ai: matches!(state.crawl, Some(ServiceChoice::Container)),
        docling_container: matches!(state.docling, Some(ServiceChoice::Container)),
    }
}

// ---------------------------------------------------------------------------
// File writing
// ---------------------------------------------------------------------------

/// Write docker-compose.yml (and supporting configs) into the workspace.
pub fn write_compose_and_configs(
    workspace: &Path,
    selections: &ServiceSelections,
    is_linux: bool,
) -> Result<(), OnboardingError> {
    let services_dir = workspace.join("services");
    std::fs::create_dir_all(&services_dir)?;

    let compose = generate_compose(selections, is_linux);
    std::fs::write(services_dir.join("docker-compose.yml"), compose)?;

    if selections.searxng {
        std::fs::write(services_dir.join("searxng-settings.yml"), SEARXNG_SETTINGS)?;
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// services.toml generation
// ---------------------------------------------------------------------------

/// Build a `ServiceRegistry` reflecting the user's service choices.
///
/// Container entries are grouped under a single `[containers]` key.
/// NixNative services (e.g. llama-server) get individual entries with
/// platform-specific systemd or launchd commands baked in as absolute paths.
pub fn build_services_toml(
    state: &OnboardingState,
    platform: &Platform,
    workspace: &Path,
    container_runtime: Option<&ContainerRuntime>,
) -> ServiceRegistry {
    let mut registry = ServiceRegistry::default();

    // ── Container services ──────────────────────────────────────────────────
    let selections = build_selections(state);
    let has_containers = selections.searxng || selections.crawl4ai || selections.docling_container;

    if has_containers {
        let runtime = container_runtime
            .map(|r| r.compose_command())
            .unwrap_or("docker");
        let compose_file = workspace.join("services").join("docker-compose.yml");
        let compose_path = compose_file.display().to_string();

        let start = format!("{runtime} compose -f {compose_path} up -d");
        let stop = format!("{runtime} compose -f {compose_path} down");
        let update = format!(
            "{runtime} compose -f {compose_path} pull && {runtime} compose -f {compose_path} up -d"
        );

        // `add` only errors on duplicate name or empty entry — neither applies here.
        let _ = registry.add(
            "containers".to_string(),
            ServiceEntry {
                start: Some(start),
                stop: Some(stop),
                update: Some(update),
                status: None,
            },
        );
    }

    // ── llama-server (NixNative embeddings) ─────────────────────────────────
    if matches!(state.embeddings, Some(ServiceChoice::NixNative)) {
        let entry = native_service_entry(platform, "llama-server", "llama-cpp");
        let _ = registry.add("llama-server".to_string(), entry);
    }

    // Native docling does not need a service entry — it is invoked on demand.

    registry
}

/// Build platform-specific start/stop/update/status commands for a nix-installed
/// service managed by systemd (Linux) or launchd (macOS).
///
/// `service_name` — the unit name (e.g. `llama-server`)
/// `nix_package`  — the nixpkgs attribute to upgrade (e.g. `llama-cpp`)
fn native_service_entry(
    platform: &Platform,
    service_name: &str,
    nix_package: &str,
) -> ServiceEntry {
    match platform {
        Platform::Linux => ServiceEntry {
            start: Some(crate::systemd::systemctl_shell_cmd(&format!(
                "start {service_name}"
            ))),
            stop: Some(crate::systemd::systemctl_shell_cmd(&format!(
                "disable --now {service_name}"
            ))),
            update: Some(format!("nix profile upgrade nixpkgs#{nix_package}")),
            status: Some(crate::systemd::systemctl_shell_cmd(&format!(
                "is-active {service_name}"
            ))),
        },
        Platform::MacOs => {
            let uid = get_uid();
            let home = dirs::home_dir()
                .map(|h| h.display().to_string())
                .unwrap_or_else(|| "/Users/user".to_string());
            let label = format!("com.ghost.{service_name}");
            let plist = format!("{home}/Library/LaunchAgents/{label}.plist");

            ServiceEntry {
                start: Some(format!("launchctl bootstrap gui/{uid} {plist}")),
                stop: Some(format!("launchctl bootout gui/{uid}/{label}")),
                update: Some(format!("nix profile upgrade nixpkgs#{nix_package}")),
                status: Some(format!("launchctl print gui/{uid}/{label}")),
            }
        }
        Platform::Other(_) => ServiceEntry {
            start: Some(format!("echo 'start {service_name}'")),
            stop: Some(format!("echo 'stop {service_name}'")),
            update: Some(format!("nix profile upgrade nixpkgs#{nix_package}")),
            status: None,
        },
    }
}

/// Obtain the current user's numeric UID via `id -u`.
fn get_uid() -> u32 {
    Command::new("id")
        .arg("-u")
        .output()
        .ok()
        .and_then(|o| {
            String::from_utf8(o.stdout)
                .ok()
                .and_then(|s| s.trim().parse().ok())
        })
        .unwrap_or(501) // macOS default UID for first user
}

/// Generate and write `$WORKSPACE/services/services.toml` from wizard state.
///
/// Always writes the file (empty registry is a valid state). The services dir is
/// expected to already exist from `write_compose_and_configs`.
pub fn write_services_toml(
    state: &OnboardingState,
    platform: &Platform,
    workspace: &Path,
    container_runtime: Option<&ContainerRuntime>,
) -> Result<(), OnboardingError> {
    let registry = build_services_toml(state, platform, workspace, container_runtime);
    let path = workspace.join("services").join("services.toml");
    registry
        .save(&path)
        .map_err(|e| OnboardingError::Io(std::io::Error::other(e.to_string())))
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compose_with_all_services() {
        let sel = ServiceSelections {
            searxng: true,
            crawl4ai: true,
            docling_container: false,
        };
        let compose = generate_compose(&sel, true);
        assert!(compose.contains("searxng"));
        assert!(compose.contains("crawl4ai"));
        assert!(compose.contains("chrome"));
        assert!(!compose.contains("docling"));
    }

    #[test]
    fn compose_empty() {
        let sel = ServiceSelections::default();
        let compose = generate_compose(&sel, true);
        assert!(compose.contains("services:"));
    }

    #[test]
    fn compose_macos_no_host_network() {
        let sel = ServiceSelections {
            searxng: true,
            crawl4ai: true,
            docling_container: false,
        };
        let compose = generate_compose(&sel, false);
        assert!(!compose.contains("network_mode: host"));
    }

    #[test]
    fn compose_linux_has_host_network_for_crawl4ai() {
        let sel = ServiceSelections {
            searxng: false,
            crawl4ai: true,
            docling_container: false,
        };
        let compose = generate_compose(&sel, true);
        assert!(compose.contains("network_mode: host"));
    }

    #[test]
    fn build_selections_from_state() {
        let state = OnboardingState {
            search: Some(SearchChoice::SearxngLocal),
            crawl: Some(ServiceChoice::Container),
            docling: Some(ServiceChoice::Native),
            ..Default::default()
        };
        let sel = build_selections(&state);
        assert!(sel.searxng);
        assert!(sel.crawl4ai);
        assert!(!sel.docling_container);
    }

    #[test]
    fn build_selections_docling_container() {
        let state = OnboardingState {
            docling: Some(ServiceChoice::Container),
            ..Default::default()
        };
        let sel = build_selections(&state);
        assert!(sel.docling_container);
        assert!(!sel.searxng);
        assert!(!sel.crawl4ai);
    }

    #[test]
    fn search_choice_from_flag() {
        assert!(matches!(
            SearchChoice::from_flag("local"),
            Ok(SearchChoice::SearxngLocal)
        ));
        assert!(matches!(
            SearchChoice::from_flag("searxng"),
            Ok(SearchChoice::SearxngLocal)
        ));
        assert!(matches!(
            SearchChoice::from_flag("skip"),
            Ok(SearchChoice::Skip)
        ));
        assert!(SearchChoice::from_flag("invalid").is_err());

        match SearchChoice::from_flag("brave:abc123") {
            Ok(SearchChoice::BraveApi(k)) => assert_eq!(k, "abc123"),
            other => panic!("expected BraveApi, got: {other:?}"),
        }

        match SearchChoice::from_flag("remote:http://my.host:8080") {
            Ok(SearchChoice::SearxngRemote(u)) => {
                assert_eq!(u, "http://my.host:8080")
            }
            other => panic!("expected SearxngRemote, got: {other:?}"),
        }
    }

    #[test]
    fn compose_macos_has_bridge_network() {
        let sel = ServiceSelections {
            searxng: true,
            crawl4ai: false,
            docling_container: false,
        };
        let compose = generate_compose(&sel, false);
        assert!(compose.contains("networks:"));
        assert!(compose.contains("driver: bridge"));
        assert!(compose.contains("host.docker.internal"));
    }

    #[test]
    fn compose_linux_strips_ports() {
        let sel = ServiceSelections {
            searxng: true,
            crawl4ai: false,
            docling_container: false,
        };
        let compose = generate_compose(&sel, true);
        // On Linux, port bindings are stripped.
        assert!(!compose.contains("127.0.0.1:8080:8080"));
    }

    #[test]
    fn is_service_name_line_works() {
        assert!(is_service_name_line("  searxng:"));
        assert!(is_service_name_line("  crawl4ai:"));
        assert!(!is_service_name_line("    image: foo"));
        assert!(!is_service_name_line("services:"));
    }

    /// Parse compose output as YAML and return the `services` mapping.
    fn parse_compose(yaml: &str) -> serde_yaml::Value {
        serde_yaml::from_str(yaml).unwrap_or_else(|e| {
            panic!("generated compose is not valid YAML:\n{yaml}\n\nerror: {e}")
        })
    }

    #[test]
    fn compose_linux_all_services_valid_yaml() {
        let sel = ServiceSelections {
            searxng: true,
            crawl4ai: true,
            docling_container: true,
        };
        let yaml = generate_compose(&sel, true);
        let doc = parse_compose(&yaml);

        let services = doc["services"]
            .as_mapping()
            .expect("services should be a mapping");
        assert!(services.contains_key("searxng"), "missing searxng service");
        assert!(
            services.contains_key("crawl4ai"),
            "missing crawl4ai service"
        );
        assert!(services.contains_key("chrome"), "missing chrome service");
        assert!(
            services.contains_key("docling-serve"),
            "missing docling-serve service"
        );

        // crawl4ai should have network_mode: host on Linux
        assert_eq!(
            doc["services"]["crawl4ai"]["network_mode"].as_str(),
            Some("host")
        );

        // Ports should be stripped on Linux
        assert!(doc["services"]["searxng"].get("ports").is_none());
    }

    #[test]
    fn compose_macos_all_services_valid_yaml() {
        let sel = ServiceSelections {
            searxng: true,
            crawl4ai: true,
            docling_container: true,
        };
        let yaml = generate_compose(&sel, false);
        let doc = parse_compose(&yaml);

        let services = doc["services"]
            .as_mapping()
            .expect("services should be a mapping");
        assert!(services.contains_key("searxng"), "missing searxng service");
        assert!(
            services.contains_key("crawl4ai"),
            "missing crawl4ai service"
        );
        assert!(services.contains_key("chrome"), "missing chrome service");
        assert!(
            services.contains_key("docling-serve"),
            "missing docling-serve service"
        );

        // All services should have extra_hosts on macOS
        for name in &["searxng", "crawl4ai", "chrome", "docling-serve"] {
            let svc = &doc["services"][name];
            assert!(
                svc.get("extra_hosts").is_some(),
                "{name} missing extra_hosts on macOS"
            );
            assert!(
                svc.get("networks").is_some(),
                "{name} missing networks on macOS"
            );
        }

        // Should have bridge network definition
        assert_eq!(doc["networks"]["ghost"]["driver"].as_str(), Some("bridge"));

        // Ports should be preserved on macOS
        assert!(doc["services"]["searxng"].get("ports").is_some());
    }

    #[test]
    fn compose_single_service_valid_yaml() {
        // Each service fragment alone should produce valid YAML.
        for (label, sel) in [
            (
                "searxng-only",
                ServiceSelections {
                    searxng: true,
                    crawl4ai: false,
                    docling_container: false,
                },
            ),
            (
                "crawl4ai-only",
                ServiceSelections {
                    searxng: false,
                    crawl4ai: true,
                    docling_container: false,
                },
            ),
            (
                "docling-only",
                ServiceSelections {
                    searxng: false,
                    crawl4ai: false,
                    docling_container: true,
                },
            ),
        ] {
            for is_linux in [true, false] {
                let platform = if is_linux { "linux" } else { "macos" };
                let yaml = generate_compose(&sel, is_linux);
                let doc: serde_yaml::Value = serde_yaml::from_str(&yaml).unwrap_or_else(|e| {
                    panic!("{label}/{platform} is not valid YAML:\n{yaml}\n\nerror: {e}")
                });
                assert!(
                    doc["services"].as_mapping().is_some(),
                    "{label}/{platform} missing services mapping"
                );
            }
        }
    }
}
