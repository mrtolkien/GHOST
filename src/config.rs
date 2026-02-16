use std::collections::BTreeMap;
use std::env;
use std::path::{Path, PathBuf};
use std::sync::Once;

use serde::{Deserialize, Serialize};
use thiserror::Error;

pub const CONFIG_DIR_ENV: &str = "GHOST_CONFIG_DIR";
const CONFIG_FILE_NAME: &str = "config.toml";
const DEFAULT_WORKSPACE: &str = "~/GHOST";
const DEFAULT_EMBEDDINGS_URL: &str = "http://127.0.0.1:11434";
const DEFAULT_EMBEDDINGS_MODEL: &str = "qwen3-embedding:8b";
const DEFAULT_BOOT_TEMPLATE: &str =
    "# BOOT\n\nYou are GHOST, a personal AI agent for your OPERATOR.\n";

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
    pub context_window: Option<u32>,
    #[serde(default)]
    pub headers: BTreeMap<String, String>,
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
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TimingSettings {
    pub heartbeat_idle_minutes: Option<u64>,
    pub heartbeat_check_seconds: Option<u64>,
    pub heartbeat_continue_minutes: Option<u64>,
    pub reflection_idle_minutes: Option<u64>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CompactionSettings {
    pub threshold: Option<f64>,
    pub keep_window: Option<usize>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Config {
    pub workspace: PathBuf,
    pub models: ModelsConfig,
    pub discord: DiscordConfig,
    pub embeddings: EmbeddingsConfig,
    pub timing: TimingConfig,
    pub compaction: CompactionConfig,
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
    pub context_window: Option<u32>,
    pub headers: BTreeMap<String, String>,
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
}

#[derive(Debug, Clone, Serialize)]
pub struct TimingConfig {
    pub heartbeat_idle_minutes: u64,
    pub heartbeat_check_seconds: u64,
    pub heartbeat_continue_minutes: u64,
    pub reflection_idle_minutes: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct CompactionConfig {
    pub threshold: f64,
    pub keep_window: usize,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Provider {
    OpenRouter,
    #[serde(alias = "kimi_code")]
    Kimi,
}

impl Provider {
    fn as_str(self) -> &'static str {
        match self {
            Self::OpenRouter => "openrouter",
            Self::Kimi => "kimi_code",
        }
    }
}

impl Config {
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
            },
            timing: TimingConfig {
                heartbeat_idle_minutes: settings
                    .timing
                    .as_ref()
                    .and_then(|t| t.heartbeat_idle_minutes)
                    .unwrap_or(5),
                heartbeat_check_seconds: settings
                    .timing
                    .as_ref()
                    .and_then(|t| t.heartbeat_check_seconds)
                    .unwrap_or(60),
                heartbeat_continue_minutes: settings
                    .timing
                    .as_ref()
                    .and_then(|t| t.heartbeat_continue_minutes)
                    .unwrap_or(30),
                reflection_idle_minutes: settings
                    .timing
                    .as_ref()
                    .and_then(|t| t.reflection_idle_minutes)
                    .unwrap_or(15),
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
                    .unwrap_or(20),
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

#[tracing::instrument(skip_all)]
pub fn get_resolved_value(key: &str) -> Result<String, ConfigError> {
    get_resolved_value_from_dir(&config_dir()?, key)
}

#[tracing::instrument(skip_all, fields(config_dir = %config_dir.display(), key = %key))]
pub fn get_resolved_value_from_dir(config_dir: &Path, key: &str) -> Result<String, ConfigError> {
    let config = load_from_dir(config_dir)?;
    let value = toml::Value::try_from(&config).map_err(|source| ConfigError::Serialize {
        path: config_dir.join(CONFIG_FILE_NAME),
        source,
    })?;

    let found = get_by_key_path(&value, key)?;
    Ok(render_value(found))
}

#[tracing::instrument(skip_all, fields(key = %key))]
pub fn set_value(key: &str, value: &str) -> Result<(), ConfigError> {
    set_value_in_dir(&config_dir()?, key, value)
}

#[tracing::instrument(skip_all, fields(config_dir = %config_dir.display(), key = %key))]
pub fn set_value_in_dir(config_dir: &Path, key: &str, value: &str) -> Result<(), ConfigError> {
    std::fs::create_dir_all(config_dir).map_err(|source| ConfigError::WriteFile {
        path: config_dir.to_path_buf(),
        source,
    })?;

    let path = config_dir.join(CONFIG_FILE_NAME);
    let mut root = load_toml_value(&path)?;
    let parsed_value = parse_cli_value(value);
    validate_model_object_assignment(key, &parsed_value)?;
    set_by_key_path(&mut root, key, parsed_value)?;

    let serialized = toml::to_string_pretty(&root).map_err(|source| ConfigError::Serialize {
        path: path.clone(),
        source,
    })?;

    let settings =
        toml::from_str::<Settings>(&serialized).map_err(|source| ConfigError::Parse {
            path: path.clone(),
            source,
        })?;
    let _ = Config::from_settings(settings)?;

    std::fs::write(&path, serialized).map_err(|source| ConfigError::WriteFile {
        path: path.clone(),
        source,
    })?;

    Ok(())
}

#[tracing::instrument(skip_all, fields(workspace = %config.workspace.display()))]
pub fn bootstrap_workspace(config: &Config) -> Result<(), ConfigError> {
    std::fs::create_dir_all(&config.workspace).map_err(|source| ConfigError::WriteFile {
        path: config.workspace.clone(),
        source,
    })?;

    for dir in ["jobs", "skills", ".web-cache", "knowledge"] {
        let path = config.workspace.join(dir);
        std::fs::create_dir_all(&path).map_err(|source| ConfigError::WriteFile { path, source })?;
    }

    create_file_if_missing(&config.workspace.join("BOOT.md"), DEFAULT_BOOT_TEMPLATE)?;
    create_file_if_missing(&config.workspace.join("SOUL.md"), "")?;
    create_file_if_missing(&config.workspace.join("OPERATOR.md"), "")?;

    Ok(())
}

fn create_file_if_missing(path: &Path, content: &str) -> Result<(), ConfigError> {
    if path.exists() {
        return Ok(());
    }
    std::fs::write(path, content).map_err(|source| ConfigError::WriteFile {
        path: path.to_path_buf(),
        source,
    })
}

fn load_dotenv() {
    DOTENV_INIT.call_once(|| {
        let _ = dotenvy::dotenv();
    });
}

#[tracing::instrument(skip_all)]
fn config_dir() -> Result<PathBuf, ConfigError> {
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
    }
}

fn load_toml_value(path: &Path) -> Result<toml::Value, ConfigError> {
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

fn parse_cli_value(raw: &str) -> toml::Value {
    if raw.trim().is_empty() {
        return toml::Value::String(String::new());
    }

    if let Ok(value) = raw.parse::<toml::Value>() {
        return value;
    }

    toml::Value::String(raw.to_string())
}

fn set_by_key_path(
    root: &mut toml::Value,
    key: &str,
    value: toml::Value,
) -> Result<(), ConfigError> {
    let segments = validate_key_path(key)?;
    let mut cursor = root;

    for segment in &segments[..segments.len() - 1] {
        let table = cursor
            .as_table_mut()
            .ok_or_else(|| ConfigError::InvalidKey {
                key: key.to_string(),
            })?;
        cursor = table
            .entry((*segment).to_string())
            .or_insert_with(|| toml::Value::Table(toml::map::Map::new()));
    }

    let table = cursor
        .as_table_mut()
        .ok_or_else(|| ConfigError::InvalidKey {
            key: key.to_string(),
        })?;
    table.insert(segments[segments.len() - 1].to_string(), value);
    Ok(())
}

fn get_by_key_path<'a>(root: &'a toml::Value, key: &str) -> Result<&'a toml::Value, ConfigError> {
    let segments = validate_key_path(key)?;
    let mut cursor = root;

    for segment in segments {
        let table = cursor.as_table().ok_or_else(|| ConfigError::InvalidKey {
            key: key.to_string(),
        })?;
        cursor = table.get(segment).ok_or_else(|| ConfigError::KeyNotFound {
            key: key.to_string(),
        })?;
    }

    Ok(cursor)
}

fn validate_key_path(key: &str) -> Result<Vec<&str>, ConfigError> {
    let segments = key
        .split('.')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>();

    if segments.is_empty() || key.starts_with('.') || key.ends_with('.') {
        return Err(ConfigError::InvalidKey {
            key: key.to_string(),
        });
    }

    Ok(segments)
}

fn validate_model_object_assignment(key: &str, value: &toml::Value) -> Result<(), ConfigError> {
    let segments = validate_key_path(key)?;
    if segments.len() != 2 || segments[0] != "models" || segments[1] == "default" {
        return Ok(());
    }

    let table = value.as_table().ok_or_else(|| ConfigError::InvalidKey {
        key: format!(
            "{key} must be set with an inline TOML object like {{ provider = \"openrouter\", model = \"...\" }}"
        ),
    })?;

    if !table.contains_key("provider") || !table.contains_key("model") {
        return Err(ConfigError::InvalidKey {
            key: format!("{key} requires both provider and model"),
        });
    }

    Ok(())
}

fn render_value(value: &toml::Value) -> String {
    match value {
        toml::Value::String(v) => v.clone(),
        _ => value.to_string(),
    }
}
