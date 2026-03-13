Onboarding wizard + external service management. Deferred until core install/update flow
is solid.

## Onboarding (`ghost init` interactive setup)

- LLM provider selection (OpenRouter, Kimi, OpenAI OAuth) + API key
- Discord token + user ID
- For each service (embeddings, search, crawl, docling): ask "local Docker / remote URL
  / skip?"
- Generate `config.toml` + `.env` + `docker-compose.yml` from answers
- Replace `deploy/common/onboard.py` with native Rust implementation

## Service management

- Ghost should manage its own sidecar services (start/stop/restart containers)
- Either a skill that teaches Ghost to run compose commands, or a `ghost stack` CLI
- Health checks: Ghost detects when a service goes down, notifies operator
- See also: `deployment_per_platform.md` for per-platform service fallback chains
  (Firecrawl, Brave API, remote embeddings, etc.)
