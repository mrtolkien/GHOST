# CLI Reference

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

Read and write configuration values.

```bash
ghost config get <key>          # Get a config value (with defaults)
ghost config set <key> <value>  # Set a config value
```

Examples:

```bash
ghost config get discord.allowed_user_id
ghost config set timing.heartbeat_idle_minutes 10
ghost config set web.search_max_results 10
```

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

## ghost web

Web search and fetch.

```bash
ghost web search <query>                         # Search with Brave
ghost web search <query> -n 10                   # More results
ghost web fetch <url>                            # Extract content
ghost web fetch <url> --readability              # Article mode
ghost web fetch <url> --raw                      # Raw HTML
```

## ghost version

Print the GHOST version.

```bash
ghost version
```
