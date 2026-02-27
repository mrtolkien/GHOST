# Compile-Time Checked Queries with sqlx Macros

## Context

After the SurrealDB → SQLite migration, all ~80 database queries use runtime
`sqlx::query()` / `sqlx::query_as()`. These only validate SQL at runtime — a
typo in a column name or a schema/struct mismatch compiles fine and explodes at
runtime. sqlx provides `query!()` / `query_as!()` / `query_scalar!()` macros
that validate SQL against a real database at compile time and infer return types.

**Goal**: Convert all eligible queries to compile-time checked macros, powered by
a build.rs that creates a fresh SQLite database from the migration file on every
build. No `.sqlx/` offline cache — always check against a live schema.

Additionally: hardcode embedding dimension to 1024 and simplify `connect()`.

## Phase 0 — Hardcode Embedding Dimension

The embedding dimension is currently configurable via `config.embeddings.dimension`
and passed to `connect(workspace, embedding_dim)`. In practice it's always 1024
(the default, matching `qwen3-embedding:8b`). Changing models would require a new
migration anyway (can't mix dimensions in the same vec0 table).

1. **`src/db/connection.rs`** — Remove `embedding_dim` parameter. Signature
   becomes `connect(workspace: &Path)`. Hardcode `1024` in the `CREATE VIRTUAL
   TABLE` statement. Keep the vec0 creation as runtime code (not in the
   migration) since build.rs won't have sqlite-vec loaded.

2. **`src/config.rs`** — Remove `dimension` from `EmbeddingsConfig` and
   `EmbeddingsSettings`. It's no longer needed anywhere.

3. **All `connect()` call sites** (~6) — Drop the second argument:
   - `src/daemon/run.rs`
   - `src/cli/session.rs`
   - `src/cli/knowledge.rs`
   - `src/cli/job.rs` (2 call sites)
   - `tests/common.rs` (2: `test_database` and `live_test_database`)

## Phase 1 — build.rs

Create `build.rs` in the project root. It will:

1. Read `migrations/001_initial.sql`
2. Create a temp SQLite file in `$OUT_DIR/ghost-build.db`
3. Execute the migration SQL using raw `libsqlite3-sys` FFI (already a dependency)
4. Emit `cargo:rustc-env=DATABASE_URL=sqlite://{path}` for the macros
5. Emit `cargo:rerun-if-changed=migrations/` so schema changes trigger rebuild

FTS5 virtual tables in the migration will work because FTS5 is compiled into the
bundled SQLite. The vec0 table will NOT exist in the build DB (no sqlite-vec
extension at build time) — queries touching `vec_embedding` stay as runtime
queries.

**`Cargo.toml` changes:**

```toml
[build-dependencies]
libsqlite3-sys = { version = "0.30", features = ["bundled"] }
```

Also add `"macros"` to sqlx features:
```toml
sqlx = { version = "0.8", features = ["runtime-tokio", "sqlite", "chrono", "macros"] }
```

## Phase 2 — Convert Regular-Table Queries

~69 queries across 9 files. The conversion is mechanical — three patterns:

### Pattern A: Execute-only (INSERT / UPDATE / DELETE)

```rust
// Before
sqlx::query("INSERT INTO session (id, created_at) VALUES (?, ?)")
    .bind(&id).bind(&ts).execute(db).await?;

// After
sqlx::query!("INSERT INTO session (id, created_at) VALUES (?, ?)", id, ts)
    .execute(db).await?;
```

Bind args move from `.bind()` chains to comma-separated macro arguments.

### Pattern B: Fetch returning named structs

```rust
// Before
sqlx::query_as::<_, SessionRecord>("SELECT * FROM session WHERE id = ?")
    .bind(session_id).fetch_optional(db).await?;

// After
sqlx::query_as!(SessionRecord, "SELECT * FROM session WHERE id = ?", session_id)
    .fetch_optional(db).await?;
```

### Pattern C: Scalar values (COUNT, single column)

```rust
// Before
let (count,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM message WHERE session_id = ?")
    .bind(session_id).fetch_one(db).await?;

// After
let count = sqlx::query_scalar!("SELECT COUNT(*) FROM message WHERE session_id = ?", session_id)
    .fetch_one(db).await?;
```

`query_scalar!()` returns the value directly. Note: sqlx may infer `COUNT(*)`
as `Option<i32>` on SQLite — use a type override if needed:
`"SELECT COUNT(*) as \"count!: i64\" FROM ..."`.

### File-by-file breakdown

| File | Queries | Convert | Stay runtime | Notes |
|------|---------|---------|--------------|-------|
| `db/sessions.rs` | 15 | 15 | 0 | All regular tables |
| `db/knowledge/crud.rs` | 27 | 27 | 0 | All regular tables |
| `db/knowledge/graph.rs` | 8 | 8 | 0 | All regular tables |
| `db/knowledge/stats.rs` | 7 | 7 | 0 | Inline `count_table()` per-table |
| `db/job_logs.rs` | 6 | 6 | 0 | All regular tables |
| `db/interface_sessions.rs` | 4 | 4 | 0 | All regular tables |
| `db/embeddings.rs` | 13 | 7 | 6 | vec_embedding stays runtime |
| `db/knowledge/search.rs` | 3 | 0 | 3 | FTS5 MATCH stays runtime |
| `db/connection.rs` | 1 | 0 | 1 | Dynamic vec0 creation |

**Total: ~69 convert, ~10 stay runtime.**

### stats.rs: eliminate `count_table()` helper

Replace the dynamic `format!("SELECT COUNT(*) FROM {table}")` helper with
per-table `query_scalar!()` calls. Delete the `count_table()` function:

```rust
pub async fn count_notes(db: &SqlitePool) -> Result<i64, DatabaseError> {
    sqlx::query_scalar!("SELECT COUNT(*) FROM note")
        .fetch_one(db).await.map_err(...)
}
```

Same for `count_references`, `count_diary`, `count_edges`.

### Inline structs → `query_scalar!()`

Single-field local structs (`RowidRow`, `HashRow`, `TodoRow`, `InterfaceRow`,
`SessionRow`, `OutRow`, `InRow`) can be replaced with `query_scalar!()`,
eliminating ~8 struct definitions.

## Phase 3 — Runtime Queries (No Change)

These stay as `sqlx::query()` / `sqlx::query_as::<_, T>()`:

**FTS5 queries** (`db/knowledge/search.rs`):
- `search_notes` — `note_fts MATCH` + `bm25()`
- `search_references` — `reference_fts MATCH` + `bm25()`
- `search_diary` — `diary_fts MATCH` + `bm25()`

**vec0 queries** (`db/embeddings.rs`):
- `DELETE FROM vec_embedding WHERE rowid = ?`
- `INSERT INTO vec_embedding(rowid, embedding) VALUES (?, ?)`
- `DELETE FROM vec_embedding WHERE rowid IN (...)`
- `DELETE FROM vec_embedding`
- `SELECT ... FROM vec_embedding v JOIN embedding e ...`

**Dynamic DDL** (`db/connection.rs`):
- `CREATE VIRTUAL TABLE IF NOT EXISTS vec_embedding USING vec0(...)`

Add a comment at the top of `src/db/mod.rs` explaining the split:
```rust
//! Regular-table queries use compile-time checked `query!()` macros
//! (validated against migrations/ via build.rs). FTS5 and vec0
//! virtual-table queries use runtime `query_as::<_, T>()` since sqlx
//! cannot introspect virtual table column types at build time.
```

## Phase 4 — Cleanup

1. Remove `#[derive(sqlx::FromRow)]` from inline structs replaced by
   `query_scalar!()`. Keep it on public record structs (`SessionRecord`,
   `MessageRecord`, `NoteRecord`, etc.) — they're used by both macro and
   runtime queries.

2. Run `just ci` — the macros will catch any SQL/struct mismatches at compile
   time now.

## Verification

1. `just ci` passes (compile-time query validation + all tests)
2. Introduce a deliberate typo in a `query!()` SQL string → verify it fails
   at compile time
3. Add a column to a table in the migration without updating the struct →
   verify `SELECT *` macro fails at compile time
4. Run `chat_orchestration_live` to validate end-to-end

## Risks

- **Build time**: Each `query!()` invocation runs EXPLAIN against the build DB.
  ~69 macros may add a few seconds. Monitor and consider `query_file!()` if it
  gets bad.
- **`SELECT *` fragility**: Adding a column to a table will break all
  `query_as!(Struct, "SELECT * ...")` macros for that table until the struct is
  updated. This is intentional — it catches schema drift.
- **Migration changes**: Adding `migrations/002_*.sql` requires updating
  build.rs to also apply it (or switch to glob-based application).
