---
name: services
description:
  Manage GHOST's infrastructure services. Use when you need to start, stop, restart, or
  troubleshoot any service (containers or native), check service health, or help the
  OPERATOR modify their service setup.
---

# Services Manager

## Architecture Overview

GHOST's infrastructure is split into two tiers: **native services** (ghost-daemon,
llama-server, docling-serve) managed by the OS process supervisor (systemd on Linux,
launchd on macOS), and **container services** (searxng, crawl4ai, chrome) managed by
Podman or Docker Compose.

## CLI Commands

These are the primary commands for managing services. Prefer them over raw
systemctl/launchctl/compose commands.

```
ghost start                  # start all services and the daemon
ghost stop                   # stop the daemon and all services

ghost services list           # show registered services and their state
ghost services add            # register a new service (interactive)
ghost services remove <name>  # unregister a service
ghost services update         # pull updates and restart all services (stops on first failure)
ghost services status         # check process-level status for all services

ghost status                  # check config validity + HTTP health probes (complementary)
```

To reconfigure ports, credentials, or which services are enabled: `ghost init`.

## Health Checks

`ghost status` runs these probes automatically. Use the table for manual troubleshooting.

| Service      | Endpoint                           | Expected response           |
| ------------ | ---------------------------------- | --------------------------- |
| llama-server | http://127.0.0.1:11434/health      | `{"status":"ok"}` or 200 OK |
| SearXNG      | http://127.0.0.1:8080              | HTML search page (200 OK)   |
| Chrome       | http://127.0.0.1:9222/json/version | JSON with browser version   |
| Crawl4AI     | http://127.0.0.1:11235/health      | `{"status":"healthy"}`      |
| Docling      | http://127.0.0.1:5001/health       | `{"status":"ok"}` or 200 OK |

## Troubleshooting

**Port conflict** — find the conflicting process and stop it or reconfigure via
`ghost init`:

```
ss -tlnp | grep <port>   # Linux
lsof -i :<port>          # macOS
```

**View logs:**

```
# Linux (systemd)
journalctl --user -u ghost-daemon -f
journalctl --user -u llama-server --since "10 minutes ago"

# macOS (launchd)
log stream --predicate 'subsystem == "com.ghost.daemon"' --level debug

# Container services
cd $WORKSPACE/services && podman compose logs -f <service>
cd $WORKSPACE/services && podman compose logs --tail=100 <service>
```

**Llama-server not loading a model** — verify the model is present and pull if missing:

```
curl http://127.0.0.1:11434/api/tags
ollama pull <model-name>
```

**Containers keep restarting** — check for OOM or image pull failures:

```
podman events --filter event=die --since 10m
```

**Nix garbage collection** — reclaim disk space when usage is high:

```
nix-collect-garbage -d
ghost version   # verify ghost still works after GC
```

## Optional Extras

- `observability.md` — SigNoz traces, metrics, and logs via OpenTelemetry
- `tailscale.md` — secure remote access to GHOST services over Tailscale
