-- Add 'youtube' to import_batch.source_type CHECK constraint.
-- SQLite requires table recreation to modify CHECK constraints.

PRAGMA foreign_keys=OFF;

CREATE TABLE import_batch_new (
    id TEXT PRIMARY KEY NOT NULL,
    topic_id TEXT NOT NULL REFERENCES topic(id) ON DELETE CASCADE,
    source_type TEXT NOT NULL CHECK (source_type IN ('git', 'page', 'crawl', 'file', 'book', 'youtube')),
    source_url TEXT NOT NULL,
    version_ref TEXT,
    ref_count INTEGER NOT NULL DEFAULT 0,
    import_config TEXT,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    UNIQUE(topic_id)
);

INSERT INTO import_batch_new (id, topic_id, source_type, source_url, version_ref, ref_count, import_config, created_at, updated_at)
SELECT id, topic_id, source_type, source_url, version_ref, ref_count, import_config, created_at, updated_at
FROM import_batch;

DROP TABLE import_batch;
ALTER TABLE import_batch_new RENAME TO import_batch;

CREATE INDEX idx_import_batch_topic_id ON import_batch(topic_id);

PRAGMA foreign_key_check;
PRAGMA foreign_keys=ON;
