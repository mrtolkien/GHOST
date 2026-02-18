use std::path::Path;

use crate::config::{ConfigError, Settings, config_dir, load_from_dir, load_toml_value};

const CONFIG_FILE_NAME: &str = "config.toml";

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
    let _ = crate::config::Config::from_settings(settings)?;

    std::fs::write(&path, serialized).map_err(|source| ConfigError::WriteFile {
        path: path.clone(),
        source,
    })?;

    Ok(())
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
            "{key} must be set with an inline TOML object like \
             {{ provider = \"openrouter\", model = \"...\" }}"
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
