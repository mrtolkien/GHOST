# Chat & Sessions

## Sessions

A session is a chat thread between you (the OPERATOR) and your GHOST. Each Discord
thread maps to one session.

Currently, there's only one session per GHOST and OPERATOR since there is a single
interface.

Sessions persist in the database — your GHOST can query the full conversation history.

## Message Flow

```
You send a message
  → System prompt assembled (identity + skills + agents + context)
  → GHOST processes with tool loop (up to 20 iterations)
  → GHOST calls tools as needed (search, web, files, etc.)
  → Final response sent back
```

The GHOST can chain multiple tool calls in a single turn — for example, searching the
knowledge base, then fetching a web page, then composing a response.

## Compaction

When the conversation approaches the model's context window limit (default: 85% full),
GHOST automatically **compacts** the history:

- Older messages are summarized into a compact form
- The most recent messages (default: 20) are kept in full
- Compaction is transparent — the GHOST continues the conversation naturally

Configure in `config.toml`:

```toml
[compaction]
threshold = 0.85 # Trigger at 85% context usage
keep_window = 20 # Keep last 20 messages intact
mask_preview_chars = 100
```

## Session Management

```bash
# List recent sessions
ghost session list

# View messages in a session
ghost session logs <session_id>
ghost session logs <session_id> --count 100
ghost session logs <session_id> --around <message_id>
```
