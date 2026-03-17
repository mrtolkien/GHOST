# Anthropic OAuth Provider Implementation Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if
> subagents available) or superpowers:executing-plans to implement this plan. Steps use
> checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add the Anthropic Messages API as a provider using Claude Code OAuth
credentials, mirroring pi-mono's implementation.

**Architecture:** Standalone provider module at `src/providers/anthropic/` with
submodules for credentials (read/refresh/lock), message conversion (Ghost → Anthropic
format), SSE streaming, and tool name translation. Integrates with existing `Provider`
trait, circuit breaker, and debug infrastructure.

**Tech Stack:** reqwest (existing), serde/serde_json (existing), fs2 (new — file
locking), chrono (existing — timestamp math)

**Spec:** `backlog/tasks/3-invisible-improvements/4-anthropic-provider.md`

---

## File Structure

```
src/providers/anthropic/
├── mod.rs              — AnthropicProvider struct, Provider trait impl, request dispatch
├── credentials.rs      — Read ~/.claude/.credentials.json, token refresh, file locking
├── messages.rs         — Ghost ChatRequest → Anthropic Messages API request body
├── streaming.rs        — Parse SSE stream into ChatResponse
└── tool_names.rs       — Claude Code canonical tool name bidirectional mapping
```

Also modified:

- `src/providers/mod.rs` — declare `pub mod anthropic`
- `src/providers/types.rs` — add `"anthropic"` arm to `provider_for_alias()`
- `Cargo.toml` — add `fs2` dependency

**Note on shared SSE code:** The Codex and Anthropic SSE parsers handle entirely
different event types and accumulation patterns — the only shared bit is line splitting
(`\n\n`, `data:` extraction), which is ~5 lines. Not worth extracting. Each provider
keeps its own parser.

---

### Task 1: Add `fs2` dependency and module skeleton

**Files:**

- Modify: `Cargo.toml`
- Create: `src/providers/anthropic/mod.rs`
- Create: `src/providers/anthropic/credentials.rs`
- Create: `src/providers/anthropic/messages.rs`
- Create: `src/providers/anthropic/streaming.rs`
- Create: `src/providers/anthropic/tool_names.rs`
- Modify: `src/providers/mod.rs`

- [ ] **Step 1: Add `fs2` to Cargo.toml**

```toml
fs2 = "0.4"
```

Add after the `futures` line.

- [ ] **Step 2: Create empty module files**

`src/providers/anthropic/mod.rs`:

```rust
mod credentials;
mod messages;
mod streaming;
mod tool_names;
```

`src/providers/anthropic/credentials.rs`:

```rust
// Anthropic OAuth credential reading and token refresh.
```

`src/providers/anthropic/messages.rs`:

```rust
// Ghost ChatRequest → Anthropic Messages API request format.
```

`src/providers/anthropic/streaming.rs`:

```rust
// Anthropic Messages API SSE stream parsing.
```

`src/providers/anthropic/tool_names.rs`:

```rust
// Claude Code canonical tool name translation.
```

- [ ] **Step 3: Register module in providers/mod.rs**

Add to `src/providers/mod.rs` after the `circuit_breaker` line:

```rust
pub mod anthropic;
```

- [ ] **Step 4: Verify it compiles**

Run: `cargo check` Expected: compiles with no errors (empty modules are fine)

- [ ] **Step 5: Commit**

```bash
git add Cargo.toml Cargo.lock src/providers/anthropic/ src/providers/mod.rs
git commit -m "chore: scaffold anthropic provider module and add fs2 dependency"
```

---

### Task 2: Tool name translation (`tool_names.rs`)

No dependencies on other new modules — can be built and tested in isolation.

**Files:**

- Create: `src/providers/anthropic/tool_names.rs`

- [ ] **Step 1: Write tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn to_claude_code_name_case_insensitive() {
        assert_eq!(to_claude_code_name("read"), "Read");
        assert_eq!(to_claude_code_name("BASH"), "Bash");
        assert_eq!(to_claude_code_name("webfetch"), "WebFetch");
        assert_eq!(to_claude_code_name("WebSearch"), "WebSearch");
    }

    #[test]
    fn to_claude_code_name_passthrough_unknown() {
        assert_eq!(to_claude_code_name("my_custom_tool"), "my_custom_tool");
    }

    #[test]
    fn from_claude_code_name_reverses() {
        let ghost_tools = &["file_read", "shell", "search"];
        // No match — returns as-is
        assert_eq!(from_claude_code_name("Read", ghost_tools), "Read");
    }

    #[test]
    fn from_claude_code_name_finds_original() {
        // Ghost has a tool named "read_file" — but canonical is "Read"
        // Only matches if ghost tool lowercases to same as canonical lowercase
        let ghost_tools = &["read", "bash", "grep"];
        assert_eq!(from_claude_code_name("Read", ghost_tools), "read");
        assert_eq!(from_claude_code_name("Bash", ghost_tools), "bash");
    }

    #[test]
    fn normalize_tool_call_id_strips_and_truncates() {
        assert_eq!(normalize_tool_call_id("abc-123_def"), "abc-123_def");
        assert_eq!(normalize_tool_call_id("a|b|c"), "a_b_c");
        let long = "a".repeat(100);
        assert_eq!(normalize_tool_call_id(&long).len(), 64);
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --lib providers::anthropic::tool_names -- --nocapture` Expected:
compilation errors (functions not defined)

- [ ] **Step 3: Implement**

```rust
/// Claude Code canonical tool names. Case-insensitive lookup maps Ghost
/// tool names to these exact strings before sending to the Anthropic API
/// via OAuth (stealth mode). Per pi-mono.
const CANONICAL_NAMES: &[&str] = &[
    "Read",
    "Write",
    "Edit",
    "Bash",
    "Grep",
    "Glob",
    "AskUserQuestion",
    "EnterPlanMode",
    "ExitPlanMode",
    "KillShell",
    "NotebookEdit",
    "Skill",
    "Task",
    "TaskOutput",
    "TodoWrite",
    "WebFetch",
    "WebSearch",
];

/// Map a Ghost tool name to its Claude Code canonical form.
/// Case-insensitive match. Unknown names pass through unchanged.
pub(crate) fn to_claude_code_name(name: &str) -> String {
    let lower = name.to_lowercase();
    for &canonical in CANONICAL_NAMES {
        if canonical.to_lowercase() == lower {
            return canonical.to_string();
        }
    }
    name.to_string()
}

/// Map a Claude Code canonical name back to the original Ghost tool name.
/// Searches `ghost_tool_names` for a case-insensitive match against the
/// canonical name. Falls back to returning `canonical` as-is.
pub(crate) fn from_claude_code_name(canonical: &str, ghost_tool_names: &[&str]) -> String {
    let lower = canonical.to_lowercase();
    for &ghost_name in ghost_tool_names {
        if ghost_name.to_lowercase() == lower {
            return ghost_name.to_string();
        }
    }
    canonical.to_string()
}

/// Normalize a tool call ID for Anthropic compatibility.
/// Strips non-`[a-zA-Z0-9_-]` characters (replaces with `_`) and
/// truncates to 64 chars. Per pi-mono.
pub(crate) fn normalize_tool_call_id(id: &str) -> String {
    let normalized: String = id
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect();
    if normalized.len() > 64 {
        normalized[..64].to_string()
    } else {
        normalized
    }
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --lib providers::anthropic::tool_names -- --nocapture` Expected: all
pass

- [ ] **Step 5: Commit**

```bash
git add src/providers/anthropic/tool_names.rs
git commit -m "feat: add Claude Code tool name translation for Anthropic provider"
```

---

### Task 3: Credential reading and token refresh (`credentials.rs`)

**Files:**

- Create: `src/providers/anthropic/credentials.rs`

- [ ] **Step 1: Write tests for credential parsing**

```rust
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
        write!(tmp, "{}", json).unwrap();

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
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --lib providers::anthropic::credentials -- --nocapture` Expected:
compilation errors

- [ ] **Step 3: Implement credential types and reading**

```rust
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::providers::ProviderError;

/// 5-minute safety buffer before actual expiry (per pi-mono).
const EXPIRY_BUFFER_MS: u64 = 5 * 60 * 1000;

const TOKEN_URL: &str = "https://platform.claude.com/v1/oauth/token";
const CLIENT_ID: &str = "9d1c250a-e61b-44d9-88ed-5944d1962f5e";

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
    let file: CredentialsFile = serde_json::from_str(&content).map_err(|e| {
        ProviderError::Auth(format!("failed to parse Claude credentials: {e}"))
    })?;
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
/// Returns (credentials, path_if_file_based) — path is None when env var is used.
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
        ProviderError::Auth(
            "cannot determine home directory for Claude credentials".into(),
        )
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
        .timeout(std::time::Duration::from_secs(30))
        .send()
        .await
        .map_err(|e| ProviderError::Auth(format!("token refresh request failed: {e}")))?;

    let status = response.status();
    let text = response.text().await.map_err(|e| {
        ProviderError::Auth(format!("failed to read refresh response: {e}"))
    })?;

    if !status.is_success() {
        return Err(ProviderError::Auth(format!(
            "token refresh failed (HTTP {status}): {text}. Try opening Claude Code to re-authenticate."
        )));
    }

    let refresh_resp: RefreshResponse = serde_json::from_str(&text).map_err(|e| {
        ProviderError::Auth(format!("failed to parse refresh response: {e}"))
    })?;

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

    let json = serde_json::to_string_pretty(&doc).map_err(|e| {
        ProviderError::Auth(format!("failed to serialize credentials: {e}"))
    })?;

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
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --lib providers::anthropic::credentials -- --nocapture` Expected: all
pass

- [ ] **Step 5: Commit**

```bash
git add src/providers/anthropic/credentials.rs
git commit -m "feat: anthropic credential reading and OAuth token refresh"
```

---

### Task 4: Message conversion (`messages.rs`)

Converts Ghost's `ChatRequest` into the Anthropic Messages API JSON body. This is the
most complex module — handles system prompt prepend, tool name translation, cache
control, thinking config, surrogate sanitization, orphaned tool calls, consecutive tool
result batching, and cross-model thinking block handling.

**Files:**

- Create: `src/providers/anthropic/messages.rs`

- [ ] **Step 1: Write tests for message conversion**

Focus on the core behaviors. Each test targets one specific conversion rule.

```rust
#[cfg(test)]
mod tests {
    use serde_json::json;
    use crate::providers::types::*;
    use super::*;

    fn simple_request(messages: Vec<ChatMessage>) -> ChatRequest {
        ChatRequest {
            model: "claude-sonnet-4-6-20250514".into(),
            messages,
            tools: None,
            max_tokens: Some(4096),
            temperature: Some(0.7),
            system: Some("You are helpful.".into()),
            reasoning_effort: None,
            cache_key: String::new(),
            turn_state: None,
            debug_context: None,
        }
    }

    #[test]
    fn system_prompt_prepends_preamble_with_cache_control() {
        let req = simple_request(vec![ChatMessage {
            role: Role::User,
            content: vec![ContentBlock::Text { text: "hi".into() }],
        }]);
        let body = build_request_body(&req, &[]).unwrap();
        let system = body["system"].as_array().unwrap();
        assert_eq!(system.len(), 1);
        let text = system[0]["text"].as_str().unwrap();
        assert!(text.starts_with("You are Claude Code"));
        assert!(text.contains("You are helpful."));
        assert_eq!(system[0]["cache_control"]["type"], "ephemeral");
    }

    #[test]
    fn tool_names_translated_in_definitions() {
        let req = ChatRequest {
            tools: Some(vec![ToolDefinition {
                name: "read".into(),
                description: "Read a file".into(),
                input_schema: json!({"type": "object"}),
            }]),
            ..simple_request(vec![ChatMessage {
                role: Role::User,
                content: vec![ContentBlock::Text { text: "hi".into() }],
            }])
        };
        let body = build_request_body(&req, &["read"]).unwrap();
        assert_eq!(body["tools"][0]["name"], "Read");
    }

    #[test]
    fn tool_use_in_history_gets_translated_name_and_normalized_id() {
        let req = simple_request(vec![
            ChatMessage {
                role: Role::User,
                content: vec![ContentBlock::Text { text: "hi".into() }],
            },
            ChatMessage {
                role: Role::Assistant,
                content: vec![ContentBlock::ToolUse {
                    id: "a|b|c".into(),
                    name: "read".into(),
                    input: json!({}),
                }],
            },
            ChatMessage {
                role: Role::User,
                content: vec![ContentBlock::ToolResult {
                    tool_use_id: "a|b|c".into(),
                    content: "file contents".into(),
                    is_error: false,
                }],
            },
        ]);
        let body = build_request_body(&req, &["read"]).unwrap();
        let messages = body["messages"].as_array().unwrap();
        // Assistant message tool_use
        let tool_use = &messages[1]["content"][0];
        assert_eq!(tool_use["name"], "Read");
        assert_eq!(tool_use["id"], "a_b_c");
        // User tool_result
        let tool_result = &messages[2]["content"][0];
        assert_eq!(tool_result["tool_use_id"], "a_b_c");
    }

    #[test]
    fn consecutive_tool_results_batched() {
        let req = simple_request(vec![
            ChatMessage {
                role: Role::User,
                content: vec![ContentBlock::Text { text: "hi".into() }],
            },
            ChatMessage {
                role: Role::Assistant,
                content: vec![
                    ContentBlock::ToolUse { id: "1".into(), name: "a".into(), input: json!({}) },
                    ContentBlock::ToolUse { id: "2".into(), name: "b".into(), input: json!({}) },
                ],
            },
            ChatMessage {
                role: Role::User,
                content: vec![ContentBlock::ToolResult {
                    tool_use_id: "1".into(), content: "r1".into(), is_error: false,
                }],
            },
            ChatMessage {
                role: Role::User,
                content: vec![ContentBlock::ToolResult {
                    tool_use_id: "2".into(), content: "r2".into(), is_error: false,
                }],
            },
        ]);
        let body = build_request_body(&req, &[]).unwrap();
        let messages = body["messages"].as_array().unwrap();
        // Two consecutive tool result messages should become ONE user message
        // with two tool_result content blocks
        let last_user = messages.last().unwrap();
        assert_eq!(last_user["role"], "user");
        let content = last_user["content"].as_array().unwrap();
        assert_eq!(content.len(), 2);
        assert_eq!(content[0]["type"], "tool_result");
        assert_eq!(content[1]["type"], "tool_result");
    }

    #[test]
    fn orphaned_tool_calls_get_synthetic_error_results() {
        let req = simple_request(vec![
            ChatMessage {
                role: Role::User,
                content: vec![ContentBlock::Text { text: "hi".into() }],
            },
            ChatMessage {
                role: Role::Assistant,
                content: vec![ContentBlock::ToolUse {
                    id: "orphan".into(),
                    name: "bash".into(),
                    input: json!({}),
                }],
            },
            // No tool result follows!
            ChatMessage {
                role: Role::User,
                content: vec![ContentBlock::Text { text: "continue".into() }],
            },
        ]);
        let body = build_request_body(&req, &[]).unwrap();
        let messages = body["messages"].as_array().unwrap();
        // After the assistant message, there should be a synthetic tool_result
        // before the next user message
        let synthetic = &messages[2];
        assert_eq!(synthetic["role"], "user");
        let content = synthetic["content"].as_array().unwrap();
        assert_eq!(content[0]["type"], "tool_result");
        assert_eq!(content[0]["is_error"], true);
    }

    #[test]
    fn temperature_omitted_when_thinking_enabled() {
        let req = ChatRequest {
            temperature: Some(0.7),
            reasoning_effort: Some(ReasoningEffort::High),
            ..simple_request(vec![ChatMessage {
                role: Role::User,
                content: vec![ContentBlock::Text { text: "hi".into() }],
            }])
        };
        let body = build_request_body(&req, &[]).unwrap();
        assert!(body.get("temperature").is_none());
        assert!(body.get("thinking").is_some());
    }

    #[test]
    fn adaptive_thinking_for_new_models() {
        let req = ChatRequest {
            model: "claude-opus-4-6-20250514".into(),
            reasoning_effort: Some(ReasoningEffort::High),
            ..simple_request(vec![ChatMessage {
                role: Role::User,
                content: vec![ContentBlock::Text { text: "hi".into() }],
            }])
        };
        let body = build_request_body(&req, &[]).unwrap();
        assert_eq!(body["thinking"]["type"], "adaptive");
        assert_eq!(body["output_config"]["effort"], "high");
    }

    #[test]
    fn budget_thinking_for_older_models() {
        let req = ChatRequest {
            model: "claude-3-5-sonnet-20241022".into(),
            reasoning_effort: Some(ReasoningEffort::High),
            ..simple_request(vec![ChatMessage {
                role: Role::User,
                content: vec![ContentBlock::Text { text: "hi".into() }],
            }])
        };
        let body = build_request_body(&req, &[]).unwrap();
        assert_eq!(body["thinking"]["type"], "enabled");
        assert!(body["thinking"]["budget_tokens"].as_u64().unwrap() > 0);
    }

    #[test]
    fn cache_control_on_last_user_message() {
        let req = simple_request(vec![
            ChatMessage {
                role: Role::User,
                content: vec![ContentBlock::Text { text: "first".into() }],
            },
            ChatMessage {
                role: Role::Assistant,
                content: vec![ContentBlock::Text { text: "reply".into() }],
            },
            ChatMessage {
                role: Role::User,
                content: vec![ContentBlock::Text { text: "second".into() }],
            },
        ]);
        let body = build_request_body(&req, &[]).unwrap();
        let messages = body["messages"].as_array().unwrap();
        let last_user = &messages[2];
        let last_block = last_user["content"].as_array().unwrap().last().unwrap();
        assert_eq!(last_block["cache_control"]["type"], "ephemeral");
        // First user message should NOT have cache_control
        let first_block = &messages[0]["content"].as_array().unwrap()[0];
        assert!(first_block.get("cache_control").is_none());
    }

    #[test]
    fn sanitize_surrogates_strips_unpaired() {
        // Rust strings can't contain surrogates directly. Test via JSON
        // deserialization which can produce strings with escaped surrogates.
        // In practice, surrogates arrive from user content via JSON.
        // Test the regex/char filter on the escaped representation.
        let input = "hello\\ud800world"; // escaped surrogate in raw text
        let sanitized = sanitize_surrogates(input);
        // The function should strip \uD800-\uDFFF escape sequences
        assert!(!sanitized.contains("\\ud800"));
        assert!(sanitized.contains("hello"));
        assert!(sanitized.contains("world"));
    }

    #[test]
    fn image_only_message_gets_placeholder_text() {
        let req = simple_request(vec![ChatMessage {
            role: Role::User,
            content: vec![ContentBlock::Image {
                path: "/tmp/test.png".into(),
                mime_type: "image/png".into(),
                filename: "test.png".into(),
            }],
        }]);
        let body = build_request_body(&req, &[]).unwrap();
        let messages = body["messages"].as_array().unwrap();
        let content = messages[0]["content"].as_array().unwrap();
        // Should have a text block prepended before the image
        assert!(content.iter().any(|b| b["type"] == "text"));
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --lib providers::anthropic::messages -- --nocapture` Expected:
compilation errors

- [ ] **Step 3: Implement `build_request_body`**

This is the core function. It takes a `ChatRequest` and a list of Ghost tool names (for
reverse translation) and returns a `serde_json::Value` representing the Anthropic
Messages API request body.

Key behaviors:

- Prepend Claude Code preamble to system prompt
- System prompt as array with `cache_control: {type: "ephemeral"}`
- Convert messages: user/assistant roles, content blocks
- Translate tool names via `to_claude_code_name()`
- Normalize tool call IDs via `normalize_tool_call_id()`
- Batch consecutive tool results into single user messages
- Insert synthetic error results for orphaned tool calls
- Handle thinking/redacted_thinking blocks in history (cross-model: convert or drop)
- Skip error/aborted assistant messages
- `cache_control` on last content block of last user message
- Thinking config: adaptive for new models, budget for old
- Omit temperature when thinking is enabled
- Sanitize surrogates in all text
- Image-only messages: prepend placeholder text
- Drop empty text blocks
- `metadata.user_id` if available (from debug_context session_id for now)
- `stream: true` always

The implementation should be ~200-300 lines. The function signature:

```rust
pub(crate) fn build_request_body(
    request: &ChatRequest,
    ghost_tool_names: &[&str],
) -> Result<serde_json::Value, ProviderError>
```

Reference `openai_oauth.rs:build_codex_request_body()` for structural pattern, but the
output format is completely different (Anthropic Messages API, not Codex Responses API).

**Note:** `sanitize_surrogates()` replaces unpaired surrogates. In Rust, `String` is
always valid UTF-8 so isolated surrogates can't appear in normal strings. However, they
can appear in content deserialized from JSON (e.g. `\uD800`). Use a regex or char-level
filter to strip code points in U+D800..U+DFFF.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --lib providers::anthropic::messages -- --nocapture` Expected: all pass

- [ ] **Step 5: Commit**

```bash
git add src/providers/anthropic/messages.rs
git commit -m "feat: anthropic message conversion (Ghost → Anthropic Messages API)"
```

---

### Task 5: SSE stream parsing (`streaming.rs`)

Parses the Anthropic Messages API SSE stream into a `ChatResponse`.

**Files:**

- Create: `src/providers/anthropic/streaming.rs`

- [ ] **Step 1: Write tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn make_sse(events: &[(&str, serde_json::Value)]) -> String {
        events
            .iter()
            .map(|(event, data)| format!("event: {event}\ndata: {}\n\n", data))
            .collect()
    }

    #[test]
    fn parse_simple_text_response() {
        let sse = make_sse(&[
            ("message_start", serde_json::json!({
                "type": "message_start",
                "message": {
                    "id": "msg_1", "model": "claude-sonnet-4-6-20250514",
                    "usage": {"input_tokens": 10, "output_tokens": 0}
                }
            })),
            ("content_block_start", serde_json::json!({
                "type": "content_block_start", "index": 0,
                "content_block": {"type": "text", "text": ""}
            })),
            ("content_block_delta", serde_json::json!({
                "type": "content_block_delta", "index": 0,
                "delta": {"type": "text_delta", "text": "Hello"}
            })),
            ("content_block_delta", serde_json::json!({
                "type": "content_block_delta", "index": 0,
                "delta": {"type": "text_delta", "text": " world"}
            })),
            ("content_block_stop", serde_json::json!({
                "type": "content_block_stop", "index": 0
            })),
            ("message_delta", serde_json::json!({
                "type": "message_delta",
                "delta": {"stop_reason": "end_turn"},
                "usage": {"output_tokens": 5}
            })),
            ("message_stop", serde_json::json!({"type": "message_stop"})),
        ]);
        let resp = parse_sse_response(&sse, "fallback", &[]).unwrap();
        assert_eq!(resp.content.len(), 1);
        match &resp.content[0] {
            ContentBlock::Text { text } => assert_eq!(text, "Hello world"),
            other => panic!("expected Text, got {other:?}"),
        }
        assert_eq!(resp.stop_reason, StopReason::EndTurn);
        assert_eq!(resp.usage.input_tokens, 10);
        assert_eq!(resp.usage.output_tokens, 5);
    }

    #[test]
    fn parse_tool_use_response() {
        let sse = make_sse(&[
            ("message_start", serde_json::json!({
                "type": "message_start",
                "message": {"id": "msg_1", "model": "claude-sonnet-4-6-20250514",
                    "usage": {"input_tokens": 10, "output_tokens": 0}}
            })),
            ("content_block_start", serde_json::json!({
                "type": "content_block_start", "index": 0,
                "content_block": {"type": "tool_use", "id": "toolu_1", "name": "Read", "input": {}}
            })),
            ("content_block_delta", serde_json::json!({
                "type": "content_block_delta", "index": 0,
                "delta": {"type": "input_json_delta", "partial_json": "{\"path\":"}
            })),
            ("content_block_delta", serde_json::json!({
                "type": "content_block_delta", "index": 0,
                "delta": {"type": "input_json_delta", "partial_json": "\"foo.rs\"}"}
            })),
            ("content_block_stop", serde_json::json!({
                "type": "content_block_stop", "index": 0
            })),
            ("message_delta", serde_json::json!({
                "type": "message_delta",
                "delta": {"stop_reason": "tool_use"},
                "usage": {"output_tokens": 20}
            })),
            ("message_stop", serde_json::json!({"type": "message_stop"})),
        ]);
        let resp = parse_sse_response(&sse, "fallback", &["read"]).unwrap();
        assert_eq!(resp.stop_reason, StopReason::ToolUse);
        match &resp.content[0] {
            ContentBlock::ToolUse { id, name, input } => {
                assert_eq!(id, "toolu_1");
                assert_eq!(name, "read"); // reverse-translated from "Read"
                assert_eq!(input["path"], "foo.rs");
            }
            other => panic!("expected ToolUse, got {other:?}"),
        }
    }

    #[test]
    fn parse_thinking_block() {
        let sse = make_sse(&[
            ("message_start", serde_json::json!({
                "type": "message_start",
                "message": {"id": "msg_1", "model": "claude-opus-4-6-20250514",
                    "usage": {"input_tokens": 10, "output_tokens": 0}}
            })),
            ("content_block_start", serde_json::json!({
                "type": "content_block_start", "index": 0,
                "content_block": {"type": "thinking", "thinking": "", "signature": ""}
            })),
            ("content_block_delta", serde_json::json!({
                "type": "content_block_delta", "index": 0,
                "delta": {"type": "thinking_delta", "thinking": "Let me think..."}
            })),
            ("content_block_delta", serde_json::json!({
                "type": "content_block_delta", "index": 0,
                "delta": {"type": "signature_delta", "signature": "sig123"}
            })),
            ("content_block_stop", serde_json::json!({
                "type": "content_block_stop", "index": 0
            })),
            ("content_block_start", serde_json::json!({
                "type": "content_block_start", "index": 1,
                "content_block": {"type": "text", "text": ""}
            })),
            ("content_block_delta", serde_json::json!({
                "type": "content_block_delta", "index": 1,
                "delta": {"type": "text_delta", "text": "Here's my answer."}
            })),
            ("content_block_stop", serde_json::json!({
                "type": "content_block_stop", "index": 1
            })),
            ("message_delta", serde_json::json!({
                "type": "message_delta",
                "delta": {"stop_reason": "end_turn"},
                "usage": {"output_tokens": 30}
            })),
            ("message_stop", serde_json::json!({"type": "message_stop"})),
        ]);
        let resp = parse_sse_response(&sse, "fallback", &[]).unwrap();
        assert_eq!(resp.content.len(), 2);
        assert!(matches!(&resp.content[0], ContentBlock::RawOutput { original_type, .. } if original_type == "thinking"));
        assert!(matches!(&resp.content[1], ContentBlock::Text { text } if text == "Here's my answer."));
    }

    #[test]
    fn parse_redacted_thinking_block() {
        let sse = make_sse(&[
            ("message_start", serde_json::json!({
                "type": "message_start",
                "message": {"id": "msg_1", "model": "claude-opus-4-6-20250514",
                    "usage": {"input_tokens": 10, "output_tokens": 0}}
            })),
            ("content_block_start", serde_json::json!({
                "type": "content_block_start", "index": 0,
                "content_block": {"type": "redacted_thinking", "data": "encrypted_payload"}
            })),
            ("content_block_stop", serde_json::json!({
                "type": "content_block_stop", "index": 0
            })),
            ("content_block_start", serde_json::json!({
                "type": "content_block_start", "index": 1,
                "content_block": {"type": "text", "text": ""}
            })),
            ("content_block_delta", serde_json::json!({
                "type": "content_block_delta", "index": 1,
                "delta": {"type": "text_delta", "text": "Answer."}
            })),
            ("content_block_stop", serde_json::json!({
                "type": "content_block_stop", "index": 1
            })),
            ("message_delta", serde_json::json!({
                "type": "message_delta",
                "delta": {"stop_reason": "end_turn"},
                "usage": {"output_tokens": 10}
            })),
            ("message_stop", serde_json::json!({"type": "message_stop"})),
        ]);
        let resp = parse_sse_response(&sse, "fallback", &[]).unwrap();
        assert_eq!(resp.content.len(), 2);
        match &resp.content[0] {
            ContentBlock::RawOutput { original_type, value } => {
                assert_eq!(original_type, "redacted_thinking");
                assert_eq!(value["data"], "encrypted_payload");
            }
            other => panic!("expected RawOutput, got {other:?}"),
        }
    }

    #[test]
    fn sensitive_stop_reason_is_error() {
        let sse = make_sse(&[
            ("message_start", serde_json::json!({
                "type": "message_start",
                "message": {"id": "msg_1", "model": "claude-sonnet-4-6-20250514",
                    "usage": {"input_tokens": 10, "output_tokens": 0}}
            })),
            ("message_delta", serde_json::json!({
                "type": "message_delta",
                "delta": {"stop_reason": "sensitive"},
                "usage": {"output_tokens": 0}
            })),
            ("message_stop", serde_json::json!({"type": "message_stop"})),
        ]);
        let result = parse_sse_response(&sse, "fallback", &[]);
        assert!(result.is_err());
    }

    #[test]
    fn usage_from_message_start_preserved() {
        let sse = make_sse(&[
            ("message_start", serde_json::json!({
                "type": "message_start",
                "message": {"id": "msg_1", "model": "claude-sonnet-4-6-20250514",
                    "usage": {"input_tokens": 42, "output_tokens": 0,
                              "cache_read_input_tokens": 100,
                              "cache_creation_input_tokens": 50}}
            })),
            ("content_block_start", serde_json::json!({
                "type": "content_block_start", "index": 0,
                "content_block": {"type": "text", "text": ""}
            })),
            ("content_block_delta", serde_json::json!({
                "type": "content_block_delta", "index": 0,
                "delta": {"type": "text_delta", "text": "hi"}
            })),
            ("content_block_stop", serde_json::json!({
                "type": "content_block_stop", "index": 0
            })),
            ("message_delta", serde_json::json!({
                "type": "message_delta",
                "delta": {"stop_reason": "end_turn"},
                "usage": {"output_tokens": 1}
            })),
            ("message_stop", serde_json::json!({"type": "message_stop"})),
        ]);
        let resp = parse_sse_response(&sse, "fallback", &[]).unwrap();
        assert_eq!(resp.usage.input_tokens, 42);
        assert_eq!(resp.usage.output_tokens, 1);
        assert_eq!(resp.usage.cache_read_tokens, Some(100));
        assert_eq!(resp.usage.cache_creation_tokens, Some(50));
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --lib providers::anthropic::streaming -- --nocapture` Expected:
compilation errors

- [ ] **Step 3: Implement SSE parser**

The function signature:

```rust
pub(crate) fn parse_sse_response(
    raw: &str,
    fallback_model: &str,
    ghost_tool_names: &[&str],
) -> Result<ChatResponse, ProviderError>
```

Implementation approach:

- Track in-progress content blocks by index (Vec or HashMap)
- Each block tracks: type (text/tool_use/thinking/redacted_thinking), accumulated text,
  accumulated json (for tool input), thinking text, signature, tool id, tool name
- On `message_start`: capture response_id, model, initial usage
- On `content_block_start`: init block state by type
- On `content_block_delta`: accumulate based on delta type
- On `content_block_stop`: finalize block into ContentBlock
- On `message_delta`: capture stop_reason, update usage (only non-null fields)
- On `error` event: return ProviderError
- After parsing: map stop_reason string to StopReason enum (with error variants per
  spec)
- Reverse-translate tool names via `from_claude_code_name()`

Reference the Codex SSE parser at `codex_responses.rs:269-375` for the SSE line
extraction pattern (split on `\n\n`, extract `data:` lines), but the event types and
accumulation logic are completely different.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --lib providers::anthropic::streaming -- --nocapture` Expected: all
pass

- [ ] **Step 5: Commit**

```bash
git add src/providers/anthropic/streaming.rs
git commit -m "feat: anthropic SSE stream parser"
```

---

### Task 6: Provider struct and trait impl (`mod.rs`)

Wire everything together: construct the provider, dispatch requests, handle errors.

**Files:**

- Modify: `src/providers/anthropic/mod.rs`
- Modify: `src/providers/types.rs`

- [ ] **Step 1: Write the provider struct and constructor**

In `src/providers/anthropic/mod.rs`:

```rust
mod credentials;
mod messages;
mod streaming;
mod tool_names;

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::RwLock;
use std::time::Instant;

use async_trait::async_trait;
use reqwest::header::{ACCEPT, AUTHORIZATION, CONTENT_TYPE, HeaderMap, HeaderName, HeaderValue};
use serde_json::Value;
use tracing::Span;

use crate::providers::circuit_breaker::CircuitBreaker;
use crate::providers::types::{
    ChatRequest, ChatResponse, ContentBlock, Provider, ProviderError, StopReason,
};

use credentials::OAuthCredentials;

const ANTHROPIC_API_URL: &str = "https://api.anthropic.com/v1/messages";
const ANTHROPIC_VERSION: &str = "2023-06-01";
const USER_AGENT: &str = "claude-cli/2.1.75";

/// Beta flags always included.
const BASE_BETA_FLAGS: &str = "claude-code-20250219,oauth-2025-04-20,fine-grained-tool-streaming-2025-05-14";
/// Additional beta flag for older models that need interleaved thinking.
const INTERLEAVED_THINKING_BETA: &str = "interleaved-thinking-2025-05-14";

#[derive(Debug)]
pub struct AnthropicProvider {
    client: reqwest::Client,
    credentials: RwLock<OAuthCredentials>,
    credentials_path: Option<PathBuf>,
    circuit_breaker: CircuitBreaker,
    static_headers: HeaderMap,
    debug_save_requests: bool,
    debug_dir: Option<PathBuf>,
}
```

`new()`:

- Call `credentials::load_credentials()` to get initial token
- Build `static_headers`: Content-Type, Accept, anthropic-version, user-agent, x-app
- Merge extra_headers from config
- Store credentials in `RwLock` for refresh

`set_debug()`: same pattern as other providers.

`send_request()`:

- Circuit breaker check
- Check if credentials expired → refresh if file-based (has credentials_path)
- Build headers: static + Authorization Bearer + anthropic-beta (with model-dependent
  interleaved-thinking flag)
- Call `messages::build_request_body()` with request and ghost tool names
- POST to endpoint
- Parse status, handle errors (same pattern as `openai_oauth.rs:199-231`)
- Call `streaming::parse_sse_response()`
- Circuit breaker record
- OTel span recording (same pattern as `openai_oauth.rs:246-283`)
- Debug request saving

`Provider` impl: delegate to `send_request()`, name returns `"anthropic"`.

Extract ghost tool names from `request.tools` for reverse translation.

- [ ] **Step 2: Detect adaptive vs budget thinking by model ID**

Helper function:

```rust
fn is_adaptive_thinking_model(model: &str) -> bool {
    let lower = model.to_lowercase();
    lower.contains("opus-4-6") || lower.contains("opus-4.6")
        || lower.contains("sonnet-4-6") || lower.contains("sonnet-4.6")
}
```

Used in both `messages.rs` (for thinking config in request body) and `mod.rs` (for beta
header selection).

Put this in `mod.rs` and make it `pub(crate)` so `messages.rs` can use it too.

- [ ] **Step 3: Register in `provider_for_alias()`**

In `src/providers/types.rs`, add a new arm to the match in `provider_for_alias()`:

```rust
"anthropic" => {
    let mut provider =
        crate::providers::anthropic::AnthropicProvider::new(model.headers.clone())?;
    provider.set_debug(config.debug.save_requests, &config.workspace);
    Ok(Arc::new(provider))
}
```

Add it before the `unsupported =>` arm.

- [ ] **Step 4: Update `src/providers/mod.rs` exports**

```rust
pub mod anthropic;
```

And add to the re-exports if needed:

```rust
pub use anthropic::AnthropicProvider;
```

- [ ] **Step 5: Verify it compiles**

Run: `cargo check` Expected: compiles clean

- [ ] **Step 6: Commit**

```bash
git add src/providers/anthropic/mod.rs src/providers/types.rs src/providers/mod.rs
git commit -m "feat: AnthropicProvider struct, trait impl, and registration"
```

---

### Task 7: Integration smoke test

Write a live test that exercises the full provider round-trip.

**Files:**

- Create: `tests/providers/anthropic_live.rs` (or add to existing provider test file)

- [ ] **Step 1: Write the live test**

```rust
//! Live test for the Anthropic OAuth provider.
//! Requires Claude Code OAuth credentials and `--features live-tests-llms`.

#[cfg(feature = "live-tests-llms")]
mod anthropic_live {
    use ghost::providers::anthropic::AnthropicProvider;
    use ghost::providers::types::*;
    use serde_json::json;
    use std::collections::BTreeMap;

    #[tokio::test]
    async fn anthropic_simple_chat() {
        let provider = AnthropicProvider::new(BTreeMap::new())
            .expect("provider init (need Claude Code credentials)");

        let response = provider
            .chat(ChatRequest {
                model: "claude-sonnet-4-6-20250514".into(),
                messages: vec![ChatMessage {
                    role: Role::User,
                    content: vec![ContentBlock::Text {
                        text: "Say 'hello' and nothing else.".into(),
                    }],
                }],
                tools: None,
                max_tokens: Some(100),
                temperature: Some(0.0),
                system: Some("You are a test assistant.".into()),
                reasoning_effort: None,
                cache_key: String::new(),
                turn_state: None,
                debug_context: None,
            })
            .await
            .expect("chat request");

        assert!(!response.content.is_empty(), "response should have content");
        let text = response
            .content
            .iter()
            .find_map(|b| match b {
                ContentBlock::Text { text } => Some(text.as_str()),
                _ => None,
            })
            .expect("should have text block");
        assert!(
            text.to_lowercase().contains("hello"),
            "expected 'hello' in response, got: {text}"
        );
        assert_eq!(response.stop_reason, StopReason::EndTurn);
        assert!(response.usage.input_tokens > 0);
        assert!(response.usage.output_tokens > 0);
    }

    #[tokio::test]
    async fn anthropic_tool_use() {
        let provider = AnthropicProvider::new(BTreeMap::new())
            .expect("provider init");

        let response = provider
            .chat(ChatRequest {
                model: "claude-sonnet-4-6-20250514".into(),
                messages: vec![ChatMessage {
                    role: Role::User,
                    content: vec![ContentBlock::Text {
                        text: "What's the weather in Paris? Use the get_weather tool.".into(),
                    }],
                }],
                tools: Some(vec![ToolDefinition {
                    name: "get_weather".into(),
                    description: "Get the weather for a city.".into(),
                    input_schema: json!({
                        "type": "object",
                        "properties": {
                            "city": {"type": "string"}
                        },
                        "required": ["city"]
                    }),
                }]),
                max_tokens: Some(1024),
                temperature: None,
                system: None,
                reasoning_effort: None,
                cache_key: String::new(),
                turn_state: None,
                debug_context: None,
            })
            .await
            .expect("chat request");

        assert_eq!(response.stop_reason, StopReason::ToolUse);
        let tool_use = response
            .content
            .iter()
            .find(|b| matches!(b, ContentBlock::ToolUse { .. }))
            .expect("should have tool_use block");
        match tool_use {
            ContentBlock::ToolUse { name, input, .. } => {
                assert_eq!(name, "get_weather");
                assert!(input.get("city").is_some());
            }
            _ => unreachable!(),
        }
    }
}
```

- [ ] **Step 2: Run with live test feature**

Run: `cargo test --features live-tests-llms anthropic_live -- --nocapture` Expected:
both tests pass (requires Claude Code credentials on the machine)

- [ ] **Step 3: Commit**

```bash
git add tests/providers/anthropic_live.rs
git commit -m "test: live tests for Anthropic OAuth provider"
```

---

### Task 8: Run `just ci` and fix any issues

- [ ] **Step 1: Run CI**

Run: `just ci` Expected: format, check, clippy, and tests all pass

- [ ] **Step 2: Fix any issues found**

Address clippy warnings, formatting issues, test failures.

- [ ] **Step 3: Final commit**

```bash
git add -A
git commit -m "fix: address CI issues for anthropic provider"
```

---

## Dependencies

```
Task 1 (skeleton) → Task 2 (tool_names) → Task 4 (messages)
Task 1 (skeleton) → Task 2 (tool_names) → Task 5 (streaming)
Task 1 (skeleton) → Task 3 (credentials)
Tasks 2,3,4,5 → Task 6 (mod.rs / wiring)
Task 6 → Task 7 (live tests)
Task 7 → Task 8 (CI)
```

Tasks 3, 4, 5 are independent of each other but Tasks 4 and 5 both depend on Task 2
(tool name functions). Task 3 can run in parallel with everything after Task 1.
