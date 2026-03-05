---
title: Overview
description:
  GHOST HACK — summon the Puppet Master to take over your GHOST and work
  directly in a repo.
---

GHOST HACK summons the **Puppet Master** — a superior coding entity that takes
over your GHOST. When hacked, the GHOST steps aside. Your Discord channel
becomes a direct line to the Puppet Master, which reads, edits, runs commands,
and commits in your repository.

Unlike background [agents](/agents/introduction/) that run autonomously, the
Puppet Master is interactive. It asks questions, waits for your input, and
reports progress. Think pair programming with something that knows your entire
codebase.

## Lifecycle

```text
You: "Hack on my-project, fix the failing tests"
  → GHOST reads the coding skill
  → GHOST runs `ghost hack start code/my-project --prompt "fix failing tests"`
  → The Puppet Master takes over (new session + channel takeover)
  → You talk directly to the Puppet Master
  → You work together until you send /kill
  → The Puppet Master releases the GHOST, posts a git summary
```

## Starting a Session

### Via GHOST (Recommended)

Ask your GHOST to hack on a repo. It will identify the repo path from your
[references](/knowledge/knowledge/) and summon the Puppet Master:

```
You: Hack on my-project, the tests in auth/ are broken
```

GHOST reads the `coding` skill, figures out the repo location, and runs the CLI
command with an appropriate initial prompt. Then it steps aside.

### Via CLI

```bash
ghost hack start <dir> --prompt "fix the auth tests"
ghost hack start code/my-project
```

The `dir` is relative to the workspace (or absolute). The optional `--prompt`
seeds the first message so the Puppet Master starts working immediately.

## Channel Takeover

When the Puppet Master takes over, the Discord channel is **hacked**. Every
message you send goes to the Puppet Master, not your GHOST.

On the first message you'll see:

> **GHOST HACKED** — you're now talking to the Puppet Master.
> Send `/kill` to end the session.

The takeover is per-channel. Other channels continue to talk to your GHOST
normally.

## Ending a Session

Send `/kill` in the hacked channel. The Puppet Master will:

1. Generate a git summary (branch, recent commits, files changed)
2. Post the summary to the channel
3. Release the GHOST, returning the channel to normal

## Resuming a Session

Previous sessions can be resumed:

```bash
ghost hack resume <session_id> --prompt "continue where we left off"
```

The Puppet Master returns with the full conversation history preserved.

## Listing Sessions

```bash
ghost hack list
```

Shows recent sessions with their status, working directory, and start time.
Active sessions are marked with `*`.
