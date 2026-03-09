# Embedding Chunking Bug — Daemon E2E Test Findings

## How the test was run

```bash
cargo test --features live-tests test_ark_nova_import -- --nocapture
```

Branch: `feat/daemon-e2e-testing` (now merged to main).
Full output preserved in `e2e-output/2026-03-09T03-27-29_ark_nova_import/`.

## What happened

The daemon booted, chat received "Import the Ark Nova rules for future reference", the LLM used the `import_reference` tool which fetched the PDF via docling, converted it to markdown, and wrote it to disk. The full pipeline completed in ~135 seconds.

### ASSERT 1: PASSED

`count_references > 0` — the reference was created in the DB.

### ASSERT 2: FAILED — 3 embedding chunks instead of 50+

The reference file at `references/boardgames/arknova/feuerland-spiele-de-fileadmin-game-arche-nova-arche-nova-rul.md` is **1140 lines** — a substantial document. Yet the embedding pipeline produced only **1 chunk per source** (3 total: 2 from references, 1 from a note).

### ASSERT 3: Never reached

## Analysis

### Timeline from logs

1. **03:25:15** — Daemon boots, connects to DB, watcher starts
2. **03:25:22** — Chat begins, LLM calls `import_reference`
3. **03:26:33** — Docling converts PDF → 1140-line markdown file written to disk
4. **03:27:13** — Embedding pipeline runs on 2 sources:
   ```
   embed sources sources=2
   embed batch model=qwen3-embedding:8b, batch_size=2
   replace_embeddings_for_source ..., chunks=1   ← PROBLEM
   replace_embeddings_for_source ..., chunks=1   ← PROBLEM
   ```
5. **03:27:25** — File watcher picks up note change (NOT the reference change):
   ```
   process file_change kind=note, path=.../notes/boardgames/arknova/index.md
   replace_embeddings_for_source ..., chunks=1
   ```
6. **03:27:29** — settle() completes, assertion fires: 3 chunks total

### The bug: `chunk_markdown` produces 1 chunk from 1140 lines

The chunker at `src/embeddings/chunker.rs` has a `CHUNK_TARGET` of 2000 chars. A 1140-line file (~90K chars) should produce ~45 chunks. Instead it produces 1.

`chunk_content()` flow for `.md` files:
1. `detect_code_language("references/.../file.md")` → should return `None` for `.md`
2. Falls through to `chunk_markdown(content, tag_prefix)`
3. `chunk_markdown` uses tree-sitter to parse markdown into sections
4. Sections fitting within `CHUNK_TARGET` are emitted whole; oversized sections recurse

**Likely causes** (in order of probability):

1. **Tree-sitter parse failure on docling output** — docling-converted markdown may have unusual structure (no headers, raw text blocks, HTML artifacts) that tree-sitter can't parse, hitting `fallback_single_chunk` which emits one chunk regardless of size.

2. **Single top-level section** — the document may have one `#` header with everything underneath. If the tree-sitter AST has one root section, and the recursion into child nodes doesn't work correctly, it could produce one chunk.

3. **`split_oversized` not being called** — when a section exceeds `CHUNK_TARGET`, it should be split. If there's a code path that skips this for certain AST shapes, oversized sections pass through as single chunks.

### Secondary issue: watcher didn't re-embed the reference

The file watcher's `process_batch` at 03:27:25 only processed the note change, not the reference file. This could be because:
- The reference was already embedded by the import tool's direct call to `embed_sources` (at 03:27:13)
- The watcher detected the reference change but debounced it into the earlier batch
- Or the watcher just happened to only see the note change in its debounce window

This is separate from the chunking bug but worth noting — the watcher may not be reliably processing reference files written by tools.

## Next step

Investigate `chunk_markdown` with the actual docling output:

```bash
head -50 e2e-output/2026-03-09T03-27-29_ark_nova_import/references/boardgames/arknova/feuerland-spiele-de-fileadmin-game-arche-nova-arche-nova-rul.md
```

Write a unit test in `chunker.rs` that feeds docling-style markdown and asserts multiple chunks are produced.

## Files involved

- `src/embeddings/chunker.rs` — `chunk_content`, `chunk_markdown`, `CHUNK_TARGET`
- `src/embeddings/pipeline.rs` — `embed_sources`, `replace_embeddings_for_source`
- `src/daemon/watcher.rs` — `process_reference_change`, `process_batch`
- `tests/daemon_e2e.rs` — the failing test
