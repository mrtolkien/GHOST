# E2E Transcript

## Chat

### 1. user

**Content**

```text
Import the Dioxus documentation into my knowledge base. I'll ask questions about it once the import is done.
```

### 2. assistant

**Thinking / Raw Output**

- `reasoning`: 

**Tool Calls**

- `read_file`
```json
{
  "path": "skills/reference-import/skill.md"
}
```

### 3. user

**Tool Results**

- error=false
```text
File: skills/reference-import/skill.md (114 lines)
  1 | ---
  2 | name: reference-import
  3 | description:
  4 |   Import and query external documentation, code, and API references. Use when the
  5 |   OPERATOR asks about a library, framework, SDK, or tool — especially if
  6 |   knowledge_search returns no results for it.
  7 | ---
  8 | 
  9 | # Reference Import Skill
 10 | 
 11 | Import git repos and web pages as topic-scoped references into the knowledge base.
 12 | 
 13 | ## Decision Flow
 14 | 
 15 | Follow this order — stop as soon as you have an answer:
 16 | 
 17 | 1. **Search first**: `knowledge_search(query="<topic>", categories=["references"])`. If
 18 |    results exist, use them to answer. Done.
 19 | 2. **Git import** (preferred): find the docs repo via `gh`, import with
 20 |    `background: true`, tell the OPERATOR it's importing.
 21 | 3. **Crawl import** (fallback): only if no git source exists (e.g. docs-only site).
 22 | 4. **After import completes**: the result arrives as a `[shell-command completed]`
 23 |    system message. Then search the imported refs, edit the topic note, and answer.
 24 | 
 25 | ## CLI Commands
 26 | 
 27 | ```
 28 | ghost reference import --source git --url <url> --topic <name> \
 29 |     [--paths dir1,dir2] [--extensions .md,.rs]
 30 | 
 31 | ghost reference import --source page --url <url> --topic <name>
 32 | 
 33 | ghost reference import --source crawl --url <url> --topic <name> \
 34 |     [--max-depth 3] [--max-pages 50]
 35 | 
 36 | ghost topics list
 37 | 
 38 | ghost reference delete --topic <name>
 39 | ```
 40 | 
 41 | ## Git Import (Preferred)
 42 | 
 43 | ### Finding the docs repo
 44 | 
 45 | Documentation often lives in a separate repo (e.g. `DioxusLabs/docsite`, not
 46 | `DioxusLabs/dioxus`). One search is enough:
 47 | 
 48 | ```bash
 49 | gh search repos "docs OR docsite OR website" --owner=<Org> --json name,description
 50 | ```
 51 | 
 52 | ### Choosing paths + extensions
 53 | 
 54 | Browse …[4024b total]
```

### 4. assistant

**Thinking / Raw Output**

- `reasoning`: 

**Tool Calls**

- `todo`
```json
{
  "action": "plan",
  "description": "",
  "index": 0,
  "items": [
    {
      "description": "Check existing references for Dioxus docs in knowledge base",
      "title": "Search existing references"
    },
    {
      "description": "Find official Dioxus documentation repository and docs path",
      "title": "Locate docs source"
    },
    {
      "description": "Start background import into topic dioxus/docs",
      "title": "Run import"
    },
    {
      "description": "Confirm import started and report next step to operator",
      "title": "Report status"
    }
  ],
  "note": "",
  "status": "pending",
  "title": "",
  "updates": []
}
```

### 5. user

**Tool Results**

- error=false
```text
TODO [0/4]
1. ○ Search existing references — Check existing references for Dioxus docs in knowledge base
2. ○ Locate docs source — Find official Dioxus documentation repository and docs path
3. ○ Run import — Start background import into topic dioxus/docs
4. ○ Report status — Confirm import started and report next step to operator

```

### 6. assistant

**Thinking / Raw Output**

- `reasoning`: 

**Tool Calls**

- `todo`
```json
{
  "action": "update",
  "description": "",
  "index": 1,
  "items": [],
  "note": "Checking references for existing Dioxus imports",
  "status": "in_progress",
  "title": "",
  "updates": []
}
```

### 7. user

**Tool Results**

- error=false
```text
TODO [0/4]
1. ◉ Search existing references — Check existing references for Dioxus docs in knowledge base [Checking references for existing Dioxus imports]
2. ○ Locate docs source — Find official Dioxus documentation repository and docs path
3. ○ Run import — Start background import into topic dioxus/docs
4. ○ Report status — Confirm import started and report next step to operator

```

### 8. assistant

**Tool Calls**

- `knowledge_search`
```json
{
  "categories": [
    "references"
  ],
  "limit": 10,
  "query": "Dioxus documentation",
  "topic": "dioxus"
}
```

### 9. user

**Tool Results**

- error=false
```text
No results found.
```

### 10. assistant

**Thinking / Raw Output**

- `reasoning`: 

**Tool Calls**

- `todo`
```json
{
  "action": "batch_update",
  "description": "",
  "index": 0,
  "items": [],
  "note": "",
  "status": "pending",
  "title": "",
  "updates": [
    {
      "index": 1,
      "note": "No existing Dioxus references found in knowledge base.",
      "status": "done"
    },
    {
      "index": 2,
      "note": "Looking up the official docs repository.",
      "status": "in_progress"
    }
  ]
}
```

### 11. user

**Tool Results**

- error=false
```text
TODO [1/4]
1. ✓ Search existing references — Check existing references for Dioxus docs in knowledge base [No existing Dioxus references found in knowledge base.]
2. ◉ Locate docs source — Find official Dioxus documentation repository and docs path [Looking up the official docs repository.]
3. ○ Run import — Start background import into topic dioxus/docs
4. ○ Report status — Confirm import started and report next step to operator

```

### 12. assistant

**Tool Calls**

- `run_shell_command`
```json
{
  "background": false,
  "command": "gh search repos \"docs OR docsite OR website\" --owner=DioxusLabs --json name,description,url",
  "directory": "/tmp/.tmpAr24kD",
  "timeout_ms": 120000
}
```

### 13. user

**Tool Results**

- error=false
```text
Exit code: 0

--- stdout ---
[]

```

### 14. assistant

**Thinking / Raw Output**

- `reasoning`: 

**Tool Calls**

- `run_shell_command`
```json
{
  "background": false,
  "command": "gh search repos \"Dioxus docsite\" --json owner,name,description,url --limit 10",
  "directory": "/tmp/.tmpAr24kD",
  "timeout_ms": 120000
}
```

### 15. user

**Tool Results**

- error=false
```text
Exit code: 0

--- stdout ---
[{"description":"","name":"dioxus-docsite","owner":{"id":"U_kgDOBaYJqw","is_bot":false,"login":"scapula07","type":"User","url":"https://github.com/scapula07"},"url":"https://github.com/scapula07/dioxus-docsite"}]

```

### 16. assistant

**Thinking / Raw Output**

- `reasoning`: 

**Tool Calls**

- `web_search`
```json
{
  "max_results": 5,
  "query": "Dioxus documentation GitHub repository source"
}
```

### 17. user

**Tool Results**

- error=false
```text
1. Dioxuslabs.com documentation site - GitHub
   https://github.com/DioxusLabs/docsite
   This repository contains the source code for the https://dioxuslabs.com website. This website is written with Dioxus, pre-generated with dioxus_ssr.
   Sources: google #1, startpage #2 · score: 3.0

2. DioxusLabs/dioxus: Fullstack app framework for web, desktop, and ...
   https://github.com/DioxusLabs/dioxus
   GitHub SponsorsFund open source developers. PROGRAMS ... The examples in the main branch of this repository target the git version of dioxus and the CLI.
   Sources: google #2, startpage #1 · score: 3.0

3. dioxus-document - crates.io: Rust Package Registry
   https://crates.io/crates/dioxus-document
   Jan 16, 2026 ... Documentation. docs.rs/dioxus-document/0.7.3. Browse source. docs.rs/crate/dioxus-document/0.7.3/source. Repository. github.com/DioxusLabs/ ...
   Sources: google #4, startpage #3 · score: 1.2

4. Dioxus Labs - GitHub
   https://github.com/dioxuslabs
   Dioxus Labs is a small team of full-time open source enthusiasts on a mission to simplify app development. We want to support developers every step of the way.
   Sources: google #5, startpage #4 · score: 0.9

5. Dioxus | Fullstack crossplatform app framework for Rust
   https://dioxuslabs.com/
   Dioxus is the Rust framework for building fullstack web, desktop, and mobile apps. Iterate with live hotreloading, add server functions, and deploy in ...
   Sources: google #9, startpage #5 · score: 0.6


```

### 18. assistant

**Thinking / Raw Output**

- `reasoning`: 

**Tool Calls**

- `run_shell_command`
```json
{
  "background": false,
  "command": "gh api repos/DioxusLabs/docsite/contents/ --jq '.[].name'",
  "directory": "/tmp/.tmpAr24kD",
  "timeout_ms": 120000
}
```

### 19. user

**Tool Results**

- error=false
```text
Exit code: 0

--- stdout ---
.dockerignore
.github
.gitignore
.gitmodules
.vscode
Cargo.lock
Cargo.toml
Dockerfile
README.md
_typos.toml
docs-src
fly.toml
notes.md
packages

```

### 20. assistant

**Thinking / Raw Output**

- `reasoning`: 

**Tool Calls**

- `run_shell_command`
```json
{
  "background": false,
  "command": "gh api repos/DioxusLabs/docsite/contents/docs-src --jq '.[].name'",
  "directory": "/tmp/.tmpAr24kD",
  "timeout_ms": 120000
}
```

### 21. user

**Tool Results**

- error=false
```text
Exit code: 0

--- stdout ---
0.3
0.4
0.5
0.6
0.7
blog

```

### 22. assistant

**Thinking / Raw Output**

- `reasoning`: 

**Tool Calls**

- `run_shell_command`
```json
{
  "background": false,
  "command": "gh api repos/DioxusLabs/docsite/contents/docs-src/0.7 --jq '.[].name'",
  "directory": "/tmp/.tmpAr24kD",
  "timeout_ms": 120000
}
```

### 23. user

**Tool Results**

- error=false
```text
Exit code: 0

--- stdout ---
book.toml
src

```

### 24. assistant

**Thinking / Raw Output**

- `reasoning`: 

**Tool Calls**

- `todo`
```json
{
  "action": "batch_update",
  "description": "",
  "index": 0,
  "items": [],
  "note": "",
  "status": "pending",
  "title": "",
  "updates": [
    {
      "index": 2,
      "note": "Official docs source found: https://github.com/DioxusLabs/docsite (docs at docs-src/0.7/src).",
      "status": "done"
    },
    {
      "index": 3,
      "note": "Starting background import from official docsite repo.",
      "status": "in_progress"
    }
  ]
}
```

### 25. user

**Tool Results**

- error=false
```text
TODO [2/4]
1. ✓ Search existing references — Check existing references for Dioxus docs in knowledge base [No existing Dioxus references found in knowledge base.]
2. ✓ Locate docs source — Find official Dioxus documentation repository and docs path [Official docs source found: https://github.com/DioxusLabs/docsite (docs at docs-src/0.7/src).]
3. ◉ Run import — Start background import into topic dioxus/docs [Starting background import from official docsite repo.]
4. ○ Report status — Confirm import started and report next step to operator

```

### 26. assistant

**Tool Calls**

- `run_shell_command`
```json
{
  "background": true,
  "command": "ghost reference import --source git --url https://github.com/DioxusLabs/docsite --topic dioxus/docs --paths docs-src/0.7/src --extensions .md",
  "directory": "/tmp/.tmpAr24kD",
  "timeout_ms": 120000
}
```

### 27. user

**Tool Results**

- error=false
```text
Command started in background. You'll see the result as a system message when it completes.
```

### 28. assistant

**Thinking / Raw Output**

- `reasoning`: 

**Tool Calls**

- `todo`
```json
{
  "action": "batch_update",
  "description": "",
  "index": 0,
  "items": [],
  "note": "",
  "status": "pending",
  "title": "",
  "updates": [
    {
      "index": 3,
      "note": "Import command started in background.",
      "status": "done"
    },
    {
      "index": 4,
      "note": "Notified operator that import is running and will report completion.",
      "status": "done"
    }
  ]
}
```

### 29. user

**Tool Results**

- error=false
```text
TODO [4/4]
1. ✓ Search existing references — Check existing references for Dioxus docs in knowledge base [No existing Dioxus references found in knowledge base.]
2. ✓ Locate docs source — Find official Dioxus documentation repository and docs path [Official docs source found: https://github.com/DioxusLabs/docsite (docs at docs-src/0.7/src).]
3. ✓ Run import — Start background import into topic dioxus/docs [Import command started in background.]
4. ✓ Report status — Confirm import started and report next step to operator [Notified operator that import is running and will report completion.]

```

### 30. assistant

**Content**

```text
Done — I’ve started importing the official Dioxus docs in the background from:

- https://github.com/DioxusLabs/docsite  
- path: `docs-src/0.7/src`  
- topic: `dioxus/docs`

I’ll check and let you know as soon as the import finishes, then you can start asking questions.
```

### 31. user

**Content**

```text
Great, now explain how hooks work in Dioxus. Search the imported references for relevant docs.
```

### 32. assistant

**Thinking / Raw Output**

- `reasoning`: 

**Tool Calls**

- `read_file`
```json
{
  "path": "skills/knowledge-navigator/skill.md"
}
```

### 33. user

**Tool Results**

- error=false
```text
File: skills/knowledge-navigator/skill.md (81 lines)
  1 | ---
  2 | name: knowledge-navigator
  3 | description:
  4 |   Navigate and query the knowledge base effectively. Use when you need to search
  5 |   existing knowledge, browse references by topic, explore the knowledge graph, or check
  6 |   for orphan notes.
  7 | ---
  8 | 
  9 | # Knowledge Navigator Skill
 10 | 
 11 | This skill teaches you how to use the knowledge system's query capabilities.
 12 | 
 13 | ## Searching
 14 | 
 15 | Use `knowledge_search` for hybrid BM25 + semantic search. It defaults to notes and diary
 16 | — pass `categories: ["notes", "references", "diary"]` to include references.
 17 | 
 18 | For CLI access: `ghost knowledge search "query" [--kind note|reference|diary]`
 19 | 
 20 | ## Browsing References
 21 | 
 22 | References are organized by topic (subdirectory name under `references/`).
 23 | 
 24 | CLI commands:
 25 | 
 26 | - `ghost knowledge references` — list all references, grouped by topic
 27 | - `ghost knowledge references --topic rust` — list references for a specific topic
 28 | - `ghost knowledge references --limit 50` — increase result limit
 29 | 
 30 | ## Graph Traversal
 31 | 
 32 | The knowledge graph connects notes via typed edges (`[[wiki links]]`) and citations.
 33 | 
 34 | CLI commands:
 35 | 
 36 | - `ghost knowledge graph "Note Title"` — show incoming and outgoing edges
 37 | - `ghost knowledge graph "Note Title" --direction out` — outgoing edges only
 38 | - `ghost knowledge graph "Note Title" --direction in` — incoming edges only
 39 | - `ghost knowledge graph --orphans` — find notes with no connections
 40 | - `ghost knowledge graph --stats` — edge and stub counts
 41 | 
 42 | ## Tags
 43 | 
 44 | - `ghost knowledge tags` — list all tags with counts
 45 | 
 46 | ## Recent Activity
 47 | 
 48 | - `ghost knowledge recent [--limit 20]` — recently updated knowledge items
 49 | 
 50 | ## Stats
 51 | 
 52 | - `ghost knowledge stats` …[3571b total]
```

### 34. assistant

**Thinking / Raw Output**

- `reasoning`: 

**Tool Calls**

- `knowledge_search`
```json
{
  "categories": [
    "references"
  ],
  "limit": 8,
  "query": "hooks",
  "topic": "dioxus"
}
```

### 35. user

**Tool Results**

- error=false
```text
## References

- **dioxus/docs** (id: 01KJT46FSBTDN4KZKR5J81Y1V1, score: 0.40)
  # Hooks

- **dioxus/docs** (id: 01KJT46FSXVVWBBY3Z8X8M01XB, score: 0.40)
  # Custom Hooks

- **dioxus/docs** (id: 01KJT46FQE0ZJ34EG9HHFQVTW1, score: 0.40)
  # Hooks

- **dioxus/docs** (id: 01KJT46FSTDF48ERMWS8RZS9PW, score: 0.38)
  # Advanced Topics

- **dioxus/docs** (id: 01KJT46FQDPATNDQMNSY8D3CWQ, score: 0.37)
  # State Migration

- **dioxus/docs** (id: 01KJT46FSS0EMM9C30Y7H8ZJYX, score: 0.36)
  # Component Lifecycle

- **dioxus/docs** (id: 01KJT46FQQGPC95YJSXXRWPNWJ, score: 0.36)
  # Testing

- **dioxus/docs** (id: 01KJT46FQ24XMHKA0B4JDQTJC3, score: 0.35)
  # Interactivity

---
8 results total.
```

### 36. assistant

**Thinking / Raw Output**

- `reasoning`: 

**Tool Calls**

- `run_shell_command`
```json
{
  "background": false,
  "command": "ghost knowledge references --topic dioxus/docs --limit 30",
  "directory": "/tmp/.tmpAr24kD",
  "timeout_ms": 120000
}
```

### 37. user

**Tool Results**

- error=false
```text
Exit code: 0

--- stdout ---
No references for topic 'dioxus/docs'

```

### 38. assistant

**Thinking / Raw Output**

- `reasoning`: 

**Tool Calls**

- `run_shell_command`
```json
{
  "background": false,
  "command": "ghost knowledge search \"hooks\" --kind reference --topic dioxus --limit 10",
  "directory": "/tmp/.tmpAr24kD",
  "timeout_ms": 120000
}
```

### 39. user

**Tool Results**

- error=false
```text
Exit code: 0

--- stdout ---
0.400  [reference]  dioxus/docs  # Hooks
0.398  [reference]  dioxus/docs  # Custom Hooks
0.395  [reference]  dioxus/docs  # Hooks
0.380  [reference]  dioxus/docs  # Advanced Topics
0.375  [reference]  dioxus/docs  # State Migration
0.364  [reference]  dioxus/docs  # Component Lifecycle
0.360  [reference]  dioxus/docs  # Testing
0.355  [reference]  dioxus/docs  # Interactivity
0.353  [reference]  dioxus/docs  # How to Upgrade to Dioxus 0.5
0.341  [reference]  dioxus/docs  # Data Fetching

```

### 40. assistant

**Thinking / Raw Output**

- `reasoning`: 

**Tool Calls**

- `run_shell_command`
```json
{
  "background": false,
  "command": "find references -maxdepth 4 -type f | head -n 40",
  "directory": "/tmp/.tmpAr24kD",
  "timeout_ms": 120000
}
```

### 41. user

**Tool Results**

- error=false
```text
Exit code: 0

(no output)
```

### 42. assistant

**Thinking / Raw Output**

- `reasoning`: 

**Tool Calls**

- `run_shell_command`
```json
{
  "background": false,
  "command": "gh api repos/DioxusLabs/docsite/git/trees/main?recursive=1 --jq '.tree[].path' | grep 'docs-src/0.7/src' | grep -i 'hook' | head -n 30",
  "directory": "/tmp/.tmpAr24kD",
  "timeout_ms": 120000
}
```

### 43. user

**Tool Results**

- error=false
```text
Exit code: 0

--- stdout ---
docs-src/0.7/src/essentials/advanced/custom_hooks.md
docs-src/0.7/src/essentials/basics/hooks.md
docs-src/0.7/src/migration/to_05/hooks.md

```

### 44. assistant

**Thinking / Raw Output**

- `reasoning`: 

**Tool Calls**

- `web_fetch`
```json
{
  "url": "https://raw.githubusercontent.com/DioxusLabs/docsite/main/docs-src/0.7/src/essentials/basics/hooks.md"
}
```
- `web_fetch`
```json
{
  "url": "https://raw.githubusercontent.com/DioxusLabs/docsite/main/docs-src/0.7/src/essentials/advanced/custom_hooks.md"
}
```
- `web_fetch`
```json
{
  "url": "https://raw.githubusercontent.com/DioxusLabs/docsite/main/docs-src/0.7/src/SUMMARY.md"
}
```

### 45. user

**Tool Results**

- error=false
```text
# Hooks

In Dioxus, state that is local to a component is stored in *hooks*.

Dioxus hooks work similarly to React's hooks. If you haven't done much web development, hooks might seem particularly unusual. Hooks provide a way of storing state, and attaching effects composability in components. Even better - they're less verbose than declaring structs and implementing "render" traits!

## The `use_hook` primitive

All hooks in Dioxus are built on the `use_hook` primitive. While you might never directly use this primitive, it's good to know where all state eventually resides. The `use_hook` primitive is a function that takes an initializer and returns a `.clone()` of the value.

```rust
fn Simple() -> Element {
    let count = use_hook(|| 123);
    rsx! { "{count}" }
}
```

Whenever `use_hook` is called, one of two things happens:
- if this `use_hook` has never been called before, the initializer is ran and a new slot is created
- otherwise, `use_hook` returns a clone of the current value in the slot.

Internally, the "hook index" is incremented by 1 on every call to `use_hook` and reset to 0 before the component re-renders.

![Hook List](/assets/07/hook-list.png)

## Rules of Hooks

In Dioxus, we are transparent with the inner workings of the framework. Because hooks are implemented by walking an internal "hook list," they have certain rules that would cause walking the list to fail and your app to panic. It's important to note that these rules are not arbitrary - they are the intended result of how hooks are implemented.

Hooks use their call order to keep track of what state belongs to which hook. You must call hooks in the same order every time the component is run. To make sure the order is always the same, **you should only call hooks at the top level of a component or another hook**.

These rules mean that there are certain things you can't do with hooks:

### No Hooks in Conditionals

You should not call a hook function conditionally. When the component re-rend…[6173b total]
```
- error=false
```text
# Custom Hooks

Hooks are a great way to encapsulate business logic. If none of the existing hooks work for your problem, you can write your own.

When writing your hook, you can make a function that starts with `use_` and takes any arguments you need. You can then use the `use_hook` method to create a hook that will be called the first time the component is rendered.

## Composing Hooks

To avoid repetition, you can encapsulate business logic based on existing hooks to create a new hook.

For example, if many components need to access an `AppSettings` struct, you can create a "shortcut" hook:

```rust
{{#include ../docs-router/src/doc_examples/hooks_composed.rs:wrap_context}}
```

Or if you want to wrap a hook that persists reloads with the storage API, you can build on top of the use_signal hook to work with mutable state:

```rust
{{#include ../docs-router/src/doc_examples/hooks_composed.rs:use_storage}}
```

## Custom Hook Logic

You can use [`use_hook`](https://docs.rs/dioxus/latest/dioxus/prelude/fn.use_hook.html) to build your own hooks. In fact, this is what all the standard hooks are built on!

`use_hook` accepts a single closure for initializing the hook. It will be only run the first time the component is rendered. The return value of that closure will be used as the value of the hook – Dioxus will take it, and store it for as long as the component is alive. On every render (not just the first one!), you will get a reference to this value.

> Note: You can use the `use_on_destroy` hook to clean up any resources the hook uses when the component is destroyed.

Inside the initialization closure, you will typically make calls to other dioxus runtime methods. For example:

- The `use_signal` hook tracks state in the hook value, and uses [`ReactiveContext`](https://docs.rs/dioxus/latest/dioxus/prelude/struct.ReactiveContext.html) to make Dioxus re-render any component that has observed it whenever the signal's value changes.

Here is a simplified implementati…[4332b total]
```
- error=false
```text
# Summary

# Introduction

- [Welcome](index.md)
- [Getting Started](getting_started/index.md)

# Take a Tour

- [Overview](tutorial/index.md)
- [Tooling Setup](tutorial/tooling.md)
- [Creating a new app](tutorial/new_app.md)
- [Your First Component](tutorial/component.md)
- [Creating UI with RSX](tutorial/rsx.md)
- [Styling and Assets](tutorial/assets.md)
- [Adding State](tutorial/state.md)
- [Fetching Data](tutorial/data_fetching.md)
- [Add a Backend](tutorial/backend.md)
- [Working with Databases](tutorial/databases.md)
- [Routing and Structure](tutorial/routing.md)
- [Bundling](tutorial/bundle.md)
- [Deploying](tutorial/deploy.md)
- [Next Steps](tutorial/next_steps.md)

# Core Concepts

- [Overview](essentials/index.md)
- [Building User Interfaces](essentials/ui/index.md)
  - [Introducing RSX](essentials/ui/rsx.md)
  - [Elements and Text](essentials/ui/elements.md)
  - [Dynamic Attributes](essentials/ui/attributes.md)
  - [Conditional Rendering](essentials/ui/conditional.md)
  - [Rendering Lists](essentials/ui/iteration.md)
  - [Components](essentials/ui/components.md)
  - [Reconciliation](essentials/ui/render.md)
  - [Assets](essentials/ui/assets.md)
  - [Styling](essentials/ui/styling.md)
  - [Hot-Reload](essentials/ui/hotreload.md)
  - [Escape Hatches](essentials/ui/escape.md)
- [The Basics of State](essentials/basics/index.md)
  - [Intro to Reactivity](essentials/basics/reactivity.md)
  - [Storing State in Hooks](essentials/basics/hooks.md)
  - [Reactive Signals](essentials/basics/signals.md)
  - [User Input](essentials/basics/event_handlers.md)
  - [Async and Futures](essentials/basics/async.md)
  - [Data Fetching](essentials/basics/resources.md)
  - [Effects and Memos](essentials/basics/effects.md)
  - [Hoisting State](essentials/basics/hoisting.md)
  - [Global Context](essentials/basics/context.md)
  - [Stores and Collections](essentials/basics/collections.md)
  - [Error Handling](essentials/basics/error_handling.md)
  - [Suspense](essentials/basics/suspe…[6084b total]
```

### 46. assistant

**Thinking / Raw Output**

- `reasoning`: 

**Tool Calls**

- `web_fetch`
```json
{
  "url": "https://raw.githubusercontent.com/DioxusLabs/docsite/main/docs-src/0.7/src/essentials/basics/signals.md"
}
```
- `web_fetch`
```json
{
  "url": "https://raw.githubusercontent.com/DioxusLabs/docsite/main/docs-src/0.7/src/essentials/basics/effects.md"
}
```
- `web_fetch`
```json
{
  "url": "https://raw.githubusercontent.com/DioxusLabs/docsite/main/docs-src/0.7/src/essentials/basics/resources.md"
}
```

### 47. user

**Tool Results**

- error=false
```text
# Reactive Signals

In Dioxus, your app's UI is defined as a function of its current state. As the state changes, the components and effects that depend on that state will automatically re-run. Reactivity automatically *tracks* state and *derives* new state, making it easy to build large applications that are efficient and simple to reason about.

Dioxus provides a single source of mutable state: the **Signal**.

## State with Signals

In Dioxus, mutable state is stored in Signals. Signals are *tracked* values that automatically update *reactive contexts* that watch them. They are the source of state from which all other state is derived from. Signals are modified directly by event handlers in response to user input or asynchronously in futures.

You can create a signal with the `use_signal` hook:

```rust
let mut signal = use_signal(|| 0);
```

Once you have your signal, you can gain a reference to the signal's inner value by calling the `.read()`:

```rust
let mut signal = use_signal(|| 0);

// use `.read()` to access the inner value
let inner = signal.read();
```

For Signals whose inner can be cheaply cloneable, you can also use "function" syntax to get a direct `Clone` of the value.

```rust
let name = use_signal(|| "Bob".to_string());

// Call the signal like a function
let inner = name();

// Or use `.cloned()`
let inner = name.cloned();
```

Finally, you can set the value of the signal with the `.set()` method or get a mutable reference to the inner value with the `.write()` method:

```rust
{{#include ../docs-router/src/doc_examples/untested_06/reactivity.rs:signal_write}}
```

A simple component that uses `.read()` and `.write()` to update its own state with signals may look like:

```rust
fn Demo() -> Element {
    let mut count = use_signal(|| 0);

    // read the current value
    let current = count.read().clone();

    rsx! {
        button {
            onclick: move |_| *count.write() = current,
            "Increment ({current})"
        }
    }
}
…[19978b total]
```
- error=false
```text
# Effects and Memos

Signals provide a foundation for mutable state in Dioxus apps. Calls to `.read()` subscribe reactive scopes and calls to `.write()` queue side-effects.

However, sometimes we want to run *our own* side-effects when a Signal's value changes. Other times, we want to isolate reactive scopes such that changes to a signal do not automatically queue a component to be re-rendered. In these cases, we reach for Memos with `use_memo` and Effects with `use_effect`.

## Multiple Reactive Scopes

To understand Effects and Memos, we need to first understand that a single Signal (or other reactive value) can be read in multiple reactive scopes simultaneously. For instance, a signal may be shared among several components via props. Each component that calls `.read()` on the signal value is automatically subscribed to any changes of the signal's value. When the signal value changes, it runs the re-render side-effect.

Effects and Memos allow us to observe changes in reactive values without re-rendering components. We can isolate smaller units of reactivity with memos and then queue our own side-effects with effects.

![Multiple Readers](/assets/07/multiple-scopes.png)

Memos implement the `Readable` trait (but not the Writable trait!) and thus implement the same ergonomic extensions as signals. Both Memos and Effects are `Copy` and have the same lifecycle and Drop semantics as signals.

## Derived State with Memo

`use_memo` is a reactive primitive that lets you derive state from any tracked value. It takes a closure that computes the new state and returns a tracked value that contains the current state of the memo. When a dependency of the memo changes, the memo will rerun, and a new value will be calculated.

The value returned from the closure will only cause the memo's value to update - and thus any side-effects - when they are not equal, determined by the `PartialEq` between the old and new value.

```rust
{{#include ../docs-router/src/doc_examples/untested…[8229b total]
```
- error=false
```text
# Data Fetching

One of the most common asynchronous operations in applications is making network requests. This guide will cover how to fetch data in Dioxus, how to avoid waterfalls, and using libraries to manage caching and invalidating requests.

The hooks and techniques we cover here are built on top of the Future and Signal primitives.

## Library Dependencies

While Dioxus does not provide a built-in HTTP client, you can use the popular [reqwest](https://docs.rs/reqwest/latest/reqwest/) library to make asynchronous network requests. We will be using the reqwest library throughout the examples in this page. Before we start, make sure to add the `reqwest` and `serde` libraries to your `Cargo.toml`:

```sh
cargo add reqwest --features json
cargo add serde --features derive
```

Your Cargo.toml should have the reqwest and serde libraries:
```toml
[dependencies]
# ... dioxus and other dependencies
reqwest = { version = "*", features = ["json"] }
serde = { version = "1", features = ["derive"] }
```

We are planning on eventually integrating a library like [dioxus-query](https://crates.io/crates/dioxus-query) directly into Dioxus for better integration with the app router.

## Requests from Event Handlers

The simplest way to request data is simply by attaching an async closure to an EventHandler.

```rust
#[derive(serde::Deserialize)]
struct DogApi {
    message: String,
}

let mut img_src = use_signal(|| "image.png".to_string());

let fetch_new = move |_| async move {
    let response = reqwest::get("https://dog.ceo/api/breeds/image/random")
        .await
        .unwrap()
        .json::<DogApi>()
        .await
        .unwrap();

    img_src.set(response.message);
};

rsx! {
    img { src: img_src }
    button { onclick: fetch_new, "Fetch a new dog!" }
}
```

Whenever the user clicks the button, the `fetch_new` closure is fired, a new Future is spawned, and the network request is made. When the response is complete, we set `img_src` to the return value.

Unfort…[7939b total]
```

### 48. assistant

**Content**

```text
Great — I searched the imported `dioxus/docs` references (hits included **“Hooks”** and **“Custom Hooks”**) and here’s the practical summary.

## How hooks work in Dioxus

### 1) Hooks are component-local state + behavior
In Dioxus, local component state is stored in hooks. Under the hood, hooks are built on `use_hook`, which stores per-component values in an internal hook list.  
Source: https://raw.githubusercontent.com/DioxusLabs/docsite/main/docs-src/0.7/src/essentials/basics/hooks.md

### 2) Call-order is the core mechanism
Dioxus tracks hooks by **call order**, so hooks must be called in a consistent order each render.

Rules (same spirit as React):
- Call hooks at top level of a component or another hook
- Don’t call hooks in conditionals, loops, or closures  
Source: https://raw.githubusercontent.com/DioxusLabs/docsite/main/docs-src/0.7/src/essentials/basics/hooks.md

### 3) Main built-in pattern: signals + reactive scopes
Most state is done with `use_signal`:
```rust
let mut count = use_signal(|| 0);
```
Reading a signal subscribes the current reactive scope; writing queues updates. Components/effects/memos/resources all use this reactive model.  
Sources:  
- https://raw.githubusercontent.com/DioxusLabs/docsite/main/docs-src/0.7/src/essentials/basics/signals.md  
- https://raw.githubusercontent.com/DioxusLabs/docsite/main/docs-src/0.7/src/essentials/basics/effects.md

### 4) Other common hooks
- `use_memo`: derived state (recomputes when dependencies change)
- `use_effect`: run side-effects when tracked values change
- `use_resource` / `use_loader`: async derived state and loading flows  
Sources:  
- https://raw.githubusercontent.com/DioxusLabs/docsite/main/docs-src/0.7/src/essentials/basics/effects.md  
- https://raw.githubusercontent.com/DioxusLabs/docsite/main/docs-src/0.7/src/essentials/basics/resources.md

### 5) Custom hooks are first-class
You can create your own hooks by writing `use_*` functions and composing existing hooks (or `use_hook` directly).  
Source: https://raw.githubusercontent.com/DioxusLabs/docsite/main/docs-src/0.7/src/essentials/advanced/custom_hooks.md

---

If you want, next I can give you a **“which hook to use when” cheat sheet** (e.g., `use_signal` vs `use_memo` vs `use_resource`), plus a few gotchas that cause panics.
```

**Thinking / Raw Output**

- `reasoning`: 

## Agent

