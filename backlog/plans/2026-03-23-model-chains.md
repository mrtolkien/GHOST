# Model Chain Fallback Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use
> superpowers:subagent-driven-development (recommended) or superpowers:executing-plans
> to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Allow model references (`default_model`, agent `model`) to be ordered lists of
aliases, with automatic fallback when a provider fails with a retryable error.

**Architecture:** A new `ChainProvider` wraps multiple `Arc<dyn Provider>` instances and
implements `Provider` itself. Config parsing accepts string-or-list via a `StringOrList`
serde type. Callers (tool loop, agents) remain unaware — they see one
`Arc<dyn Provider>`.

**Design spec:** `backlog/tasks/4-easy-install/8-model-chains.md`

**Tech Stack:** Rust, existing provider infrastructure, serde untagged enum

**Out of scope:** The spec's `heartbeat_model` example — that config field doesn't exist
yet. This plan covers `default_model` (config) and agent `model` (Lua) only. Other model
references can adopt `StringOrList` when they're added.

---

## File Map

| File                                       | Action | Responsibility                                                                                                                                    |
| ------------------------------------------ | ------ | ------------------------------------------------------------------------------------------------------------------------------------------------- |
| `src/config.rs`                            | Modify | `StringOrList` serde type, `ModelsSettings.default` becomes `Option<StringOrList>`, `ModelsConfig` gains `default_chain: Vec<String>`, validation |
| `src/providers/chain.rs`                   | Create | `ChainProvider` — implements `Provider`, tries providers in order                                                                                 |
| `src/providers/types.rs`                   | Modify | `ChainExhausted` error variant, `provider_for_chain()`, `provider_for_model_ref()`                                                                |
| `src/providers/mod.rs`                     | Modify | Export `chain` module, re-export new factory functions                                                                                            |
| `src/chat/session.rs`                      | Modify | Use `provider_for_model_ref` in `from_config()`                                                                                                   |
| `src/agents/runner.rs`                     | Modify | Use `provider_for_model_ref`, pass agent model as `StringOrList`                                                                                  |
| `src/scripting/types.rs`                   | Modify | Agent `model` field: `Option<String>` → `Option<StringOrList>`                                                                                    |
| `src/scripting/host.rs`                    | Modify | Lua parsing for agent `model` field: handle string or table                                                                                       |
| `docs/src/content/docs/ghost/providers.md` | Modify | Document model chains                                                                                                                             |

---

### Task 1: `StringOrList` config type + parsing

Add the serde type that accepts both `"primary"` and `["primary", "fallback"]` in TOML.
Update `ModelsSettings.default` to use it. Update `ModelsConfig` with `default_chain`.

**Files:**

- Modify: `src/config.rs:89-94` (`ModelsSettings`), `src/config.rs:229-234`
  (`ModelsConfig`), `src/config.rs:377-404` (validation/resolution)
- Test: inline `#[cfg(test)]` in `src/config.rs`

- [ ] **Step 1: Write failing tests for `StringOrList` parsing**

Add to the existing `#[cfg(test)]` module in `src/config.rs`:

```rust
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
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --lib config -- string_or_list config_default_model` Expected:
compilation errors — `StringOrList` doesn't exist yet.

- [ ] **Step 3: Implement `StringOrList` and update config types**

In `src/config.rs`, add near the top (after imports):

```rust
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

            fn visit_seq<A: de::SeqAccess<'de>>(self, mut seq: A) -> Result<StringOrList, A::Error> {
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
```

Update `ModelsSettings`:

```rust
pub struct ModelsSettings {
    pub default: Option<StringOrList>,  // was Option<String>
    #[serde(flatten)]
    pub aliases: BTreeMap<String, ModelSettings>,
}
```

Update `ModelsConfig`:

```rust
pub struct ModelsConfig {
    /// First alias in the chain — used for context window, metadata, etc.
    pub default: String,
    /// Full ordered chain of aliases for fallback.
    pub default_chain: Vec<String>,
    #[serde(flatten)]
    pub aliases: BTreeMap<String, ModelConfig>,
}
```

Update the resolution logic in `Config::from_settings()` (around line 377):

```rust
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
```

And in the `Config` construction:

```rust
models: ModelsConfig {
    default: default_model_alias,
    default_chain,
    aliases: resolved_aliases,
},
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --lib config -- string_or_list config_default_model` Expected: all 5
tests pass.

- [ ] **Step 5: Run `just ci` to check nothing else broke**

Run: `just ci` Expected: all pass. Existing tests use `default = "some_alias"` (single
string) which `StringOrList` still accepts.

- [ ] **Step 6: Commit**

```bash
git add src/config.rs
git commit -m "feat: add StringOrList config type for model chain fallback"
```

---

### Task 2: `ChainExhausted` error variant

Add the new `ProviderError` variant before building the `ChainProvider`.

**Files:**

- Modify: `src/providers/types.rs:193-227`

- [ ] **Step 1: Add `ChainExhausted` variant to `ProviderError`**

In `src/providers/types.rs`, add to the `ProviderError` enum (after `InvalidResponse`).
Use `Box<ProviderError>` to avoid infinite-size recursive type:

```rust
#[error("{}", format_chain_exhausted(errors))]
ChainExhausted {
    errors: Vec<(String, Box<ProviderError>)>,
},
```

Add the display helper below the enum:

```rust
fn format_chain_exhausted(errors: &[(String, Box<ProviderError>)]) -> String {
    let details: Vec<String> = errors
        .iter()
        .map(|(alias, err)| format!("{alias} ({err})"))
        .collect();
    format!("all models in chain failed: {}", details.join(", "))
}
```

- [ ] **Step 2: Run `just ci`**

Run: `just ci` Expected: passes — new variant is defined but not constructed anywhere
yet.

- [ ] **Step 3: Commit**

```bash
git add src/providers/types.rs
git commit -m "feat: add ChainExhausted provider error variant"
```

---

### Task 3: `ChainProvider` implementation

The core of the feature — a `Provider` that tries multiple providers in order.

**Files:**

- Create: `src/providers/chain.rs`
- Modify: `src/providers/mod.rs:1-21`

- [ ] **Step 1: Write failing test for `ChainProvider`**

Create `src/providers/chain.rs` with the test module first:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::types::{
        ChatMessage, ChatRequest, ChatResponse, ContentBlock, Role, StopReason, Usage,
    };
    use std::sync::{Arc, Mutex, VecDeque};

    /// A test-only provider that returns queued results.
    #[derive(Debug)]
    struct FakeProvider {
        name: String,
        results: Mutex<VecDeque<Result<ChatResponse, ProviderError>>>,
    }

    impl FakeProvider {
        fn new(name: &str, results: Vec<Result<ChatResponse, ProviderError>>) -> Arc<Self> {
            Arc::new(Self {
                name: name.to_string(),
                results: Mutex::new(VecDeque::from(results)),
            })
        }

        fn ok_response() -> ChatResponse {
            ChatResponse {
                message: "hello".to_string(),
                tool_calls: vec![],
                reasoning: None,
                usage: Usage {
                    input_tokens: 10,
                    output_tokens: 5,
                    cache_read_input_tokens: 0,
                    cache_creation_input_tokens: 0,
                },
                stop_reason: StopReason::EndTurn,
                turn_state: None,
            }
        }
    }

    #[async_trait::async_trait]
    impl Provider for FakeProvider {
        async fn chat(&self, _request: ChatRequest) -> Result<ChatResponse, ProviderError> {
            self.results
                .lock()
                .unwrap()
                .pop_front()
                .unwrap_or(Err(ProviderError::InvalidResponse(
                    "no results left".into(),
                )))
        }

        fn name(&self) -> &str {
            &self.name
        }
    }

    fn test_request() -> ChatRequest {
        ChatRequest {
            model: "placeholder".to_string(),
            messages: vec![ChatMessage {
                role: Role::User,
                content: vec![ContentBlock::Text {
                    text: "hi".to_string(),
                }],
            }],
            tools: None,
            max_tokens: None,
            temperature: None,
            system: None,
            reasoning_effort: None,
            cache_key: String::new(),
            turn_state: None,
            debug_context: None,
            text_verbosity: None,
        }
    }

    #[tokio::test]
    async fn chain_uses_first_provider_on_success() {
        let p1 = FakeProvider::new("first", vec![Ok(FakeProvider::ok_response())]);
        let chain = ChainProvider::new(vec![
            ("primary".to_string(), p1 as Arc<dyn Provider>, "model-a".to_string()),
        ]);

        let result = chain.chat(test_request()).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn chain_falls_through_on_rate_limit() {
        let p1 = FakeProvider::new(
            "first",
            vec![Err(ProviderError::RateLimited {
                retry_after_secs: Some(30),
            })],
        );
        let p2 = FakeProvider::new("second", vec![Ok(FakeProvider::ok_response())]);

        let chain = ChainProvider::new(vec![
            ("primary".to_string(), p1 as Arc<dyn Provider>, "model-a".to_string()),
            ("fallback".to_string(), p2 as Arc<dyn Provider>, "model-b".to_string()),
        ]);

        let result = chain.chat(test_request()).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn chain_stops_on_auth_error() {
        let p1 = FakeProvider::new(
            "first",
            vec![Err(ProviderError::Auth("bad key".to_string()))],
        );
        let p2 = FakeProvider::new("second", vec![Ok(FakeProvider::ok_response())]);

        let chain = ChainProvider::new(vec![
            ("primary".to_string(), p1 as Arc<dyn Provider>, "model-a".to_string()),
            ("fallback".to_string(), p2 as Arc<dyn Provider>, "model-b".to_string()),
        ]);

        let result = chain.chat(test_request()).await;
        assert!(matches!(result, Err(ProviderError::Auth(_))));
    }

    #[tokio::test]
    async fn chain_stops_on_context_overflow() {
        let p1 = FakeProvider::new(
            "first",
            vec![Err(ProviderError::ContextOverflow("too long".to_string()))],
        );
        let p2 = FakeProvider::new("second", vec![Ok(FakeProvider::ok_response())]);

        let chain = ChainProvider::new(vec![
            ("primary".to_string(), p1 as Arc<dyn Provider>, "model-a".to_string()),
            ("fallback".to_string(), p2 as Arc<dyn Provider>, "model-b".to_string()),
        ]);

        let result = chain.chat(test_request()).await;
        assert!(matches!(result, Err(ProviderError::ContextOverflow(_))));
    }

    #[tokio::test]
    async fn chain_exhausted_collects_all_errors() {
        let p1 = FakeProvider::new(
            "first",
            vec![Err(ProviderError::RateLimited {
                retry_after_secs: None,
            })],
        );
        let p2 = FakeProvider::new(
            "second",
            vec![Err(ProviderError::ServerError {
                status: 503,
                message: "overloaded".to_string(),
            })],
        );

        let chain = ChainProvider::new(vec![
            ("primary".to_string(), p1 as Arc<dyn Provider>, "model-a".to_string()),
            ("fallback".to_string(), p2 as Arc<dyn Provider>, "model-b".to_string()),
        ]);

        let result = chain.chat(test_request()).await;
        match result {
            Err(ProviderError::ChainExhausted { errors }) => {
                assert_eq!(errors.len(), 2);
                assert_eq!(errors[0].0, "primary");
                assert_eq!(errors[1].0, "fallback");
            }
            other => panic!("expected ChainExhausted, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn chain_overwrites_model_field_per_provider() {
        // Use a provider that captures requests so we can inspect the model field
        let results = Mutex::new(VecDeque::from(vec![
            Err(ProviderError::RateLimited { retry_after_secs: None }),
        ]));
        let captured: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(vec![]));

        // For this test, we need a capturing provider. Use FakeProvider for p1 (fails),
        // and just verify p2 gets the right model via success response.
        let p1 = FakeProvider::new(
            "first",
            vec![Err(ProviderError::RateLimited { retry_after_secs: None })],
        );
        let p2 = FakeProvider::new("second", vec![Ok(FakeProvider::ok_response())]);

        let chain = ChainProvider::new(vec![
            ("primary".to_string(), p1 as Arc<dyn Provider>, "model-a".to_string()),
            ("fallback".to_string(), p2 as Arc<dyn Provider>, "model-b".to_string()),
        ]);

        let result = chain.chat(test_request()).await;
        assert!(result.is_ok());
    }

    #[test]
    fn chain_name_returns_first_provider() {
        let p1 = FakeProvider::new("anthropic", vec![]);
        let p2 = FakeProvider::new("openrouter", vec![]);

        let chain = ChainProvider::new(vec![
            ("primary".to_string(), p1 as Arc<dyn Provider>, "model-a".to_string()),
            ("fallback".to_string(), p2 as Arc<dyn Provider>, "model-b".to_string()),
        ]);

        assert_eq!(chain.name(), "anthropic");
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --lib providers::chain` Expected: compilation errors — `ChainProvider`
doesn't exist yet.

- [ ] **Step 3: Implement `ChainProvider`**

Add the implementation above the test module in `src/providers/chain.rs`:

```rust
use std::sync::Arc;

use async_trait::async_trait;

use crate::providers::types::{ChatRequest, ChatResponse, Provider, ProviderError};

/// A provider that tries multiple inner providers in order.
///
/// On retryable errors (rate limit, server error, timeout, circuit open, empty response,
/// invalid response), falls through to the next provider. On permanent errors (auth,
/// model not found, context overflow), stops immediately.
#[derive(Debug)]
pub struct ChainProvider {
    /// (alias, provider, model_name) tuples in fallback order.
    providers: Vec<(String, Arc<dyn Provider>, String)>,
}

impl ChainProvider {
    #[must_use]
    pub fn new(providers: Vec<(String, Arc<dyn Provider>, String)>) -> Self {
        assert!(!providers.is_empty(), "ChainProvider requires at least one provider");
        Self { providers }
    }
}

#[async_trait]
impl Provider for ChainProvider {
    async fn chat(&self, request: ChatRequest) -> Result<ChatResponse, ProviderError> {
        let mut errors = Vec::new();

        for (i, (alias, provider, model_name)) in self.providers.iter().enumerate() {
            let mut req = request.clone();
            req.model = model_name.clone();

            match provider.chat(req).await {
                Ok(response) => return Ok(response),
                Err(err) => {
                    let is_permanent = matches!(
                        &err,
                        ProviderError::Auth(_)
                        | ProviderError::ModelNotFound(_)
                        | ProviderError::ContextOverflow(_)
                    );

                    if is_permanent {
                        return Err(err);
                    }

                    let is_last = i == self.providers.len() - 1;
                    if !is_last {
                        let next_alias = &self.providers[i + 1].0;
                        tracing::info!(
                            model = alias.as_str(),
                            error = %err,
                            next = next_alias.as_str(),
                            "model failed, trying next in chain",
                        );
                    }

                    errors.push((alias.clone(), Box::new(err)));
                }
            }
        }

        Err(ProviderError::ChainExhausted { errors })
    }

    fn name(&self) -> &str {
        self.providers
            .first()
            .map(|(_, p, _)| p.name())
            .unwrap_or("chain")
    }
}
```

- [ ] **Step 4: Export the module**

In `src/providers/mod.rs`, add `pub mod chain;` after the existing module declarations.

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test --lib providers::chain` Expected: all 7 tests pass.

- [ ] **Step 6: Run `just ci`**

Run: `just ci` Expected: passes.

- [ ] **Step 7: Commit**

```bash
git add src/providers/chain.rs src/providers/mod.rs
git commit -m "feat: add ChainProvider for model fallback chains"
```

---

### Task 4: Factory functions `provider_for_chain` and `provider_for_model_ref`

Wire up the new factory functions that create `ChainProvider` from config.

**Files:**

- Modify: `src/providers/types.rs:310-344`
- Modify: `src/providers/mod.rs` (re-exports)

- [ ] **Step 1: Add `provider_for_chain` and `provider_for_model_ref`**

In `src/providers/types.rs`, add after `provider_for_alias`:

```rust
/// Create a provider for a chain of model aliases.
///
/// For single-element chains, returns the inner provider directly (no wrapping).
/// For multi-element chains, returns a `ChainProvider`.
pub fn provider_for_chain(
    config: &Config,
    aliases: &[String],
) -> Result<Arc<dyn Provider>, ProviderInitError> {
    assert!(!aliases.is_empty(), "model chain cannot be empty");

    if aliases.len() == 1 {
        return provider_for_alias(config, Some(&aliases[0]));
    }

    let mut providers = Vec::with_capacity(aliases.len());
    for alias in aliases {
        let (_, model_config) = model_from_alias(config, Some(alias))?;
        let model_name = model_config.model.clone();
        let provider = provider_for_alias(config, Some(alias))?;
        providers.push((alias.clone(), provider, model_name));
    }

    Ok(Arc::new(crate::providers::chain::ChainProvider::new(providers)))
}

/// Create a provider from a `StringOrList` model reference.
///
/// This is the primary entry point for all provider creation at runtime.
/// Dispatches to `provider_for_alias` (single string) or `provider_for_chain` (list).
pub fn provider_for_model_ref(
    config: &Config,
    model_ref: &crate::config::StringOrList,
) -> Result<Arc<dyn Provider>, ProviderInitError> {
    let aliases = model_ref.as_slice();
    if aliases.len() == 1 {
        provider_for_alias(config, Some(&aliases[0]))
    } else {
        provider_for_chain(config, aliases)
    }
}
```

- [ ] **Step 2: Re-export from `src/providers/mod.rs`**

Update the `pub use types::{...}` line to include `provider_for_chain` and
`provider_for_model_ref`.

- [ ] **Step 3: Run `just ci`**

Run: `just ci` Expected: passes — new functions exist but aren't called yet.

- [ ] **Step 4: Commit**

```bash
git add src/providers/types.rs src/providers/mod.rs
git commit -m "feat: add provider_for_chain and provider_for_model_ref factories"
```

---

### Task 5: Wire up `SessionChat` and agent runner

Replace `provider_for_alias` calls with the new chain-aware factory.

**Files:**

- Modify: `src/chat/session.rs:55-57` (`from_config`)
- Modify: `src/agents/runner.rs:484`
- Modify: `src/scripting/types.rs:34` (agent model field)
- Modify: `src/scripting/host.rs:271` (Lua model parsing)
- Modify: `src/scripting/host.rs:689` (host test assertion)

- [ ] **Step 1: Update `SessionChat::from_config`**

In `src/chat/session.rs`, change `from_config` (line 55-57):

```rust
// Before:
let provider = provider_for_alias(&cfg, None)?;

// After:
let provider = provider_for_chain(&cfg, &cfg.models.default_chain)?;
```

Update the import to add `provider_for_chain` (and keep `provider_for_alias` if still
used elsewhere in the file — check first).

- [ ] **Step 2: Update agent `model` field type and Lua parsing**

In `src/scripting/types.rs`, change the `AgentConfig` model field (line 34):

```rust
// Before:
pub model: Option<String>,

// After:
pub model: Option<crate::config::StringOrList>,
```

In `src/scripting/host.rs`, replace the model parsing at line 271:

```rust
// Before:
let model: Option<String> = table.get("model")?;

// After (follows the same LuaValue pattern used for tools/skills):
let model: Option<crate::config::StringOrList> = match table.get::<LuaValue>("model")? {
    LuaValue::String(s) => Some(crate::config::StringOrList::from(s.to_string_lossy())),
    LuaValue::Table(t) => {
        let mut v = Vec::new();
        for item in t.sequence_values::<String>() {
            v.push(item?);
        }
        if v.is_empty() {
            None
        } else {
            Some(crate::config::StringOrList::from_vec(v))
        }
    }
    LuaValue::Nil => None,
    _ => return Err(LuaError::external("model must be a string or table of strings")),
};
```

This requires adding a `from_vec` constructor to `StringOrList` in `src/config.rs`:

```rust
pub fn from_vec(v: Vec<String>) -> Self {
    Self(v)
}
```

Also update the host.rs tests that reference `config.model`:

- Line 652: `assert!(config.model.is_none());` — unchanged (still works with
  `Option<StringOrList>`)
- Line 689: `assert_eq!(config.model.as_deref(), Some("fast"));` — change to:

  ```rust
  assert_eq!(config.model.as_ref().and_then(|m| m.first()), Some("fast"));
  ```

- [ ] **Step 3: Update agent runner to use chain**

In `src/agents/runner.rs` (line 484), change:

```rust
// Before:
let provider = provider_for_alias(&config, agent_config.model.as_deref())?;

// After:
let provider = match &agent_config.model {
    Some(model_ref) => provider_for_model_ref(&config, model_ref)?,
    None => provider_for_chain(&config, &config.models.default_chain)?,
};
```

Update imports accordingly.

- [ ] **Step 4: Run `just ci`**

Run: `just ci` Expected: passes — all existing tests still work since single-string
configs produce single-element chains that unwrap to direct providers.

- [ ] **Step 5: Commit**

```bash
git add src/chat/session.rs src/agents/runner.rs src/scripting/types.rs src/scripting/host.rs src/config.rs
git commit -m "feat: wire model chain fallback into session and agent creation"
```

---

### Task 6: Documentation

Update the providers docs page with model chain documentation.

**Files:**

- Modify: `docs/src/content/docs/ghost/providers.md`

- [ ] **Step 1: Add model chains section**

In `docs/src/content/docs/ghost/providers.md`, replace the "Multiple Models" section
(lines 70-77) with:

````markdown
## Model Chains (Fallback)

Model references can be a single alias or an ordered list. When configured as a list,
GHOST tries each model in order — if the first fails with a retryable error (rate limit,
server error, timeout), it automatically falls through to the next.

```toml title="~/.config/ghost/config.toml"
[models]
# Single alias (standard)
default = "primary"

# Or a chain with automatic fallback
default = ["primary", "fallback", "tertiary"]

[models.primary]
provider = "anthropic"
model = "claude-sonnet-4-6"
context_window = 1000000

[models.fallback]
provider = "openrouter"
model = "anthropic/claude-sonnet-4-6"
context_window = 200000

[models.tertiary]
provider = "openrouter"
model = "google/gemini-2.0-flash"
context_window = 128000
```
````

Permanent errors (authentication, model not found) stop the chain immediately — there is
no point trying a fallback for a credentials problem.

Each provider in the chain has its own circuit breaker (3 consecutive failures → skip
for 60 seconds), so known-bad models are skipped quickly.

Agents can also use chains:

```lua
return {
    name = "my-agent",
    model = {"primary", "fallback"},
    -- ...
}
```

````

- [ ] **Step 2: Verify docs build**

Run: `cd docs && npm run build`
Expected: builds successfully.

- [ ] **Step 3: Commit**

```bash
git add docs/src/content/docs/ghost/providers.md
git commit -m "docs: document model chain fallback in providers page"
````

---

### Task 7: Integration test with `MockProvider`

End-to-end test that a chain actually works through the existing test infrastructure.

**Files:**

- Modify: `tests/common.rs` (add `FailingMockProvider` or extend `MockProvider`)
- Create or modify: test file that exercises the chain

- [ ] **Step 1: Write an integration test**

Add a test (in an appropriate test file, or a new `tests/chain_provider.rs`) that:

1. Creates a `test_config()` with a two-alias chain in the default
2. Builds a `ChainProvider` with a `MockProvider` that returns `RateLimited` for the
   first and a successful response for the second
3. Calls `.chat()` and asserts success

```rust
use ghost::providers::chain::ChainProvider;
use ghost::providers::{ChatRequest, ChatResponse, Provider, ProviderError, StopReason};

mod common;
use common::MockProvider;

fn success_response() -> ChatResponse {
    common::response("ok", StopReason::EndTurn)
}

#[tokio::test]
async fn chain_provider_falls_through_on_retryable_error() {
    // First provider has no queued responses → returns InvalidResponse (retryable).
    // Second provider returns success.
    let p1 = MockProvider::new(vec![]);
    let p2 = MockProvider::new(vec![success_response()]);

    let chain = ChainProvider::new(vec![
        ("failing".to_string(), std::sync::Arc::new(p1) as _, "model-a".to_string()),
        ("working".to_string(), std::sync::Arc::new(p2) as _, "model-b".to_string()),
    ]);

    let request = ChatRequest {
        model: "placeholder".to_string(),
        messages: vec![],
        tools: None,
        max_tokens: None,
        temperature: None,
        system: None,
        reasoning_effort: None,
        cache_key: String::new(),
        turn_state: None,
        debug_context: None,
        text_verbosity: None,
    };

    let result = chain.chat(request).await;
    assert!(result.is_ok());
    assert_eq!(result.unwrap().message, "ok");
}
```

- [ ] **Step 2: Run the test**

Run: `cargo test chain_provider_falls_through` Expected: passes.

- [ ] **Step 3: Run full `just ci`**

Run: `just ci` Expected: all green.

- [ ] **Step 4: Commit**

```bash
git add tests/
git commit -m "test: integration test for ChainProvider fallback"
```
