## Status: Partially addressed

`run_shell_command` now supports `background: true` — the import runs detached and posts
a `[shell-command completed]` system message on completion. The `reference-import` skill
uses this for all git/crawl imports.

### What's done

- Background shell mode (no timeout, result as system message)
- Skill instructs model to tell the OPERATOR and wait
- Model sees the completion on the next conversation turn

### Remaining gaps

- **No auto-trigger**: unlike agent completions (which have a watcher that triggers a
  follow-up chat turn + Discord notification), background shell completions just sit in
  the DB until the user sends another message. For the PoC this is fine — the model
  tells the user it's importing and they ask a follow-up when ready.
- **No Discord notification**: the agent watcher sends a compact summary to Discord when
  an agent finishes. Background shell commands don't notify Discord at all.
- **Resumability**: if the daemon restarts mid-import, the spawned task is lost. The
  embedding pipeline can resume (it skips already-embedded content), but the
  `[shell-command completed]` message is never posted.

### Future: shell command watcher

To match the agent watcher pattern, we could:

1. Track background shell tasks in a registry (like `AgentRunner`)
2. Extend the daemon watcher to detect completions and trigger follow-up turns
3. Send Discord notifications on completion

This is not needed for the PoC but would improve the UX for long-running imports.
