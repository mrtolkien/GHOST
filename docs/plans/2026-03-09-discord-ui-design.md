# Discord UI Overhaul Design

Addresses `specs/3_ui_discord.md` — readability, feedback, width, and visual polish.

## Problems

1. **Tool call display is noisy** — each iteration sends a separate container with raw
   `tool_name arg_summary` lines. Research-heavy turns produce 3-5 containers of
   scrolling before the actual answer.
2. **No feedback on running agents/background commands** — agent spawns are invisible
   until completion.
3. **Text uses ~50% width** — likely a Components v2 rendering quirk on desktop.
4. **URL auto-embeds are distracting** — Discord generates link previews for every URL
   in responses.

## Design

### 1. Tool Display Trait

A new trait that each tool implements to describe its UI representation:

```rust
pub trait ToolDisplay {
    /// Before execution: human-readable call summary
    fn display_request(&self, args: &Value) -> String;
    /// After execution: compact result hint
    fn display_result(&self, args: &Value, result: &Value) -> String;
}
```

Each tool gets an **emoji prefix** and a **human-readable format**:

| Tool             | Request phase                         | After result          |
| ---------------- | ------------------------------------- | --------------------- |
| knowledge_search | `🔍 "weed barrier fabric..."`         | `→ 3 results`         |
| web_search       | `🌐 "weed barrier trunk distance..."` | `→ 5 results`         |
| web_fetch        | `📄 ask.extension.org/kb/faq...`      | `→ 2.3k chars`        |
| shell_command    | `$ ls -l /root/GHOST/uploads/...`     | `# 0`                 |
| create_note      | `📝 "Garden Tips"`                    | `✓`                   |
| read_file        | `📖 skills/research.md`               | `→ 1.2k chars`        |
| run_agent        | `🤖 deep_research`                    | `⟳ 3 turns · 5 calls` |
| todo             | (existing rendering, unchanged)       |                       |

The trait lives in `src/tools/` and is interface-agnostic — Discord, TUI, or any future
interface can consume it.

### 2. Two-Phase Container Rendering (Discord)

Per tool-loop iteration:

1. **Request phase**: send a compact container listing all tool calls for this iteration
2. **Result phase**: edit the same message to append result hints

Example — one iteration calling 3 web_search tools:

**Phase 1 (sent):**

```
🌐 "weed barrier fabric around tree trunk..."
🌐 "landscape fabric shrubs trunk distance"
🌐 "防草シート 樹木 幹 どこまで離す"
```

**Phase 2 (edited):**

```
🌐 "weed barrier fabric around tree trunk..."   → 5 results
🌐 "landscape fabric shrubs trunk distance"     → 4 results
🌐 "防草シート 樹木 幹 どこまで離す"               → 3 results
```

Container color stays `0x6C_70_86` (muted Catppuccin overlay).

### 3. Live Agent Status

When `run_agent` is called, the tool call container message keeps getting edited with
live progress:

```
🤖 deep_research  ⟳ running · 3 turns · 5 calls
```

Updated each time the agent completes a turn. On finish:

```
🤖 deep_research  ✓ 12 turns · 23 calls · 45s
```

This reuses the existing `ToolLoopEvent` channel — add a new variant
`ToolLoopEvent::AgentProgress { name, turns, tool_calls }`.

### 4. Width Fix

Investigate whether Components v2 `TextDisplay` or `Container` wrappers cause the ~50%
width issue on desktop. Likely fixes:

- Send final response as a plain message (no v2 flag) if TextDisplay is the bottleneck
- Or remove unnecessary container wrapping on the response body

Needs testing in Discord to confirm the actual cause.

### 5. URL Embed Suppression

Set the `suppress_embeds` flag (`1 << 2` = `4`) on response messages. This prevents
Discord from generating link preview cards. Does not affect our Components v2 containers
which are a separate system.

Apply to: `send_assistant_v2()` messages.

## Not Changing

- Statusline format (already works well)
- TODO rendering (already uses in-place editing)
- Internal tool result storage/processing
- Lua/agent tools (not visible in Discord)

## Key Files

- `src/tools/` — new `ToolDisplay` trait + impls per tool
- `src/interfaces/discord/ui_events.rs` — two-phase rendering, agent progress
- `src/interfaces/discord/send.rs` — suppress_embeds flag, width investigation
- `src/interfaces/discord/components_v2.rs` — message editing (already exists)
- `src/chat/tool_loop.rs` — emit richer events with display info
- `src/chat/types.rs` — new event variants
