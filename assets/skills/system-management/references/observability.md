# SigNoz Observability

GHOST emits OpenTelemetry traces, metrics, and logs for every meaningful operation.
SigNoz is the self-hosted backend that collects and visualises this data. You do not
need SigNoz to run GHOST — it is optional but very useful for understanding what GHOST
is doing and diagnosing problems.

## What SigNoz Provides

- **Traces** — distributed traces for every chat turn, tool call, agent run, and web
  fetch. See exactly what happened, how long each step took, and where errors occurred.
- **Metrics** — request rates, latency percentiles, error rates over time.
- **Logs** — structured log ingestion correlated with traces.
- **Dashboards** — custom dashboards and alerting on any signal.

Access the UI at: **http://localhost:3301**

## Compose File

Add a SigNoz stack to `$WORKSPACE/services/docker-compose.yml` or run it as a separate
compose project. The minimal self-hosted stack uses ClickHouse as the storage backend.

Reference compose file for the SigNoz all-in-one container (quickstart mode):

```yaml
services:
  signoz:
    image: signoz/signoz:latest
    container_name: signoz
    ports:
      - "3301:3301" # Web UI
      - "4317:4317" # OTLP gRPC receiver
      - "4318:4318" # OTLP HTTP receiver
    volumes:
      - signoz-data:/var/lib/signoz
    restart: unless-stopped

volumes:
  signoz-data:
```

For the full production stack (ClickHouse + query service + frontend as separate
containers), refer to the official SigNoz repository:
https://github.com/SigNoz/signoz/tree/main/deploy/docker

## Configuration

Set the OTLP endpoint in `$WORKSPACE/.env` so GHOST sends telemetry to SigNoz:

```
OTEL_EXPORTER_OTLP_ENDPOINT=http://127.0.0.1:4317
```

GHOST reads this at startup. Restart the daemon after changing `.env`:

```
ghost config reload
```

Or if using systemd:

```
systemctl --user restart ghost-daemon
```

## Start / Stop SigNoz

If SigNoz is included in `$WORKSPACE/services/docker-compose.yml`:

```
# Start
cd $WORKSPACE/services && podman compose up -d signoz

# Stop
cd $WORKSPACE/services && podman compose stop signoz

# View logs
cd $WORKSPACE/services && podman compose logs -f signoz
```

If running as a separate compose project:

```
cd ~/signoz && podman compose up -d
cd ~/signoz && podman compose down
```

## Key Spans to Look For

When reviewing traces in the SigNoz UI, these are the most informative spans:

| Span name           | What it represents                              |
| ------------------- | ----------------------------------------------- |
| `chat.turn`         | A single OPERATOR message + GHOST response      |
| `tool.call.<name>`  | Execution of a specific tool (shell, search, …) |
| `agent.run.<name>`  | A background agent tick                         |
| `web.fetch`         | HTTP fetch of a URL (web cache or live)         |
| `web.search`        | SearXNG search query                            |
| `embeddings.upsert` | Embedding a knowledge item                      |
| `db.query`          | SQLite query (appears as child of above spans)  |

## Verifying Telemetry is Flowing

1. Open http://localhost:3301 in the OPERATOR's browser
2. Navigate to **Services** — `ghost` should appear within a minute of daemon start
3. Navigate to **Traces** — recent chat turns will appear as root spans

If no data appears after a few minutes, check:

```
# Verify OTLP endpoint is set
ghost config get

# Check daemon logs for exporter errors
journalctl --user -u ghost-daemon --since "5 minutes ago" | grep -i otel
```

## Disabling Telemetry

Remove or unset `OTEL_EXPORTER_OTLP_ENDPOINT` from `.env` and reload config. GHOST will
continue running without emitting telemetry.
