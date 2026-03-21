# `ghost status` Implementation Plan

> **For agentic workers:** Use superpowers:executing-plans to implement this plan
> task-by-task.

**Goal:** Add a `ghost status` CLI command that reports config validity, daemon status,
and external service health.

**Architecture:** Extract shared health-check primitives (`probe_url`, `HealthResult`,
`display_health_table`) from `onboarding::health` into a new top-level `src/health.rs`
module. Create `src/cli/status.rs` that loads the resolved `Config` and probes each
configured service URL. Wire into `main.rs`.

---

### Task 1: Extract shared health primitives to `src/health.rs`

**Files:**

- Create: `src/health.rs`
- Modify: `src/lib.rs` (add `pub mod health;`)
- Modify: `src/onboarding/health.rs` (re-import from `crate::health`)

- [ ] **Step 1: Create `src/health.rs`** with `probe_url`, `HealthResult`,
      `display_health_table` moved from `onboarding::health`.

- [ ] **Step 2: Update `src/onboarding/health.rs`** to import `probe_url` and
      `HealthResult` from `crate::health` instead of defining them locally. Keep
      `check_all_services`, `probe_choice`, and all onboarding-specific logic in place.

- [ ] **Step 3: Add `pub mod health;`** to `src/lib.rs`.

- [ ] **Step 4: Run `just ci`** to verify nothing broke.

- [ ] **Step 5: Commit** — `refactor: extract shared health primitives to src/health.rs`

---

### Task 2: Create `src/cli/status.rs`

**Files:**

- Create: `src/cli/status.rs`
- Modify: `src/cli/mod.rs` (add `pub mod status;`)

- [ ] **Step 1: Create `src/cli/status.rs`** with `pub async fn execute()`:
  1. **Config validity** — `config::load()`, print path + ok/error
  2. **Daemon status** — `systemctl --user is-active ghost-daemon` (Linux) /
     `launchctl print` (macOS), plus probe `http://127.0.0.1:7432/health`
  3. **Services** — build `Vec<HealthResult>` by probing:
     - `config.embeddings.url` + `/health`
     - Search URL (SearXNG) or label (Brave API)
     - `config.web.crawl4ai_url` + `/health`
     - `config.docling.url` + `/health`
     - Each browser CDP URL
  4. Display via `display_health_table`

- [ ] **Step 2: Add `pub mod status;`** to `src/cli/mod.rs`.

- [ ] **Step 3: Run `cargo check`** to verify compilation.

---

### Task 3: Wire up CLI command in `main.rs`

**Files:**

- Modify: `src/main.rs`

- [ ] **Step 1: Add `Status` variant** to `Commands` enum with doc comment.

- [ ] **Step 2: Add dispatch arm**
      `Commands::Status => ghost::cli::status::execute().await`.

- [ ] **Step 3: Run `just ci`** — everything compiles and passes.

- [ ] **Step 4: Manual test** — `cargo run -- status`.

- [ ] **Step 5: Commit** — `feat: add ghost status command`
