-- Ghost initial schema (SQLite + FTS5)

-- Session management
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
CREATE INDEX idx_message_session ON message(session_id, created_at);

-- Interface-to-session mapping
CREATE TABLE interface_session (
    id TEXT PRIMARY KEY NOT NULL,
    interface TEXT NOT NULL UNIQUE,
    session_id TEXT NOT NULL REFERENCES session(id) ON DELETE CASCADE,
    created_at TEXT NOT NULL
);

-- Job execution logging
CREATE TABLE job_log (
    id TEXT PRIMARY KEY NOT NULL,
    job_name TEXT NOT NULL,
    job_kind TEXT NOT NULL,
    session_id TEXT REFERENCES session(id) ON DELETE SET NULL,
    agent_session_id TEXT REFERENCES session(id) ON DELETE SET NULL,
    started_at TEXT NOT NULL,
    finished_at TEXT,
    status TEXT NOT NULL CHECK (status IN ('running', 'ok', 'failed')),
    transcript TEXT,
    handoff_note TEXT,
    todo_list TEXT -- JSON array
);

-- LLM usage tracking
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

-- Knowledge: notes
CREATE TABLE note (
    id TEXT PRIMARY KEY NOT NULL,
    title TEXT NOT NULL UNIQUE,
    body TEXT NOT NULL,
    archetype TEXT,
    tags TEXT NOT NULL DEFAULT '[]',    -- JSON array of strings
    sources TEXT NOT NULL DEFAULT '[]', -- JSON array of strings
    trust INTEGER NOT NULL DEFAULT 5,
    path TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

-- Knowledge: references
CREATE TABLE reference (
    id TEXT PRIMARY KEY NOT NULL,
    topic TEXT NOT NULL,
    path TEXT NOT NULL,
    content TEXT NOT NULL,
    source_url TEXT,
    created_at TEXT NOT NULL,
    UNIQUE(topic, path)
);

-- Knowledge: diary
CREATE TABLE diary (
    id TEXT PRIMARY KEY NOT NULL,
    date TEXT NOT NULL UNIQUE,
    body TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

-- Graph edges: note-to-note relations
CREATE TABLE relates_to (
    id TEXT PRIMARY KEY NOT NULL,
    from_id TEXT NOT NULL REFERENCES note(id) ON DELETE CASCADE,
    to_id TEXT NOT NULL REFERENCES note(id) ON DELETE CASCADE,
    label TEXT NOT NULL DEFAULT 'relates_to',
    created_at TEXT NOT NULL
);

-- Graph edges: note-to-reference citations
CREATE TABLE cited (
    id TEXT PRIMARY KEY NOT NULL,
    from_id TEXT NOT NULL REFERENCES note(id) ON DELETE CASCADE,
    to_id TEXT NOT NULL REFERENCES reference(id) ON DELETE CASCADE,
    created_at TEXT NOT NULL
);

-- Embedding metadata (vectors stored in vec0 table created at runtime)
CREATE TABLE embedding (
    id TEXT PRIMARY KEY NOT NULL,
    source_table TEXT NOT NULL,
    source_id TEXT NOT NULL,
    chunk_index INTEGER NOT NULL,
    chunk_text TEXT NOT NULL,
    content_hash TEXT NOT NULL,
    created_at TEXT NOT NULL,
    UNIQUE(source_id, chunk_index)
);

-- FTS5 full-text search (external content mode with sync triggers)

CREATE VIRTUAL TABLE note_fts USING fts5(
    title,
    body,
    content=note,
    content_rowid=rowid,
    tokenize='porter unicode61'
);

CREATE TRIGGER note_fts_ai AFTER INSERT ON note BEGIN
    INSERT INTO note_fts(rowid, title, body)
    VALUES (new.rowid, new.title, new.body);
END;
CREATE TRIGGER note_fts_ad AFTER DELETE ON note BEGIN
    INSERT INTO note_fts(note_fts, rowid, title, body)
    VALUES ('delete', old.rowid, old.title, old.body);
END;
CREATE TRIGGER note_fts_au AFTER UPDATE ON note BEGIN
    INSERT INTO note_fts(note_fts, rowid, title, body)
    VALUES ('delete', old.rowid, old.title, old.body);
    INSERT INTO note_fts(rowid, title, body)
    VALUES (new.rowid, new.title, new.body);
END;

CREATE VIRTUAL TABLE reference_fts USING fts5(
    content,
    content=reference,
    content_rowid=rowid,
    tokenize='porter unicode61'
);

CREATE TRIGGER reference_fts_ai AFTER INSERT ON reference BEGIN
    INSERT INTO reference_fts(rowid, content)
    VALUES (new.rowid, new.content);
END;
CREATE TRIGGER reference_fts_ad AFTER DELETE ON reference BEGIN
    INSERT INTO reference_fts(reference_fts, rowid, content)
    VALUES ('delete', old.rowid, old.content);
END;
CREATE TRIGGER reference_fts_au AFTER UPDATE ON reference BEGIN
    INSERT INTO reference_fts(reference_fts, rowid, content)
    VALUES ('delete', old.rowid, old.content);
    INSERT INTO reference_fts(rowid, content)
    VALUES (new.rowid, new.content);
END;

CREATE VIRTUAL TABLE diary_fts USING fts5(
    body,
    content=diary,
    content_rowid=rowid,
    tokenize='porter unicode61'
);

CREATE TRIGGER diary_fts_ai AFTER INSERT ON diary BEGIN
    INSERT INTO diary_fts(rowid, body)
    VALUES (new.rowid, new.body);
END;
CREATE TRIGGER diary_fts_ad AFTER DELETE ON diary BEGIN
    INSERT INTO diary_fts(diary_fts, rowid, body)
    VALUES ('delete', old.rowid, old.body);
END;
CREATE TRIGGER diary_fts_au AFTER UPDATE ON diary BEGIN
    INSERT INTO diary_fts(diary_fts, rowid, body)
    VALUES ('delete', old.rowid, old.body);
    INSERT INTO diary_fts(rowid, body)
    VALUES (new.rowid, new.body);
END;
