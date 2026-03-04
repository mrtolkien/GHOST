# Reference Import — Deferred Work

Follow-up items not covered in the core phase. To be tackled after Steps 1-8 are
implemented and validated.

## Tree-sitter code chunking

AST-aware chunking for code files (`.rs`, `.py`, `.js`, `.ts`, `.go`, `.java`, etc.).
Splits at function/struct boundaries instead of character count. Metadata prepend with
file path, language, and scope (e.g., `[scope: impl Foo > bar]`).

- New file: `src/embeddings/code_chunker.rs`
- Dependencies: `tree-sitter` + per-language grammar crates (~10 crates)
- `chunk_code()` delegates to code_chunker when language detected, else falls back to
  `chunk_text()`
- See spec `01_reference-import-specs.md` Step 3 for full algorithm (cAST-inspired)

## Crawl import (BFS)

BFS crawler for documentation sites not on git.

- New file: `src/reference_import/crawl.rs`
- `VecDeque<(Url, usize)>` queue + `HashSet<String>` visited
- Reuses `web::fetch::fetch()` pipeline (htmd → readability → crawl4ai)
- Same-host link extraction from markdown output
- Respects `max_depth` and `max_pages`
- CLI:
  `ghost reference import --source crawl --url <url> --topic <name> [--max-depth 3] [--max-pages 50]`
- Test: `tests/reference_import_crawl.rs` under `live-tests` feature

## E2e step-based test harness

Shared harness for multi-step e2e scenarios using fixture chains.

- `tests/e2e/harness.rs` — step runner, fixture management, workspace snapshots
- `scripts/e2e/` — Python tooling (launcher, refresh, render_log, diff, analyze_request)
- Hard fail on missing predecessor fixtures
- Artifacts per model: state.json, transcript.json/md, metrics.json
- Manual refresh: `uv run scripts/e2e refresh --models <aliases>`
- Sequential execution only (`--test-threads=1`)

## Reference-import e2e scenario

After harness exists, add a dedicated scenario:

- Step 01: import Dioxus docs reference topic
- Step 02: ask a Dioxus question in chat
- Step 03: verify topic-scoped retrieval and response quality
- Optional Step 04: reflection checks on produced notes/references

## Skill discovery e2e test

- Chat session: user asks "How do I create components in Dioxus?"
- Assert: GHOST reads a skill file
- Assert: GHOST calls `knowledge_search` before answering
- Soft assertion for reference-import suggestion

## Topic files (`topics/` workspace directory)

Currently topic descriptions live in notes (archetype=Topic). Future evolution:

- New `topics/` directory alongside `notes/`, `references/`, `diary/`
- Topic files sync to topic DB table via watcher
- Removes dependency on notes for topic metadata
- Topics become fully first-class workspace entities

## Topic enrichment agent

Post-import LLM pass to auto-generate topic descriptions from imported content.
Currently manual: the GHOST or user writes the topic note description after import.

## Sub-collection depth

Convention is max 3 levels (`topic/collection/subcollection`). No enforcement exists. If
deeper hierarchies become common, add validation or tree-aware queries.
