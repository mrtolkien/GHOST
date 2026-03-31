-- Add 'book' to import_batch.source_type CHECK constraint.
-- SQLite requires table recreation to modify CHECK constraints.

CREATE TABLE import_batch_new (
    id TEXT PRIMARY KEY NOT NULL,
    topic_id TEXT NOT NULL REFERENCES topic(id) ON DELETE CASCADE,
    source_type TEXT NOT NULL CHECK (source_type IN ('git', 'page', 'crawl', 'file', 'book')),
    source_url TEXT NOT NULL,
    version_ref TEXT,
    ref_count INTEGER NOT NULL DEFAULT 0,
    import_config TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    UNIQUE(topic_id)
);

INSERT INTO import_batch_new SELECT * FROM import_batch;
DROP TABLE import_batch;
ALTER TABLE import_batch_new RENAME TO import_batch;
