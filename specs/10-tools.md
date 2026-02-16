# 10 — Tool System and Chat Tools

## Overview

Tools are capabilities the GHOST can invoke during conversations. The tool system
manages registration, schema generation (for the provider's function calling API), and
execution.

Different contexts (chat, reflection, heartbeat) have different tool sets.

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

| Context    | Tools Available                                                      |
| ---------- | -------------------------------------------------------------------- |
| Chat       | shell, read_file, create_file, file_edit, find_files, search_files,  |
|            | list_dir, web_search, web_fetch, knowledge_search, knowledge_get     |
| Reflection | knowledge_search, knowledge_get, note_write, reference_write,        |
|            | reference_manage, diary_write, identity_edit, read_file, find_files, |
|            | web_search, web_fetch                                                |
| Heartbeat  | (minimal — defined per heartbeat job)                                |
| Custom job | (defined per job in frontmatter, defaults to chat tools)             |

Note: There is no `load_skill` tool. Skills are in `$WORKSPACE/skills/` and the GHOST
reads them with `read_file`. The system prompt lists available skills.

## Chat Tools

### Filesystem Tools

**`run_shell_command`** — Execute a shell command from the current working directory.

- Parameters: `command: string`, `timeout_ms: number (optional, default 30000)`
- Returns: stdout + stderr, exit code
- Safety: Commands run in the workspace directory. The GHOST should ask the OPERATOR
  before leaving the workspace.

**`read_file`** — Read file contents.

- Parameters: `path: string`, `offset: number (optional)`, `limit: number (optional)`
- Returns: File contents with line numbers

**`create_file`** — Create a new file (fails if exists).

- Parameters: `path: string`, `content: string`
- Returns: Success message

**`file_edit`** — Edit an existing file by string replacement.

- Parameters: `path: string`, `old_string: string`, `new_string: string`
- Returns: Success message with context

**`find_files`** — Find files by glob pattern.

- Parameters: `pattern: string`, `path: string (optional)`
- Returns: List of matching file paths

**`search_files`** — Search file contents by regex.

- Parameters: `pattern: string`, `path: string (optional)`, `glob: string (optional)`
- Returns: Matching lines with file paths and line numbers

**`list_dir`** — List directory contents.

- Parameters: `path: string (optional)`
- Returns: Files and directories with sizes

### Knowledge Tools (Query Only in Chat)

**`knowledge_search`** — Search notes, references, and diary.

- Parameters: `query: string`, `categories: string[] (optional)`,
  `topic: string (optional)`, `limit: number (optional, default 10)`
- Returns: Ranked search results with snippets

**`knowledge_get`** — Get full content of a note, reference, or diary entry.

- Parameters: `id: string (optional)`, `topic: string (optional)`,
  `path: string (optional)`
- Returns: Full content

## Observability

Every tool execution MUST produce a span:

```rust
#[tracing::instrument(skip_all, fields(tool_name = %self.name()))]
async fn execute(&self, params: Value, ctx: &ToolContext) -> Result<String, ToolError> {
    let start = Instant::now();
    let result = self.run(params, ctx).await;
    logfire::info!("tool executed",
        tool_name = %self.name(),
        duration_ms = start.elapsed().as_millis(),
        success = result.is_ok(),
    );
    result
}
```

## Acceptance Criteria

- ToolManager registers tools and generates schemas for the provider
- Each tool has a JSON Schema definition for function calling
- Tool execution errors are returned as tool results (not crashes)
- Different tool sets can be constructed for different contexts
- All filesystem tools respect workspace boundaries
- Shell commands have configurable timeouts
- All tool executions produce tracing spans with name and duration
- `just ci` passes

## Prior Art

Old code in `../t-koma`:

- `t-koma-gateway/src/tools/manager.rs` — Tool registration, schema generation, context
  management. Directly reusable pattern.
- `t-koma-gateway/src/tools/*.rs` — Individual tool implementations (shell, read_file,
  create_file, file_edit, search, find_files, list_dir). Directly reusable with minor
  type changes.
- `t-koma-gateway/src/tools/mod.rs` — ToolSet enum for different contexts (chat vs
  reflection). Reusable concept.
