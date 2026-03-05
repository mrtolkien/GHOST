# Coding Agent (`ghost hack`) — Design

## Status: APPROVED

## Goal

Enable GHOST to handle coding tasks by spawning a Rust-native coding agent that takes
over the OPERATOR's Discord channel with a clean context, repo-aware conventions, and
direct OPERATOR interaction.

## Architecture

The coding agent is a **Rust-native module** (`src/coding/`) that reuses the existing
`SessionChat` + tool loop infrastructure with a different system prompt, skill set, and
session. It is NOT a Lua agent.

### Session Takeover

Implemented at the session-resolution level: a `coding_sessions` DB table stores
`(channel_id, session_id, working_dir, started_at)`. The Discord handler checks for an
active takeover before resolving the normal GHOST session. Messages route to
`SessionChat` with the coding agent's session, prompt, and tools.

### Entry Points (CLI)

```
ghost hack start <dir> [--prompt "..."]     # start new coding session
ghost hack resume <session_id> [--prompt "..."]  # resume previous session
ghost hack list                              # list recent coding sessions
```

The GHOST triggers these via `run_shell_command`. Paths are relative to workspace root.

### Tool Set

Same tools as the GHOST: shell, read, write, edit, todo, knowledge_search, web_search,
web_fetch, agent_control. Different prompt and skills, not different tools.

### System Prompt

Base template: `prompts/coding-agent.md`

- Identity: "You are a coding agent working in `{working_dir}`"
- Full `using-superpowers` content embedded (~1K tokens)
- Workflow guidance: explore -> understand -> brainstorm -> plan -> implement -> verify
- OPERATOR communication: ask questions directly, don't assume

Injected at session start (same mechanism as chat prompt):

- AGENTS.md / CLAUDE.md from repo root (if present)
- Skill listing (path + description) from three sources:
  1. `$WORKSPACE/skills/` (GHOST skills)
  2. Superpowers skills (coding-agent subset, ported to `prompts/skills/`)
  3. `.agents/skills/` from the repo (repo-specific skills)

### Model

Config field `[coding] model`. Falls back to default GHOST model if unset.

### Compaction

Uses existing compaction infrastructure with coding-specific instructions:

> Preserve: current plan/TODO status, files modified and why, test results, OPERATOR
> decisions. Drop: verbose file contents, raw shell output, intermediate diffs.

### Discord UI

- Tool call embeds use **Teal (`0x29FFD9`)** to visually distinguish from GHOST
- Entry message:
  `GHOST HACKED -- you're now talking to the coding agent. /kill to exit.`
- Exit message: deterministic summary (git log + git diff --stat), then
  `GHOST HACKED -- session ended.`

### `/kill` and Session Lifecycle

- `/kill` is the only exit mechanism (no natural-language detection)
- On `/kill`: generate deterministic summary (no LLM), delete takeover record, inject
  summary as system message into GHOST's session, GHOST resumes
- Sessions are resumable via `ghost hack resume`

### Repo Management

- Repos cloned to `$WORKSPACE/code/$slug/`
- `ghost hack start` takes any path (relative to workspace)
- The GHOST's `coding` skill handles clone/pull before starting
- Project linkage: GHOST creates a note called `repo.md` in the project with the repo
  path and context. On subsequent coding requests, checks for this note first.

### Superpowers

- `scripts/sync-superpowers.py`: vendors upstream into `vendor/superpowers/`
- All 14 skills ported to `prompts/skills/` with Ghost adaptations (tool name mappings,
  OPERATOR terminology)
- `using-superpowers` embedded in coding agent system prompt, not a separate skill

## Key Design Decisions

1. **Rust-native, not Lua**: The coding agent needs repo context injection, channel
   takeover, skill loading from multiple sources, and potential MCP support. Important
   enough to be first-class Rust.

2. **Session-level takeover, not handler-level**: Reuses `SessionChat` and the tool loop
   with a different session/prompt/tools. Minimal new code, no duplication.

3. **Same tools, different prompt**: No tool set reduction. Knowledge search is valuable
   (reference imports give perfect docs). Todo tracks plan execution. Web tools useful
   for looking up docs.

4. **Full `using-superpowers` in prompt**: ~1K tokens, high value. Models need the
   rationalization red flags table and skill priority guidance.

5. **CLI as the spawn mechanism**: `ghost hack start` via `run_shell_command`. No new
   tool schema polluting context. The CLI is the natural entry point.

6. **Convention-based project linkage**: Notes, not DB fields. GHOST writes
   `notes/{project}/repo.md` with repo path. Searchable via knowledge_search.

7. **Vendor + port superpowers**: All 14 skills. They're small and each adds value. Sync
   script maintains upstream connection.

## Competitors Reviewed

- **Pi-mono**: 4 tools, <1K token prompt, AGENTS.md, no sub-agents/MCP/permissions. Key
  insight: minimal works for frontier models. We adopt the "keep it simple" spirit but
  add skills and knowledge search since GHOST already has them.

- **Codex CLI (codex-rs)**: Rust-native, sandboxed execution. We skip sandboxing (single
  OPERATOR, trusted) but note it for future multi-user support.

- **Mario Zechner's blog**: Edit via exact text matching (not line numbers). File-based
  plans over ephemeral state. Full observability matters. Progressive skill disclosure.
  All principles we follow.
