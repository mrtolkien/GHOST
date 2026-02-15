# 04 — SurrealDB Embedded Setup

## Overview

GHOST uses SurrealDB in embedded mode (surrealkv backend) for all structured data. This
keeps the local-first spirit — no external database process needed.

SurrealDB was chosen over SQLite for its native graph capabilities, which enable typed
edges in the knowledge system (e.g., `[[written_in>Rust]]` becomes a `written_in` edge
between two records).

## Connection

```rust
use surrealdb::Surreal;
use surrealdb::engine::local::SurrealKv;

pub async fn connect(data_dir: &Path) -> Result<Surreal<Db>> {
    let db_path = data_dir.join("ghost.db");
    let db = Surreal::new::<SurrealKv>(db_path).await?;
    db.use_ns("ghost").use_db("main").await?;
    apply_schema(&db).await?;
    Ok(db)
}
```

The database file lives at `$WORKSPACE/ghost.db` (inside the workspace directory).

## Schema

SurrealDB uses SurrealQL for schema definitions. Define tables and fields explicitly for
type safety.

### Core Tables

```surql
-- Sessions
DEFINE TABLE session SCHEMAFULL;
DEFINE FIELD created_at ON session TYPE datetime;
DEFINE FIELD updated_at ON session TYPE datetime;
DEFINE FIELD last_activity_at ON session TYPE datetime;
DEFINE FIELD compaction_summary ON session TYPE option<string>;
DEFINE FIELD compaction_cursor_id ON session TYPE option<string>;

-- Messages (within sessions)
DEFINE TABLE message SCHEMAFULL;
DEFINE FIELD session ON message TYPE record<session>;
DEFINE FIELD role ON message TYPE string ASSERT $value IN ["user", "assistant", "system"];
DEFINE FIELD content ON message TYPE string;
DEFINE FIELD tool_calls ON message TYPE option<array>;
DEFINE FIELD tool_results ON message TYPE option<array>;
DEFINE FIELD created_at ON message TYPE datetime;
DEFINE INDEX idx_message_session ON message FIELDS session, created_at;

-- Job logs
DEFINE TABLE job_log SCHEMAFULL;
DEFINE FIELD job_name ON job_log TYPE string;
DEFINE FIELD job_kind ON job_log TYPE string;
DEFINE FIELD session ON job_log TYPE option<record<session>>;
DEFINE FIELD started_at ON job_log TYPE datetime;
DEFINE FIELD finished_at ON job_log TYPE option<datetime>;
DEFINE FIELD status ON job_log TYPE string ASSERT $value IN ["running", "ok", "failed"];
DEFINE FIELD transcript ON job_log TYPE option<string>;
DEFINE FIELD handoff_note ON job_log TYPE option<string>;

-- Usage log (token tracking)
DEFINE TABLE usage_log SCHEMAFULL;
DEFINE FIELD session ON usage_log TYPE record<session>;
DEFINE FIELD model ON usage_log TYPE string;
DEFINE FIELD provider ON usage_log TYPE string;
DEFINE FIELD input_tokens ON usage_log TYPE int;
DEFINE FIELD output_tokens ON usage_log TYPE int;
DEFINE FIELD cache_read_tokens ON usage_log TYPE option<int>;
DEFINE FIELD cache_creation_tokens ON usage_log TYPE option<int>;
DEFINE FIELD created_at ON usage_log TYPE datetime;
```

### Knowledge Tables (see also 13-knowledge-system.md)

```surql
-- Notes (atomic knowledge units)
DEFINE TABLE note SCHEMAFULL;
DEFINE FIELD title ON note TYPE string;
DEFINE FIELD body ON note TYPE string;
DEFINE FIELD archetype ON note TYPE option<string>;
DEFINE FIELD tags ON note TYPE array<string>;
DEFINE FIELD trust ON note TYPE int DEFAULT 5;
DEFINE FIELD created_at ON note TYPE datetime;
DEFINE FIELD updated_at ON note TYPE datetime;
DEFINE INDEX idx_note_title ON note FIELDS title UNIQUE;

-- References (preserved source material)
DEFINE TABLE reference SCHEMAFULL;
DEFINE FIELD topic ON reference TYPE string;
DEFINE FIELD path ON reference TYPE string;
DEFINE FIELD content ON reference TYPE string;
DEFINE FIELD source_url ON reference TYPE option<string>;
DEFINE FIELD created_at ON reference TYPE datetime;
DEFINE INDEX idx_reference_topic ON reference FIELDS topic, path UNIQUE;

-- Diary entries
DEFINE TABLE diary SCHEMAFULL;
DEFINE FIELD date ON diary TYPE string;  -- YYYY-MM-DD
DEFINE FIELD body ON diary TYPE string;
DEFINE FIELD updated_at ON diary TYPE datetime;
DEFINE INDEX idx_diary_date ON diary FIELDS date UNIQUE;

-- Typed graph edges between notes (the key SurrealDB differentiator)
-- Created from wiki links like [[written_in>Rust]]
DEFINE TABLE relates_to SCHEMAFULL TYPE RELATION IN note OUT note;
DEFINE FIELD label ON relates_to TYPE string DEFAULT "relates_to";
DEFINE FIELD created_at ON relates_to TYPE datetime;

-- Note-to-reference edges
DEFINE TABLE references SCHEMAFULL TYPE RELATION IN note OUT reference;
DEFINE FIELD created_at ON references TYPE datetime;
```

## Migration Strategy

SurrealDB doesn't have a built-in migration framework like sqlx. Options:

1. **Schema-on-connect**: Apply the full schema on every connect. SurrealDB's `DEFINE`
   statements are idempotent — re-defining an existing table/field is a no-op if the
   definition matches.
2. **Version tracking**: Store a `schema_version` record and apply incremental changes.

For the PoC, use approach 1 (schema-on-connect). It's simple and SurrealDB handles
idempotent definitions well. Add version-tracked migrations later if needed.

## Repository Pattern

Thin query functions grouped by domain:

```rust
// src/db/sessions.rs
pub async fn create_session(db: &Surreal<Db>) -> Result<Thing> { ... }
pub async fn get_session(db: &Surreal<Db>, id: &Thing) -> Result<Session> { ... }
pub async fn update_activity(db: &Surreal<Db>, id: &Thing) -> Result<()> { ... }

// src/db/knowledge.rs
pub async fn upsert_note(db: &Surreal<Db>, note: &Note) -> Result<Thing> { ... }
pub async fn create_edge(db: &Surreal<Db>, from: &Thing, to: &Thing, label: &str) -> Result<()> { ... }
pub async fn search_notes(db: &Surreal<Db>, query: &str) -> Result<Vec<Note>> { ... }
```

## Acceptance Criteria

- SurrealDB connects in embedded mode on daemon start
- Database file is created at `$WORKSPACE/ghost.db`
- Schema is applied idempotently on connect
- Basic CRUD operations work for sessions, messages, and notes
- Graph edges can be created and queried (e.g., find all notes linked to a given note)
- Database operations produce tracing spans
- Errors include context (table name, operation type)
