# Docker Packaging

Docker is **the** installation method for GHOST. The image bundles the binary with a Nix
runtime for deterministic shell tool management.

## Architecture

```
docker-compose.yml         # Production: pulls mrtolkien/ghost:latest from DockerHub
docker-compose.local.yml   # Dev override: builds from local source
docker/
├── Dockerfile             # Multi-stage: rust builder → nixos/nix runtime
├── default-flake.nix      # Default workspace shell flake (copied at bootstrap)
├── entrypoint.sh          # exec ghost daemon "$@"
└── searxng-settings.yml   # SearXNG config (JSON output, curated engines)
```

## Image

- **Builder stage**: `rust:1.85-bookworm` — compiles + strips the binary
- **Runtime stage**: `nixos/nix:latest` — copies binary, enables flakes, pre-warms Nix
  store with default packages
- Multi-platform: `linux/amd64` + `linux/arm64` via `docker buildx`
- CI pushes to DockerHub on version tags (`.github/workflows/docker.yml`)

## Compose Services

| Service  | Image                       | Purpose             |
| -------- | --------------------------- | ------------------- |
| ghost    | `mrtolkien/ghost:latest`    | GHOST daemon        |
| crawl4ai | `unclecode/crawl4ai:latest` | Web page extraction |
| searxng  | `searxng/searxng:latest`    | Web search          |

All services on a shared `ghost-net` bridge network. No ports exposed to host by
default.

## Volumes

- `$GHOST_WORKSPACE` → `/workspace` (bind mount)
- `$GHOST_CONFIG` → `/config` (bind mount)
- `nix-store` → `/nix` (named volume, persists packages across restarts)

## Nix Shell Wrapping

Each `run_shell_command` is wrapped in
`nix develop $WORKSPACE/shell/ --command sh -c "..."`. Falls back to direct `sh -c` if
no flake exists. GHOST can self-extend its toolset by editing the flake — changes take
effect on the next command.

## Service Discovery

`CRAWL4AI_URL` and `SEARXNG_URL` env vars are set by docker-compose pointing to
container hostnames. Config.toml values override env vars if set.
