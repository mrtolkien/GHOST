## Status: Mostly addressed

`run_shell_command` supports `background: true` — the import runs detached and posts a
`[shell-command completed]` system message on completion. The completion watcher
(`src/daemon/completion_watcher.rs`) automatically triggers a follow-up chat turn and
sends the result to Discord.

### What's done

- Background shell mode (no timeout, result as system message)
- Completion watcher: consumes `CompletionEvent` channel, waits for session idle,
  triggers continuation chat turn, sends result to Discord
- Skill instructs model to end turn after starting import; watcher handles the rest
- Same event-driven pattern as the agent watcher

### Remaining gaps

- **Resumability**: if the daemon restarts mid-import, the spawned task is lost. The
  embedding pipeline can resume (it skips already-embedded content), but the
  `[shell-command completed]` message is never posted and no event is fired.
