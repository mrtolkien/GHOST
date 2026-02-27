---
name: testing
description: >-
  Test conventions, harness API, and readability rules for the Ghost codebase. MUST READ
  before writing or modifying any test — unit tests, integration tests, or live tests.
  Covers: test helper functions (test_config, test_workspace, test_database),
  MockProvider, LiveTestEnv, live test isolation rules, and the non-negotiable
  readability requirements. For step-based end-to-end tests, also read the
  `e2e-testing` skill. Failure to read this will produce duplicated boilerplate and
  broken test isolation.
---

# Testing Conventions

## Strategy

- **Integration tests** with a `live-tests` feature flag for all major features.
- Maintain robust, reusable test fixtures shared across tests.
- **Unit tests** only for genuinely complex behaviors or external crate behaviors.
- Live tests (`--features live-tests`) are human-run only. Ask the user before running
  them.

## Live Test Assertions (NON-NEGOTIABLE)

Live test assertions encode real quality requirements. **NEVER weaken, remove, or relax
assertions to make a flaky test pass.** If the test fails, fix the agent behavior
(prompt, nudges, tool loop, context management) — not the test.

See `memory/deep-research-agent.md` for the full iteration history, known failure modes,
and nudge architecture for the deep research agent specifically.

For step-based e2e fixture/log tooling, use `uv run scripts/e2e` and select the
operation (`refresh`, `render-log`, `diff`, `analyze-request`).

## Live Test Isolation (NON-NEGOTIABLE)

Live tests must **never** load data from the user's real workspace (`~/GHOST/`). Always
use `LiveTestEnv` which provides a fresh temp workspace with repo-current agent
definitions (via `include_str!` + `install_default_agents()`). The real workspace may
contain stale agent prompts referencing deleted tools.

## Test Readability (NON-NEGOTIABLE)

Tests must be **readable first**. A reader should understand what a test does in
seconds, not minutes.

- Extract ALL setup boilerplate into helper functions. A test body should be setup +
  action + assert, each in 1–3 lines.
- Prefer `let workspace = test_workspace()` over 15 lines of `TempDir` + `fs::write` +
  `env::set_var` inline.
- Name helpers after what they produce, not what they do: `test_config()` not
  `setup_config_dir_and_write_toml_and_set_env()`.
- Never duplicate setup across tests — if two tests need similar state, extract a
  helper.
- Extend existing helpers with parameters rather than creating new ones. If
  `test_config()` works for most tests but one needs a custom model, add an optional
  parameter — don't write `test_config_with_custom_model()`.
- Every test should read like a specification.

## Test Harness API

All reusable infrastructure lives in `tests/common.rs`.

### Non-Live Helpers (always available)

| Helper                                                      | Purpose                                                            |
| ----------------------------------------------------------- | ------------------------------------------------------------------ |
| `test_config()`                                             | Temp config + workspace dirs, returns `(Config, TempDir, TempDir)` |
| `test_workspace()`                                          | `test_config()` + bootstrapped workspace                           |
| `test_database()`                                           | `test_workspace()` + connected SurrealDB                           |
| `write_test_note(workspace, title, body)`                   | Write a note file with frontmatter                                 |
| `write_test_reference(workspace, topic, filename, content)` | Write a reference file                                             |

### Mock Types (always available)

| Type/Function                          | Purpose                                                    |
| -------------------------------------- | ---------------------------------------------------------- |
| `MockProvider`                         | Queued `ChatResponse` provider; records all `ChatRequest`s |
| `MockProvider::requests()`             | Access recorded requests for assertion                     |
| `EchoTool`                             | Simple tool that returns `echo:{text}`                     |
| `response(content, stop_reason)`       | Build a `ChatResponse`                                     |
| `respond_response(message, citations)` | Build a response that calls the `respond` tool             |

### Live Test Helpers (`#[cfg(feature = "live-tests")]`)

Created via `let env = live_test_database("test_name").await;`

`LiveTestEnv` uses the real `~/.config/ghost/config.toml` (with real API keys) but
creates a fresh temp workspace and database.

#### Session Helpers

| Method                                              | Purpose                                |
| --------------------------------------------------- | -------------------------------------- |
| `env.create_session()`                              | Create a bare session, returns `Thing` |
| `env.session_with_messages(&[("role", "content")])` | Session with pre-filled messages       |

#### Chat Helpers

| Method                  | Purpose                                             |
| ----------------------- | --------------------------------------------------- |
| `env.chat()`            | `SessionChat` with real provider + chat tools       |
| `env.reflection_chat()` | `SessionChat` with real provider + reflection tools |

#### Job Runners

| Method                                              | Purpose                                                           |
| --------------------------------------------------- | ----------------------------------------------------------------- |
| `env.run_heartbeat(&session_id)`                    | Load prompt, run heartbeat via `chat_job`, return `JobTranscript` |
| `env.run_reflection(&session_id, previous_handoff)` | Load + interpolate reflection prompt, run with reflection tools   |

#### Agent Helpers

| Method                   | Purpose                                                       |
| ------------------------ | ------------------------------------------------------------- |
| `env.load_agent("name")` | Load agent definition from temp workspace (repo-current copy) |

#### Assertion Helpers

| Method                                         | Purpose                                           |
| ---------------------------------------------- | ------------------------------------------------- |
| `env.workspace_file_exists("relative/path")`   | Check file exists in temp workspace               |
| `env.read_workspace_file("relative/path")`     | Read file content from temp workspace             |
| `env.list_notes()`                             | List all files under `knowledge/notes/`           |
| `env.list_references()`                        | List all files under `knowledge/references/`      |
| `env.find_file_containing("subdir", "needle")` | Recursive content search under a workspace subdir |

#### Diagnostic Logging

| Method                                  | Purpose                                                  |
| --------------------------------------- | -------------------------------------------------------- |
| `env.log_session("label", &session_id)` | Dump all messages from a session into the diagnostic log |
| `env.log("custom message")`             | Add a note to the diagnostic log                         |

On drop, the diagnostic log is written to
`e2e-output/<timestamp>_<test_name>/diagnostic.log` and printed to stderr with
`--nocapture`.

### Writing a New Live Test

```rust
#[tokio::test]
async fn my_live_test() {
    let env = common::live_test_database("my_test").await;
    let session = env.session_with_messages(&[
        ("user", "Hello"),
        ("assistant", "Hi!"),
    ]).await;

    let result = env.run_heartbeat(&session).await;
    env.log_session("heartbeat", &session).await;

    assert!(!result.result.message.trim().is_empty());
}
```
