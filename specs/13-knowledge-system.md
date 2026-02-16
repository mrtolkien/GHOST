# 13 — Knowledge System with SurrealDB Graph

## Overview

The knowledge system is the GHOST's persistent memory. Knowledge is **fulltext-first**:
notes, references, and diary entries are markdown files on disk. SurrealDB indexes them
for search (BM25 + embeddings) and stores the knowledge graph (typed edges from wiki
links), but the files are the source of truth.

The GHOST queries knowledge via `ghost knowledge search` CLI commands (defined below),
which return **file paths** — then uses `read_file` to get full content. Knowledge write
operations during reflection use dedicated tools (`note_write`, `reference_write`,
`reference_manage`) for structured parameter validation.

The key evolution from t-koma is the graph model: wiki links can now carry relationship
types, enabling queries like "all things written_in Rust" or "all things that depend_on
X".

## Knowledge Types

### Notes

Atomic knowledge units (100-400 words typical, 1000 max). Each note covers one concept.

```rust
pub struct Note {
    pub id: Option<Thing>,
    pub title: String,
    pub body: String,
    pub archetype: Option<Archetype>,
    pub tags: Vec<String>,
    pub trust: u8,  // 1-10, default 5
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}
```

### Archetypes

Archetypes are note super-types that provide suggested templates. They are NOT enforced
— a note can have any archetype or none. Templates are guidance, not schema.

| Archetype      | Template guidance                                    |
| -------------- | ---------------------------------------------------- |
| `person`       | Name, role, context, relationship to OPERATOR        |
| `concept`      | Definition, examples, related concepts               |
| `decision`     | Context, options, chosen path, rationale, trade-offs |
| `event`        | When, where, who, what happened, outcome             |
| `place`        | Location, purpose, context                           |
| `project`      | Goals, status, dependencies, key decisions           |
| `organization` | What it does, key people, relationship to OPERATOR   |
| `procedure`    | Steps, prerequisites, expected outcome               |
| `media`        | Title, creator, summary, key takeaways               |
| `quote`        | Quote text, attribution, context                     |
| `topic`        | Hub note for a reference topic collection            |

### References

Preserved source material from the web, documentation, or code. References are the raw
sources that notes cite. Organized into topics:

```
knowledge/references/
├── surrealdb/
│   ├── getting-started.md
│   └── graph-queries.md
├── dioxus/
│   └── component-lifecycle.md
└── ...
```

### Diary

Daily timeline entries (`YYYY-MM-DD.md`). Append-only bullet points for events,
decisions, and observations. Details belong in notes — diary is the timeline.

## Graph Model (Typed Wiki Links)

The key SurrealDB differentiator. Wiki links in notes carry an optional relationship
type:

### Syntax

- `[[Target]]` — Default `relates_to` edge
- `[[relationship>Target]]` — Typed edge (e.g., `[[written_in>Rust]]`)

### Examples

```markdown
# Dioxus

Dioxus is a UI framework [[written_in>Rust]]. It was [[created_by>Jonathan Kelley]]. The
project [[competes_with>React]] in the frontend space and [[depends_on>VirtualDom]] for
its rendering pipeline.
```

This creates edges:

```
(Dioxus) --written_in--> (Rust)
(Dioxus) --created_by--> (Jonathan Kelley)
(Dioxus) --competes_with--> (React)
(Dioxus) --depends_on--> (VirtualDom)
```

### Graph Queries

SurrealDB makes graph traversal natural:

```surql
-- Find all things written in Rust
SELECT <-written_in<-note AS things FROM note WHERE title = "Rust";

-- Find all dependencies of Dioxus (1 hop)
SELECT ->depends_on->note AS deps FROM note WHERE title = "Dioxus";

-- Find all transitive dependencies (recursive)
SELECT ->depends_on->note->depends_on->note AS deep_deps FROM note WHERE title = "Dioxus";

-- Find all relationships of a note
SELECT ->relates_to->note, <-relates_to<-note FROM note WHERE title = "Dioxus";
```

### Edge Resolution

When a note is saved or updated:

1. Parse wiki links from the body using regex: `\[\[(?:(\w+)>)?([^\]]+)\]\]`
2. For each link: a. Check if target note exists (by title) b. If not, create a stub
   note (title only, no body) — it will be filled in later c. Create/update the edge
   with the appropriate label
3. Remove edges that no longer have corresponding wiki links

## Scopes (Simplified from t-koma)

Since there's one GHOST, the shared/ghost scope distinction collapses. All knowledge is
owned by the single GHOST. The only distinction that matters:

- **Notes** — The GHOST's interpretations and summaries
- **References** — Preserved source material (not the GHOST's words)
- **Diary** — Timeline entries

No SharedNote vs GhostNote distinction. No cross-ghost visibility concerns.

## Search

Hybrid search combining full-text and embeddings (see 14-embeddings.md for the
embeddings side):

### Full-Text Search

SurrealDB has built-in full-text search:

```surql
DEFINE ANALYZER note_analyzer TOKENIZERS blank, class FILTERS lowercase, snowball(english);
DEFINE INDEX idx_note_fts ON note FIELDS title, body SEARCH ANALYZER note_analyzer BM25;
DEFINE INDEX idx_reference_fts ON reference FIELDS content SEARCH ANALYZER note_analyzer BM25;
DEFINE INDEX idx_diary_fts ON diary FIELDS body SEARCH ANALYZER note_analyzer BM25;
```

### Search Strategy

1. Run full-text search (BM25) for keyword relevance
2. Run embeddings search (vector similarity) for semantic relevance
3. Merge results with weighted scoring (BM25 weight + embedding weight)
4. Optionally boost results that are graph-connected to recent conversation topics

## CLI Commands

### `ghost knowledge search "query"`

Hybrid search across notes, references, and diary. Returns **file paths** and snippets
so the GHOST can use `read_file` to get full content.

```
$ ghost knowledge search "surrealdb graph traversal"
knowledge/notes/surrealdb.md (score: 0.92)
  ...SurrealDB makes graph traversal natural using RELATE statements...

knowledge/references/surrealdb/graph-queries.md (score: 0.87)
  ...SELECT ->depends_on->note AS deps FROM note...

diary/2026-02-14.md (score: 0.61)
  ...Explored SurrealDB graph model for knowledge system...
```

Output is plain text, one result per block, sorted by relevance score. The
knowledge-search skill (spec 12) documents advanced options (type filters, tag filters,
output formats).

### `ghost knowledge get <path>`

Convenience to read a knowledge item with parsed metadata.

```
$ ghost knowledge get knowledge/notes/surrealdb.md
title: SurrealDB
archetype: concept
tags: database, graph
trust: 7
---
SurrealDB is an embedded database with native graph...
```

Also accepts a title: `ghost knowledge get --title "SurrealDB"`.

### `ghost knowledge graph <title-or-path>`

Show direct edges (incoming and outgoing) for a knowledge item. Depth 1 only — the GHOST
can call the command again on any listed node to explore further.

```
$ ghost knowledge graph "Dioxus"
Dioxus (note)
├─ written_in-> Rust (note)
├─ created_by-> Jonathan Kelley (stub)
├─ depends_on-> VirtualDom (note)
├─ competes_with-> React (note)
├<─ depends_on─ Freya (note)
└<─ cited_in─ session:abc123/msg:42
```

Options:

- `--type <edge_type>` — filter by edge type (e.g., `--type depends_on`)
- `--direction in|out` — show only incoming or outgoing edges
- `--types` — list all edge types in the graph with counts
- `--orphans` — list notes with no edges
- `--stats` — summary counts: notes, references, diary, edges, tags

The `cited_in` edges come from the citation system (spec 06) — when the GHOST cites a
note or reference in a response, a `cited` edge links the message to the knowledge item.
The knowledge-graph skill (spec 12) documents these advanced options.

### `ghost knowledge tags`

List all tags with note counts.

```
$ ghost knowledge tags
database (5)
database/graph (3)
rust (12)
rust/async (4)
people (8)
```

### `ghost knowledge recent`

Recently created or updated knowledge items (last 20 by default).

```
$ ghost knowledge recent
2026-02-16 14:30  knowledge/notes/surrealdb.md (updated)
2026-02-16 12:00  knowledge/references/dioxus/lifecycle.md (created)
2026-02-15 18:45  knowledge/notes/dioxus.md (updated)
```

### `ghost knowledge stats`

Summary of the knowledge graph.

```
$ ghost knowledge stats
Notes: 47 (3 stubs)
References: 23 across 8 topics
Diary entries: 12
Edges: 89 (12 types)
Tags: 34
```

### `ghost knowledge reindex`

Rebuild all search indexes and embeddings (spec 14).

## Knowledge Write Tools (Reflection Only)

These tools are available during reflection jobs, NOT during regular chat:

**`note_write`** — Create or update a note.

- Parameters: `action: "create" | "update"`, `title: string`, `body: string`,
  `archetype: string (optional)`, `tags: string[] (optional)`,
  `trust: number (optional)`
- On create: parses wiki links, creates edges
- On update: re-parses wiki links, reconciles edges

**`reference_write`** — Save a reference file.

- Parameters: `topic: string`, `path: string`, `content: string`,
  `source_url: string (optional)`

**`reference_manage`** — Move/delete reference files and web cache.

- Parameters: `action: "move" | "delete"`, `cache_file: string (optional)`,
  `target_topic: string (optional)`, `target_filename: string (optional)`
- On move: the file moves from `.web-cache/` to `knowledge/references/{topic}/` AND the
  SurrealDB record is updated (path field changes, type changes from `web_cache` to
  `reference`). All graph edges (including `cited` edges from messages) stay intact
  because they point to the record ID, not the file path. **This is critical** — moving
  a file must never break citation traceability.
- On delete: the file and its SurrealDB record are removed. Edges pointing to the record
  are cleaned up.

Diary and identity file editing are handled by `file_edit` directly — no dedicated tools
needed. The reflection prompt (spec 17) explains the file paths and conventions.

## Validation

1. `cargo test` — create a note, retrieve it by ID, verify all fields
2. `cargo test` — create a note with `[[Rust]]` wiki link, verify a `relates_to` edge
   and a stub note for "Rust" are created
3. `cargo test` — create a note with `[[written_in>Rust]]`, verify a `written_in` typed
   edge is created
4. `cargo test` — update a note: remove a wiki link, verify the corresponding edge is
   deleted
5. `cargo test` — full-text search: create several notes, search by keyword, verify
   ranked results
6. `cargo test` — graph traversal: create a chain (A ->depends_on-> B ->depends_on-> C),
   query 2-hop from A, verify C is returned
7. `cargo test` — knowledge write tools (`note_write`, `reference_write`,
   `reference_manage`) work through the ToolManager
8. `cargo test` — `reference_manage` move: move a web cache file to references, verify
   the SurrealDB record's path is updated and all edges (including `cited`) are
   preserved
9. `cargo test` — `ghost knowledge graph "X"` returns incoming + outgoing edges at depth
   1
10. `cargo test` — `ghost knowledge tags` returns tags with correct counts
11. `cargo test` — `ghost knowledge recent` returns items sorted by updated_at
12. `just ci` — passes

## Acceptance Criteria

- Notes can be created, updated, and searched
- Wiki links are parsed and create typed graph edges in SurrealDB
- `[[relationship>Target]]` syntax creates edges with the specified label
- Missing link targets create stub notes
- Full-text search works across notes, references, and diary
- Graph queries can traverse relationships (1-hop and multi-hop)
- `ghost knowledge search` returns file paths + snippets sorted by relevance
- `ghost knowledge get` reads a knowledge item with parsed metadata
- `ghost knowledge graph` shows depth-1 incoming and outgoing edges
- `ghost knowledge tags` lists all tags with counts
- `ghost knowledge recent` lists recently modified items
- `ghost knowledge stats` shows graph summary
- `reference_manage` move preserves all graph edges (record ID stays, path updates)
- Knowledge write tools are separate from chat tools (reflection only)
- All knowledge operations produce tracing spans
- Tags are hierarchical, lowercase, slash-separated
- Archetypes provide template guidance but are not enforced
- `just ci` passes

## Prior Art

Old code in `../t-koma`:

- `t-koma-knowledge/` — Knowledge indexing and search engine. The storage layer changes
  completely (SQLite + sqlite-vec → SurrealDB), but the concepts transfer: chunking,
  hybrid search scoring, BM25 + embeddings merge, note/reference/diary types.
- `t-koma-knowledge/src/engine/` — Search engine with ranking. Same concept, different
  backend.
- `t-koma-gateway/src/tools/knowledge_search.rs`, `knowledge_get.rs` — Search/get tool
  implementations. Reusable tool interface, change storage calls.
- `t-koma-gateway/src/tools/note_write.rs`, `reference_write.rs`, `diary_write.rs` —
  Write tool implementations. Reusable, add wiki link edge creation for SurrealDB.
