use std::collections::BTreeMap;
use std::env;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Once};

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::providers::openai_compatible::ProviderRouting;
use crate::providers::types::ReasoningEffort;

/// Accepts either a single string or a list of strings in TOML/serde.
/// Normalized internally to `Vec<String>`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct StringOrList(Vec<String>);

impl StringOrList {
    pub fn as_slice(&self) -> &[String] {
        &self.0
    }

    pub fn first(&self) -> Option<&str> {
        self.0.first().map(String::as_str)
    }

    pub fn into_vec(self) -> Vec<String> {
        self.0
    }

    pub fn from_vec(v: Vec<String>) -> Self {
        Self(v)
    }
}

impl<'de> serde::Deserialize<'de> for StringOrList {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        use serde::de;

        struct Visitor;

        impl<'de> de::Visitor<'de> for Visitor {
            type Value = StringOrList;

            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("a string or list of strings")
            }

            fn visit_str<E: de::Error>(self, v: &str) -> Result<StringOrList, E> {
                Ok(StringOrList(vec![v.to_string()]))
            }

            fn visit_seq<A: de::SeqAccess<'de>>(
                self,
                mut seq: A,
            ) -> Result<StringOrList, A::Error> {
                let mut v = Vec::new();
                while let Some(s) = seq.next_element::<String>()? {
                    v.push(s);
                }
                if v.is_empty() {
                    return Err(de::Error::custom("model chain cannot be empty"));
                }
                Ok(StringOrList(v))
            }
        }

        deserializer.deserialize_any(Visitor)
    }
}

impl From<String> for StringOrList {
    fn from(s: String) -> Self {
        Self(vec![s])
    }
}

pub const CONFIG_DIR_ENV: &str = "GHOST_CONFIG_DIR";
const CONFIG_FILE_NAME: &str = "config.toml";
const DEFAULT_WORKSPACE: &str = "~/GHOST";
const DEFAULT_EMBEDDINGS_URL: &str = "http://127.0.0.1:11434";
const DEFAULT_EMBEDDINGS_MODEL: &str = "qwen3-embedding:8b";
static DOTENV_INIT: Once = Once::new();

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("home directory is unavailable; cannot resolve {path}")]
    HomeDirUnavailable { path: String },

    #[error("failed to read config file {path}: {source}")]
    ReadFile {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("invalid config in {path}: {source}")]
    Parse {
        path: PathBuf,
        #[source]
        source: toml::de::Error,
    },

    #[error("failed to serialize config for {path}: {source}")]
    Serialize {
        path: PathBuf,
        #[source]
        source: toml::ser::Error,
    },

    #[error("failed to write config file {path}: {source}")]
    WriteFile {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("invalid config key '{key}'")]
    InvalidKey { key: String },

    #[error("config key '{key}' not found")]
    KeyNotFound { key: String },

    #[error("default model alias '{alias}' does not exist in models")]
    UnknownDefaultModelAlias { alias: String },

    #[error("models section must define at least one alias")]
    MissingModels,

    #[error("models.default is required when multiple model aliases are defined")]
    MissingDefaultModelAlias,

    #[error("web.search.url is required when provider is 'searxng'")]
    MissingSearxngUrl,

    #[error("cannot change '{field}' at runtime (requires restart)")]
    ImmutableFieldChanged { field: String },
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Settings {
    pub workspace: Option<String>,
    pub models: Option<ModelsSettings>,
    pub discord: Option<DiscordSettings>,
    pub embeddings: Option<EmbeddingsSettings>,
    pub timing: Option<TimingSettings>,
    pub compaction: Option<CompactionSettings>,
    pub web: Option<WebSettings>,
    pub docling: Option<DoclingSettings>,
    pub coding: Option<CodingSettings>,
    pub debug: Option<DebugSettings>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ModelsSettings {
    pub default: Option<StringOrList>,
    #[serde(flatten)]
    pub aliases: BTreeMap<String, ModelSettings>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ModelSettings {
    pub provider: ProviderKind,
    pub model: String,
    pub context_window: u32,
    #[serde(default)]
    pub headers: BTreeMap<String, String>,
    pub reasoning_effort: Option<ReasoningEffort>,
    /// OpenRouter provider routing preferences (only/ignore/order).
    pub provider_routing: Option<ProviderRouting>,
    /// Codex text output verbosity (default: "low"). Set to "medium" for
    /// models like gpt-5-codex that don't support "low".
    pub text_verbosity: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DiscordSettings {
    pub enabled: Option<bool>,
    #[serde(
        default,
        deserialize_with = "deserialize_string_or_vec",
        skip_serializing_if = "Vec::is_empty"
    )]
    pub allowed_user_id: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EmbeddingsSettings {
    pub url: Option<String>,
    pub model: Option<String>,
    pub batch_size: Option<usize>,
    pub dimension: Option<usize>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TimingSettings {
    pub reflection_idle_minutes: Option<u64>,
    pub scheduler_tick_seconds: Option<u64>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CompactionSettings {
    pub threshold: Option<f64>,
    pub mask_preview_chars: Option<usize>,
    /// Extra instructions appended to the compaction prompt.
    pub instructions: Option<String>,
}

/// A browser definition in config.toml.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct BrowserSettings {
    pub name: String,
    pub cdp_url: String,
    /// Marker for browsers added by `discover` (not manually).
    #[serde(default)]
    pub discovered: bool,
}

/// Resolved browser configuration.
#[derive(Debug, Clone, Serialize)]
pub struct BrowserConfig {
    pub name: String,
    pub cdp_url: String,
    pub discovered: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct WebSettings {
    pub search_max_results: Option<usize>,
    pub crawl4ai_url: Option<String>,
    /// Deprecated: use [[web.browsers]] instead. Kept for config compat.
    pub chrome_cdp_url: Option<String>,
    pub browsers: Option<Vec<BrowserSettings>>,
    pub search: Option<SearchProviderSettings>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DoclingSettings {
    pub url: Option<String>,
    pub timeout: Option<u64>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SearchProviderSettings {
    pub provider: SearchProviderKind,
    pub url: Option<String>,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum SearchProviderKind {
    Brave,
    Searxng,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CodingSettings {
    pub model: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DebugSettings {
    pub save_requests: Option<bool>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Config {
    pub workspace: PathBuf,
    pub models: ModelsConfig,
    pub discord: DiscordConfig,
    pub embeddings: EmbeddingsConfig,
    pub timing: TimingConfig,
    pub compaction: CompactionConfig,
    pub web: WebConfig,
    pub docling: DoclingConfig,
    pub coding: CodingConfig,
    pub debug: DebugConfig,
    /// Whether to install bundled docs to references/ghost/docs/ on boot.
    /// Defaults to true; set to false in tests to avoid embedding overhead.
    #[serde(skip)]
    pub install_bundled_docs: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct ModelsConfig {
    /// First alias in the chain -- used for context window, metadata, etc.
    pub default: String,
    /// Full ordered chain of aliases for fallback.
    pub default_chain: Vec<String>,
    #[serde(flatten)]
    pub aliases: BTreeMap<String, ModelConfig>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ModelConfig {
    pub provider: ProviderKind,
    pub model: String,
    pub context_window: u32,
    pub headers: BTreeMap<String, String>,
    pub reasoning_effort: Option<ReasoningEffort>,
    /// OpenRouter provider routing preferences (only/ignore/order).
    pub provider_routing: Option<ProviderRouting>,
    /// Codex text output verbosity (default: "low").
    pub text_verbosity: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DiscordConfig {
    pub enabled: bool,
    pub allowed_user_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct EmbeddingsConfig {
    pub url: String,
    pub model: String,
    pub batch_size: usize,
    pub dimension: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct TimingConfig {
    pub reflection_idle_minutes: u64,
    pub scheduler_tick_seconds: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct CompactionConfig {
    pub threshold: f64,
    pub mask_preview_chars: usize,
    /// Extra instructions appended to the compaction prompt.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub instructions: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct WebConfig {
    pub search_max_results: usize,
    pub crawl4ai_url: Option<String>,
    pub browsers: Vec<BrowserConfig>,
    pub search_provider: SearchProviderConfig,
}

#[derive(Debug, Clone, Serialize)]
pub struct DoclingConfig {
    pub url: Option<String>,
    pub timeout: u64,
}

#[derive(Debug, Clone, Serialize)]
pub enum SearchProviderConfig {
    Brave,
    Searxng { url: String },
}

#[derive(Debug, Clone, Serialize)]
pub struct CodingConfig {
    pub model: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DebugConfig {
    pub save_requests: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ProviderKind {
    OpenRouter,
    #[serde(alias = "kimi_code")]
    Kimi,
    #[serde(alias = "openai_oauth")]
    OpenAiOAuth,
    Anthropic,
}

impl ProviderKind {
    /// Stable string for serialization to config.toml.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::OpenRouter => "openrouter",
            Self::Kimi => "kimi_code",
            Self::OpenAiOAuth => "openai_oauth",
            Self::Anthropic => "anthropic",
        }
    }

    /// Parse from a CLI flag value (user-facing aliases).
    pub fn from_cli_flag(s: &str) -> Option<Self> {
        match s {
            "openrouter" => Some(Self::OpenRouter),
            "anthropic" => Some(Self::Anthropic),
            "kimi" | "kimi_code" => Some(Self::Kimi),
            "openai-oauth" | "chatgpt-oauth" | "openai_oauth" => Some(Self::OpenAiOAuth),
            _ => None,
        }
    }
}

impl Config {
    /// Resolve a user-facing `Settings` (with optional fields and tilde paths)
    /// into a fully validated `Config` with concrete defaults.
    #[tracing::instrument(skip_all)]
    pub fn from_settings(settings: Settings) -> Result<Self, ConfigError> {
        let workspace = expand_tilde(settings.workspace.as_deref().unwrap_or(DEFAULT_WORKSPACE))?;

        let aliases = settings
            .models
            .as_ref()
            .map(|m| m.aliases.clone())
            .unwrap_or_default();
        if aliases.is_empty() {
            return Err(ConfigError::MissingModels);
        }

        let resolved_aliases = aliases
            .into_iter()
            .map(|(name, model)| {
                (
                    name,
                    ModelConfig {
                        provider: model.provider,
                        model: model.model,
                        context_window: model.context_window,
                        headers: model.headers,
                        reasoning_effort: model.reasoning_effort,
                        provider_routing: model.provider_routing,
                        text_verbosity: model.text_verbosity,
                    },
                )
            })
            .collect::<BTreeMap<_, _>>();

        let default_chain: Vec<String> = settings
            .models
            .as_ref()
            .and_then(|m| m.default.clone())
            .map(|sol| sol.into_vec())
            .unwrap_or_else(|| {
                if resolved_aliases.len() == 1 {
                    vec![resolved_aliases.keys().next().cloned().unwrap_or_default()]
                } else {
                    vec![]
                }
            });

        let default_model_alias = default_chain.first().cloned().unwrap_or_default();

        if default_model_alias.is_empty() {
            return Err(ConfigError::MissingDefaultModelAlias);
        }

        // Validate ALL aliases in the chain exist
        for alias in &default_chain {
            if !resolved_aliases.contains_key(alias) {
                return Err(ConfigError::UnknownDefaultModelAlias {
                    alias: alias.clone(),
                });
            }
        }

        Ok(Self {
            workspace,
            models: ModelsConfig {
                default: default_model_alias,
                default_chain,
                aliases: resolved_aliases,
            },
            discord: DiscordConfig {
                enabled: settings
                    .discord
                    .as_ref()
                    .and_then(|d| d.enabled)
                    .unwrap_or(true),
                allowed_user_ids: settings
                    .discord
                    .as_ref()
                    .map(|d| d.allowed_user_id.clone())
                    .unwrap_or_default(),
            },
            embeddings: EmbeddingsConfig {
                url: settings
                    .embeddings
                    .as_ref()
                    .and_then(|e| e.url.clone())
                    .unwrap_or_else(|| DEFAULT_EMBEDDINGS_URL.to_string()),
                model: settings
                    .embeddings
                    .as_ref()
                    .and_then(|e| e.model.clone())
                    .unwrap_or_else(|| DEFAULT_EMBEDDINGS_MODEL.to_string()),
                batch_size: settings
                    .embeddings
                    .as_ref()
                    .and_then(|e| e.batch_size)
                    .unwrap_or(32),
                dimension: settings
                    .embeddings
                    .as_ref()
                    .and_then(|e| e.dimension)
                    .unwrap_or(1024),
            },
            timing: TimingConfig {
                reflection_idle_minutes: settings
                    .timing
                    .as_ref()
                    .and_then(|t| t.reflection_idle_minutes)
                    .unwrap_or(10),
                scheduler_tick_seconds: settings
                    .timing
                    .as_ref()
                    .and_then(|t| t.scheduler_tick_seconds)
                    .unwrap_or(60),
            },
            compaction: CompactionConfig {
                threshold: settings
                    .compaction
                    .as_ref()
                    .and_then(|c| c.threshold)
                    .unwrap_or(0.90),
                mask_preview_chars: settings
                    .compaction
                    .as_ref()
                    .and_then(|c| c.mask_preview_chars)
                    .unwrap_or(100),
                instructions: settings
                    .compaction
                    .as_ref()
                    .and_then(|c| c.instructions.clone()),
            },
            web: {
                let crawl4ai_url = settings
                    .web
                    .as_ref()
                    .and_then(|w| w.crawl4ai_url.clone())
                    .or_else(|| env::var("CRAWL4AI_URL").ok());

                let browsers = {
                    let configured = settings
                        .web
                        .as_ref()
                        .and_then(|w| w.browsers.clone())
                        .unwrap_or_default();

                    if !configured.is_empty() {
                        configured
                            .into_iter()
                            .map(|b| BrowserConfig {
                                name: b.name,
                                cdp_url: b.cdp_url,
                                discovered: b.discovered,
                            })
                            .collect()
                    } else {
                        // Deprecated config field fallback
                        let legacy_url = settings
                            .web
                            .as_ref()
                            .and_then(|w| w.chrome_cdp_url.clone())
                            .or_else(|| env::var("CHROME_CDP_URL").ok());

                        if let Some(url) = legacy_url {
                            vec![BrowserConfig {
                                name: "headless".to_string(),
                                cdp_url: url,
                                discovered: false,
                            }]
                        } else {
                            vec![]
                        }
                    }
                };

                let search_provider = match settings.web.as_ref().and_then(|w| w.search.as_ref()) {
                    Some(s) => match s.provider {
                        SearchProviderKind::Brave => SearchProviderConfig::Brave,
                        SearchProviderKind::Searxng => SearchProviderConfig::Searxng {
                            url: s.url.clone().ok_or(ConfigError::MissingSearxngUrl)?,
                        },
                    },
                    None => {
                        // Fall back to SEARXNG_URL env var if set
                        if let Ok(url) = env::var("SEARXNG_URL") {
                            SearchProviderConfig::Searxng { url }
                        } else {
                            SearchProviderConfig::Brave
                        }
                    }
                };
                WebConfig {
                    search_max_results: settings
                        .web
                        .as_ref()
                        .and_then(|w| w.search_max_results)
                        .unwrap_or(5),
                    crawl4ai_url,
                    browsers,
                    search_provider,
                }
            },
            docling: {
                let url = settings
                    .docling
                    .as_ref()
                    .and_then(|d| d.url.clone())
                    .or_else(|| env::var("DOCLING_URL").ok());
                let timeout = settings
                    .docling
                    .as_ref()
                    .and_then(|d| d.timeout)
                    .unwrap_or(600);
                DoclingConfig { url, timeout }
            },
            coding: CodingConfig {
                model: settings.coding.as_ref().and_then(|c| c.model.clone()),
            },
            debug: DebugConfig {
                save_requests: settings
                    .debug
                    .as_ref()
                    .and_then(|d| d.save_requests)
                    .unwrap_or(false),
            },
            install_bundled_docs: true,
        })
    }
}

/// A shared, dynamically-reloadable config handle.
/// Cheap to clone — distribute to all consumers.
pub type SharedConfig = tokio::sync::watch::Receiver<Arc<Config>>;

/// Sender half — held by the daemon to publish config updates.
pub(crate) type ConfigSender = tokio::sync::watch::Sender<Arc<Config>>;

/// Convenience: grab a snapshot without dealing with borrow lifetimes.
pub trait SharedConfigExt {
    fn current(&self) -> Arc<Config>;
}

impl SharedConfigExt for SharedConfig {
    fn current(&self) -> Arc<Config> {
        self.borrow().clone()
    }
}

#[tracing::instrument(skip_all)]
pub fn load() -> Result<Config, ConfigError> {
    load_dotenv();
    load_from_dir(&config_dir()?)
}

/// Re-read `.env` and `config.toml` for hot-reload.
///
/// Unlike `load()`, this always re-reads `.env` (bypassing the `Once` guard)
/// so that environment variable changes take effect. Uses `from_path_override`
/// to overwrite existing env vars with new `.env` values.
pub fn reload() -> Result<Config, ConfigError> {
    reload_dotenv();
    load_from_dir(&config_dir()?)
}

/// Check that immutable fields haven't changed between old and new config.
pub fn validate_reload(current: &Config, new: &Config) -> Result<(), ConfigError> {
    if current.workspace != new.workspace {
        return Err(ConfigError::ImmutableFieldChanged {
            field: "workspace".into(),
        });
    }
    if current.embeddings.dimension != new.embeddings.dimension {
        return Err(ConfigError::ImmutableFieldChanged {
            field: "embeddings.dimension".into(),
        });
    }
    if current.discord.enabled != new.discord.enabled {
        return Err(ConfigError::ImmutableFieldChanged {
            field: "discord.enabled".into(),
        });
    }
    Ok(())
}

#[tracing::instrument(skip_all, fields(config_dir = %config_dir.display()))]
pub fn load_from_dir(config_dir: &Path) -> Result<Config, ConfigError> {
    let settings = load_settings_from_path(&config_dir.join(CONFIG_FILE_NAME))?;
    Config::from_settings(settings)
}

fn load_dotenv() {
    DOTENV_INIT.call_once(|| {
        load_dotenv_from_config_dir();
    });
}

/// Load `.env` from the config directory (`~/.config/ghost/.env`).
///
/// Falls back to CWD-based `.env` (the `dotenvy` default) if the
/// config directory cannot be resolved.
pub(crate) fn load_dotenv_from_config_dir() {
    if let Ok(dir) = config_dir() {
        let env_path = dir.join(".env");
        if env_path.exists() {
            let _ = dotenvy::from_path(&env_path);
            return;
        }
    }
    // Fallback: CWD-based .env (original behaviour)
    let _ = dotenvy::dotenv();
}

/// Re-read `.env` for hot-reload, overwriting existing env vars.
fn reload_dotenv() {
    if let Ok(dir) = config_dir() {
        let env_path = dir.join(".env");
        if env_path.exists() {
            let _ = dotenvy::from_path_override(&env_path);
            return;
        }
    }
    let _ = dotenvy::dotenv_override();
}

#[tracing::instrument(skip_all)]
pub(crate) fn config_dir() -> Result<PathBuf, ConfigError> {
    if let Some(path) = env::var_os(CONFIG_DIR_ENV) {
        return Ok(PathBuf::from(path));
    }

    let home = env::var("HOME").map_err(|_| ConfigError::HomeDirUnavailable {
        path: "~/.config/ghost".to_string(),
    })?;
    Ok(PathBuf::from(home).join(".config").join("ghost"))
}

fn load_settings_from_path(path: &Path) -> Result<Settings, ConfigError> {
    if !path.exists() {
        return Ok(empty_settings());
    }

    let content = std::fs::read_to_string(path).map_err(|source| ConfigError::ReadFile {
        path: path.to_path_buf(),
        source,
    })?;

    toml::from_str::<Settings>(&content).map_err(|source| ConfigError::Parse {
        path: path.to_path_buf(),
        source,
    })
}

fn empty_settings() -> Settings {
    Settings {
        workspace: None,
        models: None,
        discord: None,
        embeddings: None,
        timing: None,
        compaction: None,
        web: None,
        docling: None,
        coding: None,
        debug: None,
    }
}

pub(crate) fn load_toml_value(path: &Path) -> Result<toml::Value, ConfigError> {
    if !path.exists() {
        return Ok(toml::Value::Table(toml::map::Map::new()));
    }

    let content = std::fs::read_to_string(path).map_err(|source| ConfigError::ReadFile {
        path: path.to_path_buf(),
        source,
    })?;

    toml::from_str::<toml::Value>(&content).map_err(|source| ConfigError::Parse {
        path: path.to_path_buf(),
        source,
    })
}

/// Accept `"single_value"` or `["a", "b"]` and always produce a `Vec<String>`.
fn deserialize_string_or_vec<'de, D>(deserializer: D) -> Result<Vec<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum StringOrVec {
        Single(String),
        Multiple(Vec<String>),
    }

    match StringOrVec::deserialize(deserializer)? {
        StringOrVec::Single(s) => Ok(vec![s]),
        StringOrVec::Multiple(v) => Ok(v),
    }
}

fn expand_tilde(input: &str) -> Result<PathBuf, ConfigError> {
    if input == "~" {
        return env::var("HOME")
            .map(PathBuf::from)
            .map_err(|_| ConfigError::HomeDirUnavailable {
                path: input.to_string(),
            });
    }

    if let Some(rest) = input.strip_prefix("~/") {
        let home = env::var("HOME").map_err(|_| ConfigError::HomeDirUnavailable {
            path: input.to_string(),
        })?;
        return Ok(PathBuf::from(home).join(rest));
    }

    Ok(PathBuf::from(input))
}

/// Create a minimal [`Config`] for unit tests that need a `ToolContext` but don't
/// exercise any real config behavior. The workspace is set to the given path.
pub fn test_config(workspace: &std::path::Path) -> Config {
    let mut aliases = BTreeMap::new();
    aliases.insert(
        "primary".to_string(),
        ModelConfig {
            provider: ProviderKind::OpenRouter,
            model: "test/model".to_string(),
            context_window: 200_000,
            headers: BTreeMap::new(),
            reasoning_effort: None,
            provider_routing: None,
            text_verbosity: None,
        },
    );
    Config {
        workspace: workspace.to_path_buf(),
        models: ModelsConfig {
            default: "primary".to_string(),
            default_chain: vec!["primary".to_string()],
            aliases,
        },
        discord: DiscordConfig {
            enabled: false,
            allowed_user_ids: Vec::new(),
        },
        embeddings: EmbeddingsConfig {
            url: "http://localhost:11434".to_string(),
            model: "test".to_string(),
            batch_size: 32,
            dimension: 1024,
        },
        timing: TimingConfig {
            reflection_idle_minutes: 10,
            scheduler_tick_seconds: 60,
        },
        compaction: CompactionConfig {
            threshold: 0.90,
            mask_preview_chars: 100,
            instructions: None,
        },
        web: WebConfig {
            search_max_results: 5,
            crawl4ai_url: None,
            browsers: vec![],
            search_provider: SearchProviderConfig::Brave,
        },
        docling: DoclingConfig {
            url: None,
            timeout: 600,
        },
        coding: CodingConfig { model: None },
        debug: DebugConfig {
            save_requests: false,
        },
        install_bundled_docs: false,
    }
}

#[cfg(test)]
mod reload_tests {
    use super::*;

    #[test]
    fn validate_reload_accepts_identical_config() {
        let workspace = std::path::Path::new("/tmp/test");
        let config = test_config(workspace);
        assert!(validate_reload(&config, &config).is_ok());
    }

    #[test]
    fn validate_reload_rejects_workspace_change() {
        let a = test_config(std::path::Path::new("/tmp/a"));
        let b = test_config(std::path::Path::new("/tmp/b"));
        let err = validate_reload(&a, &b).unwrap_err();
        assert!(matches!(err, ConfigError::ImmutableFieldChanged { .. }));
    }

    #[test]
    fn validate_reload_rejects_dimension_change() {
        let workspace = std::path::Path::new("/tmp/test");
        let mut a = test_config(workspace);
        let mut b = test_config(workspace);
        a.embeddings.dimension = 1024;
        b.embeddings.dimension = 768;
        let err = validate_reload(&a, &b).unwrap_err();
        assert!(matches!(err, ConfigError::ImmutableFieldChanged { .. }));
    }

    #[test]
    fn validate_reload_rejects_discord_enabled_change() {
        let workspace = std::path::Path::new("/tmp/test");
        let mut a = test_config(workspace);
        let mut b = test_config(workspace);
        a.discord.enabled = false;
        b.discord.enabled = true;
        let err = validate_reload(&a, &b).unwrap_err();
        assert!(matches!(err, ConfigError::ImmutableFieldChanged { .. }));
    }

    #[test]
    fn shared_config_current_returns_snapshot() {
        let config = test_config(std::path::Path::new("/tmp/test"));
        let (_tx, rx) = tokio::sync::watch::channel(Arc::new(config.clone()));
        let snapshot = rx.current();
        assert_eq!(snapshot.workspace, config.workspace);
    }

    #[test]
    fn string_or_list_from_single_string() {
        let toml = r#"value = "primary""#;

        #[derive(Deserialize)]
        struct T {
            value: StringOrList,
        }

        let t: T = toml::from_str(toml).unwrap();
        assert_eq!(t.value.as_slice(), &["primary"]);
    }

    #[test]
    fn string_or_list_from_list() {
        let toml = r#"value = ["primary", "fallback"]"#;

        #[derive(Deserialize)]
        struct T {
            value: StringOrList,
        }

        let t: T = toml::from_str(toml).unwrap();
        assert_eq!(t.value.as_slice(), &["primary", "fallback"]);
    }

    #[test]
    fn config_default_model_single_string() {
        let toml = r#"
        [models]
        default = "primary"

        [models.primary]
        provider = "openrouter"
        model = "anthropic/claude-sonnet-4"
        context_window = 200000
        "#;

        let settings: Settings = toml::from_str(toml).unwrap();
        let config = Config::from_settings(settings).unwrap();
        assert_eq!(config.models.default_chain, vec!["primary"]);
        assert_eq!(config.models.default, "primary");
    }

    #[test]
    fn config_default_model_chain() {
        let toml = r#"
        [models]
        default = ["primary", "fallback"]

        [models.primary]
        provider = "openrouter"
        model = "anthropic/claude-sonnet-4"
        context_window = 200000

        [models.fallback]
        provider = "openrouter"
        model = "google/gemini-flash"
        context_window = 128000
        "#;

        let settings: Settings = toml::from_str(toml).unwrap();
        let config = Config::from_settings(settings).unwrap();
        assert_eq!(config.models.default_chain, vec!["primary", "fallback"]);
        assert_eq!(config.models.default, "primary");
    }

    #[test]
    fn config_default_model_chain_unknown_alias_fails() {
        let toml = r#"
        [models]
        default = ["primary", "nonexistent"]

        [models.primary]
        provider = "openrouter"
        model = "anthropic/claude-sonnet-4"
        context_window = 200000
        "#;

        let settings: Settings = toml::from_str(toml).unwrap();
        assert!(Config::from_settings(settings).is_err());
    }
}
