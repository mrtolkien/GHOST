CREATE TABLE code_file (
    id          TEXT PRIMARY KEY,
    repo        TEXT NOT NULL,
    path        TEXT NOT NULL,
    content     TEXT NOT NULL,
    file_hash   TEXT,
    created_at  TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now')),
    updated_at  TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now')),
    UNIQUE(repo, path)
);

CREATE VIRTUAL TABLE code_file_fts USING fts5(
    repo, path, content,
    content=code_file, content_rowid=rowid,
    tokenize='porter unicode61'
);

-- Sync triggers (same pattern as script_fts in 008_scripts.sql)
CREATE TRIGGER code_file_fts_ai AFTER INSERT ON code_file BEGIN
    INSERT INTO code_file_fts(rowid, repo, path, content)
    VALUES (new.rowid, new.repo, new.path, new.content);
END;

CREATE TRIGGER code_file_fts_ad AFTER DELETE ON code_file BEGIN
    INSERT INTO code_file_fts(code_file_fts, rowid, repo, path, content)
    VALUES ('delete', old.rowid, old.repo, old.path, old.content);
END;

CREATE TRIGGER code_file_fts_au AFTER UPDATE ON code_file BEGIN
    INSERT INTO code_file_fts(code_file_fts, rowid, repo, path, content)
    VALUES ('delete', old.rowid, old.repo, old.path, old.content);
    INSERT INTO code_file_fts(rowid, repo, path, content)
    VALUES (new.rowid, new.repo, new.path, new.content);
END;
