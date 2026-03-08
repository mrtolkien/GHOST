# Skill Path Standardization Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this
> plan task-by-task.

**Goal:** Auto-discover skill extras on `read_file`, standardize all skill path
references to name-only (cross-skill) or `./`-relative (same-skill).

**Architecture:** New `collect_extras()` in `src/skills.rs` walks the skill directory
excluding agent subdirs. `read_file` calls it when reading a `skill.md` and appends an
`<extra-files>` XML block. Skill `.md` files are rewritten to remove hardcoded paths.

**Tech Stack:** Rust (std::fs), existing `src/skills.rs` and `src/tools/read_file.rs`

**Design doc:** `docs/plans/2026-03-08-skill-paths-design.md`

---

### Task 1: `collect_extras()` in `src/skills.rs`

**Files:**

- Modify: `src/skills.rs`

**Step 1: Write the failing test**

Add to the `tests` module in `src/skills.rs`:

```rust
#[test]
fn collect_extras_finds_non_agent_files() {
    let dir = TempDir::new().unwrap();
    let skill_dir = dir.path().join("skills").join("my-skill");
    fs::create_dir_all(skill_dir.join("scripts")).unwrap();
    fs::create_dir_all(skill_dir.join("my-agent")).unwrap();

    // Skill file (excluded from extras)
    fs::write(skill_dir.join("skill.md"), "---\nname: my-skill\ndescription: Test.\n---\n").unwrap();
    // Extra files (included)
    fs::write(skill_dir.join("reference.md"), "ref").unwrap();
    fs::write(skill_dir.join("schema.sql"), "CREATE TABLE").unwrap();
    fs::write(skill_dir.join("scripts/run.py"), "print()").unwrap();
    // Agent dir (excluded entirely)
    fs::write(skill_dir.join("my-agent/agent.lua"), "return {}").unwrap();
    fs::write(skill_dir.join("my-agent/prompt.md"), "prompt").unwrap();

    let extras = collect_extras(&skill_dir);
    let paths: Vec<String> = extras.iter().map(|p| p.display().to_string()).collect();

    assert_eq!(paths.len(), 3);
    assert!(paths.contains(&"./reference.md".to_string()));
    assert!(paths.contains(&"./schema.sql".to_string()));
    assert!(paths.contains(&"./scripts/run.py".to_string()));
}

#[test]
fn collect_extras_empty_when_no_extras() {
    let dir = TempDir::new().unwrap();
    let skill_dir = dir.path().join("skills").join("simple");
    fs::create_dir_all(&skill_dir).unwrap();
    fs::write(skill_dir.join("skill.md"), "---\nname: simple\ndescription: Test.\n---\n").unwrap();

    let extras = collect_extras(&skill_dir);
    assert!(extras.is_empty());
}

#[test]
fn collect_extras_skips_nested_agent_dirs() {
    let dir = TempDir::new().unwrap();
    let skill_dir = dir.path().join("skills").join("complex");
    fs::create_dir_all(skill_dir.join("agent-a")).unwrap();
    fs::create_dir_all(skill_dir.join("agent-b")).unwrap();

    fs::write(skill_dir.join("skill.md"), "---\nname: complex\ndescription: Test.\n---\n").unwrap();
    fs::write(skill_dir.join("agent-a/agent.lua"), "return {}").unwrap();
    fs::write(skill_dir.join("agent-a/prompt.md"), "p").unwrap();
    fs::write(skill_dir.join("agent-b/agent.lua"), "return {}").unwrap();
    fs::write(skill_dir.join("agent-b/user-message.md"), "u").unwrap();

    let extras = collect_extras(&skill_dir);
    assert!(extras.is_empty());
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test --lib skills::tests::collect_extras -- --nocapture` Expected: FAIL —
`collect_extras` doesn't exist yet.

**Step 3: Implement `collect_extras`**

Add above the `#[cfg(test)]` block in `src/skills.rs`:

```rust
/// Collect extra files in a skill directory for the `<extra-files>` block.
///
/// Walks `skill_dir` recursively, returning `./`-relative paths for all
/// files except `skill.md` and anything inside agent directories (dirs
/// containing `agent.lua`). Returns sorted paths; empty vec if no extras.
pub fn collect_extras(skill_dir: &Path) -> Vec<PathBuf> {
    let mut extras = Vec::new();
    walk_extras(skill_dir, skill_dir, &mut extras);
    extras.sort();
    extras
}

fn walk_extras(base: &Path, dir: &Path, extras: &mut Vec<PathBuf>) {
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(_) => return,
    };

    // Skip this directory entirely if it contains agent.lua
    if dir != base && dir.join("agent.lua").exists() {
        return;
    }

    for entry in entries.flatten() {
        let path = entry.path();

        if path.is_dir() {
            walk_extras(base, &path, extras);
        } else {
            let name = entry.file_name();
            let name = name.to_string_lossy();

            // Skip skill.md itself
            if name == "skill.md" {
                continue;
            }

            if let Ok(rel) = path.strip_prefix(base) {
                extras.push(PathBuf::from(".").join(rel));
            }
        }
    }
}
```

**Step 4: Run tests to verify they pass**

Run: `cargo test --lib skills::tests::collect_extras` Expected: all 3 tests PASS.

**Step 5: Commit**

```bash
git add src/skills.rs
git commit -m "feat: add collect_extras() for skill companion file discovery"
```

---

### Task 2: Wire `read_file` to append `<extra-files>`

**Files:**

- Modify: `src/tools/read_file.rs`

**Step 1: Write the failing test**

Add to the `tests` module in `src/tools/read_file.rs`:

```rust
#[tokio::test]
async fn read_skill_md_appends_extra_files() {
    let workspace = TempDir::new().unwrap();
    let skill_dir = workspace.path().join("skills").join("test-skill");
    std::fs::create_dir_all(skill_dir.join("scripts")).unwrap();

    std::fs::write(
        skill_dir.join("skill.md"),
        "---\nname: test-skill\ndescription: Test.\n---\n\n# Test Skill\n",
    )
    .unwrap();
    std::fs::write(skill_dir.join("reference.md"), "ref content").unwrap();
    std::fs::write(skill_dir.join("scripts/run.py"), "print()").unwrap();

    let ctx = test_ctx_in(workspace.path());
    let result = ReadFile
        .execute(json!({"path": "skills/test-skill/skill.md"}), &ctx)
        .await
        .unwrap();

    assert!(result.contains("<extra-files>"));
    assert!(result.contains("./reference.md"));
    assert!(result.contains("./scripts/run.py"));
    assert!(result.contains("</extra-files>"));
}

#[tokio::test]
async fn read_skill_md_no_extras_no_block() {
    let workspace = TempDir::new().unwrap();
    let skill_dir = workspace.path().join("skills").join("bare-skill");
    std::fs::create_dir_all(&skill_dir).unwrap();

    std::fs::write(
        skill_dir.join("skill.md"),
        "---\nname: bare-skill\ndescription: Bare.\n---\n\n# Bare\n",
    )
    .unwrap();

    let ctx = test_ctx_in(workspace.path());
    let result = ReadFile
        .execute(json!({"path": "skills/bare-skill/skill.md"}), &ctx)
        .await
        .unwrap();

    assert!(!result.contains("<extra-files>"));
}

#[tokio::test]
async fn read_skill_md_excludes_agent_dirs() {
    let workspace = TempDir::new().unwrap();
    let skill_dir = workspace.path().join("skills").join("with-agent");
    std::fs::create_dir_all(skill_dir.join("my-agent")).unwrap();

    std::fs::write(
        skill_dir.join("skill.md"),
        "---\nname: with-agent\ndescription: Has agent.\n---\n\n# Agent Skill\n",
    )
    .unwrap();
    std::fs::write(skill_dir.join("my-agent/agent.lua"), "return {}").unwrap();
    std::fs::write(skill_dir.join("my-agent/prompt.md"), "prompt").unwrap();

    let ctx = test_ctx_in(workspace.path());
    let result = ReadFile
        .execute(json!({"path": "skills/with-agent/skill.md"}), &ctx)
        .await
        .unwrap();

    assert!(!result.contains("<extra-files>"));
    assert!(!result.contains("agent.lua"));
    assert!(!result.contains("prompt.md"));
}
```

**Step 2: Run tests to verify they fail**

Run: `cargo test --lib tools::read_file::tests::read_skill` Expected: FAIL — no
`<extra-files>` in output.

**Step 3: Implement the extra-files append**

In `src/tools/read_file.rs`, modify the `execute` method. After building the `result`
string (after the `for` loop at line 63), add:

```rust
// Append extra-files block for skill.md files
if raw_path.ends_with("skill.md") && path.components().any(|c| c.as_os_str() == "skills") {
    if let Some(skill_dir) = path.parent() {
        let extras = crate::skills::collect_extras(skill_dir);
        if !extras.is_empty() {
            result.push_str("\n<extra-files>\n");
            for extra in &extras {
                result.push_str(&format!("  <file path=\"{}\" />\n", extra.display()));
            }
            result.push_str("</extra-files>\n");
        }
    }
}
```

**Step 4: Run tests to verify they pass**

Run: `cargo test --lib tools::read_file::tests` Expected: all tests PASS (new and
existing).

**Step 5: Commit**

```bash
git add src/tools/read_file.rs
git commit -m "feat: read_file appends <extra-files> block for skill.md"
```

---

### Task 3: Rewrite cross-skill references in skill files

**Files:** All `.md` files under `prompts/skills/` with `read_file("skills/` patterns
pointing to _other_ skills' `skill.md`.

These references need to become name-only prose. The affected files and changes:

| File                                                                  | Old                                                                  | New                                                                                    |
| --------------------------------------------------------------------- | -------------------------------------------------------------------- | -------------------------------------------------------------------------------------- |
| `superpowers/writing-skills/skill.md:278`                             | `read_file("skills/other-skill/skill.md")`                           | "Read the other skill's `skill.md`" (example text — adjust to match surrounding prose) |
| `superpowers/writing-skills/skill.md:308`                             | `read_file("skills/superpowers/tdd/skill.md")`                       | "Read the `tdd` skill"                                                                 |
| `superpowers/writing-plans/skill.md:45`                               | `read_file("skills/superpowers/executing-plans/skill.md")`           | "Read the `executing-plans` skill"                                                     |
| `superpowers/writing-plans/skill.md:107`                              | `read_file("skills/<name>/skill.md")`                                | "Read the relevant skill"                                                              |
| `superpowers/brainstorming/skill.md:47`                               | `read_file("skills/superpowers/writing-plans/skill.md")`             | "Read the `writing-plans` skill"                                                       |
| `superpowers/brainstorming/skill.md:111`                              | `read_file("skills/superpowers/writing-plans/skill.md")`             | "Read the `writing-plans` skill"                                                       |
| `superpowers/executing-plans/skill.md:62`                             | `read_file("skills/superpowers/finishing-branch/skill.md")`          | "Read the `finishing-branch` skill"                                                    |
| `superpowers/executing-plans/skill.md:99`                             | `read_file("skills/superpowers/writing-plans/skill.md")`             | "Read the `writing-plans` skill"                                                       |
| `superpowers/executing-plans/skill.md:101`                            | `read_file("skills/superpowers/finishing-branch/skill.md")`          | "Read the `finishing-branch` skill"                                                    |
| `superpowers/systematic-debugging/skill.md:185`                       | `read_file("skills/superpowers/tdd/skill.md")`                       | "Read the `tdd` skill"                                                                 |
| `superpowers/subagent-development/code-quality-reviewer-prompt.md:12` | `read_file("skills/superpowers/requesting-review/code-reviewer.md")` | "Read the `code-reviewer.md` extra from the `requesting-review` skill"                 |

**Step 1: Edit each file**

Read each file, apply the substitution preserving surrounding context and sentence
structure. Keep the instruction clear for the LLM (e.g., "**REQUIRED:** Read the
`finishing-branch` skill").

**Step 2: Verify no stale references remain**

Run:
`grep -r 'read_file("skills/' prompts/skills/ --include='*.md' | grep -v 'same-skill extras'`

Filter out same-skill extras (handled in Task 4). Only cross-skill `skill.md` references
should be gone.

**Step 3: Commit**

```bash
git add prompts/skills/
git commit -m "refactor: replace cross-skill read_file paths with name-only references"
```

---

### Task 4: Rewrite same-skill extra references

**Files:** Skill `.md` files referencing their own extras via full
`read_file("skills/...")` paths.

| File                                                   | Old                                                                               | New                                                                                                     |
| ------------------------------------------------------ | --------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------- |
| `superpowers/writing-skills/skill.md:658`              | `read_file("skills/superpowers/writing-skills/best-practices.md")`                | "Read the `best-practices.md` extra"                                                                    |
| `superpowers/writing-skills/skill.md:661`              | `read_file("skills/superpowers/writing-skills/persuasion-principles.md")`         | "Read the `persuasion-principles.md` extra"                                                             |
| `superpowers/writing-skills/skill.md:664`              | `read_file("skills/superpowers/writing-skills/testing-skills-with-subagents.md")` | "Read the `testing-skills-with-subagents.md` extra"                                                     |
| `superpowers/writing-skills/best-practices.md:182-183` | `read_file("skills/pdf/forms.md")` / `read_file("skills/pdf/reference.md")`       | "Read the `forms.md` extra from the `pdf` skill" / "Read the `reference.md` extra from the `pdf` skill" |
| `superpowers/tdd/skill.md:354`                         | `read_file("skills/superpowers/tdd/testing-anti-patterns.md")`                    | "Read the `testing-anti-patterns.md` extra"                                                             |
| `superpowers/systematic-debugging/skill.md:292-298`    | 3x `read_file("skills/superpowers/systematic-debugging/...")`                     | "Read the `root-cause-tracing.md` extra", etc.                                                          |
| `superpowers/subagent-development/skill.md:271-273`    | 3x `read_file("skills/superpowers/subagent-development/...")`                     | "Read the `implementer-prompt.md` extra", etc.                                                          |
| `superpowers/requesting-review/skill.md:104`           | `read_file("skills/superpowers/requesting-review/code-reviewer.md")`              | "Read the `code-reviewer.md` extra"                                                                     |

**Step 1: Edit each file**

Same approach as Task 3. Replace with relative prose.

**Step 2: Verify no stale references remain**

Run: `grep -rn 'read_file("skills/' prompts/skills/ --include='*.md'` Expected: zero
matches.

**Step 3: Commit**

```bash
git add prompts/skills/
git commit -m "refactor: replace same-skill read_file paths with relative extra references"
```

---

### Task 5: Rewrite `$WORKSPACE` shell command paths

**Files:**

- Modify: `prompts/skills/image-generation/skill.md`

**Step 1: Edit the skill**

Replace the two shell command examples (lines 19 and 25):

Before:

```sh
uv run $WORKSPACE/skills/image-generation/scripts/generate_image.py --prompt '...' --filename '$WORKSPACE/tmp/...'
```

After:

```sh
uv run ./scripts/generate_image.py --prompt '...' --filename 'tmp/...'
```

Keep the argument documentation, use-case descriptions, and everything else unchanged.
The LLM constructs the full path from the skill's `<location>`.

**Step 2: Verify no `$WORKSPACE/skills/` references remain**

Run: `grep -rn '\$WORKSPACE/skills/' prompts/skills/ --include='*.md'` Expected: zero
matches.

**Step 3: Commit**

```bash
git add prompts/skills/image-generation/skill.md
git commit -m "refactor: use relative paths in image-generation skill shell commands"
```

---

### Task 6: Run CI and verify

**Step 1: Run full CI**

Run: `just ci` Expected: all checks pass (fmt, check, clippy, tests).

**Step 2: Verify extra-files works end-to-end**

Manually inspect by running Ghost and reading a skill with extras (e.g.,
`knowledge-navigator`) to confirm the `<extra-files>` block appears.

**Step 3: Final commit if any fixups needed**

```bash
git add -A
git commit -m "chore: CI fixups for skill path standardization"
```
