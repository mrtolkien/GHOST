# `ghost run` and `nix run`: unified script execution

## Problem

Ghost has five independent extension mechanisms that evolved separately:

1. **Lua agents** — 4,454 lines of Rust bridge code, fake sandbox, no typing
2. **Python scripts** — uv inline deps work, but native deps (pymupdf, poppler) leak
   through the nix shell flake, requiring flake rebuilds and bloating the base
   environment
3. **Nix shell flake** — accumulates permanent dependencies for tools used rarely or by
   single scripts
4. **Docker services** — config lives in root config.toml alongside core settings
5. **Built-in tools** — all compiled unconditionally into the binary

The GHOST is the primary extension author. It already self-extends by writing agent Lua
files, Python scripts, and editing the nix flake. But there's no unified way to:

- Run a script from a skill (`uv run` for Python, agent runner for Lua, shell for
  others)
- Manage running scripts (status, cancel, logs)
- Handle native dependencies without permanently polluting the shell flake

## Goal

Two changes that together solve the immediate problems:

1. **`nix run`** — use `nix run nixpkgs#package -- command` instead of requiring tools
   in the permanent shell flake. Nix caches after first invocation; garbage-collects
   when unused.
2. **`ghost run`** — a unified CLI command to run scripts from skill directories, with
   background job lifecycle (status, cancel, logs). Subsumes `ghost agent run`.

## Key design decisions

- **Skills remain the extension unit.** No new abstraction. Skills already have
  `scripts/` directories per agentskills.io convention; this formalizes how Ghost
  executes them.
- **`ghost run` dispatches by file type.** `.py` → uv, `.lua` → existing agent runner,
  `.wasm` → wasmtime (future). Ghost looks at what's there, no manifest needed.
- **`nix run` for native tool deps.** Scripts that need CLI tools (pdftoppm, pandoc,
  yt-dlp) use `nix run nixpkgs#package -- args` at the call site. No declaration file,
  no framework. If it fails, the GHOST gets the error and fixes it.
- **The shell flake stays lean.** Only tools the GHOST uses constantly (git, curl, uv,
  python3, ripgrep, etc.) live in the flake. Everything else is `nix run`.
- **`ghost run` subsumes `ghost agent run`.** One command for all runtimes. The existing
  `ghost agent` subcommands (list, validate, status, show) move under `ghost run`.
- **Config lives in the skill.** Skill-specific configuration (e.g., docling remote URL
  vs local mode) is stored in a file within the skill directory (e.g.,
  `skills/<name>/config.toml`). The extension reads its own config. Ghost's core
  `config.toml` is not involved.

## `nix run` for native dependencies

### What changes

Replace direct tool invocations with `nix run` wrappers where the tool is not guaranteed
to be in the shell flake.

**Before** (pdftoppm in `src/docling/vision.rs`):

```rust
let mut cmd = tokio::process::Command::new("pdftoppm");
// Prepend nix shell bin to PATH so pdftoppm is found.
if let Some(nix_bin) = read_shell_bin(workspace) {
    let current_path = std::env::var("PATH").unwrap_or_default();
    cmd.env("PATH", format!("{nix_bin}:{current_path}"));
}
```

**After:**

```rust
let mut cmd = tokio::process::Command::new("nix");
cmd.args(["run", "nixpkgs#poppler_utils", "--"]);
cmd.args(["pdftoppm", "-png", "-singlefile", ...]);
```

No PATH manipulation needed. Nix resolves the package, caches it in the store, and runs
the command. First invocation downloads; subsequent ones are instant.

### Helper function

A small utility to build nix-run commands:

```rust
/// Build a command that runs `tool` from `nix_package` via `nix run`.
pub fn nix_run_command(nix_package: &str, tool: &str) -> tokio::process::Command {
    let mut cmd = tokio::process::Command::new("nix");
    cmd.args(["run", &format!("nixpkgs#{nix_package}"), "--", tool]);
    cmd
}
```

### Candidates for migration

| Current call site             | Tool     | Nix package     | Keep in flake?                                   |
| ----------------------------- | -------- | --------------- | ------------------------------------------------ |
| `src/docling/vision.rs`       | pdftoppm | `poppler_utils` | No — only used during PDF import                 |
| `src/docling/convert.rs`      | uv       | `uv`            | **Yes** — used constantly for all Python scripts |
| `src/reference_import/git.rs` | git      | `git`           | **Yes** — core tool                              |
| `src/onboarding/`             | podman   | `podman`        | **Yes** — used for service management            |

The flake should keep: git, gh, curl, wget, jq, ripgrep, fd, tree, coreutils, findutils,
bash, uv, python3, sqlite-interactive. Everything else is a `nix run` candidate.

### For Python scripts

Python scripts that need native libraries (like pymupdf needing mupdf headers) should
use `nix run` in the skill.md instructions or in wrapper scripts. Example:

```bash
nix run nixpkgs#poppler_utils -- pdftoppm -png file.pdf output
```

The GHOST learns this pattern from skills and applies it. If a Python package fails to
install because of missing native deps, the error message guides the GHOST to use
`nix run` for the underlying tool instead.

## `ghost run` — unified script execution

### CLI interface

```
ghost run <skill>:<script> [args...]     Run a script from a skill
ghost run <skill>:<script> --background  Run in background
ghost run status [--limit N]             Show running/recent jobs
ghost run show <job-id> [--full]         Show job details
ghost run cancel <job-id>               Cancel a running job
ghost run list                           List all runnable scripts
ghost run test <skill>[:<script>]        Run tests for a skill (or specific script)
```

**Naming convention:** `ghost run <skill-name>:<script-name>`. The colon separates the
skill name from the script name. The script name is the filename stem (without
extension). Examples:

```
ghost run deep-research:agent            # Lua agent
ghost run image-generation:generate      # Python script
ghost run document-processing:convert    # Python script
```

### Discovery

`ghost run list` scans all skill directories for runnable scripts:

1. Walk `$WORKSPACE/skills/*/scripts/`
2. Walk `$WORKSPACE/agents/*/` (for backward compat with standalone agents)
3. For each directory, find runnable files: `*.py`, `*.lua`, `*.wasm`, and directories
   containing `Cargo.toml` (future WASM crates)
4. Display: `<skill>:<script> — <runtime> — <description if available>`

### Dispatch by file type

| Extension                   | Runtime          | How it runs                                        |
| --------------------------- | ---------------- | -------------------------------------------------- |
| `.py`                       | Python/uv        | `uv run <script.py> [args]`                        |
| `.lua`                      | Lua agent runner | Existing `AgentRunner::run()` path                 |
| `.wasm`                     | wasmtime         | Future — WASM agents spec                          |
| directory with `Cargo.toml` | Rust → WASM      | `cargo build --target wasm32-wasip2` then wasmtime |

Ghost infers the runtime from the file. No manifest declaring "this is python" needed.

### Background execution and job lifecycle

All `ghost run` invocations can run in foreground (default) or background
(`--background`).

**Foreground:**

- Streams stdout/stderr to the terminal
- Blocks until completion
- Ctrl+C cancels

**Background:**

- Returns immediately with a job ID
- Output captured to DB (reuse existing `agent_runs` table, generalized to
  `script_runs`)
- `ghost run status` shows all running/recent jobs
- `ghost run show <id>` shows output
- `ghost run cancel <id>` sends cancellation signal

This reuses the existing agent run infrastructure (`db/agent_runs.rs`, the background
agent runner in `agents/runner.rs`) but generalized. The `agent_runs` table becomes
`script_runs` (or is aliased — migration TBD).

### Subsumes `ghost agent`

Current `ghost agent` commands map to `ghost run`:

| Current                       | New                                   | Notes                                      |
| ----------------------------- | ------------------------------------- | ------------------------------------------ |
| `ghost agent list`            | `ghost run list`                      | Now shows all scripts, not just Lua agents |
| `ghost agent validate <name>` | `ghost run validate <skill>:<script>` | Validate Lua/WASM config                   |
| `ghost agent status`          | `ghost run status`                    | Shows all script runs                      |
| `ghost agent show <id>`       | `ghost run show <id>`                 | Unchanged                                  |

`ghost agent` can remain as a deprecated alias during transition.

### How the GHOST uses `ghost run`

The GHOST invokes `ghost run` through the shell tool, same as any other CLI command.
Skills teach the GHOST when and how to use it:

```markdown
<!-- In skill.md -->

## Scripts

- `ghost run document-processing:convert --path file.pdf --output out.md` Converts PDF
  to markdown using docling. Use `--no-ocr` for clean PDFs.
```

No dedicated `run_script` tool needed. The shell tool is the interface. Progressive
disclosure: the GHOST only knows about scripts that its loaded skills mention.

## Skill-level configuration

Extensions that need configuration store it in their own skill directory:

```
skills/
  document-processing/
    skill.md
    config.toml          ← skill-specific config
    scripts/
      convert.py
```

**Example `config.toml` for docling:**

```toml
# Remote docling-serve URL. Omit or leave empty for local uv mode.
# url = "http://localhost:5001"
timeout = 600
device = "auto"  # auto | cpu | cuda | mps
```

The script reads its own `config.toml` at runtime (via relative path or a `--config`
argument). The GHOST edits this file directly when configuration needs to change, same
as it edits `flake.nix` today. No framework, no schema declaration — just a TOML file
that the extension owns.

Ghost's core `config.toml` loses the `[docling]` section (and eventually other
extension-specific sections) as those migrate to skill-level config.

## Migration path

### Phase 1: `nix run` adoption

1. Add `nix_run_command()` helper to `src/tools/shell.rs` (or a new `src/nix.rs`)
2. Migrate `pdftoppm` call in `src/docling/vision.rs` to use `nix run`
3. Remove `poppler_utils` from `assets/shell/flake.nix`
4. Update skill docs to teach the GHOST the `nix run` pattern

### Phase 2: `ghost run` for Python scripts

1. Add `ghost run` CLI subcommand with dispatch logic
2. Implement Python script discovery and `uv run` dispatch
3. Add background execution with job tracking (reuse agent run infra)
4. Add `ghost run status`, `ghost run show`, `ghost run cancel`

### Phase 2.5: `ghost run test`

1. Add `ghost run test` subcommand with test discovery and dispatch
2. Write smoke tests for all bundled Python scripts
3. Add `just test-scripts` target
4. Wire into CI

### Phase 3: `ghost run` for Lua agents

1. Wire Lua agent execution through `ghost run` dispatch
2. Migrate `ghost agent run` to `ghost run <skill>:agent`
3. Keep `ghost agent list/validate/status/show` as aliases during transition
4. Update crontab system to use `skill:script` paths

### Phase 4: skill-level config migration

1. Move `[docling]` config from core config.toml to skill config.toml
2. Update `convert.py` and Rust docling code to read from skill directory
3. Repeat for other extension-specific config sections as they arise

### Future: WASM runtime

Phase 5+ per the existing WASM agents spec
(`backlog/tasks/5-management-safety/wasm-agents.md`). `ghost run` gains `.wasm` and
Cargo crate dispatch. `agent!` and `script!` macros provide the authoring experience.

## Testing scripts

A major gap today: bundled and user-created scripts have no testing story. `ghost run`
introduces one.

### Convention

Tests live in `skills/<name>/tests/` alongside the scripts they test:

```
skills/
  document-processing/
    skill.md
    config.toml
    scripts/
      convert.py
    tests/
      test_convert.py        ← tests for convert.py
      fixtures/
        sample.pdf           ← test fixtures

  image-generation/
    skill.md
    scripts/
      generate.py
    tests/
      test_generate.py
```

### CLI

```
ghost run test <skill>                Run all tests for a skill
ghost run test <skill>:<script>       Run tests for a specific script
ghost run test --all                  Run all tests across all skills
```

### How tests run

Tests are dispatched by file type, same as scripts:

| Test file     | How it runs                                                        |
| ------------- | ------------------------------------------------------------------ |
| `test_*.py`   | `uv run pytest <test_file>` (or `uv run <test_file>` if no pytest) |
| `test_*.lua`  | Load in ScriptHost, call `test()` export                           |
| `test_*.wasm` | Future — WASM test harness                                         |

Ghost discovers tests by scanning `skills/*/tests/` for files matching `test_*.*`.

### What tests verify

Tests for scripts should cover:

- **Smoke test**: does the script run without crashing with minimal valid input?
- **Output format**: does it produce the expected output structure (JSON, markdown,
  etc.)?
- **Error handling**: does it fail cleanly with bad input (missing file, invalid args)?
- **Idempotency**: running twice with the same input produces the same output.

For Python scripts, tests can use pytest or be standalone scripts that exit 0 on success
and non-zero on failure. The simplest valid test:

```python
#!/usr/bin/env python3
# /// script
# requires-python = ">=3.10"
# dependencies = []
# ///
"""Smoke test for convert.py — verifies it fails cleanly with no arguments."""
import subprocess
import sys

result = subprocess.run(
    ["uv", "run", "scripts/convert.py"],
    capture_output=True, text=True, cwd=".."
)
# Should fail with usage error, not crash
assert result.returncode != 0
assert "error" in result.stderr.lower() or "usage" in result.stderr.lower()
print("PASS: convert.py fails cleanly with no args")
```

### Testing bundled scripts

Scripts bundled with Ghost (in `assets/skills/*/scripts/`) should have tests in
`assets/skills/*/tests/`. These run as part of `just ci` or a dedicated
`just test-scripts` target.

This catches regressions before they ship — the pymupdf breakage would have been caught
by a smoke test that actually invoked the script.

### How the GHOST tests its own scripts

When the GHOST creates or modifies a script, it should also write/update a test:

1. GHOST writes `skills/foo/scripts/bar.py`
2. GHOST writes `skills/foo/tests/test_bar.py`
3. GHOST runs `ghost run test foo:bar` to verify
4. If the test fails, the GHOST reads the error output and fixes the script

Skills can teach this workflow explicitly:

```markdown
<!-- In skill.md -->

## Development

After modifying any script, run its tests: `ghost run test document-processing:convert`
```

### Migration to testing

Phase 2.5 (after `ghost run` exists, before Lua agent migration):

1. Add `ghost run test` subcommand with discovery and dispatch
2. Write smoke tests for all bundled Python scripts (`assets/skills/*/tests/`)
3. Add `just test-scripts` target that runs `ghost run test --all`
4. Add smoke tests to CI pipeline

## Open questions

- **Agent discovery paths.** Currently agents are found in both `agents/` and
  `skills/*/`. Should `ghost run` unify to `skills/*/scripts/` only, or keep both?
- **Crontab migration.** The current `crontab.lua` references agent names. When agents
  move under `ghost run`, does the crontab format change to `skill:script` paths?
- **Service definitions.** Should services (docker-compose fragments) also live in skill
  directories? Deferred — not enough urgency to design now.
