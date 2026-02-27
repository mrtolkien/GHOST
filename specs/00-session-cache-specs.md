# Simple Session Cache

## Context

Every message triggers full reconstruction: DB load + JSON parse + file I/O (identity
files, skill discovery, agent discovery) + tool schema generation + full history clone +
provider serialization. This happens **every tool-loop iteration**, not just
per-message. A 5-iteration tool loop means 5x file reads, 5x skill dir scans.

Goal: active sessions live in memory. Reconstruct from DB only on cache miss (first
message, skill change, restart, compaction).

## Design

### New module: `src/chat/cache.rs`

```rust
pub struct SessionCache {
    sessions: Mutex<HashMap<String, CachedSession>>,
    prompt: Mutex<CachedPrompt>,
}

struct CachedSession {
    history: Vec<ChatMessage>,
    stored_message_ids: Vec<String>,
    generation: u32,                    // bumped on compaction
}

struct CachedPrompt {
    system_prompt: String,
    tool_schemas: Vec<ToolDefinition>,
    skills_hash: u64,                   // hash of skill mtimes
}
```

### SessionChat changes (`src/chat/session.rs`)

Add `cache: SessionCache` field to `SessionChat`. Since `SessionChat` is `Arc`-wrapped
at daemon level and `&self` during chat, the `Mutex` provides interior mutability.

**`chat()` flow becomes:**

```
1. Lock sessions, check cache hit (session_id exists)
   HIT:  clone history + stored_ids from cache, unlock
   MISS: load_provider_history() from DB, populate cache

2. Lock prompt, check skills_hash matches current skills
   HIT:  clone system_prompt from cache
   MISS: render_system_prompt(), update cache

3. Run tool loop (existing logic, but system_prompt comes from cache)

4. After tool loop returns:
   Lock sessions, write back updated history + stored_ids
```

Lock durations are microseconds (clone a Vec), never held across I/O or provider calls.

### Tool loop changes (`src/chat/tool_loop.rs`)

Currently `handler.system_prompt()` is called every iteration. Change:

- Pass pre-resolved system prompt to `run_tool_loop()` (new parameter)
- Remove per-iteration `handler.system_prompt()` call
- Tool schemas: pass pre-resolved `&[ToolDefinition]` instead of calling
  `tool_manager.all_tool_schemas()` every iteration

### Compaction integration (`src/chat/compaction.rs`)

When Phase 2 (LLM summarization) completes:

- Bump `CachedSession.generation`
- Replace cached history with the post-compaction history
- Update `prompt_cache_key` sent to provider: `"{session_id}:{generation}"`

Phase 1 (masking) only modifies the in-memory working copy, not the cache — masking is
re-applied each time from the cached history.

### Invalidation

| Trigger            | Action                                                   |
| ------------------ | -------------------------------------------------------- |
| Skill file change  | Clear prompt cache (check `skills_hash` on next request) |
| Compaction Phase 2 | Bump generation, clear response chain, rewrite history   |
| Session reboot     | Remove session from cache                                |
| Daemon restart     | Natural (cache is in-memory)                             |

### Skills hash (`src/skills.rs`)

Add `pub fn skills_hash(workspace: &Path) -> u64` — hash the sorted list of
`(skill_name, mtime)` pairs from the skills directory. Quick filesystem stat calls, no
file reads. Called once at start of `chat()` to check invalidation.

## Files to modify

| File                      | Changes                                                   |
| ------------------------- | --------------------------------------------------------- |
| `src/chat/cache.rs` (NEW) | `SessionCache`, `CachedSession`, `CachedPrompt`           |
| `src/chat/mod.rs`         | Re-export cache module                                    |
| `src/chat/session.rs`     | Add cache field, use cache in `chat()` flow               |
| `src/chat/tool_loop.rs`   | Accept pre-resolved prompt/tools, track response chaining |
| `src/chat/compaction.rs`  | Notify cache on Phase 2 completion                        |
| `src/providers/types.rs`  | Add `previous_response_id` to `ChatRequest`               |
| `src/skills.rs`           | Add `skills_hash()` function                              |

## Verification

1. `just ci` — ensure all existing tests pass
2. Manual test: send 2+ messages in same session via Discord, verify second message logs
   "cache hit" (add logfire span with cache_hit field)
3. Modify a skill file, send another message — verify prompt cache miss + re-render
