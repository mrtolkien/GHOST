# Migrate from SurrealDB to SQLite + sqlite-vec + FTS5

## Context

SurrealDB has been unstable — FLEXIBLE broken on SCHEMAFULL, pinned to a git alpha rev
for `search::score` bind param fix, 4 GiB memory cap needed as safety net, SCHEMALESS
workarounds everywhere. SQLite is battle-tested, the predecessor t-koma already proved
the stack works (sqlx + sqlite-vec + FTS5), and it eliminates an entire class of bugs.

**Goal:** Replace `surrealdb` with `sqlx` (SQLite) + `sqlite-vec` (vector KNN) + FTS5
(full-text search). No data migration — fresh DB on first run.

---

## Decisions

| Question           | Decision                                                           |
| ------------------ | ------------------------------------------------------------------ |
| IDs                | ULID strings (`ulid` crate) — sortable, no collisions              |
| Schema versioning  | sqlx migrations (`migrations/` dir)                                |
| Graph edges        | Regular junction tables with `from_id`/`to_id` + CASCADE deletes   |
| vec0 dimension     | Dynamic at startup via `connect(workspace, embedding_dim)`         |
| DB files           | Single `ghost.db` (not split like t-koma)                          |
| FTS5 scores        | Negate at search boundary (FTS5 returns negative = better)         |
| Vector distance    | Normalize vectors to unit length at insert → L2 ≈ cosine ranking   |
| JSON arrays        | `TEXT` columns with `serde_json` ser/de for tool_calls, tags, etc. |
| sqlite-vec loading | `sqlite3_auto_extension` via sqlx's FFI before pool creation       |

---

## Phase 1 — Dependencies & connection scaffold

**Files:**

- `Cargo.toml` — remove `surrealdb`, add `sqlx`, `sqlite-vec`, `ulid`
- `migrations/001_initial.sql` — new (full schema)
- `src/db/connection.rs` — rewrite (SqlitePool + sqlite-vec init + migrations)
- `src/db/error.rs` — rewrite (sqlx::Error sources)
- `src/db/mod.rs` — update exports, `fmt_id` becomes `&str → String`
- `src/db/schema.rs` — **delete** (replaced by migrations)
- `src/db/query.rs` — **delete** (SurrealDB-specific helpers)

### Cargo.toml changes

Remove:

```toml
surrealdb = { git = "...", features = ["kv-surrealkv"] }
```

Add:

```toml
sqlx = { version = "0.8", features = ["runtime-tokio", "sqlite", "chrono"] }
sqlite-vec = "0.1"
ulid = "1"
libsqlite3-sys = { version = "0.30", features = ["bundled"] }
```

Note: `libsqlite3-sys` with `bundled` ensures a consistent SQLite version with FTS5
support. Need to verify sqlx's bundled SQLite includes FTS5 — if not, this explicit dep
forces it.

### Migration SQL (`migrations/001_initial.sql`)

Tables: `session`, `message`, `interface_session`, `job_log`, `usage_log`, `note`,
`reference`, `diary`, `relates_to`, `cited`, `embedding`.

Virtual tables: `note_fts`, `reference_fts`, `diary_fts` (FTS5 with `content=` external
content mode + sync triggers).

The `vec_embedding` vec0 table is created at runtime in `connection.rs` because its
dimension comes from config.

FTS5 uses `tokenize='porter unicode61'` (Porter stemmer = English snowball equivalent
from SurrealDB's `note_analyzer`).

Note title gets 2x weight via `bm25(note_fts, 2.0, 1.0)` at query time.

### Connection setup

```
connect(workspace, embedding_dim) → SqlitePool
  1. sqlite3_auto_extension(sqlite3_vec_init) — once per process
  2. SqlitePool with WAL, 64MB cache, foreign_keys=ON, busy_timeout=5s
  3. sqlx::migrate!("./migrations").run(&pool)
  4. CREATE VIRTUAL TABLE IF NOT EXISTS vec_embedding USING vec0(...)
```

`GhostDb` type alias changes from `Surreal<Db>` to `SqlitePool`.

---

## Phase 2 — Port DB modules

Each module: rewrite queries from SurrealQL to SQL, change record types from
`SurrealValue` derive to `sqlx::FromRow`, change `RecordId` to `String`, change
`Datetime` to `String`.

### 2a. `src/db/sessions.rs` (~15 operations)

Pattern for all queries:

```rust
sqlx::query_as::<_, Row>("SELECT ... WHERE id = ?")
    .bind(id)
    .fetch_optional(db)  // or fetch_one, fetch_all
    .await
    .map_err(|e| DatabaseError::Query { table: "session", operation: "get", source: e })?
```

Key changes:

- `create_session()` / `create_agent_session()` — generate ULID, INSERT
- `create_message_with_metadata()` — serialize tool_calls/tool_results/raw_output to
  JSON strings
- `get_session_todo_list()` / `set_session_todo_list()` — JSON TEXT column
- `list_messages_by_session()` — same query, just `?` params instead of `$param`

### 2b. `src/db/interface_sessions.rs` (4 operations)

- `set_active_session_for_interface()` — `INSERT OR REPLACE` (UNIQUE index on interface)
- `replace_session_everywhere()` —
  `UPDATE interface_session SET session_id = ? WHERE session_id = ?`

### 2c. `src/db/job_logs.rs` (5 operations)

Straightforward INSERT/UPDATE/SELECT ports.

### 2d. `src/db/knowledge/records.rs`

All record types:

- Drop `SurrealValue` derive + `#[surreal(...)]` attributes
- Add `sqlx::FromRow` derive
- `RecordId` → `String`
- `Datetime` → `String`
- `Vec<String>` tags/sources → `String` (JSON), add `_parsed()` helpers
- `EdgeRecord`: `in_node`/`out` → `from_id`/`to_id`

### 2e. `src/db/knowledge/crud.rs` (~25 operations)

- Tags/sources: `serde_json::to_string(&tags)` on insert, `json_each()` for queries that
  split
- `append_diary()` — `UPDATE diary SET body = body || char(10) || ? WHERE date = ?`
- `list_recent()` — single `UNION ALL` query across note/reference/diary

### 2f. `src/db/knowledge/search.rs` (most complex)

FTS5 queries:

```sql
-- search_notes (title 2x weighted)
SELECT n.id, n.title, n.body, bm25(note_fts, 2.0, 1.0) AS score
FROM note_fts JOIN note n ON n.rowid = note_fts.rowid
WHERE note_fts MATCH ? ORDER BY score LIMIT ?
```

FTS5 `bm25()` returns **negative** scores (lower = better). Negate at the boundary so
`SearchHit.score` is positive-higher-is-better, keeping `hybrid_merge()` unchanged.

`hybrid_merge()` itself: only change is `fmt_id(&hit.id)` → `hit.id.clone()`.

### 2g. `src/db/knowledge/graph.rs` (8 operations)

- `create_edge()` —
  `INSERT INTO relates_to (id, from_id, to_id, label, created_at) VALUES (?, ?, ?, ?, ?)`
- `orphan_notes()` —
  `NOT EXISTS (SELECT 1 FROM relates_to WHERE from_id = n.id) AND NOT EXISTS (...to_id...)`
- `delete_outgoing_edges()` — `DELETE FROM relates_to WHERE from_id = ?`

### 2h. `src/db/knowledge/stats.rs` (6 operations)

- `list_tags_with_counts()` —
  `SELECT j.value AS tag, COUNT(*) FROM note, json_each(note.tags) AS j GROUP BY j.value`
- Count queries: `SELECT COUNT(*) FROM {table}` (identical)

### 2i. `src/db/embeddings.rs` (6 operations)

Dual-table pattern: `embedding` (metadata) + `vec_embedding` (vec0 vectors).

- `upsert_embedding()` — delete old row from both tables, insert new into both (use
  RETURNING rowid to link them)
- `vector_search()` —
  `SELECT ... FROM vec_embedding v JOIN embedding e ON e.rowid = v.rowid WHERE v.embedding MATCH ? ORDER BY v.distance LIMIT ?`
- Distance → similarity: `1.0 / (1.0 + distance)` or just negate for ranking (since
  hybrid_merge normalizes anyway)
- `delete_embeddings_for_source()` — delete from both tables
- `delete_all_embeddings()` — delete from both tables

---

## Phase 3 — Update consumers

Mechanical changes across ~26 files:

### Global search-and-replace patterns

1. `use surrealdb::*` → remove
2. `Surreal<Db>` → `SqlitePool` (or just `GhostDb`)
3. `&RecordId` → `&str` in params
4. `RecordId` → `String` in struct fields / returns
5. `RecordId::from_table_key("table", id)` → just `id`
6. `crate::db::fmt_id(&id)` → `id.clone()` / `&id`
7. `surrealdb::types::Datetime` → `String`

### Key files

- `src/chat/session.rs` — `SessionChat.db` type, session_id types
- `src/chat/convert.rs` — `parse_session_thing()` simplifies to strip `"session:"`
  prefix
- `src/chat/compaction.rs` — param types
- `src/tools/context.rs` — `ToolContext.db: SqlitePool`
- `src/tools/todo.rs` — `parse_session_thing()` + param types
- `src/tools/knowledge_search.rs` — uses search functions (API unchanged if search fns
  keep same signature)
- `src/tools/note_write.rs` — uses crud + graph functions
- `src/agents/runner.rs` — `TaskRunner.db`, `TaskHandle` fields,
  `parse_task_session_thing()`
- `src/agents/watcher.rs` — param types
- `src/jobs/reflection.rs` — param types
- `src/daemon/run.rs` — `connect()` call, pass embedding_dim
- `src/daemon/watcher.rs` — `GhostDb` type
- `src/interfaces/discord/bot.rs` — `DiscordHandler.db` type
- `src/embeddings/pipeline.rs` — `&RecordId` → `&str`
- `src/knowledge/reconcile.rs` — `RecordId` → `String`
- `src/cli/session.rs`, `src/cli/knowledge.rs`, `src/cli/job.rs` — param types
- `src/error.rs`, `src/agents/error.rs`, `src/jobs/error.rs`, `src/knowledge/error.rs` —
  `surrealdb::Error` → `sqlx::Error`

---

## Phase 4 — Tests

- `tests/common.rs` — `test_database()` creates temp SQLite, `LiveTestEnv` uses
  `SqlitePool`
- All test files: `RecordId` → `String`, remove surrealdb imports
- Run `just ci` until clean

---

## Phase 5 — Cleanup

- Delete `src/db/schema.rs`, `src/db/query.rs`
- Remove `surrealdb` from Cargo.toml
- Delete `workspace/ghost.db` (old SurrealDB data)
- Update `CLAUDE.md`: SurrealDB → SQLite in deps list, remove SurrealDB skill reference
- Update `MEMORY.md`: remove SurrealDB section
- Update any `specs/` that reference SurrealDB

---

## Risks

1. **sqlite-vec + sqlx FFI compatibility** — sqlite-vec's Rust bindings target rusqlite.
   We need `sqlite3_auto_extension` via sqlx's FFI (`sqlx::sqlite::ffi`). If sqlx
   doesn't re-export this, fallback: use `libsqlite3-sys` directly for the
   auto_extension call.

2. **FTS5 availability** — SQLite must be compiled with FTS5. sqlx's bundled SQLite
   should have it, but verify. If not, the explicit `libsqlite3-sys` dep with `bundled`
   feature ensures it.

3. **FTS5 query syntax** — special chars in user queries (quotes, hyphens, parens) can
   break FTS5 MATCH. Add a sanitizer that wraps terms in double quotes or strips
   operators.

4. **Transaction safety** — multi-table ops (upsert_embedding, delete_note with edges)
   need explicit `pool.begin()` transactions.

---

## Verification

1. `just ci` — must pass (fmt + check + clippy + test)
2. Verify hybrid search returns relevant results (BM25 + vector fusion)
3. Verify graph edges work (create note with [[wiki links]], check edges created, orphan
   detection works)
4. `cargo test --features live-tests` — e2e tests pass (knowledge search, embeddings,
   chat orchestration)
5. Manual smoke test: `ghost daemon` → send messages via Discord → verify session
   persistence, knowledge creation, search results
