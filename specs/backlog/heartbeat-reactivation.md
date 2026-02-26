# Heartbeat Reactivation

Heartbeat was disabled because it wasn't working correctly:

- Default heartbeat should be empty (no run) — it was firing every minute
- Heartbeat runs didn't properly execute in the chat session context (way
  less tokens than expected)
- The idle-timeout + last-message check logic needs a full review

## Before re-enabling

1. Review the heartbeat spec and agent prompt (`prompts/agents/heartbeat.md`)
2. Decide on the correct default behavior: should the default heartbeat do
   nothing (require explicit opt-in), or should it run but with a much
   longer idle threshold?
3. Fix the session context issue — heartbeat runs should have the same
   token budget as regular chat
4. Re-examine the `HEARTBEAT_CONTINUE` suppression mechanism
5. Add proper integration tests that verify timing behavior
6. Unwire the disable in `src/daemon/run.rs` (search for "Heartbeat
   disabled")
