// Anthropic OAuth credential reading and token refresh.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::providers::ProviderError;

/// 5-minute safety buffer before actual expiry (per pi-mono).
const EXPIRY_BUFFER_MS: u64 = 5 * 60 * 1000;

const TOKEN_URL: &str = "https://platform.claude.com/v1/oauth/token";
const CLIENT_ID: &str = "9d1c250a-e61b-44d9-88ed-5944d1962f5e";
const TOKEN_REFRESH_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);

#[derive(Debug, Clone)]
pub(crate) struct OAuthCredentials {
    pub access_token: String,
    pub refresh_token: String,
    /// Unix timestamp in milliseconds.
    pub expires_at: u64,
}

impl OAuthCredentials {
    /// Returns true if the token is expired or will expire within the
    /// 5-minute safety buffer.
    pub fn is_expired(&self) -> bool {
        let now_ms = chrono::Utc::now().timestamp_millis() as u64;
        now_ms + EXPIRY_BUFFER_MS >= self.expires_at
    }
}

/// Detect OAuth tokens by prefix (per pi-mono).
#[allow(
    dead_code,
    reason = "called only from tests; kept in production module to stay close to the token handling code it documents"
)]
pub(crate) fn is_oauth_token(token: &str) -> bool {
    token.contains("sk-ant-oat")
}

/// Default credentials file path.
pub(crate) fn default_credentials_path() -> Option<PathBuf> {
    dirs::home_dir().map(|h| h.join(".claude/.credentials.json"))
}

/// JSON structure of ~/.claude/.credentials.json
#[derive(Deserialize)]
struct CredentialsFile {
    #[serde(rename = "claudeAiOauth")]
    claude_ai_oauth: Option<ClaudeAiOauth>,
}

#[derive(Deserialize)]
struct ClaudeAiOauth {
    #[serde(rename = "accessToken")]
    access_token: String,
    #[serde(rename = "refreshToken")]
    refresh_token: String,
    #[serde(rename = "expiresAt")]
    expires_at: u64,
}

/// Read credentials from a specific path.
pub(crate) fn read_credentials_from_path(path: &Path) -> Result<OAuthCredentials, ProviderError> {
    let content = std::fs::read_to_string(path).map_err(|e| {
        ProviderError::Auth(format!(
            "failed to read Claude credentials at {}: {e}",
            path.display()
        ))
    })?;
    let file: CredentialsFile = serde_json::from_str(&content)
        .map_err(|e| ProviderError::Auth(format!("failed to parse Claude credentials: {e}")))?;
    let oauth = file.claude_ai_oauth.ok_or_else(|| {
        ProviderError::Auth("no claudeAiOauth section in credentials file".into())
    })?;
    Ok(OAuthCredentials {
        access_token: oauth.access_token,
        refresh_token: oauth.refresh_token,
        expires_at: oauth.expires_at,
    })
}

/// Read credentials from env var or default file path.
///
/// Returns `(credentials, path_if_file_based)` — path is `None` when env var is used.
pub(crate) fn load_credentials() -> Result<(OAuthCredentials, Option<PathBuf>), ProviderError> {
    // 1. Env var takes precedence (access token only, no refresh)
    if let Ok(token) = std::env::var("ANTHROPIC_OAUTH_TOKEN") {
        return Ok((
            OAuthCredentials {
                access_token: token,
                refresh_token: String::new(),
                expires_at: u64::MAX, // no expiry info — will fail on 401
            },
            None,
        ));
    }

    // 2. Read from Claude Code credentials file
    let path = default_credentials_path().ok_or_else(|| {
        ProviderError::Auth("cannot determine home directory for Claude credentials".into())
    })?;
    let creds = read_credentials_from_path(&path)?;
    Ok((creds, Some(path)))
}

/// Refresh token request body.
#[derive(Serialize)]
struct RefreshRequest {
    grant_type: String,
    client_id: String,
    refresh_token: String,
}

/// Refresh token response.
#[derive(Deserialize)]
struct RefreshResponse {
    access_token: String,
    refresh_token: String,
    expires_in: u64,
}

/// Refresh the OAuth token and write updated credentials back to disk.
///
/// Uses a file lock to avoid races with Claude Code or other Ghost processes.
pub(crate) async fn refresh_token(
    client: &reqwest::Client,
    creds: &OAuthCredentials,
    creds_path: &Path,
) -> Result<OAuthCredentials, ProviderError> {
    let body = RefreshRequest {
        grant_type: "refresh_token".into(),
        client_id: CLIENT_ID.into(),
        refresh_token: creds.refresh_token.clone(),
    };

    let response = client
        .post(TOKEN_URL)
        .json(&body)
        .timeout(TOKEN_REFRESH_TIMEOUT)
        .send()
        .await
        .map_err(|e| ProviderError::Auth(format!("token refresh request failed: {e}")))?;

    let status = response.status();
    let text = response
        .text()
        .await
        .map_err(|e| ProviderError::Auth(format!("failed to read refresh response: {e}")))?;

    if !status.is_success() {
        return Err(ProviderError::Auth(format!(
            "token refresh failed (HTTP {status}): {text}. Try opening Claude Code to re-authenticate."
        )));
    }

    let refresh_resp: RefreshResponse = serde_json::from_str(&text)
        .map_err(|e| ProviderError::Auth(format!("failed to parse refresh response: {e}")))?;

    let now_ms = chrono::Utc::now().timestamp_millis() as u64;
    let new_creds = OAuthCredentials {
        access_token: refresh_resp.access_token,
        refresh_token: refresh_resp.refresh_token,
        expires_at: now_ms + refresh_resp.expires_in * 1000 - EXPIRY_BUFFER_MS,
    };

    // Write back with file lock
    write_credentials(creds_path, &new_creds)?;

    Ok(new_creds)
}

/// Write updated credentials back to the credentials file with a file lock.
///
/// Lock is acquired BEFORE reading to avoid TOCTOU races with Claude Code.
fn write_credentials(path: &Path, creds: &OAuthCredentials) -> Result<(), ProviderError> {
    use fs2::FileExt;
    use std::io::{Read as _, Seek, SeekFrom, Write as _};

    // Open for read+write, acquire exclusive lock FIRST
    let mut file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(path)
        .map_err(|e| ProviderError::Auth(format!("failed to open credentials for writing: {e}")))?;
    file.lock_exclusive()
        .map_err(|e| ProviderError::Auth(format!("failed to lock credentials file: {e}")))?;

    // Read under lock to avoid races
    let mut content = String::new();
    file.read_to_string(&mut content).unwrap_or_default();
    let mut doc: serde_json::Value =
        serde_json::from_str(&content).unwrap_or(serde_json::json!({}));

    doc["claudeAiOauth"]["accessToken"] = serde_json::Value::String(creds.access_token.clone());
    doc["claudeAiOauth"]["refreshToken"] = serde_json::Value::String(creds.refresh_token.clone());
    doc["claudeAiOauth"]["expiresAt"] = serde_json::json!(creds.expires_at);

    let json = serde_json::to_string_pretty(&doc)
        .map_err(|e| ProviderError::Auth(format!("failed to serialize credentials: {e}")))?;

    // Truncate and write under the same lock
    file.set_len(0)
        .map_err(|e| ProviderError::Auth(format!("failed to truncate credentials: {e}")))?;
    file.seek(SeekFrom::Start(0))
        .map_err(|e| ProviderError::Auth(format!("failed to seek credentials: {e}")))?;
    file.write_all(json.as_bytes())
        .map_err(|e| ProviderError::Auth(format!("failed to write credentials: {e}")))?;

    file.unlock()
        .map_err(|e| ProviderError::Auth(format!("failed to unlock credentials file: {e}")))?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn parse_credentials_file() {
        let json = serde_json::json!({
            "claudeAiOauth": {
                "accessToken": "sk-ant-oat01-test",
                "refreshToken": "sk-ant-ort01-test",
                "expiresAt": 9999999999999_u64,
                "scopes": ["user:inference"],
                "subscriptionType": "max",
                "rateLimitTier": "default"
            }
        });
        let mut tmp = tempfile::NamedTempFile::new().unwrap();
        write!(tmp, "{json}").unwrap();

        let creds = read_credentials_from_path(tmp.path()).unwrap();
        assert_eq!(creds.access_token, "sk-ant-oat01-test");
        assert_eq!(creds.refresh_token, "sk-ant-ort01-test");
        assert!(creds.expires_at > 0);
    }

    #[test]
    fn is_expired_with_buffer() {
        let creds = OAuthCredentials {
            access_token: "tok".into(),
            refresh_token: "ref".into(),
            expires_at: chrono::Utc::now().timestamp_millis() as u64 + 60_000, // 1min from now
        };
        // 5-minute buffer means 1 minute remaining = expired
        assert!(creds.is_expired());
    }

    #[test]
    fn is_not_expired_far_future() {
        let creds = OAuthCredentials {
            access_token: "tok".into(),
            refresh_token: "ref".into(),
            expires_at: chrono::Utc::now().timestamp_millis() as u64 + 600_000, // 10min
        };
        assert!(!creds.is_expired());
    }

    #[test]
    fn is_oauth_token_detects_prefix() {
        assert!(is_oauth_token("sk-ant-oat01-abc123"));
        assert!(!is_oauth_token("sk-ant-api03-abc123"));
        assert!(!is_oauth_token("some-random-key"));
    }
}
