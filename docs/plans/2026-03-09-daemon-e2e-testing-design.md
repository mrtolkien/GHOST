# Daemon-Level E2E Testing

## Problem

Existing e2e tests sidestep the real daemon lifecycle. They call `SessionChat::chat()`
directly, skipping the file watcher, scheduler, event handler, reconciliation loop, and
shutdown coordination. Real bugs only surface when the full system runs together.

## Goal

Tests that boot the real daemon (minus Discord), send messages through the real pipeline,
wait for all background activity to complete, and assert on the final workspace/DB state.
Real LLM calls, real embeddings, real agents.

## Design

### 1. Conditional Discord in `boot()`

Skip `start_discord()` when no Discord bot token is configured. Discord already returns
`Option<(DiscordSender, JoinHandle)>` — the change is to not attempt connection at all
when the token is absent. `LiveTestEnv` ensures no token is present.

### 2. `DaemonHandle` struct

Replace the flat `BootResult` tuple with a struct:

```rust
pub struct DaemonHandle {
    pub session_chat: Arc<SessionChat>,
    pub db: GhostDb,
    pub agent_runner: Arc<AgentRunner>,
    pub config: Config,
    shutdown_tx: watch::Sender<bool>,
    idle_trigger_tx: mpsc::Sender<()>,
    // subsystem join handles for graceful shutdown
}
```

Returned by `boot()`. Production `run()` uses it the same way. Tests get direct access
to the chat system and control knobs.

### 3. Scheduler manual trigger

Add an `mpsc::channel` to the scheduler. On receive, immediately run idle agent checks
(same codepath as the idle timer firing). Exposed as:

```rust
impl DaemonHandle {
    /// Trigger idle agents now (reflection, etc.) without waiting for the timer.
    pub async fn trigger_idle_agents(&self) { ... }
}
```

This doubles as the foundation for a future `ghost reflect` CLI command.

### 4. `settle()` — wait for system quiescence

Polls until the system is idle:

- `active_sessions.len() == 0` (no chat tool loops running)
- Agent runner has no active tasks
- Event handler queue is drained
- No in-flight embedding jobs
- No background shell commands running

Each subsystem exposes an "am I busy?" check (atomic counter or similar).
Configurable timeout, default ~120s for live LLM calls.

```rust
impl DaemonHandle {
    /// Wait until all subsystems are idle.
    pub async fn settle(&self) -> Result<(), SettleTimeout> { ... }
}
```

### 5. `LiveTestEnv` integration

`LiveTestEnv` gets a `boot()` method that calls the real `boot()` with a test config
(no Discord token, temp workspace, test provider config). Returns the `DaemonHandle`.

Existing helpers (assertions, snapshots, polling, `collect_tool_calls`, etc.) remain
and operate on the same DB instance.

### Subsystem busy counters

Each subsystem needs to expose whether it has in-flight work:

| Subsystem | Mechanism |
|-----------|-----------|
| Active sessions | `active_sessions.len()` (already exists) |
| Agent runner | `AtomicUsize` task counter, inc on spawn, dec on complete |
| Event handler | Channel length or processed counter |
| Embedding pipeline | `AtomicUsize` in-flight counter |
| Background shell | `AtomicUsize` in-flight counter |

`settle()` polls all of these at ~500ms intervals.

### Shutdown

`DaemonHandle` implements `Drop` (or an explicit `shutdown()`) that:
1. Sends `true` on `shutdown_tx`
2. Awaits all subsystem join handles

For tests, `LiveTestEnv::Drop` calls this before snapshotting diagnostics.

## First test: Ark Nova reference import

This test exercises the full pipeline: chat → tool use → document import → file watcher
→ chunking → embedding.

### ACT

- Prompt: "Import the Ark Nova rules for future reference"
- Wait for all to settle

### ASSERT

1. **PDF imported and transformed**: A `.md` file exists under `references/` for the
   Ark Nova rules (the daemon used `ghost document import` to convert the PDF via
   docling)
2. **Chunked and embedded**: The document produced 50+ chunks in the DB, each with an
   associated embedding vector
3. **Semantic search works**: A knowledge search for "ark nova break rules" returns
   snippets containing the actual break rules paragraph — not just the top of the
   document. (This is expected to fail initially, validating that the test catches a
   real current issue with snippet retrieval.)

### Example

```rust
#[tokio::test]
async fn test_ark_nova_import() {
    let env = LiveTestEnv::new("ark_nova_import").await;
    let daemon = env.boot().await;

    let session_id = daemon.create_session().await;
    daemon.chat(&session_id, "Import the Ark Nova rules for future reference").await;
    daemon.settle().await;

    // 1. PDF was imported and converted to markdown
    let refs_dir = env.workspace().join("references");
    let md_files: Vec<_> = glob(&refs_dir, "**/*.md");
    assert!(!md_files.is_empty(), "should have imported a .md reference");

    // 2. Chunked and embedded (50+ chunks with vectors)
    let chunks = count_chunks_for_reference(&daemon.db, "ark-nova").await;
    assert!(chunks >= 50, "expected 50+ chunks, got {chunks}");
    let embedded = count_embedded_chunks(&daemon.db, "ark-nova").await;
    assert_eq!(chunks, embedded, "all chunks should have embeddings");

    // 3. Semantic search returns relevant snippets (not just document top)
    let results = knowledge_search(&daemon.db, "ark nova break rules").await;
    let snippets: String = results.iter().map(|r| &r.snippet).collect();
    assert!(
        snippets.contains("break") || snippets.contains("Break"),
        "search for 'break rules' should return the break rules section, got: {snippets}"
    );
}
```

## Trade-offs

- **Pro**: Near-zero gap between test and production boot paths
- **Pro**: `DaemonHandle` is cleaner than the current 6-element tuple
- **Pro**: `trigger_idle_agents()` useful beyond tests (CLI, future API)
- **Pro**: Reuses all existing `LiveTestEnv` infrastructure
- **Con**: `settle()` requires plumbing busy counters into several subsystems
- **Con**: Tests are slow (real LLM + embeddings) — but that's the point
- **Con**: Flakiness risk from real LLM calls — mitigated by generous timeouts and
  state-based assertions rather than exact output matching

## Non-goals

- Mid-turn interception (pausing during tool loop)
- Controlling timing of individual background events
- Mock providers — these tests use real LLMs deliberately
- Replacing existing step-based e2e tests (they serve a different purpose)
