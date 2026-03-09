---
name: writing-plans
description:
  Use when you have a spec or requirements for a multi-step task, before touching code.
  Write comprehensive implementation plans assuming the engineer has zero context for
  our codebase and questionable taste.
available: coding
---

# Writing Plans

## Overview

Write comprehensive implementation plans assuming the engineer has zero context for our
codebase and questionable taste. Document everything they need to know: which files to
touch for each task, code, testing, docs they might need to check, how to test it. Give
them the whole plan as bite-sized tasks. DRY. YAGNI. TDD. Frequent commits.

Assume they are a skilled developer, but know almost nothing about our toolset or
problem domain. Assume they don't know good test design very well.

**Announce at start:** "I'm using the writing-plans skill to create the implementation
plan."

**Save plans to:** `docs/plans/YYYY-MM-DD-<feature-name>.md`

## Bite-Sized Task Granularity

**Each step is one action (2-5 minutes):**

- "Write the failing test" - step
- "Run it to make sure it fails" - step
- "Implement the minimal code to make the test pass" - step
- "Run the tests and make sure they pass" - step
- "Commit" - step

## Plan Document Header

**Every plan MUST start with this header:**

```markdown
# [Feature Name] Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this
> plan task-by-task.

**Goal:** [One sentence describing what this builds]

**Architecture:** [2-3 sentences about approach]

**Tech Stack:** [Key technologies/libraries]

---
```

## Task Structure

````markdown
### Task N: [Component Name]

**Files:**

- Create: `exact/path/to/file.rs`
- Modify: `exact/path/to/existing.rs:123-145`
- Test: `tests/exact/path/to/test.rs`

**Step 1: Write the failing test**

```rust
#[test]
fn test_specific_behavior() {
    let result = function(input);
    assert_eq!(result, expected);
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test test_name -- --nocapture` Expected: FAIL with "cannot find function"

**Step 3: Write minimal implementation**

```rust
fn function(input: &str) -> Expected {
    Expected
}
```

**Step 4: Run test to verify it passes**

Run: `cargo test test_name -- --nocapture` Expected: PASS

**Step 5: Commit**

```bash
git add tests/path/test.rs src/path/file.rs
git commit -m "feat: add specific feature"
```
````

## Remember

- Exact file paths always
- Complete code in plan (not "add validation")
- Exact commands with expected output
- Reference relevant skills by name (e.g., "Read the `tdd` skill")
- DRY, YAGNI, TDD, frequent commits

## Execution Handoff

After saving the plan, offer execution to the OPERATOR:

**"Plan complete and saved to `docs/plans/<filename>.md`. Ready to execute?"**

If yes, use `agent_control(action: "start", ...)` to dispatch a coding agent per task,
reviewing between tasks for fast iteration.
