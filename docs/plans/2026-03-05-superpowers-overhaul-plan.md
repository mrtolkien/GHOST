# Superpowers Skills Overhaul Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this
> plan task-by-task.

**Goal:** Properly separate superpowers skills from GHOST-native skills, fully port all
companion files, add an `available` frontmatter field for skill visibility filtering,
and convert upstream agent prompts into proper Lua agents.

**Architecture:** Skills move to a nested directory structure
(`prompts/skills/superpowers/<name>/`) with recursive discovery. A new `available`
frontmatter field controls which skills are visible to the GHOST (chat) vs the coding
agent. Agent prompts become Lua agents under `prompts/agents/` with cwd propagation from
the coding agent.

**Tech Stack:** Rust (same crate), Lua (agent definitions), Python (sync script update).

**Design doc:** `docs/plans/2026-03-05-superpowers-overhaul-design.md` (this plan
supersedes the skills portion of `docs/plans/2026-03-05-coding-agent-plan.md`)

---

## Phase 1: Skill Infrastructure Changes

### Task 1: Add `available` field to skill frontmatter parsing

**Files:**

- Modify: `src/skills.rs`

**Step 1: Update the `Skill` struct**

Add `pub available: Option<String>` to the `Skill` struct:

```rust
#[derive(Debug)]
pub struct Skill {
    pub name: String,
    pub description: String,
    pub path: PathBuf,
    pub available: Option<String>,
}
```

**Step 2: Update `parse_frontmatter` signature and logic**

Change return type from `Option<(String, String)>` to
`Option<(String, String, Option<String>)>`.

Add parsing for `available:` in the same style as `name:` — a simple single-line string
field:

```rust
} else if let Some(value) = trimmed.strip_prefix("available:") {
    available = Some(value.trim().to_string());
    in_description = false;
}
```

Return `Some((name, description, available))`.

**Step 3: Update all callers of `parse_frontmatter`**

In `discover_skills`:

```rust
match parse_frontmatter(&content) {
    Some((name, description, available)) => {
        skills.push(Skill {
            name,
            description,
            path: skill_path,
            available,
        });
    }
    // ...
}
```

In `src/coding/prompt.rs` `discover_repo_skills`:

```rust
if let Some((name, description, available)) = skills::parse_frontmatter(&content) {
    found.push(skills::Skill {
        name,
        description,
        path: skill_path,
        available,
    });
}
```

**Step 4: Add tests for the `available` field**

```rust
#[test]
fn parse_frontmatter_extracts_available() {
    let content = "\
---
name: test-skill
description: A test skill.
available: coding
---
";
    let (name, desc, available) = parse_frontmatter(content).unwrap();
    assert_eq!(name, "test-skill");
    assert_eq!(desc, "A test skill.");
    assert_eq!(available, Some("coding".to_string()));
}

#[test]
fn parse_frontmatter_defaults_available_to_none() {
    let content = "\
---
name: test-skill
description: A test skill.
---
";
    let (_, _, available) = parse_frontmatter(content).unwrap();
    assert!(available.is_none());
}
```

**Step 5: Run tests**

Run: `just ci` Expected: All tests pass. Some tests need updating for the new tuple
arity.

**Step 6: Commit**

```bash
git add src/skills.rs src/coding/prompt.rs
git commit -m "feat: add 'available' field to skill frontmatter parsing"
```

---

### Task 2: Filter skills by availability in prompt builders

**Files:**

- Modify: `src/prompt/context.rs`
- Modify: `src/coding/prompt.rs`

**Step 1: Filter in `build_ghost_skills`**

In `src/prompt/context.rs`, after `discover_skills`, filter out coding-only skills:

```rust
pub fn build_ghost_skills(workspace: &Path) -> String {
    let skills = crate::skills::discover_skills(workspace);
    let skills: Vec<_> = skills
        .into_iter()
        .filter(|s| s.available.as_deref() != Some("coding"))
        .collect();
    // ... rest unchanged
}
```

**Step 2: No filter in `build_coding_skills`**

The coding agent sees all skills (both `available: coding` and default/unset). No
changes needed to the coding prompt builder — it already returns all skills.

**Step 3: Update test in `context.rs`**

Add a test that verifies `build_ghost_skills` excludes `available: coding` skills:

```rust
#[test]
fn build_ghost_skills_excludes_coding_only() {
    let dir = TempDir::new().unwrap();
    let skills = dir.path().join("skills");

    // Ghost skill (no available field)
    let ghost = skills.join("ghost-skill");
    fs::create_dir_all(&ghost).unwrap();
    fs::write(
        ghost.join("skill.md"),
        "---\nname: ghost-skill\ndescription: Ghost only.\n---\n",
    )
    .unwrap();

    // Coding-only skill
    let coding = skills.join("coding-skill");
    fs::create_dir_all(&coding).unwrap();
    fs::write(
        coding.join("skill.md"),
        "---\nname: coding-skill\ndescription: Coding only.\navailable: coding\n---\n",
    )
    .unwrap();

    let result = build_ghost_skills(dir.path());
    assert!(result.contains("ghost-skill"));
    assert!(!result.contains("coding-skill"));
}
```

**Step 4: Run tests**

Run: `just ci` Expected: All pass.

**Step 5: Commit**

```bash
git add src/prompt/context.rs src/coding/prompt.rs
git commit -m "feat: filter skills by 'available' field in prompt builders"
```

---

### Task 3: Make `discover_skills` recursive

**Files:**

- Modify: `src/skills.rs`

**Step 1: Rewrite `discover_skills` to walk recursively**

Replace the single-level `read_dir` scan with a recursive walk. Any directory containing
`skill.md` is a skill, regardless of depth. Directories starting with `.` are skipped at
every level.

```rust
pub fn discover_skills(workspace: &Path) -> Vec<Skill> {
    let skills_dir = workspace.join("skills");
    let mut skills = Vec::new();
    walk_skills_dir(&skills_dir, &mut skills);
    skills.sort_by(|a, b| a.name.cmp(&b.name));
    skills
}

fn walk_skills_dir(dir: &Path, skills: &mut Vec<Skill>) {
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };

    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with('.') {
            continue;
        }

        let path = entry.path();
        if !path.is_dir() {
            continue;
        }

        let skill_file = path.join("skill.md");
        if skill_file.exists() {
            // This directory is a skill
            if let Ok(content) = std::fs::read_to_string(&skill_file) {
                match parse_frontmatter(&content) {
                    Some((name, description, available)) => {
                        skills.push(Skill {
                            name,
                            description,
                            path: skill_file,
                            available,
                        });
                    }
                    None => {
                        logfire::warn!(
                            "Malformed skill frontmatter in {path}",
                            path = skill_file.display().to_string(),
                        );
                    }
                }
            }
        } else {
            // Not a skill dir — recurse into it (namespace directory)
            walk_skills_dir(&path, skills);
        }
    }
}
```

Key behavior: if a directory has `skill.md`, it's a leaf (skill). If it doesn't, it's a
namespace directory — recurse into it. This means `skills/superpowers/brainstorming/` is
a skill (has `skill.md`), and `skills/superpowers/` is a namespace (no `skill.md`,
contains skill subdirs).

**Step 2: Update tests**

Add a test for nested discovery:

```rust
#[test]
fn discover_skills_finds_nested() {
    let dir = TempDir::new().unwrap();
    let skills = dir.path().join("skills");

    // Top-level skill
    let top = skills.join("top-skill");
    fs::create_dir_all(&top).unwrap();
    fs::write(
        top.join("skill.md"),
        "---\nname: top-skill\ndescription: Top.\n---\n",
    )
    .unwrap();

    // Nested skill under a namespace
    let nested = skills.join("superpowers").join("nested-skill");
    fs::create_dir_all(&nested).unwrap();
    fs::write(
        nested.join("skill.md"),
        "---\nname: nested-skill\ndescription: Nested.\n---\n",
    )
    .unwrap();

    let found = discover_skills(dir.path());
    assert_eq!(found.len(), 2);
    assert_eq!(found[0].name, "nested-skill");
    assert_eq!(found[1].name, "top-skill");
}
```

**Step 3: Run tests**

Run: `just ci` Expected: All pass. Existing tests should still work (flat skills are a
subset of recursive scanning).

**Step 4: Commit**

```bash
git add src/skills.rs
git commit -m "feat: make discover_skills recursive for nested skill directories"
```

---

### Task 4: Change `DEFAULT_SKILLS` to support multi-file skills

**Files:**

- Modify: `src/skills.rs`

**Step 1: Change the DEFAULT_SKILLS data structure**

Replace `&[(&str, &str)]` with a struct that supports multiple files per skill:

```rust
struct DefaultSkill {
    /// Relative path under `$WORKSPACE/skills/`, e.g. "agent-creator" or
    /// "superpowers/brainstorming".
    path: &'static str,
    /// Files to install. First entry is always ("skill.md", <content>).
    files: &'static [(&'static str, &'static str)],
}

const DEFAULT_SKILLS: &[DefaultSkill] = &[
    DefaultSkill {
        path: "agent-creator",
        files: &[("skill.md", include_str!("../prompts/skills/agent-creator.md"))],
    },
    // ... all other skills
];
```

For GHOST-native skills that are currently flat `.md` files (e.g.
`prompts/skills/agent-creator.md`), they stay as single-file entries. No need to move
them to directories in `prompts/skills/` — the `include_str!` path just points to the
flat file, and `install_default_skills` writes it as `agent-creator/skill.md`.

**Step 2: Update `install_default_skills`**

```rust
pub fn install_default_skills(workspace: &Path) -> Result<(), std::io::Error> {
    let skills_dir = workspace.join("skills");

    for skill in DEFAULT_SKILLS {
        let skill_dir = skills_dir.join(skill.path);
        fs::create_dir_all(&skill_dir)?;
        for (filename, content) in skill.files {
            fs::write(skill_dir.join(filename), content)?;
        }
    }

    Ok(())
}
```

**Step 3: Update test assertion**

The `install_default_skills_creates_files` test checks `DEFAULT_SKILLS.len()`. Update
the count to match the new array length (still 21 at this point — structure changed but
skill count hasn't).

**Step 4: Run tests**

Run: `just ci` Expected: All pass.

**Step 5: Commit**

```bash
git add src/skills.rs
git commit -m "refactor: change DEFAULT_SKILLS to support multi-file skill directories"
```

---

## Phase 2: Move Superpowers Skills to Nested Directory

### Task 5: Create superpowers skill directory structure

**Files:**

- Create: `prompts/skills/superpowers/brainstorming/skill.md`
- Create: `prompts/skills/superpowers/executing-plans/skill.md`
- Create: `prompts/skills/superpowers/finishing-branch/skill.md`
- Create: `prompts/skills/superpowers/git-worktrees/skill.md`
- Create: `prompts/skills/superpowers/parallel-agents/skill.md`
- Create: `prompts/skills/superpowers/receiving-review/skill.md`
- Create: `prompts/skills/superpowers/requesting-review/skill.md`
- Create: `prompts/skills/superpowers/subagent-development/skill.md`
- Create: `prompts/skills/superpowers/systematic-debugging/skill.md`
- Create: `prompts/skills/superpowers/tdd/skill.md`
- Create: `prompts/skills/superpowers/verification/skill.md`
- Create: `prompts/skills/superpowers/writing-plans/skill.md`
- Create: `prompts/skills/superpowers/writing-skills/skill.md`
- Delete: `prompts/skills/brainstorming.md` (and all 12 other flat superpowers files)
- Modify: `src/skills.rs` — update `include_str!` paths and `path` fields

**Step 1: Create the directory structure**

For each of the 13 superpowers skills, create
`prompts/skills/superpowers/<name>/skill.md` with the content from the existing flat
file. The skill.md content stays the same EXCEPT:

- Add `available: coding` to the frontmatter of every superpowers skill
- Update any `read_file` references from `skills/<name>/` to
  `skills/superpowers/<name>/` (check each file for internal references)

Example for brainstorming — add `available: coding` after the description:

```yaml
---
name: brainstorming
description: You MUST use this before any creative work...
available: coding
---
```

**Step 2: Delete the old flat files**

Remove all 13 flat `.md` files from `prompts/skills/` that were superpowers skills:
`brainstorming.md`, `executing-plans.md`, `finishing-branch.md`, `git-worktrees.md`,
`parallel-agents.md`, `receiving-review.md`, `requesting-review.md`,
`subagent-development.md`, `systematic-debugging.md`, `tdd.md`, `verification.md`,
`writing-plans.md`, `writing-skills.md`.

**Step 3: Update `DEFAULT_SKILLS` entries**

Change paths and `include_str!` paths for all 13 superpowers skills:

```rust
DefaultSkill {
    path: "superpowers/brainstorming",
    files: &[(
        "skill.md",
        include_str!("../prompts/skills/superpowers/brainstorming/skill.md"),
    )],
},
```

**Step 4: Update the test assertion count**

Still 21 skills total — only paths changed.

**Step 5: Run tests**

Run: `just ci` Expected: All pass. `discover_skills` finds the nested skills via
recursive walk.

**Step 6: Commit**

```bash
git add prompts/skills/superpowers/ src/skills.rs
git rm prompts/skills/brainstorming.md prompts/skills/executing-plans.md \
  prompts/skills/finishing-branch.md prompts/skills/git-worktrees.md \
  prompts/skills/parallel-agents.md prompts/skills/receiving-review.md \
  prompts/skills/requesting-review.md prompts/skills/subagent-development.md \
  prompts/skills/systematic-debugging.md prompts/skills/tdd.md \
  prompts/skills/verification.md prompts/skills/writing-plans.md \
  prompts/skills/writing-skills.md
git commit -m "refactor: move superpowers skills to nested directory with available: coding"
```

---

## Phase 3: Fully Port Companion Files

### Task 6: Update sync script to vendor all files

**Files:**

- Modify: `scripts/sync-superpowers.py`

**Step 1: Change `collect_skills` to collect ALL files per skill**

The current `collect_skills` only reads `SKILL.md`. Change it to collect all files:

```python
def collect_skills(repo: Path) -> dict[str, dict[str, str]]:
    """Returns {skill_name: {filename: content}}"""
    skills_dir = repo / "skills"
    result = {}
    if not skills_dir.exists():
        return result
    for skill_dir in sorted(skills_dir.iterdir()):
        if not skill_dir.is_dir():
            continue
        files = {}
        for file_path in sorted(skill_dir.rglob("*")):
            if file_path.is_file():
                rel = file_path.relative_to(skill_dir)
                try:
                    files[str(rel)] = file_path.read_text()
                except UnicodeDecodeError:
                    # Skip binary files
                    continue
        if files:
            result[skill_dir.name] = files
    return result
```

**Step 2: Update `load_vendored`, `show_diff`, and `apply` to handle multi-file skills**

`load_vendored` should return the same `dict[str, dict[str, str]]` structure.

`show_diff` should diff each file within each skill.

`apply` should write all files per skill directory.

**Step 3: Run the updated sync**

Run: `uv run scripts/sync-superpowers.py --apply` Expected: Vendors all files
(SKILL.md + companions) into `vendor/superpowers/`.

**Step 4: Verify**

Run: `ls vendor/superpowers/subagent-driven-development/` Expected:
`SKILL.md implementer-prompt.md spec-reviewer-prompt.md code-quality-reviewer-prompt.md`

**Step 5: Commit**

```bash
git add scripts/sync-superpowers.py vendor/superpowers/
git commit -m "chore: update sync script to vendor all skill files, re-vendor"
```

---

### Task 7: Port companion files for subagent-driven-development

**Files:**

- Create: `prompts/skills/superpowers/subagent-development/implementer-prompt.md`
- Create: `prompts/skills/superpowers/subagent-development/spec-reviewer-prompt.md`
- Create:
  `prompts/skills/superpowers/subagent-development/code-quality-reviewer-prompt.md`
- Modify: `prompts/skills/superpowers/subagent-development/skill.md`
- Modify: `src/skills.rs` — add files to the DefaultSkill entry

**Step 1: Port each companion file**

Read each vendored file from `vendor/superpowers/subagent-driven-development/` and port
with the standard adaptation rules:

- `Agent tool` / `Task tool` → `agent_control(action: "start", ...)`
- `TodoWrite` → `todo(action: "plan", ...)`
- `your human partner` → `the OPERATOR`
- Remove Claude Code-specific references
- Keep all workflow content, criteria, checklists

These files are agent prompt templates. In Phase 4 they become the basis for Lua agents,
but for now port them as readable companion files alongside the skill.

**Step 2: Update skill.md references**

In `subagent-development/skill.md`, update references to point to companion files:

```
Read the implementer prompt template:
read_file("skills/superpowers/subagent-development/implementer-prompt.md")
```

**Step 3: Add files to DEFAULT_SKILLS entry**

```rust
DefaultSkill {
    path: "superpowers/subagent-development",
    files: &[
        ("skill.md", include_str!("../prompts/skills/superpowers/subagent-development/skill.md")),
        ("implementer-prompt.md", include_str!("../prompts/skills/superpowers/subagent-development/implementer-prompt.md")),
        ("spec-reviewer-prompt.md", include_str!("../prompts/skills/superpowers/subagent-development/spec-reviewer-prompt.md")),
        ("code-quality-reviewer-prompt.md", include_str!("../prompts/skills/superpowers/subagent-development/code-quality-reviewer-prompt.md")),
    ],
},
```

**Step 4: Run tests**

Run: `just ci` Expected: All pass.

**Step 5: Commit**

```bash
git add prompts/skills/superpowers/subagent-development/ src/skills.rs
git commit -m "feat: port subagent-development companion files (implementer, reviewers)"
```

---

### Task 8: Port companion files for systematic-debugging

**Files:**

- Create: `prompts/skills/superpowers/systematic-debugging/root-cause-tracing.md`
- Create: `prompts/skills/superpowers/systematic-debugging/condition-based-waiting.md`
- Create: `prompts/skills/superpowers/systematic-debugging/defense-in-depth.md`
- Create: `prompts/skills/superpowers/systematic-debugging/find-polluter.sh`
- Modify: `prompts/skills/superpowers/systematic-debugging/skill.md`
- Modify: `src/skills.rs`

**Step 1: Port methodology guides**

Read vendored files and port with adaptation rules. These are methodology guides — the
content is mostly tool-agnostic, so minimal adaptation needed. Update any Claude
Code-specific examples.

**Step 2: Decide on test scenario files**

The upstream has `test-academic.md`, `test-pressure-1.md`, `test-pressure-2.md`,
`test-pressure-3.md`, and `condition-based-waiting-example.ts`. These are test/example
files used to verify the skill works. **Skip these** — they're testing infrastructure
for the skill author, not useful at runtime. The methodology guides are the valuable
content.

**Step 3: Update skill.md references**

Update `systematic-debugging/skill.md` to reference companion files:

```
For root cause analysis methodology:
read_file("skills/superpowers/systematic-debugging/root-cause-tracing.md")
```

**Step 4: Add files to DEFAULT_SKILLS entry**

```rust
DefaultSkill {
    path: "superpowers/systematic-debugging",
    files: &[
        ("skill.md", include_str!("...")),
        ("root-cause-tracing.md", include_str!("...")),
        ("condition-based-waiting.md", include_str!("...")),
        ("defense-in-depth.md", include_str!("...")),
        ("find-polluter.sh", include_str!("...")),
    ],
},
```

**Step 5: Run tests**

Run: `just ci` Expected: All pass.

**Step 6: Commit**

```bash
git add prompts/skills/superpowers/systematic-debugging/ src/skills.rs
git commit -m "feat: port systematic-debugging companion files"
```

---

### Task 9: Port companion files for remaining skills

**Files:**

- Create: `prompts/skills/superpowers/writing-skills/anthropic-best-practices.md`
- Create: `prompts/skills/superpowers/writing-skills/persuasion-principles.md`
- Create: `prompts/skills/superpowers/writing-skills/testing-skills-with-subagents.md`
- Create: `prompts/skills/superpowers/requesting-review/code-reviewer.md`
- Create: `prompts/skills/superpowers/tdd/testing-anti-patterns.md`
- Modify: skill.md for each of the above
- Modify: `src/skills.rs`

**Step 1: Port writing-skills companions**

Port `anthropic-best-practices.md`, `persuasion-principles.md`,
`testing-skills-with-subagents.md` from vendor. Skip `graphviz-conventions.dot`,
`render-graphs.js`, and the `examples/` directory — these are authoring tools, not
runtime content.

**Step 2: Port requesting-review/code-reviewer.md**

This is a detailed code review framework. Port with adaptation rules.

**Step 3: Port tdd/testing-anti-patterns.md**

A methodology guide — minimal adaptation needed.

**Step 4: Update skill.md references and DEFAULT_SKILLS entries for each**

Same pattern as Tasks 7 and 8.

**Step 5: Run tests**

Run: `just ci` Expected: All pass.

**Step 6: Commit**

```bash
git add prompts/skills/superpowers/writing-skills/ \
  prompts/skills/superpowers/requesting-review/ \
  prompts/skills/superpowers/tdd/ \
  src/skills.rs
git commit -m "feat: port companion files for writing-skills, requesting-review, tdd"
```

---

## Phase 4: Lua Agents for Subagent Workflows

### Task 10: Propagate cwd through agent_control to spawned agents

**Files:**

- Modify: `src/tools/agent_control.rs`
- Modify: `src/agents/runner.rs`

**Step 1: Add `cwd` parameter to `run_in_background`**

```rust
pub async fn run_in_background(
    &self,
    agent_name: &str,
    prompt: &str,
    parent_session_id: Option<&str>,
    cwd: Option<PathBuf>,  // NEW
) -> Result<String, AgentError> {
```

Thread the `cwd` through `BackgroundTask` and into `execute_agent` → `setup_agent`.

**Step 2: In `setup_agent`, apply cwd to `SessionChat`**

After creating `SessionChat` at line ~505 of `runner.rs`:

```rust
let mut session_chat = SessionChat::new(db.clone(), provider, tool_manager, config.clone())
    .with_max_tool_iterations(agent_config.max_iterations)
    .with_compaction_config(agent_compaction);

if let Some(cwd) = cwd {
    session_chat = session_chat.with_cwd_override(cwd);
}
```

**Step 3: Pass caller's cwd from agent_control**

In `agent_control.rs` `action_start`, pass the caller's cwd:

```rust
let cwd = if ctx.cwd != ctx.workspace {
    Some(ctx.cwd.clone())
} else {
    None
};

let agent_id = runner
    .run_in_background(agent_name, prompt, parent_session_id.as_deref(), cwd)
    .await
    .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;
```

This way, when the coding agent (whose cwd is the repo) spawns a sub-agent, the
sub-agent inherits the repo as its cwd. When the GHOST spawns an agent, cwd == workspace
so `None` is passed and behavior is unchanged.

**Step 4: Do the same for `resume_in_background`**

Add `cwd: Option<PathBuf>` and thread it through the resume path too.

**Step 5: Run tests**

Run: `just ci` Expected: All pass. Existing agent tests don't set cwd, so they get
`None` (unchanged behavior).

**Step 6: Commit**

```bash
git add src/tools/agent_control.rs src/agents/runner.rs
git commit -m "feat: propagate cwd from agent_control caller to spawned agent"
```

---

### Task 11: Create coding-implementer Lua agent

**Files:**

- Create: `prompts/agents/coding-implementer/agent.lua`
- Create: `prompts/agents/coding-implementer/prompt.md`

**Step 1: Write prompt.md**

Port the content from
`prompts/skills/superpowers/subagent-development/implementer-prompt.md` into a
Tera-style template. The prompt should include `{{ task_text }}` and `{{ context }}`
variables. Adapt tool references to Ghost tool names (`read_file`, `write_file`,
`file_edit`, `run_shell_command`, `todo`).

**Step 2: Write agent.lua**

```lua
local template = require("ghost.template")

return {
    name = "coding-implementer",
    description = "Implement a single task following TDD. "
        .. "Spawned by the subagent-development workflow.",

    max_iterations = 50,

    compaction = {
        keep_window = 10,
        instructions = "Preserve: current task text, files modified, test results, "
            .. "decisions made. Drop: verbose file contents, raw shell output from "
            .. "successful commands.",
    },

    tools = {
        "read_file",
        "write_file",
        "file_edit",
        "run_shell_command",
        "todo",
    },

    build = function(ctx, args)
        return {
            system_prompt = template.render(read_file("prompt.md"), {
                task_text = args.task_text or args.prompt or "No task specified.",
                context = args.context or "",
            }),
            messages = {
                { role = "user", content = args.task_text or args.prompt or "Begin." },
            },
        }
    end,
}
```

**Step 3: Verify agent loads**

Run: `cargo test -- agent` (to check that agent loading doesn't break)

**Step 4: Commit**

```bash
git add prompts/agents/coding-implementer/
git commit -m "feat: add coding-implementer Lua agent"
```

---

### Task 12: Create coding-spec-reviewer Lua agent

**Files:**

- Create: `prompts/agents/coding-spec-reviewer/agent.lua`
- Create: `prompts/agents/coding-spec-reviewer/prompt.md`

**Step 1: Write prompt.md**

Port from `spec-reviewer-prompt.md`. This agent checks that implementation matches spec
exactly — reports missing requirements and anything extra. Template variable:
`{{ task_text }}` (the spec to check against).

**Step 2: Write agent.lua**

```lua
local template = require("ghost.template")

return {
    name = "coding-spec-reviewer",
    description = "Review implementation against spec for compliance. "
        .. "Spawned by the subagent-development workflow.",

    max_iterations = 20,

    tools = {
        "read_file",
        "run_shell_command",
    },

    custom_tools = {
        submit_review = {
            description = "Submit your spec compliance review. This ends your session.",
            parameters = {
                type = "object",
                properties = {
                    compliant = {
                        type = "boolean",
                        description = "Whether the implementation matches the spec.",
                    },
                    issues = {
                        type = "string",
                        description = "Missing requirements, extra additions, or "
                            .. "deviations from spec. Empty if compliant.",
                    },
                },
                required = { "compliant", "issues" },
            },
            handler = function(ctx, args)
                local result = json.encode({
                    compliant = args.compliant,
                    issues = args.issues,
                })
                ctx:set("review_result", result)
                return result
            end,
            terminal = true,
        },
    },

    build = function(ctx, args)
        return {
            system_prompt = template.render(read_file("prompt.md"), {
                task_text = args.task_text or args.prompt or "No spec provided.",
            }),
            messages = {
                { role = "user", content = "Review the implementation against the spec." },
            },
        }
    end,
}
```

**Step 3: Commit**

```bash
git add prompts/agents/coding-spec-reviewer/
git commit -m "feat: add coding-spec-reviewer Lua agent"
```

---

### Task 13: Create coding-quality-reviewer Lua agent

**Files:**

- Create: `prompts/agents/coding-quality-reviewer/agent.lua`
- Create: `prompts/agents/coding-quality-reviewer/prompt.md`

**Step 1: Write prompt.md**

Port from `code-quality-reviewer-prompt.md`. Reviews code quality, style, test coverage,
naming — after spec compliance is confirmed.

**Step 2: Write agent.lua**

Same structure as spec-reviewer: read-only tools + `submit_review` terminal tool. The
review result includes a severity classification (e.g. `approved`, `minor_issues`,
`major_issues`).

```lua
custom_tools = {
    submit_review = {
        description = "Submit your code quality review. This ends your session.",
        parameters = {
            type = "object",
            properties = {
                approved = {
                    type = "boolean",
                    description = "Whether the code quality is acceptable.",
                },
                issues = {
                    type = "string",
                    description = "Quality issues found. Empty if approved.",
                },
            },
            required = { "approved", "issues" },
        },
        handler = function(ctx, args)
            local result = json.encode({
                approved = args.approved,
                issues = args.issues,
            })
            ctx:set("review_result", result)
            return result
        end,
        terminal = true,
    },
},
```

**Step 3: Commit**

```bash
git add prompts/agents/coding-quality-reviewer/
git commit -m "feat: add coding-quality-reviewer Lua agent"
```

---

### Task 14: Create coding-reviewer Lua agent

**Files:**

- Create: `prompts/agents/coding-reviewer/agent.lua`
- Create: `prompts/agents/coding-reviewer/prompt.md`

**Step 1: Write prompt.md**

Port from `requesting-review/code-reviewer.md`. This is the general-purpose code
reviewer used for final review after all tasks complete. Covers five dimensions:
correctness, security, performance, maintainability, test coverage.

**Step 2: Write agent.lua**

Same pattern as the other reviewers. Read-only tools + `submit_review` terminal tool.

**Step 3: Commit**

```bash
git add prompts/agents/coding-reviewer/
git commit -m "feat: add coding-reviewer Lua agent for final code review"
```

---

### Task 15: Update subagent-development skill to reference Lua agents

**Files:**

- Modify: `prompts/skills/superpowers/subagent-development/skill.md`

**Step 1: Update the Subagent Roles section**

Replace the generic "dispatch via agent_control with a focused prompt" with specific
agent names:

```markdown
## Subagent Roles

Three Lua agents, dispatched via `agent_control`:

1. **Implementer** —
   `agent_control(action: "start", agent: "coding-implementer", prompt: "<full task text + context>")`
2. **Spec reviewer** —
   `agent_control(action: "start", agent: "coding-spec-reviewer", prompt: "<spec text>")`
3. **Code quality reviewer** —
   `agent_control(action: "start", agent: "coding-quality-reviewer", prompt: "<scope>")`

After all tasks, dispatch the final reviewer:
`agent_control(action: "start", agent: "coding-reviewer", prompt: "<overall scope>")`
```

**Step 2: Remove the generic prompt templates from the skill body**

The skill.md no longer needs to inline prompt templates — the agents have their own
`prompt.md` files. Keep the workflow, process, red flags, and checklist sections intact.

**Step 3: Run tests**

Run: `just ci` Expected: All pass.

**Step 4: Commit**

```bash
git add prompts/skills/superpowers/subagent-development/skill.md
git commit -m "feat: update subagent-development skill to reference Lua agents"
```

---

## Phase 5: Install Default Agents + Polish

### Task 16: Add default agent installation for coding agents

**Files:**

- Modify: `src/agents/loader.rs` (or wherever default agents are installed)

**Step 1: Check if agent installation already handles defaults**

Look at how existing default agents (deep-research, deep-research-reflection,
chat-reflection) are installed. Follow the same pattern.

**Step 2: Add the 4 new agents to the default installation list**

Add `coding-implementer`, `coding-spec-reviewer`, `coding-quality-reviewer`,
`coding-reviewer` to whatever mechanism installs default agents.

**Step 3: Run tests**

Run: `just ci` Expected: All pass.

**Step 4: Commit**

```bash
git add src/agents/
git commit -m "feat: register coding Lua agents as default agents"
```

---

### Task 17: Update coding prompt builder for new skill paths

**Files:**

- Modify: `src/coding/prompt.rs`

**Step 1: Verify `build_coding_skills` finds superpowers skills**

Since `discover_skills` is now recursive and superpowers skills are under
`$WORKSPACE/skills/superpowers/`, they should be discovered automatically. The coding
prompt builder doesn't filter by `available`, so all skills appear.

Verify by reading the code — if `build_coding_skills` calls `skills::discover_skills`,
it already works. The only thing to check: `discover_repo_skills` (for
`.agents/skills/`) should also be recursive. If it isn't, apply the same recursive walk
pattern.

**Step 2: Make `discover_repo_skills` recursive if needed**

If `discover_repo_skills` in `coding/prompt.rs` still does single-level scanning, update
it to use the same `walk_skills_dir` helper (make it `pub` in `skills.rs`).

**Step 3: Run tests**

Run: `just ci` Expected: All pass.

**Step 4: Commit**

```bash
git add src/coding/prompt.rs src/skills.rs
git commit -m "refactor: ensure coding prompt builder uses recursive skill discovery"
```

---

### Task 18: Final verification and cleanup

**Files:**

- Modify: `src/skills.rs` — update test assertion count if changed

**Step 1: Verify skill counts**

Check `DEFAULT_SKILLS.len()` matches the test assertion:

- 8 GHOST-native skills (agent-creator, coding, deep-research, knowledge-navigator,
  nix-shell, note-writer, project-manager, reference-import)
- 13 superpowers skills
- Total: 21 (unchanged from before)

**Step 2: Verify GHOST prompt excludes superpowers skills**

Manually check: run the binary or write a quick test that `build_ghost_skills` with a
workspace containing installed default skills only shows the 8 GHOST skills.

**Step 3: Verify coding prompt includes all skills**

Similarly verify `build_coding_skills` shows all 21 skills.

**Step 4: Run full CI**

Run: `just ci` Expected: All pass, no clippy warnings.

**Step 5: Commit any remaining fixes**

```bash
git add -A
git commit -m "chore: final cleanup for superpowers overhaul"
```

---

## Implementation Notes

### Things to figure out during implementation

1. **GHOST native skill migration**: The 8 GHOST-native skills are currently flat files
   (`prompts/skills/agent-creator.md`). They don't need to become directories in the
   source tree — `include_str!` can point to a flat file and `install_default_skills`
   writes it as `<name>/skill.md`. But if we want consistency, we could move them to
   `prompts/skills/<name>/skill.md` too. Decision: keep flat for now, migrate later if
   needed.

2. **BackgroundTask struct**: Task 10 adds `cwd: Option<PathBuf>` to `BackgroundTask`.
   Check all places that construct `BackgroundTask` and add `cwd: None` for existing
   callers.

3. **Agent args passing**: The current `agent_control` `start` action only passes
   `prompt` (a string). The Lua agents expect `args.task_text`, `args.context`, etc. The
   `prompt` string becomes `args.prompt` in the Lua `build` function. For richer args,
   the coding agent would need to encode structured data in the prompt string, or
   `agent_control` needs an `args` JSON parameter. Check if the existing `run_with_args`
   path can be exposed. If not, the agents should work with just `args.prompt` and parse
   structured content from the prompt text.

4. **Companion file `read_file` paths**: When the coding agent runs with
   `cwd = /home/user/GHOST/code/ghost`, calling `read_file("skills/superpowers/...")`
   won't resolve because skills are under `$WORKSPACE/skills/`, not the repo. File tools
   resolve relative to `cwd`. The coding agent may need to use absolute paths for skill
   files, or `read_file` needs to also search the workspace. Check how the current
   `read_file` tool resolves paths — it uses `resolve_path(raw, base=cwd, workspace)`.
   Since skills are under workspace, `read_file("skills/...")` would fail if cwd is the
   repo dir. **Fix option**: in the coding agent's prompt, provide the workspace path
   and instruct it to use absolute paths for skill reads. Or: adjust `resolve_path` to
   try workspace as fallback. Investigate during implementation.

### Order of operations

Phase 1 (Tasks 1-4) is pure infrastructure — no behavior change. Phase 2 (Task 5) is the
big move — renames + adds `available: coding`. Phase 3 (Tasks 6-9) adds companion files.
Phase 4 (Tasks 10-15) creates Lua agents and wires cwd propagation. Phase 5 (Tasks
16-18) is polish and verification.

Within each phase, tasks are sequential.
