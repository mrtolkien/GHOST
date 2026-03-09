---
name: fix-feedback
description: >-
  Triage and fix issues reported via Ghost's /feedback command. Use when the user points
  you to a feedback folder or asks you to fix a feedback report. Reads the pre-rendered
  feedback.md and transcript.md to understand and fix the issue.
---

# Fix Feedback

## Getting Feedback

Feedback is saved on the running GHOST instance, not locally. To retrieve it:

```bash
# SSH to the GHOST server and list available feedback
ssh root@192.168.1.13 'ls ~/GHOST/feedback/'

# Copy the feedback folder locally
scp -r root@192.168.1.13:~/GHOST/feedback/<folder-name>/ /tmp/ghost-feedback/
```

The user will typically give you the folder name. Each folder contains `feedback.md`,
`transcript.md`, and `ghost.db`.

## Process

1. Read `feedback.md` in the feedback folder for the issue description and session ID
2. Read `transcript.md` for the last 10 messages with tool calls and results
3. Analyze the conversation: what the OPERATOR said, what GHOST did, what went wrong
4. Categorize the root cause:
   - **Bad tool use**: wrong tool chosen, bad parameters, missing tool
   - **Bad response**: tone, content, format issues
   - **UI problem**: Discord rendering, embed issues
   - **Prompt issue**: system prompt missing context, wrong instructions
   - **Code bug**: tool implementation, chat loop, provider issue
5. Locate the relevant source files and propose a fix
6. Implement the fix, run `just ci`

## Reading the transcript

`transcript.md` contains messages in chronological order. Pay attention to:

- Tool call names and arguments — did GHOST pick the right tool?
- Tool results — did the tool return what was expected?
- The sequence of messages leading up to the issue
- System messages that may have influenced behavior

If the transcript is insufficient, `ghost.db` is in the same folder. You can query it
with a Python script (use @uv-scripts conventions):

```python
import sqlite3
conn = sqlite3.connect("path/to/ghost.db")
# Query any table: session, message, knowledge, etc.
```
