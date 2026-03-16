---
name: coding
description:
  Manage coding sessions. Use when the OPERATOR asks to work on code, hack on a repo,
  implement features, fix bugs, or do any development work in a codebase.
---

# Coding Session Manager

Use this skill when the OPERATOR asks for coding work on a repository.

## What This Does

You spawn a **coding agent** that takes over the Discord channel. The coding agent has
full access to the repo, can read/edit files, run commands, and commit changes. The
OPERATOR talks directly to the coding agent until they send `/kill` to end the session.

**Don't create a project** for single-session coding tasks. Only suggest a project if
the work will span multiple sessions or has 3+ distinct tasks to track.

## Workflow

### 1. Identify the Repo

Check if the OPERATOR has a project with a `repo.md` reference note:

```
knowledge_search query="repo.md <project-name>" scope="references"
```

If found, the `repo.md` will contain the repo URL and local path.

### 2. Set Up the Code Directory

Repos live under `code/` in the workspace. Use the project slug as the directory name.

**If not cloned yet:**

```
shell command="git clone <repo-url> code/<slug>"
```

**If already cloned:**

```
shell command="cd code/<slug> && git fetch && git status"
```

### 3. Start the Coding Session

```
shell command="ghost hack start code/<slug> --prompt '<task description>'"
```

The command outputs:

```
coding_session_id=<id>
session_id=<id>
working_dir=<path>
```

### 4. Explain to the OPERATOR

After starting, tell the OPERATOR:

> "I've started a coding session on `<repo>`. You're now talking to the coding agent —
> it has full access to the repo. Send `/kill` when you're done."

### 5. After `/kill`

When the OPERATOR ends the session, you'll receive a system message with a git summary
(branch, commits, changed files). Use this to:

- Update the project's task status if applicable
- Write a diary entry about what was accomplished
- Note any follow-up items the OPERATOR mentioned

### 6. Resume a Previous Session

If the OPERATOR wants to continue previous work:

```
shell command="ghost hack list"
```

Then resume with:

```
shell command="ghost hack resume <coding_session_id>"
```

## Creating `repo.md` References

When setting up a new repo for the first time, create a reference note:

```
note_write title="repo.md" project="<slug>" content="..."
```

Include: repo URL, local path (`code/<slug>`), branch conventions, build/test commands,
and any setup notes.

## Important

- Always check for an existing `repo.md` before cloning
- The channel ID is passed automatically — no need to specify it manually
- The coding agent has its own session — your conversation history is separate
- After `/kill`, the coding agent's session is preserved for future `resume`
