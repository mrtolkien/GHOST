# Generic Agent Progress Rules

## Context

The deep research agent needs runtime feedback to enforce minimum tool call counts
(e.g., "call `web_fetch` at least 5 times"). Currently `build_agent_progress_nudge()` in
`session.rs` reports generic tool call counts (e.g.
`[Progress] web_fetch: 3, web_search: 4`) — the prompt is responsible for interpreting
thresholds.

**Experimentally proven**: Prompt engineering alone cannot enforce this. Three different
prompt approaches (running totals, TODO self-tracking, STOP gates) all failed — the
model consistently stops at 3 fetches regardless of instructions. Runtime feedback is
necessary.

**Goal**: Move threshold-based rules into the agent definition file (TOML frontmatter)
so agents can declare progress rules with custom below/met messages without code
changes. Currently the nudge is a generic counter; per-agent rules would add
threshold-aware guidance.

## Agent Definition Format

```toml
+++
name = "deep-research"
description = "Iterative web research with full page reading and source evaluation"
tools = ["knowledge_search", "web_search", "web_fetch", "read_file", "todo"]
max_iterations = 50

[[progress]]
tool = "web_fetch"
min = 5
below = "You need at least {min} web_fetch calls (currently {count}). Do NOT write your final report yet — keep researching."
met = "Minimum met. Consider fetching 2-3 more pages for a stronger report — check for newest releases and specific model reviews you haven't read yet."
+++
```

- `[[progress]]` is a TOML array of tables — one entry per rule
- `tool`: which tool to count
- `min`: minimum calls required
- `below`/`met`: optional custom messages; `{tool}`, `{min}`, `{count}` interpolated at
  runtime. If omitted, sensible defaults are used.
- Agents without `[[progress]]` → empty vec → no nudge (backward compatible via
  `#[serde(default)]`)

## Implementation

### 1. `src/agents/definition.rs` — Add `ProgressRule` and parsing

Add struct (next to `AgentFrontmatter`):

```rust
#[derive(Debug, Clone, Deserialize)]
pub struct ProgressRule {
    pub tool: String,
    pub min: u32,
    #[serde(default)]
    pub below: Option<String>,
    #[serde(default)]
    pub met: Option<String>,
}
```

Add `progress` to `AgentFrontmatter` (serde default = empty vec):

```rust
#[serde(default)]
progress: Vec<ProgressRule>,
```

Add `progress_rules: Vec<ProgressRule>` to `AgentDefinition`. Map in `parse_agent_file`.

### 2. `src/chat/session.rs` — Generic nudge builder + plumbing

Replace `build_agent_progress_nudge(history)` with a generic function that takes rules:

```rust
fn build_progress_nudge(rules: &[ProgressRule], history: &[ChatMessage]) -> Option<String>
```

Logic:

1. If no rules, return `None`
2. For each rule, count calls to `rule.tool` in assistant messages
3. If no tracked tools have been called yet, return `None`
4. Build progress line: `"[Progress] web_fetch: 3/5."`
5. If `count < min`: append rule's `below` message (or default)
6. If `count >= min`: append rule's `met` message (or default)
7. Return `Some(nudge)`

Interpolation: simple
`.replace("{count}", ...).replace("{min}", ...).replace("{tool}", ...)`

Default messages:

- below:
  `"You need at least {min} {tool} calls (currently {count}). Keep going — do NOT write your final output yet."`
- met:
  `"{tool} minimum reached ({count}/{min}). You may continue for thoroughness or wrap up."`

**Why in `session.rs` (not `definition.rs`)**: The function operates on `ChatMessage` /
`ContentBlock` (provider types). Keeping it in `session.rs` avoids adding a provider
dependency to the pure parsing module.

Add `progress_rules: Vec<ProgressRule>` to `AgentHandler` struct. Update
`post_tool_iteration` to call `build_progress_nudge(&self.progress_rules, history)`.

Update `chat_agent` and `continue_agent` signatures to accept `progress_rules`:

```rust
pub async fn chat_agent(
    &self, agent_name: &str, session_id: &str, prompt: &str,
    system_prompt: String, max_iterations: usize,
    progress_rules: Vec<ProgressRule>,  // new
) -> Result<ChatResult, ChatError>
```

(Same for `continue_agent`.)

### 3. `src/agents/runner.rs` — Pass rules through

Both `run_agent()` and `continue_agent_run()` pass `definition.progress_rules.clone()`
to the new parameter.

### 4. `prompts/agents/deep-research.md` — Declare rule in frontmatter

Add to frontmatter (body unchanged):

```toml
[[progress]]
tool = "web_fetch"
min = 5
below = "You need at least {min} web_fetch calls (currently {count}). Do NOT write your final report yet — keep researching."
met = "Minimum met. Consider fetching 2-3 more pages for a stronger report — check for newest releases and specific model reviews you haven't read yet."
```

### 5. `tests/deep_research_live.rs` — Update call site

Pass `definition.progress_rules.clone()` to `chat_agent()`.

### 6. Re-exports

Ensure `ProgressRule` is re-exported from `src/agents/mod.rs` (check what's currently
re-exported and follow the pattern).

## Tests

### Unit tests in `definition.rs`

- `parse_agent_with_progress_rules` — one rule, assert fields
- `parse_agent_with_multiple_progress_rules` — two rules, both parsed
- `parse_agent_without_progress_rules` — backward compat, empty vec
- `default_deep_research_agent_has_progress_rules` — extend existing test

### Unit tests for `build_progress_nudge` in `session.rs`

- No rules → `None`
- Rules but no tool calls yet → `None`
- Below minimum → returns nudge with below message
- At/above minimum → returns nudge with met message
- Custom messages with `{tool}`, `{min}`, `{count}` interpolation
- Default messages when `below`/`met` omitted

### Live test

Existing `deep_research_agent_produces_findings` validates end-to-end behavior. Run
after all changes:
`cargo test --features live-tests deep_research_agent_produces_findings`

## Files

| File                              | Change                                                                       |
| --------------------------------- | ---------------------------------------------------------------------------- |
| `src/agents/definition.rs`        | Add `ProgressRule`, parsing, unit tests                                      |
| `src/agents/mod.rs`               | Re-export `ProgressRule`                                                     |
| `src/chat/session.rs`             | Generic `build_progress_nudge`, `AgentHandler` plumbing, delete old function |
| `src/agents/runner.rs`            | Pass `progress_rules` to `chat_agent`/`continue_agent`                       |
| `prompts/agents/deep-research.md` | `[[progress]]` in frontmatter                                                |
| `tests/deep_research_live.rs`     | Pass `progress_rules` to `chat_agent`                                        |

## Verification

1. `just ci` — all tests pass, no new warnings
2. `cargo test -- progress` — new unit tests pass
3. `cargo test --features live-tests deep_research_agent_produces_findings` — live test
   still passes (5+ fetches, all3dp, aurora, P2S)
