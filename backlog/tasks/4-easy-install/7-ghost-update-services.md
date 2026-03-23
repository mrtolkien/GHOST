- ghost services
  - Lists services (saved as toml in /services) and how they are run (nix, docker,
    podman, ...). The goal is to have a dedicated way to list services _managed_ by
    GHOST.
  - The services.toml needs to be generated during init and should be editable through
    the CLI (ghost services add)
  - Mostly a way to validate it's all working, not a real way to manage (left up to
    shell tool + the proper commands of nix/docker/podman)
- ghost start/stop
  - Simple way to stop the daemon + associated services
  - should ghost daemon also start the services? I'd say no

---

# Design: services.toml + ghost services + ghost start/stop

## Overview

A generic, TOML-based service registry that records how to start, stop, update, and
check each service managed by GHOST. The GHOST writes and edits entries via CLI commands.
`ghost start`/`ghost stop` use the registry to control the full stack.

## services.toml

Lives at `$WORKSPACE/services/services.toml`. Generated during `ghost init` based on the
services the user chose. Editable afterwards via `ghost services add/remove` or by the
GHOST using shell commands.

Each entry is a TOML table with optional command fields — no special entry types. A
compose stack, a systemd unit, and a nix-managed process all look the same:

```toml
[containers]
start = "podman compose -f $WORKSPACE/services/docker-compose.yml up -d"
stop = "podman compose -f $WORKSPACE/services/docker-compose.yml down"
update = "podman compose -f $WORKSPACE/services/docker-compose.yml pull && podman compose -f $WORKSPACE/services/docker-compose.yml up -d"

[llama-server]
start = "systemctl --user start llama-server"
stop = "systemctl --user disable --now llama-server"
update = "nix profile upgrade nixpkgs#llama-cpp"
status = "systemctl --user is-active llama-server"

[docling-serve]
start = "systemctl --user start docling-serve"
stop = "systemctl --user disable --now docling-serve"
status = "systemctl --user is-active docling-serve"
```

macOS equivalent uses `launchctl bootstrap`/`bootout`/`print` instead.

All fields (`start`, `stop`, `update`, `status`) are optional. Commands that reference a
missing field are silently skipped for that entry.

### Ordering

Entries execute **top-to-bottom** for `start`, `update`, and `status`. For `stop`,
entries execute **bottom-to-top** (reverse order). This lets the user/GHOST control
execution order by positioning entries in the file.

### Daemon is NOT in services.toml

The ghost daemon is handled separately by `ghost start`/`ghost stop` — it is not an
entry in `services.toml`. This avoids circular issues (the daemon can't start itself) and
keeps `ghost update` (the binary) separate from `ghost services update`.

## CLI Commands

### ghost services list

Print a table of entries from `services.toml` showing name and which fields are present.

### ghost services add

```
ghost services add --name <name> [--start "cmd"] [--stop "cmd"] [--update "cmd"] [--status "cmd"]
```

All fields are optional, but at least one must be provided. Appends a new entry to
`services.toml`. Errors if name already exists.

### ghost services remove \<name\>

Removes an entry from `services.toml`. Errors if name doesn't exist.

### ghost services update

For each entry top-to-bottom:
- Skip if no `update` field
- Run the `update` command
- On failure: print the error and **stop** (do not continue to next entry)
- On success: print confirmation, continue

Does NOT include the ghost binary itself — `ghost update` remains separate to avoid
unexpected downtime.

### ghost services status

For each entry top-to-bottom:
- Skip if no `status` field
- Run the `status` command
- Display result (pass/fail) per entry

### ghost start

1. Load `services.toml` (if missing, skip to step 3 — no services to start)
2. For each entry top-to-bottom, run its `start` command (skip if missing)
3. Start the ghost daemon via the OS service manager (`launchctl bootstrap` / `systemctl --user start ghost-daemon`). This tells the service manager to run `ghost daemon` — it does NOT run the daemon in-process.
4. Run `ghost status` and display output

### ghost stop

1. Stop the ghost daemon via the OS service manager (`launchctl bootout` / `systemctl --user disable --now ghost-daemon`)
2. Load `services.toml` (if missing, skip to step 4)
3. For each entry **bottom-to-top**, run its `stop` command (skip if missing)
4. Run `ghost status` and display output

## ghost init integration

During `ghost init`, after service files and compose are written, always generate
`services.toml` with entries matching the user's choices (even if empty — no optional
services selected). Platform-specific commands (systemd vs launchd) are filled in based
on `detect::Platform`. All paths in command strings are baked in as absolute paths at
write time (no `$WORKSPACE` variable expansion at runtime).

## ghost status — unchanged

`ghost status` continues to do config validation + HTTP health probes. It does NOT read
`services.toml`. The two are complementary:
- `ghost status` = "is my GHOST healthy?" (config valid, endpoints responding)
- `ghost services status` = "are managed processes running?" (process-level checks)

## Interaction with ghost reset

`ghost reset` currently hardcodes service shutdown. After this feature, it should read
`services.toml` (if it exists) and run `stop` commands instead, falling back to the
current hardcoded behavior if the file is missing (for backwards compat during the
transition).

## Documentation

Update `assets/skills/services/skill.md` to document the new CLI commands (`ghost
services list/add/remove/update/status`, `ghost start`, `ghost stop`). The GHOST reads
this skill to manage services — it must know about these commands so it uses them instead
of raw systemctl/launchctl/compose.

**Keep the skill SHORT AND CONCISE.** The new commands replace most of the manual
command examples currently in the skill. Remove redundant sections — if `ghost start`
handles starting everything, we don't need pages of systemctl/launchctl/compose start
examples anymore. The skill should tell the GHOST _what commands to run_, not be a
sysadmin reference manual.

## Error handling

- Missing `services.toml`: error for `ghost services *` commands. For `ghost
  start`/`ghost stop`, treat as empty (just control the daemon, no services)
- Malformed TOML: parse error with file path and line number
- Command failure: show stderr, identify which service failed, stop execution
  (for `update` and `start`). For `stop`, continue to next entry (best-effort cleanup)
- Missing field: silently skip (not an error)
