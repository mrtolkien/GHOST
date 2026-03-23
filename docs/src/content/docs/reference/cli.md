---
title: CLI Reference
description: Complete reference for all ghost CLI commands.
tableOfContents:
  maxHeadingLevel: 3
---

## ghost daemon

Start the GHOST daemon (Discord bot + job scheduler).

```bash
ghost daemon
```

## ghost init

Initialize workspace and config files.

```bash
ghost init
```

Creates `~/.config/ghost/config.toml` and the workspace directory with default files.

## ghost config

Read, write, and reload configuration values.

```bash
ghost config get <key>          # Get a config value (with defaults)
ghost config set <key> <value>  # Set a config value
ghost config reload             # Validate config and signal the daemon to reload
```

Examples:

```bash title="Config examples"
ghost config get discord.allowed_user_id
ghost config set timing.heartbeat_idle_minutes 10
ghost config set web.search_max_results 10
ghost config reload
```

### ghost config reload

Validates `config.toml` and `.env`, then sends `SIGHUP` to a running daemon to apply
changes without restarting.

```bash
ghost config reload
```

**Hot-reloadable** — the daemon picks up these changes immediately:

- `model` — default chat model
- `timing` — heartbeat, idle, and compaction intervals
- `compaction` — token thresholds and limits
- `web` — search provider, result counts
- `docling` — document import settings
- `coding` — coding session settings
- `debug` — log level and debug flags
- `discord.allowed_user_ids` — who can interact with GHOST
- `embeddings.url` and `embeddings.model` — embedding endpoint and model name

**Requires restart** — these settings are read once at startup:

- `workspace` — workspace directory path
- `embeddings.dimension` — vector size (changing this invalidates stored embeddings)
- `discord.enabled` — enables or disables the Discord interface

:::caution[Embeddings model change]
Changing `embeddings.model` without also changing `embeddings.dimension` may produce
embeddings that are incompatible with existing ones in the database. Run
`ghost knowledge reindex` after changing the model to regenerate all embeddings.
:::

## ghost auth

Manage authentication for providers.

```bash
ghost auth codex     # OpenAI OAuth flow (opens browser)
ghost auth status    # Check authentication status
ghost auth revoke    # Revoke OpenAI tokens
```

## ghost job

Manage cron jobs.

```bash
ghost job list                   # List jobs with next-run times
ghost job validate <path>        # Validate job file syntax
ghost job run <name>             # Execute job immediately
ghost job logs [name]            # View job execution logs
```

## ghost hack

Manage coding sessions. See [GHOST HACK](/ghost-hack/overview/) for details.

```bash
ghost hack start <dir> [--prompt "task"]   # Start a coding session
ghost hack resume <id> [--prompt "task"]   # Resume a previous session
ghost hack list                            # List recent sessions
```

## ghost session

Inspect chat sessions.

```bash
ghost session list                              # Recent sessions (up to 50)
ghost session logs <session_id>                 # View messages
ghost session logs <session_id> --count 100     # More messages
ghost session logs <session_id> --around <id>   # Center on a message
```

## ghost knowledge

Query and manage the knowledge base.

### Search

```bash
ghost knowledge search <query>
ghost knowledge search <query> --kind note      # Filter by type
ghost knowledge search <query> --limit 20       # More results
```

### Read

```bash
ghost knowledge get --title "Note Title"        # Get by title
ghost knowledge get <path>                      # Get by path
```

### Graph

```bash
ghost knowledge graph <target>                  # Show connections
ghost knowledge graph <target> --direction out   # Outgoing only
ghost knowledge graph --orphans                  # Unconnected notes
ghost knowledge graph --stats                    # Edge/stub counts
```

### Browse

```bash
ghost knowledge tags                             # Tags with counts
ghost knowledge recent                           # Recent activity
ghost knowledge recent --limit 50
ghost knowledge stats                            # Type counts
ghost knowledge references                       # All reference topics
ghost knowledge references --topic rust          # By topic
```

### Maintenance

```bash
ghost knowledge reindex                          # Sync files → database
ghost knowledge reindex --skip-embeddings        # Skip embedding gen
```

## ghost reference

Import, update, and delete external reference material. See
[Reference Import](/knowledge/reference-import/) for details.

### Import

```bash
# From a git repository (preferred for doc sets)
ghost reference import git --url <url> --topic <name> \
    [--paths dir1,dir2] [--extensions .md,.rs] [--ref <tag-or-branch>]

# By crawling a website (fallback)
ghost reference import crawl --url <url> --topic <name> \
    [--max-depth 3] [--max-pages 50]
```

### Update

```bash
ghost reference update --topic <name>              # Re-fetch from source
ghost reference update --topic <name> --ref v2.0   # Switch branch/tag
```

Re-fetches from the original source and applies changes. New files are
added, changed files are updated, and files deleted upstream are removed
— unless cited by notes, in which case they are moved to `_orphaned/`.

### Delete

```bash
ghost reference delete --topic <name>
```

Removes the topic, all its references, embeddings, import metadata, and
workspace files.

## ghost document

Import documents (PDF, DOCX, etc.) via docling-serve. See
[Reference Import](/knowledge/reference-import/) for details.

```bash
ghost document import file --path <path> --topic <name>
```

For URLs, download the file first (`curl -L -o uploads/doc.pdf <url>`), then
import with the command above.

Optional flags (use only when explicitly needed):

| Flag | Default | Purpose |
| --- | --- | --- |
| `--no-ocr` | OCR on | Skip OCR for digital PDFs |
| `--page-range "1-10"` | full doc | Import specific pages only |
| `--timeout 900` | 600s | Extend timeout for large docs |

## ghost web

Web search and fetch.

```bash
ghost web search <query>                         # Search with Brave
ghost web search <query> -n 10                   # More results
ghost web fetch <url>                            # Extract content
ghost web fetch <url> --readability              # Article mode
ghost web fetch <url> --raw                      # Raw HTML
```

## ghost status

Show config validity, daemon status, and service health at a glance.

```bash
ghost status
```

Checks:

- **Config** — loads and validates `config.toml`, reports parse errors
- **Daemon** — checks whether `ghost-daemon` is active via systemd/launchd and probes its health endpoint
- **Services** — probes each configured service URL (embeddings, search, Crawl4AI, Docling, browsers)

## ghost version

Print the GHOST version.

```bash
ghost version
```
