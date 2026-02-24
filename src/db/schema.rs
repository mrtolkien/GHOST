use surrealdb::Surreal;
use surrealdb::engine::local::Db;

use crate::db::error::DatabaseError;

pub const SCHEMA: &str = r#"
DEFINE TABLE session SCHEMAFULL;
DEFINE FIELD created_at ON session TYPE datetime;
DEFINE FIELD updated_at ON session TYPE datetime;
DEFINE FIELD last_activity_at ON session TYPE datetime;
DEFINE FIELD status ON session TYPE string ASSERT $value IN ["active", "rebooted", "agent"];
DEFINE FIELD compaction_summary ON session TYPE option<string>;
DEFINE FIELD compaction_cursor_id ON session TYPE option<string>;
DEFINE FIELD todo_list ON session TYPE option<array>;
DEFINE FIELD todo_list.* ON session FLEXIBLE TYPE object;

DEFINE TABLE message SCHEMAFULL;
DEFINE FIELD session ON message TYPE record<session>;
DEFINE FIELD role ON message TYPE string ASSERT $value IN ["user", "assistant", "system"];
DEFINE FIELD content ON message TYPE string;
DEFINE FIELD tool_calls ON message TYPE option<array>;
DEFINE FIELD tool_calls.* ON message FLEXIBLE TYPE object;
DEFINE FIELD tool_results ON message TYPE option<array>;
DEFINE FIELD tool_results.* ON message FLEXIBLE TYPE object;
DEFINE FIELD raw_output ON message TYPE option<array>;
DEFINE FIELD raw_output.* ON message FLEXIBLE TYPE object;
DEFINE FIELD created_at ON message TYPE datetime;
DEFINE INDEX idx_message_session ON message FIELDS session, created_at;

DEFINE TABLE job_log SCHEMAFULL;
DEFINE FIELD job_name ON job_log TYPE string;
DEFINE FIELD job_kind ON job_log TYPE string;
DEFINE FIELD session ON job_log TYPE option<record<session>>;
DEFINE FIELD agent_session ON job_log TYPE option<record<session>>;
DEFINE FIELD started_at ON job_log TYPE datetime;
DEFINE FIELD finished_at ON job_log TYPE option<datetime>;
DEFINE FIELD status ON job_log TYPE string ASSERT $value IN ["running", "ok", "failed"];
DEFINE FIELD transcript ON job_log TYPE option<string>;
DEFINE FIELD handoff_note ON job_log TYPE option<string>;
DEFINE FIELD todo_list ON job_log TYPE option<array>;
DEFINE FIELD todo_list.* ON job_log FLEXIBLE TYPE object;

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
DEFINE FIELD sources ON note TYPE array<string> DEFAULT [];
DEFINE FIELD trust ON note TYPE int DEFAULT 5;
DEFINE FIELD path ON note TYPE option<string>;
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

DEFINE TABLE interface_session SCHEMAFULL;
-- TEMPORARY SCAFFOLDING for spec 06 reboot/session wiring.
-- Expected to be refined in spec 09 Discord interface implementation.
DEFINE FIELD interface ON interface_session TYPE string;
DEFINE FIELD session ON interface_session TYPE record<session>;
DEFINE FIELD created_at ON interface_session TYPE datetime;
DEFINE INDEX idx_interface ON interface_session FIELDS interface UNIQUE;

DEFINE TABLE cited SCHEMAFULL TYPE RELATION IN note OUT reference;
DEFINE FIELD created_at ON cited TYPE datetime;

DEFINE TABLE embedding SCHEMAFULL;
DEFINE FIELD source_table ON embedding TYPE string;
DEFINE FIELD source_id ON embedding TYPE record;
DEFINE FIELD chunk_index ON embedding TYPE int;
DEFINE FIELD chunk_text ON embedding TYPE string;
DEFINE FIELD content_hash ON embedding TYPE string;
DEFINE FIELD vector ON embedding TYPE array<float>;
DEFINE FIELD created_at ON embedding TYPE datetime;
DEFINE INDEX idx_embedding_source ON embedding FIELDS source_id, chunk_index UNIQUE;
DEFINE INDEX idx_embedding_vector ON embedding FIELDS vector
    MTREE DIMENSION 1024 DIST COSINE;

DEFINE ANALYZER note_analyzer TOKENIZERS blank, class FILTERS lowercase, snowball(english);
DEFINE INDEX idx_note_title_fts ON note FIELDS title SEARCH ANALYZER note_analyzer BM25;
DEFINE INDEX idx_note_body_fts ON note FIELDS body SEARCH ANALYZER note_analyzer BM25;
DEFINE INDEX idx_reference_fts ON reference FIELDS content SEARCH ANALYZER note_analyzer BM25;
DEFINE INDEX idx_diary_fts ON diary FIELDS body SEARCH ANALYZER note_analyzer BM25;
"#;

#[tracing::instrument(skip_all)]
pub async fn apply_schema(db: &Surreal<Db>) -> Result<(), DatabaseError> {
    db.query(SCHEMA)
        .await
        .map_err(|source| DatabaseError::ApplySchema { source })?;
    Ok(())
}
