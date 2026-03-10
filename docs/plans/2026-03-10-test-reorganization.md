# Test Reorganization Plan

## Problem

The naming is confusing:

- "e2e tests" (`tests/e2e/`, `tests/e2e_steps.rs`, `--features e2e-tests`) are actually
  **stepwise fixture tests** — they snapshot and restore state between steps, and don't
  boot the daemon.
- "daemon tests" (`tests/daemon.rs`, `tests/daemon/`) are the **true end-to-end tests**
  — they boot the real daemon and exercise the full stack.
- The `live-tests` feature is a grab-bag: it covers both LLM-hitting tests (expensive)
  and local-service tests (Ollama, SearXNG, crawl4ai — cheap).

## Goals

1. **Rename** stepwise fixture tests from "e2e" to "stepwise".
2. **Rename** daemon tests to "e2e" since they're the real end-to-end tests.
3. **Split features** so you can run local-service tests without paying for LLM calls.
4. **Update the e2e-testing skill** to document both test approaches clearly.

## New Feature Flags

```toml
[features]
live-tests = []                  # base infra (LiveTestEnv) + service tests
live-tests-llms = ["live-tests"] # tests that call paid LLM providers
```

The `e2e-tests` feature is removed entirely. Stepwise tests and e2e tests both gate on
`live-tests-llms`.

### Which tests go where

| Feature           | Test files                                                                                                                                                                       | What they hit                        |
| ----------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------ |
| `live-tests`      | `embedding_live.rs`, `searxng_live.rs`, `web_fetch_live.rs`, `reference_import_crawl.rs`, `reference_import_git.rs`                                                              | Ollama, SearXNG, crawl4ai, git repos |
| `live-tests-llms` | `e2e.rs` (was daemon.rs), `chat_orchestration_live.rs`, `codex_tools_live.rs`, `stepwise.rs`, `providers/{cache,codex_turn_state,image,openai_oauth,reasoning,tool_use}_live.rs` | OpenRouter, OpenAI, Codex APIs       |

## File Renames

Two renames happen in sequence (to avoid collision on `tests/e2e/`):

### Step 1: Stepwise fixture tests (old "e2e" → "stepwise")

| Old                  | New                 |
| -------------------- | ------------------- |
| `tests/e2e_steps.rs` | `tests/stepwise.rs` |
| `tests/e2e/`         | `tests/stepwise/`   |

### Step 2: Daemon tests → e2e

| Old               | New            |
| ----------------- | -------------- |
| `tests/daemon.rs` | `tests/e2e.rs` |
| `tests/daemon/`   | `tests/e2e/`   |

The redundant `#[path = "daemon/..."]` directives in the old `daemon.rs` are dropped.
Rust resolves `mod ark_nova;` in `tests/e2e.rs` to `tests/e2e/ark_nova.rs`
automatically.

Fixture directory stays at `tests/fixtures/e2e/` (refers to stepwise fixtures — no
reason to rename data dirs).

## Feature Gate Changes

| File                                   | Old                           | New                                |
| -------------------------------------- | ----------------------------- | ---------------------------------- |
| `Cargo.toml`                           | `e2e-tests = ["live-tests"]`  | `live-tests-llms = ["live-tests"]` |
| `tests/stepwise.rs` (was e2e_steps.rs) | `cfg(feature = "e2e-tests")`  | `cfg(feature = "live-tests-llms")` |
| `tests/e2e.rs` (was daemon.rs)         | `cfg(feature = "live-tests")` | `cfg(feature = "live-tests-llms")` |
| `tests/chat_orchestration_live.rs`     | `cfg(feature = "live-tests")` | `cfg(feature = "live-tests-llms")` |
| `tests/codex_tools_live.rs`            | `cfg(feature = "live-tests")` | `cfg(feature = "live-tests-llms")` |
| `tests/providers/mod.rs` (6 modules)   | `cfg(feature = "live-tests")` | `cfg(feature = "live-tests-llms")` |
| `tests/common.rs`                      | `cfg(feature = "live-tests")` | No change (base infra)             |
| `tests/embedding_live.rs`              | `cfg(feature = "live-tests")` | No change                          |
| `tests/searxng_live.rs`                | `cfg(feature = "live-tests")` | No change                          |
| `tests/web_fetch_live.rs`              | `cfg(feature = "live-tests")` | No change                          |
| `tests/reference_import_crawl.rs`      | `cfg(feature = "live-tests")` | No change                          |
| `tests/reference_import_git.rs`        | `cfg(feature = "live-tests")` | No change                          |

## Other Updates

| File                                  | Change                                                                                                                 |
| ------------------------------------- | ---------------------------------------------------------------------------------------------------------------------- |
| `justfile:40`                         | `e2e-check` → `stepwise-check`, `--features e2e-tests --test e2e_steps` → `--features live-tests-llms --test stepwise` |
| `scripts/e2e/refresh.py:62-64`        | `"e2e-tests"` → `"live-tests-llms"`, `"e2e_steps"` → `"stepwise"`                                                      |
| `.agents/skills/e2e-testing/SKILL.md` | Full rewrite to document both approaches (e2e = daemon-boot tests, stepwise = fixture-chain tests)                     |
| `MEMORY.md`                           | Update feature flag and path references                                                                                |

## Execution Order

1. `git mv tests/e2e tests/stepwise` + create `tests/stepwise.rs` + delete
   `tests/e2e_steps.rs`
2. `git mv tests/daemon tests/e2e` + `git mv tests/daemon.rs tests/e2e.rs` + drop
   `#[path]` directives
3. Update `Cargo.toml` features
4. Re-gate all test files
5. Update justfile + scripts
6. Rewrite e2e-testing skill
7. Update MEMORY.md
8. `just ci`
