# 13 — Knowledge System with SurrealDB Graph

## Overview

The knowledge system is the GHOST's persistent memory. It stores notes, references, and
diary entries in SurrealDB with typed graph edges and hybrid search (full-text +
embeddings).

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

**`diary_write`** — Append to today's diary entry.

- Parameters: `content: string` (appended as bullet points)

**`identity_edit`** — Edit SOUL.md or OPERATOR.md.

- Parameters: `file: "SOUL" | "OPERATOR"`, `content: string`
- BOOT.md is only editable when explicitly directed by the OPERATOR.

## Acceptance Criteria

- Notes can be created, updated, and searched
- Wiki links are parsed and create typed graph edges in SurrealDB
- `[[relationship>Target]]` syntax creates edges with the specified label
- Missing link targets create stub notes
- Full-text search works across notes, references, and diary
- Graph queries can traverse relationships (1-hop and multi-hop)
- Knowledge write tools are separate from chat tools
- All knowledge operations produce tracing spans
- Tags are hierarchical, lowercase, slash-separated
- Archetypes provide template guidance but are not enforced

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
