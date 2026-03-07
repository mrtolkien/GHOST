CREATE TABLE message_source (
    id TEXT PRIMARY KEY NOT NULL,
    message_id TEXT NOT NULL REFERENCES message(id) ON DELETE CASCADE,
    reference_id TEXT REFERENCES reference(id) ON DELETE SET NULL,
    url TEXT NOT NULL,
    title TEXT,
    created_at TEXT NOT NULL
);
CREATE INDEX idx_message_source_message ON message_source(message_id);
CREATE INDEX idx_message_source_reference ON message_source(reference_id);
CREATE INDEX idx_message_source_url ON message_source(url);
