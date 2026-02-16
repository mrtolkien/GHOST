# 05a — Kimi Code Provider

## Overview

Kimi Code (Moonshot AI) is an OpenAI-compatible provider that requires special handling:
a custom User-Agent header and a specific base URL. It reuses the OpenAI-compatible
client internally.

This provider was in t-koma and worked well. The implementation is straightforward but
the User-Agent requirement was easy to miss and caused silent failures.

## How It Differs from Generic OpenAI-Compatible

| Aspect          | Generic OpenAI-compatible | Kimi Code                         |
| --------------- | ------------------------- | --------------------------------- |
| Base URL        | Configured per model      | `https://api.kimi.com/coding/v1`  |
| Auth            | Optional API key          | Required `KIMI_API_KEY`           |
| User-Agent      | Default reqwest           | **Must be** `KimiCLI/1.12.0`      |
| Context window  | Varies                    | 262,144 tokens                    |
| Empty responses | Rare                      | Occasional — needs retry_on_empty |

## Implementation

Kimi Code should NOT be a separate provider implementation. It's a configuration preset
on top of the OpenAI-compatible client:

```rust
pub fn create_kimi_code_provider(config: &ModelConfig) -> Result<Box<dyn Provider>> {
    let base_url = config.base_url.clone()
        .unwrap_or_else(|| "https://api.kimi.com/coding/v1".to_string());

    let api_key = std::env::var("KIMI_API_KEY")
        .map_err(|_| ProviderError::Auth("KIMI_API_KEY not set".into()))?;

    let mut headers = HeaderMap::new();
    headers.insert(USER_AGENT, HeaderValue::from_static("KimiCLI/1.12.0"));

    // Per-model headers can override defaults
    if let Some(cfg_headers) = &config.headers {
        for (k, v) in cfg_headers {
            if let (Ok(name), Ok(val)) = (
                HeaderName::try_from(k.as_str()),
                HeaderValue::from_str(v),
            ) {
                headers.insert(name, val);
            }
        }
    }

    Ok(Box::new(OpenAiCompatibleClient::new(
        base_url, Some(api_key), &config.model, "kimi_code",
    ).with_extra_headers(headers)))
}
```

## Config

```toml
[models.kimi]
provider = "kimi_code"
model = "kimi-k2-0711-chat"
# base_url and headers are optional overrides

# Override User-Agent if Kimi changes their requirement
# [models.kimi.headers]
# User-Agent = "KimiCLI/2.0.0"
```

```bash
KIMI_API_KEY=...
```

## Gotchas (from t-koma experience)

1. **User-Agent is mandatory** — Without the correct User-Agent header, the API returns
   opaque errors. This was the #1 debugging headache.
2. **Empty responses** — Kimi occasionally returns empty content. Use
   `retry_on_empty: 2` in config.
3. **Distinct from Moonshot Open Platform** — `api.kimi.com/coding` is a different
   endpoint from `api.moonshot.ai` with different API keys and models.
4. **Version sensitivity** — The `KimiCLI/1.12.0` version string may need updating if
   Kimi starts rejecting older versions. Make it configurable via headers.

## Acceptance Criteria

- Kimi Code works as a provider with the correct User-Agent header
- Falls back to default User-Agent if none configured
- Empty response retry works
- Provider is registered as `kimi_code` in model config
- Integration test (live-tests) validates a chat completion
- `just ci` passes

## Prior Art

Old code in `../t-koma`:

- `t-koma-gateway/src/model_registry.rs` (lines 119-164) — Kimi Code provider
  instantiation with User-Agent header handling. Directly reusable logic.
- `t-koma-gateway/src/providers/openai_compatible/client.rs` — The OpenAI-compatible
  client that Kimi Code wraps. Directly reusable.
- `t-koma-gateway/tests/kimi_code_live.rs` — Live integration test. Reusable test
  structure.
- `docs/providers/kimi_code.md` — Provider documentation with setup instructions.
