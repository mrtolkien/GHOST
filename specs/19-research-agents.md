# Spec 19: Research Skill + Agent Infrastructure

## Summary

Consolidated research skill replacing two older skills, plus background agent
infrastructure with the deep research agent as the first use case.

## Research Skill

`prompts/skills/research.md` replaces the old `web-search` and
`reference-researcher` skills. Six sections: research workflow, source
evaluation, citation discipline, web fetch modes, query crafting, and deep
research escalation.

The key behavior change: the GHOST must `web_fetch` at least 2-3 results before
answering research questions. Search snippets alone are insufficient.

## Agent Infrastructure

### Agent Definitions

Markdown files with TOML frontmatter (`+++` delimiters) in `$WORKSPACE/agents/`.
Parsed by `src/agents/definition.rs`. Fields:

- `name`, `description`: metadata
- `tools`: whitelist of allowed tools for this agent
- `max_iterations`: hard limit on tool loop iterations (default: 40)
- `model`: optional model alias override
- Body: system prompt template with `{{ query }}` interpolation

### Agent Runner (`src/agents/runner.rs`)

Background agent execution manager. Maintains a `HashMap<String, AgentHandle>`
of running agents. Each agent:

1. Gets its own isolated session (status: "agent")
2. Gets a `job_log` entry (kind: "agent")
3. Runs with a restricted `ToolManager::for_agent(tools)` built from frontmatter
4. Uses `AgentHandler` (ToolLoopHandler impl) for DB persistence
5. Can be cancelled via `CancellationToken`

API: `start()`, `status()`, `stop()`, `take_completed()`, `list_agent_ids()`.

### agent_control Tool (`src/tools/agent_control.rs`)

Single tool registered in `for_chat()` with three actions:

- `start`: spawn a background agent by name with a prompt
- `status`: check progress (message count, TODO list, findings)
- `stop`: terminate and retrieve partial findings

### Agent Watcher (`src/agents/watcher.rs`)

Polling loop (3s interval) in the daemon that checks for completed agents. On
completion:

1. Injects findings as a system message in the parent session
2. Triggers a new chat turn with a synthetic user message
3. Sends the response to the originating Discord channel

### Workspace Bootstrap

`bootstrap_workspace` creates `agents/` directory and installs default agent
definitions via `install_default_agents()`.

## Deep Research Agent

`prompts/agents/deep-research.md` — the first default agent. Tools:
`knowledge_search`, `web_search`, `web_fetch`, `read_file`, `todo`. Max 40
iterations.

Methodology: mandatory TODO planning, broad search (2-3 angles), deep read (3-8
full pages via web_fetch), cross-referencing, targeted follow-up. Structured
output format with summary, findings by sub-question, ranked sources, and
uncertainties.

## Files

| File | Change |
|------|--------|
| `prompts/skills/research.md` | NEW |
| `prompts/skills/web-search.md` | DELETED |
| `prompts/skills/reference-researcher.md` | DELETED |
| `prompts/agents/deep-research.md` | NEW |
| `src/skills.rs` | Updated DEFAULT_SKILLS |
| `src/lib.rs` | Added `pub mod agents` |
| `src/agents/mod.rs` | NEW — barrel file |
| `src/agents/definition.rs` | NEW — parse + install agents |
| `src/agents/error.rs` | NEW — AgentError enum |
| `src/agents/runner.rs` | NEW — background execution |
| `src/agents/watcher.rs` | NEW — completion polling |
| `src/tools/mod.rs` | Added agent_control |
| `src/tools/manager.rs` | for_agent(), all_available() |
| `src/tools/agent_control.rs` | NEW — start/status/stop |
| `src/tools/context.rs` | Added agent_runner field |
| `src/chat/session.rs` | AgentHandler, chat_agent() |
| `src/config_workspace.rs` | Install default agents |
| `src/db/schema.rs` | Agent session status, agent_session field |
| `src/db/sessions.rs` | create_agent_session() |
| `src/db/job_logs.rs` | create_agent_job_log() |
| `src/daemon/run.rs` | Wire AgentRunner + watcher |
| `Cargo.toml` | Added tokio-util |
