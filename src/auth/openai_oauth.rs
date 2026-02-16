use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use chrono::{DateTime, Duration, Utc};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;

use crate::config::CONFIG_DIR_ENV;

const OPENAI_AUTH_AUTHORIZE_URL: &str = "https://auth.openai.com/oauth/authorize";
const OPENAI_AUTH_TOKEN_URL: &str = "https://auth.openai.com/oauth/token";
const OPENAI_CLIENT_ID: &str = "app_EMoamEEZ73f0CkXaXp7hrann";
const OPENAI_REDIRECT_URI: &str = "http://localhost:1455/auth/callback";
const OPENAI_SCOPE: &str = "openid profile email offline_access";
const TOKEN_FILE_NAME: &str = "openai.json";

#[derive(Debug, thiserror::Error)]
pub enum AuthError {
    #[error("home directory is unavailable; cannot resolve config directory")]
    HomeDirUnavailable,

    #[error("failed to create token directory {path}: {source}")]
    CreateTokenDir {
        path: PathBuf,
        #[source]
        source: io::Error,
    },

    #[error("failed to read token file {path}: {source}")]
    ReadTokenFile {
        path: PathBuf,
        #[source]
        source: io::Error,
    },

    #[error("failed to parse token file {path}: {source}")]
    ParseTokenFile {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },

    #[error("failed to write token file {path}: {source}")]
    WriteTokenFile {
        path: PathBuf,
        #[source]
        source: io::Error,
    },

    #[error("failed to serialize token data for {path}: {source}")]
    SerializeTokenFile {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },

    #[error("failed to set permissions on token file {path}: {source}")]
    SetPermissions {
        path: PathBuf,
        #[source]
        source: io::Error,
    },

    #[error("failed to delete token file {path}: {source}")]
    DeleteTokenFile {
        path: PathBuf,
        #[source]
        source: io::Error,
    },

    #[error("OAuth request failed: {0}")]
    Request(#[from] reqwest::Error),

    #[error("OAuth server rejected request: {0}")]
    OAuthServer(String),

    #[error("no OpenAI OAuth token is stored; run `ghost auth codex` first")]
    MissingStoredToken,

    #[error("failed to read input from terminal: {0}")]
    Input(#[from] io::Error),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredTokens {
    pub access_token: String,
    pub refresh_token: String,
    pub expires_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct AuthorizationRequest {
    pub url: String,
    pub code_verifier: String,
    pub state: String,
}

#[derive(Debug, Clone)]
pub struct TokenStore {
    path: PathBuf,
    tokens: Arc<RwLock<Option<StoredTokens>>>,
}

#[derive(Debug, Deserialize)]
struct TokenResponse {
    access_token: String,
    refresh_token: Option<String>,
    expires_in: i64,
}

#[derive(Debug, Deserialize)]
struct OAuthErrorResponse {
    error: Option<String>,
    error_description: Option<String>,
}

#[derive(Debug, Clone)]
pub struct OpenAiOAuthClient {
    client: reqwest::Client,
}

impl OpenAiOAuthClient {
    #[tracing::instrument(skip_all)]
    pub fn new() -> Result<Self, AuthError> {
        let client = reqwest::Client::builder().build()?;
        Ok(Self { client })
    }

    #[tracing::instrument(skip_all)]
    pub fn build_authorization_request(&self) -> AuthorizationRequest {
        let code_verifier = random_urlsafe(32);
        let state = random_urlsafe(32);
        let challenge = code_challenge(&code_verifier);
        let mut url = reqwest::Url::parse(OPENAI_AUTH_AUTHORIZE_URL).expect("valid authorize url");
        url.query_pairs_mut()
            .append_pair("response_type", "code")
            .append_pair("client_id", OPENAI_CLIENT_ID)
            .append_pair("redirect_uri", OPENAI_REDIRECT_URI)
            .append_pair("scope", OPENAI_SCOPE)
            .append_pair("code_challenge", &challenge)
            .append_pair("code_challenge_method", "S256")
            .append_pair("state", &state)
            .append_pair("id_token_add_organizations", "true")
            .append_pair("codex_cli_simplified_flow", "true")
            .append_pair("originator", "ghost");
        AuthorizationRequest {
            url: url.to_string(),
            code_verifier,
            state,
        }
    }

    #[tracing::instrument(skip_all)]
    pub async fn exchange_code(
        &self,
        code: &str,
        code_verifier: &str,
    ) -> Result<StoredTokens, AuthError> {
        let response = self
            .client
            .post(OPENAI_AUTH_TOKEN_URL)
            .form(&[
                ("grant_type", "authorization_code"),
                ("code", code),
                ("client_id", OPENAI_CLIENT_ID),
                ("redirect_uri", OPENAI_REDIRECT_URI),
                ("code_verifier", code_verifier),
            ])
            .send()
            .await?;

        parse_token_response(response).await
    }

    #[tracing::instrument(skip_all)]
    pub async fn refresh(&self, refresh_token: &str) -> Result<StoredTokens, AuthError> {
        let response = self
            .client
            .post(OPENAI_AUTH_TOKEN_URL)
            .form(&[
                ("grant_type", "refresh_token"),
                ("refresh_token", refresh_token),
                ("client_id", OPENAI_CLIENT_ID),
            ])
            .send()
            .await?;

        parse_token_response(response).await
    }
}

impl TokenStore {
    #[tracing::instrument(skip_all)]
    pub fn new(path: PathBuf) -> Self {
        Self {
            path,
            tokens: Arc::new(RwLock::new(None)),
        }
    }

    #[tracing::instrument(skip_all)]
    pub fn default_openai_path() -> Result<PathBuf, AuthError> {
        Ok(config_dir()?.join("tokens").join(TOKEN_FILE_NAME))
    }

    #[tracing::instrument(skip_all)]
    pub fn default_openai_store() -> Result<Self, AuthError> {
        Ok(Self::new(Self::default_openai_path()?))
    }

    #[tracing::instrument(skip_all)]
    pub fn path(&self) -> &Path {
        &self.path
    }

    #[tracing::instrument(skip_all)]
    pub async fn load(&self) -> Result<Option<StoredTokens>, AuthError> {
        if !self.path.exists() {
            *self.tokens.write().await = None;
            return Ok(None);
        }
        let raw =
            std::fs::read_to_string(&self.path).map_err(|source| AuthError::ReadTokenFile {
                path: self.path.clone(),
                source,
            })?;
        let parsed = serde_json::from_str::<StoredTokens>(&raw).map_err(|source| {
            AuthError::ParseTokenFile {
                path: self.path.clone(),
                source,
            }
        })?;
        *self.tokens.write().await = Some(parsed.clone());
        Ok(Some(parsed))
    }

    #[tracing::instrument(skip_all)]
    pub async fn save(&self, tokens: &StoredTokens) -> Result<(), AuthError> {
        let directory =
            self.path
                .parent()
                .map(Path::to_path_buf)
                .ok_or(AuthError::CreateTokenDir {
                    path: self.path.clone(),
                    source: io::Error::other("token file path has no parent directory"),
                })?;
        std::fs::create_dir_all(&directory).map_err(|source| AuthError::CreateTokenDir {
            path: directory.clone(),
            source,
        })?;

        let serialized = serde_json::to_string_pretty(tokens).map_err(|source| {
            AuthError::SerializeTokenFile {
                path: self.path.clone(),
                source,
            }
        })?;
        std::fs::write(&self.path, serialized).map_err(|source| AuthError::WriteTokenFile {
            path: self.path.clone(),
            source,
        })?;
        set_token_permissions(&self.path)?;

        *self.tokens.write().await = Some(tokens.clone());
        Ok(())
    }

    #[tracing::instrument(skip_all)]
    pub async fn revoke(&self) -> Result<(), AuthError> {
        if self.path.exists() {
            std::fs::remove_file(&self.path).map_err(|source| AuthError::DeleteTokenFile {
                path: self.path.clone(),
                source,
            })?;
        }
        *self.tokens.write().await = None;
        Ok(())
    }

    #[tracing::instrument(skip_all)]
    pub async fn current(&self) -> Result<Option<StoredTokens>, AuthError> {
        if let Some(tokens) = self.tokens.read().await.clone() {
            return Ok(Some(tokens));
        }
        self.load().await
    }

    #[tracing::instrument(skip_all)]
    pub async fn get_valid_access_token(
        &self,
        oauth_client: &OpenAiOAuthClient,
    ) -> Result<String, AuthError> {
        let tokens = self.current().await?.ok_or(AuthError::MissingStoredToken)?;
        if tokens.expires_at > Utc::now() + Duration::minutes(5) {
            return Ok(tokens.access_token);
        }

        logfire::info!("oauth token expiring soon, refreshing");
        let refreshed = oauth_client.refresh(&tokens.refresh_token).await?;
        self.save(&refreshed).await?;
        Ok(refreshed.access_token)
    }
}

#[tracing::instrument(skip_all)]
pub async fn run_codex_auth_flow() -> Result<PathBuf, AuthError> {
    let oauth_client = OpenAiOAuthClient::new()?;
    let token_store = TokenStore::default_openai_store()?;

    let auth_request = oauth_client.build_authorization_request();
    println!("Visit this URL to authorize Ghost with your OpenAI account:\n");
    println!("  {}", auth_request.url);
    println!("\nAfter logging in, paste the authorization code or full redirect URL here:");
    print!("> ");
    io::stdout().flush()?;

    let mut input = String::new();
    io::stdin().read_line(&mut input)?;
    let parsed = parse_authorization_input(input.trim());
    if let Some(state) = parsed.state
        && state != auth_request.state
    {
        return Err(AuthError::OAuthServer(
            "state mismatch in pasted authorization input".to_string(),
        ));
    }
    let code = parsed
        .code
        .ok_or_else(|| AuthError::OAuthServer("missing authorization code".to_string()))?;
    let tokens = oauth_client
        .exchange_code(&code, &auth_request.code_verifier)
        .await?;

    token_store.save(&tokens).await?;
    Ok(token_store.path().to_path_buf())
}

#[tracing::instrument(skip_all)]
pub async fn auth_status() -> Result<Option<StoredTokens>, AuthError> {
    TokenStore::default_openai_store()?.current().await
}

#[tracing::instrument(skip_all)]
pub async fn revoke_openai_tokens() -> Result<(), AuthError> {
    TokenStore::default_openai_store()?.revoke().await
}

async fn parse_token_response(response: reqwest::Response) -> Result<StoredTokens, AuthError> {
    let status = response.status();
    let body = response.text().await?;

    if !status.is_success() {
        if let Ok(error) = serde_json::from_str::<OAuthErrorResponse>(&body) {
            let message = error
                .error_description
                .or(error.error)
                .unwrap_or_else(|| body.clone());
            return Err(AuthError::OAuthServer(message));
        }
        return Err(AuthError::OAuthServer(body));
    }

    let token_response = serde_json::from_str::<TokenResponse>(&body).map_err(|source| {
        AuthError::ParseTokenFile {
            path: PathBuf::from("<oauth-response>"),
            source,
        }
    })?;

    let refresh_token = token_response
        .refresh_token
        .ok_or_else(|| AuthError::OAuthServer("missing refresh_token in response".to_string()))?;
    Ok(StoredTokens {
        access_token: token_response.access_token,
        refresh_token,
        expires_at: Utc::now() + Duration::seconds(token_response.expires_in),
    })
}

fn set_token_permissions(path: &Path) -> Result<(), AuthError> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let permissions = std::fs::Permissions::from_mode(0o600);
        std::fs::set_permissions(path, permissions).map_err(|source| {
            AuthError::SetPermissions {
                path: path.to_path_buf(),
                source,
            }
        })?;
    }
    Ok(())
}

fn config_dir() -> Result<PathBuf, AuthError> {
    if let Some(path) = std::env::var_os(CONFIG_DIR_ENV) {
        return Ok(PathBuf::from(path));
    }
    let home = std::env::var("HOME").map_err(|_| AuthError::HomeDirUnavailable)?;
    Ok(PathBuf::from(home).join(".config").join("ghost"))
}

fn random_urlsafe(bytes_len: usize) -> String {
    let mut bytes = vec![0_u8; bytes_len];
    rand::rng().fill_bytes(&mut bytes);
    URL_SAFE_NO_PAD.encode(bytes)
}

fn code_challenge(code_verifier: &str) -> String {
    use sha2::{Digest, Sha256};
    URL_SAFE_NO_PAD.encode(Sha256::digest(code_verifier.as_bytes()))
}

#[derive(Debug)]
struct ParsedAuthorizationInput {
    code: Option<String>,
    state: Option<String>,
}

fn parse_authorization_input(input: &str) -> ParsedAuthorizationInput {
    let value = input.trim();
    if value.is_empty() {
        return ParsedAuthorizationInput {
            code: None,
            state: None,
        };
    }

    if let Ok(url) = reqwest::Url::parse(value) {
        return ParsedAuthorizationInput {
            code: url
                .query_pairs()
                .find_map(|(key, val)| (key == "code").then_some(val.into_owned())),
            state: url
                .query_pairs()
                .find_map(|(key, val)| (key == "state").then_some(val.into_owned())),
        };
    }

    if value.contains('#') {
        let mut parts = value.splitn(2, '#');
        return ParsedAuthorizationInput {
            code: parts.next().map(ToString::to_string),
            state: parts.next().map(ToString::to_string),
        };
    }

    if value.contains("code=")
        && let Ok(url) = reqwest::Url::parse(&format!("http://localhost?{value}"))
    {
        return ParsedAuthorizationInput {
            code: url
                .query_pairs()
                .find_map(|(key, val)| (key == "code").then_some(val.into_owned())),
            state: url
                .query_pairs()
                .find_map(|(key, val)| (key == "state").then_some(val.into_owned())),
        };
    }

    ParsedAuthorizationInput {
        code: Some(value.to_string()),
        state: None,
    }
}

#[cfg(test)]
mod tests {
    use tempfile::TempDir;

    use super::*;

    #[tokio::test]
    async fn token_store_round_trip() {
        let temp = TempDir::new().expect("tempdir");
        let store = TokenStore::new(temp.path().join("tokens").join("openai.json"));
        let tokens = StoredTokens {
            access_token: "a".to_string(),
            refresh_token: "r".to_string(),
            expires_at: Utc::now() + Duration::hours(1),
        };
        store.save(&tokens).await.expect("save");
        let loaded = store.load().await.expect("load").expect("tokens");
        assert_eq!(loaded.access_token, "a");
        assert_eq!(loaded.refresh_token, "r");
    }

    #[test]
    fn authorization_request_contains_pkce_fields() {
        let client = OpenAiOAuthClient::new().expect("oauth client");
        let auth = client.build_authorization_request();
        assert!(auth.url.contains("code_challenge="));
        assert!(auth.url.contains("code_challenge_method=S256"));
        assert!(auth.url.contains("codex_cli_simplified_flow=true"));
        assert!(!auth.url.contains("scope=openid profile"));
        assert!(!auth.code_verifier.trim().is_empty());
        assert!(!auth.state.trim().is_empty());
    }

    #[test]
    fn parses_authorization_input_from_redirect_url() {
        let parsed =
            parse_authorization_input("http://localhost:1455/auth/callback?code=abc123&state=xyz");
        assert_eq!(parsed.code.as_deref(), Some("abc123"));
        assert_eq!(parsed.state.as_deref(), Some("xyz"));
    }
}
