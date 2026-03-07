-- Auto-generated from migrations/ — do not edit manually.
-- Regenerate with: just generate-schema

CREATE TABLE agent_run (
    id TEXT PRIMARY KEY NOT NULL,
    agent_name TEXT NOT NULL,
    run_kind TEXT NOT NULL,
    session_id TEXT REFERENCES session(id) ON DELETE SET NULL,
    agent_session_id TEXT REFERENCES session(id) ON DELETE SET NULL,
    started_at TEXT NOT NULL,
    finished_at TEXT,
    status TEXT NOT NULL CHECK (status IN ('running', 'ok', 'failed')),
    transcript TEXT,
    handoff_note TEXT,
    todo_list TEXT -- JSON array
);
CREATE TABLE agent_state (
    agent_slug TEXT NOT NULL,
    key        TEXT NOT NULL,
    value      TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    PRIMARY KEY (agent_slug, key)
);
CREATE TABLE cited (
    id TEXT PRIMARY KEY NOT NULL,
    from_id TEXT NOT NULL REFERENCES note(id) ON DELETE CASCADE,
    to_id TEXT NOT NULL REFERENCES reference(id) ON DELETE CASCADE,
    created_at TEXT NOT NULL
);
CREATE TABLE coding_sessions (
    id          TEXT PRIMARY KEY,
    session_id  TEXT NOT NULL REFERENCES session(id) ON DELETE CASCADE,
    channel_id  TEXT,
    working_dir TEXT NOT NULL,
    status      TEXT NOT NULL DEFAULT 'active',
    started_at  TEXT NOT NULL,
    ended_at    TEXT
);
CREATE TABLE diary (
    id TEXT PRIMARY KEY NOT NULL,
    date TEXT NOT NULL UNIQUE,
    body TEXT NOT NULL,
    updated_at TEXT NOT NULL
);
CREATE TABLE embedding (
    id TEXT PRIMARY KEY NOT NULL,
    source_table TEXT NOT NULL,
    source_id TEXT NOT NULL,
    chunk_index INTEGER NOT NULL,
    chunk_text TEXT NOT NULL,
    content_hash TEXT NOT NULL,
    topic_id TEXT,
    created_at TEXT NOT NULL,
    UNIQUE(source_id, chunk_index)
);
CREATE TABLE import_batch (
    id TEXT PRIMARY KEY NOT NULL,
    topic_id TEXT NOT NULL REFERENCES topic(id) ON DELETE CASCADE,
    source_type TEXT NOT NULL CHECK (source_type IN ('git', 'page', 'crawl', 'file')),
    source_url TEXT NOT NULL,
    version_ref TEXT,
    ref_count INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    UNIQUE(topic_id)
);
CREATE TABLE interface_session (
    id TEXT PRIMARY KEY NOT NULL,
    interface TEXT NOT NULL UNIQUE,
    session_id TEXT NOT NULL REFERENCES session(id) ON DELETE CASCADE,
    created_at TEXT NOT NULL
);
CREATE TABLE message (
    id TEXT PRIMARY KEY NOT NULL,
    session_id TEXT NOT NULL REFERENCES session(id) ON DELETE CASCADE,
    role TEXT NOT NULL CHECK (role IN ('user', 'assistant', 'system')),
    content TEXT NOT NULL,
    tool_calls TEXT,   -- JSON array
    tool_results TEXT, -- JSON array
    raw_output TEXT,   -- JSON array
    created_at TEXT NOT NULL
);
CREATE TABLE message_source (
    id TEXT PRIMARY KEY NOT NULL,
    message_id TEXT NOT NULL REFERENCES message(id) ON DELETE CASCADE,
    reference_id TEXT REFERENCES reference(id) ON DELETE SET NULL,
    url TEXT NOT NULL,
    title TEXT,
    created_at TEXT NOT NULL
);
CREATE TABLE note (
    id TEXT PRIMARY KEY NOT NULL,
    title TEXT NOT NULL UNIQUE,
    body TEXT NOT NULL,
    tags TEXT NOT NULL DEFAULT '[]',    -- JSON array of strings
    sources TEXT NOT NULL DEFAULT '[]', -- JSON array of strings
    trust INTEGER NOT NULL DEFAULT 5,
    topic_id TEXT REFERENCES topic(id) ON DELETE SET NULL,
    path TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);
CREATE TABLE reference (
    id TEXT PRIMARY KEY NOT NULL,
    topic_id TEXT NOT NULL REFERENCES topic(id),
    path TEXT NOT NULL,
    content TEXT NOT NULL,
    source_url TEXT,
    import_batch_id TEXT REFERENCES import_batch(id) ON DELETE SET NULL,
    created_at TEXT NOT NULL,
    UNIQUE(topic_id, path)
);
CREATE TABLE relates_to (
    id TEXT PRIMARY KEY NOT NULL,
    from_id TEXT NOT NULL REFERENCES note(id) ON DELETE CASCADE,
    to_id TEXT NOT NULL REFERENCES note(id) ON DELETE CASCADE,
    label TEXT NOT NULL DEFAULT 'relates_to',
    created_at TEXT NOT NULL
);
CREATE TABLE session (
    id TEXT PRIMARY KEY NOT NULL,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    last_activity_at TEXT NOT NULL,
    status TEXT NOT NULL CHECK (status IN ('active', 'rebooted', 'agent')),
    compaction_summary TEXT,
    compaction_cursor_id TEXT,
    todo_list TEXT -- JSON array
);
CREATE TABLE topic (
    id TEXT PRIMARY KEY NOT NULL,
    name TEXT NOT NULL UNIQUE,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);
CREATE TABLE usage_log (
    id TEXT PRIMARY KEY NOT NULL,
    session_id TEXT NOT NULL REFERENCES session(id) ON DELETE CASCADE,
    model TEXT NOT NULL,
    provider TEXT NOT NULL,
    input_tokens INTEGER NOT NULL,
    output_tokens INTEGER NOT NULL,
    cache_read_tokens INTEGER,
    cache_creation_tokens INTEGER,
    created_at TEXT NOT NULL
);
CREATE INDEX idx_coding_sessions_channel ON coding_sessions(channel_id)
    WHERE status = 'active';
CREATE INDEX idx_coding_sessions_status ON coding_sessions(status);
CREATE INDEX idx_message_session ON message(session_id, created_at);
CREATE INDEX idx_message_source_message ON message_source(message_id);
CREATE INDEX idx_message_source_reference ON message_source(reference_id);
CREATE INDEX idx_message_source_url ON message_source(url);
