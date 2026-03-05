# Coding Agent (`ghost hack`) Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this
> plan task-by-task.

**Goal:** Enable GHOST to spawn a Rust-native coding agent that takes over the
OPERATOR's Discord channel with repo-aware context and direct OPERATOR interaction.

**Architecture:** Reuses `SessionChat` + tool loop with a different system prompt,
skills, and session. Channel takeover via DB-level session override in Discord handler.
CLI commands (`ghost hack start/resume/list`) triggered by GHOST via
`run_shell_command`.

**Tech Stack:** Rust (same crate), SQLite (new migration), clap (CLI), Tera-style
template rendering, uv (sync script).

**Design doc:** `docs/plans/2026-03-05-coding-agent-design.md`

---

## Phase 1: Superpowers Vendor + Sync

### Task 1: Write sync script

**Files:**

- Create: `scripts/sync-superpowers.py`

**Step 1: Write the sync script**

```python
# /// script
# requires-python = ">=3.11"
# dependencies = ["rich"]
# ///
"""
Vendor superpowers skills from obra/superpowers into vendor/superpowers/.

Usage:
    uv run scripts/sync-superpowers.py          # fetch + show diff
    uv run scripts/sync-superpowers.py --apply  # update vendor dir
"""

import argparse
import difflib
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

from rich.console import Console
from rich.syntax import Syntax

ROOT = Path(__file__).resolve().parent.parent
VENDOR_DIR = ROOT / "vendor" / "superpowers"
REPO_URL = "https://github.com/obra/superpowers.git"

console = Console()


def clone_repo(tmp: Path) -> Path:
    subprocess.run(
        ["git", "clone", "--depth=1", REPO_URL, str(tmp / "repo")],
        check=True,
        capture_output=True,
    )
    return tmp / "repo"


def collect_skills(repo: Path) -> dict[str, str]:
    skills_dir = repo / "skills"
    result = {}
    if not skills_dir.exists():
        return result
    for skill_dir in sorted(skills_dir.iterdir()):
        if not skill_dir.is_dir():
            continue
        skill_file = skill_dir / "SKILL.md"
        if skill_file.exists():
            result[skill_dir.name] = skill_file.read_text()
    return result


def show_diff(old_skills: dict[str, str], new_skills: dict[str, str]) -> bool:
    changed = False
    all_names = sorted(set(old_skills) | set(new_skills))
    for name in all_names:
        old = old_skills.get(name, "")
        new = new_skills.get(name, "")
        if old == new:
            continue
        changed = True
        diff = difflib.unified_diff(
            old.splitlines(keepends=True),
            new.splitlines(keepends=True),
            fromfile=f"vendor/{name}/SKILL.md",
            tofile=f"upstream/{name}/SKILL.md",
        )
        console.print(f"\n[bold]{name}[/bold]:")
        console.print(Syntax("".join(diff), "diff"))
    return changed


def load_vendored() -> dict[str, str]:
    result = {}
    if not VENDOR_DIR.exists():
        return result
    for skill_dir in sorted(VENDOR_DIR.iterdir()):
        if not skill_dir.is_dir():
            continue
        skill_file = skill_dir / "SKILL.md"
        if skill_file.exists():
            result[skill_dir.name] = skill_file.read_text()
    return result


def apply(new_skills: dict[str, str]) -> None:
    if VENDOR_DIR.exists():
        shutil.rmtree(VENDOR_DIR)
    VENDOR_DIR.mkdir(parents=True)
    for name, content in sorted(new_skills.items()):
        skill_dir = VENDOR_DIR / name
        skill_dir.mkdir()
        (skill_dir / "SKILL.md").write_text(content)
    console.print(f"\n[green]Vendored {len(new_skills)} skills to {VENDOR_DIR}[/green]")
    console.print("[yellow]Review diffs and port changes to prompts/skills/ manually.[/yellow]")


def main() -> None:
    parser = argparse.ArgumentParser(description="Sync superpowers skills")
    parser.add_argument("--apply", action="store_true", help="Update vendor dir")
    args = parser.parse_args()

    with tempfile.TemporaryDirectory() as tmp:
        console.print("[dim]Cloning obra/superpowers...[/dim]")
        repo = clone_repo(Path(tmp))
        new_skills = collect_skills(repo)

    console.print(f"Found {len(new_skills)} upstream skills")

    old_skills = load_vendored()
    changed = show_diff(old_skills, new_skills)

    if not changed:
        console.print("[green]No changes from upstream.[/green]")
        return

    if args.apply:
        apply(new_skills)
    else:
        console.print("\n[yellow]Run with --apply to update vendor dir.[/yellow]")


if __name__ == "__main__":
    main()
```

**Step 2: Run the sync script**

Run: `uv run scripts/sync-superpowers.py --apply` Expected: Clones repo, vendors all
skills into `vendor/superpowers/`

**Step 3: Verify vendor directory**

Run: `ls vendor/superpowers/` Expected: 14 skill directories (brainstorming,
dispatching-parallel-agents, etc.)

**Step 4: Commit**

```bash
git add scripts/sync-superpowers.py vendor/superpowers/
git commit -m "chore: add superpowers sync script and initial vendor"
```

### Task 2: Port superpowers skills to prompts/skills/

**Files:**

- Create: `prompts/skills/brainstorming.md`
- Create: `prompts/skills/writing-plans.md`
- Create: `prompts/skills/executing-plans.md`
- Create: `prompts/skills/subagent-development.md`
- Create: `prompts/skills/parallel-agents.md`
- Create: `prompts/skills/tdd.md`
- Create: `prompts/skills/systematic-debugging.md`
- Create: `prompts/skills/verification.md`
- Create: `prompts/skills/requesting-review.md`
- Create: `prompts/skills/receiving-review.md`
- Create: `prompts/skills/git-worktrees.md`
- Create: `prompts/skills/finishing-branch.md`
- Create: `prompts/skills/writing-skills.md`
- Modify: `src/skills.rs` — add these to `DEFAULT_SKILLS` array

For each skill, read `vendor/superpowers/<name>/SKILL.md` and port with these
adaptations:

- `SKILL.md` -> `skill.md` format (agentskills.io frontmatter)
- `Skill tool` / `invoke skill` -> `read_file("skills/<name>/skill.md")`
- `Agent tool` / `Task tool` -> `agent_control(action: "start", ...)`
- `TodoWrite` -> `todo(action: "plan", ...)`
- `EnterPlanMode` -> remove
- `your human partner` -> `the OPERATOR`
- Remove Claude Code-specific references (plugins, /commands, IDE)
- Keep all workflow wisdom, rationalization tables, red flags, checklists

**Step 1: Port each skill file**

Read each vendored skill, apply adaptations, write to `prompts/skills/<name>.md` with
agentskills.io frontmatter. Use the existing skills in `prompts/skills/` as format
reference.

**Step 2: Register in DEFAULT_SKILLS**

Add all 13 new skills to the `DEFAULT_SKILLS` array in `src/skills.rs`
(using-superpowers is NOT a skill file — it goes into the coding agent prompt directly).

**Step 3: Replace skill-creator**

The existing `skill-creator` skill in `DEFAULT_SKILLS` should be replaced by the ported
`writing-skills` skill. Remove the old `prompts/skills/skill-creator.md` file and its
`DEFAULT_SKILLS` entry.

**Step 4: Update test assertion**

In `src/skills.rs`, the test `install_default_skills_creates_files` asserts
`DEFAULT_SKILLS.len() == 8`. Update to match the new count (8 - 1 + 13 = 20).

**Step 5: Run tests**

Run: `just ci` Expected: All tests pass, no clippy warnings.

**Step 6: Commit**

```bash
git add prompts/skills/ src/skills.rs
git commit -m "feat: port superpowers skills to ghost skill format"
```

---

## Phase 2: Config + DB Schema

### Task 3: Add `[coding]` config section

**Files:**

- Modify: `src/config.rs`

**Step 1: Add CodingSettings and CodingConfig structs**

In `src/config.rs`, add near the other settings structs:

```rust
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CodingSettings {
    pub model: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CodingConfig {
    pub model: Option<String>,
}
```

**Step 2: Add to Settings and Config**

Add `pub coding: Option<CodingSettings>` to `Settings`. Add `pub coding: CodingConfig`
to `Config`.

**Step 3: Add resolution in Config::from_settings()**

```rust
coding: CodingConfig {
    model: settings
        .coding
        .as_ref()
        .and_then(|c| c.model.clone()),
},
```

**Step 4: Add to test_config()**

```rust
coding: CodingConfig {
    model: None,
},
```

**Step 5: Run tests**

Run: `just ci` Expected: All tests pass.

**Step 6: Commit**

```bash
git add src/config.rs
git commit -m "feat: add [coding] config section with optional model override"
```

### Task 4: Add coding_sessions DB table

**Files:**

- Create: `migrations/004_coding_sessions.sql`

**Step 1: Write the migration**

```sql
CREATE TABLE coding_sessions (
    id          TEXT PRIMARY KEY,
    session_id  TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
    channel_id  TEXT,
    working_dir TEXT NOT NULL,
    status      TEXT NOT NULL DEFAULT 'active',
    started_at  TEXT NOT NULL,
    ended_at    TEXT
);

CREATE INDEX idx_coding_sessions_channel ON coding_sessions(channel_id)
    WHERE status = 'active';
CREATE INDEX idx_coding_sessions_status ON coding_sessions(status);
```

**Step 2: Run to verify migration applies**

Run: `cargo test -- --ignored db 2>&1 | head -20` (or any DB test to trigger migration)
Expected: No migration errors.

**Step 3: Commit**

```bash
git add migrations/004_coding_sessions.sql
git commit -m "feat: add coding_sessions table for channel takeover"
```

### Task 5: Add coding session DB queries

**Files:**

- Create: `src/db/coding_sessions.rs`
- Modify: `src/db/mod.rs`

**Step 1: Write the CRUD functions**

```rust
use sqlx::SqlitePool;

use crate::db::DatabaseError;

pub async fn create_coding_session(
    db: &SqlitePool,
    id: &str,
    session_id: &str,
    channel_id: Option<&str>,
    working_dir: &str,
) -> Result<(), DatabaseError> {
    let now = chrono::Utc::now().to_rfc3339();
    sqlx::query(
        "INSERT INTO coding_sessions (id, session_id, channel_id, working_dir, status, started_at)
         VALUES (?, ?, ?, ?, 'active', ?)",
    )
    .bind(id)
    .bind(session_id)
    .bind(channel_id)
    .bind(working_dir)
    .bind(&now)
    .execute(db)
    .await?;
    Ok(())
}

pub async fn get_active_takeover(
    db: &SqlitePool,
    channel_id: &str,
) -> Result<Option<(String, String, String)>, DatabaseError> {
    // Returns (coding_session_id, session_id, working_dir)
    let row = sqlx::query_as::<_, (String, String, String)>(
        "SELECT id, session_id, working_dir FROM coding_sessions
         WHERE channel_id = ? AND status = 'active'
         LIMIT 1",
    )
    .bind(channel_id)
    .fetch_optional(db)
    .await?;
    Ok(row)
}

pub async fn end_coding_session(
    db: &SqlitePool,
    id: &str,
) -> Result<(), DatabaseError> {
    let now = chrono::Utc::now().to_rfc3339();
    sqlx::query(
        "UPDATE coding_sessions SET status = 'ended', ended_at = ? WHERE id = ?",
    )
    .bind(&now)
    .bind(id)
    .execute(db)
    .await?;
    Ok(())
}

pub async fn list_recent_coding_sessions(
    db: &SqlitePool,
    limit: u32,
) -> Result<Vec<(String, String, String, String, String)>, DatabaseError> {
    // Returns (id, session_id, working_dir, status, started_at)
    let rows = sqlx::query_as::<_, (String, String, String, String, String)>(
        "SELECT id, session_id, working_dir, status, started_at
         FROM coding_sessions
         ORDER BY started_at DESC
         LIMIT ?",
    )
    .bind(limit)
    .fetch_all(db)
    .await?;
    Ok(rows)
}
```

**Step 2: Add module to db/mod.rs**

Add `pub mod coding_sessions;` to `src/db/mod.rs`.

**Step 3: Run tests**

Run: `just ci` Expected: Compiles, no warnings.

**Step 4: Commit**

```bash
git add src/db/coding_sessions.rs src/db/mod.rs
git commit -m "feat: add coding_sessions DB queries"
```

---

## Phase 3: Coding Agent Core

### Task 6: Write the coding agent system prompt

**Files:**

- Create: `prompts/coding-agent.md`

**Step 1: Write the prompt template**

The prompt should contain:

- Identity section: "You are a coding agent working in `{{ working_dir }}`"
- Full `using-superpowers` content (copy from vendored
  `vendor/superpowers/using-superpowers/SKILL.md`, adapt tool references)
- Workflow: explore repo -> understand task -> brainstorm -> plan -> implement -> verify
- Tool guidance: use `read_file` to read skills before starting work, commit
  incrementally, run tests after changes
- OPERATOR communication: ask questions directly, don't assume, one question at a time

The prompt template uses `{{ variable_name }}` syntax for:

- `{{ working_dir }}` — repo path
- `{{ repo_context }}` — AGENTS.md/CLAUDE.md content (may be empty)
- `{{ coding_skills }}` — skill listing XML (same format as chat prompt)
- `{{ model_info }}` — model name, provider

Keep the core prompt concise. The `using-superpowers` content is the bulk of it.

**Step 2: Verify prompt file is valid**

Read it back and check template variables are consistent.

**Step 3: Commit**

```bash
git add prompts/coding-agent.md
git commit -m "feat: add coding agent system prompt template"
```

### Task 7: Create the coding module

**Files:**

- Create: `src/coding/mod.rs`
- Create: `src/coding/session.rs`
- Create: `src/coding/prompt.rs`
- Modify: `src/lib.rs` — add `pub mod coding;`

**Step 1: Create mod.rs (barrel file)**

```rust
pub mod prompt;
pub mod session;
```

**Step 2: Write prompt.rs — prompt builder for the coding agent**

This module builds the coding agent's system prompt. It should:

1. Load the base template (`prompts/coding-agent.md`, embedded via `include_str!`)
2. Read AGENTS.md or CLAUDE.md from `working_dir` (if present)
3. Discover skills from three sources:
   - `$WORKSPACE/skills/` (existing `discover_skills()`)
   - `.agents/skills/` from `working_dir` (new scan)
   - Superpowers skills from `$WORKSPACE/skills/` are already included via
     `discover_skills()`
4. Format skills in the same XML format as `build_ghost_skills()` in
   `src/prompt/context.rs`
5. Render the template with variables

```rust
use std::fs;
use std::path::Path;

use crate::config::Config;
use crate::prompt::template::render_template;
use crate::skills;

const CODING_AGENT_PROMPT: &str = include_str!("../../prompts/coding-agent.md");

pub fn build_coding_prompt(config: &Config, working_dir: &Path) -> String {
    let repo_context = load_repo_context(working_dir);
    let coding_skills = build_coding_skills(&config.workspace, working_dir);
    let model_info = build_model_info(config);

    let mut vars = std::collections::HashMap::new();
    vars.insert("working_dir", working_dir.display().to_string());
    vars.insert("repo_context", repo_context);
    vars.insert("coding_skills", coding_skills);
    vars.insert("model_info", model_info);

    render_template(CODING_AGENT_PROMPT, &vars)
}

fn load_repo_context(working_dir: &Path) -> String {
    for name in ["AGENTS.md", "CLAUDE.md"] {
        let path = working_dir.join(name);
        if let Ok(content) = fs::read_to_string(&path) {
            return format!("## Project Conventions ({name})\n\n{content}");
        }
    }
    String::new()
}

fn build_coding_skills(workspace: &Path, working_dir: &Path) -> String {
    let mut all_skills = skills::discover_skills(workspace);

    // Also discover repo-local skills from .agents/skills/
    let repo_skills_dir = working_dir.join(".agents").join("skills");
    if repo_skills_dir.is_dir() {
        // Use a temporary workspace-like scan
        // (reuse discover_skills logic but pointed at repo)
        let repo_skills = discover_repo_skills(&repo_skills_dir);
        all_skills.extend(repo_skills);
    }

    // Format as XML (same as build_ghost_skills in src/prompt/context.rs)
    if all_skills.is_empty() {
        return String::new();
    }

    let entries: Vec<String> = all_skills
        .iter()
        .map(|s| {
            format!(
                "  <skill>\n    <name>{}</name>\n    \
                 <description>{}</description>\n    \
                 <location>{}</location>\n  </skill>",
                s.name, s.description, s.path.display(),
            )
        })
        .collect();

    format!(
        "## Available Skills\n\n\
         ALWAYS read the full skill file with `read_file` before starting any task \
         that matches a skill's description. Skills contain critical workflow \
         instructions that you MUST follow.\n\n\
         <available_skills>\n{}\n</available_skills>",
        entries.join("\n"),
    )
}

fn discover_repo_skills(skills_dir: &Path) -> Vec<skills::Skill> {
    // Same logic as skills::discover_skills but for an arbitrary dir
    let entries = match fs::read_dir(skills_dir) {
        Ok(e) => e,
        Err(_) => return Vec::new(),
    };

    let mut found = Vec::new();
    for entry in entries.flatten() {
        if entry.file_name().to_string_lossy().starts_with('.') {
            continue;
        }
        let skill_path = entry.path().join("skill.md");
        let content = match fs::read_to_string(&skill_path) {
            Ok(c) => c,
            Err(_) => continue,
        };
        if let Some((name, description)) = skills::parse_frontmatter(&content) {
            found.push(skills::Skill {
                name,
                description,
                path: skill_path,
            });
        }
    }
    found.sort_by(|a, b| a.name.cmp(&b.name));
    found
}
```

Note: The exact implementation will depend on what `render_template` expects. Check
`src/prompt/template.rs` for the signature. The code above is a guide — adapt types as
needed.

**Step 3: Write session.rs — coding session lifecycle**

This module manages starting, resuming, and ending coding sessions.

```rust
use std::path::PathBuf;

use crate::chat::session::SessionChat;
use crate::config::Config;
use crate::db;
use crate::db::GhostDb;

pub struct CodingSession {
    pub id: String,
    pub session_id: String,
    pub working_dir: PathBuf,
    pub channel_id: Option<String>,
}

/// Start a new coding session. Creates a chat session, registers the
/// coding session in the DB, and returns the IDs needed for takeover.
pub async fn start(
    db: &GhostDb,
    config: &Config,
    working_dir: PathBuf,
    channel_id: Option<String>,
    prompt: Option<String>,
) -> Result<CodingSession, CodingError> {
    let session_id = db::sessions::create_session(db).await?;
    let coding_id = ulid::Ulid::new().to_string();

    db::coding_sessions::create_coding_session(
        db,
        &coding_id,
        &session_id,
        channel_id.as_deref(),
        &working_dir.display().to_string(),
    )
    .await?;

    // If there's an initial prompt, store it as the first user message
    if let Some(prompt) = prompt {
        db::sessions::create_message(db, &session_id, "user", &prompt, None, None)
            .await?;
    }

    Ok(CodingSession {
        id: coding_id,
        session_id,
        working_dir,
        channel_id,
    })
}

/// End a coding session. Generates deterministic summary from git state.
pub async fn end(
    db: &GhostDb,
    coding_session_id: &str,
    working_dir: &std::path::Path,
) -> Result<String, CodingError> {
    db::coding_sessions::end_coding_session(db, coding_session_id).await?;
    generate_summary(working_dir).await
}

/// Generate a deterministic summary from git log + diff --stat.
/// No LLM involved — pure git commands.
async fn generate_summary(working_dir: &std::path::Path) -> Result<String, CodingError> {
    // Get current branch
    let branch = run_git(working_dir, &["rev-parse", "--abbrev-ref", "HEAD"]).await?;

    // Get recent commits (since session start — use last 20 as approximation)
    let log = run_git(
        working_dir,
        &["log", "--oneline", "-20", "--no-decorate"],
    )
    .await
    .unwrap_or_default();

    // Get diff stat
    let stat = run_git(working_dir, &["diff", "--stat", "HEAD~20..HEAD"])
        .await
        .unwrap_or_default();

    let mut summary = String::new();
    summary.push_str(&format!("Branch: {branch}\n"));
    if !log.is_empty() {
        summary.push_str(&format!("\nCommits:\n{log}\n"));
    }
    if !stat.is_empty() {
        summary.push_str(&format!("\nChanged:\n{stat}\n"));
    }
    Ok(summary)
}

async fn run_git(dir: &std::path::Path, args: &[&str]) -> Result<String, CodingError> {
    let output = tokio::process::Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .await
        .map_err(|e| CodingError::Git(e.to_string()))?;
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

#[derive(Debug, thiserror::Error)]
pub enum CodingError {
    #[error("database error: {0}")]
    Database(#[from] db::DatabaseError),
    #[error("git error: {0}")]
    Git(String),
    #[error("working directory not found: {0}")]
    WorkingDirNotFound(String),
}
```

Note: The summary generation is approximate (last 20 commits). A more precise approach
would store a "base commit" at session start and diff against that. Consider adding
`base_commit TEXT` to the `coding_sessions` table — but this can be refined later.

**Step 4: Add module to lib.rs**

Add `pub mod coding;` to `src/lib.rs`.

**Step 5: Run tests**

Run: `just ci` Expected: Compiles, no warnings.

**Step 6: Commit**

```bash
git add src/coding/ src/lib.rs
git commit -m "feat: add coding module with session lifecycle and prompt builder"
```

---

## Phase 4: CLI Commands

### Task 8: Add `ghost hack` CLI subcommand

**Files:**

- Create: `src/cli/hack.rs`
- Modify: `src/cli/mod.rs`
- Modify: `src/main.rs`

**Step 1: Write hack.rs**

```rust
use clap::Subcommand;
use std::path::PathBuf;

use crate::coding;
use crate::config;
use crate::error::GhostError;

#[derive(Debug, Subcommand)]
pub enum HackCommand {
    /// Start a new coding session
    Start {
        /// Working directory (relative to workspace)
        dir: String,
        /// Initial prompt for the coding agent
        #[arg(long)]
        prompt: Option<String>,
    },
    /// Resume a previous coding session
    Resume {
        /// Coding session ID
        session_id: String,
        /// Resume prompt
        #[arg(long)]
        prompt: Option<String>,
    },
    /// List recent coding sessions
    List,
}

pub async fn execute(command: HackCommand) -> Result<(), GhostError> {
    let config = config::load()?;
    crate::config_workspace::bootstrap_workspace(&config)?;
    let db = crate::db::connect(&config.workspace, config.embeddings.dimension).await?;

    match command {
        HackCommand::Start { dir, prompt } => {
            let working_dir = config.workspace.join(&dir);
            if !working_dir.is_dir() {
                return Err(GhostError::Generic(format!(
                    "Directory not found: {}",
                    working_dir.display()
                )));
            }

            let session = coding::session::start(
                &db, &config, working_dir, None, prompt,
            )
            .await?;

            println!("coding_session_id={}", session.id);
            println!("session_id={}", session.session_id);
            println!("working_dir={}", session.working_dir.display());
        }
        HackCommand::Resume { session_id, prompt } => {
            // Look up coding session, verify it exists
            // Add resume prompt as user message if provided
            // Re-activate the coding session
            todo!("implement resume")
        }
        HackCommand::List => {
            let sessions =
                crate::db::coding_sessions::list_recent_coding_sessions(&db, 10).await?;

            if sessions.is_empty() {
                println!("No coding sessions found.");
                return Ok(());
            }

            for (id, _session_id, working_dir, status, started_at) in &sessions {
                let marker = if status == "active" { "*" } else { " " };
                println!("{marker} {id}  {working_dir}  ({status}, {started_at})");
            }
        }
    }

    Ok(())
}
```

**Step 2: Add to cli/mod.rs**

Add `pub mod hack;`.

**Step 3: Wire into main.rs**

Add to the `Commands` enum:

```rust
Hack {
    #[command(subcommand)]
    command: ghost::cli::hack::HackCommand,
},
```

Add to `dispatch()`:

```rust
Commands::Hack { command } => ghost::cli::hack::execute(command).await,
```

**Step 4: Test CLI parses**

Run: `cargo run -- hack list` Expected: "No coding sessions found." (or similar, DB is
empty)

Run: `cargo run -- hack start nonexistent` Expected: Error about directory not found.

**Step 5: Run tests**

Run: `just ci` Expected: All pass.

**Step 6: Commit**

```bash
git add src/cli/hack.rs src/cli/mod.rs src/main.rs
git commit -m "feat: add ghost hack CLI subcommand (start/resume/list)"
```

---

## Phase 5: Discord Channel Takeover

### Task 9: Add takeover check to Discord handler

**Files:**

- Modify: `src/interfaces/discord/bot.rs`

**Step 1: Add takeover check in handle_message**

In `handle_message()`, after the `/reboot` check and before normal message processing,
add a `/kill` command handler and a takeover routing check:

````rust
// Handle /kill command — end coding session takeover
if content.eq_ignore_ascii_case("/kill") {
    let channel_str = msg.channel_id.to_string();
    match db::coding_sessions::get_active_takeover(&self.db, &channel_str).await {
        Ok(Some((coding_id, session_id, working_dir))) => {
            let summary = coding::session::end(
                &self.db,
                &coding_id,
                &std::path::Path::new(&working_dir),
            )
            .await
            .unwrap_or_else(|e| format!("(summary failed: {e})"));

            // Inject summary into GHOST's main session
            let ghost_session = self.resolve_session(msg.channel_id).await;
            if let Ok(ghost_sid) = ghost_session {
                let summary_msg = format!(
                    "[coding session ended]\n\n{summary}"
                );
                let _ = db::sessions::create_message(
                    &self.db,
                    &ghost_sid,
                    "system",
                    &summary_msg,
                    None,
                    None,
                )
                .await;
            }

            let _ = send_gateway_v2(
                &ctx.http,
                msg.channel_id,
                &format!("GHOST HACKED -- session ended.\n\n```\n{summary}\n```"),
                Some(CODING_EMBED_COLOR),
            )
            .await;
            return;
        }
        Ok(None) => {
            // No active takeover, /kill is a no-op
            let _ = send_gateway_v2(
                &ctx.http,
                msg.channel_id,
                "No active coding session.",
                Some(WARNING_EMBED_COLOR),
            )
            .await;
            return;
        }
        Err(e) => {
            error!("Failed to check takeover: {e}");
            return;
        }
    }
}

// Check for active coding session takeover
let channel_str = msg.channel_id.to_string();
if let Ok(Some((coding_id, session_id, working_dir))) =
    db::coding_sessions::get_active_takeover(&self.db, &channel_str).await
{
    // Route to coding session instead of GHOST
    self.handle_coding_message(ctx, msg, &session_id, &working_dir).await;
    return;
}
````

**Step 2: Add handle_coding_message method**

```rust
async fn handle_coding_message(
    &self,
    ctx: Context,
    msg: Message,
    session_id: &str,
    working_dir: &str,
) {
    let content = self.strip_bot_mention(&msg.content);

    // Build coding-agent-specific SessionChat
    let coding_prompt = coding::prompt::build_coding_prompt(
        &self.config,
        std::path::Path::new(working_dir),
    );

    // Use the existing session_chat but with:
    // - the coding session's session_id
    // - the coding agent's system prompt
    // - working_dir as cwd for tools

    // This needs a way to override session_chat's prompt and cwd.
    // Implementation detail: may need SessionChat::chat_with_overrides()
    // or a CodingSessionChat wrapper. Decide during implementation.

    // For now, the key insight: the tool loop, tool execution, and
    // message persistence all work the same. Only the system prompt,
    // session_id, and tool cwd change.
}
```

**Step 3: Add CODING_EMBED_COLOR constant**

Add to the constants in `bot.rs` (or wherever embed colors are defined):

```rust
const CODING_EMBED_COLOR: u32 = 0x29FFD9; // Teal — coding session
```

**Step 4: Run tests**

Run: `just ci` Expected: Compiles. Full integration testing happens in Phase 6.

**Step 5: Commit**

```bash
git add src/interfaces/discord/bot.rs
git commit -m "feat: add coding session takeover to Discord handler"
```

### Task 10: Wire SessionChat for coding sessions

**Files:**

- Modify: `src/chat/session.rs` (or `src/coding/session.rs`)

The coding agent needs `SessionChat` to use a custom system prompt and cwd. The current
`SessionChat::chat()` uses `self.prompt_renderer.render_system_prompt()`.

**Step 1: Add a method to chat with a custom prompt and cwd**

Add to `SessionChat`:

```rust
/// Chat in a coding session with a custom system prompt and working directory.
pub async fn chat_coding(
    &self,
    session_id: &str,
    user_message: &str,
    system_prompt: String,
    working_dir: PathBuf,
    event_tx: Option<&EventSender>,
) -> Result<(ChatResult, RunMetadata), ChatError> {
    // Same as chat() but:
    // 1. Uses provided system_prompt instead of render_system_prompt()
    // 2. Sets ToolContext.cwd to working_dir
    // 3. Uses coding model if configured (config.coding.model)
    // ...
}
```

The exact implementation depends on how deeply `chat()` couples to the prompt renderer.
If it's a single call to `system_prompt()`, this is a simple override. If more complex,
may need to refactor the `ChatHandler` to accept an optional prompt override.

**Step 2: Run tests**

Run: `just ci` Expected: Compiles.

**Step 3: Commit**

```bash
git add src/chat/session.rs
git commit -m "feat: add chat_coding method for custom prompt and cwd"
```

---

## Phase 6: GHOST-Side Skill

### Task 11: Write the `coding` skill for the GHOST

**Files:**

- Create: `prompts/skills/coding.md`
- Modify: `src/skills.rs` — add to `DEFAULT_SKILLS`

**Step 1: Write the skill**

This skill teaches the GHOST how to recognize coding requests, manage repos, and trigger
the coding agent. It should cover:

- Recognizing coding intent ("fix", "implement", "hack on", "build", "code")
- Checking for existing `repo.md` notes in the project
- Setting up `code/$slug/` (clone or pull)
- Calling `ghost hack start <dir> --prompt "..."` via `run_shell_command`
- Explaining the takeover to the OPERATOR
- Creating `repo.md` notes in project for future reference
- After `/kill`, interpreting the deterministic summary

**Step 2: Register in DEFAULT_SKILLS**

Add `("coding", include_str!("../prompts/skills/coding.md"))` to the array. Update the
test assertion count.

**Step 3: Run tests**

Run: `just ci` Expected: All pass.

**Step 4: Commit**

```bash
git add prompts/skills/coding.md src/skills.rs
git commit -m "feat: add coding skill for GHOST-side session management"
```

---

## Phase 7: Integration + Polish

### Task 12: Add `ghost hack start` channel takeover registration

**Files:**

- Modify: `src/cli/hack.rs`

The `ghost hack start` command needs to register the channel takeover so Discord routes
messages to the coding session. But the CLI doesn't know the channel_id — the GHOST
does.

**Step 1: Accept channel_id as a CLI flag**

```rust
Start {
    dir: String,
    #[arg(long)]
    prompt: Option<String>,
    /// Discord channel ID for takeover (passed by GHOST)
    #[arg(long)]
    channel_id: Option<String>,
},
```

The GHOST's `coding` skill instructs it to pass `--channel-id` when starting. The skill
knows the channel because it's chatting on it.

But wait — the GHOST uses `run_shell_command` which doesn't have access to the
channel_id directly. The channel_id would need to be available in the tool context or
system info.

**Alternative approach:** The GHOST's shell command starts the session without a
channel_id. The daemon (which knows the channel) then registers the takeover after
seeing the session created. OR: the `ghost hack start` command outputs the session ID,
and the GHOST's next action is intercepted by the daemon which registers the takeover.

**Simplest approach:** Add the channel_id to the system prompt or tool context info so
the GHOST can pass it. The system prompt already has `{{ system_info }}` — add the
current channel_id there. Then the GHOST passes it to
`ghost hack start --channel-id <id>`.

This is an implementation detail that will need some investigation. The key requirement:
somehow the channel_id reaches `ghost hack start` so it can register the takeover.

**Step 2: Commit whatever approach works**

```bash
git add src/cli/hack.rs
git commit -m "feat: wire channel takeover registration in ghost hack start"
```

### Task 13: Implement `ghost hack resume`

**Files:**

- Modify: `src/cli/hack.rs`
- Modify: `src/db/coding_sessions.rs` (add `get_coding_session` query)

**Step 1: Add get_coding_session query**

```rust
pub async fn get_coding_session(
    db: &SqlitePool,
    id: &str,
) -> Result<Option<(String, String, String)>, DatabaseError> {
    // Returns (session_id, working_dir, status)
    let row = sqlx::query_as::<_, (String, String, String)>(
        "SELECT session_id, working_dir, status FROM coding_sessions WHERE id = ?",
    )
    .bind(id)
    .fetch_optional(db)
    .await?;
    Ok(row)
}

pub async fn reactivate_coding_session(
    db: &SqlitePool,
    id: &str,
    channel_id: Option<&str>,
) -> Result<(), DatabaseError> {
    sqlx::query(
        "UPDATE coding_sessions SET status = 'active', channel_id = ?, ended_at = NULL WHERE id = ?",
    )
    .bind(channel_id)
    .bind(id)
    .execute(db)
    .await?;
    Ok(())
}
```

**Step 2: Implement Resume in hack.rs**

```rust
HackCommand::Resume { session_id, prompt } => {
    let (chat_session_id, working_dir, status) =
        db::coding_sessions::get_coding_session(&db, &session_id)
            .await?
            .ok_or_else(|| GhostError::Generic(
                format!("Coding session not found: {session_id}")
            ))?;

    if status == "active" {
        return Err(GhostError::Generic("Session is already active".into()));
    }

    db::coding_sessions::reactivate_coding_session(&db, &session_id, None)
        .await?;

    if let Some(prompt) = prompt {
        db::sessions::create_message(
            &db, &chat_session_id, "user", &prompt, None, None,
        )
        .await?;
    }

    println!("coding_session_id={session_id}");
    println!("session_id={chat_session_id}");
    println!("working_dir={working_dir}");
}
```

**Step 3: Run tests**

Run: `just ci`

**Step 4: Commit**

```bash
git add src/cli/hack.rs src/db/coding_sessions.rs
git commit -m "feat: implement ghost hack resume"
```

### Task 14: Add coding session entry/exit messages

**Files:**

- Modify: `src/interfaces/discord/bot.rs`

**Step 1: Send entry message when takeover starts**

When the GHOST runs `ghost hack start` and the output is parsed, or when the first
message hits the coding session, send the entry banner:

```
GHOST HACKED -- you're now talking to the coding agent. /kill to exit.
```

Using Teal embed color.

**Step 2: Verify /kill sends exit message with summary**

Already implemented in Task 9, verify it works end-to-end.

**Step 3: Commit**

```bash
git add src/interfaces/discord/bot.rs
git commit -m "feat: add coding session entry/exit Discord messages"
```

### Task 15: Add compaction config for coding sessions

**Files:**

- Modify: `src/coding/session.rs`

**Step 1: Set compaction overrides**

When creating the `SessionChat` for coding sessions, configure compaction:

```rust
let compaction = CompactionConfig {
    // Use reasonable defaults for long coding sessions
    keep_window: 12,
    instructions: Some(
        "Preserve: current plan/TODO status, files modified and why, test results, \
         OPERATOR decisions and preferences. Drop: verbose file contents already \
         committed, raw shell output from successful commands, intermediate diffs."
            .to_string(),
    ),
    ..config.compaction.clone()
};
```

**Step 2: Run tests**

Run: `just ci`

**Step 3: Commit**

```bash
git add src/coding/session.rs
git commit -m "feat: add coding-specific compaction config"
```

---

## Phase 8: Testing

### Task 16: Manual integration test

**No code changes — manual verification.**

1. Start the daemon: `ghost daemon`
2. In Discord, ask the GHOST to hack on a test repo
3. Verify: GHOST clones/finds repo, runs `ghost hack start`, takeover message appears
4. Chat with the coding agent: ask it to read files, run commands, edit code
5. Verify: coding agent reads AGENTS.md, uses skills, asks questions
6. Send `/kill`
7. Verify: deterministic summary appears, GHOST resumes
8. Ask GHOST to resume: verify `ghost hack resume` works

### Task 17: Unit tests for coding module

**Files:**

- Add tests in: `src/coding/prompt.rs`
- Add tests in: `src/coding/session.rs`
- Add tests in: `src/db/coding_sessions.rs`

**Step 1: Test prompt building**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn build_coding_prompt_includes_working_dir() {
        let config = crate::config::test_config();
        let dir = TempDir::new().unwrap();
        let prompt = build_coding_prompt(&config, dir.path());
        assert!(prompt.contains(&dir.path().display().to_string()));
    }

    #[test]
    fn load_repo_context_reads_agents_md() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("AGENTS.md"), "# Test\nBe nice.").unwrap();
        let ctx = load_repo_context(dir.path());
        assert!(ctx.contains("Be nice."));
        assert!(ctx.contains("AGENTS.md"));
    }

    #[test]
    fn load_repo_context_prefers_agents_over_claude() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("AGENTS.md"), "agents").unwrap();
        std::fs::write(dir.path().join("CLAUDE.md"), "claude").unwrap();
        let ctx = load_repo_context(dir.path());
        assert!(ctx.contains("agents"));
        assert!(!ctx.contains("claude"));
    }

    #[test]
    fn load_repo_context_empty_when_no_file() {
        let dir = TempDir::new().unwrap();
        let ctx = load_repo_context(dir.path());
        assert!(ctx.is_empty());
    }
}
```

**Step 2: Test DB queries**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::test_db;

    #[tokio::test]
    async fn create_and_get_coding_session() {
        let db = test_db().await;
        let session_id = crate::db::sessions::create_session(&db).await.unwrap();
        create_coding_session(&db, "cs1", &session_id, Some("chan1"), "/tmp/repo")
            .await
            .unwrap();

        let takeover = get_active_takeover(&db, "chan1").await.unwrap();
        assert!(takeover.is_some());
        let (id, sid, dir) = takeover.unwrap();
        assert_eq!(id, "cs1");
        assert_eq!(sid, session_id);
        assert_eq!(dir, "/tmp/repo");
    }

    #[tokio::test]
    async fn end_session_clears_takeover() {
        let db = test_db().await;
        let session_id = crate::db::sessions::create_session(&db).await.unwrap();
        create_coding_session(&db, "cs2", &session_id, Some("chan2"), "/tmp/repo")
            .await
            .unwrap();

        end_coding_session(&db, "cs2").await.unwrap();

        let takeover = get_active_takeover(&db, "chan2").await.unwrap();
        assert!(takeover.is_none());
    }
}
```

**Step 3: Run all tests**

Run: `just ci` Expected: All pass.

**Step 4: Commit**

```bash
git add src/coding/ src/db/coding_sessions.rs
git commit -m "test: add unit tests for coding module"
```

---

## Implementation Notes

### Things to figure out during implementation

1. **Channel ID plumbing**: How does the GHOST pass the Discord channel_id to
   `ghost hack start`? Options: add to system prompt `{{ system_info }}`, add to tool
   context, or have the daemon detect new coding sessions and register takeover
   automatically.

2. **SessionChat override**: `chat_coding()` needs to override the system prompt and
   cwd. Check how tightly `ChatHandler` couples to `PromptRenderer` — may need a small
   refactor to accept prompt overrides.

3. **Summary accuracy**: The deterministic summary uses `HEAD~20` which is approximate.
   Consider storing `base_commit` in `coding_sessions` at start time for precise diffs.

4. **Coding model resolution**: When `config.coding.model` is set, the coding
   `SessionChat` should use that model. Check how model selection works in the provider
   layer — it may already support per-request model overrides.

5. **ToolContext.cwd**: Verify that tools respect `cwd` for file operations and shell
   commands. The coding agent's `cwd` should be the repo's `working_dir`, not the
   workspace root.

### Order of operations

Phases 1-4 can be implemented independently and tested without Discord. Phase 5 requires
a running daemon + Discord. Phase 6 requires the GHOST to understand the `coding` skill.
Phase 7 ties everything together. Phase 8 is manual verification.

Within phases, tasks are sequential (each builds on the previous).
