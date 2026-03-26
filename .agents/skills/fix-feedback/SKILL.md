---
name: fix-feedback
description: >-
  Triage and fix issues reported via Ghost's /feedback command. MUST READ when the user
  points you to a feedback folder, mentions a feedback report, or asks you to
  investigate a bug that happened during a GHOST session. Covers: retrieving feedback
  from remote servers, reading transcripts and debug request dumps, root-cause
  categorization, and the fix workflow.
---

# Fix Feedback

## Getting Feedback

Feedback is saved on running GHOST instances, not locally. GHOST runs on servers
`192.168.1.3` through `192.168.1.7`. The user will typically tell you which server and
folder name. If they don't specify the server, ask.

```bash
# List available feedback on a server
ssh root@192.168.1.X 'ls ~/GHOST/feedback/'

# Copy the feedback folder locally
scp -r root@192.168.1.X:~/GHOST/feedback/<folder-name>/ /tmp/ghost-feedback/
```

Each feedback folder contains:

| File            | Contents                                                      |
| --------------- | ------------------------------------------------------------- |
| `feedback.md`   | Issue description, session ID, operator comment               |
| `transcript.md` | Last 10 messages with tool calls and results                  |
| `ghost.db`      | Full SQLite database snapshot (sessions, messages, knowledge) |

## Process

1. Read `feedback.md` for the issue description and **session ID**
2. Read `transcript.md` for the conversation leading to the issue
3. If the issue looks like a provider/API problem, pull debug request dumps (see below)
4. Analyze: what the OPERATOR asked, what GHOST did, what went wrong
5. Categorize the root cause:
   - **Bad tool use**: wrong tool chosen, bad parameters, missing tool
   - **Bad response**: tone, content, format issues
   - **UI problem**: Discord rendering, embed issues
   - **Prompt issue**: system prompt missing context, wrong instructions
   - **Code bug**: tool implementation, chat loop, provider issue
   - **Provider issue**: wrong model output, empty response, malformed API response
6. Locate the relevant source files and propose a fix
7. Implement the fix, run `just ci`

## Reading the Transcript

`transcript.md` contains messages in chronological order. Pay attention to:

- Tool call names and arguments — did GHOST pick the right tool?
- Tool results — did the tool return what was expected?
- The sequence of messages leading up to the issue
- System messages that may have influenced behavior

## Debug Request Dumps

When `debug.save_requests = true` is enabled (it is on all servers), raw LLM
request/response pairs are saved as JSON files in `~/GHOST/debug/requests/`.

**Filename format**: `{timestamp}_{session_id_last8}_{iteration}.json`

Use the session ID from `feedback.md` to find matching files:

```bash
# Find debug dumps for a specific session (last 8 chars of session ID)
ssh root@192.168.1.X 'ls ~/GHOST/debug/requests/ | grep <last-8-chars>'

# Copy them locally
scp root@192.168.1.X:~/GHOST/debug/requests/*<last-8-chars>*.json /tmp/ghost-feedback/
```

Each JSON file contains:

```json
{
  "timestamp": "...",
  "provider": "openrouter",
  "model": "claude-opus-4-6",
  "session_id": "...",
  "iteration": 3,
  "status": 200,
  "duration_ms": 4521,
  "request": { "...full request body..." },
  "response": { "...full response body..." }
}
```

**When to check debug dumps:**

- Empty or nonsensical GHOST responses (check if the API returned an error or empty
  content)
- Suspiciously wrong tool calls (check if the request included the right tools/system
  prompt)
- Rate limiting or timeout issues (check `status` and `duration_ms`)
- Model mismatch suspicions (check `model` vs what was configured)

Note: files are pruned to the last 500 by default (`debug.max_saved_requests`), so very
old sessions may no longer have dumps available.

## Querying the Database

If the transcript is insufficient, query `ghost.db` with a uv script (see `/uv-scripts`
skill). Useful queries:

```python
# All messages for a session
cursor.execute("""
    SELECT role, content, tool_calls, created_at
    FROM message WHERE session_id = ? ORDER BY created_at
""", (session_id,))

# Recent sessions with their message counts
cursor.execute("""
    SELECT s.id, s.title, COUNT(m.id) as msg_count, s.created_at
    FROM session s LEFT JOIN message m ON s.id = m.session_id
    GROUP BY s.id ORDER BY s.created_at DESC LIMIT 20
""")
```

## Rules

- **Never modify a test to make it pass.** Fix the code under test instead.
- **Never weaken assertions.** If a test catches the bug, that's working as intended.
- Always run `just ci` before considering the fix complete.
