# Bundled File Update System Design

## Problem

Ghost bundles ~59 files (skills, agents, crontab, flake template, type stubs, etc.) that
are written to the workspace on bootstrap. Currently, `install_default_*` functions
always overwrite without consent. When a new image ships updated bundled files, the user
has no visibility into what changed and no way to reject changes to files they've
customized.

## Solution

On daemon boot, diff all bundled files against their workspace counterparts. If any
differ, block boot and present a Discord dialog (v2 buttons) for the user to
accept/reject changes before proceeding.

## Architecture

### Bundled File Registry

A single central registry replaces the scattered `install_default_*` file lists. Every
bundled file is a `BundledFile` with a workspace-relative path and `include_str!`
content:

```rust
struct BundledFile {
    workspace_relative_path: &'static str,  // e.g. "skills/nix-shell/skill.md"
    content: &'static str,                   // include_str!() content
}

fn bundled_files() -> &'static [BundledFile] { ... }
```

All current install paths (`install_default_skills`, `install_default_agents`,
`bootstrap_workspace`) converge to use this registry. One code path for all bundled
files.

### Boot Flow

1. Build list of all bundled files from registry
2. For each file, compare against workspace:
   - **File doesn't exist in workspace** → auto-install silently
   - **File exists, content matches** → skip
   - **File exists, content differs** → add to pending updates
   - **File was removed from bundle but exists in workspace** → add to pending deletions
3. If no pending changes → boot normally
4. If pending changes → send Discord dialog, **block** until user responds

Blocking is acceptable because this only triggers on new image deployments, not routine
restarts.

### Discord Interaction

Requires building v2 button components and a component interaction handler (neither
exists today).

**Initial message** (v2 Container with action row):

> **Workspace Update Available** N files modified, M files removed
>
> [Accept All] [Review] [Reject All]

**Review mode** — for each changed file, a separate message:

> **skills/nix-shell/skill.md**
>
> ```diff
> - old line
> + new line
> ```
>
> [Accept] [Reject]

After all files reviewed (or Accept All / Reject All clicked):

- Apply accepted changes (overwrite workspace files)
- Delete accepted removals
- Skip rejected changes
- Boot continues

**Button interaction handling:**

- Extend `interaction_create` in `bot.rs` to handle component interactions (currently
  returns early for non-slash-commands)
- Use `custom_id` prefixes: `bundled_accept_all`, `bundled_review`,
  `bundled_reject_all`, `bundled_file_accept:{path}`, `bundled_file_reject:{path}`
- Communication between interaction handler and boot blocker via a `tokio::sync` channel

### Diff Generation

Use the `similar` crate for unified diffs. Truncate output at ~3500 chars with "... and
N more lines changed" to stay under Discord's 4000 char TextDisplay limit.

For deleted bundled files, show "This file was removed from the default bundle" with no
diff.

### Detecting Removed Bundled Files

The registry defines the current set of bundled files. To detect files that _were_
bundled but no longer are, store a manifest of previously installed bundled file paths
(a simple text file or JSON in `$WORKSPACE/.cache/bundled-manifest.json`). On boot, any
path in the old manifest but not in the current registry is a candidate for deletion
review.

## What Changes

- **New:** `src/bundled.rs` — central registry of all bundled files + diff/apply logic
- **New:** Discord v2 button components (type 2) + action row (type 1)
- **New:** Component interaction handler in `bot.rs` `interaction_create`
- **Refactor:** `install_default_skills()`, `install_default_agents()`,
  `bootstrap_workspace()` use the registry
- **New file in workspace:** `$WORKSPACE/.cache/bundled-manifest.json`
- **New dependency:** `similar` crate

## Key Decisions

- **Diff against workspace, not against last-installed version**: catches user edits in
  the diff, which is the point — user sees exactly what will change in their files
- **New files auto-install**: no user content to conflict with, no reason to gate
- **Removed bundle files go through review**: user may have customized them
- **Blocking boot**: simpler than async state tracking, acceptable UX since it only
  triggers on image updates
- **Single registry**: enforces that all bundled files go through the same code path,
  preventing future files from bypassing the update check
