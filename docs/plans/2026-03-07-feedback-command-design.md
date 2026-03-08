# `/feedback` Command — Design

## What it does

Discord slash command: `/feedback description of the issue`

Snapshots the current session context to a workspace folder so the OPERATOR can scp it
to their dev machine and point Claude Code at it for fixing.

## On invocation

1. Resolve current session ID from the channel
2. Create `$WORKSPACE/feedback/<timestamp>-<slug>/` (slug = first 5 words slugified)
3. Write `feedback.md` — timestamp, session ID, issue description
4. Copy `$WORKSPACE/ghost.db` into the folder as `ghost.db`
5. Query the last 10 messages from the session. Write `transcript.md` with:
   - Role, timestamp, content for each message
   - Tool calls: name + arguments (truncated at 2000 chars each)
   - Tool results (truncated at 2000 chars each)
6. Reply ephemeral: "Feedback saved to `feedback/<folder>/`"

## Folder structure

```
feedback/
  2026-03-07T14-30-00-bad-tool-use/
    feedback.md       # Issue description + metadata
    transcript.md     # Last 10 messages with tool calls/results
    ghost.db          # Full DB snapshot for deeper queries if needed
```

## Claude Code skill: `fix-feedback`

- Triggered when user points to a feedback folder
- Reads `feedback.md` for issue description
- Reads `transcript.md` for the conversation context
- `ghost.db` available as fallback for deeper investigation
- Analyzes the conversation, identifies root cause, proposes and implements fix

## What's NOT included

- No new DB tables
- No status tracking (open/resolved) — just files
- No reaction-based capture — explicit command only
- No Python scripts — transcript is pre-rendered at feedback time
