-- Drop old triggers
DROP TRIGGER IF EXISTS reference_fts_ai;
DROP TRIGGER IF EXISTS reference_fts_ad;
DROP TRIGGER IF EXISTS reference_fts_au;

-- Drop and recreate FTS table without topic_name
DROP TABLE IF EXISTS reference_fts;
CREATE VIRTUAL TABLE reference_fts USING fts5(
    content,
    content=reference,
    content_rowid=rowid,
    tokenize='porter unicode61'
);

-- Rebuild triggers (no more topic_name lookup)
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

-- Rebuild the FTS index from existing data
INSERT INTO reference_fts(reference_fts) VALUES ('rebuild');
