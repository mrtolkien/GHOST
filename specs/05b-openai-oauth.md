# 05b — OpenAI OAuth Provider

## Overview

An OpenAI-compatible provider that authenticates via OAuth instead of a static API key.
The user visits a URL, logs into their OpenAI account, and pastes a token back into the
CLI. The token is then stored locally and refreshed as needed.

This enables using OpenAI models without manually managing API keys — the user
authenticates with their OpenAI account directly.

## Authentication Flow

### Initial Setup (`ghost auth codex`)

1. Ghost opens or prints an OpenAI authorization URL
2. User visits the URL and logs in with their OpenAI account
3. User copies the authorization token from the browser
4. User pastes the token into the ghost CLI
5. Ghost exchanges the token for an access token + refresh token
6. Tokens are stored securely in the config directory

### Token Storage

```
~/.config/ghost/tokens/
└── openai.json
```

```json
{
  "access_token": "...",
  "refresh_token": "...",
  "expires_at": "2025-02-15T16:00:00Z"
}
```

File permissions should be `600` (owner read/write only).

### Token Refresh

Before each API call, check if the access token is expired or close to expiry (within 5
minutes). If so, use the refresh token to get a new access token. Update the stored
tokens.

If the refresh token is also expired, prompt the user to re-authenticate via
`ghost auth codex`.

## Implementation

```rust
pub struct OpenAiOAuthProvider {
    client: OpenAiCompatibleClient,
    token_store: TokenStore,
}

pub struct TokenStore {
    path: PathBuf,
    tokens: RwLock<Option<StoredTokens>>,
}

pub struct StoredTokens {
    pub access_token: String,
    pub refresh_token: String,
    pub expires_at: DateTime<Utc>,
}

impl OpenAiOAuthProvider {
    /// Get a valid access token, refreshing if needed.
    #[tracing::instrument(skip_all)]
    async fn get_token(&self) -> Result<String, ProviderError> {
        let tokens = self.token_store.tokens.read().await;
        if let Some(t) = tokens.as_ref() {
            if t.expires_at > Utc::now() + Duration::minutes(5) {
                return Ok(t.access_token.clone());
            }
        }
        drop(tokens);
        self.refresh_token().await
    }

    #[tracing::instrument(skip_all)]
    async fn refresh_token(&self) -> Result<String, ProviderError> {
        // POST to OpenAI token endpoint with refresh_token
        // Update stored tokens
        // Return new access_token
    }
}

#[async_trait]
impl Provider for OpenAiOAuthProvider {
    async fn chat(&self, mut request: ChatRequest) -> Result<ChatResponse, ProviderError> {
        let token = self.get_token().await?;
        // Set Authorization: Bearer {token} and delegate to OpenAI-compatible client
    }
}
```

## CLI Command

```
ghost auth codex     # Start OAuth flow, store tokens
ghost auth status    # Show which providers are authenticated
ghost auth revoke    # Delete stored tokens
```

### Auth Flow UX

```
$ ghost auth codex
Visit this URL to authorize Ghost with your OpenAI account:

  https://auth.openai.com/authorize?...

After logging in, paste the token here:
> [user pastes token]

Authenticated successfully. Token stored at ~/.config/ghost/tokens/openai.json
```

## Config

```toml
[models.openai]
provider = "openai_oauth"
model = "gpt-4o"
# No api_key_env needed — uses OAuth tokens
# base_url defaults to https://api.openai.com/v1
```

## Research Needed

The exact OAuth endpoints and flow need to be reverse-engineered or documented. Key
unknowns:

- Authorization URL format and required parameters
- Token exchange endpoint
- Refresh token endpoint and flow
- Token lifetime (access token + refresh token expiry)
- Required scopes/permissions

Look at how Codex CLI and Copilot CLI handle this — they use similar OAuth flows with
OpenAI.

## Observability

- Log token refresh events (not the tokens themselves!)
- Span on every `get_token()` call with `token_refreshed: bool` field
- Log auth failures with clear error messages (expired refresh token, network error,
  etc.)
- Never log token values

## Acceptance Criteria

- `ghost auth codex` completes the OAuth flow and stores tokens
- Provider automatically refreshes expired tokens before API calls
- Token file has restricted permissions (600)
- Provider works as a drop-in replacement for API key-based OpenAI
- Clear error message when refresh token expires (re-run `ghost auth codex`)
- `ghost auth status` shows authentication state
- Integration test (live-tests) validates the full flow
- `just ci` passes

## Prior Art

This is a **new feature** — no implementation exists in t-koma. However:

- https://github.com/badlogic/pi-mono/tree/main/packages/ai - this package supports
  OpenAI Codex login extremely well. Read their source for inspiration.
- `t-koma-gateway/src/providers/openai_compatible/client.rs` — The OpenAI-compatible
  client can be reused as the underlying HTTP layer. The OAuth provider wraps it with
  token management.
- Codex CLI's auth flow is a good reference for the OAuth endpoints and UX.
