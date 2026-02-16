# 10 — Tool System

## Overview

Minimal tool set following the "bash-first" philosophy (inspired by pi-mono's approach).
The GHOST has **5 core tools** for all contexts and **3 additional tools** for the
reflection context. Everything else — file search, knowledge queries, web search — is
accessed via CLI commands through bash (see specs 11 and 13 for the CLI definitions).

This keeps the tool count low, reducing token overhead in the system prompt and
cognitive load on the model. Dedicated tools exist only where bash falls short
(structured file I/O, surgical edits).

## Architecture

```rust
pub struct ToolManager {
    tools: HashMap<String, Arc<dyn Tool>>,
}

#[async_trait]
pub trait Tool: Send + Sync {
    /// Tool name (used in function calling).
    fn name(&self) -> &str;

    /// JSON Schema for the tool's parameters.
    fn schema(&self) -> ToolDefinition;

    /// Execute the tool with the given parameters.
    async fn execute(&self, params: serde_json::Value, ctx: &ToolContext) -> Result<String, ToolError>;
}

pub struct ToolContext {
    pub workspace: PathBuf,
    pub cwd: PathBuf,
    pub db: Surreal<Db>,
    pub config: Config,
}

#[derive(Debug, thiserror::Error)]
pub enum ToolError {
    #[error("invalid parameters: {0}")]
    InvalidParams(String),

    #[error("execution failed: {0}")]
    ExecutionFailed(String),

    #[error("permission denied: {0}")]
    PermissionDenied(String),
}
```

## Tool Sets

| Context    | Tools                                                               |
| ---------- | ------------------------------------------------------------------- |
| Chat       | `run_shell_command`, `read_file`, `write_file`, `file_edit`, `todo` |
| Reflection | Chat tools + `note_write`, `reference_write`, `reference_manage`    |
| Heartbeat  | Chat tools                                                          |
| Custom job | Chat tools (default)                                                |

All contexts share the same 5 core tools. Only reflection adds 3 knowledge-write tools
that need structured parameter validation (see spec 13 for their definitions).

## Core Tools (5)

**`run_shell_command`** — Execute a shell command.

- Parameters: `command: string`, `timeout_ms: number (optional, default 30000)`,
  `directory: string (optional, default .)`
- Returns: stdout + stderr, exit code
- Commands run in the workspace directory by default
- This is the primary way the GHOST accesses knowledge search, web search, file search,
  and all other CLI features (see spec 10b)

**`read_file`** — Read file contents with pagination.

- Parameters: `path: string`, `offset: number (optional)`, `limit: number (optional)`
- Returns: File contents with line numbers
- Why dedicated: pagination, line numbers, and structured output that `cat` can't match

**`write_file`** — Create or overwrite a file.

- Parameters: `path: string`, `content: string`
- Returns: Success message
- Automatically creates parent directories
- Replaces the old `create_file` — a single tool for all file creation/overwriting

**`file_edit`** — Edit an existing file by string replacement.

- Parameters: `path: string`, `old_string: string`, `new_string: string`
- Returns: Success message with diff context
- Rejects ambiguous edits (multiple matches)
- Why dedicated: more reliable than `sed`, handles edge cases (whitespace, encoding)

**`todo`** — Session-scoped TODO list for working memory.

- Parameters:
  - `action: "plan" | "add" | "update" | "batch_update" | "clear"`
  - `items: [{title, description?}]` — for `plan` (replaces entire list)
  - `title: string`, `description: string (optional)` — for `add`
  - `index: number (1-based)`, `status: "pending" | "in_progress" | "done" | "skipped"`,
    `note: string (optional)` — for `update`
  - `updates: [{index, status, note?}]` — for `batch_update`
  - No parameters for `clear` (resets list)
- Returns: Formatted TODO list with status symbols (○ pending, ◉ in_progress, ✓ done, –
  skipped) and progress counter (e.g., `TODO [2/5]`)
- Session-scoped: stored in `session.todo_list` (chat) or `job_log.todo_list` (jobs)
- `plan` is the standard starting point: the GHOST creates its work plan as a TODO list
- `batch_update` is essential: mark multiple items done/skipped in a single tool call
  instead of N separate `update` calls
- Why dedicated: structured working memory that persists across tool loops without
  polluting files. The GHOST's equivalent of a scratchpad that doesn't get lost mid-run.

### TODO Prompt Guidance

The base system prompt MUST include guidance on when and how to use the `todo` tool.
Research shows that models without clear planning guidance can produce worse results
than no planning at all (Plan-and-Act, ICML 2025: zero-shot planner decreased
performance by 13pp on WebArena). Good guidance makes planning a net positive for
complex tasks.

The prompt should cover:

- **When to plan**: research tasks requiring multiple searches, tasks with 3+ steps,
  multi-part requests from the OPERATOR
- **When NOT to plan**: simple questions, single-step tasks, conversational responses
- **How to plan well**: concrete steps (5-10 words each), mark current step
  `in_progress` before starting, use `batch_update` not individual `update` calls, add
  new steps with `add` when discovered mid-task

### TODO Context Injection

The current TODO state is injected into each provider call as a message **after** the
user's message (not in the system prompt — that would break prompt caching). This
ensures the GHOST always sees its outstanding items without wasting a tool call on
listing.

```
[system prompt — cached by provider]
[message history]
[user message]
[system: "Current TODO:\n○ Item 1\n✓ Item 2\n..."]   ← injected here
```

If the TODO list is empty, nothing is injected.

## What Moved to CLI + Bash

These are no longer dedicated tools. The GHOST invokes them via `run_shell_command`:

| Old tool           | New approach                                                          |
| ------------------ | --------------------------------------------------------------------- |
| `find_files`       | `fd` or `find` via bash                                               |
| `search_files`     | `rg` or `grep` via bash                                               |
| `list_dir`         | `ls` via bash                                                         |
| `knowledge_search` | `ghost knowledge search "query"` via bash                             |
| `knowledge_get`    | `read_file` (notes and references are files) or `ghost knowledge get` |
| `web_search`       | `ghost web search "query"` via bash                                   |
| `web_fetch`        | `ghost web fetch "url"` via bash                                      |
| `diary_write`      | `file_edit` on `$WORKSPACE/diary/YYYY-MM-DD.md`                       |
| `identity_edit`    | `file_edit` on SOUL.md / OPERATOR.md / BOOT.md                        |

See spec 11 (web CLI), spec 12 (skills), and spec 13 (knowledge CLI) for details.

## Observability

Every tool execution MUST produce a span:

```rust
#[tracing::instrument(skip_all, fields(tool_name = %self.name(), params=...))]
async fn execute(&self, params: Value, ctx: &ToolContext) -> Result<String, ToolError> {
    let start = Instant::now();
    let result = self.run(params, ctx).await;
    logfire::info!("tool executed",
        tool_name = %self.name(),
        duration_ms = start.elapsed().as_millis(),
        success = result.is_ok(),
        result,
    );
    result
}
```

## Validation

1. `cargo test` — ToolManager registers tools and `all_tool_schemas()` returns valid
   JSON Schema for every tool
2. `cargo test` — `read_file` reads a file in a temp workspace, returns content with
   line numbers
3. `cargo test` — `run_shell_command` executes `echo hello` and returns stdout
4. `cargo test` — `write_file` creates a file with parent directories, then `file_edit`
   edits it via string replacement, verify the result
5. `cargo test` — tool execution error (e.g., read nonexistent file) returns a
   `ToolError`, not a panic
6. `cargo test` — `todo plan` creates a list, `todo update` changes status,
   `todo
   batch_update` updates multiple items, `todo clear` resets the list
7. `cargo test` — TODO state is injected after the user message (not in system prompt)
8. Now that tools exist, add the full provider live test from spec 05: send all tool
   schemas to each provider and verify the model can call `run_shell_command`
9. `just ci` — passes

## Acceptance Criteria

- ToolManager registers tools and generates schemas for the provider
- Each tool has a JSON Schema definition for function calling
- Tool execution errors are returned as tool results (not crashes)
- Chat and reflection tool sets are constructible
- `write_file` auto-creates parent directories
- Shell commands have configurable timeouts
- `todo` tool manages session-scoped TODO lists with plan/add/update/batch_update/clear
- TODO state is injected after user message (not in system prompt) to preserve caching
- All tool executions produce tracing spans with name and duration
- `just ci` passes

## Prior Art

Old code in `../t-koma`:

- `t-koma-gateway/src/tools/manager.rs` — Tool registration, schema generation, context
  management. Directly reusable pattern.
- `t-koma-gateway/src/tools/*.rs` — Individual tool implementations (shell, read_file,
  create_file, file_edit). Directly reusable with minor type changes.
- `t-koma-gateway/src/tools/mod.rs` — ToolSet enum for different contexts. Reusable
  concept, simplified now (chat vs reflection only).
- `t-koma-gateway/src/tools/reflection_todo.rs` — TODO tool with plan/add/update/
  batch_update actions, TodoItem/TodoStatus types, formatted output with progress
  counter. Directly reusable — expand from reflection-only to all contexts, add `clear`
  action.
