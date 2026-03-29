# Replace Lua agent runtime with WASM components

## Problem

Ghost uses Lua (via mlua) for agent definitions: 4,454 lines of Rust bridge code. Lua
gives no typing, no compile-time validation, and no real sandboxing — the current
"sandbox" removes globals but agents can still shell out via `ctx:call_tool("shell")`.

## Goal

Replace the Lua agent runtime with WASM components compiled from Rust. A WIT interface
defines the exact contract between Ghost and agents. The shell tool is explicitly excluded
— the WIT becomes the complete, exhaustive list of what an agent can do.

A `ghost-agent` SDK crate (published on crates.io, alpha versioned, own release-please)
provides macros that eliminate WIT boilerplate. Ghost scaffolds each agent as a minimal
Rust crate; from there it's a normal crate the author can grow.

## Key design decisions

- **WIT as the security boundary.** Capability-based security by construction — agents
  can only call functions explicitly provided in the WIT interface.
- **No shell tool in agents.** If an agent needs a capability, it should be a named tool
  or a new WIT import, never an open shell.
- **Config and hooks in one file.** The `agent!` wrapping macro contains both config keys
  and hook functions. No sidecar config file — the macro needs to see everything to
  generate the Guest trait impl.
- **Standalone crates, not a Cargo workspace.** Agents live wherever makes sense (next to
  their skill, in `agents/` for scheduled ones). Ghost sets a shared
  `CARGO_TARGET_DIR=$WORKSPACE/.agent-cache/target` for build caching.
- **Cargo.toml is Ghost-created, not Ghost-managed.** Scaffolded once, then the author's
  file — they can add deps, change settings.
- **SDK published on crates.io.** Agents depend on it by version. Alpha-versioned with
  its own release-please component and publish workflow.
- **Schema derive from Rust types.** `#[derive(Schema)]` generates JSON schemas for
  custom tool parameters. No hand-written JSON schemas.
- **Crontab as TOML.** Pure config, no scripting needed — `toml::from_str` replaces Lua.
- **WIT embedded in SDK.** The `agent!` macro passes it inline to `wit_bindgen::generate!`
  — no WIT file in agent directories.
- **wasmtime with async imports.** All ctx methods (DB queries, tool execution) are async.
  wasmtime suspends the WASM fiber during await without blocking tokio.

## Agent syntax (target)

```rust
use ghost_agent::prelude::*;

agent! {
    name: "deep-research",
    description: "Iterative web research with source evaluation",
    model: "fast",
    reasoning_effort: high,
    max_iterations: 30,
    tools: ["knowledge_search", "web_search", "web_fetch", "file_read", "todo"],
    compaction: "Preserve: all URLs, TODO list, search history",

    build(ctx, args) {
        let prompt = ctx.read_file("prompt.md")?;
        BuildResult::new(prompt, [user(&args["prompt"])])
    }

    #[tool(description = "Submit findings", terminal)]
    report_findings(ctx, input: ReportInput) -> String {
        let data = serde_json::to_string(&input).unwrap();
        ctx.set("report_data", &data);
        ctx.spawn("deep-research-reflection", &[("report_data", &data)]);
        data
    }
}

#[derive(Deserialize, Schema)]
struct ReportInput {
    report: String,
    sources: Vec<Source>,
}
```

## Open questions

- **Exact `agent!` macro design.** Wrapping macro (shown above) vs attribute macro on a
  module vs `Agent` trait with default methods. Refine during SDK implementation.
- **Pre-turn / on-end-turn hooks.** Currently omitted from the WIT (nudges removed). Add
  back if agents need per-turn injection or progress gating.
- **TinyGo support.** TinyGo can target `wasip2` but doesn't support custom WIT worlds
  yet (tinygo-org/tinygo#4843). Go becomes viable when that's resolved.
- **Store reuse across hook calls.** Fresh instance per hook (simpler, safer) vs
  persistent instance within a run (avoids ~5us re-instantiation, negligible).
- **wasmtime binary size.** Adds ~5-10 MB to Ghost's 69 MB binary. Monitor.

## Implementation plan

`backlog/plans/2026-03-29-wasm-agents.md` — 15 tasks across 5 phases.
