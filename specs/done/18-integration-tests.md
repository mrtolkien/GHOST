# 18 — Integration Test Harness

## Overview

Integration tests validate major features end-to-end. They use a `live-tests` feature
flag and share a common test fixture for consistent starting state.

## Feature Flag

```toml
[features]
live-tests = []
```

Tests behind `live-tests` are excluded from `cargo test` (CI/local quick checks) and run
manually with `cargo test --features live-tests`.

## Test Fixture

A reusable starting state that all integration tests can build on:

```rust
// tests/common/mod.rs

pub struct TestFixture {
    pub db: Surreal<Db>,
    pub config: Config,
    pub workspace: TempDir,
    pub session_chat: Arc<SessionChat>,
}

impl TestFixture {
    /// Create a new test fixture with:
    /// - Temporary workspace directory
    /// - In-memory SurrealDB
    /// - Schema applied
    /// - Default config with test overrides
    /// - Identity files (BOOT.md, SOUL.md, OPERATOR.md)
    /// - Default skills installed
    pub async fn new() -> Self { ... }

    /// Create a fixture with pre-populated knowledge
    pub async fn with_knowledge(notes: Vec<Note>) -> Self { ... }

    /// Create a fixture with pre-populated sessions and messages
    pub async fn with_history(messages: Vec<StoredMessage>) -> Self { ... }

    /// Create a fixture with jobs installed
    pub async fn with_jobs(jobs: Vec<&str>) -> Self { ... }
}
```

### Key Design Decisions

- Use SurrealDB's in-memory mode for tests (no disk I/O)
- Use `tempdir` for workspace filesystem
- Test config uses a mock provider (records requests, returns canned responses)
- Cleanup is automatic via `Drop` on `TempDir`

## Mock Provider

```rust
pub struct MockProvider {
    responses: Arc<Mutex<VecDeque<ChatResponse>>>,
    requests: Arc<Mutex<Vec<ChatRequest>>>,
}

impl MockProvider {
    pub fn new() -> Self { ... }

    /// Queue a response that will be returned on the next chat() call
    pub fn queue_response(&self, response: ChatResponse) { ... }

    /// Get all requests that were made
    pub fn requests(&self) -> Vec<ChatRequest> { ... }
}
```

## Test Categories

### Provider Integration (live-tests)

- Test real OpenRouter API calls
- Validate request/response serialization
- Test tool use round-trips
- Test rate limit handling

### Chat Orchestration

- Test message persistence
- Test tool use loop
- Test compaction triggering
- Test max iteration cap

### Knowledge System

- Test note CRUD
- Test wiki link parsing and edge creation
- Test typed wiki links (`[[rel>Target]]`)
- Test full-text search
- Test embedding search (when Ollama available)
- Test hybrid search scoring

### Job System

- Test cron trigger timing
- Test cooldown enforcement
- Test `carry_last_output` state persistence
- Test `HEARTBEAT_CONTINUE` suppression
- Test file watcher picking up changes

### Discord (live-tests)

- Test message sending/receiving (requires bot token)
- Test message splitting
- Test typing indicator

## Running Tests

```bash
# Quick tests (no external dependencies)
just test

# Full integration tests (requires API keys, Ollama, etc.)
cargo test --features live-tests

# Specific test
cargo test --features live-tests test_openrouter_chat
```

## Validation

1. `cargo test` — `TestFixture::new()` creates a working environment (DB, config, temp
   workspace, mock provider) without errors
2. `cargo test` — mock provider: queue a response, call `SessionChat::chat()`, verify
   the queued response is returned and the request is recorded
3. `cargo test` — end-to-end: send a message through `SessionChat`, verify it's
   persisted, compaction works, and knowledge tools function
4. `cargo test --features live-tests` — all live tests pass (provider calls, embeddings,
   web tools)
5. `just ci` — passes (confirms no live-test code leaks into default test suite)

## Acceptance Criteria

- `TestFixture::new()` creates a fully functional test environment
- Mock provider records requests and returns queued responses
- All major features have at least one integration test
- Tests are isolated (no shared state between tests)
- `just test` passes without external dependencies
- Live tests require `--features live-tests` flag
- `just ci` passes
