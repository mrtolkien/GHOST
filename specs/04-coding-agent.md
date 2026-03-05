# Coding Agent (`ghost hack`)

## Status: SPEC DRAFT

## Goal

Enable GHOST to handle coding tasks by spawning a dedicated coding agent that **takes
over** the OPERATOR's session with clean context, repo-aware conventions, and direct
OPERATOR interaction. The GHOST remains an assistant/orchestrator — it doesn't code
itself, it dispatches a specialist.

## Key Insight

GHOST is an assistant: it manages projects, knowledge, answers questions. Asking it to
also code suffers from context rot — the coding task doesn't get a focused system
prompt, a clean context window, or repo-specific conventions. Instead, the coding agent
is a **session takeover**: a full coding agent (like Claude Code / Pi) with its own
system prompt, the repo's AGENTS.md/CLAUDE.md, and superpowers workflow skills — talking
directly to the OPERATOR.

## Design Decisions

### Session takeover, not sub-agent

The coding agent is NOT a background worker that reports back. It takes over the
OPERATOR's channel (Discord DM) and chats with them directly. The OPERATOR interacts
with it the same way they'd interact with Claude Code — asking questions, refining
requirements, reviewing output. When done, the GHOST resumes with a summary of what
happened.

**Why takeover?**

- Clean context: the coding agent starts fresh with a focused system prompt + repo
  context, not a 50-turn conversation about groceries followed by "also fix this bug"
- Direct communication: the coding agent can ask the OPERATOR clarifying questions
  without relay through the GHOST
- Full workflow: brainstorming → planning → TDD → review → verification all happen in
  the coding session with the OPERATOR in the loop
- Discord constraint: DMs don't support threads, so same-channel is the only option

### Superpowers as the coding agent's skill library

All 14 skills from [obra/superpowers](https://github.com/obra/superpowers) (MIT) are
ported and installed as skills the coding agent can read. They teach workflow
discipline: brainstorming, planning, TDD, debugging, verification, code review. The
`using- superpowers` content is embedded in the coding agent's system prompt (not a
separate skill).

**These skills belong to the coding agent, not the GHOST.** The GHOST doesn't need TDD
or git-worktrees. It keeps its existing skills (deep-research, knowledge-navigator,
etc.) plus a new `coding` skill that triggers the takeover.

### Code lives in `code/`

Repos and code are persistent infrastructure, independent of projects. A project might
reference a repo, but the repo outlives the project.

Storage: `$WORKSPACE/code/$slug/` (e.g., `code/ghost/`, `code/my-scripts/`).

## Architecture

```
OPERATOR (Discord)
    ↕ (chat)
GHOST (assistant)
    │
    ├── OPERATOR: "hack on ghost" or "clone X and fix Y"
    │
    ├── GHOST reads `coding` skill → sets up code/$slug/ + clones if needed
    │
    ├── GHOST spawns coding agent → SESSION TAKEOVER
    │   ├── coding agent now owns the Discord channel
    │   ├── clean system prompt + repo context (AGENTS.md, etc.)
    │   ├── superpowers skills available (brainstorming, TDD, ...)
    │   ├── OPERATOR chats directly with coding agent
    │   └── OPERATOR sends /kill → coding agent terminates
    │
    └── GHOST resumes with deterministic session summary
```

## User Experience

### Starting a coding session

```
OPERATOR: Can you work on fixing the auth bug in the ghost repo?

GHOST: I'll start a coding session on code/ghost. Pulling latest...

       🔧 GHOST HACKED — you're now talking to the coding agent.
       Send /kill to end the session.

CODING AGENT: I've read the project conventions (AGENTS.md). Let me
              understand the auth bug. Can you describe the symptoms?
```

### During a coding session

The coding agent behaves like Claude Code / Pi:

- Reads and writes files in the repo
- Runs shell commands (tests, builds, git)
- Follows superpowers workflow (brainstorm → plan → TDD → verify)
- Asks the OPERATOR questions directly
- Commits work incrementally

### Ending a coding session

```
OPERATOR: /kill

[deterministic summary — no LLM generation, instant response]

🔧 GHOST HACKED — session ended.

  Branch: fix/auth-refresh
  Commits:
    a1b2c3d feat: refresh auth token on 401
    e4f5g6h test: add auth refresh edge cases
  Changed:
    src/auth/refresh.rs | 42 +++++++++
    tests/auth_refresh.rs | 38 +++++++++
    2 files changed, 80 insertions(+)

GHOST: Welcome back! The coding session made 2 commits on
       fix/auth-refresh. Anything else?
```

## Components

### 1. `coding` skill (`prompts/skills/coding.md`)

A GHOST-side skill that teaches the GHOST how to:

- Recognize when the OPERATOR wants to code
- Set up `code/$slug/` if it doesn't exist
- Clone a repo or create a new project directory
- Trigger the coding agent session takeover
- Resume after the session with the deterministic summary

Triggers: "fix", "implement", "build", "code", "hack", "work on [repo]", "clone and..."

### 2. Coding agent (Rust-native, `src/coding/`)

The coding agent is too complex for the Lua agent framework. It needs channel takeover,
repo context injection, skills loading, and sub-agent spawning — all first-class Rust
concerns. Lua agents are the _workers_ it dispatches, not the coding agent itself.

**Why not Lua?**

- Channel takeover requires deep integration with Discord message routing
- Repo context injection (AGENTS.md, `.agents/skills/`) reuses existing Rust infra
- Sub-agent spawning means the coding agent is an _orchestrator_ of Lua agents
- Session lifecycle (deterministic summary on /kill) needs git CLI integration
- High iteration count (200+) with direct OPERATOR chat — this is a session, not a job

**Feature parity target**: [Pi-mono](https://github.com/badlogic/pi-mono) — a minimal
coding agent with 4 tools (read, write, edit, shell), a <1000-token system prompt, and
AGENTS.md for project context. Our coding agent should match Pi's core capabilities
(file manipulation, shell access, repo-aware context for prompt, skills, ...) and layer
superpowers workflow skills on top.

**System prompt** (`prompts/coding-agent.md`):

- Identity: "You are a coding agent working in {{ working_dir }}"
- Using-superpowers content embedded (red flags table, skill priority, "check skills
  before responding")
- Workflow: explore repo → understand task → brainstorm → plan → implement → verify
- Tool guidance: file tools + shell, commit incrementally, run tests after changes
- OPERATOR communication: ask questions directly, don't assume

**Repo context injection** (Rust, at session start):

- Read `AGENTS.md` or `CLAUDE.md` from `working_dir` if present
- Read `.agents/skills/` from `working_dir` — repo-specific developer skills (same
  convention as Claude Code's project skills)
- Discover superpowers skills from `$WORKSPACE/skills/` (coding-agent subset)
- Append all context to system prompt before first turn

**Tools**: same as GHOST core (`read_file`, `write_file`, `file_edit`,
`run_shell_command`, `todo`) plus `agent_control` for spawning Lua worker agents.

**Sub-agents**: The coding agent can spawn Lua agents for parallel tasks (e.g.,
"implement these 3 independent functions" → 3 worker agents). This is how superpowers
skills like `subagent-development` and `parallel-agents` work in practice.

### 3. Session takeover mechanism

When a coding agent is spawned, it takes over the OPERATOR's Discord channel.

**Implementation**:

- GHOST spawns the coding agent (Rust-native, not via `agent_control`)
- Coding agent registers a **channel takeover**: its session receives messages from the
  OPERATOR's Discord channel
- Discord bot routes incoming messages to the active takeover session instead of the
  GHOST's main session
- `/kill` command ends the takeover and returns control to the GHOST
- If the coding agent finishes naturally (via `end_session`), takeover also ends

**Data model**:

- `channel_takeover`: `{ channel_id, agent_id, agent_session_id, started_at }`
- Discord message handler checks for active takeover before routing to main session

### 4. Superpowers skill library

All 14 skills from obra/superpowers, ported and adapted for the coding agent.

**Source of truth**: `obra/superpowers` GitHub repo (MIT).

**Vendoring**: `scripts/sync-superpowers.py` fetches latest into `vendor/superpowers/`,
shows diff against previous vendor. Porting to `prompts/skills/` is always manual.

```
uv run scripts/sync-superpowers.py              # fetch + show diff
uv run scripts/sync-superpowers.py --apply      # update vendor dir
```

`vendor/superpowers/` is tracked in git so diffs are visible in PRs.

**Adaptation rules** (when porting from vendor):

- `Skill tool` / `invoke skill` → `read_file("skills/<name>/skill.md")`
- `Agent tool` / `Task tool` → `agent_control(action: "start", ...)`
- `TodoWrite` → `todo(action: "plan", ...)`
- `EnterPlanMode` → remove
- `your human partner` → `the OPERATOR`
- Remove Claude Code-specific references (plugins, /commands, IDE)
- Keep all workflow wisdom, rationalization tables, red flags, checklists
- Rename `SKILL.md` → `skill.md` (GHOST convention)

**Skill list**:

| Superpowers skill              | GHOST skill name           | Notes                    |
| ------------------------------ | -------------------------- | ------------------------ |
| using-superpowers              | _(in coding agent prompt)_ | Not a separate skill     |
| brainstorming                  | `brainstorming`            |                          |
| writing-plans                  | `writing-plans`            |                          |
| executing-plans                | `executing-plans`          |                          |
| subagent-driven-development    | `subagent-development`     |                          |
| dispatching-parallel-agents    | `parallel-agents`          |                          |
| test-driven-development        | `tdd`                      |                          |
| systematic-debugging           | `systematic-debugging`     |                          |
| verification-before-completion | `verification`             |                          |
| requesting-code-review         | `requesting-review`        |                          |
| receiving-code-review          | `receiving-review`         |                          |
| using-git-worktrees            | `git-worktrees`            |                          |
| finishing-a-development-branch | `finishing-branch`         |                          |
| writing-skills                 | `writing-skills`           | Replaces `skill-creator` |

### 5. Repo management

**Storage**: `$WORKSPACE/code/$slug/`

**Two entry paths**:

1. **Remote repo**: OPERATOR provides a URL. The `coding` skill tells the GHOST to clone
   it into `code/$slug/`, then start the coding agent with `working_dir` pointing there.

2. **New project**: OPERATOR asks to build something new. The GHOST creates
   `code/$slug/`, optionally runs `git init`, then starts the coding agent.

**On subsequent sessions**: if `code/$slug/` already exists, the GHOST pulls latest (if
remote-tracked) and starts the coding agent in the existing checkout.

### 6. Sync script

`scripts/sync-superpowers.py` — uv inline script (PEP 723).

```python
# /// script
# requires-python = ">=3.11"
# dependencies = ["rich"]
# ///
```

**Implementation**:

1. `git clone --depth=1` (or pull) `obra/superpowers` to temp dir
2. Copy `skills/*/` to `vendor/superpowers/skills/`
3. Diff old vs new vendor using `difflib`
4. With `--apply`: overwrite vendor, print summary of changed files
5. Reminder: "Review diffs and port changes to prompts/skills/ manually"

## Open Questions

1. **Coding agent model**: Should the coding agent use a different (stronger/cheaper)
   model than the GHOST? Configurable in `config.toml`. Likely want the strongest
   available model for coding.

2. **Sub-agent spawning**: The superpowers skills (subagent-development,
   parallel-agents) assume the coding agent can spawn sub-agents. With
   `MAX_SPAWN_DEPTH = 2` and the coding agent at depth 1, workers would be at the limit.
   May need to bump or rethink.

3. **Session persistence**: Can the OPERATOR `/kill` and resume later? The agent
   infrastructure supports `continue`. This maps to "I'll keep working on this
   tomorrow."

4. **Multiple concurrent sessions**: Can the OPERATOR have coding sessions on two repos?
   Probably not with single-channel takeover in DMs. Defer to multi-interface support.

5. **Security**: The coding agent has `run_shell_command` with no sandbox. Acceptable
   for a personal agent (single OPERATOR, trusted) but needs thought for any multi-user
   future.

6. **Project linkage**: A project can reference a `code/$slug/` repo. How? A field in
   the project frontmatter like `repo: ghost`? Or just convention?

## Implementation Order

### Phase 1: Superpowers vendor + sync

1. Write `scripts/sync-superpowers.py`
2. Vendor current superpowers into `vendor/superpowers/`
3. Port all 14 skills to `prompts/skills/` with adaptations
4. Replace `skill-creator` with ported `writing-skills`

### Phase 2: Coding agent (Rust)

1. Create `src/coding/` module — session struct, tool loop, prompt builder
2. Write `prompts/coding-agent.md` system prompt
3. Implement repo context injection: AGENTS.md + `.agents/skills/` + superpowers skills
4. Wire tool access: file tools + shell + todo + agent_control
5. Create `coding` GHOST skill (`prompts/skills/coding.md`)

### Phase 3: Session takeover

1. Add channel takeover data model
2. Modify Discord message routing to check for active takeover
3. Implement `/kill` command to end takeover
4. Implement takeover end → deterministic summary (git diff --stat + git log)
5. Handle edge cases: GHOST reboot during takeover, agent crash

### Phase 4: Repo management

1. Add `code/` to workspace directory structure
2. Implement clone/pull logic in the `coding` skill
3. Wire `working_dir` through to agent spawning

### Phase 5: Integration testing

1. Manual test: ask GHOST to hack on a test repo
2. Verify: takeover, AGENTS.md reading, TDD workflow
3. Verify: /kill summary, GHOST resume
4. Write e2e test for spawn → takeover → end flow

## References

- [obra/superpowers](https://github.com/obra/superpowers) — MIT, workflow skills
- [OpenClaw](https://github.com/openclaw/openclaw) — Pi SDK integration, session model
- [Pi-mono](https://github.com/badlogic/pi-mono) — minimal coding agent (4 tools)
- [What I learned building a coding agent](https://mariozechner.at/posts/2025-11-30-pi-coding-agent/)
- `specs/04c-superpowers.md` — original notes
- `specs/notes/prompt-design.md` — layered prompt architecture
