# Services Reference

## Health Checks

`ghost status` runs these probes automatically. Use the table for manual
troubleshooting.

| Service      | Endpoint                           | Expected response           |
| ------------ | ---------------------------------- | --------------------------- |
| llama-server | http://127.0.0.1:11434/health      | `{"status":"ok"}` or 200 OK |
| SearXNG      | http://127.0.0.1:8080              | HTML search page (200 OK)   |
| Chrome       | http://127.0.0.1:9222/json/version | JSON with browser version   |
| Crawl4AI     | http://127.0.0.1:11235/health      | `{"status":"healthy"}`      |
| Docling      | http://127.0.0.1:5001/health       | `{"status":"ok"}` or 200 OK |

## Troubleshooting

**Port conflict** -- find the conflicting process and stop it or reconfigure via
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

**Llama-server not loading a model** -- verify the model is present and pull if missing:

```
curl http://127.0.0.1:11434/api/tags
ollama pull <model-name>
```

**Containers keep restarting** -- check for OOM or image pull failures:

```
podman events --filter event=die --since 10m
```
