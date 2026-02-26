# Spec 25: Deep Research Agent Live Test

## Problem

The deep research agent is the most complex agent in the system — 50 iterations max, 4
tools, a long multi-step prompt. It needs a live test that validates end-to-end
behavior: prompt rendering, tool execution, source discovery, and report quality.

Previous attempt (reverted in 398e050) had two issues:

1. **Loaded agent definition from real workspace** (`~/GHOST/agents/deep-research.md`)
   instead of temp workspace. The real workspace had a stale prompt referencing deleted
   tools, causing silent failures.
2. **No visibility into provider requests**. When the agent underperformed (1 web_fetch
   instead of 5+), there was no way to inspect what was actually sent to the model at
   each step.

## Prerequisites

- Spec 22 (provider request debug logging) — the test depends on being able to inspect
  raw provider requests in the debug directory

## Test Design

### Setup

Uses `LiveTestEnv` (from `tests/common.rs`):

- Real provider credentials from `~/.config/ghost/config.toml`
- Fresh temp workspace with repo-current agent definitions (via `include_str!` +
  `install_default_agents()`)
- Fresh SurrealDB instance
- `debug.save_requests = true` enabled (from spec 22)

### Test: `deep_research_agent_produces_findings`

```
File: tests/deep_research_live.rs
```

1. Load agent definition from temp workspace via `env.load_agent("deep-research")`
2. Build provider, tool manager, and `SessionChat` from `env.config`
3. Run `chat_agent` with the 3D printer research prompt, 5-minute timeout
4. Log session to diagnostic JSON via `env.log_session_json()`
5. Assert:
   - Non-empty findings (>200 chars)
   - At least 5 web_fetch calls made (process quality)
   - `all3dp.com` appears in fetched URLs (domain specialist discovered)
   - Aurora Tech mentioned in findings or fetched URLs (expert reviewer)
   - Bambu Lab P2S mentioned in findings (correct recommendation for 2026)

### Diagnostics on Failure

When a test fails, the following are available in
`e2e-output/<timestamp>_deep_research/`:

- `diagnostic.json` — full session messages (from `LiveTestEnv`)
- `debug/requests/*.json` — every provider request/response (from spec 22)
- Workspace snapshot (agents, notes, web cache)

This means any failure can be debugged by reading the actual JSON sent to the model.

### LiveTestEnv Additions

Add `load_agent(name)` helper method to `LiveTestEnv`:

```rust
pub fn load_agent(&self, name: &str) -> AgentDefinition {
    load_agent(&self.config.workspace, name)
        .unwrap_or_else(|e| panic!("load agent '{name}': {e}"))
}
```

### Live Test Isolation Rule

**Live tests must NEVER load data from the user's real workspace (`~/GHOST/`).** Always
use `LiveTestEnv` which provides a fresh temp workspace with repo-current agent
definitions. The real workspace retains agent files from first install
(`install_default_agents` only writes if file doesn't exist), so it may contain stale
prompts referencing deleted tools.

This should be added to CLAUDE.md under Testing Strategy, or in a dedicated notes in
specs/notes that is referenced in CLAUDE.md.

## Files

| File                          | Change                                       |
| ----------------------------- | -------------------------------------------- |
| `tests/deep_research_live.rs` | NEW — live test                              |
| `tests/common.rs`             | Add `load_agent()` helper to `LiveTestEnv`   |
| `CLAUDE.md`                   | Add live test isolation rule                 |
| `specs/notes/test-harness.md` | Update with `load_agent` + fix stale entries |
