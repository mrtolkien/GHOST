---
name: services
description:
  Manage GHOST's infrastructure services. Use when you need to start, stop, restart,
  or troubleshoot any service (containers or native), check service health, or help
  the OPERATOR modify their service setup.
---

# Services Manager

## Architecture Overview

GHOST's infrastructure is split into two service tiers:

**Native services** — installed via Nix profile, managed by the OS process supervisor:

- `ghost-daemon` — the GHOST process itself
- `llama-server` — local LLM inference (Ollama)
- `docling-serve` — document parsing (PDF, DOCX, etc.)

**Container services** — managed by Podman or Docker Compose:

- `searxng` — privacy-respecting meta-search engine
- `crawl4ai` — AI-optimised web crawler
- `chrome` — headless Chrome for browser-use tools

## File Layout

```
$WORKSPACE/services/
├── docker-compose.yml       # container stack definition
└── searxng-settings.yml     # SearXNG configuration

# Linux (systemd user units)
~/.config/systemd/user/
├── ghost-daemon.service
├── llama-server.service
└── docling-serve.service

# macOS (launchd plists)
~/Library/LaunchAgents/
├── com.ghost.daemon.plist
├── com.ghost.llama-server.plist
└── com.ghost.docling-serve.plist
```

## Container Operations

All container commands run from `$WORKSPACE/services/`. The compose file name is
`docker-compose.yml`. Use `podman compose` or `docker compose` depending on what the
OPERATOR has installed — prefer `podman compose` on Linux.

### Start all containers

```
cd $WORKSPACE/services && podman compose up -d
```

### Stop all containers

```
cd $WORKSPACE/services && podman compose down
```

### Restart a single container

```
cd $WORKSPACE/services && podman compose restart searxng
cd $WORKSPACE/services && podman compose restart crawl4ai
cd $WORKSPACE/services && podman compose restart chrome
```

### View container logs

```
# Follow logs for a specific service
cd $WORKSPACE/services && podman compose logs -f searxng

# Last 100 lines
cd $WORKSPACE/services && podman compose logs --tail=100 crawl4ai

# All services
cd $WORKSPACE/services && podman compose logs
```

### Check container status

```
cd $WORKSPACE/services && podman compose ps
```

## Native Service Operations (Linux — systemd)

### Start / stop / restart

```
systemctl --user start ghost-daemon
systemctl --user stop ghost-daemon
systemctl --user restart ghost-daemon

systemctl --user start llama-server
systemctl --user restart llama-server

systemctl --user start docling-serve
systemctl --user restart docling-serve
```

### Check status

```
systemctl --user status ghost-daemon
systemctl --user status llama-server
systemctl --user status docling-serve
```

### View logs

```
journalctl --user -u ghost-daemon -f
journalctl --user -u llama-server --since "10 minutes ago"
```

### Enable at login

```
systemctl --user enable ghost-daemon
systemctl --user enable llama-server
systemctl --user enable docling-serve
```

## Native Service Operations (macOS — launchd)

### Load / unload

```
launchctl load ~/Library/LaunchAgents/com.ghost.daemon.plist
launchctl unload ~/Library/LaunchAgents/com.ghost.daemon.plist
```

### Start / stop

```
launchctl start com.ghost.daemon
launchctl stop com.ghost.daemon
```

### View logs

```
log stream --predicate 'subsystem == "com.ghost.daemon"' --level debug
```

## Health Checks

Use these to verify a service is up and responding before relying on it.

| Service       | Endpoint                              | Expected response              |
| ------------- | ------------------------------------- | ------------------------------ |
| llama-server  | http://127.0.0.1:11434/health         | `{"status":"ok"}` or 200 OK    |
| SearXNG       | http://127.0.0.1:8080                 | HTML search page (200 OK)      |
| Chrome        | http://127.0.0.1:9222/json/version    | JSON with browser version      |
| Crawl4AI      | http://127.0.0.1:11235/health         | `{"status":"healthy"}`         |
| Docling       | http://127.0.0.1:5001/health          | `{"status":"ok"}` or 200 OK    |

Quick health check one-liner for all container services:

```
for url in \
  "http://127.0.0.1:11434/health" \
  "http://127.0.0.1:8080" \
  "http://127.0.0.1:9222/json/version" \
  "http://127.0.0.1:11235/health" \
  "http://127.0.0.1:5001/health"; do
  echo -n "$url: "; curl -sf "$url" > /dev/null && echo OK || echo FAIL
done
```

## Adding or Removing Container Services

1. Edit `$WORKSPACE/services/docker-compose.yml` — add or remove a service block
2. Apply the change:

```
cd $WORKSPACE/services && podman compose up -d
```

Compose will start new services and leave unchanged ones alone. To remove a service
that was deleted from the compose file:

```
cd $WORKSPACE/services && podman compose down
cd $WORKSPACE/services && podman compose up -d
```

## Reconfiguring Services

If the OPERATOR wants to change ports, credentials, resource limits, or which optional
services are enabled, re-run the setup wizard:

```
ghost init
```

The wizard will read the current configuration and let the OPERATOR change only what
they need. Existing services will be reconfigured in place.

## SearXNG Configuration

The SearXNG settings file at `$WORKSPACE/services/searxng-settings.yml` controls search
engines, UI language, and privacy settings. Edit it directly, then restart the container:

```
cd $WORKSPACE/services && podman compose restart searxng
```

Common adjustments:
- Enable/disable specific search engines under `engines:`
- Change `search.default_lang` for localised results
- Set `server.secret_key` if it was not generated during init

## Troubleshooting

**Container fails to start** — check logs first:

```
cd $WORKSPACE/services && podman compose logs <service-name>
```

**Port conflict** — another process may be using the port. Find it:

```
ss -tlnp | grep <port>       # Linux
lsof -i :<port>              # macOS
```

Then stop the conflicting process or reconfigure the port via `ghost init`.

**Llama-server not loading a model** — verify the model is pulled:

```
curl http://127.0.0.1:11434/api/tags
```

Pull a missing model:

```
ollama pull <model-name>
```

**Containers keep restarting** — container runtime OOM or image pull failure. Check:

```
podman events --filter event=die --since 10m
```

## Nix Garbage Collection

Over time, old Nix store paths accumulate. To reclaim disk space:

```
nix-collect-garbage -d
```

This removes all generations of all profiles and deletes unreachable store paths. Run
it when disk usage is high. After GC, verify ghost still works:

```
ghost version
```

## Optional Extras

- `observability.md` — SigNoz traces, metrics, and logs via OpenTelemetry
- `tailscale.md` — secure remote access to GHOST services over Tailscale
