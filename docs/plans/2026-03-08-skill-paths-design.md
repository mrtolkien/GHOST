# Skill Path Standardization & Auto Extra-Files

**Date:** 2026-03-08 **Status:** Approved

## Problem

Skill path references are inconsistent: some absolute (`$WORKSPACE/skills/...`), some
workspace-relative (`skills/superpowers/tdd/skill.md`), some assume folder names. This
makes skills fragile and verbose, and forces skill authors to manually list companion
files in the skill body.

## Design

### 1. Auto extra-files discovery

When `read_file` returns a `skill.md`, it auto-appends an XML block listing companion
files in the skill directory.

**New function in `src/skills.rs`:**

```rust
pub fn collect_extras(skill_dir: &Path) -> Vec<PathBuf>
```

- Walks `skill_dir` recursively
- Skips `skill.md` itself
- Skips any directory containing `agent.lua` (and all its contents — these are agent
  runtime internals, not skill extras)
- Returns `./`-relative paths, sorted alphabetically

**In `src/tools/read_file.rs`:** After reading a file, if the path ends with `skill.md`
and lives under a `skills/` directory, call `collect_extras` on the parent. If
non-empty, append:

```xml

<extra-files>
  <file path="./schema.sql" />
  <file path="./scripts/generate_image.py" />
</extra-files>
```

Nothing appended if no extras exist.

### 2. Cross-skill references: name-only

Replace all `read_file("skills/.../skill.md")` patterns with prose references by skill
name. The LLM resolves from the `<available_skills>` list in the system prompt, which
provides `<name>` and `<location>`.

Before: `read_file("skills/superpowers/tdd/skill.md")` After: "Read the `tdd` skill"

### 3. Same-skill extra references: relative prose

Replace explicit `read_file` paths to sibling files with prose references. The
`<extra-files>` block already lists them with `./`-relative paths, so the LLM can
construct the full path from the skill's `<location>`.

Before: `read_file("skills/superpowers/systematic-debugging/root-cause-tracing.md")`
After: "Read the `root-cause-tracing.md` extra"

### 4. Shell command paths: relative to skill dir

Shell commands in skills use `./`-relative paths instead of `$WORKSPACE/skills/...`. The
LLM constructs absolute paths via string concatenation from the known skill location.
The skill body focuses on _how_ to use the script (arguments, use-cases), not exact
paths.

Before:
`uv run $WORKSPACE/skills/image-generation/scripts/generate_image.py --prompt ...`
After: `uv run ./scripts/generate_image.py --prompt ...`

### 5. Path resolution

No changes to `read_file`'s path resolution. The LLM does string concatenation:

- Knows skill location from `<location>skills/foo/skill.md</location>`
- Sees extra at `./bar.md`
- Constructs `skills/foo/bar.md` for the `read_file` call

## Scope

### Code changes

1. `src/skills.rs` — new `collect_extras()` function
2. `src/tools/read_file.rs` — detect `skill.md`, call `collect_extras`, append XML

### Skill rewrites

~25 `read_file("skills/...")` references across skill `.md` files need rewriting to
name-only or relative prose. Shell command paths with `$WORKSPACE/` need simplification.

## Non-goals

- No changes to Lua agent `read_file` (agent-dir-relative resolution stays as-is)
- No changes to `read_file` path resolution logic
- No frontmatter changes (no `extras` field — discovery is automatic)
