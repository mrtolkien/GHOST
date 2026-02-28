use std::collections::BTreeMap;
use std::env;
use std::path::{Path, PathBuf};
use std::sync::Once;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::providers::openai_compatible::ProviderRouting;
use crate::providers::types::ReasoningEffort;

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
    pub debug: Option<DebugSettings>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ModelsSettings {
    pub default: Option<String>,
    #[serde(flatten)]
    pub aliases: BTreeMap<String, ModelSettings>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ModelSettings {
    pub provider: Provider,
    pub model: String,
    pub context_window: u32,
    #[serde(default)]
    pub headers: BTreeMap<String, String>,
    pub reasoning_effort: Option<ReasoningEffort>,
    /// OpenRouter provider routing preferences (only/ignore/order).
    pub provider_routing: Option<ProviderRouting>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DiscordSettings {
    pub enabled: Option<bool>,
    pub allowed_user_id: Option<String>,
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
    pub keep_window: Option<usize>,
    pub mask_preview_chars: Option<usize>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct WebSettings {
    pub search_max_results: Option<usize>,
    pub crawl4ai_url: Option<String>,
    pub search: Option<SearchProviderSettings>,
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
    pub debug: DebugConfig,
}

#[derive(Debug, Clone, Serialize)]
pub struct ModelsConfig {
    pub default: String,
    #[serde(flatten)]
    pub aliases: BTreeMap<String, ModelConfig>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ModelConfig {
    pub provider: String,
    pub model: String,
    pub context_window: u32,
    pub headers: BTreeMap<String, String>,
    pub reasoning_effort: Option<ReasoningEffort>,
    /// OpenRouter provider routing preferences (only/ignore/order).
    pub provider_routing: Option<ProviderRouting>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DiscordConfig {
    pub enabled: bool,
    pub allowed_user_id: String,
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
    pub keep_window: usize,
    pub mask_preview_chars: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct WebConfig {
    pub search_max_results: usize,
    pub crawl4ai_url: Option<String>,
    pub search_provider: SearchProviderConfig,
}

#[derive(Debug, Clone, Serialize)]
pub enum SearchProviderConfig {
    Brave,
    Searxng { url: String },
}

#[derive(Debug, Clone, Serialize)]
pub struct DebugConfig {
    pub save_requests: bool,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Provider {
    OpenRouter,
    #[serde(alias = "kimi_code")]
    Kimi,
    #[serde(alias = "openai_oauth")]
    OpenAiOAuth,
}

impl Provider {
    fn as_str(self) -> &'static str {
        match self {
            Self::OpenRouter => "openrouter",
            Self::Kimi => "kimi_code",
            Self::OpenAiOAuth => "openai_oauth",
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
                        provider: model.provider.as_str().to_string(),
                        model: model.model,
                        context_window: model.context_window,
                        headers: model.headers,
                        reasoning_effort: model.reasoning_effort,
                        provider_routing: model.provider_routing,
                    },
                )
            })
            .collect::<BTreeMap<_, _>>();

        let default_model_alias = settings
            .models
            .as_ref()
            .and_then(|m| m.default.clone())
            .unwrap_or_else(|| {
                if resolved_aliases.len() == 1 {
                    resolved_aliases.keys().next().cloned().unwrap_or_default()
                } else {
                    String::new()
                }
            });

        if default_model_alias.is_empty() {
            return Err(ConfigError::MissingDefaultModelAlias);
        }

        if !resolved_aliases.contains_key(&default_model_alias) {
            return Err(ConfigError::UnknownDefaultModelAlias {
                alias: default_model_alias,
            });
        }

        Ok(Self {
            workspace,
            models: ModelsConfig {
                default: default_model_alias,
                aliases: resolved_aliases,
            },
            discord: DiscordConfig {
                enabled: settings
                    .discord
                    .as_ref()
                    .and_then(|d| d.enabled)
                    .unwrap_or(true),
                allowed_user_id: settings
                    .discord
                    .as_ref()
                    .and_then(|d| d.allowed_user_id.clone())
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
                    .unwrap_or(10),
            },
            compaction: CompactionConfig {
                threshold: settings
                    .compaction
                    .as_ref()
                    .and_then(|c| c.threshold)
                    .unwrap_or(0.85),
                keep_window: settings
                    .compaction
                    .as_ref()
                    .and_then(|c| c.keep_window)
                    .unwrap_or(12),
                mask_preview_chars: settings
                    .compaction
                    .as_ref()
                    .and_then(|c| c.mask_preview_chars)
                    .unwrap_or(100),
            },
            web: {
                let search_provider = match settings.web.as_ref().and_then(|w| w.search.as_ref()) {
                    Some(s) => match s.provider {
                        SearchProviderKind::Brave => SearchProviderConfig::Brave,
                        SearchProviderKind::Searxng => SearchProviderConfig::Searxng {
                            url: s.url.clone().ok_or(ConfigError::MissingSearxngUrl)?,
                        },
                    },
                    None => SearchProviderConfig::Brave,
                };
                WebConfig {
                    search_max_results: settings
                        .web
                        .as_ref()
                        .and_then(|w| w.search_max_results)
                        .unwrap_or(5),
                    crawl4ai_url: settings.web.as_ref().and_then(|w| w.crawl4ai_url.clone()),
                    search_provider,
                }
            },
            debug: DebugConfig {
                save_requests: settings
                    .debug
                    .as_ref()
                    .and_then(|d| d.save_requests)
                    .unwrap_or(false),
            },
        })
    }
}

#[tracing::instrument(skip_all)]
pub fn load() -> Result<Config, ConfigError> {
    load_dotenv();
    load_from_dir(&config_dir()?)
}

#[tracing::instrument(skip_all, fields(config_dir = %config_dir.display()))]
pub fn load_from_dir(config_dir: &Path) -> Result<Config, ConfigError> {
    let settings = load_settings_from_path(&config_dir.join(CONFIG_FILE_NAME))?;
    Config::from_settings(settings)
}

fn load_dotenv() {
    DOTENV_INIT.call_once(|| {
        let _ = dotenvy::dotenv();
    });
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

/// Create a minimal `Config` for unit tests that need a ToolContext but don't
/// exercise any real config behavior. The workspace is set to the given path.
pub fn test_config(workspace: &std::path::Path) -> Config {
    let mut aliases = BTreeMap::new();
    aliases.insert(
        "primary".to_string(),
        ModelConfig {
            provider: "openrouter".to_string(),
            model: "test/model".to_string(),
            context_window: 200_000,
            headers: BTreeMap::new(),
            reasoning_effort: None,
            provider_routing: None,
        },
    );
    Config {
        workspace: workspace.to_path_buf(),
        models: ModelsConfig {
            default: "primary".to_string(),
            aliases,
        },
        discord: DiscordConfig {
            enabled: false,
            allowed_user_id: String::new(),
        },
        embeddings: EmbeddingsConfig {
            url: "http://localhost:11434".to_string(),
            model: "test".to_string(),
            batch_size: 32,
            dimension: 1024,
        },
        timing: TimingConfig {
            reflection_idle_minutes: 10,
            scheduler_tick_seconds: 10,
        },
        compaction: CompactionConfig {
            threshold: 0.85,
            keep_window: 10,
            mask_preview_chars: 100,
        },
        web: WebConfig {
            search_max_results: 5,
            crawl4ai_url: None,
            search_provider: SearchProviderConfig::Brave,
        },
        debug: DebugConfig {
            save_requests: false,
        },
    }
}
