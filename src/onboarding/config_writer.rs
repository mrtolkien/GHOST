use std::path::Path;

use similar::{ChangeTag, TextDiff};

use super::{OnboardingError, OnboardingState, SearchChoice, ServiceChoice};
use crate::config::ProviderKind;

/// Keys managed by the onboarding wizard in the .env file.
const MANAGED_ENV_KEYS: &[&str] = &[
    "DISCORD_BOT_TOKEN",
    "OPENROUTER_API_KEY",
    "KIMI_API_KEY",
    "BRAVE_API_KEY",
];

/// Build a config.toml string from the wizard state.
///
/// Sections are written in a fixed order; uses hand-rolled formatting rather
/// than a serialiser so the output is stable and human-readable.
#[must_use]
pub fn generate_config_toml(state: &OnboardingState) -> String {
    let mut out = String::new();

    // [models]
    out.push_str("[models]\ndefault = \"primary\"\n\n");

    // [models.primary]
    out.push_str("[models.primary]\n");
    if let Some(provider) = &state.provider {
        out.push_str(&format!("provider = \"{}\"\n", provider.as_str()));
    }
    if let Some(model) = &state.model {
        out.push_str(&format!("model = \"{model}\"\n"));
    }
    if let Some(cw) = state.context_window {
        out.push_str(&format!("context_window = {cw}\n"));
    }
    out.push('\n');

    // [discord]
    out.push_str("[discord]\n");
    if let Some(uid) = &state.discord_user_id {
        out.push_str(&format!("allowed_user_id = \"{uid}\"\n"));
    }
    out.push('\n');

    // [embeddings]
    if let Some(emb) = &state.embeddings {
        if *emb != ServiceChoice::Skip {
            out.push_str("[embeddings]\n");
            let url = match emb {
                ServiceChoice::Remote(u) => u.as_str(),
                _ => "http://127.0.0.1:11434",
            };
            out.push_str(&format!("url = \"{url}\"\n"));
            if let Some(m) = &state.embedding_model {
                out.push_str(&format!("model = \"{m}\"\n"));
            }
            out.push('\n');
        }
    }

    // [web.search]
    if let Some(search) = &state.search {
        match search {
            SearchChoice::Skip => {}
            SearchChoice::BraveApi(_) => {
                out.push_str("[web.search]\nprovider = \"brave\"\n\n");
            }
            SearchChoice::SearxngLocal => {
                out.push_str(
                    "[web.search]\nprovider = \"searxng\"\nurl = \"http://127.0.0.1:8080\"\n\n",
                );
            }
            SearchChoice::SearxngRemote(url) => {
                out.push_str(&format!(
                    "[web.search]\nprovider = \"searxng\"\nurl = \"{url}\"\n\n"
                ));
            }
        }
    }

    // [web] crawl section
    if let Some(crawl) = &state.crawl {
        if *crawl != ServiceChoice::Skip {
            let crawl_url = match crawl {
                ServiceChoice::Remote(u) => u.as_str(),
                _ => "http://127.0.0.1:11235",
            };
            out.push_str(&format!("[web]\ncrawl4ai_url = \"{crawl_url}\"\n\n"));
            // [[web.browsers]]
            out.push_str(
                "[[web.browsers]]\nname = \"chrome\"\ncdp_url = \"http://127.0.0.1:9222\"\n\n",
            );
        }
    }

    // [docling]
    if let Some(docling) = &state.docling {
        if *docling != ServiceChoice::Skip {
            let docling_url = match docling {
                ServiceChoice::Remote(u) => u.as_str(),
                _ => "http://127.0.0.1:5001",
            };
            out.push_str(&format!("[docling]\nurl = \"{docling_url}\"\n\n"));
        }
    }

    // Trim trailing newline — one final newline is canonical.
    let trimmed = out.trim_end_matches('\n');
    format!("{trimmed}\n")
}

/// Build .env file content containing only secrets managed by the wizard.
///
/// The API key env-var name is provider-specific. OAuth providers do not
/// write an API key line.
#[must_use]
pub fn generate_env(state: &OnboardingState) -> String {
    let mut lines: Vec<String> = Vec::new();

    if let Some(token) = &state.discord_token {
        lines.push(format!("DISCORD_BOT_TOKEN={token}"));
    }

    if let Some(provider) = &state.provider {
        let key_name = provider_env_key(provider);
        if let Some(key_name) = key_name {
            if let Some(api_key) = &state.api_key {
                lines.push(format!("{key_name}={api_key}"));
            }
        }
    }

    if let Some(SearchChoice::BraveApi(brave_key)) = &state.search {
        lines.push(format!("BRAVE_API_KEY={brave_key}"));
    }

    let mut out = lines.join("\n");
    if !out.is_empty() {
        out.push('\n');
    }
    out
}

/// Returns the env-var name for the provider's API key, or `None` for OAuth
/// providers that don't use a static key.
fn provider_env_key(provider: &ProviderKind) -> Option<&'static str> {
    match provider {
        ProviderKind::OpenRouter => Some("OPENROUTER_API_KEY"),
        ProviderKind::Kimi => Some("KIMI_API_KEY"),
        ProviderKind::Anthropic | ProviderKind::OpenAiOAuth => None,
    }
}

/// Compute a human-readable unified diff between two config strings.
///
/// Lines prefixed with `+` are additions, `-` are deletions, ` ` are
/// unchanged context.
#[must_use]
pub fn compute_config_diff(old: &str, new: &str) -> String {
    let diff = TextDiff::from_lines(old, new);
    let mut out = String::new();
    for change in diff.iter_all_changes() {
        let prefix = match change.tag() {
            ChangeTag::Insert => '+',
            ChangeTag::Delete => '-',
            ChangeTag::Equal => ' ',
        };
        // `value()` already includes a trailing newline for each line.
        out.push(prefix);
        out.push_str(change.value());
    }
    out
}

/// Show the config.toml diff with syntax highlighting and prompt to confirm.
pub fn display_diff_and_confirm(
    old_config: &str,
    new_config: &str,
) -> Result<bool, OnboardingError> {
    let diff = compute_config_diff(old_config, new_config);
    let colored = colorize_diff_toml(&diff);
    cliclack::note("Changes to config.toml", &colored)
        .map_err(|e| OnboardingError::Io(std::io::Error::other(e.to_string())))?;
    let confirmed = cliclack::confirm("Apply these changes?")
        .interact()
        .map_err(|e| OnboardingError::Io(std::io::Error::other(e.to_string())))?;
    Ok(confirmed)
}

// ---------------------------------------------------------------------------
// TOML syntax highlighting with diff colors
// ---------------------------------------------------------------------------

/// Colorize a unified diff of TOML content.
///
/// Added lines get a dark green background, deleted lines dark red. TOML
/// syntax (section headers, key/value pairs) is highlighted on top.
fn colorize_diff_toml(diff: &str) -> String {
    let mut out = String::new();
    for line in diff.lines() {
        match line.chars().next() {
            Some('+') => {
                // Dark green background, TOML highlighting with fg-only resets.
                out.push_str("\x1b[48;5;22m+");
                out.push_str(&highlight_toml(&line[1..], true));
                out.push_str("\x1b[0m\n");
            }
            Some('-') => {
                // Dark red background, TOML highlighting with fg-only resets.
                out.push_str("\x1b[48;5;52m-");
                out.push_str(&highlight_toml(&line[1..], true));
                out.push_str("\x1b[0m\n");
            }
            Some(' ') => {
                out.push(' ');
                out.push_str(&highlight_toml(&line[1..], false));
                out.push('\n');
            }
            _ => {
                out.push_str(line);
                out.push('\n');
            }
        }
    }
    out
}

/// Apply TOML syntax coloring to a single line.
///
/// When `preserve_bg` is true, resets only affect foreground (so diff
/// background color is preserved).
fn highlight_toml(line: &str, preserve_bg: bool) -> String {
    let reset = if preserve_bg {
        "\x1b[39;22m"
    } else {
        "\x1b[0m"
    };
    let trimmed = line.trim();

    // Section headers: [section] or [[section]]
    if (trimmed.starts_with('[') && trimmed.ends_with(']'))
        || (trimmed.starts_with("[[") && trimmed.ends_with("]]"))
    {
        return format!("\x1b[1;36m{line}{reset}");
    }

    // Key = value pairs
    if let Some((key, value)) = line.split_once(" = ") {
        return format!("{key} = \x1b[33m{value}{reset}");
    }

    line.to_string()
}

/// Write config.toml and .env to `config_dir`.
///
/// For .env, lines not owned by the wizard (i.e. not matching any key in
/// `MANAGED_ENV_KEYS`) are preserved verbatim; wizard-managed keys are
/// updated/added.
pub fn write_config_files(
    config_dir: &Path,
    config_toml: &str,
    env_content: &str,
) -> Result<(), OnboardingError> {
    std::fs::create_dir_all(config_dir)?;

    // config.toml — always overwritten wholesale.
    std::fs::write(config_dir.join("config.toml"), config_toml)?;

    // .env — merge: preserve unmanaged lines, update/add managed ones.
    let env_path = config_dir.join(".env");
    let merged = if env_path.exists() {
        let existing = std::fs::read_to_string(&env_path)?;
        merge_env(&existing, env_content)
    } else {
        env_content.to_string()
    };
    std::fs::write(env_path, merged)?;

    Ok(())
}

/// Merge new wizard-managed env lines into an existing .env string.
///
/// - Lines in `existing` whose key is not in `MANAGED_ENV_KEYS` are kept.
/// - Managed keys present in `new_content` replace or are appended.
/// - Managed keys absent from `new_content` are removed.
fn merge_env(existing: &str, new_content: &str) -> String {
    // Parse new key=value pairs for managed keys.
    let mut new_managed: std::collections::HashMap<&str, &str> = std::collections::HashMap::new();
    for line in new_content.lines() {
        if let Some((k, v)) = line.split_once('=') {
            if MANAGED_ENV_KEYS.contains(&k) {
                new_managed.insert(k, v);
            }
        }
    }

    let mut out_lines: Vec<String> = Vec::new();

    // Keep non-managed lines unchanged; replace managed ones inline.
    for line in existing.lines() {
        let key = line.split_once('=').map(|(k, _)| k);
        match key {
            Some(k) if MANAGED_ENV_KEYS.contains(&k) => {
                // Replace with new value if present; drop if absent.
                if let Some(new_val) = new_managed.remove(k) {
                    out_lines.push(format!("{k}={new_val}"));
                }
            }
            _ => {
                out_lines.push(line.to_string());
            }
        }
    }

    // Append any new managed keys that weren't already in the file.
    for key in MANAGED_ENV_KEYS {
        if let Some(val) = new_managed.get(key) {
            out_lines.push(format!("{key}={val}"));
        }
    }

    let mut result = out_lines.join("\n");
    if !result.is_empty() {
        result.push('\n');
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::onboarding::*;

    #[test]
    fn generates_valid_config_toml() {
        let state = OnboardingState {
            provider: Some(ProviderKind::OpenRouter),
            api_key: Some("sk-test".into()),
            model: Some("anthropic/claude-sonnet-4".into()),
            context_window: Some(200_000),
            discord_token: Some("token".into()),
            discord_user_id: Some("123456789012345678".into()),
            embeddings: Some(ServiceChoice::NixNative),
            embedding_model: Some("qwen3-embedding:8b".into()),
            search: Some(SearchChoice::SearxngLocal),
            crawl: Some(ServiceChoice::Container),
            docling: Some(ServiceChoice::NixNative),
        };
        let toml_str = generate_config_toml(&state);
        let parsed: toml::Value = toml::from_str(&toml_str).expect("valid TOML");
        assert_eq!(
            parsed["models"]["primary"]["provider"].as_str(),
            Some("openrouter")
        );
        assert_eq!(
            parsed["discord"]["allowed_user_id"].as_str(),
            Some("123456789012345678")
        );
        assert_eq!(
            parsed["embeddings"]["model"].as_str(),
            Some("qwen3-embedding:8b")
        );
    }

    #[test]
    fn generates_env_file() {
        let state = OnboardingState {
            provider: Some(ProviderKind::OpenRouter),
            api_key: Some("sk-or-test-123".into()),
            discord_token: Some("discord-token-123".into()),
            ..Default::default()
        };
        let env_str = generate_env(&state);
        assert!(env_str.contains("OPENROUTER_API_KEY=sk-or-test-123"));
        assert!(env_str.contains("DISCORD_BOT_TOKEN=discord-token-123"));
    }

    #[test]
    fn diff_shows_additions() {
        let old = "";
        let new = "[discord]\nallowed_user_id = \"123\"\n";
        let diff = compute_config_diff(old, new);
        assert!(diff.contains("+[discord]"));
    }

    #[test]
    fn env_kimi_provider() {
        let state = OnboardingState {
            provider: Some(ProviderKind::Kimi),
            api_key: Some("kimi-key".into()),
            discord_token: Some("tok".into()),
            ..Default::default()
        };
        let env_str = generate_env(&state);
        assert!(env_str.contains("KIMI_API_KEY=kimi-key"));
        assert!(!env_str.contains("OPENROUTER"));
    }

    #[test]
    fn env_oauth_no_api_key() {
        let state = OnboardingState {
            provider: Some(ProviderKind::Anthropic),
            discord_token: Some("tok".into()),
            ..Default::default()
        };
        let env_str = generate_env(&state);
        assert!(!env_str.contains("API_KEY="));
        assert!(env_str.contains("DISCORD_BOT_TOKEN=tok"));
    }

    #[test]
    fn merge_env_preserves_unmanaged_lines() {
        let existing = "MY_CUSTOM_VAR=hello\nDISCORD_BOT_TOKEN=old\nANOTHER=world\n";
        let new_content = "DISCORD_BOT_TOKEN=new\n";
        let merged = merge_env(existing, new_content);
        assert!(merged.contains("MY_CUSTOM_VAR=hello"));
        assert!(merged.contains("ANOTHER=world"));
        assert!(merged.contains("DISCORD_BOT_TOKEN=new"));
        assert!(!merged.contains("DISCORD_BOT_TOKEN=old"));
    }

    #[test]
    fn brave_search_writes_provider_only() {
        let state = OnboardingState {
            provider: Some(ProviderKind::OpenRouter),
            api_key: Some("k".into()),
            discord_token: Some("t".into()),
            discord_user_id: Some("1".into()),
            search: Some(SearchChoice::BraveApi("brave-secret".into())),
            ..Default::default()
        };
        let toml_str = generate_config_toml(&state);
        let parsed: toml::Value = toml::from_str(&toml_str).expect("valid TOML");
        assert_eq!(parsed["web"]["search"]["provider"].as_str(), Some("brave"));
        // URL should NOT be present for BraveApi
        assert!(parsed["web"]["search"].get("url").is_none());

        let env_str = generate_env(&state);
        assert!(env_str.contains("BRAVE_API_KEY=brave-secret"));
    }
}
