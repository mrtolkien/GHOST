We need a good onboarding flow:

- Check nix install (or install nix ourself?)
- Setup model provider
  - The model could be used for some questions during the onboarding if there's a need
    to debug things?
- Setup embeddings
- Setup discord (bot token + approved user id)
- Setup tailscale (both host and clients)
- Setup opentelemetry -> Optional

---

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

---

- Onboarding should include oauth sync
- Onboarding/cli config picker should properly list available models for all providers
  - For example, get top models on openrouter, ...
  - Check model-picker spec
- Onboarding/deployment should work on Linux with all GPU types (Nvidia, AMD, Intel,
  ...)

---

Create a clean services list and docker compose file to be included in the binary and
deploy it with podman rootless by default (or docker if available):

- crawl4ai
- searxng (also possible native with nix, but no gain?)
- Headless chrome w/ CDP

Native would be better for:

- Docling (maybe even use the CLI? Can we install it with nix as part of the flake?)
- Llama.cpp
