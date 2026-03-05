---
title: Configuration
description:
  Configuring GHOST HACK — model selection, available tools, and CLI reference.
---

## Config

```toml title="~/.config/ghost/config.toml"
[coding]
model = "fast"  # Use a different model alias for the Puppet Master
```

The `model` field is optional. If set, the Puppet Master uses this
[model alias](/ghost/providers/) instead of the default. This lets you use a
faster or cheaper model for interactive coding while keeping a stronger model
for your GHOST's main chat.

If not set, the Puppet Master uses `[models].default`.

## Available Tools

The Puppet Master has access to the same tools as your GHOST:

| Tool               | Purpose                                |
| ------------------ | -------------------------------------- |
| `read_file`        | Read file contents                     |
| `write_file`       | Create new files                       |
| `file_edit`        | Targeted edits to existing files       |
| `run_shell_command` | Builds, tests, git, any shell command |
| `search_notes`     | Query the knowledge base               |
| `search_references` | Search reference documents            |
| `web_search`       | Search the web                         |
| `web_fetch`        | Fetch and extract web page content     |

All file paths resolve relative to the session's working directory. The Puppet
Master can't accidentally edit files outside the repo.

## CLI Reference

### `ghost hack start`

Summon the Puppet Master for a new coding session.

```bash
ghost hack start <dir> [--prompt "task"] [--channel-id <id>]
```

| Argument        | Description                                           |
| --------------- | ----------------------------------------------------- |
| `dir`           | Working directory (relative to workspace or absolute) |
| `--prompt`      | Initial message for the Puppet Master                 |
| `--channel-id`  | Discord channel ID for takeover (set by GHOST)        |

Prints the `coding_session_id`, `session_id`, and `working_dir` on success.

### `ghost hack resume`

Resummon the Puppet Master for a previous session.

```bash
ghost hack resume <session_id> [--prompt "task"] [--channel-id <id>]
```

The Puppet Master returns with the full conversation history preserved.

### `ghost hack list`

List recent coding sessions.

```bash
ghost hack list
```

Shows up to 10 recent sessions. Active sessions are marked with `*`.
