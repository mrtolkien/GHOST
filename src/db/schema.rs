use surrealdb::Surreal;
use surrealdb::engine::local::Db;

use crate::db::error::DatabaseError;

pub const SCHEMA: &str = r#"
DEFINE TABLE session SCHEMAFULL;
DEFINE FIELD created_at ON session TYPE datetime;
DEFINE FIELD updated_at ON session TYPE datetime;
DEFINE FIELD last_activity_at ON session TYPE datetime;
DEFINE FIELD compaction_summary ON session TYPE option<string>;
DEFINE FIELD compaction_cursor_id ON session TYPE option<string>;

DEFINE TABLE message SCHEMAFULL;
DEFINE FIELD session ON message TYPE record<session>;
DEFINE FIELD role ON message TYPE string ASSERT $value IN ["user", "assistant", "system"];
DEFINE FIELD content ON message TYPE string;
DEFINE FIELD tool_calls ON message TYPE option<array>;
DEFINE FIELD tool_results ON message TYPE option<array>;
DEFINE FIELD created_at ON message TYPE datetime;
DEFINE INDEX idx_message_session ON message FIELDS session, created_at;

DEFINE TABLE job_log SCHEMAFULL;
DEFINE FIELD job_name ON job_log TYPE string;
DEFINE FIELD job_kind ON job_log TYPE string;
DEFINE FIELD session ON job_log TYPE option<record<session>>;
DEFINE FIELD started_at ON job_log TYPE datetime;
DEFINE FIELD finished_at ON job_log TYPE option<datetime>;
DEFINE FIELD status ON job_log TYPE string ASSERT $value IN ["running", "ok", "failed"];
DEFINE FIELD transcript ON job_log TYPE option<string>;
DEFINE FIELD handoff_note ON job_log TYPE option<string>;

DEFINE TABLE usage_log SCHEMAFULL;
DEFINE FIELD session ON usage_log TYPE record<session>;
DEFINE FIELD model ON usage_log TYPE string;
DEFINE FIELD provider ON usage_log TYPE string;
DEFINE FIELD input_tokens ON usage_log TYPE int;
DEFINE FIELD output_tokens ON usage_log TYPE int;
DEFINE FIELD cache_read_tokens ON usage_log TYPE option<int>;
DEFINE FIELD cache_creation_tokens ON usage_log TYPE option<int>;
DEFINE FIELD created_at ON usage_log TYPE datetime;

DEFINE TABLE note SCHEMAFULL;
DEFINE FIELD title ON note TYPE string;
DEFINE FIELD body ON note TYPE string;
DEFINE FIELD archetype ON note TYPE option<string>;
DEFINE FIELD tags ON note TYPE array<string>;
DEFINE FIELD trust ON note TYPE int DEFAULT 5;
DEFINE FIELD created_at ON note TYPE datetime;
DEFINE FIELD updated_at ON note TYPE datetime;
DEFINE INDEX idx_note_title ON note FIELDS title UNIQUE;

DEFINE TABLE reference SCHEMAFULL;
DEFINE FIELD topic ON reference TYPE string;
DEFINE FIELD path ON reference TYPE string;
DEFINE FIELD content ON reference TYPE string;
DEFINE FIELD source_url ON reference TYPE option<string>;
DEFINE FIELD created_at ON reference TYPE datetime;
DEFINE INDEX idx_reference_topic ON reference FIELDS topic, path UNIQUE;

DEFINE TABLE diary SCHEMAFULL;
DEFINE FIELD date ON diary TYPE string;
DEFINE FIELD body ON diary TYPE string;
DEFINE FIELD updated_at ON diary TYPE datetime;
DEFINE INDEX idx_diary_date ON diary FIELDS date UNIQUE;

DEFINE TABLE relates_to SCHEMAFULL TYPE RELATION IN note OUT note;
DEFINE FIELD label ON relates_to TYPE string DEFAULT "relates_to";
DEFINE FIELD created_at ON relates_to TYPE datetime;

DEFINE TABLE references SCHEMAFULL TYPE RELATION IN note OUT reference;
DEFINE FIELD created_at ON references TYPE datetime;
"#;

#[tracing::instrument(skip_all)]
pub async fn apply_schema(db: &Surreal<Db>) -> Result<(), DatabaseError> {
    db.query(SCHEMA)
        .await
        .map_err(|source| DatabaseError::ApplySchema { source })?;
    Ok(())
}
